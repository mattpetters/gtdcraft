//! Sync between your devices through a small server you run (`gtdcraft --sync-server`).
//!
//! Each device sends its whole database; the server merges it with its copy and sends the result back. Merging is per
//! record: the newer `modified` wins for tasks, projects and tags; deletes travel as tombstones (`Store::deleted`) and win
//! over older edits; the list order is the pushing device's, with records it hasn't seen yet kept after their
//! neighbours. Settings (themes, shortcuts, perspectives, keys) stay on each device. Each device mints IDs from its own
//! range (`Store::id_base`) so two devices never create the same ID.
use crate::model::{Dt, Id, Store};
use chrono::Datelike;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// The records two copies agree on, merged. `ours` supplies the order and the settings; on equal `modified` times
/// `ours` (the copy already here) wins: the existing record is kept unless it was modified strictly before the
/// incoming one.
pub fn merge(theirs: &Store, ours: &Store) -> Store {
    merge_ties(theirs, ours, false)
}

/// `merge`, but `theirs` wins ties: the server merges a device's copy in while its own stored copy is the existing one.
pub fn merge_ties(theirs: &Store, ours: &Store, ties_to_theirs: bool) -> Store {
    let mut out = ours.clone();
    // tombstones: union, latest time
    let mut deleted: BTreeMap<Id, Dt> = theirs.deleted.clone();
    for (id, at) in &ours.deleted {
        let e = deleted.entry(*id).or_insert(*at);
        if *at > *e {
            *e = *at;
        }
    }
    out.tasks = merge_list(&theirs.tasks, &ours.tasks, |t| t.id, |t| Some(t.modified), &deleted, ties_to_theirs);
    out.projects = merge_list(&theirs.projects, &ours.projects, |p| p.id, |p| Some(p.modified), &deleted, ties_to_theirs);
    out.tags = merge_list(&theirs.tags, &ours.tags, |g| g.id, |g| g.modified, &deleted, ties_to_theirs);
    out.folders = merge_list(&theirs.folders, &ours.folders, |f| f.id, |f| f.modified, &deleted, ties_to_theirs);
    // the sidebar's perspectives and the inspector's section order travel too: the newer layout wins
    let (tl, ol) = (theirs.settings.layout_modified, ours.settings.layout_modified);
    if tl > ol || (ties_to_theirs && tl == ol && tl.is_some()) {
        out.settings.perspectives = theirs.settings.perspectives.clone();
        out.settings.inspector_order = theirs.settings.inspector_order.clone();
        out.settings.layout_modified = tl;
    }
    // connections (Linear, GitHub, AI) too: the newer set wins
    let (tc, oc) = (theirs.settings.connections_modified, ours.settings.connections_modified);
    if tc > oc || (ties_to_theirs && tc == oc && tc.is_some()) {
        let (t, o) = (&theirs.settings, &mut out.settings);
        (o.linear_key, o.github_token, o.ai_provider, o.ai_key, o.ai_model, o.ai_base_url) = (t.linear_key.clone(), t.github_token.clone(), t.ai_provider.clone(), t.ai_key.clone(), t.ai_model.clone(), t.ai_base_url.clone());
        (o.decisions_on, o.decisions_provider, o.decisions_url, o.decisions_model, o.decisions_key, o.decisions_account) = (t.decisions_on, t.decisions_provider.clone(), t.decisions_url.clone(), t.decisions_model.clone(), t.decisions_key.clone(), t.decisions_account.clone());
        o.agent_templates = t.agent_templates.clone();
        o.connections_modified = tc;
    }
    // Up Next: the newer order wins
    if theirs.queue_modified > ours.queue_modified || (ties_to_theirs && theirs.queue_modified == ours.queue_modified && theirs.queue_modified.is_some()) {
        out.queue = theirs.queue.clone();
        out.queue_modified = theirs.queue_modified;
    }
    // a copy without an identity (from before) takes the other's
    if out.db_id.is_empty() {
        out.db_id = theirs.db_id.clone();
    }
    out.deleted = deleted;
    out.next_id = theirs.next_id.max(ours.next_id);
    // append-only bits: union
    for p in &theirs.pomodoros {
        if !out.pomodoros.contains(p) {
            out.pomodoros.push(*p);
        }
    }
    // a deleted task takes its Pomodoros with it (the union above would bring them back from the other copy)
    out.pomodoros.retain(|p| p.1.is_none_or(|id| !out.deleted.contains_key(&id)));
    out.pomodoros.sort_by_key(|p| p.0);
    for (k, v) in &theirs.link_cards {
        out.link_cards.entry(k.clone()).or_insert_with(|| v.clone());
    }
    for (k, v) in &theirs.linear {
        out.linear.entry(k.clone()).or_insert_with(|| v.clone());
    }
    // drop references to deleted things
    let live: std::collections::HashSet<Id> = out.tasks.iter().map(|t| t.id).collect();
    out.queue.retain(|id| live.contains(id));
    let tag_ids: std::collections::HashSet<Id> = out.tags.iter().map(|g| g.id).collect();
    for t in &mut out.tasks {
        t.tags.retain(|g| tag_ids.contains(g));
    }
    out
}

/// Per-record newest-wins; a tombstone at least as new as a record removes it.
fn merge_list<T: Clone>(theirs: &[T], ours: &[T], id: impl Fn(&T) -> Id, when: impl Fn(&T) -> Option<Dt>, deleted: &BTreeMap<Id, Dt>, ties_to_theirs: bool) -> Vec<T> {
    let their_map: HashMap<Id, &T> = theirs.iter().map(|x| (id(x), x)).collect();
    let our_ids: std::collections::HashSet<Id> = ours.iter().map(&id).collect();
    let pick = |x: &T| -> T {
        match their_map.get(&id(x)) {
            Some(t) if when(t) > when(x) || (ties_to_theirs && when(t) == when(x)) => (*t).clone(),
            _ => x.clone(),
        }
    };
    // our order, then theirs-only records after the record they follow in their list
    let mut out: Vec<T> = ours.iter().map(pick).collect();
    let mut prev: Option<Id> = None;
    for t in theirs {
        let tid = id(t);
        if !our_ids.contains(&tid) {
            let at = prev.and_then(|p| out.iter().position(|x| id(x) == p)).map(|i| i + 1).unwrap_or(out.len());
            out.insert(at, t.clone());
        }
        prev = Some(tid);
    }
    out.retain(|x| match deleted.get(&id(x)) {
        Some(at) => when(x).is_some_and(|m| m > *at),
        None => true,
    });
    out
}

// ---------- device identity ----------

/// This device's ID range: a random 20-bit tag × 2^32, kept in Application Support (so every database opened on this
/// Mac uses the same range). IDs stay below 2^53, which JSON readers in JavaScript keep exact.
/// IDs the sync server itself mints (inbox items from the Share extension): above every device's range.
/// The sync server's database is read, merged and written by requests and by approved agent threads (agent.rs) on
/// other threads: one at a time.
pub static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub const SERVER_ID_BASE: Id = (1 << 20) << 32;

pub fn device_base() -> Id {
    let f = crate::dbfile::app_dir().join("device-id");
    let tag: u64 = match std::fs::read_to_string(&f).ok().and_then(|s| s.trim().parse().ok()) {
        Some(t) => t,
        None => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let t = ((nanos ^ (std::process::id() as u128) << 17) % ((1 << 20) - 1)) as u64 + 1;
            let _ = std::fs::create_dir_all(crate::dbfile::app_dir());
            let _ = std::fs::write(&f, t.to_string());
            t
        }
    };
    tag << 32
}

// ---------- client ----------

/// One round trip: send our database, get the merged one back.
pub fn sync_with(url: &str, token: &str, ours: &Store, secs: u64) -> Result<Store, String> {
    let body = serde_json::to_string(ours).map_err(|e| e.to_string())?;
    let r = ureq::post(&format!("{}/v1/sync", url.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(secs))
        .send_string(&body);
    match r {
        Ok(resp) => serde_json::from_reader(resp.into_reader()).map_err(|e| format!("Sync server reply: {e}")),
        Err(ureq::Error::Status(401, _)) => Err("The sync server didn't accept the token.".into()),
        Err(ureq::Error::Status(code, resp)) => Err(format!("Sync server error {code}: {}", resp.into_string().unwrap_or_default())),
        Err(e) => Err(format!("Couldn't reach the sync server: {e}")),
    }
}

/// The database this device syncs (its identity), kept beside the app's files — not in the database, so opening another
/// database file doesn't carry it along.
pub fn linked_db() -> String {
    std::fs::read_to_string(crate::dbfile::app_dir().join("sync-db")).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn set_linked_db(id: &str) {
    let _ = std::fs::write(crate::dbfile::app_dir().join("sync-db"), id);
}

/// A backup's file name: the app name, the reason (e.g. "Before Replacing Local Data with Sync Data"), "on" and the
/// date as "yyyy-MM-dd HHmmss", then the backup extension.
pub fn backup_name(reason: &str, at: crate::model::Dt) -> String {
    format!("GTDCraft {reason} on {}.json", at.format("%Y-%m-%d %H%M%S"))
}

/// Whether `store` is the database this device syncs: yes when it has never synced (the first sync links it), and
/// when the identities match; otherwise syncing waits for the person (a different or brand-new database, or a file
/// from before identities, must not merge into the synced one).
pub fn same_database(linked: &str, store_id: &str) -> bool {
    linked.is_empty() || linked == store_id
}

/// Nothing in it yet: a new install or a new database (sample data aside).
pub fn is_empty_store(s: &Store) -> bool {
    s.tasks.is_empty() && s.projects.is_empty()
}

/// One sync, then the server's revision for this database. A device that has never synced and holds nothing joins
/// the server's most recently changed database rather than starting an empty one there.
pub fn sync_round(url: &str, token: &str, ours: &Store, join_newest: bool) -> Result<(Store, String), String> {
    let base = url.trim_end_matches('/');
    let mut ours = ours.clone();
    if join_newest {
        if let Ok(list) = crate::agent::request(base, token, "GET", "/v1/databases", None) {
            if let Some(id) = list.as_array().and_then(|l| l.first()).and_then(|d| d["id"].as_str()).filter(|i| !i.is_empty()) {
                ours.db_id = id.to_string();
            }
        }
    }
    let remote = sync_with(base, token, &ours, 30)?;
    let rev = crate::agent::request(base, token, "GET", &format!("/v1/rev?db={}", remote.db_id), None).ok().and_then(|v| v["rev"].as_str().map(String::from)).unwrap_or_default();
    Ok((remote, rev))
}

// ---------- server ----------

pub struct ServerConfig {
    pub listen: String,
    pub data: PathBuf,
    pub token: String,
    /// Where `/v1/systemone` forwards to (the Clef sidecar on this machine by default).
    pub clef: String,
    /// `--agents`: Send to Agent is on regardless of the opt-in file (agent.rs).
    pub agents: bool,
}

/// `gtdcraft --sync-server --listen 0.0.0.0:8765 --data ~/gtdcraft-sync --token <secret>`.
/// One database per server (one person, many devices). GET /v1/health, POST /v1/sync, and POST /v1/systemone, which
/// forwards decisions to Clef (`--clef-url`) so iPhone and iPad get them through the same server and token.
///
/// Sync is the core; decisions and agents are subsystems (`subsystems.rs`). Requests run on a pool of workers, so a
/// slow decision or agent call never holds up a sync, and a request that panics is answered 500 without taking the
/// server down. Writes to a database stay one at a time (`DB_LOCK`).
pub fn serve(cfg: ServerConfig) -> Result<(), String> {
    let server = std::sync::Arc::new(tiny_http::Server::http(&cfg.listen).map_err(|e| format!("Couldn't listen on {}: {e}", cfg.listen))?);
    std::fs::create_dir_all(&cfg.data).map_err(|e| e.to_string())?;
    let db = cfg.data.join("gtdcraft.jsonl");
    eprintln!("GTDCraft sync server on http://{} (data: {})", cfg.listen, db.display());
    let ctx = std::sync::Arc::new(Ctx {
        db,
        hub: crate::agent::hub(&cfg.data, cfg.agents),
        decisions: crate::subsystems::Decisions::new(&cfg.clef),
        token: cfg.token,
    });
    let probe = ctx.clone();
    std::thread::spawn(move || loop {
        probe.decisions.probe();
        std::thread::sleep(std::time::Duration::from_secs(crate::subsystems::PROBE_SECS));
    });
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let (server, ctx) = (server.clone(), ctx.clone());
            std::thread::spawn(move || {
                while let Ok(req) = server.recv() {
                    respond(req, &ctx);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

/// Requests handled at once. A decision call holds one for up to `subsystems::FORWARD_SECS` and at most
/// `subsystems::MAX_INFLIGHT` do, so syncs always have workers.
const WORKERS: usize = 8;

struct Ctx {
    db: PathBuf,
    token: String,
    hub: std::sync::Arc<crate::agent::Hub>,
    decisions: crate::subsystems::Decisions,
}

fn respond(mut req: tiny_http::Request, ctx: &Ctx) {
    let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if req.url().starts_with("/v1/agent/") {
            agent_route(&mut req, &ctx.hub, &ctx.token)
        } else {
            handle(&mut req, ctx)
        }
    }))
    .unwrap_or_else(|_| Err((500, "internal error (logged); sync and the other subsystems keep running".into())));
    {
        let (code, body) = match reply {
            Ok(b) => (200, b),
            Err((c, msg)) => {
                // every refusal and failure goes in the server's log (sync-server.log), with its path but no query
                // (it may carry a database id) and never the token, so "is it running without errors" has an answer
                let path = req.url().split('?').next().unwrap_or("").to_string();
                eprintln!("{} {} {} → {c}: {msg}", crate::model::now().format("%Y-%m-%d %H:%M:%S"), req.method(), path);
                (c, serde_json::json!({ "error": msg }).to_string())
            }
        };
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("header");
        let mut resp = tiny_http::Response::from_string(body).with_status_code(code).with_header(header);
        if code == 503 {
            let secs = crate::subsystems::RETRY_AFTER_SECS.to_string();
            resp = resp.with_header(tiny_http::Header::from_bytes(&b"Retry-After"[..], secs.as_bytes()).expect("header"));
        }
        let _ = req.respond(resp);
    }
}

/// The server's copy of a database: one file per database identity. The first file (`gtdcraft.jsonl`, from before
/// databases had identities) belongs to the identity it was given, or to copies without one; any other identity has
/// its own `db-<id>.jsonl`. So a device that opens or starts a different database syncs it separately — it never
/// merges into, or empties, the one your other devices use.
pub fn db_path(data: &Path, id: &str) -> PathBuf {
    let legacy = data.join("gtdcraft.jsonl");
    let safe: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect();
    if safe.is_empty() || stored_id(&legacy).as_deref() == Some(&safe) {
        return legacy;
    }
    data.join(format!("db-{safe}.jsonl"))
}

/// The identity recorded in a server copy (read from its first lines; None if there's no file).
fn stored_id(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    Store::load(path).ok().map(|s| s.db_id)
}

/// `?db=…` from a request's URL.
fn db_param(url: &str) -> String {
    url.split_once('?').map(|x| x.1).unwrap_or("").split('&').find_map(|kv| kv.strip_prefix("db=")).unwrap_or("").to_string()
}

/// What syncing shares, for telling whether a sync changed anything: everything but a device's own settings (its
/// theme, window, server address…), which differ between devices and would otherwise make every sync look like a
/// change — devices would keep sending each other syncing every few seconds. The shared settings (perspectives,
/// section order, connections) stay in, with their stamps.
fn shared_content(s: &Store) -> serde_json::Value {
    let mut v = serde_json::to_value(s).unwrap_or_default();
    let st = &s.settings;
    if let Some(o) = v.as_object_mut() {
        o.insert(
            "settings".into(),
            serde_json::json!({"perspectives": st.perspectives, "inspector_order": st.inspector_order, "layout_modified": st.layout_modified, "connections_modified": st.connections_modified}),
        );
    }
    v
}

/// The databases this server keeps: identity, action count, last change (`GET /v1/databases`).
fn list_databases(data: &Path) -> Vec<serde_json::Value> {
    let mut out = vec![];
    for e in std::fs::read_dir(data).into_iter().flatten().flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name == "gtdcraft.jsonl" || (name.starts_with("db-") && name.ends_with(".jsonl"))) {
            continue;
        }
        let Ok(s) = Store::load(&p) else { continue };
        let modified = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        out.push(serde_json::json!({"id": s.db_id, "actions": s.tasks.len(), "modified": modified}));
    }
    out.sort_by_key(|v| std::cmp::Reverse(v["modified"].as_u64()));
    out
}

/// A database's revision for `GET /v1/rev`: its file's modification time in milliseconds — devices ask every few
/// seconds and sync at once when it moves (so starting the clock on one shows on the others within seconds).
fn rev(path: &Path) -> u128 {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis()).unwrap_or(0)
}

/// /v1/agent/… under the same token (agent.rs).
fn agent_route(req: &mut tiny_http::Request, hub: &std::sync::Arc<crate::agent::Hub>, token: &str) -> Result<String, (u16, String)> {
    let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string()).unwrap_or_default();
    if token.is_empty() || auth != format!("Bearer {token}") {
        return Err((401, "bad token".into()));
    }
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let mut body = String::new();
    std::io::Read::read_to_string(req.as_reader(), &mut body).map_err(|e| (400, e.to_string()))?;
    hub.route(req.method().as_str(), path, query, &body)
}

fn handle(req: &mut tiny_http::Request, ctx: &Ctx) -> Result<String, (u16, String)> {
    let (db, token) = (&ctx.db, &ctx.token);
    let full_path = req.url().to_string();
    let path = full_path.split('?').next().unwrap_or(&full_path);
    let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string()).unwrap_or_default();
    let authed = !token.is_empty() && auth == format!("Bearer {token}");
    if matches!((req.method(), path), (tiny_http::Method::Get, "/v1/health")) {
        // without the token: only that sync answers (a hosting platform's health check needs no secret, and says
        // nothing about what runs here); with it, each subsystem's state
        if !authed {
            return Ok(r#"{"ok":true}"#.into());
        }
        let agents = if ctx.hub.enabled() { "up" } else { "off" };
        return Ok(serde_json::json!({"ok": true, "subsystems": {"sync": {"state": "up"}, "decisions": ctx.decisions.status(), "agents": {"state": agents}}}).to_string());
    }
    if !authed {
        return Err((401, "bad token".into()));
    }
    let data = db.parent().unwrap_or(Path::new(".")).to_path_buf();
    let pick = db_path(&data, &db_param(&full_path));
    match (req.method(), path) {
        (tiny_http::Method::Post, "/v1/sync") => handle_sync(req, &data),
        (tiny_http::Method::Get, "/v1/rev") => Ok(serde_json::json!({"rev": rev(&pick).to_string()}).to_string()),
        (tiny_http::Method::Get, "/v1/databases") => Ok(serde_json::json!(list_databases(&data)).to_string()),
        (tiny_http::Method::Post, "/v1/systemone") => {
            let mut body = String::new();
            std::io::Read::read_to_string(req.as_reader(), &mut body).map_err(|e| (400, e.to_string()))?;
            ctx.decisions.forward(&body)
        }
        (tiny_http::Method::Get, "/v1/widget") => handle_widget(&pick),
        (tiny_http::Method::Post, "/v1/inbox") => handle_inbox(req, &pick),
        _ => Err((404, "not found".into())),
    }
}

fn handle_sync(req: &mut tiny_http::Request, data: &Path) -> Result<String, (u16, String)> {
    let mut body = String::new();
    std::io::Read::read_to_string(req.as_reader(), &mut body).map_err(|e| (400, e.to_string()))?;
    let theirs: Store = serde_json::from_str(&body).map_err(|e| (400, format!("not a GTDCraft database: {e}")))?;
    let _db = DB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // the copy for this database's identity (a device's other database syncs to its own copy)
    let db = &db_path(data, &theirs.db_id);
    // the server's copy merges in the device's changes; the device's order and settings win for its reply, and the
    // server's stored copy (the existing one) wins records modified at the same moment
    let ours = if db.exists() { Store::load(db).map_err(|e| (500, e.to_string()))? } else { Store::default() };
    let mut merged = merge_ties(&ours, &theirs, true);
    // the identity: the stored copy's, else the device's; the first copy from before gets one now, which the devices
    // syncing it adopt
    merged.db_id = [&ours.db_id, &theirs.db_id].into_iter().find(|i| !i.is_empty()).cloned().unwrap_or_else(crate::model::new_db_id);
    // only a real change moves the revision devices watch (`/v1/rev`), so a sync that brings nothing new doesn't
    // send every other device syncing too
    if !db.exists() || shared_content(&merged) != shared_content(&ours) {
        merged.save(db).map_err(|e| (500, e.to_string()))?;
    }
    serde_json::to_string(&merged).map_err(|e| (500, e.to_string()))
}

/// A small snapshot of the database for widgets: focus task, Up Next, due today, time tracked.
pub fn widget_summary(store: &Store) -> serde_json::Value {
    let now = crate::model::now();
    let d = store.derive(now);
    let today = now.date();
    // Focus task: active clock, else first in Up Next
    let focus_id = store.active_clock().map(|c| c.0).or_else(|| store.queue_live().first().copied());
    let focus = focus_id.and_then(|id| store.task(id)).map(|t| {
        let project = t.project.and_then(|p| store.project(p)).map(|p| p.name.as_str()).unwrap_or("");
        serde_json::json!({ "name": t.name, "project": project })
    });
    // Up Next, after the Focus item (which led the list too)
    let up_next: Vec<serde_json::Value> = store.queue_live().iter().filter(|id| Some(**id) != focus_id).take(5).filter_map(|id| {
        store.task(*id).map(|t| serde_json::json!({ "name": t.name }))
    }).collect();
    // Due today count + names
    let due_today: Vec<serde_json::Value> = store.tasks.iter().filter(|t| {
        t.completed.is_none() && t.dropped.is_none() && d.eff_due.get(&t.id).is_some_and(|dt| dt.date() == today)
    }).take(10).map(|t| serde_json::json!({ "name": t.name })).collect();
    let due_today_count = store.tasks.iter().filter(|t| {
        t.completed.is_none() && t.dropped.is_none() && d.eff_due.get(&t.id).is_some_and(|dt| dt.date() == today)
    }).count();
    // Time tracked today and this week
    let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    let time_today: i64 = store.tasks.iter().flat_map(|t| t.clock.iter()).filter(|c| c.end.unwrap_or(now).date() >= today || c.start.date() == today).map(|c| {
        let start = c.start.max(today.and_time(chrono::NaiveTime::MIN));
        (c.end.unwrap_or(now) - start).num_minutes().max(0)
    }).sum();
    let time_week: i64 = store.tasks.iter().flat_map(|t| t.clock.iter()).filter(|c| c.end.unwrap_or(now).date() >= week_start || c.start.date() >= week_start).map(|c| {
        let start = c.start.max(week_start.and_time(chrono::NaiveTime::MIN));
        (c.end.unwrap_or(now) - start).num_minutes().max(0)
    }).sum();
    // Active pomodoro: check for a running clock entry with pomodoro=true
    let active_pomo = store.tasks.iter().flat_map(|t| t.clock.iter()).any(|c| c.end.is_none() && c.pomodoro);
    serde_json::json!({
        "focus": focus,
        "up_next": up_next,
        "due_today": due_today,
        "due_today_count": due_today_count,
        "time_today_min": time_today,
        "time_week_min": time_week,
        "active_pomodoro": active_pomo,
    })
}

fn handle_widget(db: &Path) -> Result<String, (u16, String)> {
    let store = if db.exists() { Store::load(db).map_err(|e| (500, e.to_string()))? } else { Store::default() };
    Ok(widget_summary(&store).to_string())
}

/// POST /v1/inbox — create a task in the inbox, optionally with a reference URL.
fn handle_inbox(req: &mut tiny_http::Request, db: &Path) -> Result<String, (u16, String)> {
    let mut body = String::new();
    std::io::Read::read_to_string(req.as_reader(), &mut body).map_err(|e| (400, e.to_string()))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| (400, e.to_string()))?;
    let name = v["name"].as_str().unwrap_or("").trim();
    let note = v["note"].as_str().unwrap_or("").trim();
    let ref_url = v["ref_url"].as_str().unwrap_or("").trim();
    let ref_title = v["ref_title"].as_str().unwrap_or("").trim();
    if name.is_empty() && ref_url.is_empty() {
        return Err((400, "name or ref_url required".into()));
    }
    let _db = DB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = if db.exists() { Store::load(db).map_err(|e| (500, e.to_string()))? } else { Store::default() };
    // the server mints from a range of its own: device ranges are tag << 32 with tags 1..2^20 (`device_base`), and
    // this server shares a machine (and its device-id file) with the Mac app, so it must not borrow that range
    store.id_base = SERVER_ID_BASE;
    let task_name = if name.is_empty() { ref_title.to_string().chars().take(80).collect::<String>() } else { name.to_string() };
    let id = store.add_task(&task_name, None, None);
    if !note.is_empty() {
        if let Some(t) = store.task_mut(id) {
            t.note = note.to_string();
        }
    }
    if !ref_url.is_empty() {
        if let Some(t) = store.task_mut(id) {
            t.refs.push(crate::refs::Ref { url: ref_url.to_string(), title: ref_title.to_string(), state: String::new() });
        }
    }
    store.save(db).map_err(|e| (500, e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "id": id }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{now, Task};
    use chrono::Duration;

    fn named<'a>(s: &'a Store, name: &str) -> Option<&'a Task> {
        s.tasks.iter().find(|t| t.name == name)
    }

    #[test]
    fn merge_newest_wins_and_deletes_travel() {
        let mut a = Store::default();
        a.id_base = 1 << 32;
        let x = a.add_task("Shared", None, None);
        let mut b = a.clone();
        b.id_base = 2 << 32;
        // both add, both edit the shared task; B's edit is newer
        let ya = a.add_task("Only on A", None, None);
        let yb = b.add_task("Only on B", None, None);
        assert_ne!(ya, yb); // device ranges
        a.task_mut(x).unwrap().name = "Shared (A)".into();
        a.task_mut(x).unwrap().modified = now() - Duration::minutes(5);
        b.task_mut(x).unwrap().name = "Shared (B)".into();
        b.task_mut(x).unwrap().modified = now();
        let m = merge(&a, &b);
        assert!(named(&m, "Only on A").is_some() && named(&m, "Only on B").is_some());
        assert_eq!(m.task(x).unwrap().name, "Shared (B)");
        // A deletes the shared task later: the delete wins on both sides
        let mut a2 = merge(&m, &a);
        a2.delete_task(x);
        let back = merge(&m, &a2);
        assert!(back.task(x).is_none());
        let other = merge(&back, &m); // B syncs again with its stale copy
        assert!(other.task(x).is_none());
        // an edit made after the delete brings it back (newer than the tombstone)
        let mut revived = m.clone();
        revived.task_mut(x).unwrap().modified = now() + Duration::minutes(1);
        assert!(merge(&back, &revived).task(x).is_some());
    }

    #[test]
    fn ties_keep_the_existing_copy() {
        let mut here = Store::default();
        let x = here.add_task("Here", None, None);
        let mut incoming = here.clone();
        incoming.task_mut(x).unwrap().name = "Incoming".into();
        incoming.task_mut(x).unwrap().modified = here.task(x).unwrap().modified;
        assert_eq!(merge(&incoming, &here).task(x).unwrap().name, "Here");
        // the server: its stored copy is the existing one, the device's order is kept
        assert_eq!(merge_ties(&here, &incoming, true).task(x).unwrap().name, "Here");
    }

    /// One copy per database: two devices without identities share the first copy (and adopt the identity it gets);
    /// a new database syncs to its own copy and never mixes in; a sync with nothing new leaves the revision alone.
    #[test]
    fn databases_stay_apart() {
        let dir = std::env::temp_dir().join(format!("gtdcraft-dbs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = 21765 + (std::process::id() % 1000) as u16;
        let listen = format!("127.0.0.1:{port}");
        let cfg = ServerConfig { listen: listen.clone(), data: dir.clone(), token: "tk".into(), clef: String::new(), agents: false };
        std::thread::spawn(move || serve(cfg));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let url = format!("http://{listen}");
        let mut mac = Store::default();
        mac.id_base = 1 << 32;
        mac.add_task("Shared work", None, None);
        let mac = merge(&sync_with(&url, "tk", &mac, 30).unwrap(), &mac);
        assert!(!mac.db_id.is_empty(), "the first copy got an identity, and the device adopted it");
        let mut phone = Store::default();
        phone.id_base = 2 << 32;
        let phone = merge(&sync_with(&url, "tk", &phone, 30).unwrap(), &phone);
        assert_eq!(phone.db_id, mac.db_id);
        assert!(named(&phone, "Shared work").is_some());
        // a brand-new database: its own copy, nothing from the other
        let mut other = Store { db_id: crate::model::new_db_id(), ..Store::default() };
        other.id_base = 3 << 32;
        other.add_task("Somewhere else", None, None);
        let other = sync_with(&url, "tk", &other, 30).unwrap();
        assert!(named(&other, "Shared work").is_none());
        let mac2 = sync_with(&url, "tk", &mac, 30).unwrap();
        assert!(named(&mac2, "Somewhere else").is_none());
        assert_eq!(list_databases(&dir).len(), 2);
        // nothing new: the revision devices watch stays put — even from a device whose own settings differ
        let path = db_path(&dir, &mac.db_id);
        let r0 = rev(&path);
        std::thread::sleep(std::time::Duration::from_millis(20));
        sync_with(&url, "tk", &mac2, 30).unwrap();
        let mut phone2 = merge(&mac2, &phone);
        phone2.settings.theme = "Something else".into();
        sync_with(&url, "tk", &phone2, 30).unwrap();
        assert_eq!(rev(&path), r0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The newer Up Next wins, and a clock started on one device reaches the other.
    #[test]
    fn up_next_and_clocks_travel() {
        let mut mac = Store::default();
        mac.id_base = 1 << 32;
        let a = mac.add_task("Write", None, None);
        let b = mac.add_task("Call", None, None);
        mac.queue = vec![a];
        mac.queue_modified = Some(now() - Duration::minutes(5));
        for t in &mut mac.tasks {
            t.modified = now() - Duration::minutes(5); // made a while ago
        }
        let mut phone = mac.clone();
        phone.queue = vec![b, a];
        phone.queue_modified = Some(now());
        phone.clock_in(b, now(), false);
        let m = merge(&phone, &mac);
        assert_eq!(m.queue, vec![b, a]);
        assert_eq!(m.active_clock().map(|c| c.0), Some(b));
        assert_eq!(merge(&mac, &phone).queue, vec![b, a]); // either way round
    }

    #[test]
    fn backup_file_names() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().and_hms_opt(16, 45, 7).unwrap();
        assert_eq!(backup_name("Before Replacing Local Data with Sync Data", at), "GTDCraft Before Replacing Local Data with Sync Data on 2026-11-03 164507.json");
    }

    #[test]
    fn same_database_rules() {
        assert!(same_database("", "abc")); // never synced: the first sync links it
        assert!(same_database("abc", "abc"));
        assert!(!same_database("abc", "def")); // another database
        assert!(!same_database("abc", "")); // a file from before identities
    }

    /// `decisions_url` is one of the connection fields copied when `connections_modified` is newer
    /// (merge, the block that starts at the connections stamp). `shared_content` stores that stamp
    /// and not the URL, and the server skips a save when shared content is unchanged — so a URL
    /// edit that does not move the stamp never reaches another device. Device settings
    /// (sync address, token, theme) stay on the copy already here. The newer stamp publishes the
    /// whole connection bundle, including keys, not the URL by itself.
    #[test]
    fn decisions_url_propagates_only_when_connections_modified_advances() {
        let older = now() - Duration::hours(2);
        let newer = now() - Duration::hours(1);
        let mut mac = Store::default();
        mac.settings.decisions_on = true;
        mac.settings.decisions_provider = "local".into();
        mac.settings.decisions_url = "http://127.0.0.1:8001".into();
        mac.settings.decisions_model = "clef-flash".into();
        mac.settings.decisions_key = "secret-decisions".into();
        mac.settings.decisions_account = "acct".into();
        mac.settings.linear_key = "lin_secret".into();
        mac.settings.github_token = "gh_secret".into();
        mac.settings.ai_key = "ai_secret".into();
        mac.settings.sync_url = "http://mac.lan:8765".into();
        mac.settings.sync_token = "device-token".into();
        mac.settings.theme = "Focus Dark".into();
        mac.settings.connections_modified = Some(older);

        let mut url_only = mac.clone();
        url_only.settings.decisions_url = "http://127.0.0.1:8642".into();
        assert_eq!(shared_content(&url_only), shared_content(&mac));
        let stayed = shared_content(&url_only).to_string();
        assert!(
            !stayed.contains("8642"),
            "shared_content tracks the stamp, not the URL"
        );
        assert!(stayed.contains("connections_modified"));
        let stayed = merge(&url_only, &mac);
        assert_eq!(stayed.settings.decisions_url, "http://127.0.0.1:8001");
        assert_eq!(stayed.settings.connections_modified, Some(older));
        assert_eq!(stayed.settings.decisions_key, "secret-decisions");
        assert_eq!(stayed.settings.sync_token, "device-token");
        assert_eq!(stayed.settings.theme, "Focus Dark");

        let mut stamped = url_only.clone();
        stamped.settings.connections_modified = Some(newer);
        assert_ne!(shared_content(&stamped), shared_content(&mac));
        let moved = merge(&stamped, &mac);
        assert_eq!(moved.settings.decisions_url, "http://127.0.0.1:8642");
        assert_eq!(moved.settings.connections_modified, Some(newer));
        assert_eq!(moved.settings.decisions_key, "secret-decisions");
        assert_eq!(moved.settings.decisions_model, "clef-flash");
        assert_eq!(moved.settings.decisions_on, true);
        assert_eq!(moved.settings.linear_key, "lin_secret");
        assert_eq!(moved.settings.github_token, "gh_secret");
        assert_eq!(moved.settings.ai_key, "ai_secret");
        assert_eq!(moved.settings.sync_url, "http://mac.lan:8765");
        assert_eq!(moved.settings.sync_token, "device-token");
        assert_eq!(moved.settings.theme, "Focus Dark");

        // an older stamp must not replace a newer URL
        let mut stale = mac.clone();
        stale.settings.decisions_url = "http://127.0.0.1:8001/v1/systemone".into();
        assert_eq!(
            merge(&stale, &moved).settings.decisions_url,
            "http://127.0.0.1:8642"
        );
        // the stamp publishes every connection field from that copy, keys included
        let mut bundle = stamped.clone();
        bundle.settings.decisions_key = "other-key".into();
        assert_eq!(merge(&bundle, &mac).settings.decisions_key, "other-key");
        assert_eq!(
            merge(&bundle, &mac).settings.decisions_url,
            "http://127.0.0.1:8642"
        );

        // through the server: no new stamp, no new revision and the stored URL stays;
        // a newer stamp is what the other device receives, without its own sync token
        let dir =
            std::env::temp_dir().join(format!("gtdcraft-decisions-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = 31000 + (std::process::id() % 1000) as u16;
        let listen = format!("127.0.0.1:{port}");
        let cfg = ServerConfig {
            listen: listen.clone(),
            data: dir.clone(),
            token: "tk".into(),
            clef: String::new(),
            agents: false,
        };
        std::thread::spawn(move || serve(cfg));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let url = format!("http://{listen}");
        let mut mac = mac;
        mac.id_base = 1 << 32;
        mac.add_task("Keep me", None, None);
        let mac = merge(&sync_with(&url, "tk", &mac, 30).expect("sync server"), &mac);
        let stored_path = db_path(&dir, &mac.db_id);
        let rev0 = rev(&stored_path);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut url_only = mac.clone();
        url_only.settings.decisions_url = "http://127.0.0.1:8642".into();
        let echoed = sync_with(&url, "tk", &url_only, 30).unwrap();
        assert_eq!(echoed.settings.decisions_url, "http://127.0.0.1:8001");
        assert_eq!(rev(&stored_path), rev0);
        assert_eq!(
            Store::load(&stored_path).unwrap().settings.decisions_url,
            "http://127.0.0.1:8001"
        );

        let mut stamped = mac.clone();
        stamped.settings.decisions_url = "http://127.0.0.1:8642".into();
        stamped.settings.connections_modified = Some(newer);
        let echoed = sync_with(&url, "tk", &stamped, 30).unwrap();
        assert_eq!(echoed.settings.decisions_url, "http://127.0.0.1:8642");
        assert_eq!(echoed.settings.decisions_key, "secret-decisions");
        assert_eq!(
            Store::load(&stored_path).unwrap().settings.decisions_url,
            "http://127.0.0.1:8642"
        );

        let mut phone = Store::default();
        phone.id_base = 2 << 32;
        phone.db_id = mac.db_id.clone();
        phone.settings.sync_url = "http://phone.local:8765".into();
        phone.settings.sync_token = "phone-token".into();
        phone.settings.theme = "Focus Light".into();
        phone.settings.decisions_url = "http://127.0.0.1:8001".into();
        phone.settings.connections_modified = Some(older);
        let remote = sync_with(&url, "tk", &phone, 30).unwrap();
        let phone = merge(&remote, &phone);
        assert_eq!(phone.settings.decisions_url, "http://127.0.0.1:8642");
        assert_eq!(phone.settings.decisions_key, "secret-decisions");
        assert_eq!(phone.settings.linear_key, "lin_secret");
        assert_eq!(phone.settings.sync_url, "http://phone.local:8765");
        assert_eq!(phone.settings.sync_token, "phone-token");
        assert_eq!(phone.settings.theme, "Focus Light");
        assert!(phone.tasks.iter().any(|t| t.name == "Keep me"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hung_decision_upstream_never_blocks_sync() {
        // an upstream that accepts and never answers in time (a stuck Clef)
        let hung = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let hung_url = format!("http://127.0.0.1:{}", hung.server_addr().to_ip().unwrap().port());
        std::thread::spawn(move || {
            for req in hung.incoming_requests() {
                std::thread::sleep(std::time::Duration::from_secs(40));
                let _ = req.respond(tiny_http::Response::from_string("{}"));
            }
        });
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("gtdcraft-subsys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = ServerConfig { listen: format!("127.0.0.1:{port}"), data: dir.clone(), token: "tk".into(), clef: hung_url, agents: false };
        std::thread::spawn(move || serve(cfg));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let url = format!("http://127.0.0.1:{port}");
        // fill every decision slot with calls that hang
        let q = serde_json::json!({"q": {"type": "noul"}});
        for _ in 0..crate::subsystems::MAX_INFLIGHT {
            let (url, q) = (url.clone(), q.clone());
            std::thread::spawn(move || {
                let r = crate::systemone::Route { url: crate::systemone::endpoint_url(&url), model: "m".into(), key: Some("tk".into()) };
                let _ = crate::systemone::decide(&r, &serde_json::json!("x"), &q);
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        // one more decision is refused at once, with Retry-After
        let busy = ureq::post(&format!("{url}/v1/systemone")).set("Authorization", "Bearer tk").send_string("{}");
        match busy {
            Err(ureq::Error::Status(503, r)) => assert_eq!(r.header("Retry-After"), Some("30")),
            other => panic!("expected 503, got {other:?}"),
        }
        // and sync still answers promptly
        let started = std::time::Instant::now();
        let mut mac = Store::default();
        mac.add_task("Still syncing", None, None);
        let merged = sync_with(&url, "tk", &mac, 5).unwrap();
        assert!(named(&merged, "Still syncing").is_some());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "sync took {:?}", started.elapsed());
        // health: bare without the token, subsystems with it
        let public: serde_json::Value = serde_json::from_reader(ureq::get(&format!("{url}/v1/health")).call().unwrap().into_reader()).unwrap();
        assert_eq!(public, serde_json::json!({"ok": true}));
        let private: serde_json::Value = serde_json::from_reader(ureq::get(&format!("{url}/v1/health")).set("Authorization", "Bearer tk").call().unwrap().into_reader()).unwrap();
        assert_eq!(private["subsystems"]["decisions"]["in_flight"], crate::subsystems::MAX_INFLIGHT);
        assert_eq!(private["subsystems"]["agents"]["state"], "off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn server_round_trip() {
        let dir = std::env::temp_dir().join(format!("gtdcraft-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = 18765 + (std::process::id() % 1000) as u16;
        let listen = format!("127.0.0.1:{port}");
        let (clef, _) = crate::systemone::tests::fake_clef();
        let cfg = ServerConfig { listen: listen.clone(), data: dir.clone(), token: "t0ken".into(), clef, agents: false };
        std::thread::spawn(move || serve(cfg));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let url = format!("http://{listen}");
        let mut mac = Store::default();
        mac.id_base = 1 << 32;
        mac.add_task("From the Mac", None, None);
        let mut phone = Store::default();
        phone.id_base = 2 << 32;
        phone.add_task("From the phone", None, None);
        assert!(sync_with(&url, "wrong", &mac, 30).unwrap_err().contains("token"));
        let mac = sync_with(&url, "t0ken", &mac, 30).unwrap();
        let phone = sync_with(&url, "t0ken", &phone, 30).unwrap();
        let mac = sync_with(&url, "t0ken", &mac, 30).unwrap();
        for s in [&mac, &phone] {
            assert!(named(s, "From the Mac").is_some() && named(s, "From the phone").is_some());
        }
        // decisions go through the same server and token
        let q = serde_json::json!({"urgent": {"type": "noul"}});
        let route = |key: &str| crate::systemone::Route { url: crate::systemone::endpoint_url(&url), model: "clef-flash".into(), key: Some(key.into()) };
        let a = crate::systemone::decide(&route("t0ken"), &serde_json::json!("server down"), &q).unwrap();
        assert_eq!(a["urgent"]["noul"], 0.75);
        assert!(crate::systemone::decide(&route("wrong"), &serde_json::json!("x"), &q).unwrap_err().contains("401"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn widget_endpoint_returns_summary() {
        let dir = std::env::temp_dir().join(format!("gtdcraft-widget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = 19765 + (std::process::id() % 1000) as u16;
        let listen = format!("127.0.0.1:{port}");
        let cfg = ServerConfig { listen: listen.clone(), data: dir.clone(), token: "wt".into(), clef: String::new(), agents: false };
        std::thread::spawn(move || serve(cfg));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let url = format!("http://{listen}");
        // push some data first
        let mut s = Store::default();
        s.id_base = 1 << 32;
        s.add_task("Buy milk", None, None);
        let t = s.tasks.last_mut().unwrap();
        t.due = Some(now());
        sync_with(&url, "wt", &s, 30).unwrap();
        // fetch the widget summary
        let resp = ureq::get(&format!("{url}/v1/widget")).set("Authorization", "Bearer wt").call().unwrap();
        let v: serde_json::Value = serde_json::from_reader(resp.into_reader()).unwrap();
        assert_eq!(v["due_today_count"], 1);
        assert!(v["due_today"].as_array().unwrap().iter().any(|d| d["name"] == "Buy milk"));
        // bad token
        assert!(ureq::get(&format!("{url}/v1/widget")).set("Authorization", "Bearer wrong").call().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_endpoint_creates_task() {
        let dir = std::env::temp_dir().join(format!("gtdcraft-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = 20765 + (std::process::id() % 1000) as u16;
        let listen = format!("127.0.0.1:{port}");
        let cfg = ServerConfig { listen: listen.clone(), data: dir.clone(), token: "ib".into(), clef: String::new(), agents: false };
        std::thread::spawn(move || serve(cfg));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let url = format!("http://{listen}");
        // create a task with a reference
        let body = serde_json::json!({ "name": "Read article", "ref_url": "https://example.com/article", "ref_title": "Good article" });
        let resp = ureq::post(&format!("{url}/v1/inbox")).set("Authorization", "Bearer ib").set("Content-Type", "application/json").send_string(&body.to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_reader(resp.into_reader()).unwrap();
        assert!(v["ok"].as_bool().unwrap());
        let id = v["id"].as_u64().unwrap();
        // verify it's in the database
        let store = Store::load(&dir.join("gtdcraft.jsonl")).unwrap();
        let t = store.task(id).expect("task should exist");
        assert_eq!(t.name, "Read article");
        assert_eq!(t.refs.len(), 1);
        assert_eq!(t.refs[0].url, "https://example.com/article");
        assert!(t.project.is_none(), "inbox task has no project");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn widget_summary_computes_stats() {
        let mut s = Store::default();
        s.id_base = 1 << 32;
        let a = s.add_task("Due today", None, None);
        s.task_mut(a).unwrap().due = Some(now());
        let b = s.add_task("Clock test", None, None);
        s.task_mut(b).unwrap().clock.push(crate::model::Clock {
            start: now() - Duration::minutes(30),
            end: Some(now()),
            pomodoro: false,
        });
        s.queue_add(a);
        let c = s.add_task("Then this", None, None);
        s.queue_add(c);
        let v = widget_summary(&s);
        assert_eq!(v["due_today_count"], 1);
        assert!(v["time_today_min"].as_i64().unwrap() >= 29);
        assert!(v["time_week_min"].as_i64().unwrap() >= 29);
        // the first in Up Next is the Focus item; the list shows what comes after it
        assert_eq!(v["focus"]["name"], "Due today");
        assert_eq!(v["up_next"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Then this"]);
    }
}

// ---------- the app's side ----------

/// Every minute while syncing is set up (and a second after a save, on Sync Now, and when the server's copy moves).
const EVERY: f64 = 60.0;
/// Failed syncs in a row before the banner shows (one can be a blip: the Mac waking, Wi-Fi changing).
const FAIL_BANNER_AFTER: u32 = 2;
/// How often devices ask the server whether its copy moved (`/v1/rev`).
const REV_EVERY: f64 = 3.0;
/// How long quitting waits for the last sync.
const QUIT_SECS: u64 = 5;

impl crate::Gtd {
    pub(crate) fn sync_tick(&mut self, ctx: &eframe::egui::Context) {
        if let Some(rx) = &self.sync_rx {
            // While you're typing in a title, note or field the pulled changes wait in the channel and fold in once
            // you stop
            if self.user_active() {
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
                return;
            }
            let Ok(r) = rx.try_recv() else { return };
            self.sync_rx = None;
            match r {
                Ok((remote, rev)) => {
                    // the first sync links this device to the server's copy (a new, empty one joined the newest)
                    if linked_db().is_empty() && !remote.db_id.is_empty() {
                        self.store.db_id = remote.db_id.clone();
                    }
                    if !remote.db_id.is_empty() && remote.db_id == self.store.db_id {
                        set_linked_db(&remote.db_id);
                    }
                    self.last_rev = rev;
                    // fold in edits made while the request was out; ours win where newer
                    let mut merged = merge(&remote, &self.store);
                    merged.id_base = self.store.id_base;
                    let changed = serde_json::to_value(&merged.tasks).ok() != serde_json::to_value(&self.store.tasks).ok()
                        || serde_json::to_value(&merged.projects).ok() != serde_json::to_value(&self.store.projects).ok()
                        || serde_json::to_value(&merged.tags).ok() != serde_json::to_value(&self.store.tags).ok()
                        || merged.deleted != self.store.deleted
                        || merged.folders != self.store.folders
                        || merged.settings.layout_modified != self.store.settings.layout_modified
                        || merged.settings.connections_modified != self.store.settings.connections_modified
                        || merged.queue != self.store.queue
                        || merged.db_id != self.store.db_id;
                    self.store = merged;
                    self.adopt_prints(); // adopted, not a new change here
                    if changed {
                        self.snapshot = Some(self.store.clone());
                        self.save();
                    }
                    // iPhone/iPad: widgets show the new state now, not at their next scheduled refresh
                    #[cfg(target_os = "ios")]
                    crate::stats::live::reload_widgets();
                    self.sync_msg = format!("Synced at {}", crate::model::now().format("%-I:%M %p"));
                    self.sync_failures = 0;
                    self.status.synced_at = Some(crate::model::now());
                    self.sync_fail_hidden = false;
                }
                Err(e) => {
                    self.sync_msg = e;
                    self.sync_failures += 1;
                }
            }
            self.sync_due = Some(self.time + EVERY);
            return;
        }
        let url = self.store.settings.sync_url.trim().to_string();
        if url.is_empty() || self.shot.is_some() || !self.persist {
            return;
        }
        // a different database than the one this device syncs: wait for the person (the banner)
        let linked = linked_db();
        self.sync_paused = !same_database(&linked, &self.store.db_id);
        if self.sync_paused {
            return;
        }
        // every few seconds, ask whether the server's copy moved (another device changed something): sync at once
        if let Some(rx) = &self.rev_rx {
            if let Ok(rev) = rx.try_recv() {
                self.rev_rx = None;
                if !rev.is_empty() && !self.last_rev.is_empty() && rev != self.last_rev {
                    self.sync_due = Some(0.0);
                }
            }
        } else if self.time - self.rev_at > REV_EVERY && !self.last_rev.is_empty() {
            self.rev_at = self.time;
            let (tx, rx) = std::sync::mpsc::channel();
            let (u, t, id) = (url.clone(), self.store.settings.sync_token.clone(), self.store.db_id.clone());
            std::thread::spawn(move || {
                let r = crate::agent::request(&u, &t, "GET", &format!("/v1/rev?db={id}"), None).ok().and_then(|v| v["rev"].as_str().map(String::from)).unwrap_or_default();
                let _ = tx.send(r);
            });
            self.rev_rx = Some(rx);
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(REV_EVERY));
        let due = self.sync_due.unwrap_or(0.0);
        if self.time < due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((due - self.time).clamp(0.05, REV_EVERY)));
            return;
        }
        self.sync_due = None;
        let (token, ours) = (self.store.settings.sync_token.clone(), self.store.clone());
        let join_newest = linked.is_empty() && is_empty_store(&self.store);
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(sync_round(&url, &token, &ours, join_newest));
            ctx.request_repaint();
        });
        self.sync_rx = Some(rx);
    }

    /// Syncing keeps failing: say so where it's seen, with the reason and Retry. Changes stay on this device meanwhile
    /// and go out at the next sync that works.
    fn sync_fail_banner(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let r = ctx.content_rect();
        let mut act = 0;
        egui::Area::new(egui::Id::new("sync-failing")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, r.top() + 64.0)).show(ctx, |ui| {
            egui::Frame::new().fill(crate::mix(crate::th().bg, crate::th().overdue, 0.22)).corner_radius(14).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_max_width((r.width() - 32.0).min(560.0));
                ui.label(egui::RichText::new("Can't sync").strong());
                ui.label(format!("{} Your changes stay on this device and go out when the server answers.", self.sync_msg));
                ui.horizontal(|ui| {
                    if ui.button("Retry").clicked() {
                        act = 1;
                    }
                    if ui.button("Not Now").clicked() {
                        act = 2;
                    }
                });
            });
        });
        match act {
            1 => self.sync_due = Some(0.0),
            2 => self.sync_fail_hidden = true,
            _ => {}
        }
    }

    /// A banner while syncing waits: this isn't the database this device syncs (another file, a new one, or one from
    /// before databases had identities). Sync it on its own, or replace it with the synced one.
    pub(crate) fn sync_banner(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        if !self.sync_paused && self.sync_failures >= FAIL_BANNER_AFTER && !self.sync_fail_hidden && self.shot.is_none() {
            self.sync_fail_banner(ctx);
        }
        if !self.sync_paused || self.sync_banner_hidden || self.shot.is_some() {
            return;
        }
        let r = ctx.content_rect();
        let mut act = 0;
        egui::Area::new(egui::Id::new("sync-paused")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, r.top() + 64.0)).show(ctx, |ui| {
            egui::Frame::new().fill(crate::mix(crate::th().bg, crate::hue("orange"), 0.22)).corner_radius(14).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_max_width((r.width() - 32.0).min(560.0));
                ui.label(egui::RichText::new("Sync paused").strong());
                ui.label("This isn't the database this device syncs with your others, so nothing is sent or merged — neither copy can overwrite the other.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Sync This Database on Its Own").clicked() {
                        act = 1;
                    }
                    if ui.button("Replace with the Synced Database").clicked() {
                        act = 2;
                    }
                    if ui.button("Not Now").clicked() {
                        act = 3;
                    }
                });
            });
        });
        match act {
            1 => {
                // its own copy on the server, and this device now syncs it
                if self.store.db_id.is_empty() {
                    self.store.db_id = crate::model::new_db_id();
                    self.touch();
                }
                set_linked_db(&self.store.db_id);
                self.sync_paused = false;
                self.last_rev.clear();
                self.sync_due = Some(0.0);
            }
            2 => {
                // keep a copy of what's here, then start from the synced database (the next sync fills it in)
                let dir = crate::dbfile::app_dir().join("Backups");
                let _ = std::fs::create_dir_all(&dir);
                let backup = dir.join(backup_name("Before Replacing Local Data with Sync Data", crate::model::now()));
                let _ = self.store.save(&backup);
                let settings = self.store.settings.clone();
                self.store = Store { db_id: linked_db(), settings, ..Store::default() };
                self.touch();
                self.sync_paused = false;
                self.last_rev.clear();
                self.sync_due = Some(0.0);
                self.sync_msg = format!("Your earlier copy is saved as {}", backup.display());
            }
            3 => self.sync_banner_hidden = true,
            _ => {}
        }
    }

    pub(crate) fn user_active(&self) -> bool {
        self.editing.is_some() || self.field_open.is_some()
    }

    /// One last short, blocking sync as the app closes.
    pub(crate) fn sync_on_quit(&mut self) {
        let url = self.store.settings.sync_url.trim().to_string();
        if url.is_empty() || self.shot.is_some() || !self.persist {
            return;
        }
        if let Ok(remote) = sync_with(&url, &self.store.settings.sync_token, &self.store, QUIT_SECS) {
            let id_base = self.store.id_base;
            self.store = merge(&remote, &self.store);
            self.adopt_prints();
            self.store.id_base = id_base;
            self.save();
        }
    }

    /// Settings › General › Sync.
    pub(crate) fn settings_sync(&mut self, ui: &mut eframe::egui::Ui) -> bool {
        use eframe::egui;
        let mut ch = false;
        ui.heading("Sync");
        ui.label(egui::RichText::new(if cfg!(target_os = "ios") {
            "Keep this device in step with your Mac. On the Mac, open GTDCraft › Settings › Sync and scan the code there with the Camera, or type its server and token here."
        } else {
            "Keep your Mac, iPhone and iPad in step through a sync server you run: gtdcraft --sync-server --token <secret>"
        }).color(crate::th().dim).size(12.0));
        let st = &mut self.store.settings;
        ui.horizontal(|ui| {
            ui.add_sized([70.0, 20.0], egui::Label::new("Server"));
            ch |= ui.add(crate::nativetext::Edit::singleline(&mut st.sync_url).kind(crate::nativetext::Kind::Url).hint_text("http://my-mac.local:8765 — or paste a gtdcraft:///setup-sync link").desired_width(f32::INFINITY)).changed();
        });
        // a pasted pairing link fills in both fields
        if let Some((server, token)) = parse_pair_link(&st.sync_url) {
            (st.sync_url, st.sync_token) = (server, token);
            ch = true;
        }
        ui.horizontal(|ui| {
            ui.add_sized([70.0, 20.0], egui::Label::new("Token"));
            ch |= ui.add(crate::nativetext::Edit::singleline(&mut st.sync_token).password(true).desired_width(f32::INFINITY)).changed();
        });
        ui.horizontal(|ui| {
            let busy = self.sync_rx.is_some();
            if ui.add_enabled(!busy && !self.store.settings.sync_url.trim().is_empty(), egui::Button::new("Sync Now")).clicked() {
                self.sync_due = Some(0.0);
            }
            if busy {
                ui.spinner();
            }
            if !self.sync_msg.is_empty() {
                ui.label(egui::RichText::new(&self.sync_msg).color(crate::th().dim).size(12.0));
            }
        });
        // which database this device syncs, and whether syncing waits for the person
        let short = |id: &str| id.chars().take(8).collect::<String>();
        if self.sync_paused {
            ui.label(egui::RichText::new(format!("Paused: this database ({}) isn't the one this device syncs ({}).", if self.store.db_id.is_empty() { "from before".into() } else { short(&self.store.db_id) }, short(&linked_db()))).color(crate::hue("orange")).size(12.0));
            if ui.button("Show the Choices").clicked() {
                self.sync_banner_hidden = false;
            }
        } else if !self.store.db_id.is_empty() && !self.store.settings.sync_url.trim().is_empty() {
            ui.label(egui::RichText::new(format!("Database {} — the server keeps one copy per database, so opening another never mixes them.", short(&self.store.db_id))).color(crate::th().dim).size(12.0));
        }
        // Send to Agent: an opt-in feature of the sync server (agent.rs). On the Mac running it, the switch; anywhere,
        // what the server offers.
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Agents").strong());
        #[cfg(target_os = "macos")]
        if local_server_token().is_some() {
            let data = crate::dbfile::app_dir().join("sync-server");
            let file = crate::agent::optin_file(&data);
            let mut on = file.exists();
            let found = crate::acpfind::find();
            if ui.checkbox(&mut on, "Let this Mac's sync server run agents for Send to Agent").changed() {
                let _ = if on { std::fs::write(&file, "on\n") } else { std::fs::remove_file(&file) };
                self.agentui_refresh_soon();
            }
            ui.label(egui::RichText::new(if found.is_empty() {
                "Needs an Agent Client Protocol agent installed on this Mac (Claude Code, Codex, Gemini CLI, goose…); none found.".to_string()
            } else {
                format!("Agents here: {}. Each works on a staged copy of your database with GTDCraft's MCP server and its own connectors; reading is always allowed, and its changes apply only when you approve them. A thread lasts two hours.", found.iter().map(|f| f.name).collect::<Vec<_>>().join(", "))
            }).color(crate::th().dim).size(12.0));
        }
        match &self.agentui.agents {
            Some(a) if !a.is_empty() => ui.label(egui::RichText::new(format!("Your sync server offers: {}. Use Send to Agent… on an action.", a.join(", "))).color(crate::th().dim).size(12.0)),
            Some(_) => ui.label(egui::RichText::new("Your sync server doesn't offer agents (turn them on on the Mac that runs it).").color(crate::th().dim).size(12.0)),
            None => ui.label(egui::RichText::new("Checking your sync server for agents…").color(crate::th().dim).size(12.0)),
        };
        // this Mac runs the server (`local.gtdcraft.syncserver`): one click fills in its address and token
        #[cfg(target_os = "macos")]
        if let Some(token) = local_server_token() {
            let st = &mut self.store.settings;
            if (st.sync_url.trim().is_empty() || st.sync_token.trim().is_empty()) && ui.button("Use This Mac's Server").clicked() {
                st.sync_url = format!("http://{}:8765", mac_host());
                st.sync_token = token;
                ch = true;
            }
        }
        // pair a phone: a QR code the iPhone's Camera opens in GTDCraft (gtdcraft:///setup-sync?server=…&token=…)
        #[cfg(not(target_os = "ios"))]
        {
            let st = &self.store.settings;
            if !st.sync_url.trim().is_empty() && !st.sync_token.trim().is_empty() {
                let link = pair_link(&phone_server(&st.sync_url), &st.sync_token, &self.store.db_id);
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Pair a phone").strong());
                ui.label(egui::RichText::new("Point your iPhone's Camera at this code and tap Open in GTDCraft. It carries the server address and token, so keep it to yourself.").color(crate::th().dim).size(12.0));
                qr(ui, &link, 4.0);
                if ui.button("Copy Link").on_hover_text("For pasting into Settings › Sync on another device").clicked() {
                    ui.ctx().copy_text(link);
                    self.sync_msg = "Link copied".into();
                }
            }
        }
        if ch {
            self.sync_due = Some(self.time + 2.0);
        }
        ch
    }

    /// iPhone/iPad: a setup-sync or add link the Camera / Share extension / widget opened in the app.
    /// setup-sync: `gtdcraft:///setup-sync` sets up sync (see `pair_link`).
    /// report-bug: `gtdcraft:///report-bug` opens Report a Bug.
/// add: `gtdcraft:///add?name=…` creates an inbox item (Quick Entry from a Lock Screen widget or Share).
    pub(crate) fn take_opened_link(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        #[cfg(target_os = "ios")]
        {
            open_url::install();
            let opened = open_url::OPENED.lock().ok().and_then(|mut o| o.take());
            if let Some(url) = &opened {
                if let Some(link) = parse_pair_link(url) {
                    self.pending_sync_link = Some(link);
                    self.pending_sync_db = pair_link_db(url);
                } else if is_bug_link(url) {
                    self.report_bug();
                } else if let Some(add) = parse_add_link(url) {
                    let id = self.store.add_task(&add.name, None, None);
                    if let Some(t) = self.store.task_mut(id) {
                        if !add.note.is_empty() {
                            t.note = add.note;
                        }
                        if !add.ref_url.is_empty() {
                            t.refs.push(crate::refs::Ref { url: add.ref_url, title: add.ref_title, state: String::new() });
                        }
                    }
                    self.touch();
                }
            }
        }
        let Some((server, token)) = self.pending_sync_link.clone() else { return };
        let mut answer = None;
        egui::Modal::new(egui::Id::new("setup-sync")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(egui::RichText::new("Replace GTDCraft Settings?").strong().size(15.0));
            ui.add_space(4.0);
            ui.label("You have clicked on a GTDCraft settings link.  Replace your GTDCraft synchronization settings?");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(false);
                }
                if ui.add(egui::Button::new(egui::RichText::new("Replace Settings").color(crate::th().on_accent)).fill(crate::th().accent)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    answer = Some(true);
                }
            });
        });
        match answer {
            Some(true) => {
                self.pending_sync_link = None;
                self.sync_msg = format!("Linked to {server}");
                (self.store.settings.sync_url, self.store.settings.sync_token) = (server, token);
                // the Mac's database: an empty device joins it; one with its own data waits (the banner offers both)
                if let Some(db) = self.pending_sync_db.take().filter(|d| !d.is_empty()) {
                    if is_empty_store(&self.store) {
                        self.store.db_id = db.clone();
                    }
                    set_linked_db(&db);
                }
                self.sync_due = Some(0.0);
                self.touch();
            }
            Some(false) => self.pending_sync_link = None,
            None => {}
        }
    }
}

/// iPhone/iPad: hand the sync server's URL and token to the widgets and the Share extension through the shared App
/// Group (`GTDAppGroup` in Info.plist, from `GTD_APP_GROUP` in ios/Config), whenever they change — so those don't need them typed in again.
#[cfg(target_os = "ios")]
pub fn share_connection(url: &str, token: &str, db: &str) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    static LAST: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);
    let now = (url.trim().to_string(), format!("{}\n{db}", token.trim()));
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_ref() == Some(&now) {
        return;
    }
    unsafe {
        // the App Group from Info.plist (GTDAppGroup, set from GTD_APP_GROUP in ios/Config)
        let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
        let group: Option<Retained<AnyObject>> = msg_send![&*bundle, objectForInfoDictionaryKey: &*NSString::from_str("GTDAppGroup")];
        let Some(group) = group else { return };
        let d: Option<Retained<AnyObject>> = msg_send![msg_send![class!(NSUserDefaults), alloc], initWithSuiteName: &*group];
        let Some(d) = d else { return };
        let _: () = msg_send![&*d, setObject: &*NSString::from_str(&now.0), forKey: &*NSString::from_str("sync_url")];
        let _: () = msg_send![&*d, setObject: &*NSString::from_str(token.trim()), forKey: &*NSString::from_str("sync_token")];
        // which database the widgets and Share extension use on the server (one copy per database)
        let _: () = msg_send![&*d, setObject: &*NSString::from_str(db), forKey: &*NSString::from_str("sync_db")];
    }
    *last = Some(now);
}

/// The link a phone opens to set up sync: `gtdcraft:///setup-sync?server=<url>&token=<secret>`.
#[cfg(not(target_os = "ios"))]
pub fn pair_link(server: &str, token: &str, db: &str) -> String {
    let base = format!("gtdcraft:///setup-sync?server={}&token={}", pct(server.trim()), pct(token.trim()));
    if db.is_empty() { base } else { format!("{base}&db={}", pct(db)) }
}

/// The database identity in a pairing link, if it carries one.
#[allow(dead_code)] // used on iOS
pub fn pair_link_db(s: &str) -> Option<String> {
    let q = s.trim().split_once('?')?.1;
    q.split('&').find_map(|kv| kv.strip_prefix("db=")).map(unpct)
}

/// Parsed `gtdcraft:///add?name=…&note=…&ref_url=…&ref_title=…` link.
#[allow(dead_code)] // used on iOS
pub struct AddLink {
    pub name: String,
    pub note: String,
    pub ref_url: String,
    pub ref_title: String,
}

/// Parse a `gtdcraft:///add` link into its fields.
#[allow(dead_code)] // used on iOS
pub fn parse_add_link(s: &str) -> Option<AddLink> {
    let s = s.trim();
    let q = s.strip_prefix("gtdcraft:///add?").or_else(|| s.strip_prefix("gtdcraft://add?"))?;
    let get = |k: &str| q.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('=')).map(unpct).unwrap_or_default();
    let name = get("name");
    let ref_url = get("ref_url");
    if name.is_empty() && ref_url.is_empty() {
        return None;
    }
    Some(AddLink { name, note: get("note"), ref_url, ref_title: get("ref_title") })
}

/// `gtdcraft:///report-bug`: the path must match exactly.
#[allow(dead_code)] // used on iOS
pub fn is_bug_link(s: &str) -> bool {
    matches!(s.trim().trim_end_matches('/'), "gtdcraft:///report-bug" | "gtdcraft://report-bug")
}

/// The server and token in a pairing link, if `s` is one.
pub fn parse_pair_link(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let q = s.strip_prefix("gtdcraft:///setup-sync?").or_else(|| s.strip_prefix("gtdcraft://setup-sync?"))?;
    let get = |k: &str| q.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('=')).map(unpct);
    let (server, token) = (get("server")?, get("token")?);
    (server.starts_with("http") && !token.is_empty()).then_some((server, token))
}

#[cfg(not(target_os = "ios"))]
/// The address a phone should use for the server this device syncs with: "localhost" means this Mac, which other
/// devices reach by its network name.
pub fn phone_server(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    for local in ["localhost", "127.0.0.1", "0.0.0.0"] {
        if url.contains(&format!("//{local}")) {
            return url.replacen(local, &mac_host(), 1);
        }
    }
    url.to_string()
}

#[cfg(not(target_os = "ios"))]
/// This Mac's Bonjour name ("my-mac.local").
fn mac_host() -> String {
    let name = std::process::Command::new("scutil").args(["--get", "LocalHostName"]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    format!("{}.local", name.unwrap_or_else(|| "localhost".into()))
}

/// The token of the sync server this Mac runs from launchd, if it does.
#[cfg(target_os = "macos")]
fn local_server_token() -> Option<String> {
    let plist = std::env::var_os("HOME").map(PathBuf::from)?.join("Library/LaunchAgents/local.gtdcraft.syncserver.plist");
    let text = String::from_utf8(std::process::Command::new("plutil").args(["-convert", "json", "-o", "-"]).arg(&plist).output().ok()?.stdout).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let args: Vec<&str> = v["ProgramArguments"].as_array()?.iter().filter_map(|a| a.as_str()).collect();
    args.windows(2).find(|w| w[0] == "--token").map(|w| w[1].to_string())
}

#[cfg(not(target_os = "ios"))]
fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn unpct(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Draws `text` as a QR code: dark modules on white with the standard 4-module quiet zone.
#[cfg(not(target_os = "ios"))]
fn qr(ui: &mut eframe::egui::Ui, text: &str, module: f32) {
    use eframe::egui;
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return };
    let n = code.width();
    let side = (n + 8) as f32 * module;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 6.0, egui::Color32::WHITE);
    for y in 0..n {
        for x in 0..n {
            if code[(x, y)] == qrcode::Color::Dark {
                let min = rect.min + egui::vec2((x + 4) as f32, (y + 4) as f32) * module;
                p.rect_filled(egui::Rect::from_min_size(min, egui::vec2(module, module)), 0.0, egui::Color32::BLACK);
            }
        }
    }
}

/// iOS hands opened `gtdcraft://` links to the app delegate's `application:openURL:options:`. winit sets no delegate
/// ("to allow the user to do so themselves"), so GTDCraft sets one that only records the link for the next frame.
#[cfg(target_os = "ios")]
mod open_url {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{class, define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::NSString;
    use std::sync::Mutex;

    pub static OPENED: Mutex<Option<String>> = Mutex::new(None);

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; Delegate has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "GTDCraftAppDelegate"]
        struct Delegate;

        impl Delegate {
            #[unsafe(method(application:openURL:options:))]
            fn open_url(&self, _app: &AnyObject, url: &AnyObject, _options: &AnyObject) -> Bool {
                let s: Option<Retained<NSString>> = unsafe { msg_send![url, absoluteString] };
                if let (Some(s), Ok(mut o)) = (s, OPENED.lock()) {
                    *o = Some(s.to_string());
                }
                Bool::YES
            }
        }
    );

    /// Sets the delegate once (UIKit keeps it weakly, so it lives for the app's lifetime).
    pub fn install() {
        static DONE: std::sync::Once = std::sync::Once::new();
        let Some(mtm) = MainThreadMarker::new() else { return };
        DONE.call_once(|| {
            let d: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(mtm), init] };
            let app: *mut AnyObject = unsafe { msg_send![class!(UIApplication), sharedApplication] };
            let _: () = unsafe { msg_send![app, setDelegate: &*d] };
            std::mem::forget(d);
            // `--open-url <link>`: hand a link to the delegate the way iOS does after "Open in GTDCraft?" (for tests)
            let args: Vec<String> = std::env::args().collect();
            if let Some(link) = args.iter().position(|a| a == "--open-url").and_then(|i| args.get(i + 1)) {
                let url: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSURL), URLWithString: &*NSString::from_str(link)] };
                if let Some(url) = url {
                    let delegate: *mut AnyObject = unsafe { msg_send![app, delegate] };
                    let opts: Retained<AnyObject> = unsafe { msg_send![class!(NSDictionary), dictionary] };
                    let _: Bool = unsafe { msg_send![delegate, application: app, openURL: &*url, options: &*opts] };
                }
            }
        });
    }
}

#[cfg(test)]
mod pair_tests {
    use super::*;

    #[test]
    fn pairing_link_round_trips() {
        let link = pair_link("http://my-mac.lan:8765", "s3cr3t/+&=x", "");
        assert!(link.starts_with("gtdcraft:///setup-sync?server=http%3A%2F%2Fmy-mac.lan%3A8765&token="));
        assert_eq!(parse_pair_link(&link), Some(("http://my-mac.lan:8765".into(), "s3cr3t/+&=x".into())));
        assert_eq!(parse_pair_link("http://my-mac.lan:8765"), None);
        let with_db = pair_link("http://my-mac.lan:8765", "t", "19a2b3c4d5e6");
        assert_eq!(pair_link_db(&with_db).as_deref(), Some("19a2b3c4d5e6"));
        assert_eq!(parse_pair_link(&with_db), Some(("http://my-mac.lan:8765".into(), "t".into())));
        assert_eq!(pair_link_db(&link), None);
        assert_eq!(parse_pair_link("gtdcraft:///setup-sync?server=ftp%3A%2F%2Fx&token=t"), None);
        assert_eq!(parse_pair_link("gtdcraft://setup-sync?server=http%3A%2F%2Fx&token=t"), Some(("http://x".into(), "t".into())));
    }

    #[test]
    fn phones_get_the_macs_name_not_localhost() {
        assert_eq!(phone_server("http://my-mac.lan:8765/"), "http://my-mac.lan:8765");
        assert!(!phone_server("http://127.0.0.1:8765").contains("127.0.0.1"));
    }

    #[test]
    fn add_link_parses() {
        let a = parse_add_link("gtdcraft:///add?name=Buy%20milk&note=2%25%20fat").unwrap();
        assert_eq!(a.name, "Buy milk");
        assert_eq!(a.note, "2% fat");
        assert!(a.ref_url.is_empty());
        let b = parse_add_link("gtdcraft:///add?ref_url=https%3A%2F%2Fexample.com&ref_title=Example").unwrap();
        assert!(b.name.is_empty());
        assert_eq!(b.ref_url, "https://example.com");
        assert_eq!(b.ref_title, "Example");
        // no name and no ref = None
        assert!(parse_add_link("gtdcraft:///add?note=just+a+note").is_none());
        // also works with double-slash
        assert!(parse_add_link("gtdcraft://add?name=test").is_some());
        assert!(is_bug_link("gtdcraft:///report-bug") && is_bug_link("gtdcraft://report-bug/"));
        assert!(!is_bug_link("gtdcraft:///report-bugs") && !is_bug_link("gtdcraft:///add?name=report-bug"));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn the_server_mints_ids_no_device_can() {
        let biggest_device = ((1u64 << 20) - 1) << 32;
        assert!(SERVER_ID_BASE > biggest_device + u32::MAX as u64);
        assert!(device_base() < SERVER_ID_BASE);
    }

    #[test]
    fn the_newer_layout_wins_and_other_settings_stay() {
        let mut phone = Store::default();
        phone.settings.perspectives = crate::model::default_perspectives();
        let mut mac = phone.clone();
        mac.settings.sync_url = "http://mac.lan:8765".into();
        phone.settings.perspectives.swap(0, 1);
        phone.settings.inspector_order = vec!["note".into(), "status".into()];
        phone.settings.layout_modified = Some(crate::model::now());
        phone.settings.sync_url = "http://other".into();
        let m = merge(&phone, &mac);
        assert_eq!(m.settings.perspectives[0].key, phone.settings.perspectives[0].key);
        assert_eq!(m.settings.inspector_order, phone.settings.inspector_order);
        assert_eq!(m.settings.sync_url, "http://mac.lan:8765", "device settings stay");
        // connections travel the same way
        let mut phone2 = phone.clone();
        phone2.settings.linear_key = "lin_api_x".into();
        phone2.settings.connections_modified = Some(crate::model::now());
        assert_eq!(merge(&phone2, &mac).settings.linear_key, "lin_api_x");
        assert_eq!(merge(&phone2, &mac).settings.sync_url, "http://mac.lan:8765");
        // and an older layout doesn't overwrite a newer one
        let mut stale = mac.clone();
        stale.settings.layout_modified = Some(crate::model::now() - chrono::Duration::days(1));
        assert_eq!(merge(&stale, &m).settings.inspector_order, phone.settings.inspector_order);
    }
}
