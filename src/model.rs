//! Data model and GTD rules. Every rule cites MECHANICS.md.
use chrono::{Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type Id = u64;
pub type Dt = NaiveDateTime; // floating time zone

/// Task status.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Blocked = 0,
    Available = 1,
    Next = 2,
    Completed = 3,
    DueSoon = 4,
    Overdue = 5,
    Dropped = 6,
}

impl Status {
    pub fn remaining(self) -> bool {
        !matches!(self, Status::Completed | Status::Dropped)
    }
    pub fn available(self) -> bool {
        matches!(self, Status::Available | Status::Next | Status::DueSoon | Status::Overdue)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    #[default]
    Parallel,
    Sequential,
    SingleActions,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    OnHold,
    Completed,
    Dropped,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RepeatUnit {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

/// Manual, "Schedule Regularly or From Completion".
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RepeatSchedule {
    Regularly,
    FromCompletion,
}

/// Manual, "Based On": the date the next occurrence is calculated from.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BasedOn {
    Defer,
    Planned,
    Due,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Repeat {
    pub every: u32,
    pub unit: RepeatUnit,
    pub schedule: RepeatSchedule,
    pub based_on: BasedOn,
    #[serde(default)]
    pub catch_up: bool,
    /// Manual, "End ... After": occurrences left including this one.
    #[serde(default)]
    pub end_after: Option<u32>,
}

impl Default for Repeat {
    /// Manual: "new repetitions ... repeat every One Day, using the Regularly schedule, based on the assigned Due Date."
    fn default() -> Self {
        Repeat { every: 1, unit: RepeatUnit::Day, schedule: RepeatSchedule::Regularly, based_on: BasedOn::Due, catch_up: false, end_after: None }
    }
}

impl Repeat {
    pub fn add(&self, d: Dt) -> Dt {
        let n = self.every.max(1);
        match self.unit {
            RepeatUnit::Minute => d + Duration::minutes(n as i64),
            RepeatUnit::Hour => d + Duration::hours(n as i64),
            RepeatUnit::Day => d + Duration::days(n as i64),
            RepeatUnit::Week => d + Duration::weeks(n as i64),
            RepeatUnit::Month => d.checked_add_months(Months::new(n)).unwrap_or(d),
            RepeatUnit::Year => d.checked_add_months(Months::new(n * 12)).unwrap_or(d),
        }
    }
    /// Manual: From Completion rounds to the unit so times stay predictable
    /// (hours round at the 30th minute; days and longer keep the old time of day).
    fn rounded(&self, at: Dt, keep_time: NaiveTime) -> Dt {
        use chrono::Timelike;
        let t0 = at.with_second(0).unwrap_or(at);
        match self.unit {
            RepeatUnit::Minute => if at.second() >= 30 { t0 + Duration::minutes(1) } else { t0 },
            RepeatUnit::Hour => {
                let h = t0.with_minute(0).unwrap_or(t0);
                if at.minute() >= 30 { h + Duration::hours(1) } else { h }
            }
            _ => at.date().and_time(keep_time),
        }
    }
    pub fn unit_name(&self) -> &'static str {
        match self.unit {
            RepeatUnit::Minute => "minute",
            RepeatUnit::Hour => "hour",
            RepeatUnit::Day => "day",
            RepeatUnit::Week => "week",
            RepeatUnit::Month => "month",
            RepeatUnit::Year => "year",
        }
    }
    pub fn label(&self) -> String {
        let u = self.unit_name();
        let every = if self.every <= 1 { format!("Every {u}") } else { format!("Every {} {u}s", self.every) };
        let sched = match self.schedule {
            RepeatSchedule::Regularly if self.catch_up => ", catch up",
            RepeatSchedule::Regularly => "",
            RepeatSchedule::FromCompletion => " from completion",
        };
        let end = self.end_after.map(|n| format!(", {n} left")).unwrap_or_default();
        format!("{every}{sched}{end}")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub project: Option<Id>,
    #[serde(default)]
    pub parent: Option<Id>,
    #[serde(default)]
    pub tags: Vec<Id>,
    #[serde(default)]
    pub flagged: bool,
    #[serde(default)]
    pub sequential: bool,
    /// Complete this group when its last action is completed (default false).
    #[serde(default)]
    pub complete_with_last: bool,
    /// Linked Linear issue identifier, e.g. "ENG-123".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear: Option<String>,
    /// References: GitHub repos, issues and pull requests, emails, Messages conversations, web pages (`refs.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<crate::refs::Ref>,
    /// File/folder attachments: paths or `file://` URLs, separate from links found in notes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    /// Org-mode style LOGBOOK: clock-in / clock-out pairs (end None = running).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clock: Vec<Clock>,
    #[serde(default)]
    pub defer: Option<Dt>,
    #[serde(default)]
    pub planned: Option<Dt>,
    #[serde(default)]
    pub due: Option<Dt>,
    #[serde(default)]
    pub completed: Option<Dt>,
    #[serde(default)]
    pub dropped: Option<Dt>,
    #[serde(default)]
    pub duration_min: Option<u32>,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    pub added: Dt,
    pub modified: Dt,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub start: Dt,
    pub end: Option<Dt>,
    /// Clocked by a Pomodoro work interval.
    #[serde(default)]
    pub pomodoro: bool,
}

impl Clock {
    pub fn minutes(&self, now: Dt) -> i64 {
        (self.end.unwrap_or(now) - self.start).num_minutes().max(0)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub kind: ProjectKind,
    #[serde(default)]
    pub status: ProjectStatus,
    #[serde(default)]
    pub flagged: bool,
    pub review_every_days: u32,
    pub next_review: NaiveDate,
    /// Linked Linear issue identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear: Option<String>,
    /// References (the project's repo, its issues and pull requests, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<crate::refs::Ref>,
    /// File/folder attachments: paths or `file://` URLs, separate from links found in notes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub last_review: Option<Dt>,
    pub added: Dt,
    pub modified: Dt,
    /// Board columns for this project (overrides the Projects perspective's): see `PerspectiveCfg::columns`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<String>,
}

/// Separates a nested folder from its parent in its name, like tags: "Work : Clients".
pub const FOLDER_SEP: &str = " : ";

/// `name` is `folder` or inside it.
pub fn in_folder(name: &str, folder: &str) -> bool {
    name == folder || name.strip_prefix(folder).is_some_and(|rest| rest.starts_with(FOLDER_SEP))
}

/// The folder a nested folder is in ("Work" for "Work : Clients").
pub fn folder_parent(name: &str) -> Option<&str> {
    name.rfind(FOLDER_SEP).map(|i| &name[..i])
}

/// A folder of projects. Projects name their folder (`Project::folder`); a record keeps a folder that has no projects
/// yet, carries its renames, and syncs (by id, newest `modified` wins, deletes as tombstones — like tags).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub modified: Option<Dt>,
    /// A folder is Active or Dropped;
    /// dropping it drops everything inside (its projects).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<Dt>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tag {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub on_hold: bool,
    /// A place for the Nearby perspective.
    #[serde(default)]
    pub place: Option<Place>,
    /// Parent tag; typed as "Parent : Child".
    #[serde(default)]
    pub parent: Option<Id>,
    /// Last change (for sync); older files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<Dt>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Place {
    pub label: String,
    /// The postal address, one line per row ("7535 Torrey Santa Fe Rd\nSan Diego CA 92129\nUnited States");
    /// empty when the place was dropped as a pin and not looked up.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    pub lat: f64,
    pub lon: f64,
    /// Metres; None or ≤ 0 means the default 1 km.
    #[serde(default)]
    pub radius_m: Option<f64>,
    /// iPhone/iPad location reminder: none, on arriving, or on leaving (the inspector's "Notification").
    #[serde(default)]
    pub notify: Notify,
}

/// When a tag's place sends its location reminder. Places from before this field existed notified on arrival,
/// so that stays the default.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    None,
    #[default]
    Arriving,
    Leaving,
}

/// Place sizes small / medium / large in metres
/// ("Small — roughly 200 meters", "Medium — roughly 500 meters or about 1/4 mile", "Large — about 10 kilometers";
/// the iPhone shows medium as "~¼ mi").
pub const PLACE_RADII: [f64; 3] = [200.0, 500.0, 10000.0];

/// The "Distance" caption for a size: miles/feet where the locale uses them, else metres.
pub fn radius_label(tier: usize, imperial: bool) -> &'static str {
    const LABELS: [(&str, &str); 3] = [("~200 m", "~650 ft"), ("~500 m", "~\u{bc} mi"), ("~10 km", "~6\u{bc} mi")];
    let (m, i) = LABELS[tier.min(2)];
    if imperial {
        i
    } else {
        m
    }
}

impl Place {
    pub fn radius(&self) -> f64 {
        self.radius_m.filter(|r| *r > 0.0).unwrap_or(1000.0)
    }
    /// The size (index into `PLACE_RADII`) nearest this place's radius, by ratio.
    pub fn tier(&self) -> usize {
        let r = self.radius().ln();
        (0..3).min_by(|a, b| (PLACE_RADII[*a].ln() - r).abs().total_cmp(&(PLACE_RADII[*b].ln() - r).abs())).unwrap_or(1)
    }
    /// A new spot for a tag that keeps the old place's size and notification.
    pub fn relocated(old: Option<&Place>, label: String, address: String, lat: f64, lon: f64) -> Place {
        Place { label, address, lat, lon, ..old.cloned().unwrap_or_default() }
    }
    /// Great-circle distance in km.
    pub fn km_to(&self, o: &Place) -> f64 {
        let (a1, a2) = (self.lat.to_radians(), o.lat.to_radians());
        let (da, db) = (a2 - a1, (o.lon - self.lon).to_radians());
        let h = (da / 2.0).sin().powi(2) + a1.cos() * a2.cos() * (db / 2.0).sin().powi(2);
        6371.0 * 2.0 * h.sqrt().asin()
    }
}

/// Rules for a custom perspective (manual: "Availability", flagged, tag, project, due window, grouping).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rules {
    /// available | remaining | completed | dropped | all
    pub availability: String,
    #[serde(default)]
    pub flagged_only: bool,
    #[serde(default)]
    pub tag: Option<Id>,
    #[serde(default)]
    pub project: Option<Id>,
    #[serde(default)]
    pub due_within_days: Option<u32>,
    /// none | project | tag
    #[serde(default)]
    pub group: String,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { availability: "available".into(), flagged_only: false, tag: None, project: None, due_within_days: None, group: "project".into() }
    }
}

/// One entry in the perspective bar. Built-ins have a fixed key; custom ones carry rules.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PerspectiveCfg {
    /// Starred: a favourite shows in the iPhone's bottom bar. None = the default
    /// (`PERSPECTIVE_FAVORITES`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    pub key: String,
    pub name: String,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Theme hue: red, orange, yellow, green, cyan, blue, indigo, purple, violet.
    pub hue: String,
    /// Earlier flat rules; read once and turned into `filter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Rules>,
    /// Custom perspectives: the rule tree ("Contents").
    #[serde(default)]
    pub filter: Option<crate::rules::Rule>,
    /// View options availability: first | available | remaining | everything (None = perspective default).
    #[serde(default)]
    pub show: Option<String>,
    /// Kanban board instead of a list.
    #[serde(default)]
    pub board: bool,
    /// Custom perspectives' Structure: none (flexible) | project | tag | folder | due | flagged.
    #[serde(default)]
    pub group: Option<String>,
    /// Custom perspectives' initial order: none | due | defer | planned | added | modified | name | flagged.
    #[serde(default)]
    pub sort: Option<String>,
    /// Board columns: None = the perspective's usual ones; "status" (To Do / Waiting / Done), "project", "tag",
    /// or "group:<tag id>" = one column per child of that tag (a status set like Status : Backlog, To Do, In Progress).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<String>,
}

impl PerspectiveCfg {
    pub fn is_custom(&self) -> bool {
        self.key.starts_with("custom-")
    }
    /// The rule tree, converting earlier flat rules on the fly.
    pub fn rule(&self) -> crate::rules::Rule {
        self.filter.clone().or_else(|| self.rules.as_ref().map(crate::rules::from_legacy)).unwrap_or_else(crate::rules::Rule::default_root)
    }
    pub fn group_by(&self) -> String {
        self.group.clone().or_else(|| self.rules.as_ref().map(|r| r.group.clone())).unwrap_or_else(|| "none".into())
    }
}

fn yes() -> bool {
    true
}

/// The default favourite perspectives.
pub const PERSPECTIVE_FAVORITES: [&str; 7] = ["inbox", "projects", "tags", "forecast", "flagged", "nearby", "review"];

impl PerspectiveCfg {
    pub fn is_favorite(&self) -> bool {
        self.favorite.unwrap_or_else(|| PERSPECTIVE_FAVORITES.contains(&self.key.as_str()))
    }

    pub fn blank() -> PerspectiveCfg {
        PerspectiveCfg { favorite: None, key: String::new(), name: String::new(), visible: true, hue: "blue".into(), rules: None, filter: None, show: None, board: false, group: None, sort: None, columns: None }
    }
}

pub fn default_perspectives() -> Vec<PerspectiveCfg> {
    // Order and names from the person's perspective bar; hues per perspective icon.
    [
        ("inbox", "Inbox", "purple"),
        ("forecast", "Forecast", "red"),
        ("review", "Review", "indigo"),
        ("flagged", "Flagged", "orange"),
        ("projects", "Projects", "blue"),
        ("tags", "Tags", "violet"),
        ("nearby", "Nearby", "green"),
        ("completed", "Completed", "violet"),
        ("changed", "Changed", "violet"),
        ("stats", "Stats", "cyan"),
        ("agents", "Agents", "indigo"),
    ]
    .into_iter()
    .map(|(k, n, h)| PerspectiveCfg { key: k.into(), name: n.into(), visible: true, hue: h.into(), ..PerspectiveCfg::blank() })
    .collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LinkCard {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub site: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub due_soon_days: u32,
    pub default_due_time: NaiveTime,
    pub default_defer_time: NaiveTime,
    pub default_planned_time: NaiveTime,
    pub review_every_days: u32,
    /// Off by default; delay in main.rs.
    #[serde(default)]
    pub auto_hide_completed: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Follow macOS light/dark with the two themes below.
    #[serde(default)]
    pub follow_system: bool,
    #[serde(default = "default_light")]
    pub light_theme: String,
    #[serde(default = "default_theme")]
    pub dark_theme: String,
    #[serde(default = "default_perspectives")]
    pub perspectives: Vec<PerspectiveCfg>,
    /// Defaults: reminders on, due on, defer and planned off.
    #[serde(default = "yes")]
    pub notify: bool,
    #[serde(default = "yes")]
    pub notify_due: bool,
    #[serde(default)]
    pub notify_defer: bool,
    #[serde(default)]
    pub notify_planned: bool,
    /// Remind at the latest time to start (default off): due date minus estimated duration.
    #[serde(default)]
    pub notify_start: bool,
    /// Also remind this many minutes before an item's planned and due times, to wrap up and get ready (0 = off).
    #[serde(default)]
    pub notify_before_min: u32,
    /// Your location for Nearby distances.
    #[serde(default)]
    pub here: Option<Place>,
    /// Keyboard overrides: action id -> bindings like "cmd+shift+n".
    #[serde(default)]
    pub keymap: std::collections::BTreeMap<String, Vec<String>>,
    /// Sync server (gtdcraft --sync-server) address and token; empty = not syncing. Kept per device.
    #[serde(default)]
    pub sync_url: String,
    #[serde(default)]
    pub sync_token: String,
    /// Show Calendar events in Forecast, the Focus window and to agents (on by default).
    /// Reading them needs macOS permission.
    #[serde(default = "yes")]
    pub calendar_events: bool,
    /// Calendars left out (calendar identifiers).
    #[serde(default)]
    pub hidden_calendars: Vec<String>,
    /// Include all-day events.
    #[serde(default)]
    pub calendar_all_day: bool,
    /// Inspector section order (section keys); the task and project inspectors each show the ones they have.
    #[serde(default)]
    pub inspector_order: Vec<String>,
    /// When the perspectives or the inspector's section order last changed on any device: these two sync (newest
    /// wins); the rest of Settings stays on each device (server, keys, places, themes).
    #[serde(default)]
    pub layout_modified: Option<Dt>,
    /// When a connection (Linear key, GitHub token, AI provider/key/model/server) last changed on any device: these
    /// sync too (newest wins), so a new iPhone is connected as soon as it syncs.
    #[serde(default)]
    pub connections_modified: Option<Dt>,
    /// Pomodoro lengths in minutes: work, short break, long break, and work intervals per long break.
    #[serde(default = "pomo_default")]
    pub pomodoro: [u32; 4],
    /// AI helper: claude | openai (any OpenAI-compatible server, e.g. OpenAI or Hermes on Ollama) | acp.
    #[serde(default = "default_provider")]
    pub ai_provider: String,
    /// API key (Claude falls back to ANTHROPIC_API_KEY, OpenAI-compatible to OPENAI_API_KEY).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ai_key: String,
    #[serde(default)]
    pub ai_model: String,
    /// OpenAI-compatible base URL, e.g. https://api.openai.com/v1 or http://localhost:11434/v1.
    #[serde(default)]
    pub ai_base_url: String,
    /// ACP agent command line, run through the shell.
    #[serde(default)]
    pub acp_command: String,
    /// Send to Agent's prompt templates (agent.rs `Template`); they travel with the connections.
    #[serde(default = "crate::agent::default_templates")]
    pub agent_templates: Vec<crate::agent::Template>,
    /// System One decisions (`systemone.rs`), off until turned on.
    #[serde(default)]
    pub decisions_on: bool,
    /// local | cloudflare | jev | rune | custom (`systemone::PROVIDERS`).
    #[serde(default)]
    pub decisions_provider: String,
    /// The decision endpoint; empty = the provider's. A base URL gets /v1/systemone.
    #[serde(default, alias = "clef_url")]
    pub decisions_url: String,
    /// Empty = the provider's default model.
    #[serde(default)]
    pub decisions_model: String,
    /// Bearer key; empty = the provider's environment variable (CLOUDFLARE_API_TOKEN, TYPESAFE_API_KEY, FIREWORKS_API_KEY).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decisions_key: String,
    /// Cloudflare account ID (falls back to CLOUDFLARE_ACCOUNT_ID).
    #[serde(default)]
    pub decisions_account: String,
    /// Linear personal API key (falls back to LINEAR_API_KEY).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub linear_key: String,
    /// GitHub token for References (optional; falls back to GITHUB_TOKEN; public repos work without one).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub github_token: String,
    /// Forecast view options: include items on their planned / defer dates.
    #[serde(default = "yes")]
    pub forecast_planned: bool,
    #[serde(default = "yes")]
    pub forecast_defer: bool,
    /// Forecast's day timeline runs down the page (hours as rows) rather than across it.
    #[serde(default)]
    pub forecast_vertical: bool,
    /// All-day calendar events fill the day timeline (off: they're listed but don't block time).
    #[serde(default)]
    pub forecast_all_day_blocks: bool,
    /// The Quick Entry card's size in points, as the person last dragged it (default: a small window, about
    /// 713 × 196).
    #[serde(default = "quick_size_default")]
    pub quick_size: [f32; 2],
    /// Where the person left the Quick Entry card (top-left, points); none = centred, as on first open.
    #[serde(default)]
    pub quick_pos: Option<[f32; 2]>,
}

fn quick_size_default() -> [f32; 2] {
    [713.0, 196.0]
}

fn pomo_default() -> [u32; 4] {
    [25, 5, 15, 4]
}
fn default_provider() -> String {
    "claude".into()
}
fn default_theme() -> String {
    "Focus Dark".into()
}
fn default_light() -> String {
    "Focus Light".into()
}

/// Inspector sections in their default order. Integrations (Linear now, GitHub and others later) are sections too.
pub const INSPECTOR_SECTIONS: &[(&str, &str)] = &[
    ("status", "Status"),
    ("type", "Project Type"),
    ("folder", "Folder"),
    ("review", "Review"),
    ("project", "Project"),
    ("tags", "Tags"),
    ("duration", "Duration"),
    ("dates", "Dates"),
    ("notifications", "Notifications"),
    ("completed", "Completed & Dropped"),
    ("repeat", "Repeat"),
    ("note", "Note"),
    ("time", "Time"),
    ("linear", "Linear"),
    ("attachments", "Attachments"),
    ("refs", "References"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecMove {
    Up,
    Down,
    Top,
    Bottom,
    /// Open the inspector's drag-to-rearrange mode.
    Arrange,
}

impl Settings {
    /// The saved order with unknown keys dropped and new sections appended.
    pub fn inspector_order(&self) -> Vec<String> {
        let mut out: Vec<String> = self.inspector_order.iter().filter(|k| INSPECTOR_SECTIONS.iter().any(|s| s.0 == k.as_str())).cloned().collect();
        for (k, _) in INSPECTOR_SECTIONS {
            if !out.iter().any(|x| x == k) {
                out.push(k.to_string());
            }
        }
        out
    }

    /// Moves a section among the ones an inspector shows (`visible`), keeping the others where they are.
    pub fn move_section(&mut self, key: &str, m: SecMove, visible: &[&str]) {
        let order = self.inspector_order();
        let vis: Vec<usize> = order.iter().enumerate().filter(|(_, k)| visible.contains(&k.as_str())).map(|(i, _)| i).collect();
        let Some(at) = vis.iter().position(|&i| order[i] == key) else { return };
        let target = match m {
            SecMove::Up if at > 0 => at - 1,
            SecMove::Down if at + 1 < vis.len() => at + 1,
            SecMove::Top => 0,
            SecMove::Bottom => vis.len() - 1,
            _ => return,
        };
        self.place_section(key, target, visible);
    }

    /// Puts a section at position `target` among the `visible` ones (drag and drop), keeping the others where they are.
    pub fn place_section(&mut self, key: &str, target: usize, visible: &[&str]) {
        let mut order = self.inspector_order();
        let vis: Vec<usize> = order.iter().enumerate().filter(|(_, k)| visible.contains(&k.as_str())).map(|(i, _)| i).collect();
        let Some(at) = vis.iter().position(|&i| order[i] == key) else { return };
        let target = target.min(vis.len() - 1);
        // rotate the visible keys so `key` lands at `target`; hidden keys keep their slots
        let mut keys: Vec<String> = vis.iter().map(|&i| order[i].clone()).collect();
        let k = keys.remove(at);
        keys.insert(target, k);
        for (slot, k) in vis.iter().zip(keys) {
            order[*slot] = k;
        }
        self.inspector_order = order;
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            // two days
            due_soon_days: 2,
            default_due_time: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            default_defer_time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            default_planned_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            // ProjectInfoReviewRepetitionString "@1w"
            review_every_days: 7,
            auto_hide_completed: false,
            theme: default_theme(),
            follow_system: false,
            light_theme: default_light(),
            dark_theme: default_theme(),
            perspectives: default_perspectives(),
            notify: true,
            notify_due: true,
            notify_defer: false,
            notify_planned: false,
            notify_start: false,
            notify_before_min: 0,
            here: None,
            keymap: Default::default(),
            inspector_order: vec![],
            layout_modified: None,
            connections_modified: None,
            sync_url: String::new(),
            sync_token: String::new(),
            calendar_events: true,
            hidden_calendars: vec![],
            calendar_all_day: false,
            pomodoro: pomo_default(),
            ai_provider: default_provider(),
            ai_key: String::new(),
            ai_model: String::new(),
            ai_base_url: String::new(),
            acp_command: String::new(),
            agent_templates: crate::agent::default_templates(),
            decisions_on: false,
            decisions_provider: String::new(),
            decisions_url: String::new(),
            decisions_model: String::new(),
            decisions_key: String::new(),
            decisions_account: String::new(),
            linear_key: String::new(),
            github_token: String::new(),
            forecast_planned: true,
            forecast_defer: true,
            forecast_vertical: false,
            forecast_all_day_blocks: false,
            quick_size: quick_size_default(),
            quick_pos: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Store {
    pub next_id: Id,
    pub tasks: Vec<Task>,
    pub projects: Vec<Project>,
    pub tags: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub settings: Settings,
    /// Link previews for URLs in notes.
    #[serde(default)]
    pub link_cards: std::collections::BTreeMap<String, LinkCard>,
    /// Finished Pomodoros: (end time, task).
    #[serde(default)]
    pub pomodoros: Vec<(Dt, Option<Id>)>,
    /// Last-fetched Linear issues by identifier.
    #[serde(default)]
    pub linear: std::collections::BTreeMap<String, crate::linear::Issue>,
    /// "Up Next": the actions you mean to work on, in order (Focus window).
    #[serde(default)]
    pub queue: Vec<Id>,
    /// When Up Next last changed: sync keeps the newer order (it kept whichever device synced last, so the widget showed
    /// the Mac's queue over the phone's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_modified: Option<Dt>,
    /// This database's identity: the sync server keeps one copy per database, so opening or starting another one
    /// never merges into (or empties) this one. Empty in databases from before; the server gives them one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub db_id: String,
    /// Deleted tasks and tags (id → when), so a delete on one device reaches the others through sync.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub deleted: std::collections::BTreeMap<Id, Dt>,
    /// This device's ID range (set when the app or MCP server opens a database): new IDs are `id_base + counter`,
    /// so two synced devices never hand out the same ID. Not saved.
    #[serde(skip)]
    pub id_base: Id,
}

/// Segments of a colon path ("Work : Clients : Acme"), trimmed, empty ones dropped.
pub fn path_parts(path: &str) -> Vec<&str> {
    path.split(':').map(str::trim).filter(|s| !s.is_empty()).collect()
}

fn norm_path(p: &str) -> String {
    path_parts(p).join(":").to_lowercase()
}

/// Same colon path, ignoring case and spaces around the colons.
pub fn path_eq(a: &str, b: &str) -> bool {
    norm_path(a) == norm_path(b)
}

/// Does a typed query match a colon path? "wo:ac" matches "Work : Acme"; plain text matches anywhere.
pub fn path_matches(path: &str, query: &str) -> bool {
    let q = norm_path(query);
    if q.is_empty() {
        return true;
    }
    let p = norm_path(path);
    if !q.contains(':') {
        return p.contains(&q);
    }
    // each typed segment is a prefix of consecutive path segments, ending at the last one
    let (ps, qs): (Vec<&str>, Vec<&str>) = (p.split(':').collect(), q.split(':').collect());
    qs.len() <= ps.len() && ps[ps.len() - qs.len()..].iter().zip(&qs).all(|(a, b)| a.starts_with(b))
}

/// A new database's identity: time and process, in hex (unique enough for one person's databases).
pub fn new_db_id() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}{:04x}", n, std::process::id() & 0xffff)
}

pub fn now() -> Dt {
    Local::now().naive_local().with_nanosecond_zero()
}

trait NoNanos {
    fn with_nanosecond_zero(self) -> Self;
}
impl NoNanos for Dt {
    fn with_nanosecond_zero(self) -> Self {
        use chrono::Timelike;
        self.with_nanosecond(0).unwrap_or(self)
    }
}

pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("GTDCRAFT_DB") {
        return PathBuf::from(p);
    }
    // the database the app last opened (File › Open / New / Save As), so `--mcp` agents work on the same one
    if let Some(p) = crate::dbfile::current() {
        return p;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Application Support/GTDCraft/gtdcraft.json")
}

/// Derived facts, computed once per frame / per MCP call in O(n).
pub struct Derived {
    pub status: HashMap<Id, Status>,
    pub children: HashMap<Option<Id>, Vec<Id>>, // parent -> ordered children (top-level keyed by None)
    pub idx: HashMap<Id, usize>,
    /// Effective (inherited) due and defer dates, as the reminders use them.
    pub eff_due: HashMap<Id, Dt>,
    pub eff_defer: HashMap<Id, Dt>,
}

impl Derived {
    /// Status lookup that never panics: a task added after this snapshot reads as Available until the next frame.
    pub fn st(&self, id: Id) -> Status {
        self.status.get(&id).copied().unwrap_or(Status::Available)
    }
}

impl Store {
    pub fn load(path: &Path) -> std::io::Result<Store> {
        let s = std::fs::read_to_string(path)?;
        crate::dbfile::from_str(&s, crate::dbfile::format_of(path)).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// The database at `path`; a new one starts empty (sample items are optional:
    /// Settings › General › Add Sample Items).
    pub fn load_or_new(path: &Path) -> Store {
        match Store::load(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let s = Store { db_id: new_db_id(), ..Store::default() };
                let _ = s.save(path);
                s
            }
            Err(e) => {
                // Never overwrite a file we couldn't read: keep a copy, start from sample.
                let bak = path.with_extension("unreadable.json");
                let _ = std::fs::copy(path, &bak);
                eprintln!("gtdcraft: couldn't read {}: {e}; saved a copy to {}", path.display(), bak.display());
                Store::default()
            }
        }
    }

    /// Atomic write: temp file then rename, so a crash never leaves half a database.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, crate::dbfile::to_string(self, crate::dbfile::format_of(path))?)?;
        std::fs::rename(tmp, path)
    }

    pub fn new_id(&mut self) -> Id {
        self.next_id += 1;
        let id = self.id_base + self.next_id;
        if self.id_base > 0 && self.task(id).is_some() {
            return self.new_id(); // (a merged counter can lag) never reuse
        }
        id
    }

    /// The sample items in this database (tasks by name and project, then sample projects and tags with nothing of
    /// yours left in them): what Settings › Remove Sample Items deletes. Returns (tasks, projects, tags).
    pub fn sample_items(&self) -> (Vec<Id>, Vec<Id>, Vec<Id>) {
        let sample = Store::sample(now());
        let proj_name = |st: &Store, p: Option<Id>| p.and_then(|p| st.project(p)).map(|p| p.name.clone());
        let keys: std::collections::HashSet<(String, Option<String>)> = sample.tasks.iter().map(|t| (t.name.clone(), proj_name(&sample, t.project))).collect();
        let tasks: Vec<Id> = self.tasks.iter().filter(|t| keys.contains(&(t.name.clone(), proj_name(self, t.project)))).map(|t| t.id).collect();
        let left = |f: &dyn Fn(&Task) -> bool| self.tasks.iter().any(|t| !tasks.contains(&t.id) && f(t));
        let projects = self.projects.iter().filter(|p| sample.projects.iter().any(|q| q.name == p.name) && !left(&|t| t.project == Some(p.id))).map(|p| p.id).collect();
        let tags = self.tags.iter().filter(|g| sample.tags.iter().any(|q| q.name == g.name) && !left(&|t| t.tags.contains(&g.id))).map(|g| g.id).collect();
        (tasks, projects, tags)
    }

    /// Deletes the sample items (with tombstones, so the delete syncs to every device). Returns how many went.
    pub fn remove_sample(&mut self) -> usize {
        let (tasks, projects, tags) = self.sample_items();
        let n = tasks.len() + projects.len() + tags.len();
        for id in tasks {
            if self.task(id).is_some() {
                self.delete_task(id);
            }
        }
        self.pomodoros.retain(|p| p.1.is_none_or(|id| self.tasks.iter().any(|t| t.id == id)));
        for id in projects {
            self.delete_project(id);
        }
        for id in tags {
            self.delete_tag(id);
        }
        if self.settings.here.as_ref().is_some_and(|h| (h.lat, h.lon) == (45.5202, -122.6742)) {
            self.settings.here = None; // the sample's "Home"
        }
        n
    }

    /// Deletes a project and every action in it, remembering the deletes for sync (Drop keeps them; Delete is for
    /// projects that shouldn't exist at all, like stray sample data).
    pub fn delete_project(&mut self, id: Id) {
        let tops: Vec<Id> = self.tasks.iter().filter(|t| t.project == Some(id) && t.parent.is_none()).map(|t| t.id).collect();
        for t in tops {
            self.delete_task(t);
        }
        for t in self.tasks.iter().filter(|t| t.project == Some(id)).map(|t| t.id).collect::<Vec<_>>() {
            self.delete_task(t); // any left under a parent outside the project
        }
        self.pomodoros.retain(|p| p.1.is_none_or(|t| !self.deleted.contains_key(&t)));
        self.projects.retain(|p| p.id != id);
        self.forget(id);
    }

    /// Deletes a folder: every project in it, with their actions, and its record.
    pub fn delete_folder(&mut self, folder: &str) {
        for id in self.projects.iter().filter(|p| in_folder(&p.folder, folder)).map(|p| p.id).collect::<Vec<_>>() {
            self.delete_project(id);
        }
        for id in self.folders.iter().filter(|f| in_folder(&f.name, folder)).map(|f| f.id).collect::<Vec<_>>() {
            self.forget(id);
        }
        self.folders.retain(|f| !in_folder(&f.name, folder));
    }

    /// A fingerprint of the parts of Settings that sync (perspectives, inspector section order).
    pub fn queue_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.queue.hash(&mut h);
        h.finish()
    }

    pub fn layout_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&(&self.settings.perspectives, &self.settings.inspector_order)).unwrap_or_default().hash(&mut h);
        h.finish()
    }

    /// A fingerprint of the connections that sync (see `Settings::connections_modified`).
    pub fn connections_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let st = &self.settings;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&st.linear_key, &st.github_token, &st.ai_provider, &st.ai_key, &st.ai_model, &st.ai_base_url).hash(&mut h);
        (st.decisions_on, &st.decisions_provider, &st.decisions_url, &st.decisions_model, &st.decisions_key, &st.decisions_account).hash(&mut h);
        serde_json::to_string(&st.agent_templates).unwrap_or_default().hash(&mut h);
        h.finish()
    }

    /// Every folder's name in sidebar order: folders keep the order they were placed in (ranked like
    /// child sections), then any folder only a project names, by name.
    ///
    /// Folders nest like tags: "Work : Clients" is Clients inside Work. The list is a tree walk — each folder followed
    /// by the ones inside it — and a parent that only exists as a prefix is listed too.
    pub fn folder_names(&self) -> Vec<String> {
        let mut v: Vec<String> = vec![];
        for n in self.folders.iter().map(|f| f.name.clone()) {
            if !n.is_empty() && !v.contains(&n) {
                v.push(n);
            }
        }
        let mut rest: Vec<String> = self.projects.iter().map(|p| p.folder.clone()).filter(|n| !n.is_empty() && !v.contains(n)).collect();
        rest.sort();
        rest.dedup();
        v.extend(rest);
        // every parent of a nested folder is a folder too
        let mut all = v.clone();
        for n in &v {
            let parts: Vec<&str> = n.split(FOLDER_SEP).collect();
            for k in 1..parts.len() {
                let parent = parts[..k].join(FOLDER_SEP);
                if !all.contains(&parent) {
                    all.push(parent);
                }
            }
        }
        // tree order: children right after their parent, each level in placed order
        fn walk(all: &[String], parent: Option<&str>, out: &mut Vec<String>) {
            for n in all.iter().filter(|n| folder_parent(n) == parent) {
                out.push(n.clone());
                walk(all, Some(n), out);
            }
        }
        let mut out = vec![];
        walk(&all, None, &mut out);
        out
    }

    /// Drops a folder (with everything inside) or makes it active again.
    pub fn set_folder_dropped(&mut self, name: &str, dropped: bool) {
        let n = now();
        if !self.folders.iter().any(|f| f.name == name) {
            let id = self.new_id();
            self.folders.push(Folder { id, name: name.to_string(), modified: Some(n), dropped: None });
        }
        if let Some(f) = self.folders.iter_mut().find(|f| f.name == name) {
            (f.dropped, f.modified) = (dropped.then_some(n), Some(n));
        }
    }

    /// A new folder inside `parent` ("Parent : New Folder"), placed right after the parent's last subfolder.
    pub fn add_subfolder(&mut self, parent: &str) -> String {
        let names = self.folder_names();
        let name = (1..).map(|i| if i == 1 { format!("{parent}{FOLDER_SEP}Untitled Folder") } else { format!("{parent}{FOLDER_SEP}Untitled Folder {i}") }).find(|n| !names.contains(n)).unwrap();
        let after = names.iter().rfind(|n| in_folder(n, parent)).cloned().unwrap_or_else(|| parent.to_string());
        let _ = self.add_folder(Some(&after));
        // add_folder made "New Folder…" at the right place; give it the nested name
        let made = self.folders.iter().position(|f| !names.contains(&f.name)).expect("the new folder");
        self.folders[made].name = name.clone();
        name
    }

    /// A new, empty folder ("New Folder", "New Folder 2", …), placed right after `after` (the folder of the selected
    /// project or action, or the selected folder), else at the top (index 0). Returns its name.
    pub fn add_folder(&mut self, after: Option<&str>) -> String {
        let names = self.folder_names();
        // an empty folder name reads "Untitled Folder"
        let name = (1..).map(|i| if i == 1 { "Untitled Folder".to_string() } else { format!("Untitled Folder {i}") }).find(|n| !names.contains(n)).unwrap();
        // every folder gets a record first, so the order is explicit
        for n in &names {
            if !self.folders.iter().any(|f| &f.name == n) {
                let id = self.new_id();
                self.folders.push(Folder { id, name: n.clone(), modified: Some(now()), dropped: None });
            }
        }
        let at = after.filter(|a| !a.is_empty()).and_then(|a| self.folders.iter().position(|f| f.name == a)).map_or(0, |i| i + 1);
        let id = self.new_id();
        self.folders.insert(at, Folder { id, name: name.clone(), modified: Some(now()), dropped: None });
        name
    }

    /// Renames a folder: its record (made if it had none) and every project in it.
    pub fn rename_folder(&mut self, old: &str, new: &str) {
        let (new, n) = (new.trim().to_string(), now());
        if new.is_empty() || new == old {
            return;
        }
        if !self.folders.iter().any(|f| f.name == old) {
            let id = self.new_id();
            self.folders.push(Folder { id, name: old.to_string(), modified: Some(n), dropped: None });
        }
        // the folder and everything inside it ("Old : Child" → "New : Child")
        let moved = |name: &str| format!("{new}{}", &name[old.len()..]);
        for f in self.folders.iter_mut().filter(|f| in_folder(&f.name, old)) {
            (f.name, f.modified) = (moved(&f.name), Some(n));
        }
        for p in self.projects.iter_mut().filter(|p| in_folder(&p.folder, old)) {
            (p.folder, p.modified) = (moved(&p.folder), n);
        }
    }

    /// Records a delete for sync.
    pub fn forget(&mut self, id: Id) {
        self.deleted.insert(id, now());
    }

    /// Deletes a tag (and takes it off every task), remembering the delete for sync.
    pub fn delete_tag(&mut self, id: Id) {
        let n = now();
        self.tags.retain(|g| g.id != id);
        for t in &mut self.tasks {
            if t.tags.contains(&id) {
                t.tags.retain(|g| *g != id);
                t.modified = n;
            }
        }
        self.forget(id);
    }

    pub fn task(&self, id: Id) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }
    pub fn task_mut(&mut self, id: Id) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }
    pub fn project(&self, id: Id) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }
    pub fn project_mut(&mut self, id: Id) -> Option<&mut Project> {
        self.projects.iter_mut().find(|p| p.id == id)
    }
    pub fn tag(&self, id: Id) -> Option<&Tag> {
        self.tags.iter().find(|t| t.id == id)
    }
    pub fn project_by_name(&self, name: &str) -> Option<Id> {
        if name.contains(':') {
            return self.projects.iter().find(|p| path_eq(&self.project_path(p.id), name)).map(|p| p.id);
        }
        self.projects.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.id)
    }
    /// A tag from typed text: the tag whose whole name
    /// ("Parent : Child", joined with " : ") matches, ignoring case; else the
    /// one tag anywhere whose own name matches the last part — none when
    /// several do, so an ambiguous name never picks one at random. (Creating a missing tag is `tag_for_path`.)
    pub fn tag_by_name(&self, name: &str) -> Option<Id> {
        if let Some(t) = self.tags.iter().find(|t| path_eq(&self.tag_path(t.id), name)) {
            return Some(t.id);
        }
        let leaf = *path_parts(name).last()?;
        let mut m = self.tags.iter().filter(|t| t.name.eq_ignore_ascii_case(leaf));
        match (m.next(), m.next()) {
            (Some(t), None) => Some(t.id),
            _ => None,
        }
    }

    /// Projects and tags by how recently they were used: the newest change to any action in the project, or carrying
    /// the tag, first (the person: "project/tag lists should show most recently used at the top until they start
    /// typing"). Derived from the actions, so it syncs with them; unused ones keep their order after.
    pub fn by_recent_use<T: Copy + Eq + std::hash::Hash>(&self, items: &mut [(T, String)], key: impl Fn(&Task) -> Vec<T>) {
        let mut last: std::collections::HashMap<T, Dt> = std::collections::HashMap::new();
        for t in &self.tasks {
            for k in key(t) {
                let e = last.entry(k).or_insert(t.modified);
                *e = (*e).max(t.modified);
            }
        }
        items.sort_by_key(|(k, _)| std::cmp::Reverse(last.get(k).copied()));
    }

    /// "Parent : Child" for a nested tag.
    pub fn tag_path(&self, id: Id) -> String {
        let mut parts = vec![];
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(t) = self.tag(c) else { break };
            parts.push(t.name.as_str());
            cur = t.parent.filter(|p| *p != c && parts.len() < 32); // guard against a cycle in a hand-edited file
        }
        parts.reverse();
        parts.join(" : ")
    }

    /// A tag and every tag nested under it (a parent tag's list shows its children's actions too).
    pub fn tag_family(&self, id: Id) -> Vec<Id> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let p = out[i];
            out.extend(self.tags.iter().filter(|t| t.parent == Some(p) && !out.contains(&t.id)).map(|t| t.id).collect::<Vec<_>>());
            i += 1;
        }
        out
    }

    /// Tags in outline order (each parent followed by its children) with their depth.
    pub fn tag_tree(&self) -> Vec<(Id, usize)> {
        fn walk(s: &Store, parent: Option<Id>, depth: usize, out: &mut Vec<(Id, usize)>) {
            for t in s.tags.iter().filter(|t| t.parent == parent || (depth == 0 && t.parent.is_some_and(|p| s.tag(p).is_none()))) {
                if out.iter().any(|x| x.0 == t.id) || depth > 32 {
                    continue;
                }
                out.push((t.id, depth));
                walk(s, Some(t.id), depth + 1, out);
            }
        }
        let mut out = vec![];
        walk(self, None, 0, &mut out);
        out
    }

    /// Tags with children: the status sets a board can use as columns.
    pub fn tag_groups(&self) -> Vec<Id> {
        self.tag_tree().into_iter().map(|x| x.0).filter(|g| self.tags.iter().any(|t| t.parent == Some(*g))).collect()
    }

    /// Children of a tag in outline order (a status set's stages).
    pub fn tag_children(&self, g: Id) -> Vec<Id> {
        self.tag_tree().into_iter().map(|x| x.0).filter(|t| self.tag(*t).is_some_and(|x| x.parent == Some(g))).collect()
    }

    /// Creates the "Status" set: Backlog, To Do, In Progress, Waiting for Review (reuses any that exist).
    pub fn default_status_set(&mut self) -> Option<Id> {
        for stage in ["Backlog", "To Do", "In Progress", "Waiting for Review"] {
            self.tag_for_path(&format!("Status : {stage}"));
        }
        self.tags.iter().find(|t| t.parent.is_none() && t.name == "Status").map(|t| t.id)
    }

    /// Moves an action to a stage of a status set: drops its other stages of that set, adds this one (None = no stage).
    pub fn set_stage(&mut self, id: Id, group: Id, stage: Option<Id>) {
        let stages = self.tag_children(group);
        if let Some(t) = self.task_mut(id) {
            t.tags.retain(|g| !stages.contains(g));
            if let Some(s) = stage {
                t.tags.push(s);
            }
        }
    }

    /// Up Next without finished or deleted actions.
    pub fn queue_live(&self) -> Vec<Id> {
        self.queue.iter().copied().filter(|id| self.task(*id).is_some_and(|t| t.completed.is_none() && t.dropped.is_none())).collect()
    }

    /// Adds to the end of Up Next (no duplicates).
    pub fn queue_add(&mut self, id: Id) {
        if !self.queue.contains(&id) {
            self.queue.push(id);
        }
    }

    /// The next thing with a time today after `now` (due or planned): (when, action) — "how long until".
    pub fn next_timed(&self, now: Dt) -> Option<(Dt, Id)> {
        self.tasks
            .iter()
            .filter(|t| t.completed.is_none() && t.dropped.is_none())
            .flat_map(|t| [t.due, t.planned].into_iter().flatten().map(move |x| (x, t.id)))
            .filter(|(x, _)| *x > now && x.date() == now.date())
            .min_by_key(|x| x.0)
    }

    /// Finds or creates the tag for "A : B : C", creating each missing level under the previous one.
    pub fn tag_for_path(&mut self, path: &str) -> Option<Id> {
        let mut parent = None;
        for seg in path_parts(path) {
            let found = self.tags.iter().find(|t| t.parent == parent && t.name.eq_ignore_ascii_case(seg)).map(|t| t.id);
            parent = Some(match found {
                Some(id) => id,
                None => {
                    let id = self.new_id();
                    self.tags.push(Tag { id, name: seg.into(), on_hold: false, place: None, parent, modified: None });
                    id
                }
            });
        }
        parent
    }

    /// "Folder : Project" (folders can nest: "Work : Clients : Acme").
    pub fn project_path(&self, id: Id) -> String {
        self.project(id).map(|p| if p.folder.is_empty() { p.name.clone() } else { format!("{} : {}", p.folder, p.name) }).unwrap_or_default()
    }

    /// Finds or creates the project for "Folder : Project". Without a folder, an existing project of that name anywhere counts.
    pub fn project_for_path(&mut self, path: &str) -> Option<Id> {
        let parts = path_parts(path);
        let (name, folders) = parts.split_last()?;
        let folder = folders.join(" : ");
        let hit = self.projects.iter().find(|p| p.name.eq_ignore_ascii_case(name) && (folders.is_empty() || p.folder.eq_ignore_ascii_case(&folder)));
        if let Some(p) = hit {
            return Some(p.id);
        }
        let id = self.add_project(name, ProjectKind::Parallel);
        if let Some(p) = self.project_mut(id) {
            p.folder = folder;
        }
        Some(id)
    }

    pub fn add_task(&mut self, name: &str, project: Option<Id>, parent: Option<Id>) -> Id {
        let id = self.new_id();
        let t = now();
        let project = parent.and_then(|p| self.task(p)).map(|p| p.project).unwrap_or(project);
        self.tasks.push(Task {
            id,
            name: name.into(),
            note: String::new(),
            project,
            parent,
            tags: vec![],
            flagged: false,
            sequential: false,
            complete_with_last: false,
            linear: None,
            refs: vec![],
            attachments: vec![],
            clock: vec![],
            defer: None,
            planned: None,
            due: None,
            completed: None,
            dropped: None,
            duration_min: None,
            repeat: None,
            added: t,
            modified: t,
        });
        id
    }

    pub fn add_project(&mut self, name: &str, kind: ProjectKind) -> Id {
        let id = self.new_id();
        let t = now();
        let every = self.settings.review_every_days;
        self.projects.push(Project {
            id,
            name: name.into(),
            note: String::new(),
            folder: String::new(),
            kind,
            status: ProjectStatus::Active,
            flagged: false,
            review_every_days: every,
            linear: None,
            refs: vec![],
            attachments: vec![],
            next_review: t.date() + Duration::days(every as i64),
            last_review: None,
            added: t,
            modified: t,
            columns: None,
        });
        id
    }

    /// Moves a task (and its subtree) to a project; None = Inbox.
    pub fn set_project(&mut self, id: Id, project: Option<Id>) {
        let mut stack = vec![id];
        if let Some(t) = self.task_mut(id) {
            t.parent = None;
        }
        while let Some(cur) = stack.pop() {
            for t in self.tasks.iter_mut() {
                if t.id == cur {
                    t.project = project;
                    t.modified = now();
                }
                if t.parent == Some(cur) {
                    stack.push(t.id);
                }
            }
        }
    }

    /// Deletes a task and its subtree.
    pub fn delete_task(&mut self, id: Id) {
        let mut doomed = vec![id];
        let mut i = 0;
        while i < doomed.len() {
            let cur = doomed[i];
            doomed.extend(self.tasks.iter().filter(|t| t.parent == Some(cur)).map(|t| t.id));
            i += 1;
        }
        self.tasks.retain(|t| !doomed.contains(&t.id));
        for d in doomed {
            self.forget(d);
        }
    }

    fn sibling_ids(&self, id: Id) -> Vec<Id> {
        let Some(t) = self.task(id) else { return vec![] };
        let (parent, project) = (t.parent, t.project);
        self.tasks.iter().filter(|x| x.parent == parent && (parent.is_some() || x.project == project)).map(|x| x.id).collect()
    }

    fn pos(&self, id: Id) -> Option<usize> {
        self.tasks.iter().position(|t| t.id == id)
    }

    /// Move Up / Move Down among siblings (manual, "Moving Items").
    pub fn move_sibling(&mut self, id: Id, down: bool) -> bool {
        let sibs = self.sibling_ids(id);
        let Some(i) = sibs.iter().position(|x| *x == id) else { return false };
        let other = if down { sibs.get(i + 1) } else { i.checked_sub(1).and_then(|j| sibs.get(j)) };
        let Some(&other) = other else { return false };
        let (a, b) = (self.pos(id).unwrap(), self.pos(other).unwrap());
        self.tasks.swap(a, b);
        true
    }

    /// Indent: becomes the last child of the sibling above.
    pub fn indent(&mut self, id: Id) -> bool {
        let sibs = self.sibling_ids(id);
        let Some(i) = sibs.iter().position(|x| *x == id) else { return false };
        let Some(&above) = i.checked_sub(1).and_then(|j| sibs.get(j)) else { return false };
        let t = self.tasks.remove(self.pos(id).unwrap());
        let mut t = t;
        t.parent = Some(above);
        t.modified = now();
        self.tasks.push(t);
        true
    }

    /// Outdent: moves out of its group, right after it.
    pub fn outdent(&mut self, id: Id) -> bool {
        let Some(parent) = self.task(id).and_then(|t| t.parent) else { return false };
        let grand = self.task(parent).and_then(|p| p.parent);
        let mut t = self.tasks.remove(self.pos(id).unwrap());
        t.parent = grand;
        t.modified = now();
        let at = self.pos(parent).map(|p| p + 1).unwrap_or(self.tasks.len());
        self.tasks.insert(at, t);
        true
    }

    /// Group: wraps the task in a new action group in its place; returns the group.
    pub fn group(&mut self, id: Id) -> Option<Id> {
        let t = self.task(id)?.clone();
        let g = self.add_task("", t.project, t.parent);
        let gt = self.tasks.pop().unwrap();
        let at = self.pos(id)?;
        self.tasks.insert(at, gt);
        let tm = self.task_mut(id)?;
        tm.parent = Some(g);
        Some(g)
    }

    /// Ungroup: the group's children take its place and the group row goes away.
    pub fn ungroup(&mut self, id: Id) -> bool {
        let Some(g) = self.task(id).cloned() else { return false };
        if !self.tasks.iter().any(|t| t.parent == Some(id)) {
            return false;
        }
        for t in self.tasks.iter_mut().filter(|t| t.parent == Some(id)) {
            t.parent = g.parent;
            t.modified = now();
        }
        self.tasks.retain(|t| t.id != id);
        self.forget(id);
        true
    }

    /// Duplicate (with sub-actions), placed right after the source item.
    pub fn duplicate(&mut self, id: Id) -> Option<Id> {
        let root = self.task(id)?.clone();
        let mut map: HashMap<Id, Id> = HashMap::new();
        let mut copies = vec![];
        let mut queue = vec![root.id];
        while let Some(cur) = queue.pop() {
            let t = self.task(cur)?.clone();
            let nid = self.new_id();
            map.insert(cur, nid);
            let mut c = t.clone();
            c.id = nid;
            c.added = now();
            c.modified = c.added;
            copies.push(c);
            queue.extend(self.tasks.iter().filter(|x| x.parent == Some(cur)).map(|x| x.id));
        }
        for c in copies.iter_mut() {
            if c.id != map[&root.id] {
                c.parent = c.parent.and_then(|p| map.get(&p).copied());
            }
        }
        let at = self.pos(id)? + 1;
        for (k, c) in copies.into_iter().enumerate() {
            self.tasks.insert(at + k, c);
        }
        Some(map[&root.id])
    }

    /// Convert Action to Project: the action's sub-actions move into a new project of the same name.
    pub fn convert_to_project(&mut self, id: Id) -> Option<Id> {
        let t = self.task(id)?.clone();
        let p = self.add_project(if t.name.is_empty() { "New Project" } else { &t.name }, if t.sequential { ProjectKind::Sequential } else { ProjectKind::Parallel });
        let folder = t.project.and_then(|x| self.project(x)).map(|q| q.folder.clone()).unwrap_or_default();
        if let Some(pr) = self.project_mut(p) {
            pr.note = t.note.clone();
            pr.flagged = t.flagged;
            pr.folder = folder;
            pr.linear = t.linear.clone();
        }
        let kids: Vec<Id> = self.tasks.iter().filter(|x| x.parent == Some(id)).map(|x| x.id).collect();
        for k in kids {
            self.set_project(k, Some(p));
        }
        self.tasks.retain(|x| x.id != id);
        self.forget(id);
        Some(p)
    }

    /// Next occurrence of a repeating task (manual, "Repeat"); None when it shouldn't repeat again.
    pub fn next_occurrence(&self, t: &Task, at: Dt) -> Option<Task> {
        let r = t.repeat?;
        if matches!(r.end_after, Some(0 | 1)) {
            return None;
        }
        let mut next = t.clone();
        next.completed = None;
        next.dropped = None;
        next.added = at;
        next.modified = at;
        next.repeat = Some(Repeat { end_after: r.end_after.map(|n| n - 1), ..r });
        let st = &self.settings;
        let (base, default_time) = match r.based_on {
            BasedOn::Defer => (t.defer, st.default_defer_time),
            BasedOn::Planned => (t.planned, st.default_planned_time),
            BasedOn::Due => (t.due, st.default_due_time),
        };
        // Base falls back to whichever date is set, so date relationships are kept.
        let base = base.or(t.due).or(t.planned).or(t.defer);
        let new_base = match r.schedule {
            RepeatSchedule::Regularly => {
                let Some(b) = base else { return Some(next) };
                let mut n = r.add(b);
                if r.catch_up {
                    while n <= at {
                        n = r.add(n);
                    }
                }
                n
            }
            RepeatSchedule::FromCompletion => r.add(r.rounded(at, base.map(|b| b.time()).unwrap_or(default_time))),
        };
        match base {
            Some(_) if r.schedule == RepeatSchedule::Regularly && !r.catch_up => {
                // Same calendar step for each date (keeps "10th, 12th, 15th of the month").
                next.defer = t.defer.map(|d| r.add(d));
                next.planned = t.planned.map(|d| r.add(d));
                next.due = t.due.map(|d| r.add(d));
            }
            Some(b) => {
                let delta = new_base - b;
                next.defer = t.defer.map(|d| d + delta);
                next.planned = t.planned.map(|d| d + delta);
                next.due = t.due.map(|d| d + delta);
            }
            None => match r.based_on {
                BasedOn::Defer => next.defer = Some(new_base),
                BasedOn::Planned => next.planned = Some(new_base),
                BasedOn::Due => next.due = Some(new_base),
            },
        }
        Some(next)
    }

    /// Completing or dropping a repeating item creates the next occurrence first (manual, "Catch Up Automatically").
    fn resolve(&mut self, id: Id, at: Dt, dropped: bool, repeat: bool) {
        let Some(t) = self.task(id).cloned() else { return };
        if t.completed.is_some() || t.dropped.is_some() {
            return;
        }
        if let Some(mut next) = self.next_occurrence(&t, at).filter(|_| repeat) {
            next.id = self.new_id();
            let pos = self.tasks.iter().position(|x| x.id == id).unwrap_or(0);
            self.tasks.insert(pos + 1, next);
        }
        if let Some(t) = self.task_mut(id) {
            if dropped { t.dropped = Some(at) } else { t.completed = Some(at) }
            t.repeat = None;
            t.modified = at;
        }
        // "Complete with last action": finishing the last open child completes the group.
        if !dropped {
            if let Some(p) = t.parent.and_then(|p| self.task(p)).filter(|p| p.complete_with_last && p.completed.is_none() && p.dropped.is_none()).map(|p| p.id) {
                let open = self.tasks.iter().any(|c| c.parent == Some(p) && c.completed.is_none() && c.dropped.is_none());
                if !open {
                    self.resolve(p, at, false, true);
                }
            }
        }
    }

    /// Clock in (org-mode C-c C-x C-i): any running clock is clocked out first, so one runs at a time.
    /// Clocking changes the action (its `modified` moves), so sync — newest wins per record — carries a clock started on
    /// one device to the others (it didn't: the person's phone clock never showed on the Mac).
    pub fn clock_in(&mut self, id: Id, at: Dt, pomodoro: bool) {
        self.clock_out(at);
        if let Some(t) = self.task_mut(id) {
            t.clock.push(Clock { start: at, end: None, pomodoro });
            t.modified = at;
        }
    }

    /// Clock out whatever is running; entries under a minute are dropped like org's zero-length clocks.
    pub fn clock_out(&mut self, at: Dt) -> Option<Id> {
        let mut who = None;
        for t in self.tasks.iter_mut() {
            if let Some(c) = t.clock.iter_mut().find(|c| c.end.is_none()) {
                c.end = Some(at);
                who = Some(t.id);
                t.modified = at;
            }
            t.clock.retain(|c| c.end.is_none_or(|e| (e - c.start).num_seconds() >= 60));
        }
        who
    }

    pub fn active_clock(&self) -> Option<(Id, Dt)> {
        self.tasks.iter().find_map(|t| t.clock.iter().find(|c| c.end.is_none()).map(|c| (t.id, c.start)))
    }

    /// Minutes clocked on a task, optionally including its sub-actions (org rolls clocks up).
    pub fn clocked_minutes(&self, id: Id, with_children: bool, now: Dt) -> i64 {
        let own: i64 = self.task(id).map(|t| t.clock.iter().map(|c| c.minutes(now)).sum()).unwrap_or(0);
        if !with_children {
            return own;
        }
        own + self.tasks.iter().filter(|t| t.parent == Some(id)).map(|t| self.clocked_minutes(t.id, true, now)).sum::<i64>()
    }

    /// (own flag, inherited from a parent action or the project).
    pub fn effective_flagged(&self, id: Id) -> (bool, bool) {
        let Some(t) = self.task(id) else { return (false, false) };
        let mut inherited = t.project.and_then(|p| self.project(p)).is_some_and(|p| p.flagged);
        let mut cur = t.parent;
        let mut guard = 0;
        while let Some(pid) = cur {
            let Some(p) = self.task(pid) else { break };
            inherited |= p.flagged;
            cur = p.parent;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        (t.flagged, inherited && !t.flagged)
    }

    pub fn complete(&mut self, id: Id, at: Dt) {
        self.resolve(id, at, false, true)
    }
    /// Drop and skip to the next occurrence ("skip this occurrence").
    pub fn drop_task(&mut self, id: Id, at: Dt) {
        self.resolve(id, at, true, true)
    }
    /// Drop without a next occurrence ("drop the item completely").
    pub fn drop_completely(&mut self, id: Id, at: Dt) {
        self.resolve(id, at, true, false)
    }
    /// Back to active (clears completed / dropped).
    pub fn reactivate(&mut self, id: Id, at: Dt) {
        if let Some(t) = self.task_mut(id) {
            t.completed = None;
            t.dropped = None;
            t.modified = at;
        }
    }

    /// Manual, "Project Status": completing a project marks its unfinished actions complete.
    pub fn set_project_status(&mut self, id: Id, status: ProjectStatus, at: Dt) {
        if status == ProjectStatus::Completed {
            for t in self.tasks.iter_mut().filter(|t| t.project == Some(id) && t.completed.is_none() && t.dropped.is_none()) {
                t.completed = Some(at);
                t.modified = at;
            }
        }
        if let Some(p) = self.project_mut(id) {
            p.status = status;
            p.modified = at;
        }
    }

    pub fn mark_reviewed(&mut self, project: Id, at: Dt) {
        if let Some(p) = self.project_mut(project) {
            p.last_review = Some(at);
            p.next_review = at.date() + Duration::days(p.review_every_days.max(1) as i64);
            p.modified = at;
        }
    }

    /// Last moment that counts as due soon: the start of today plus N days (`due_soon_days`), minus 1 second.
    /// Due on or before it = due soon.
    pub fn due_soon_cutoff(&self, now: Dt) -> Dt {
        (now.date() + Duration::days(self.settings.due_soon_days as i64)).and_time(NaiveTime::MIN) - Duration::seconds(1)
    }

    pub fn derive(&self, now: Dt) -> Derived {
        let idx: HashMap<Id, usize> = self.tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        let mut children: HashMap<Option<Id>, Vec<Id>> = HashMap::new();
        for t in &self.tasks {
            children.entry(t.parent).or_default().push(t.id);
        }
        let projects: HashMap<Id, &Project> = self.projects.iter().map(|p| (p.id, p)).collect();
        let dropped_folders: Vec<&str> = self.folders.iter().filter(|f| f.dropped.is_some()).map(|f| f.name.as_str()).collect();
        let tag_hold: HashMap<Id, bool> = self.tags.iter().map(|t| (t.id, t.on_hold)).collect();
        let cutoff = self.due_soon_cutoff(now);

        // Walk top-down so parents are resolved before children.
        struct Eff {
            due: Option<Dt>,
            defer: Option<Dt>,
            done: Option<Status>,
            seq_blocked: bool,
        }
        let mut eff_due: HashMap<Id, Dt> = HashMap::new();
        let mut eff_defer: HashMap<Id, Dt> = HashMap::new();
        let mut eff: HashMap<Id, Eff> = HashMap::new();
        let mut order: Vec<Id> = Vec::with_capacity(self.tasks.len());
        let mut stack: Vec<Id> = children.get(&None).map(|v| v.iter().rev().copied().collect()).unwrap_or_default();
        while let Some(id) = stack.pop() {
            order.push(id);
            if let Some(c) = children.get(&Some(id)) {
                stack.extend(c.iter().rev());
            }
        }
        for &id in &order {
            let t = &self.tasks[idx[&id]];
            let parent = t.parent.and_then(|p| eff.get(&p));
            let own_done = if t.completed.is_some() {
                Some(Status::Completed)
            } else if t.dropped.is_some() {
                Some(Status::Dropped)
            } else {
                None
            };
            let proj_done = t.project.and_then(|p| projects.get(&p)).and_then(|p| match p.status {
                ProjectStatus::Completed => Some(Status::Completed),
                ProjectStatus::Dropped => Some(Status::Dropped),
                _ if dropped_folders.iter().any(|d| in_folder(&p.folder, d)) => Some(Status::Dropped), // in a dropped folder
                _ => None,
            });
            let done = own_done.or(parent.and_then(|p| p.done)).or(proj_done);
            let due = match (t.due, parent.and_then(|p| p.due)) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let defer = match (t.defer, parent.and_then(|p| p.defer)) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            // Sequential container: blocked when an earlier sibling is still remaining.
            let seq_container = match t.parent {
                Some(p) => self.tasks[idx[&p]].sequential,
                None => t.project.and_then(|p| projects.get(&p)).map(|p| p.kind == ProjectKind::Sequential).unwrap_or(false),
            };
            let mut seq_blocked = parent.map(|p| p.seq_blocked).unwrap_or(false);
            if seq_container && !seq_blocked {
                let sibs = children.get(&t.parent).map(|v| v.as_slice()).unwrap_or(&[]);
                for &s in sibs {
                    if s == id {
                        break;
                    }
                    let st = &self.tasks[idx[&s]];
                    if st.parent == t.parent && st.project == t.project && st.completed.is_none() && st.dropped.is_none() {
                        seq_blocked = true;
                        break;
                    }
                }
            }
            if let Some(x) = due {
                eff_due.insert(id, x);
            }
            if let Some(x) = defer {
                eff_defer.insert(id, x);
            }
            eff.insert(id, Eff { due, defer, done, seq_blocked });
        }

        let mut status: HashMap<Id, Status> = HashMap::with_capacity(order.len());
        let mut pre_next: Vec<(Id, Status)> = Vec::with_capacity(order.len());
        for &id in &order {
            let t = &self.tasks[idx[&id]];
            let e = &eff[&id];
            let in_inbox = t.project.is_none();
            let container_active = t.project.and_then(|p| projects.get(&p)).map(|p| p.status == ProjectStatus::Active).unwrap_or(false);
            let has_remaining_children = children
                .get(&Some(id))
                .map(|c| c.iter().any(|c| {
                    let ct = &self.tasks[idx[c]];
                    ct.completed.is_none() && ct.dropped.is_none()
                }))
                .unwrap_or(false);
            let on_hold_tag = t.tags.iter().any(|g| tag_hold.get(g).copied().unwrap_or(false));
            let s = if let Some(d) = e.done {
                d
            } else if !container_active && !in_inbox {
                Status::Blocked
            } else if e.due.is_some_and(|d| d < now) {
                Status::Overdue
            } else if e.defer.is_some_and(|d| d > now) {
                Status::Blocked
            } else if e.due.is_some_and(|d| d <= cutoff) {
                Status::DueSoon
            } else if e.seq_blocked || has_remaining_children || on_hold_tag {
                Status::Blocked
            } else {
                Status::Available
            };
            pre_next.push((id, s));
        }
        // Next = first available leaf of each project in outline order; in single-actions lists every
        // available action is "first available" (manual, "Projects are distinguished by their type").
        let mut seen_project: std::collections::HashSet<Id> = Default::default();
        for (id, s) in pre_next {
            let t = &self.tasks[idx[&id]];
            let mut s = s;
            if let Some(p) = t.project.and_then(|p| projects.get(&p)) {
                if s == Status::Available && (p.kind == ProjectKind::SingleActions || seen_project.insert(p.id)) {
                    s = Status::Next;
                }
            }
            status.insert(id, s);
        }
        Derived { status, children, idx, eff_due, eff_defer }
    }

    /// Neutral sample data (no content from the person's own database).
    /// The sample database (screenshots and tests; people can add it to an empty database from Settings).
    pub fn sample(now: Dt) -> Store {
        let mut s = Store { next_id: 0, ..Default::default() };
        s.fill_sample(now);
        s
    }

    /// Adds the sample projects, tags and actions, minting IDs from this device's range.
    pub fn fill_sample(&mut self, now: Dt) {
        let s = self;
        let day = |d: i64, h: u32| (now.date() + Duration::days(d)).and_hms_opt(h, 0, 0).unwrap();
        for (n, hold) in [("Home", false), ("Office", false), ("Phone", false), ("Errands", false), ("Waiting", true)] {
            let id = s.new_id();
            s.tags.push(Tag { id, name: n.into(), on_hold: hold, place: None, parent: None, modified: None });
        }
        let tag = |s: &Store, n: &str| s.tag_by_name(n).unwrap();
        // Sample places for Nearby (a neutral downtown grid, not anyone's real address).
        for (n, label, address, lat, lon) in [
            ("Errands", "Main Street shops", "SW 5th Ave & Alder St\nPortland OR 97204\nUnited States", 45.5231, -122.6765),
            ("Office", "Downtown office", "SW Broadway & Salmon St\nPortland OR 97205\nUnited States", 45.5152, -122.6784),
        ] {
            let id = s.tag_by_name(n).unwrap();
            s.tags.iter_mut().find(|t| t.id == id).unwrap().place = Some(Place { label: label.into(), address: address.into(), lat, lon, ..Place::default() });
        }
        s.settings.here = Some(Place { label: "Home".into(), lat: 45.5202, lon: -122.6742, ..Place::default() });
        let reno = s.add_project("Paint the spare room", ProjectKind::Sequential);
        let offsite = s.add_project("Plan team offsite", ProjectKind::Parallel);
        let errands = s.add_project("Errands", ProjectKind::SingleActions);
        let lang = s.add_project("Learn a language", ProjectKind::Parallel);
        s.project_mut(lang).unwrap().status = ProjectStatus::OnHold;
        s.project_mut(offsite).unwrap().next_review = now.date() - Duration::days(1);
        for p in [reno, offsite] {
            s.project_mut(p).unwrap().folder = "Personal".into();
        }
        s.project_mut(offsite).unwrap().folder = "Work".into();

        for n in [
            "Call the dentist about a cleaning",
            "Look into a standing desk",
            "Read the article saved last week",
            "Back up the laptop",
            "Sort the photo library",
            "Pick a birthday gift",
            "Renew library card",
        ] {
            s.add_task(n, None, None);
        }
        if let Some(t) = s.tasks.iter_mut().find(|t| t.name == "Read the article saved last week") {
            t.note = "**Why:** a friend recommended it.\n\n- [ ] skim the intro\n- [ ] write down *one* idea".into();
        }
        let g = s.add_task("Tidy the garage", None, None);
        s.add_task("Donate old paint cans", None, Some(g));
        s.add_task("Hang the bike rack", None, Some(g));
        if let Some(t) = s.task_mut(g) {
            t.note = "Saturday morning, before it gets hot.".into();
        }

        for n in ["Choose a paint colour", "Buy rollers and tape", "Move the furniture out", "Paint two coats"] {
            let id = s.add_task(n, Some(reno), None);
            s.task_mut(id).unwrap().tags = vec![tag(s, "Home")];
        }
        let a = s.add_task("Book the venue", Some(offsite), None);
        s.task_mut(a).unwrap().due = Some(day(1, 17));
        s.task_mut(a).unwrap().flagged = true;
        s.task_mut(a).unwrap().tags = vec![tag(s, "Phone")];
        let b = s.add_task("Send the agenda draft", Some(offsite), None);
        s.task_mut(b).unwrap().due = Some(day(-1, 17));
        s.task_mut(b).unwrap().tags = vec![tag(s, "Office")];
        let c = s.add_task("Collect dietary needs", Some(offsite), None);
        s.task_mut(c).unwrap().defer = Some(day(3, 0));
        s.task_mut(c).unwrap().planned = Some(day(3, 9));
        let d = s.add_task("Hear back about the budget", Some(offsite), None);
        s.task_mut(d).unwrap().tags = vec![tag(s, "Waiting")];
        for (n, due) in [("Pick up dry cleaning", Some(day(0, 17))), ("Return the library books", Some(day(5, 17))), ("Buy stamps", None)] {
            let id = s.add_task(n, Some(errands), None);
            s.task_mut(id).unwrap().due = due;
            s.task_mut(id).unwrap().tags = vec![tag(s, "Errands")];
        }
        let w = s.add_task("Water the plants", Some(errands), None);
        s.task_mut(w).unwrap().repeat = Some(Repeat { every: 3, schedule: RepeatSchedule::FromCompletion, ..Repeat::default() });
        s.task_mut(w).unwrap().due = Some(day(2, 17));
        s.task_mut(w).unwrap().flagged = true;
        s.add_task("Practise vocabulary for 15 minutes", Some(lang), None);
        // Sample clock history over three weeks (deterministic), for the Stats view.
        let worked: Vec<Id> = ["Choose a paint colour", "Book the venue", "Send the agenda draft", "Collect dietary needs", "Read the article saved last week", "Back up the laptop", "Pick up dry cleaning", "Practise vocabulary for 15 minutes"]
            .iter()
            .filter_map(|n| s.tasks.iter().find(|t| t.name == *n).map(|t| t.id))
            .collect();
        let mut seed: u64 = 7;
        let mut rnd = |m: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        for day in 1..22 {
            let date = now.date() - Duration::days(day);
            let sessions = 1 + rnd(4);
            let mut hour = 8 + rnd(3) as u32;
            for _ in 0..sessions {
                let id = worked[rnd(worked.len() as u64) as usize];
                let mins = 15 + rnd(80) as i64;
                let start = date.and_hms_opt(hour.min(20), (rnd(4) * 15) as u32, 0).unwrap();
                let pomo = rnd(3) == 0;
                let len = if pomo { 25 } else { mins };
                s.task_mut(id).unwrap().clock.push(Clock { start, end: Some(start + Duration::minutes(len)), pomodoro: pomo });
                if pomo {
                    s.pomodoros.push((start + Duration::minutes(25), Some(id)));
                }
                hour += 2;
            }
        }
        let done = s.add_task("Order the paint samples", Some(reno), None);
        s.complete(done, now - Duration::hours(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_notify_loads_old_data_and_round_trips() {
        // a place saved before Round 45 has no `notify` or `address`: it keeps notifying on arrival, as it did
        let old: Place = serde_json::from_str(r#"{"label":"Office","lat":45.5,"lon":-122.6,"radius_m":null}"#).unwrap();
        assert_eq!(old.notify, Notify::Arriving);
        assert_eq!(old.address, "");
        let p = Place { notify: Notify::Leaving, address: "1 Main St\nPortland OR".into(), ..old };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""notify":"leaving""#), "{json}");
        assert_eq!(serde_json::from_str::<Place>(&json).unwrap(), p);
        assert_eq!(serde_json::from_str::<Notify>(r#""none""#).unwrap(), Notify::None);
        // no address: nothing extra written, so older builds read it unchanged
        assert!(!serde_json::to_string(&Place::default()).unwrap().contains("address"));
    }

    #[test]
    fn place_size_tiers_and_labels() {
        let p = |r: Option<f64>| Place { radius_m: r, ..Place::default() };
        assert_eq!(PLACE_RADII, [200.0, 500.0, 10_000.0]);
        assert_eq!(p(Some(200.0)).tier(), 0);
        assert_eq!(p(Some(500.0)).tier(), 1);
        assert_eq!(p(Some(10_000.0)).tier(), 2);
        assert_eq!(p(None).tier(), 1); // unset = the 1 km default, nearest the medium size
        assert_eq!(p(Some(1000.0)).tier(), 1); // older builds' medium
        assert_eq!(p(Some(4000.0)).tier(), 2);
        assert_eq!(radius_label(0, true), "~650 ft");
        assert_eq!(radius_label(1, true), "~\u{bc} mi");
        assert_eq!(radius_label(2, true), "~6\u{bc} mi");
        assert_eq!(radius_label(1, false), "~500 m");
        assert_eq!(radius_label(2, false), "~10 km");
    }

    #[test]
    fn relocating_a_place_keeps_its_size_and_notification() {
        let old = Place { label: "Old".into(), address: "old".into(), lat: 1.0, lon: 2.0, radius_m: Some(10_000.0), notify: Notify::Leaving };
        let p = Place::relocated(Some(&old), "New".into(), String::new(), 3.0, 4.0);
        assert_eq!((p.label.as_str(), p.address.as_str(), p.lat, p.lon), ("New", "", 3.0, 4.0));
        assert_eq!((p.radius_m, p.notify), (Some(10_000.0), Notify::Leaving));
        let fresh = Place::relocated(None, "X".into(), "a".into(), 0.0, 0.0);
        assert_eq!((fresh.radius_m, fresh.notify), (None, Notify::Arriving));
    }

    #[test]
    fn up_next_and_time_until() {
        let mut s = Store::default();
        let now = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap().and_hms_opt(13, 0, 0).unwrap();
        let a = s.add_task("Write", None, None);
        let b = s.add_task("Call", None, None);
        s.queue_add(a);
        s.queue_add(b);
        s.queue_add(a);
        assert_eq!(s.queue_live(), vec![a, b]);
        s.complete(a, now);
        assert_eq!(s.queue_live(), vec![b]); // finished ones drop out
        s.task_mut(b).unwrap().planned = Some(now + Duration::minutes(50));
        let c = s.add_task("Standup", None, None);
        s.task_mut(c).unwrap().due = Some(now + Duration::minutes(20));
        s.task_mut(a).unwrap().due = Some(now + Duration::minutes(5)); // completed: ignored
        assert_eq!(s.next_timed(now), Some((now + Duration::minutes(20), c)));
        s.task_mut(c).unwrap().due = Some(now + Duration::days(1)); // tomorrow doesn't count
        assert_eq!(s.next_timed(now), Some((now + Duration::minutes(50), b)));
    }

    #[test]
    fn status_sets_are_tag_groups() {
        let mut s = Store::default();
        let set = s.default_status_set().unwrap();
        let stages = s.tag_children(set);
        assert_eq!(stages.iter().map(|g| s.tag(*g).unwrap().name.clone()).collect::<Vec<_>>(), ["Backlog", "To Do", "In Progress", "Waiting for Review"]);
        assert_eq!(s.default_status_set(), Some(set)); // idempotent
        assert_eq!(s.tag_groups(), vec![set]);
        let chores = s.tag_for_path("Chores").unwrap();
        let t = s.add_task("Vacuum", None, None);
        s.task_mut(t).unwrap().tags = vec![chores, stages[0]];
        s.set_stage(t, set, Some(stages[2])); // drag Backlog -> In Progress
        assert_eq!(s.task(t).unwrap().tags, vec![chores, stages[2]]); // other tags kept, one stage
        s.set_stage(t, set, None);
        assert_eq!(s.task(t).unwrap().tags, vec![chores]);
    }

    #[test]
    fn colon_paths_create_and_nest() {
        let mut s = Store::default();
        let acme = s.project_for_path("Work : Clients : Acme").unwrap();
        assert_eq!(s.project(acme).unwrap().folder, "Work : Clients");
        assert_eq!(s.project_path(acme), "Work : Clients : Acme");
        assert_eq!(s.project_for_path("acme"), Some(acme)); // existing name anywhere
        assert_eq!(s.project_for_path("work:clients:ACME"), Some(acme));
        assert_eq!(s.projects.len(), 1);
        let child = s.tag_for_path("Errands : Hardware Store").unwrap();
        let parent = s.tag_by_name("Errands").unwrap();
        assert_eq!(s.tag(child).unwrap().parent, Some(parent));
        assert_eq!(s.tag_path(child), "Errands : Hardware Store");
        assert_eq!(s.tag_for_path("errands:hardware store"), Some(child)); // no duplicate
        assert_eq!(s.tags.len(), 2);
        assert_eq!(s.tag_by_name("Errands : Hardware Store"), Some(child));
        assert_eq!(s.tag_family(parent), vec![parent, child]);
        assert_eq!(s.tag_tree(), vec![(parent, 0), (child, 1)]);
        // most recently used first: the tag on the newest-changed action leads; unused ones follow in their order
        let a = s.add_task("old", None, None);
        let b = s.add_task("new", None, None);
        s.task_mut(a).unwrap().tags = vec![parent];
        s.task_mut(a).unwrap().modified = now() - chrono::Duration::days(2);
        s.task_mut(b).unwrap().tags = vec![child];
        let mut list = vec![(parent, "Errands".to_string()), (child, "Hardware".into()), (999, "Unused".into())];
        s.by_recent_use(&mut list, |t| t.tags.clone());
        assert_eq!(list.iter().map(|x| x.0).collect::<Vec<_>>(), vec![child, parent, 999]);
        // lookup: whole name first, then a unique leaf name; an ambiguous leaf finds nothing
        assert_eq!(s.tag_by_name("errands:hardware store"), Some(child));
        assert_eq!(s.tag_by_name("Hardware Store"), Some(child)); // the only tag with that name
        let other = s.tag_for_path("Home : Hardware Store").unwrap();
        assert_ne!(other, child);
        assert_eq!(s.tag_by_name("Hardware Store"), None); // two now: ask for the whole name
        assert_eq!(s.tag_by_name("Home : Hardware Store"), Some(other));
        assert!(path_matches("Work : Clients : Acme", "cli:ac"));
        assert!(path_matches("Work : Clients : Acme", "acme"));
        assert!(!path_matches("Work : Clients : Acme", "acme:work"));
        assert_eq!(s.tag_for_path(" : "), None);
    }

    #[test]
    fn inspector_sections_move() {
        let mut st = Settings::default();
        let task: Vec<&str> = ["status", "project", "tags", "duration", "dates", "notifications", "completed", "repeat", "note", "time", "linear"].to_vec();
        let shown = |st: &Settings, vis: &[&str]| st.inspector_order().into_iter().filter(|k| vis.contains(&k.as_str())).collect::<Vec<_>>();
        assert_eq!(shown(&st, &task), task); // default order
        st.move_section("linear", SecMove::Top, &task);
        assert_eq!(shown(&st, &task)[0], "linear");
        st.move_section("linear", SecMove::Down, &task);
        assert_eq!(&shown(&st, &task)[..2], ["status", "linear"]);
        // project-only sections keep their slots when the task inspector reorders
        let proj = ["status", "type", "folder", "review", "note", "linear"];
        assert_eq!(shown(&st, &proj), ["status", "type", "folder", "review", "linear", "note"]);
        st.move_section("status", SecMove::Up, &task); // already first: no change
        assert_eq!(shown(&st, &task)[0], "status");
        st.place_section("time", 0, &task); // drag Time to the top
        assert_eq!(&shown(&st, &task)[..2], ["time", "status"]);
        st.place_section("time", 99, &task); // past the end = last
        assert_eq!(shown(&st, &task).last().unwrap(), "time");
        st.inspector_order = vec!["bogus".into(), "note".into()];
        assert_eq!(st.inspector_order()[0], "note");
        assert_eq!(st.inspector_order().len(), INSPECTOR_SECTIONS.len());
    }

    fn at(d: u32, h: u32) -> Dt {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn status_rules() {
        let now = at(2, 12);
        let mut s = Store::default();
        let seq = s.add_project("Seq", ProjectKind::Sequential);
        let a = s.add_task("a", Some(seq), None);
        let b = s.add_task("b", Some(seq), None);
        let inbox = s.add_task("inbox", None, None);
        let overdue = s.add_task("overdue", None, None);
        s.task_mut(overdue).unwrap().due = Some(at(1, 17));
        let soon = s.add_task("soon", None, None);
        s.task_mut(soon).unwrap().due = Some(at(3, 23)); // before start of Nov 4
        let later = s.add_task("later", None, None);
        s.task_mut(later).unwrap().due = Some(at(4, 0)); // cutoff is Nov 3 23:59:59
        let edge = s.add_task("edge", None, None);
        s.task_mut(edge).unwrap().due = Some(at(3, 23) + Duration::minutes(59) + Duration::seconds(59));
        let deferred = s.add_task("deferred", None, None);
        s.task_mut(deferred).unwrap().defer = Some(at(5, 0));
        let d = s.derive(now);
        assert_eq!(d.st(a), Status::Next);
        assert_eq!(d.st(b), Status::Blocked);
        assert_eq!(d.st(inbox), Status::Available);
        assert_eq!(d.st(overdue), Status::Overdue);
        assert_eq!(d.st(soon), Status::DueSoon);
        assert_eq!(d.st(later), Status::Available);
        assert_eq!(d.st(edge), Status::DueSoon);
        assert_eq!(d.st(deferred), Status::Blocked);

        s.complete(a, now);
        let d = s.derive(now);
        assert_eq!(d.st(a), Status::Completed);
        assert_eq!(d.st(b), Status::Next);

        s.project_mut(seq).unwrap().status = ProjectStatus::OnHold;
        assert_eq!(s.derive(now).status[&b], Status::Blocked);
        s.project_mut(seq).unwrap().status = ProjectStatus::Dropped;
        assert_eq!(s.derive(now).status[&b], Status::Dropped);
    }

    #[test]
    fn overdue_beats_defer_and_sequence() {
        let now = at(2, 12);
        let mut s = Store::default();
        let seq = s.add_project("Seq", ProjectKind::Sequential);
        s.add_task("first", Some(seq), None);
        let b = s.add_task("b", Some(seq), None);
        s.task_mut(b).unwrap().due = Some(at(1, 0));
        s.task_mut(b).unwrap().defer = Some(at(9, 0));
        assert_eq!(s.derive(now).status[&b], Status::Overdue);
    }

    #[test]
    fn repeat_regularly_catch_up_and_end() {
        let mut s = Store::default();
        let t = s.add_task("weekly", None, None);
        s.task_mut(t).unwrap().due = Some(at(1, 17));
        s.task_mut(t).unwrap().defer = Some(at(1, 9));
        s.task_mut(t).unwrap().repeat = Some(Repeat { unit: RepeatUnit::Day, catch_up: true, end_after: Some(2), ..Repeat::default() });
        s.drop_task(t, at(10, 12)); // dropping also repeats; catch up skips past occurrences
        let next = s.tasks.iter().find(|x| x.id != t).unwrap().clone();
        assert_eq!(next.due, Some(at(10, 17))); // first occurrence after Nov 10 12:00
        assert_eq!(next.defer, Some(at(10, 9)));
        assert_eq!(next.repeat.unwrap().end_after, Some(1));
        s.complete(next.id, at(11, 12));
        assert_eq!(s.tasks.len(), 2, "end after 2 stops repeating");
        let r = s.add_task("again", None, None);
        s.task_mut(r).unwrap().repeat = Some(Repeat::default());
        s.drop_completely(r, at(11, 12));
        assert_eq!(s.tasks.len(), 3, "drop completely makes no next occurrence");
    }

    #[test]
    fn repeat_from_completion() {
        let mut s = Store::default();
        let t = s.add_task("water", None, None);
        s.task_mut(t).unwrap().due = Some(at(1, 17));
        s.task_mut(t).unwrap().repeat = Some(Repeat { every: 3, schedule: RepeatSchedule::FromCompletion, ..Repeat::default() });
        s.complete(t, at(2, 10));
        assert!(s.task(t).unwrap().completed.is_some());
        let next = s.tasks.iter().find(|x| x.id != t).unwrap();
        assert_eq!(next.due, Some(at(5, 17)));
        assert!(next.completed.is_none() && next.repeat.is_some());
    }

    #[test]
    fn review_and_children() {
        let mut s = Store::default();
        let p = s.add_project("P", ProjectKind::Parallel);
        s.mark_reviewed(p, at(2, 9));
        assert_eq!(s.project(p).unwrap().next_review, NaiveDate::from_ymd_opt(2026, 11, 9).unwrap());
        let g = s.add_task("group", Some(p), None);
        let c = s.add_task("child", None, Some(g));
        assert_eq!(s.task(c).unwrap().project, Some(p));
        let d = s.derive(at(2, 12));
        assert_eq!(d.st(g), Status::Blocked);
        assert_eq!(d.st(c), Status::Next);
        s.set_project(g, None);
        assert_eq!(s.task(c).unwrap().project, None);
        s.delete_task(g);
        assert!(s.tasks.is_empty());
    }

    #[test]
    fn effective_dates_for_reminders() {
        let mut s = Store::default();
        let g = s.add_task("group", None, None);
        s.task_mut(g).unwrap().due = Some(at(5, 17));
        s.task_mut(g).unwrap().defer = Some(at(3, 9));
        let c = s.add_task("child", None, Some(g));
        s.task_mut(c).unwrap().due = Some(at(6, 17)); // later than parent: parent's wins
        let d = s.derive(at(2, 12));
        assert_eq!(d.eff_due[&c], at(5, 17));
        assert_eq!(d.eff_defer[&c], at(3, 9));
    }

    #[test]
    fn outline_editing() {
        let mut s = Store::default();
        let a = s.add_task("a", None, None);
        let b = s.add_task("b", None, None);
        let c = s.add_task("c", None, None);
        assert!(s.move_sibling(c, false));
        assert_eq!(s.sibling_ids(a), vec![a, c, b]);
        assert!(s.indent(c));
        assert_eq!(s.task(c).unwrap().parent, Some(a));
        assert!(s.outdent(c));
        assert_eq!(s.task(c).unwrap().parent, None);
        let g = s.group(b).unwrap();
        assert_eq!(s.task(b).unwrap().parent, Some(g));
        assert!(s.ungroup(g));
        assert!(s.task(g).is_none() && s.task(b).unwrap().parent.is_none());
        s.add_task("child", None, Some(a));
        let dup = s.duplicate(a).unwrap();
        assert_eq!(s.tasks.iter().filter(|t| t.parent == Some(dup)).count(), 1);
        let p = s.convert_to_project(a).unwrap();
        assert!(s.task(a).is_none());
        assert_eq!(s.tasks.iter().filter(|t| t.project == Some(p)).count(), 1);
        // complete with last action
        let grp = s.add_task("grp", None, None);
        s.task_mut(grp).unwrap().complete_with_last = true;
        let k1 = s.add_task("k1", None, Some(grp));
        s.complete(k1, now());
        assert!(s.task(grp).unwrap().completed.is_some());
        // inherited flag
        s.task_mut(dup).unwrap().flagged = true;
        let kid = s.tasks.iter().find(|t| t.parent == Some(dup)).unwrap().id;
        assert_eq!(s.effective_flagged(kid), (false, true));
    }

    #[test]
    fn clocking() {
        let mut s = Store::default();
        let a = s.add_task("a", None, None);
        let b = s.add_task("b", None, Some(a));
        s.clock_in(a, at(2, 9), false);
        s.clock_in(b, at(2, 10), false); // clocks a out
        assert_eq!(s.active_clock(), Some((b, at(2, 10))));
        assert_eq!(s.clock_out(at(2, 11)), Some(b));
        assert_eq!(s.clocked_minutes(a, false, at(2, 12)), 60);
        assert_eq!(s.clocked_minutes(a, true, at(2, 12)), 120);
        s.clock_in(a, at(2, 12), false);
        s.clock_out(at(2, 12)); // zero-length clock is dropped
        assert_eq!(s.task(a).unwrap().clock.len(), 1);
    }

    #[test]
    fn save_roundtrip() {
        let dir = std::env::temp_dir().join(format!("gtdcraft-test-{}", std::process::id()));
        let path = dir.join("db.json");
        let s = Store::sample(at(2, 12));
        s.save(&path).unwrap();
        let l = Store::load(&path).unwrap();
        assert_eq!(l.tasks.len(), s.tasks.len());
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod sample_check {
    use super::*;
    #[test]
    fn new_databases_start_empty() {
        let dir = std::env::temp_dir().join(format!("gtd-empty-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let s = Store::load_or_new(&dir.join("new.json"));
        assert!(s.tasks.is_empty() && s.projects.is_empty() && s.tags.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_samples_keeps_your_own_items_and_syncs_the_delete() {
        let mut s = Store { id_base: 1_000_000, ..Default::default() };
        s.fill_sample(now());
        let mine = s.add_task("My real task", None, None);
        let home = s.tag_by_name("Home").unwrap();
        s.task_mut(mine).unwrap().tags = vec![home]; // a sample-named tag you use stays
        let phone_copy = s.clone();
        let n = s.remove_sample();
        assert!(n > 20, "removed {n}");
        assert_eq!(s.tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["My real task"]);
        assert!(s.projects.is_empty());
        assert_eq!(s.tags.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Home"]);
        assert!(s.pomodoros.is_empty());
        assert_eq!(s.sample_items(), (vec![], vec![], vec![]));
        // another device still has them: merging brings none back, not even their Pomodoros
        let merged = crate::sync::merge(&phone_copy, &s);
        assert_eq!(merged.tasks.len(), 1);
        assert!(merged.projects.is_empty());
        assert!(merged.pomodoros.is_empty());
    }

    #[test]
    fn deleting_a_project_deletes_its_actions_for_every_device() {
        let mut s = Store::sample(now());
        let reno = s.projects.iter().find(|p| p.name == "Paint the spare room").unwrap().id;
        let inside: Vec<Id> = s.tasks.iter().filter(|t| t.project == Some(reno)).map(|t| t.id).collect();
        let other = s.clone();
        s.delete_project(reno);
        assert!(s.project(reno).is_none() && inside.iter().all(|t| s.task(*t).is_none()));
        assert!(s.deleted.contains_key(&reno) && inside.iter().all(|t| s.deleted.contains_key(t)));
        let merged = crate::sync::merge(&other, &s);
        assert!(merged.project(reno).is_none() && inside.iter().all(|t| merged.task(*t).is_none()));
        s.delete_folder("Personal");
        assert!(s.projects.iter().all(|p| p.folder != "Personal"));
    }

    #[test]
    fn folders_can_be_empty_renamed_and_synced() {
        let mut s = Store::sample(now());
        let name = s.add_folder(None);
        assert_eq!(name, "Untitled Folder");
        assert_eq!(s.add_folder(None), "Untitled Folder 2");
        // nothing selected: at the top; after a folder: right after it
        assert_eq!(s.folder_names()[0], "Untitled Folder 2");
        let third = s.add_folder(Some("Personal"));
        let order = s.folder_names();
        assert_eq!(order.iter().position(|n| *n == third), order.iter().position(|n| n == "Personal").map(|i| i + 1));
        s.rename_folder("Untitled Folder", "Someday");
        s.rename_folder("Work", "Job"); // a folder only projects named: gets a record, projects follow
        assert!(s.folder_names().contains(&"Someday".to_string()) && s.folder_names().contains(&"Job".to_string()));
        assert!(s.projects.iter().all(|p| p.folder != "Work"));
        let other = s.clone();
        s.delete_folder("Someday");
        let merged = crate::sync::merge(&other, &s);
        assert!(!merged.folder_names().contains(&"Someday".to_string()), "the delete wins over the other copy");
        assert!(merged.folder_names().contains(&"Untitled Folder 2".to_string()));
    }

    #[test]
    fn folders_nest_rename_and_delete_with_their_children() {
        let mut s = Store::default();
        let p = s.add_project("Acme site", ProjectKind::Parallel);
        s.project_mut(p).unwrap().folder = "Work : Clients".into();
        assert_eq!(s.folder_names(), ["Work", "Work : Clients"]);
        let sub = s.add_subfolder("Work");
        assert_eq!(sub, "Work : Untitled Folder");
        assert_eq!(s.folder_names(), ["Work", "Work : Clients", "Work : Untitled Folder"]);
        s.rename_folder("Work", "Job");
        assert_eq!(s.project(p).unwrap().folder, "Job : Clients");
        assert!(s.folder_names().iter().all(|n| n.starts_with("Job")));
        assert!(!in_folder("Jobs", "Job") && in_folder("Job : Clients", "Job"));
        s.delete_folder("Job");
        assert!(s.folder_names().is_empty() && s.project(p).is_none());
    }

    #[test]
    fn dropping_a_folder_drops_what_is_inside() {
        let mut s = Store::sample(now());
        let offsite = s.projects.iter().find(|p| p.name == "Plan team offsite").unwrap().id;
        s.project_mut(offsite).unwrap().folder = "Work : Events".into();
        let a = s.tasks.iter().find(|t| t.project == Some(offsite)).unwrap().id;
        assert_ne!(s.derive(now()).st(a), Status::Dropped);
        s.set_folder_dropped("Work", true);
        assert_eq!(s.derive(now()).st(a), Status::Dropped);
        s.set_folder_dropped("Work", false);
        assert_ne!(s.derive(now()).st(a), Status::Dropped);
    }

    #[test]
    fn favorites_default_to_the_originals() {
        let ps = default_perspectives();
        let favs: Vec<&str> = ps.iter().filter(|p| p.is_favorite()).map(|p| p.key.as_str()).collect();
        assert_eq!(favs, ["inbox", "forecast", "review", "flagged", "projects", "tags", "nearby"]);
        let mut p = ps[0].clone();
        p.favorite = Some(false);
        assert!(!p.is_favorite());
    }

    #[test]
    fn sample_article_is_open() {
        let s = Store::sample(now());
        let d = s.derive(now());
        let t = s.tasks.iter().find(|t| t.name.starts_with("Read the article")).unwrap();
        assert!(t.completed.is_none(), "completed in sample: {:?}", t.completed);
        assert!(d.st(t.id).remaining());
    }
}
