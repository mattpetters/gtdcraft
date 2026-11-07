//! TEMPORARY MCP setter for `Settings::decisions_url` only.
//!
//! Delete this file to remove the setter. Also remove `mod decisions_url_set;` in `src/main.rs`,
//! the tool registration and `call` dispatch in `src/mcp.rs`, and
//! `decisions_url_propagates_only_when_connections_modified_advances` in `src/sync.rs`.
//! Revert the commit that added this file and that list is the whole change.
//!
//! The window stamps `connections_modified` when `Store::connections_fingerprint` changes
//! (`Gtd::save`). This setter does the same assignment and no other write. Sync publishes the URL
//! only because that stamp moved (`sync::merge`); `shared_content` records the stamp, not the URL.

use crate::model::{now, Dt, Store};
use serde_json::{json, Value};
use std::path::Path;

pub const TOOL_NAME: &str = "gtd_set_decisions_url";

const NOTE_IDLE: &str = "Nothing was written.";
const NOTE_RESTART: &str = "Quit GTDCraft before relying on this file. A running window keeps settings in memory and its next save can write the old URL back over this one. This tool does not restart the app. Roll back by calling again with previous_url.";
const NOTE_STAMP: &str = "The URL was written, but connections_modified did not move forward (it has second resolution), so another device will not take this URL over an equal stamp.";

pub fn tool_spec() -> Value {
    json!({
        "name": TOOL_NAME,
        "description": "TEMPORARY local setter for the decisions URL only. Pass url (http or https, or \"\" to clear the override) and optional dry_run (true reads the current URL and writes nothing). Changes decisions_url and, when that changes the connections fingerprint, sets connections_modified the way Settings save does. Every other setting and secret is left as it was, and the reply never includes a key. Roll back by calling again with previous_url. Quit GTDCraft first: this does not restart the app, and a running window can overwrite the file on its next save. Not a general settings writer and not a file writer.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "Decision server URL, http or https. Stored as given; a base URL gets /v1/systemone when GTDCraft calls it. \"\" clears the override."},
                "dry_run": {"type": "boolean", "description": "Validate and read back without writing."}
            },
            "additionalProperties": false
        }
    })
}

struct Request {
    dry_run: bool,
    /// None reads the current URL and writes nothing.
    url: Option<String>,
}

struct Outcome {
    dry_run: bool,
    previous_url: String,
    decisions_url: String,
    previous_stamp: Option<Dt>,
    stamp: Option<Dt>,
    /// `stamp > previous_stamp`, which is what sync requires (`tc > oc`).
    advanced: bool,
    changed: bool,
    decisions_on: bool,
}

/// Validate `url` / `dry_run`, then read the database at `path` (the one `--mcp` already has).
/// A write is atomic (`Store::save`) and is skipped when the file changed after it was read.
pub fn set_decisions_url(path: &Path, args: &Value) -> Result<Value, String> {
    let request = parse_request(args)?;
    let _db = crate::sync::DB_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !path.is_file() {
        return Err(format!(
            "no database at {}; this tool does not create or choose files",
            path.display()
        ));
    }
    let snapshot = std::fs::read(path).map_err(|e| format!("couldn't read database: {e}"))?;
    let store = Store::load(path).map_err(|e| format!("couldn't read database: {e}"))?;
    let mut edited = store.clone();
    let outcome = apply(&mut edited, &request)?;
    let write = !request.dry_run && outcome.changed;
    if write {
        commit_if_snapshot(path, &snapshot, &edited)?;
        let readback =
            Store::load(path).map_err(|e| format!("saved but couldn't read back: {e}"))?;
        if readback.settings.decisions_url != edited.settings.decisions_url
            || readback.settings.connections_modified != edited.settings.connections_modified
        {
            return Err(
                "readback did not match what was written; no further change was made".into(),
            );
        }
        if rest_json(&readback)? != rest_json(&store)? {
            return Err(
                "readback changed a field other than decisions_url and connections_modified".into(),
            );
        }
    } else if std::fs::read(path).ok().as_deref() != Some(snapshot.as_slice()) {
        return Err("The database changed while it was being read. Nothing was written.".into());
    }
    Ok(reply(&outcome, write))
}

fn parse_request(args: &Value) -> Result<Request, String> {
    let obj = args.as_object().ok_or("arguments must be an object")?;
    for key in obj.keys() {
        if key != "url" && key != "dry_run" {
            return Err(format!(
                "gtd_set_decisions_url only accepts url and dry_run, not {key}"
            ));
        }
    }
    let dry_run = match obj.get("dry_run") {
        None => false,
        Some(Value::Bool(v)) => *v,
        Some(_) => return Err("dry_run must be true or false".into()),
    };
    let url = match obj.get("url") {
        None => None,
        Some(Value::String(s)) => Some(validate_decisions_url(s)?),
        Some(_) => return Err("url must be a string".into()),
    };
    if url.is_none() && !dry_run {
        return Err("url is required; pass dry_run true to read the current URL".into());
    }
    Ok(Request { dry_run, url })
}

/// `http` or `https` with a host and no username or password. Empty clears the override.
fn validate_decisions_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    if url.is_empty() {
        return Ok(String::new());
    }
    if url.len() > 2048 {
        return Err("URL is too long".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("URL must not contain spaces or control characters".into());
    }
    let rest = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if let Some(r) = url.strip_prefix("http://") {
        r
    } else {
        return Err("URL must start with http:// or https://".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("URL needs a host".into());
    }
    if authority.contains('@') {
        return Err("URL must not include a username or password".into());
    }
    let host = if let Some(h) = authority.strip_prefix('[') {
        let end = h.find(']').ok_or("URL has an unfinished IPv6 host")?;
        if end == 0 {
            return Err("URL needs a host".into());
        }
        &authority[..=end + 1]
    } else {
        authority.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        return Err("URL needs a host".into());
    }
    if let Some(port) = authority
        .strip_prefix(host)
        .and_then(|r| r.strip_prefix(':'))
    {
        if port.parse::<u16>().is_err() {
            return Err("URL port must be a number from 0 to 65535".into());
        }
    }
    Ok(url.to_string())
}

fn apply(store: &mut Store, request: &Request) -> Result<Outcome, String> {
    let previous_url = store.settings.decisions_url.clone();
    let previous_stamp = store.settings.connections_modified;
    let decisions_on = store.settings.decisions_on;
    let Some(url) = request.url.as_deref() else {
        return Ok(Outcome {
            dry_run: request.dry_run,
            previous_url: previous_url.clone(),
            decisions_url: previous_url,
            previous_stamp,
            stamp: previous_stamp,
            advanced: false,
            changed: false,
            decisions_on,
        });
    };
    let before = rest_json(store)?;
    let fingerprint = store.connections_fingerprint();
    store.settings.decisions_url = url.to_string();
    let changed = store.settings.decisions_url != previous_url;
    if changed != (store.connections_fingerprint() != fingerprint) {
        return Err(
            "refusing to write: connections_fingerprint did not follow decisions_url".into(),
        );
    }
    let stamp = if changed {
        let stamp = Some(now());
        // Same assignment as Gtd::save when the connections fingerprint changes.
        store.settings.connections_modified = stamp;
        stamp
    } else {
        previous_stamp
    };
    if rest_json(store)? != before {
        return Err("refusing to write: a field other than decisions_url would change".into());
    }
    Ok(Outcome {
        dry_run: request.dry_run,
        previous_url,
        decisions_url: store.settings.decisions_url.clone(),
        previous_stamp,
        stamp,
        advanced: stamp > previous_stamp,
        changed,
        decisions_on,
    })
}

/// The database as JSON with the two fields this tool is allowed to change removed.
fn rest_json(store: &Store) -> Result<Value, String> {
    let mut v =
        serde_json::to_value(store).map_err(|e| format!("couldn't compare the database: {e}"))?;
    if let Some(settings) = v.get_mut("settings").and_then(|s| s.as_object_mut()) {
        settings.remove("decisions_url");
        settings.remove("connections_modified");
    }
    Ok(v)
}

fn commit_if_snapshot(path: &Path, snapshot: &[u8], store: &Store) -> Result<(), String> {
    let current = std::fs::read(path).map_err(|e| format!("couldn't re-read database: {e}"))?;
    if current != snapshot {
        return Err("The database changed while it was being read. Nothing was written.".into());
    }
    store.save(path).map_err(|e| format!("couldn't save: {e}"))
}

fn stamp_text(stamp: Option<Dt>) -> Value {
    stamp
        .map(|t| json!(t.format("%Y-%m-%d %H:%M:%S").to_string()))
        .unwrap_or(Value::Null)
}

fn reply(outcome: &Outcome, written: bool) -> Value {
    let note = if written && !outcome.advanced {
        NOTE_STAMP
    } else if written {
        NOTE_RESTART
    } else {
        NOTE_IDLE
    };
    json!({
        "temporary": true,
        "dry_run": outcome.dry_run,
        "written": written,
        "previous_url": outcome.previous_url,
        "decisions_url": outcome.decisions_url,
        "connections_modified": stamp_text(if written || !outcome.changed { outcome.stamp } else { outcome.previous_stamp }),
        "connections_modified_after": stamp_text(outcome.stamp),
        "connections_modified_advanced": written && outcome.advanced,
        "would_advance_connections_modified": outcome.changed && outcome.advanced,
        "decisions_on": outcome.decisions_on,
        "restart_required": written,
        "note": note,
    })
}

#[cfg(test)]
mod tests {
    use super::{commit_if_snapshot, rest_json, set_decisions_url, TOOL_NAME};
    use crate::model::Store;
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    const SECRETS: [&str; 5] = [
        "sekret-decisions",
        "lin_api_sekret",
        "ghp_sekret",
        "sk-ai-sekret",
        "sync-tok-sekret",
    ];
    const OLD: &str = "http://127.0.0.1:8001";
    const NEW: &str = "http://127.0.0.1:8642";

    fn scratch(label: &str) -> (PathBuf, PathBuf) {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gtdcraft-decisions-url-{label}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gtdcraft.json");
        (dir, path)
    }

    fn seeded(path: &Path) -> Store {
        let mut s = Store::sample(crate::model::now());
        s.settings.decisions_on = true;
        s.settings.decisions_provider = "local".into();
        s.settings.decisions_url = OLD.into();
        s.settings.decisions_model = "clef-flash".into();
        s.settings.decisions_key = "sekret-decisions".into();
        s.settings.decisions_account = "acct-1".into();
        s.settings.linear_key = "lin_api_sekret".into();
        s.settings.github_token = "ghp_sekret".into();
        s.settings.ai_key = "sk-ai-sekret".into();
        s.settings.ai_base_url = "http://127.0.0.1:11434/v1".into();
        s.settings.sync_url = "http://mac.lan:8765".into();
        s.settings.sync_token = "sync-tok-sekret".into();
        s.settings.theme = "Focus Dark".into();
        s.settings.connections_modified = Some(crate::model::now() - chrono::Duration::hours(3));
        s.save(path).unwrap();
        // Compare the copy the file actually holds, so a serializer round trip is not a false change.
        Store::load(path).unwrap()
    }

    fn no_secrets(v: &Value) {
        let text = v.to_string();
        for secret in SECRETS {
            assert!(!text.contains(secret), "reply exposed {secret}");
        }
    }

    fn unchanged_aside(before: &Store, after: &Store) {
        assert_eq!(rest_json(before).unwrap(), rest_json(after).unwrap());
        assert_eq!(after.settings.decisions_key, "sekret-decisions");
        assert_eq!(after.settings.linear_key, "lin_api_sekret");
        assert_eq!(after.settings.github_token, "ghp_sekret");
        assert_eq!(after.settings.ai_key, "sk-ai-sekret");
        assert_eq!(after.settings.sync_token, "sync-tok-sekret");
        assert_eq!(after.settings.sync_url, "http://mac.lan:8765");
        assert_eq!(after.settings.theme, "Focus Dark");
        assert_eq!(after.settings.decisions_on, true);
        assert_eq!(after.settings.decisions_provider, "local");
        assert_eq!(after.settings.decisions_model, "clef-flash");
        assert_eq!(after.settings.decisions_account, "acct-1");
        assert_eq!(after.settings.ai_base_url, "http://127.0.0.1:11434/v1");
        assert!(after
            .projects
            .iter()
            .any(|p| p.name == "Paint the spare room"));
    }

    #[test]
    fn dry_run_reads_the_url_back_and_writes_nothing() {
        let (dir, path) = scratch("dry");
        let before = seeded(&path);
        let bytes = std::fs::read(&path).unwrap();
        let stamp = before.settings.connections_modified;
        let r = set_decisions_url(
            &path,
            &json!({"url": format!("  {NEW}/v1/systemone  "), "dry_run": true}),
        )
        .unwrap();
        no_secrets(&r);
        assert_eq!(r["written"], false);
        assert_eq!(r["dry_run"], true);
        assert_eq!(r["restart_required"], false);
        assert_eq!(r["previous_url"], OLD);
        assert_eq!(r["decisions_url"], format!("{NEW}/v1/systemone"));
        assert_eq!(r["would_advance_connections_modified"], true);
        assert_eq!(r["connections_modified_advanced"], false);
        assert_eq!(r["decisions_on"], true);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let after = Store::load(&path).unwrap();
        assert_eq!(after.settings.decisions_url, OLD);
        assert_eq!(after.settings.connections_modified, stamp);
        unchanged_aside(&before, &after);
        let read = set_decisions_url(&path, &json!({"dry_run": true})).unwrap();
        no_secrets(&read);
        assert_eq!(
            (
                read["previous_url"].as_str(),
                read["decisions_url"].as_str(),
                read["written"].as_bool()
            ),
            (Some(OLD), Some(OLD), Some(false))
        );
        assert_eq!(read["would_advance_connections_modified"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sets_only_the_url_and_advances_the_stamp_then_rolls_back() {
        let (dir, path) = scratch("set");
        let before = seeded(&path);
        let old_stamp = before.settings.connections_modified.unwrap();
        let r = crate::mcp::call(&path, TOOL_NAME, &json!({"url": NEW})).unwrap();
        no_secrets(&r);
        assert_eq!(r["written"], true);
        assert_eq!(r["restart_required"], true);
        assert_eq!(r["previous_url"], OLD);
        assert_eq!(r["decisions_url"], NEW);
        assert_eq!(r["connections_modified_advanced"], true);
        assert_eq!(r["would_advance_connections_modified"], true);
        assert!(r["note"].as_str().unwrap().contains("Quit GTDCraft"));
        let mid = Store::load(&path).unwrap();
        assert_eq!(mid.settings.decisions_url, NEW);
        assert!(mid.settings.connections_modified.unwrap() > old_stamp);
        unchanged_aside(&before, &mid);
        // adopted fingerprint: a later non-connection edit is not a new connection change
        // (Gtd::save stamps only when connections_fingerprint differs from adopt_prints)
        let adopted = mid.connections_fingerprint();
        let mut later = mid.clone();
        later.projects[0].name = "renamed in the window".into();
        assert_eq!(later.connections_fingerprint(), adopted);
        assert_eq!(later.settings.decisions_url, NEW);

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let back =
            set_decisions_url(&path, &json!({"url": r["previous_url"].as_str().unwrap()})).unwrap();
        no_secrets(&back);
        assert_eq!(back["decisions_url"], OLD);
        assert_eq!(back["previous_url"], NEW);
        assert_eq!(back["written"], true);
        let restored = Store::load(&path).unwrap();
        assert_eq!(restored.settings.decisions_url, OLD);
        assert!(
            restored.settings.connections_modified.unwrap()
                > mid.settings.connections_modified.unwrap()
        );
        unchanged_aside(&before, &restored);
        let cleared = set_decisions_url(&path, &json!({"url": ""})).unwrap();
        no_secrets(&cleared);
        assert_eq!(cleared["decisions_url"], "");
        assert_eq!(cleared["previous_url"], OLD);
        let empty = Store::load(&path).unwrap();
        assert_eq!(empty.settings.decisions_url, "");
        unchanged_aside(&before, &empty);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_same_url_does_not_write_or_restamp() {
        let (dir, path) = scratch("same");
        seeded(&path);
        let bytes = std::fs::read(&path).unwrap();
        let r = set_decisions_url(&path, &json!({"url": OLD})).unwrap();
        no_secrets(&r);
        assert_eq!(r["written"], false);
        assert_eq!(r["connections_modified_advanced"], false);
        assert_eq!(r["would_advance_connections_modified"], false);
        assert_eq!(r["restart_required"], false);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_urls_extra_fields_and_missing_files_without_creating_one() {
        let (dir, path) = scratch("bad");
        seeded(&path);
        let bytes = std::fs::read(&path).unwrap();
        for url in [
            "ftp://127.0.0.1:8642",
            "file:///tmp/gtdcraft.json",
            "http://user:pass@127.0.0.1:8642",
            "http://",
            "127.0.0.1:8642",
            "http://127.0.0.1:8642 extra",
            "javascript:alert(1)",
        ] {
            let err = set_decisions_url(&path, &json!({"url": url})).unwrap_err();
            assert!(!err.contains("sekret"), "{err}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{url}");
        }
        let err = set_decisions_url(
            &path,
            &json!({"url": NEW, "decisions_key": "sekret-decisions", "path": "/tmp/other.json"}),
        )
        .unwrap_err();
        assert!(err.contains("only accepts"));
        assert!(!err.contains("sekret-decisions"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(set_decisions_url(&path, &json!({"dry_run": "yes"})).is_err());
        assert!(set_decisions_url(&path, &json!({}))
            .unwrap_err()
            .contains("url is required"));
        let missing = dir.join("nope.json");
        let err = set_decisions_url(&missing, &json!({"url": NEW})).unwrap_err();
        assert!(err.contains("does not create"));
        assert!(!missing.exists());
        let ok = set_decisions_url(&path, &json!({"url": "http://[::1]:8642", "dry_run": true}))
            .unwrap();
        assert_eq!(ok["decisions_url"], "http://[::1]:8642");
        assert_eq!(ok["written"], false);
        let cleared = set_decisions_url(&path, &json!({"url": "  ", "dry_run": true})).unwrap();
        assert_eq!(cleared["decisions_url"], "");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_changed_under_us_is_left_alone() {
        let (dir, path) = scratch("race");
        seeded(&path);
        let snapshot = std::fs::read(&path).unwrap();
        let mut gui = Store::load(&path).unwrap();
        gui.settings.decisions_url = "http://127.0.0.1:9".into();
        gui.settings.decisions_key = "sekret-decisions".into();
        gui.tasks[0].name = "typed in the window".into();
        gui.save(&path).unwrap();
        let mut stale = crate::dbfile::from_str(
            std::str::from_utf8(&snapshot).unwrap(),
            crate::dbfile::format_of(&path),
        )
        .unwrap();
        stale.settings.decisions_url = NEW.into();
        let err = commit_if_snapshot(&path, &snapshot, &stale).unwrap_err();
        assert!(err.contains("changed"));
        let kept = Store::load(&path).unwrap();
        assert_eq!(kept.settings.decisions_url, "http://127.0.0.1:9");
        assert_eq!(kept.settings.decisions_key, "sekret-decisions");
        assert_eq!(kept.tasks[0].name, "typed in the window");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn jsonl_keeps_its_format_and_the_tool_is_on_the_mcp_list() {
        let (dir, _) = scratch("jsonl");
        let path = dir.join("db.jsonl");
        seeded(&path);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .contains("_record"));
        let r = set_decisions_url(&path, &json!({"url": NEW})).unwrap();
        assert_eq!(r["decisions_url"], NEW);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.lines().next().unwrap().contains("_record"));
        let loaded = Store::load(&path).unwrap();
        assert_eq!(loaded.settings.decisions_url, NEW);
        assert_eq!(loaded.settings.decisions_key, "sekret-decisions");
        assert!(loaded
            .projects
            .iter()
            .any(|p| p.name == "Paint the spare room"));

        let listed = crate::mcp::handle(
            &path,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )
        .unwrap();
        let spec = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == TOOL_NAME)
            .unwrap();
        let props = spec["inputSchema"]["properties"].as_object().unwrap();
        assert_eq!(props.len(), 2);
        assert!(props.contains_key("url") && props.contains_key("dry_run"));
        let msg = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name": TOOL_NAME, "arguments": {"url": OLD, "dry_run": true}}});
        let reply = crate::mcp::handle(&path, &msg).unwrap();
        assert!(reply.get("error").is_none());
        let body: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        no_secrets(&body);
        assert_eq!(body["previous_url"], NEW);
        assert_eq!(body["written"], false);
        let batched = crate::mcp::call(
            &path,
            "gtd_batch",
            &json!({"calls": [{"name": TOOL_NAME, "arguments": {"dry_run": true}}]}),
        )
        .unwrap();
        assert_eq!(batched[0]["previous_url"], NEW);
        no_secrets(&batched);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_adopted_stamp_is_what_keeps_the_next_save_from_looking_like_a_new_edit() {
        // Documents Gtd::save / adopt_prints: once the window reloads, the fingerprint matches
        // and a task edit does not assign a new connections_modified. This test does not run the
        // window. A window that still holds the old URL in memory overwrites the file on save.
        let (dir, path) = scratch("adopt");
        seeded(&path);
        set_decisions_url(&path, &json!({"url": NEW})).unwrap();
        let mut window = Store::load(&path).unwrap();
        let print = window.connections_fingerprint();
        let stamp = window.settings.connections_modified;
        window.tasks[0].note = "edited after reload".into();
        let restamp = window.connections_fingerprint() != print;
        assert!(!restamp);
        assert_eq!(window.settings.connections_modified, stamp);
        assert_eq!(window.settings.decisions_url, NEW);
        let _ = std::fs::remove_dir_all(dir);
    }
}
