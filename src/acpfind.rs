//! Finds Agent Client Protocol agents installed on this Mac, so Settings › Agent can offer them by name.
//! Launch commands come from the official ACP registry (cdn.agentclientprotocol.com/registry/v1/latest/registry.json,
//! read 2026-11-02) and, for Hermes, its docs (hermes-agent.nousresearch.com/docs/user-guide/features/acp).
use std::path::PathBuf;
use std::sync::OnceLock;

/// (name, program to look for, arguments). A `None` program row means "npx package, if this CLI is installed".
struct Known {
    name: &'static str,
    bin: &'static str,
    args: &'static str,
    /// Agents shipped as an npm ACP adapter: used through npx when `bin` (the agent's own CLI) is installed.
    npx: Option<&'static str>,
}

const KNOWN: &[Known] = &[
    Known { name: "Claude Code", bin: "claude-agent-acp", args: "", npx: None },
    Known { name: "Claude Code", bin: "claude", args: "", npx: Some("@agentclientprotocol/claude-agent-acp") },
    Known { name: "Codex", bin: "codex-acp", args: "", npx: None },
    Known { name: "Codex", bin: "codex", args: "", npx: Some("@agentclientprotocol/codex-acp") },
    Known { name: "Gemini CLI", bin: "gemini", args: "--acp", npx: None },
    Known { name: "Hermes", bin: "hermes", args: "acp", npx: None },
    Known { name: "GitHub Copilot", bin: "copilot", args: "--acp", npx: None },
    Known { name: "goose", bin: "goose", args: "acp", npx: None },
    Known { name: "OpenCode", bin: "opencode", args: "acp", npx: None },
    Known { name: "Qwen Code", bin: "qwen", args: "--acp", npx: None },
    Known { name: "Cursor", bin: "cursor-agent", args: "acp", npx: None },
    Known { name: "Kiro CLI", bin: "kiro-cli", args: "acp --agent-engine=v3", npx: None },
    Known { name: "Kilo", bin: "kilo", args: "acp", npx: None },
    Known { name: "Cline", bin: "cline", args: "--acp", npx: None },
    Known { name: "Auggie", bin: "auggie", args: "--acp", npx: None },
    Known { name: "Factory Droid", bin: "droid", args: "exec --output-format acp-daemon", npx: None },
    Known { name: "Devin", bin: "devin", args: "acp", npx: None },
    Known { name: "Kimi CLI", bin: "kimi", args: "acp", npx: None },
    Known { name: "Amp", bin: "amp-acp", args: "", npx: None },
    Known { name: "Mistral Vibe", bin: "vibe-acp", args: "", npx: None },
    Known { name: "VT Code", bin: "vtcode", args: "acp", npx: None },
    Known { name: "Stakpak", bin: "stakpak", args: "acp", npx: None },
];

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub name: &'static str,
    pub command: String,
}

/// The login shell's PATH (apps opened from Finder get a minimal one), plus the usual install folders.
pub fn login_path() -> &'static str {
    static P: OnceLock<String> = OnceLock::new();
    P.get_or_init(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let from_shell = std::process::Command::new(&shell)
            .args(["-ilc", "printf '\\n__PATH__%s\\n' \"$PATH\""])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find_map(|l| l.strip_prefix("__PATH__").map(String::from)))
            .unwrap_or_default();
        let mut dirs: Vec<String> = vec![];
        let extra = [format!("{home}/.local/bin"), "/opt/homebrew/bin".into(), "/usr/local/bin".into(), format!("{home}/.bun/bin"), format!("{home}/.npm-global/bin"), format!("{home}/.cargo/bin"), format!("{home}/.volta/bin")];
        for d in from_shell.split(':').map(String::from).chain(std::env::var("PATH").unwrap_or_default().split(':').map(String::from)).chain(extra) {
            if !d.is_empty() && !dirs.contains(&d) {
                dirs.push(d);
            }
        }
        dirs.join(":")
    })
}

fn which_in(path: &str, bin: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    path.split(':').map(|d| PathBuf::from(d).join(bin)).find(|p| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

fn quote(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    if s.contains(' ') { format!("'{s}'") } else { s }
}

/// Agents found on `path`, one per name (a native ACP binary wins over the npx adapter).
pub fn find_in(path: &str) -> Vec<Found> {
    let mut out: Vec<Found> = vec![];
    for k in KNOWN {
        if out.iter().any(|f| f.name == k.name) {
            continue;
        }
        let Some(bin) = which_in(path, k.bin) else { continue };
        let command = match k.npx {
            Some(pkg) => match which_in(path, "npx") {
                Some(npx) => format!("{} -y {pkg}", quote(&npx)),
                None => continue,
            },
            None => format!("{} {}", quote(&bin), k.args).trim().to_string(),
        };
        out.push(Found { name: k.name, command });
    }
    out
}

pub fn find() -> Vec<Found> {
    find_in(login_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn finds_installed_agents() {
        let d = std::env::temp_dir().join(format!("gtdcraft-acp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for b in ["gemini", "claude", "npx", "goose", "notes.txt"] {
            let p = d.join(b);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            if b != "notes.txt" {
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let f = find_in(d.to_str().unwrap());
        let dir = d.display();
        assert_eq!(f.iter().find(|x| x.name == "Gemini CLI").unwrap().command, format!("{dir}/gemini --acp"));
        assert_eq!(f.iter().find(|x| x.name == "Claude Code").unwrap().command, format!("{dir}/npx -y @agentclientprotocol/claude-agent-acp"));
        assert_eq!(f.iter().find(|x| x.name == "goose").unwrap().command, format!("{dir}/goose acp"));
        assert!(!f.iter().any(|x| x.name == "Codex")); // not installed
        assert_eq!(f.iter().filter(|x| x.name == "Claude Code").count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
