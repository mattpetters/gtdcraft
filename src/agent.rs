//! Send to Agent: an opt-in feature of the sync server (`--agents`, or Settings › Sync › Agents on the Mac running it).
//! A device asks the server to start a thread for an action; the server runs an Agent Client Protocol agent (Claude
//! Code, Codex, Gemini CLI… — whichever are installed there, `acpfind`) with the action's context and GTDCraft's own MCP
//! server attached, plus whatever connectors that agent already has. The agent works on a **staged copy** of the
//! database: it reads everything, and every change it makes through the gtd tools (one by one, or as a plan with
//! `gtd_batch`) lands in the copy and is listed. Nothing reaches the real database until the person approves; then the
//! copy is merged in with sync's own merge (newest wins, deletions travel), and devices pick it up at their next sync.
//! The agent can ask questions (it ends its turn; the person replies in the thread) and ask permission for its own
//! tools (the person chooses). A thread lasts two hours; after that it can continue as a new thread from a summary.
use crate::model::{now, Dt, Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

/// How long a thread may run before it must continue as a new one (the person: "a 2hr timeout for a thread").
pub const THREAD_LIFETIME_SECS: i64 = 2 * 3600;
/// How long an agent may take to answer its start-up handshake (a first `npx` run downloads the adapter).
pub const STARTUP_SECS: u64 = 180;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// the agent is working
    Working,
    /// its turn ended: it answered or asked something; the person can reply or finish
    Waiting,
    /// it asks to use one of its tools
    Permission,
    /// it staged changes: approve or reject
    Review,
    Done,
    Failed,
    /// past its two hours; continue in a new thread from a summary
    Expired,
}

impl Status {
    pub fn active(self) -> bool {
        matches!(self, Status::Working | Status::Waiting | Status::Permission | Status::Review)
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Working => "Working",
            Status::Waiting => "Waiting for you",
            Status::Permission => "Asking permission",
            Status::Review => "Changes to review",
            Status::Done => "Done",
            Status::Failed => "Failed",
            Status::Expired => "Timed out",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Msg {
    /// "you", "agent" or "note" (GTDCraft's own lines: applied, discarded, timed out…)
    pub role: String,
    pub text: String,
    pub at: Dt,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Thread {
    pub id: String,
    pub task: u64,
    pub task_name: String,
    pub agent: String,
    pub status: Status,
    pub messages: Vec<Msg>,
    /// what it's doing now (the latest tool call's title), and how many steps so far
    #[serde(default)]
    pub activity: String,
    #[serde(default)]
    pub steps: u32,
    /// the staged changes, one line each
    #[serde(default)]
    pub changes: Vec<String>,
    /// a permission request: what for, and the options (id, name)
    #[serde(default)]
    pub permission: Option<(String, Vec<(String, String)>)>,
    pub created: Dt,
    pub updated: Dt,
    /// the thread this one continues
    #[serde(default)]
    pub from: Option<String>,
    /// the database it works on (its identity; the sync server keeps one copy per database)
    #[serde(default)]
    pub db: String,
}

impl Thread {
    pub fn expired_at(&self, t: Dt) -> bool {
        (t - self.created).num_seconds() >= THREAD_LIFETIME_SECS
    }

    /// What a new thread needs to carry on: the request, what the agent said last, and what happened to its changes.
    pub fn summary(&self) -> String {
        let first = self.messages.iter().find(|m| m.role == "you").map(|m| m.text.as_str()).unwrap_or("");
        let mut out = format!("Earlier thread with {} about “{}”.\nThe request: {first}\n", self.agent, self.task_name);
        let recent: Vec<&Msg> = self.messages.iter().filter(|m| m.role != "note").rev().take(4).collect();
        for m in recent.into_iter().rev() {
            let text: String = m.text.chars().take(1200).collect();
            out.push_str(&format!("{}: {text}\n", if m.role == "you" { "The person" } else { "You" }));
        }
        if !self.changes.is_empty() {
            out.push_str(&format!("Changes staged then ({}): {}\n", self.status.label(), self.changes.join("; ")));
        }
        out
    }
}

/// One staged change, for the person to read: "Add task “Buy paint”", "Update task 42: due, flagged".
pub fn describe_change(line: &Value) -> String {
    let tool = line["tool"].as_str().unwrap_or("");
    let a = &line["args"];
    let name = a["name"].as_str().map(|n| format!(" “{n}”")).unwrap_or_default();
    let id = a["id"].as_u64().or(a["task_id"].as_u64()).map(|i| format!(" {i}")).unwrap_or_default();
    let fields = || a.as_object().map(|o| o.keys().filter(|k| *k != "id").cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();
    match tool {
        "gtd_add_task" => format!("Add task{name}"),
        "gtd_update_task" => format!("Update task{id}: {}", fields()),
        "gtd_set_task_status" => format!("Mark task{id} {}", a["status"].as_str().unwrap_or("")),
        "gtd_delete_task" => format!("Delete task{id}"),
        "gtd_add_project" => format!("Add project{name}"),
        "gtd_update_project" => format!("Update project{id}: {}", fields()),
        "gtd_create_perspective" => format!("Create perspective{name}"),
        "gtd_review" => format!("Mark project{} reviewed", a["mark_reviewed_id"].as_u64().map(|i| format!(" {i}")).unwrap_or_default()),
        "gtd_link_linear" => format!("Link Linear issue {}", a["issue"].as_str().unwrap_or("")),
        other => format!("{other} {}", a),
    }
}

// ---------- prompt templates ----------

/// A Send to Agent template: a few labelled fields with defaults, written front-matter style — `key: value` lines
/// (an optional `---` above and below; a line without `key:` continues the field above). `project:` and `tag:` also
/// choose it for actions in that project or with that tag; `followUp:` is what the agent does last.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    pub text: String,
}

/// A starter example, ready to use (Settings › Agent edits it).
pub fn default_templates() -> Vec<Template> {
    vec![Template {
        name: "Feature or fix".into(),
        text: "task: feature or bug fix\ntag: dev\nwhere: the repository this action belongs to\nacceptance: tests pass and the change is described in the summary\nfollowUp: Summarise what changed\n".into(),
    }]
}

/// A template's fields in order: (label, default).
pub fn parse_fields(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim() == "---" {
            continue;
        }
        let key = t.split_once(':').map(|(k, v)| (k.trim(), v.trim())).filter(|(k, _)| !k.is_empty() && !k.contains(' ') && !line.starts_with(char::is_whitespace));
        match key {
            Some((k, v)) => out.push((k.to_string(), v.to_string())),
            None if !t.trim().is_empty() => {
                if let Some(last) = out.last_mut() {
                    if !last.1.is_empty() {
                        last.1.push('\n');
                    }
                    last.1.push_str(t.trim());
                }
            }
            None => {}
        }
    }
    out
}

fn is_follow_up(k: &str) -> bool {
    matches!(k.to_lowercase().replace(['_', '-', ' '], "").as_str(), "followup")
}

/// Whether a template is the one for an action: its `project:` names the action's project (or a folder above it), or
/// its `tag:` / `tags:` names one of the action's tags (ignoring case).
pub fn template_matches(fields: &[(String, String)], project_path: &str, tags: &[String]) -> bool {
    let low = |s: &str| s.trim().to_lowercase();
    fields.iter().any(|(k, v)| match low(k).as_str() {
        "project" => !v.trim().is_empty() && crate::model::path_parts(project_path).iter().any(|p| low(p) == low(v)),
        "tag" | "tags" => v.split(',').any(|t| !t.trim().is_empty() && tags.iter().any(|g| crate::model::path_parts(g).iter().any(|p| low(p) == low(t)))),
        _ => false,
    })
}

/// The fields as the prompt gives them: each labelled line, then the follow-up as the last step.
/// The action's values a template field can use as `{name}`: task, project, tags, note, defer, planned, due,
/// estimate, link (its first reference).
pub fn template_env(store: &Store, task: u64) -> Vec<(&'static str, String)> {
    let Some(t) = store.task(task) else { return vec![] };
    let d = |x: Option<crate::model::Dt>| x.map(|v| v.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
    vec![
        ("task", t.name.clone()),
        ("project", t.project.map(|p| store.project_path(p)).unwrap_or_default()),
        ("tags", t.tags.iter().map(|g| store.tag_path(*g)).collect::<Vec<_>>().join(", ")),
        ("note", t.note.trim().to_string()),
        ("defer", d(t.defer)),
        ("planned", d(t.planned)),
        ("due", d(t.due)),
        ("estimate", t.duration_min.map(|m| format!("{m} minutes")).unwrap_or_default()),
        ("link", t.refs.first().map(|r| r.url.clone()).unwrap_or_default()),
    ]
}

/// A field's `{name}`s filled from the action. A name that isn't there is an error, not something sent to the
/// agent as-is, so a typo shows up before anything is sent. `{{` and `}}` are literal braces.
pub fn fill_vars(text: &str, env: &[(&str, String)]) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push(tail.as_bytes()[0] as char);
            rest = &tail[2..];
        } else if let Some(stripped) = tail.strip_prefix('{') {
            let end = stripped.find('}').ok_or_else(|| format!("A “{{” isn't closed in: {text}"))?;
            let name = stripped[..end].trim();
            let v = env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()).ok_or_else(|| {
                format!("The variable '{name}' is not defined (use {}).", env.iter().map(|(k, _)| format!("{{{k}}}")).collect::<Vec<_>>().join(", "))
            })?;
            out.push_str(&v);
            rest = &stripped[end + 1..];
        } else {
            out.push('}');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    Ok(out)
}

pub fn fields_block(fields: &[(String, String)]) -> String {
    let filled: Vec<&(String, String)> = fields.iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    if filled.is_empty() {
        return String::new();
    }
    let mut out = String::from("From the person's template:\n");
    for (k, v) in filled.iter().filter(|(k, _)| !is_follow_up(k)) {
        out.push_str(&format!("- {k}: {}\n", v.trim()));
    }
    for (_, v) in filled.iter().filter(|(k, _)| is_follow_up(k)) {
        out.push_str(&format!("Last, once everything else is done: {}\n", v.trim()));
    }
    out
}

/// The opening prompt: the action, its context, the request, and how the gtd tools behave here.
/// How a request's words map to GTDCraft's fields: the date's kind (planned, due or defer), tags and projects,
/// repeats, and durations, which aren't dates.
pub const FIELD_RULES: &str = "Choosing fields: use planned if the action will be performed 'at' or 'on' a date, due if it must be done 'by' or 'before' it, and defer if it can't begin 'until' or 'after' it. A duration ('for 40 minutes') is an estimate, not a date. Include every tag name the person mentions, and put an action in the most likely project. Describe repeats with specific days when given ('weekly every Tuesday', 'monthly on the 17th').";

/// The Review Guru's grooming pass: the default request for a whole-database run (task 0). The person can edit it
/// before sending.
pub const GURU_BRIEF: &str = "Do an assisted grooming pass over my whole GTD database. Read everything first (gtd_overview, then gtd_list_tasks and gtd_list_projects), then work through, staging changes for me to approve:\n\
- Clarify vague actions: rewrite unclear names into a concrete next action.\n\
- Suggest converting an action that is really several steps into a project with sub-actions.\n\
- Find orphans (no project) and loose items, and put each in its most likely project; add missing tags.\n\
- Flag stalled projects (no available next action, or untouched a long time): for each, ask me whether to complete, drop, or add a next action — don't complete or drop anything without my say-so.\n\
- Suggest a duration estimate where one is missing, and ask about start/defer, planned and due dates, then set the ones I confirm (see the field rules below).\n\
- Tags audit: list tags that are unused, redundant, or unclear, and suggest merging or removing them.\n\
- Projects audit: for each project say whether it should stay a parallel project, become a single-action list, or just be one action.\n\
- Deep links: for actions or projects linked to Linear or GitHub (or whose siblings are), check whether a link can be set up, and link them with gtd_link_linear or by adding the reference.\n\
Ask me one question at a time when you need a decision, and end with a short summary of everything you staged.";

pub fn prompt(store: &Store, task: u64, request: &str, earlier: Option<&str>, fields: &[(String, String)]) -> String {
    let t = store.task(task);
    let mut ctx = String::new();
    if let Some(t) = t {
        ctx.push_str(&format!("Action (id {}): {}\n", t.id, t.name));
        if let Some(p) = t.project.and_then(|p| store.project(p)) {
            ctx.push_str(&format!("Project: {}\n", store.project_path(p.id)));
        }
        let tags: Vec<String> = t.tags.iter().map(|g| store.tag_path(*g)).collect();
        if !tags.is_empty() {
            ctx.push_str(&format!("Tags: {}\n", tags.join(", ")));
        }
        for (k, v) in [("Defer", t.defer), ("Planned", t.planned), ("Due", t.due)] {
            if let Some(v) = v {
                ctx.push_str(&format!("{k}: {}\n", v.format("%Y-%m-%d %H:%M")));
            }
        }
        if let Some(m) = t.duration_min {
            ctx.push_str(&format!("Estimated: {m} minutes\n"));
        }
        if t.flagged {
            ctx.push_str("Flagged\n");
        }
        if !t.note.trim().is_empty() {
            ctx.push_str(&format!("Note:\n{}\n", t.note.trim()));
        }
        for r in &t.refs {
            ctx.push_str(&format!("Reference: {} {}\n", r.url, r.title));
        }
        for a in &t.attachments {
            ctx.push_str(&format!("Attachment: {a}\n"));
        }
        let kids: Vec<&str> = store.tasks.iter().filter(|c| c.parent == Some(t.id)).map(|c| c.name.as_str()).collect();
        if !kids.is_empty() {
            ctx.push_str(&format!("Sub-actions: {}\n", kids.join("; ")));
        }
    }
    let intro = if t.is_some() {
        "You're helping with an action in GTDCraft, the person's task manager."
    } else {
        "You're the Review Guru for GTDCraft, the person's task manager: an assisted grooming pass over their whole database."
    };
    format!(
        "{intro}\n\n{ctx}\n{}{}The request: {request}\n\n\
         You have GTDCraft's MCP server \"gtdcraft\" (gtd_* tools) for the whole database — start with gtd_overview or \
         gtd_list_tasks. Reading is always fine. Changes you make with the gtd tools (one at a time, or several at once \
         with gtd_batch) are staged: they apply to the person's database only if they approve them afterwards, so make \
         every change you intend through the gtd tools, then finish with a short summary of what you did and staged. \
         If something is unclear, ask one short question and end your turn; the person will answer here. You may also use \
         your other tools and connectors.\n\n{FIELD_RULES}",
        earlier.map(|e| format!("{e}\n")).unwrap_or_default(),
        fields_block(fields).lines().map(|l| format!("{l}\n")).collect::<String>() + if fields_block(fields).is_empty() { "" } else { "\n" }
    )
}

// ---------- the server side ----------

/// What the person can send a running thread.
#[derive(Debug)]
pub enum Cmd {
    Reply(String),
    Permission(String),
    Approve,
    Reject,
    Finish,
    Cancel,
}

enum Ev {
    Line(String),
    Closed,
    Cmd(Cmd),
}

pub struct Hub {
    data: PathBuf,
    threads: Mutex<HashMap<String, Thread>>,
    inbox: Mutex<HashMap<String, mpsc::Sender<Ev>>>,
    /// the server's flag (`--agents`); otherwise the opt-in file the Mac's Settings writes
    flag: bool,
}

static HUB: OnceLock<Arc<Hub>> = OnceLock::new();

/// The opt-in file in the server's data folder (Settings › Sync › Agents on the Mac running the server).
pub fn optin_file(data: &Path) -> PathBuf {
    data.join("agents-enabled")
}

pub fn hub(data: &Path, flag: bool) -> Arc<Hub> {
    HUB.get_or_init(|| {
        let mut threads: HashMap<String, Thread> = std::fs::read_to_string(data.join("agent-threads.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // a restart ends what was running (its agent process is gone)
        for t in threads.values_mut().filter(|t| matches!(t.status, Status::Working | Status::Permission | Status::Waiting)) {
            t.status = Status::Expired;
            t.messages.push(Msg { role: "note".into(), text: "The sync server restarted; continue in a new thread.".into(), at: now() });
        }
        let hub = Arc::new(Hub { data: data.to_path_buf(), threads: Mutex::new(threads), inbox: Mutex::new(HashMap::new()), flag });
        hub.save(); // the file says so too (it still read "working")
        hub
    })
    .clone()
}

impl Hub {
    pub fn enabled(&self) -> bool {
        self.flag || optin_file(&self.data).exists()
    }

    fn save(&self) {
        if let Ok(t) = self.threads.lock() {
            let _ = std::fs::write(self.data.join("agent-threads.json"), serde_json::to_string(&*t).unwrap_or_default());
        }
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Thread)) {
        if let Ok(mut t) = self.threads.lock() {
            if let Some(th) = t.get_mut(id) {
                f(th);
                th.updated = now();
            }
        }
        self.save();
    }

    fn get(&self, id: &str) -> Option<Thread> {
        self.threads.lock().ok()?.get(id).cloned()
    }

    fn work_dir(&self, id: &str) -> PathBuf {
        self.data.join("agent").join(id)
    }

    /// Starts a thread for an action: the agent named (one found on this machine), with the request.
    pub fn start(self: &Arc<Self>, task: u64, agent: &str, request: &str, from: Option<&str>, db: &str, fields: &[(String, String)]) -> Result<Thread, String> {
        let dbp = crate::sync::db_path(&self.data, db);
        let found = crate::acpfind::find();
        let a = found.iter().find(|f| f.name == agent).or(found.first()).ok_or("No ACP agent is installed on the sync server's machine.")?;
        let store = Store::load(&dbp).map_err(|e| format!("couldn't read the database: {e}"))?;
        // task 0 = a whole-database run (the Review Guru), anchored to no single action.
        let task_name = if task == 0 {
            "Review Guru".to_string()
        } else {
            store.task(task).map(|t| t.name.clone()).ok_or("That action isn't on the sync server yet — sync first.")?
        };
        let earlier = from.and_then(|f| self.get(f)).map(|t| t.summary());
        let id = format!("{}-{}", now().format("%Y%m%d%H%M%S"), task % 100_000);
        let dir = self.work_dir(&id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // the staged copy the agent works on
        std::fs::copy(&dbp, dir.join("staged.jsonl")).map_err(|e| format!("couldn't stage the database: {e}"))?;
        let t = now();
        let th = Thread {
            id: id.clone(),
            task,
            task_name,
            agent: a.name.to_string(),
            status: Status::Working,
            messages: vec![Msg { role: "you".into(), text: request.to_string(), at: t }],
            activity: "Starting…".into(),
            steps: 0,
            changes: vec![],
            permission: None,
            created: t,
            updated: t,
            from: from.map(String::from),
            db: db.to_string(),
        };
        self.threads.lock().map_err(|e| e.to_string())?.insert(id.clone(), th.clone());
        self.save();
        let (tx, rx) = mpsc::channel();
        self.inbox.lock().map_err(|e| e.to_string())?.insert(id.clone(), tx.clone());
        let prompt = prompt(&store, task, request, earlier.as_deref(), fields);
        let hub = self.clone();
        let command = a.command.clone();
        std::thread::spawn(move || {
            let r = hub.run(&id, &command, &prompt, tx, rx);
            if let Err(e) = r {
                hub.update(&id, |t| {
                    if t.status.active() {
                        t.status = Status::Failed;
                    }
                    t.messages.push(Msg { role: "note".into(), text: e, at: now() });
                });
            }
            if let Ok(mut i) = hub.inbox.lock() {
                i.remove(&id);
            }
        });
        Ok(th)
    }

    pub fn send(&self, id: &str, cmd: Cmd) -> Result<(), String> {
        let tx = self.inbox.lock().map_err(|e| e.to_string())?.get(id).cloned();
        match (tx, cmd) {
            (Some(tx), cmd) => tx.send(Ev::Cmd(cmd)).map_err(|_| "That thread has ended.".to_string()),
            // the agent process is gone but changes are staged: approving or rejecting still works
            (None, Cmd::Approve) => self.apply(id),
            (None, Cmd::Reject) => {
                self.discard(id);
                Ok(())
            }
            (None, _) => Err("That thread has ended.".into()),
        }
    }

    fn staged_changes(&self, id: &str) -> Vec<String> {
        std::fs::read_to_string(self.work_dir(id).join("changes.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .map(|v| describe_change(&v))
            .collect()
    }

    /// Approve: the staged copy merges into the real database (sync's merge), then it's done.
    fn apply(&self, id: &str) -> Result<(), String> {
        let staged = Store::load(&self.work_dir(id).join("staged.jsonl")).map_err(|e| e.to_string())?;
        let _db = crate::sync::DB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dbp = crate::sync::db_path(&self.data, &self.get(id).map(|t| t.db).unwrap_or_default());
        let real = Store::load(&dbp).map_err(|e| e.to_string())?;
        let merged = crate::sync::merge_ties(&real, &staged, false);
        merged.save(&dbp).map_err(|e| e.to_string())?;
        let n = self.staged_changes(id).len();
        self.update(id, |t| {
            t.status = Status::Done;
            t.messages.push(Msg { role: "note".into(), text: format!("Applied {n} change{} — your devices get them at their next sync.", if n == 1 { "" } else { "s" }), at: now() });
        });
        let _ = std::fs::remove_dir_all(self.work_dir(id));
        Ok(())
    }

    fn discard(&self, id: &str) {
        self.update(id, |t| {
            t.status = Status::Done;
            t.messages.push(Msg { role: "note".into(), text: "Changes discarded.".into(), at: now() });
        });
        let _ = std::fs::remove_dir_all(self.work_dir(id));
    }

    /// The agent's session: ACP JSON-RPC over the agent's stdin/stdout, turn by turn, until approved, rejected,
    /// finished, cancelled or two hours old.
    fn run(&self, id: &str, command: &str, prompt: &str, tx: mpsc::Sender<Ev>, rx: mpsc::Receiver<Ev>) -> Result<(), String> {
        use std::io::{BufRead, Write};
        let dir = self.work_dir(id);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .current_dir(&dir)
            .env("PATH", crate::acpfind::login_path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start the agent: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = std::io::BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let lines = tx.clone();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                if lines.send(Ev::Line(line)).is_err() {
                    return;
                }
            }
            let _ = lines.send(Ev::Closed);
        });
        let mut send = |v: Value| writeln!(stdin, "{v}").and_then(|_| stdin.flush()).map_err(|e| format!("The agent closed: {e}"));
        let created = self.get(id).map(|t| t.created).unwrap_or_else(now);
        let deadline = || {
            let left = THREAD_LIFETIME_SECS - (now() - created).num_seconds();
            std::time::Duration::from_secs(left.max(0) as u64)
        };
        let mut next_id = 10i64;
        let mut pending_permission: Option<Value> = None;
        let mut reply_text = String::new();
        // one exchange: waits for the response to `want`, handling updates, requests and the person's commands
        // the handshake (initialize, session/new) must answer within STARTUP_SECS; a turn, within the thread's two hours
        let startup = std::time::Instant::now() + std::time::Duration::from_secs(STARTUP_SECS);
        let turn = |want: i64, send: &mut dyn FnMut(Value) -> Result<(), String>, reply_text: &mut String, pending: &mut Option<Value>| -> Result<Value, String> {
            loop {
                let wait = if want <= 2 { startup.saturating_duration_since(std::time::Instant::now()).min(deadline()) } else { deadline() };
                let ev = rx.recv_timeout(wait).map_err(|_| if want <= 2 { format!("The agent didn't start: no answer to its {} in {} minutes. Is it signed in and working on the sync server's Mac?", if want == 1 { "handshake" } else { "session request" }, STARTUP_SECS / 60) } else { "timeout".to_string() })?;
                match ev {
                    Ev::Closed => return Err("The agent stopped.".into()),
                    Ev::Cmd(Cmd::Cancel) => return Err("cancel".into()),
                    Ev::Cmd(Cmd::Permission(opt)) => {
                        if let Some(req) = pending.take() {
                            send(json!({"jsonrpc": "2.0", "id": req["id"], "result": {"outcome": {"outcome": "selected", "optionId": opt}}}))?;
                            self.update(id, |t| {
                                t.status = Status::Working;
                                t.permission = None;
                            });
                        }
                    }
                    Ev::Cmd(_) => {} // replies and approvals wait for the turn to end
                    Ev::Line(line) => {
                        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                        if let Some(m) = msg["method"].as_str() {
                            if m == "session/update" {
                                let u = &msg["params"]["update"];
                                match u["sessionUpdate"].as_str().unwrap_or("") {
                                    "agent_message_chunk" => {
                                        if let Some(t) = u["content"]["text"].as_str() {
                                            reply_text.push_str(t);
                                        }
                                    }
                                    "tool_call" | "tool_call_update" | "plan" => {
                                        let title = u["title"].as_str().map(String::from).or_else(|| u["entries"][0]["content"].as_str().map(String::from));
                                        let changes = self.staged_changes(id);
                                        self.update(id, |t| {
                                            if u["sessionUpdate"] == "tool_call" {
                                                t.steps += 1;
                                            }
                                            if let Some(title) = title {
                                                t.activity = title;
                                            }
                                            t.changes = changes;
                                        });
                                    }
                                    _ => {}
                                }
                            } else if m == "session/request_permission" && msg.get("id").is_some() {
                                // its own tools: the person decides (GTDCraft's are staged anyway)
                                let title = msg["params"]["toolCall"]["title"].as_str().unwrap_or("a tool").to_string();
                                let opts: Vec<(String, String)> = msg["params"]["options"].as_array().into_iter().flatten().map(|o| (o["optionId"].as_str().unwrap_or("").to_string(), o["name"].as_str().unwrap_or("").to_string())).collect();
                                *pending = Some(msg.clone());
                                self.update(id, |t| {
                                    t.status = Status::Permission;
                                    t.permission = Some((title, opts));
                                });
                            } else if msg.get("id").is_some() {
                                // no file system or terminal is offered to the agent
                                send(json!({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": "not supported by GTDCraft"}}))?;
                            }
                            continue;
                        }
                        if msg["id"].as_i64() == Some(want) {
                            if let Some(e) = msg.get("error") {
                                return Err(format!("Agent error: {}", e["message"].as_str().unwrap_or("unknown")));
                            }
                            return Ok(msg["result"].clone());
                        }
                    }
                }
            }
        };
        let finish = |hub: &Hub, child: &mut std::process::Child, why: Result<(), String>| -> Result<(), String> {
            let _ = child.kill();
            let _ = child.wait();
            match why {
                Err(e) if e == "timeout" => {
                    hub.update(id, |t| {
                        t.status = if t.changes.is_empty() { Status::Expired } else { Status::Review };
                        t.messages.push(Msg { role: "note".into(), text: "Two hours are up: continue in a new thread from a summary.".into(), at: now() });
                    });
                    Ok(())
                }
                Err(e) if e == "cancel" => {
                    hub.update(id, |t| {
                        t.status = if t.changes.is_empty() { Status::Done } else { Status::Review };
                        t.messages.push(Msg { role: "note".into(), text: "Stopped.".into(), at: now() });
                    });
                    Ok(())
                }
                other => other,
            }
        };
        let r = (|| -> Result<(), String> {
            send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 1, "clientCapabilities": {}, "clientInfo": {"name": "gtdcraft", "title": "GTDCraft", "version": env!("CARGO_PKG_VERSION")}}}))?;
            turn(1, &mut send, &mut reply_text, &mut pending_permission)?;
            // GTDCraft's MCP server on the staged copy: its writes are listed in changes.jsonl and minted in the
            // server's ID range
            let mcp = json!({"name": "gtdcraft", "command": exe.display().to_string(), "args": ["--mcp", "--db", dir.join("staged.jsonl").display().to_string()],
                "env": [{"name": "GTDCRAFT_MCP_LOG", "value": dir.join("changes.jsonl").display().to_string()}, {"name": "GTDCRAFT_ID_BASE", "value": crate::sync::SERVER_ID_BASE.to_string()}]});
            send(json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": dir.display().to_string(), "mcpServers": [mcp]}}))?;
            let session = turn(2, &mut send, &mut reply_text, &mut pending_permission)?["sessionId"].as_str().map(String::from).ok_or("The agent didn't open a session.")?;
            let mut text = prompt.to_string();
            loop {
                next_id += 1;
                reply_text.clear();
                self.update(id, |t| {
                    t.status = Status::Working;
                    t.activity = "Thinking…".into();
                });
                send(json!({"jsonrpc": "2.0", "id": next_id, "method": "session/prompt", "params": {"sessionId": session, "prompt": [{"type": "text", "text": text}]}}))?;
                let res = turn(next_id, &mut send, &mut reply_text, &mut pending_permission);
                if let Err(e) = &res {
                    if e == "cancel" {
                        let _ = send(json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}));
                    }
                }
                res?;
                let changes = self.staged_changes(id);
                let said = reply_text.trim().to_string();
                self.update(id, |t| {
                    if !said.is_empty() {
                        t.messages.push(Msg { role: "agent".into(), text: said.clone(), at: now() });
                    }
                    t.status = if changes.is_empty() { Status::Waiting } else { Status::Review };
                    t.changes = changes;
                    t.activity.clear();
                });
                // the person's next move
                loop {
                    match rx.recv_timeout(deadline()).map_err(|_| "timeout".to_string())? {
                        Ev::Cmd(Cmd::Reply(r)) => {
                            self.update(id, |t| t.messages.push(Msg { role: "you".into(), text: r.clone(), at: now() }));
                            text = r;
                            break;
                        }
                        Ev::Cmd(Cmd::Approve) => return self.apply(id),
                        Ev::Cmd(Cmd::Reject) => {
                            self.discard(id);
                            return Ok(());
                        }
                        Ev::Cmd(Cmd::Finish) | Ev::Closed => {
                            self.update(id, |t| {
                                if t.changes.is_empty() {
                                    t.status = Status::Done;
                                }
                            });
                            return Ok(());
                        }
                        Ev::Cmd(Cmd::Cancel) => return Err("cancel".into()),
                        _ => {}
                    }
                }
            }
        })();
        finish(self, &mut child, r)
    }

    /// The HTTP side (under the sync server's token): /v1/agent/…
    pub fn route(self: &Arc<Self>, method: &str, path: &str, query: &str, body: &str) -> Result<String, (u16, String)> {
        let v: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(body).map_err(|e| (400, e.to_string()))? };
        let ok = |x: Value| Ok(x.to_string());
        if path == "/v1/agent/agents" {
            let agents: Vec<&str> = crate::acpfind::find().iter().map(|f| f.name).collect();
            return ok(json!({"enabled": self.enabled() && !agents.is_empty(), "opted_in": self.enabled(), "agents": agents}));
        }
        if !self.enabled() {
            return Err((403, "Agents aren't turned on for this sync server (Settings › Sync › Agents on the Mac running it).".into()));
        }
        // expire what's past its two hours (the runner notices too; this covers threads whose process is gone)
        let t = now();
        let expired: Vec<String> = self.threads.lock().map(|m| m.values().filter(|x| x.status.active() && x.status != Status::Review && x.expired_at(t)).map(|x| x.id.clone()).collect()).unwrap_or_default();
        for id in expired {
            let _ = self.send(&id, Cmd::Cancel);
            self.update(&id, |x| x.status = Status::Expired);
        }
        let parts: Vec<&str> = path.trim_start_matches("/v1/agent/threads").trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        match (method, parts.as_slice()) {
            ("GET", []) => {
                // ?task=ID filters; newest first
                let task = query.split('&').find_map(|kv| kv.strip_prefix("task=")).and_then(|v| v.parse::<u64>().ok());
                let mut list: Vec<Thread> = self.threads.lock().map(|m| m.values().filter(|x| task.is_none_or(|k| x.task == k)).cloned().collect()).unwrap_or_default();
                list.sort_by_key(|x| std::cmp::Reverse(x.created));
                ok(json!(list))
            }
            ("POST", []) => {
                let task = v["task"].as_u64().ok_or((400, "task: the action's id".to_string()))?;
                let request = v["request"].as_str().unwrap_or("").trim();
                if request.is_empty() {
                    return Err((400, "request: what the agent should do".into()));
                }
                // a template's fields: [[label, value], …]
                let fields: Vec<(String, String)> = v["fields"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).collect();
                let th = self.start(task, v["agent"].as_str().unwrap_or(""), request, v["from"].as_str(), v["db"].as_str().unwrap_or(""), &fields).map_err(|e| (400, e))?;
                ok(json!(th))
            }
            ("GET", [id]) => self.get(id).map(|t| json!(t).to_string()).ok_or((404, "no such thread".into())),
            ("POST", [id, action]) => {
                let cmd = match *action {
                    "reply" => Cmd::Reply(v["text"].as_str().unwrap_or("").to_string()),
                    "permission" => Cmd::Permission(v["option"].as_str().unwrap_or("").to_string()),
                    "approve" => Cmd::Approve,
                    "reject" => Cmd::Reject,
                    "finish" => Cmd::Finish,
                    "cancel" => Cmd::Cancel,
                    _ => return Err((404, "unknown action".into())),
                };
                self.send(id, cmd).map_err(|e| (409, e))?;
                ok(json!({"ok": true}))
            }
            _ => Err((404, "not found".into())),
        }
    }
}

// ---------- the client side (Mac, iPhone, iPad) ----------

/// Calls the sync server's agent API: (method, path, body) → JSON.
pub fn request(server: &str, token: &str, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{}{path}", server.trim().trim_end_matches('/'));
    let req = ureq::request(method, &url).set("Authorization", &format!("Bearer {}", token.trim())).timeout(std::time::Duration::from_secs(20));
    let r = match body {
        Some(b) => req.set("Content-Type", "application/json").send_string(&b.to_string()),
        None => req.call(),
    };
    let json = |resp: ureq::Response| resp.into_string().ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    match r {
        Ok(resp) => json(resp).ok_or_else(|| "The sync server's answer wasn't JSON.".into()),
        Err(ureq::Error::Status(_, resp)) => Err(json(resp).and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| "The sync server refused.".into())),
        Err(e) => Err(format!("Couldn't reach the sync server: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_changes_and_summarises() {
        assert_eq!(describe_change(&json!({"tool": "gtd_add_task", "args": {"name": "Buy paint"}})), "Add task “Buy paint”");
        assert_eq!(describe_change(&json!({"tool": "gtd_update_task", "args": {"id": 42, "due": "2026-11-05"}})), "Update task 42: due");
        assert_eq!(describe_change(&json!({"tool": "gtd_set_task_status", "args": {"id": 7, "status": "completed"}})), "Mark task 7 completed");
        let t0 = now();
        let th = Thread {
            id: "x".into(),
            task: 1,
            task_name: "Fix sync".into(),
            agent: "Claude Code".into(),
            status: Status::Review,
            messages: vec![Msg { role: "you".into(), text: "split it up".into(), at: t0 }, Msg { role: "agent".into(), text: "Staged 2 sub-actions.".into(), at: t0 }],
            activity: String::new(),
            steps: 3,
            changes: vec!["Add task “A”".into(), "Add task “B”".into()],
            permission: None,
            created: t0 - chrono::Duration::hours(2),
            updated: t0,
            from: None,
            db: String::new(),
        };
        assert!(th.expired_at(t0));
        assert!(!th.expired_at(t0 - chrono::Duration::minutes(1)));
        let s = th.summary();
        assert!(s.contains("The request: split it up") && s.contains("You: Staged 2 sub-actions.") && s.contains("Add task “A”; Add task “B”"), "{s}");
        let p = prompt(&Store::default(), 1, "do it", Some(&s), &[]);
        assert!(p.contains("The request: do it") && p.contains("gtd_batch") && p.contains("Earlier thread"));
        assert!(p.contains("due if it must be done 'by' or 'before'"));
        // templates: fields in order, continuation lines, the follow-up last, matching by project or tag
        let f = parse_fields("---\ntask: fix it\nproject: gtdcraft\nctx: first line\n  second line\nfollowUp: restart the sync server\nempty:\n---\n");
        assert_eq!(f, vec![("task".into(), "fix it".into()), ("project".into(), "gtdcraft".into()), ("ctx".into(), "first line\nsecond line".into()), ("followUp".into(), "restart the sync server".into()), ("empty".into(), String::new())]);
        assert!(template_matches(&f, "Work : GTDCraft", &[]));
        assert!(!template_matches(&f, "Chores", &[]));
        assert!(template_matches(&[("tag".into(), "bug, ux".into())], "", &["Status : bug".into()]));
        let b = fields_block(&f);
        assert!(b.starts_with("From the person's template:\n- task: fix it\n") && b.ends_with("Last, once everything else is done: restart the sync server\n") && !b.contains("empty"), "{b}");
        let p = prompt(&Store::default(), 1, "add dark mode", None, &f);
        assert!(p.contains("- ctx: first line"));
        assert_eq!(parse_fields(&default_templates()[0].text).len(), 5);
        // {variables} from the action; an unknown one is an error
        let env = vec![("task", "Fix sync".to_string()), ("project", "GTDCraft".to_string())];
        assert_eq!(fill_vars("work on {task} in {Project}", &env).unwrap(), "work on Fix sync in GTDCraft");
        assert_eq!(fill_vars("literal {{braces}}", &env).unwrap(), "literal {braces}");
        assert!(fill_vars("{nope}", &env).unwrap_err().starts_with("The variable 'nope' is not defined"));
        assert!(fill_vars("{task", &env).is_err());
    }
}
