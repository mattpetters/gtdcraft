//! Database files: the format follows the extension (.json, .jsonc with comments, .jsonl one record per line —
//! the git-friendly one), which database is current (shared with `--mcp`), recent databases, and git history.
use crate::model::Store;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Json,
    Jsonc,
    Jsonl,
}

pub fn format_of(path: &Path) -> Format {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jsonl") | Some("ndjson") => Format::Jsonl,
        Some("jsonc") => Format::Jsonc,
        _ => Format::Json,
    }
}

/// Record type key on each JSONL line (projects already have a `kind` field).
const TAG: &str = "_record";
const LISTS: [(&str, &str); 3] = [("projects", "project"), ("tags", "tag"), ("tasks", "task")];

pub fn to_string(store: &Store, f: Format) -> serde_json::Result<String> {
    match f {
        Format::Json => serde_json::to_string_pretty(store),
        Format::Jsonc => Ok(format!(
            "// GTDCraft database (JSON with comments). Comments are allowed anywhere; GTDCraft rewrites this header on save.\n{}\n",
            serde_json::to_string_pretty(store)?
        )),
        Format::Jsonl => {
            let Value::Object(mut meta) = serde_json::to_value(store)? else { unreachable!() };
            let mut out = String::new();
            let mut rows = vec![];
            for (list, kind) in LISTS {
                if let Some(Value::Array(items)) = meta.remove(list) {
                    for it in items {
                        let mut o = Map::new();
                        o.insert(TAG.into(), Value::String(kind.into()));
                        if let Value::Object(m) = it {
                            o.extend(m);
                        }
                        rows.push(Value::Object(o));
                    }
                }
            }
            let mut head = Map::new();
            head.insert(TAG.into(), Value::String("meta".into()));
            head.extend(meta);
            out.push_str(&serde_json::to_string(&Value::Object(head))?);
            out.push('\n');
            for r in rows {
                out.push_str(&serde_json::to_string(&r)?);
                out.push('\n');
            }
            Ok(out)
        }
    }
}

pub fn from_str(s: &str, f: Format) -> serde_json::Result<Store> {
    match f {
        Format::Json => serde_json::from_str(s),
        Format::Jsonc => serde_json::from_str(&strip_comments(s)),
        Format::Jsonl => {
            let mut meta = Map::new();
            let mut lists: Vec<(&str, Vec<Value>)> = LISTS.iter().map(|(l, _)| (*l, vec![])).collect();
            for line in s.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let Value::Object(mut o) = serde_json::from_str::<Value>(line)? else { continue };
                let kind = o.remove(TAG).and_then(|k| k.as_str().map(String::from)).unwrap_or_default();
                match LISTS.iter().position(|(_, k)| *k == kind) {
                    Some(i) => lists[i].1.push(Value::Object(o)),
                    None => meta.extend(o), // "meta"
                }
            }
            for (l, v) in lists {
                meta.insert(l.into(), Value::Array(v));
            }
            serde_json::from_value(Value::Object(meta))
        }
    }
}

/// Removes // and /* */ comments outside strings.
pub fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i, mut in_str) = (Vec::with_capacity(b.len()), 0, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
            out.push(c);
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            out.push(c);
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

// ---------- which database is open ----------

pub fn app_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Application Support/GTDCraft")
}

/// The database the app last opened (the MCP server follows it too).
pub fn current() -> Option<PathBuf> {
    let p = std::fs::read_to_string(app_dir().join("current-database")).ok()?;
    let p = PathBuf::from(p.trim());
    (!p.as_os_str().is_empty()).then_some(p)
}

pub fn set_current(path: &Path) {
    let _ = std::fs::create_dir_all(app_dir());
    let _ = std::fs::write(app_dir().join("current-database"), path.to_string_lossy().as_bytes());
    let mut r = recent();
    r.retain(|p| p != path);
    r.insert(0, path.to_path_buf());
    r.truncate(10);
    let list: Vec<String> = r.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let _ = std::fs::write(app_dir().join("recent-databases.json"), serde_json::to_vec_pretty(&list).unwrap_or_default());
}

pub fn recent() -> Vec<PathBuf> {
    std::fs::read(app_dir().join("recent-databases.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

// ---------- git ----------

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn split(path: &Path) -> (PathBuf, String) {
    let dir = path.parent().map(Path::to_path_buf).filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (dir, name)
}

/// Is the database inside a git work tree?
pub fn git_tracked(path: &Path) -> bool {
    let (dir, _) = split(path);
    git(&dir, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s.trim() == "true")
}

/// Commit identity: the person's own git config when set, otherwise a local one.
fn identity(dir: &Path) -> Vec<String> {
    if git(dir, &["config", "user.email"]).is_ok_and(|s| !s.trim().is_empty()) {
        vec![]
    } else {
        vec!["-c".into(), "user.name=GTDCraft".into(), "-c".into(), "user.email=gtdcraft@localhost".into()]
    }
}

/// Starts version history in the database's folder (a new repository unless it's already inside one).
pub fn git_init(path: &Path) -> Result<(), String> {
    let (dir, _) = split(path);
    if !git_tracked(path) {
        git(&dir, &["init", "-q"])?;
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*.tmp\n*.unreadable.json\n").map_err(|e| e.to_string())?;
            git(&dir, &["add", ".gitignore"])?;
        }
    }
    git_commit(path, "chore(gtdcraft): start version history").map(|_| ())
}

/// Commits the database file if it changed. Ok(false) = nothing to commit.
pub fn git_commit(path: &Path, msg: &str) -> Result<bool, String> {
    let (dir, name) = split(path);
    if git(&dir, &["status", "--porcelain", "--", &name])?.trim().is_empty() {
        return Ok(false);
    }
    git(&dir, &["add", "--", &name])?;
    let mut args: Vec<String> = identity(&dir);
    args.extend(["commit", "-q", "-m", msg, "--", &name].map(String::from));
    git(&dir, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    Ok(true)
}

/// Pushes when the branch has an upstream; Ok(false) when there's nowhere to push.
pub fn git_push(path: &Path) -> Result<bool, String> {
    let (dir, _) = split(path);
    if git(&dir, &["rev-parse", "--abbrev-ref", "@{u}"]).is_err() {
        return Ok(false);
    }
    git(&dir, &["push", "-q"]).map(|_| true)
}

#[derive(Clone, Debug)]
pub struct Version {
    pub hash: String,
    pub when: String,
    pub message: String,
}

pub fn git_log(path: &Path, n: usize) -> Result<Vec<Version>, String> {
    let (dir, name) = split(path);
    let out = git(&dir, &["log", &format!("-n{n}"), "--format=%H%x1f%ad%x1f%s", "--date=format:%a %b %-d %Y, %-I:%M %p", "--", &name])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\x1f');
            Some(Version { hash: p.next()?.into(), when: p.next()?.into(), message: p.next().unwrap_or("").into() })
        })
        .collect())
}

/// The database as it was at a version.
pub fn git_show(path: &Path, hash: &str) -> Result<Store, String> {
    let (dir, name) = split(path);
    let text = git(&dir, &["show", &format!("{hash}:./{name}")])?;
    from_str(&text, format_of(path)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::now;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gtdcraft-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn formats_roundtrip() {
        let s = Store::sample(now());
        for f in [Format::Json, Format::Jsonc, Format::Jsonl] {
            let text = to_string(&s, f).unwrap();
            let back = from_str(&text, f).unwrap();
            assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&s).unwrap(), "{f:?}");
        }
        let l = to_string(&s, Format::Jsonl).unwrap();
        assert_eq!(l.lines().count(), 1 + s.tasks.len() + s.projects.len() + s.tags.len()); // one record per line
        assert!(l.lines().next().unwrap().contains("\"_record\":\"meta\""));
    }

    #[test]
    fn comments_are_stripped_outside_strings() {
        let t = "// top\n{ \"a\": \"x // not a comment\", /* gone */ \"b\": 2 // tail\n}";
        let v: Value = serde_json::from_str(&strip_comments(t)).unwrap();
        assert_eq!(v["a"], "x // not a comment");
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn git_history() {
        let d = tmpdir("git");
        let p = d.join("work.jsonl");
        let mut s = Store::sample(now());
        s.save(&p).unwrap();
        git_init(&p).unwrap();
        assert!(git_tracked(&p));
        assert!(!git_commit(&p, "chore(gtdcraft): no change").unwrap());
        let first_name = s.tasks[0].name.clone();
        s.tasks[0].name = "Changed in v2".into();
        s.save(&p).unwrap();
        assert!(git_commit(&p, "chore(gtdcraft): v2").unwrap());
        let log = git_log(&p, 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message, "chore(gtdcraft): v2");
        let old = git_show(&p, &log[1].hash).unwrap();
        assert_eq!(old.tasks[0].name, first_name);
        assert!(!git_push(&p).unwrap()); // no upstream
        let _ = std::fs::remove_dir_all(&d);
    }
}
