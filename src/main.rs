//! GTDCraft: a GTD task manager window plus a built-in MCP server (`gtdcraft --mcp`).
mod acpfind;
mod agent;
mod agentui;
mod actions;
mod ai;
mod board;
#[allow(dead_code)] // the EventKit half runs in the gtdcraft-calendar helper binary
mod calendar;
mod systemone;
mod dates;
mod datepick;
mod attachpick;
mod nativetext;
mod dbfile;
mod dbui;
mod editor;
mod fields;
mod focus;
mod keys;
mod linear;
#[cfg(not(target_os = "ios"))]
mod menu;
#[cfg(target_os = "ios")]
#[path = "menu_ios.rs"]
mod menu;
mod mcp;
// TEMPORARY decisions-URL setter. Remove this line with src/decisions_url_set.rs.
mod decisions_url_set;
mod model;
mod notes;
mod places;
mod refs;
mod rules;
mod stats;
mod status;
mod subsystems;
mod sync;
mod theme;
mod webact;

use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    TextFormat, Vec2,
};
use model::*;
use theme::{hue, mix};

fn th() -> theme::Colors {
    theme::c()
}
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

// ---- Look (sampled from the person's screenshots; see MECHANICS.md "Layout") ----
const TOOLBAR_H: f32 = 52.0;
/// Inspector sections each inspector shows (their order comes from Settings).
const TASK_SECTIONS: [&str; 13] = ["status", "project", "tags", "duration", "dates", "notifications", "completed", "repeat", "note", "time", "linear", "attachments", "refs"];
const PROJECT_SECTIONS: [&str; 8] = ["status", "type", "folder", "review", "note", "linear", "attachments", "refs"];
/// Narrower than this (a phone, a slim window): one pane at a time.
const COMPACT_W: f32 = 700.0;
/// Perspective rail width.
const RAIL_W: f32 = 82.0;
/// Rail selection corner radius: 12 on macOS 27+, else 16.
const RAIL_SEL_RADIUS: f32 = 12.0;
const COLUMN_W: f32 = 241.0;
const INSPECTOR_W: f32 = 290.0;
/// The repository the version label's commit link points at.
const REPO_URL: &str = "https://github.com/mattpetters/gtdcraft";

/// "v0.2.0 · abc1234" in the bottom-right corner (version from Cargo.toml, commit stamped by build.rs); a click opens
/// that commit on GitHub.
fn version_label(ctx: &egui::Context) {
    let sha = env!("GTDCRAFT_SHA");
    let text = if sha.is_empty() { format!("v{}", env!("CARGO_PKG_VERSION")) } else { format!("v{} · {sha}", env!("CARGO_PKG_VERSION")) };
    egui::Area::new(egui::Id::new("version-label")).anchor(egui::Align2::RIGHT_BOTTOM, [-10.0, -6.0]).order(egui::Order::Middle).show(ctx, |ui| {
        let r = ui.add(egui::Label::new(egui::RichText::new(text).size(10.5).color(th().dim)).sense(egui::Sense::click()));
        if !sha.is_empty() && r.on_hover_text(format!("Build {} · open this commit on GitHub", env!("GTDCRAFT_BUILD"))).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ctx.open_url(egui::OpenUrl::new_tab(format!("{REPO_URL}/commit/{sha}")));
        }
    });
}
const ROW_H: f32 = 31.0;
const META_H: f32 = 17.0;
const INDENT: f32 = 37.0;
/// Status circle diameter = 23 × base font size ÷ 17.
const BASE_FONT: f32 = 13.5;
const STATUS_CIRCLE_R: f32 = 23.0 * BASE_FONT / 17.0 / 2.0;
/// Seconds before a completed item hides when auto-hide is on.
const HIDE_COMPLETED_DELAY: f64 = 0.25;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    Inbox,
    Forecast,
    Review,
    Flagged,
    Projects,
    Tags,
    Nearby,
    Completed,
    Changed,
    /// Time tracked: charts of clocked time.
    Stats,
    /// Send to Agent: the actions with agent threads (agent.rs), grouped by project.
    Agents,
    /// A custom perspective; the number is from its key "custom-<n>".
    Custom(u32),
}

impl View {
    const ALL: [View; 11] = [
        View::Inbox,
        View::Forecast,
        View::Review,
        View::Flagged,
        View::Projects,
        View::Tags,
        View::Nearby,
        View::Completed,
        View::Changed,
        View::Stats,
        View::Agents,
    ];
    fn key(self) -> String {
        match self {
            View::Custom(n) => format!("custom-{n}"),
            v => format!("{v:?}").to_lowercase(),
        }
    }
    fn from_key(k: &str) -> Option<View> {
        if let Some(n) = k.strip_prefix("custom-") {
            return n.parse().ok().map(View::Custom);
        }
        View::ALL.into_iter().find(|v| v.key() == k)
    }
    /// Manual, "Keyboard Shortcuts": Inbox ⌘1, Projects ⌘2, Tags ⌘3, Forecast ⌘4, Flagged ⌘5, Nearby ⌘6, Review ⌘7.
    fn shortcut(self) -> Option<Key> {
        Some(match self {
            View::Inbox => Key::Num1,
            View::Projects => Key::Num2,
            View::Tags => Key::Num3,
            View::Forecast => Key::Num4,
            View::Flagged => Key::Num5,
            View::Nearby => Key::Num6,
            View::Review => Key::Num7,
            View::Changed => Key::Num8,
            View::Completed => Key::Num9,
            View::Stats => Key::Num0,
            View::Custom(_) | View::Agents => return None,
        })
    }
    fn has_column(self) -> bool {
        !matches!(self, View::Inbox | View::Review | View::Custom(_) | View::Stats | View::Agents)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sel {
    Task(Id),
    Project(Id),
    Tag(Id),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Filter {
    All,
    Project(Id),
    Tag(Option<Id>), // None = Untagged
    Day(NaiveDate),
    Past,
    Future,
}

#[derive(Clone)]
enum Row {
    Header(String),
    /// A Calendar event (index into `Gtd::cal`) shown on a Forecast day.
    Event(usize, NaiveDate),
    /// The day's 24-hour event timeline (busy hours).
    Timeline(NaiveDate),
    Task { id: Id, depth: usize, show_project: bool },
    Project(Id),
}

enum Act {
    Select(Sel),
    /// Copy as List / Copy as List with Context for the clicked row, or the whole
    /// selection when it's one of them.
    Copy(Sel, bool),
    /// iPhone/iPad: a row's commands as a sheet (touch and hold).
    RowSheet(Id),
    /// Send to Agent… (the sync server's agents, agent.rs)
    AgentSend(Id),
    /// The row menu's Attach File… / Attach Folder… (for
    /// projects and tasks only): pick one and add it to the item's Attachments. iPhone/iPad: opens the inspector, whose
    /// Attachments section takes a pasted path.
    Attach(Sel, bool),
    /// ⌘-click: add or remove a row from the selection.
    PickToggle(Id),
    /// ⇧-click: select every row from the current one to this one.
    PickRange(Id),
    ToggleDone(Id),
    ToggleDrop(Id),
    ToggleFlag(Id),
    Collapse(Id),
    CollapseHeader(String),
    Edit(Id),
    ProjectStatus(Id, ProjectStatus),
    /// Delete a whole folder of projects (a project's Delete is `Menu(Sel::Project, Delete)`).
    DeleteFolder(String),
    /// Projects sidebar shortcut menus: a new empty folder (then rename it), rename one, a new project.
    /// A new folder after this one (or after the selected project's folder; at the top when there's none).
    NewFolder(Option<String>),
    RenameFolder(String),
    NewSubfolder(String),
    /// Drop a folder (and what's inside), or make it active again.
    DropFolder(String, bool),
    NewProject,
    Filter(Filter),
    OpenField(Id, fields::Field),
    EditNote(Id),
    /// Ask macOS for Calendar access (Forecast's "Allow Calendar Access").
    AllowCalendar,
    /// Shortcut menu (right-click / Control-click) on a row: select it (unless it's already selected) and run a command.
    Menu(Sel, keys::Action),
    /// Shortcut menu › Schedule (manual: Due Today, Due Tomorrow, Defer until Tomorrow, Plan for Today/Tomorrow).
    Schedule(Id, Schedule),
    /// Shortcut menu › Type on an action group.
    GroupType(Id, bool),
    CompleteWithLast(Id),
    /// Project row shortcut menu › Add Inside: a new action at the end of the project.
    AddToProject(Id),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Schedule {
    DueToday,
    DueTomorrow,
    DeferTomorrow,
    PlanToday,
    PlanTomorrow,
}

struct Gtd {
    path: PathBuf,
    store: Store,
    persist: bool,
    mtime: Option<SystemTime>,
    last_poll: f64,
    dirty_at: Option<f64>,
    view: View,
    back: Vec<View>,
    fwd: Vec<View>,
    filter: Filter,
    sel: Option<Sel>,
    search: String,
    collapsed: HashSet<Id>,
    collapsed_headers: HashSet<String>,
    /// Inbox items given a project this session: they stay in the Inbox until Clean Up; and last frame's Inbox.
    inbox_kept: HashSet<Id>,
    last_inbox: HashSet<Id>,
    session_resolved: HashSet<Id>,
    editing: Option<Id>,
    edit_buf: String,
    edit_focus: bool,
    date_buf: [(Option<Id>, String); 3],
    /// The date field was typed in since it was last saved (kept when focus leaves by Return, a tap elsewhere, or the
    /// keyboard closing).
    date_dirty: [bool; 3],
    /// iPhone/iPad Select mode (⋯ › Select): taps pick rows, and a bar offers commands for all of them.
    select_mode: bool,
    /// A short confirmation shown over the bottom of the window ("Copied 3 items"), and when it appeared.
    toast: Option<(String, Option<f64>)>,
    /// One pane at a time (iPhone, narrow windows): the list or the inspector as a page.
    paged: bool,
    /// iPhone: the note being edited in its row (tapping a selected row's note), and its text.
    note_inline: Option<(Id, String, bool)>,
    /// iPhone/iPad: the action whose command sheet is open.
    row_sheet: Option<Id>,
    /// iPhone portrait: the sidebar's vertical list of projects or tags as a page (the chips' ☰).
    list_page: bool,
    /// Send to Agent: the server's agents, the threads, and the open sheet or thread (agentui.rs).
    agentui: agentui::State,
    show_sidebar: bool,
    show_inspector: bool,
    show_settings: bool,
    quick: Option<QuickEntry>,
    drop_prompt: Option<Id>,
    hide_queue: Vec<(Id, f64)>,
    time: f64,
    shot: Option<PathBuf>,
    frames: u32,
    shot_sent: bool,
    themes: Vec<theme::Theme>,
    theme_errors: Vec<String>,
    applied_theme: String,
    notes: notes::Notes,
    note_edit: Option<Id>,
    map: places::MapView,
    geo_tx: std::sync::mpsc::Sender<GeoMsg>,
    geo_rx: std::sync::mpsc::Receiver<GeoMsg>,
    geo_msg: String,
    place_query: String,
    /// The tag inspector's place finder: (tag, addresses only) while Search or Address is open.
    place_find: Option<(Id, bool)>,
    /// Search results to pick from, for this tag.
    place_results: Option<(Id, Vec<places::Found>)>,
    settings_tab: u8,
    /// Reminders fire for times after this (no flood of old ones at launch).
    notified_until: Dt,
    /// iPhone/iPad: a fingerprint of the reminders handed to iOS, so they're rescheduled only when they change.
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    alarm_hash: u64,
    /// Last (view, rail height) the rail scrolled into view: shortcuts can pick an item below the fold,
    /// and a resize can push it out again.
    rail_seen: Option<(View, u32)>,
    // undo / redo (whole-store snapshots, coalesced within a second)
    undo: Vec<Store>,
    redo: Vec<Store>,
    snapshot: Option<Store>,
    undo_at: f64,
    /// Settings › Keyboard: recording a shortcut for this action (true = add, false = replace).
    recording: Option<(keys::Action, bool)>,
    /// Quick-edit shortcut asked the inspector to focus this field.
    focus_field: Option<keys::Action>,
    view_opts: bool,
    eye_rect: Rect,
    quick_open: Option<String>,
    focus_search: bool,
    /// Perspective editor open for this perspective key.
    editor: Option<String>,
    ai_text: String,
    ai_rx: Option<std::sync::mpsc::Receiver<Result<ai::Draft, String>>>,
    ai_msg: String,
    linear_rx: Option<std::sync::mpsc::Receiver<(String, Result<linear::Issue, String>)>>,
    linear_busy: HashSet<String>,
    linear_msg: String,
    link_buf: String,
    linear_tx: Option<std::sync::mpsc::Sender<(String, Result<linear::Issue, String>)>>,
    /// References: GitHub details being fetched (by URL), ones that failed (not retried this run), the paste field.
    refs_rx: Option<std::sync::mpsc::Receiver<(String, Result<refs::Ref, String>)>>,
    refs_tx: Option<std::sync::mpsc::Sender<(String, Result<refs::Ref, String>)>>,
    refs_busy: HashSet<String>,
    refs_failed: HashSet<String>,
    refs_buf: String,
    refs_msg: String,
    /// A repo reference's open issues and PRs, as fetched (repo url → list), and the fetches in flight.
    refs_lists: std::collections::HashMap<String, Vec<refs::Ref>>,
    refs_list_rx: Option<std::sync::mpsc::Receiver<RefsList>>,
    refs_list_tx: Option<std::sync::mpsc::Sender<RefsList>>,
    /// Attachments: paste field and status message.
    att_buf: String,
    att_msg: String,
    view_after_editor: Option<View>,
    pomo: Option<stats::Pomo>,
    /// The running clock as last seen, to notice one started or stopped elsewhere (`follow_focus`).
    last_clock: Option<(Id, Dt)>,
    stats_days: u32,
    /// Test hook: key presses injected on frame 10 (`--press cmd+n`), then text typed on frame 14 (`--type Work : Acme`).
    press: Vec<keys::Binding>,
    typing: Option<String>,
    /// Test hook: the Stats page's scroll offset in points (`--scroll 1100`), so screenshots show what's below the fold.
    scroll: f32,
    /// Test hook (`--landscape`, iPhone): turn the app sideways on launch, since the simulator can't be rotated from
    /// the command line.
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    landscape: bool,
    /// Field editor popover on the selected row.
    field_open: Option<(Id, fields::Field)>,
    field_buf: String,
    field_focus: bool,
    /// Where each strip field was drawn last frame (popover anchors).
    strip_rects: std::collections::HashMap<(Id, fields::Field), Rect>,
    /// Menu bar, database files and git history.
    db: dbui::DbUi,
    /// Other selected actions besides `sel` (⌘-click / ⇧-click); edits and status changes apply to all of them.
    picked: Vec<Id>,
    /// Settings › Agent: ACP agents found on this Mac (None until the background check finishes).
    acp_found: Option<Vec<acpfind::Found>>,
    /// Inspector shows its sections as a drag-to-reorder list.
    arrange_sections: bool,
    /// Sync server: when the next round trip is due (app time), the one in flight, and the last result.
    sync_due: Option<f64>,
    sync_rx: Option<std::sync::mpsc::Receiver<Result<(Store, String), String>>>,
    /// The server's revision of this database at the last sync, the pending check, and when it was last asked.
    last_rev: String,
    rev_rx: Option<std::sync::mpsc::Receiver<String>>,
    rev_at: f64,
    /// Syncing waits: this isn't the database this device syncs (sync.rs `same_database`); and the banner was dismissed.
    sync_paused: bool,
    sync_banner_hidden: bool,
    /// Failed syncs in a row (the "Can't sync" banner after two), and that banner dismissed.
    sync_failures: u32,
    sync_fail_hidden: bool,
    sync_msg: String,
    /// Bottom-bar lights and Settings › Status (status.rs).
    status: status::Monitor,
    /// `Store::layout_fingerprint` as last saved or adopted: a different one on save means the layout changed here.
    layout_print: u64,
    /// The same for the connections that sync (`Store::connections_fingerprint`).
    connections_print: u64,
    queue_print: u64,
    /// iPhone portrait: the search field is showing in the top bar; the perspectives page is open.
    phone_search: bool,
    perspectives_page: bool,
    /// iPhone portrait: the list's scroll offset last frame, and whether the bottom bar has shrunk.
    list_scroll: f32,
    bar_collapsed: bool,
    /// Folder being renamed: (its name now, the text field).
    renaming_folder: Option<(String, String)>,
    /// A setup-sync link that was opened, waiting for "Replace Settings" (server, token).
    pending_sync_link: Option<(String, String)>,
    /// the database identity the pairing link carried
    pending_sync_db: Option<String>,
    /// The Focus window (timer, current action, Up Next) and whether it floats above other apps.
    focus_win: bool,
    focus_on_top: bool,
    /// The compact (phone) layout has been set up once.
    compact_seen: bool,
    /// Web activity from the Chrome extension: (when it was read, the records).
    web: (f64, Vec<webact::Rec>),
    /// Today's Calendar events (read in the background every 5 minutes when turned on).
    cal: Vec<calendar::Event>,
    cal_at: Option<Dt>,
    cal_rx: Option<std::sync::mpsc::Receiver<CalendarRead>>,
    cal_msg: String,
    cal_ask: bool,
    /// The one automatic ask per launch has happened (see poll_calendar).
    cal_auto_asked: bool,
    /// Whether the window had focus last frame (permission is re-read on becoming active).
    was_focused: bool,
    cal_access: calendar::Access,
    /// Settings › General › Calendar: the Mac's calendars (read when that section is shown).
    cal_list: Option<Vec<calendar::CalendarInfo>>,
    acp_rx: Option<std::sync::mpsc::Receiver<Vec<acpfind::Found>>>,
    /// Settings › Agent › Decisions › Test: the last result, and the call in flight.
    decide_test: Option<Result<String, String>>,
    decide_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
}

/// One background Calendar read: the permission, the events (or why not) and the calendars.
type CalendarRead = (calendar::Access, Result<Vec<calendar::Event>, String>, Vec<calendar::CalendarInfo>);

/// Drag payload: a project being dragged in the Projects sidebar.
#[derive(Clone, Copy)]
struct ProjectDrag(Id);

/// Quick Entry: a title, one line of fields — Project, Tags, Due, Repeat — then
/// Add Note, with Cancel and Save along the bottom of the card.
struct QuickEntry {
    name: String,
    project: Option<Id>,
    tags: Vec<Id>,
    /// Typed due date ("tomorrow 5pm", "fri", "nov 15"), parsed on Save.
    due: String,
    repeat: Option<Repeat>,
    note: String,
    flagged: bool,
    show_note: bool,
    /// Which field list is open under the fields: "project", "tags", "due" or "repeat".
    open: Option<&'static str>,
    /// Focus the title the first time the card is drawn.
    focus: bool,
}
impl Default for QuickEntry {
    fn default() -> Self {
        QuickEntry { name: String::new(), project: None, tags: vec![], due: String::new(), repeat: None, note: String::new(), flagged: false, show_note: false, open: None, focus: true }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum GeoFor {
    Here,
    Tag(Id),
}
/// Answers from the background place lookups.
enum GeoMsg {
    /// Search results: for "My location" the first is taken; for a tag they're listed to pick from.
    Found(GeoFor, Result<(&'static str, Vec<places::Found>), String>),
    /// The street address at a tag's new spot (lat, lon), for Here and pins.
    Address(Id, f64, f64, String),
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let path = arg("--db").map(PathBuf::from).unwrap_or_else(default_path);
    // `--sync-server [--listen 0.0.0.0:8765] [--data DIR] [--clef-url URL] --token SECRET` (or GTDCRAFT_SYNC_TOKEN): run the sync server
    if args.iter().any(|a| a == "--sync-server") {
        let token = arg("--token").or_else(|| std::env::var("GTDCRAFT_SYNC_TOKEN").ok()).unwrap_or_default();
        if token.trim().is_empty() {
            eprintln!("gtdcraft --sync-server needs --token <secret> (or GTDCRAFT_SYNC_TOKEN); devices use the same token.");
            std::process::exit(2);
        }
        let cfg = sync::ServerConfig {
            listen: arg("--listen").unwrap_or_else(|| "0.0.0.0:8765".into()),
            data: arg("--data").map(PathBuf::from).unwrap_or_else(|| dbfile::app_dir().join("sync-server")),
            token,
            clef: arg("--clef-url").unwrap_or_else(|| systemone::LOCAL.into()),
            agents: args.iter().any(|a| a == "--agents"),
        };
        if let Err(e) = sync::serve(cfg) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--mcp") {
        mcp::run(&path);
        return Ok(());
    }
    // `--remove-sample`: Settings › Remove Sample Items from a script (deletes travel to other devices at the next sync)
    if args.iter().any(|a| a == "--remove-sample") {
        let mut store = match model::Store::load(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Couldn't read {}: {e}", path.display());
                std::process::exit(2);
            }
        };
        let n = store.remove_sample();
        if let Err(e) = store.save(&path) {
            eprintln!("Couldn't save {}: {e}", path.display());
            std::process::exit(1);
        }
        println!("Removed {n} sample items from {}", path.display());
        return Ok(());
    }
    // `--sync-now`: one round trip with the sync server set in Settings › Sync, then exit (handy from a script,
    // and the same code path the app uses on quit)
    if args.iter().any(|a| a == "--sync-now") {
        let mut store = match model::Store::load(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Couldn't read {}: {e}", path.display());
                std::process::exit(2);
            }
        };
        let url = store.settings.sync_url.trim().to_string();
        if url.is_empty() {
            eprintln!("No sync server set: put one in Settings › Sync (http://<this-mac>.local:8765) first.");
            std::process::exit(2);
        }
        match sync::sync_with(&url, &store.settings.sync_token, &store, 30) {
            Ok(remote) => {
                let id_base = store.id_base;
                store = sync::merge(&remote, &store);
                store.id_base = id_base;
                if let Err(e) = store.save(&path) {
                    eprintln!("Synced, but couldn't save {}: {e}", path.display());
                    std::process::exit(1);
                }
                println!(
                    "Synced with {url}: {} tasks, {} projects, {} tags",
                    store.tasks.len(),
                    store.projects.len(),
                    store.tags.len()
                );
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // `--find-place coffee [--near 45.52,-122.68]`: run the tag-location search once and print what it finds
    // (checks Apple Maps from the command line; MapKit answers on the main queue, so the main thread spins it)
    if let Some(q) = arg("--find-place") {
        let near = arg("--near").and_then(|s| s.split_once(',').and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?))));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let found = places::search(&q, near, false);
            let addr = near.and_then(|(lat, lon)| places::reverse(lat, lon));
            let _ = tx.send((found, addr));
        });
        let (found, addr) = loop {
            if let Ok(r) = rx.try_recv() {
                break r;
            }
            #[cfg(target_os = "macos")]
            objc2_foundation::NSRunLoop::mainRunLoop().runUntilDate(&objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.05));
            #[cfg(not(target_os = "macos"))]
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        if let Some(a) = addr {
            println!("near: {}", a.replace('\n', ", "));
        }
        match found {
            Ok((source, v)) => {
                println!("{}:", if source == "Apple Maps" { "apple" } else { "osm" });
                for f in v {
                    println!("  {} | {} | {:.5},{:.5}", f.name, f.address.replace('\n', ", "), f.lat, f.lon);
                }
            }
            Err(e) => println!("error: {e}"),
        }
        return Ok(());
    }
    let shot = arg("--screenshot").map(PathBuf::from);
    let view = arg("--view").and_then(|v| View::from_key(&v.to_lowercase())).unwrap_or(View::Inbox);
    // screenshots never write; they show sample data unless `--db` names a database to picture
    let (mut store, persist) = match (&shot, arg("--db")) {
        (Some(_), Some(_)) => (Store::load(&path).unwrap_or_else(|_| Store::sample(now())), false),
        (Some(_), None) => (Store::sample(now()), false),
        _ => {
            let mut s = Store::load_or_new(&path);
            s.id_base = sync::device_base(); // this device's ID range, so synced devices never mint the same ID
            (s, true)
        }
    };
    if let Some(t) = arg("--theme") {
        store.settings.theme = t;
        store.settings.follow_system = false;
    }
    let (themes, theme_errors) = theme::load_all();
    let center = store.settings.here.clone().unwrap_or(Place { label: String::new(), lat: 45.52, lon: -122.68, ..Place::default() });
    let (geo_tx, geo_rx) = std::sync::mpsc::channel();
    let mut app = Gtd {
        mtime: std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
        db: dbui::DbUi::new(&path),
        picked: vec![],
        acp_found: None,
        arrange_sections: std::env::args().any(|a| a == "--arrange"),
        sync_due: Some(0.0),
        sync_rx: None,
        last_rev: String::new(),
        rev_rx: None,
        rev_at: 0.0,
        sync_paused: false,
        sync_banner_hidden: false,
        sync_failures: 0,
        sync_fail_hidden: false,
        sync_msg: String::new(),
        status: Default::default(),
        pending_sync_link: None,
        pending_sync_db: None,
        renaming_folder: None,
        phone_search: false,
        list_scroll: 0.0,
        bar_collapsed: false,
        perspectives_page: false,
        layout_print: 0,
        connections_print: 0,
        queue_print: 0,
        focus_win: std::env::args().any(|a| a == "--focus"),
        focus_on_top: true,
        compact_seen: false,
        web: (0.0, vec![]),
        cal: vec![],
        cal_at: None,
        cal_rx: None,
        cal_msg: String::new(),
        cal_ask: false,
        cal_auto_asked: false,
        was_focused: false,
        cal_access: calendar::Access::NotAsked, // learned from the helper on the first background read
        cal_list: None,
        acp_rx: None,
        decide_test: None,
        decide_rx: None,
        path,
        store,
        persist,
        last_poll: 0.0,
        dirty_at: None,
        view,
        back: vec![],
        fwd: vec![],
        filter: Filter::All,
        sel: None,
        search: String::new(),
        collapsed: HashSet::new(),
        collapsed_headers: HashSet::new(),
        inbox_kept: HashSet::new(),
        last_inbox: HashSet::new(),
        session_resolved: HashSet::new(),
        editing: None,
        edit_buf: String::new(),
        edit_focus: false,
        date_buf: Default::default(),
        date_dirty: [false; 3],
        select_mode: false,
        toast: None,
        paged: false,
        note_inline: None,
        row_sheet: None,
        list_page: false,
        agentui: Default::default(),
        show_sidebar: true,
        show_inspector: true,
        show_settings: false,
        quick: None,
        drop_prompt: None,
        hide_queue: vec![],
        time: 0.0,
        notes: notes::Notes::new(shot.is_some()),
        shot,
        frames: 0,
        shot_sent: false,
        themes,
        theme_errors,
        applied_theme: String::new(),
        note_edit: None,
        map: places::MapView::new(center.lat, center.lon, 14),
        geo_tx,
        geo_rx,
        geo_msg: String::new(),
        place_query: String::new(),
        place_find: None,
        place_results: None,
        settings_tab: if cfg!(target_os = "ios") { SETTINGS_HOME } else { 0 },
        notified_until: now(),
        alarm_hash: 0,
        rail_seen: None,
        undo: vec![],
        redo: vec![],
        snapshot: None,
        undo_at: 0.0,
        recording: None,
        focus_field: None,
        view_opts: false,
        eye_rect: Rect::NOTHING,
        quick_open: None,
        focus_search: false,
        editor: None,
        ai_text: String::new(),
        ai_rx: None,
        ai_msg: String::new(),
        linear_rx: None,
        linear_busy: HashSet::new(),
        linear_msg: String::new(),
        link_buf: String::new(),
        linear_tx: None,
        refs_rx: None,
        refs_tx: None,
        refs_busy: HashSet::new(),
        refs_failed: HashSet::new(),
        refs_buf: String::new(),
        refs_msg: String::new(),
        refs_lists: std::collections::HashMap::new(),
        refs_list_rx: None,
        refs_list_tx: None,
        att_buf: String::new(),
        att_msg: String::new(),
        view_after_editor: None,
        pomo: None,
        last_clock: None,
        stats_days: 30,
        field_open: None,
        field_buf: String::new(),
        field_focus: false,
        strip_rects: Default::default(),
        typing: arg("--type"),
        scroll: arg("--scroll").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0),
        landscape: args.iter().any(|a| a == "--landscape"),
        press: args.iter().enumerate().filter(|(_, a)| *a == "--press").filter_map(|(i, _)| args.get(i + 1)).filter_map(|s| keys::Binding::parse(s)).collect(),
    };
    // Start Core Location for "here" (macOS/iOS)
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    places::cl::start();
    // perspectives added in later versions show up in older saved lists
    for d in default_perspectives() {
        if !app.store.settings.perspectives.iter().any(|p| p.key == d.key) {
            app.store.settings.perspectives.push(d);
        }
    }
    app.snapshot = Some(app.store.clone());
    if let Some(q) = arg("--select") {
        let q = q.to_lowercase();
        app.sel = app.store.tasks.iter().find(|t| t.name.to_lowercase().contains(&q)).map(|t| Sel::Task(t.id));
        if app.view == View::Tags {
            // `--view tags --select office` shows that tag's inspector
            app.sel = app.store.tags.iter().find(|g| g.name.to_lowercase() == q).map(|g| Sel::Tag(g.id)).or(app.sel);
            // `--place-search coffee`: that tag's Change Location › Search, run (checks the results list in screenshots)
            if let (Some(Sel::Tag(id)), Some(s)) = (app.sel, arg("--place-search")) {
                app.place_find = Some((id, false));
                app.place_query = s.clone();
                app.geocode(s, GeoFor::Tag(id), false);
            }
        }
        // `--select … --agent-compose`: the Send to Agent sheet for that action (checks templates in screenshots)
        if let (true, Some(Sel::Task(id))) = (args.iter().any(|a| a == "--agent-compose"), app.sel) {
            app.agent_compose(id, None);
        }
        // `--agent-thread ID`: that agent thread's window (checks it in screenshots)
        app.agentui.open = arg("--agent-thread");
        // `--select … --row-menu`: that action's command sheet (iPhone)
        if let (true, Some(Sel::Task(id))) = (args.iter().any(|a| a == "--row-menu"), app.sel) {
            app.row_sheet = Some(id);
        }
        // `--select … --date planned`: that task's inspector with its date field opened (iPhone: the date sheet)
        if let Some(k) = arg("--date") {
            app.show_inspector = true;
            app.focus_field = Some(match k.as_str() {
                "defer" => keys::Action::EditDefer,
                "due" => keys::Action::EditDue,
                "none" => keys::Action::EditCompleted, // just the inspector
                _ => keys::Action::EditPlanned,
            });
        }
    }
    // `--settings 4` opens Settings on that tab (used to check the settings windows in screenshots)
    // `--demo-events`: sample Calendar events for screenshots (screenshots never read your real calendar)
    if app.shot.is_some() && args.iter().any(|a| a == "--demo-events") {
        let t = now().date();
        let at = |d: i64, h: u32, m: u32| (t + Duration::days(d)).and_hms_opt(h, m, 0).unwrap();
        let ev = |title: &str, s: Dt, e: Dt, cal: &str, color: [u8; 3]| calendar::Event { title: title.into(), start: s, end: e, all_day: false, calendar: cal.into(), calendar_id: cal.into(), color: Some(color), location: String::new() };
        app.cal = vec![
            ev("Standup", at(0, 9, 30), at(0, 9, 45), "Work", [74, 128, 216]),
            ev("Design review", at(0, 14, 0), at(0, 15, 0), "Work", [74, 128, 216]),
            ev("Dentist", at(1, 11, 0), at(1, 12, 0), "Home", [230, 120, 60]),
            ev("Team offsite planning", at(3, 10, 0), at(3, 11, 30), "Work", [74, 128, 216]),
            ev("Dinner with Sam", at(5, 19, 0), at(5, 21, 0), "Home", [230, 120, 60]),
        ];
    }
    // `--quick` opens Quick Entry (used to check the card in screenshots)
    if args.iter().any(|a| a == "--quick") {
        app.quick = Some(QuickEntry::default());
        app.quick.as_mut().expect("quick").focus = false;
    }
    app.adopt_prints();
    if let Some(t) = arg("--settings").and_then(|t| t.parse::<u8>().ok()) {
        app.show_settings = true;
        app.settings_tab = t;
    }
    if args.iter().any(|a| a == "--board") {
        let k = view.key();
        if let Some(p) = app.store.settings.perspectives.iter_mut().find(|p| p.key == k) {
            p.board = true;
        }
    }
    if app.shot.is_some() && view == View::Inbox && app.sel.is_none() {
        app.sel = app.store.tasks.iter().find(|t| t.project.is_none() && !t.note.is_empty() && t.parent.is_none()).map(|t| Sel::Task(t.id));
    }
    // `--size 1100x640` opens at another size (used to check short windows)
    let size = arg("--size").and_then(|s| s.split_once('x').and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))).unwrap_or([1263.0, 1279.0]);
    if persist {
        webact::listen(); // the Chrome extension's records arrive on 127.0.0.1:8766
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GTDCraft")
            .with_inner_size(size)
            .with_min_inner_size([size[0].min(900.0), size[1].min(600.0)]) // `--size` may ask for a phone-sized window
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "GTDCraft",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

/// The app icon: an elephant ("never forgets") holding a done check; unique among the Craft apps.
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.to_rgba8());
    match img {
        Ok(i) => egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() },
        Err(_) => egui::IconData::default(),
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Helvetica Neue from macOS when present (index 0 regular, 1 bold); egui's fonts stay as fallbacks.
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/HelveticaNeue.ttc") {
        let reg = egui::FontData::from_owned(bytes.clone());
        let mut bold = egui::FontData::from_owned(bytes);
        bold.index = 1;
        fonts.font_data.insert("hn".into(), reg.into());
        fonts.font_data.insert("hn-bold".into(), bold.into());
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "hn".into());
        let mut b = vec!["hn-bold".to_string()];
        b.extend(fonts.families[&FontFamily::Proportional].iter().skip(1).cloned());
        fonts.families.insert(FontFamily::Name("bold".into()), b);
    } else {
        let p = fonts.families[&FontFamily::Proportional].clone();
        fonts.families.insert(FontFamily::Name("bold".into()), p);
    }
    // Apple Symbols last in every family: ⌃ ⌥ ⇧ ⌘ ⎋ ↑ ↓ ← → ⋯ ↺ and other symbols Helvetica Neue and egui's fonts lack
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf") {
        fonts.font_data.insert("symbols".into(), egui::FontData::from_owned(bytes).into());
        for fam in fonts.families.values_mut() {
            fam.push("symbols".into());
        }
    }
    ctx.set_fonts(fonts);
}

/// Applies the current theme to egui's own widgets.
fn apply_visuals(ctx: &egui::Context) {
    let mut v = if th().dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = th().bg;
    v.window_fill = th().inspector;
    v.extreme_bg_color = th().field;
    v.selection.bg_fill = th().accent;
    v.widgets.inactive.weak_bg_fill = th().pill;
    v.widgets.inactive.bg_fill = th().pill;
    v.widgets.inactive.corner_radius = CornerRadius::same(6);
    v.widgets.hovered.corner_radius = CornerRadius::same(6);
    v.widgets.active.corner_radius = CornerRadius::same(6);
    v.widgets.noninteractive.fg_stroke.color = th().text;
    v.widgets.inactive.fg_stroke.color = th().text;
    v.widgets.hovered.weak_bg_fill = th().pill_hover;
    v.widgets.hovered.bg_fill = th().pill_hover;
    v.widgets.hovered.fg_stroke.color = th().text;
    v.widgets.active.fg_stroke.color = th().text;
    v.widgets.noninteractive.bg_stroke.color = th().line;
    v.hyperlink_color = th().link;
    v.faint_bg_color = th().field;
    v.code_bg_color = th().field;
    v.override_text_color = None;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.5));
        s.spacing.item_spacing = vec2(6.0, 6.0);
    });
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

// ---------- date text ----------
fn fmt_time(t: NaiveTime) -> String {
    t.format("%-I:%M %p").to_string()
}
fn short_date(d: Dt, today: NaiveDate) -> String {
    if d.date() == today { fmt_time(d.time()) } else { d.format("%-m/%-d/%y").to_string() }
}
fn long_date(d: Dt) -> String {
    d.format("%b %-d, %Y  %-I:%M %p").to_string()
}

impl eframe::App for Gtd {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // frame count: `--press` / `--type` act on frames 10 / 14, and screenshots are taken at 120
        self.frames = self.frames.saturating_add(1);
        if let Some((msg, since)) = &mut self.toast {
            let t = ctx.input(|i| i.time);
            let since = *since.get_or_insert(t);
            if t - since > 1.8 {
                self.toast = None;
            } else {
                let r = ctx.content_rect();
                egui::Area::new(egui::Id::new("toast")).order(egui::Order::Tooltip).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -(r.bottom() - r.bottom().min(ctx.viewport_rect().bottom())) - 110.0)).interactable(false).show(ctx, |ui| {
                    egui::Frame::new().fill(mix(th().bg, th().text, 0.18)).corner_radius(18).inner_margin(egui::Margin::symmetric(16, 9)).show(ui, |ui| {
                        ui.label(egui::RichText::new(msg.as_str()).color(th().text).size(14.0));
                    });
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        self.row_sheet_ui(ctx);
        self.sync_banner(ctx);
        if self.shot.is_none() {
            self.follow_focus();
        }
        self.agent_poll(ctx);
        self.agent_compose_ui(ctx);
        self.agent_thread_ui(ctx);
        #[cfg(target_os = "ios")]
        {
            datepick::remember(ctx);
            sync::share_connection(&self.store.settings.sync_url, &self.store.settings.sync_token, &self.store.db_id);
        }
        // iOS Files / Photos: a picked file goes on the task or project it was picked for
        while let Some((target, path)) = attachpick::take() {
            let n = now();
            let list = match target {
                attachpick::Target::Task(id) => self.store.task_mut(id).map(|t| {
                    t.modified = n;
                    &mut t.attachments
                }),
                attachpick::Target::Project(id) => self.store.project_mut(id).map(|p| {
                    p.modified = n;
                    &mut p.attachments
                }),
            };
            if let Some(l) = list.filter(|l| !l.contains(&path)) {
                l.push(path);
                self.touch();
            }
        }
        // iOS date picker sheet: its changes land on the task it was opened for
        while let Some((slot, id, v)) = datepick::take() {
            if let Some(t) = self.store.task_mut(id) {
                *[&mut t.defer, &mut t.planned, &mut t.due][slot] = v;
                t.modified = now();
                self.touch();
            }
        }
        let time = ctx.input(|i| i.time);
        self.time = time;
        self.apply_theme(ctx);
        paint_safe_areas(ctx);
        if self.notes.poll(&mut self.store.link_cards) {
            self.touch();
        }
        self.poll_geocode();
        self.poll_linear();
        self.poll_refs();
        // Update "here" from Core Location when available (non-blocking check)
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let Some(loc) = places::cl::current() {
            let changed = self.store.settings.here.as_ref().is_none_or(|h| {
                (h.lat - loc.lat).abs() > 0.001 || (h.lon - loc.lon).abs() > 0.001
            });
            if changed {
                self.store.settings.here = Some(loc);
            }
        }
        // reminder buttons: Complete completes the action; tapping the reminder shows it
        for (what, id) in places::take_notification_actions() {
            if self.store.task(id).is_none() {
                continue;
            }
            if what == "complete" {
                self.store.complete(id, now());
                self.touch();
            } else if what == "clockin" {
                // the planned-time reminder's "Clock In": start the clock on it (and its Live Activity)
                self.sel = Some(Sel::Task(id));
                if self.store.active_clock().map(|c| c.0) != Some(id) {
                    self.toggle_clock(now());
                }
            } else {
                self.sel = Some(Sel::Task(id));
                self.show_inspector = true;
            }
        }
        self.take_opened_link(ctx);
        let in_inbox: HashSet<Id> = self.store.tasks.iter().filter(|t| t.project.is_none()).map(|t| t.id).collect();
        track_inbox(&self.last_inbox, &in_inbox, |id| self.store.task(id).is_some_and(|t| t.project.is_some()), &mut self.inbox_kept);
        self.last_inbox = in_inbox;
        if self.store.settings.auto_hide_completed && !self.hide_queue.is_empty() {
            let due: Vec<Id> = self.hide_queue.iter().filter(|(_, t)| time - t >= HIDE_COMPLETED_DELAY).map(|x| x.0).collect();
            self.hide_queue.retain(|(_, t)| time - t < HIDE_COMPLETED_DELAY);
            if !due.is_empty() {
                // a completion triggers a clean-up: processed inbox items leave the Inbox then too
                self.inbox_kept.clear();
            }
            for id in due {
                self.session_resolved.remove(&id);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.poll_file(time);
        if self.frames == 10 && !self.press.is_empty() {
            let presses = std::mem::take(&mut self.press);
            ctx.input_mut(|i| {
                for b in presses {
                    let modifiers = egui::Modifiers { alt: b.alt, ctrl: b.ctrl, shift: b.shift, mac_cmd: b.cmd, command: b.cmd };
                    i.events.push(egui::Event::Key { key: b.key, physical_key: None, pressed: true, repeat: false, modifiers });
                }
            });
        }
        self.menu_tick(ctx);
        #[cfg(target_os = "ios")]
        if self.frames == 3 && self.landscape {
            rotate_to_landscape();
        }
        if self.frames == 14 {
            if let Some(t) = self.typing.take() {
                ctx.input_mut(|i| i.events.push(egui::Event::Text(t)));
            }
        }
        let now = now();
        self.poll_calendar(ctx, now, false);
        let d = self.store.derive(now);
        self.reminders(&d, now);
        self.tick_pomodoro(now);
        self.keys(ctx, &d, now);
        self.menu_commands(ctx, &d, now);

        // iPhone on its side: no window-wide toolbar; the sidebar and the list each head themselves
        let landscape = phone_landscape(ctx);
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::NONE.fill(th().toolbar))
            .show_separator_line(false)
            .show_animated(ctx, !landscape, |ui| self.toolbar(ui));
        // iPhone in portrait: no perspective rail; a floating bottom bar instead
        let portrait = phone_portrait(ctx);
        // iPhone/iPad keyboard (UIKit's keyboard-frame notifications): the content ends above it, the bottom bar steps
        // aside while typing, and the field being edited is scrolled into view — the platform's usual behaviour
        let kb = keyboard::height();
        if kb > 0.0 {
            let below_safe = ctx.viewport_rect().bottom() - ctx.content_rect().bottom();
            egui::TopBottomPanel::bottom("keyboard-space").exact_height((kb - below_safe).max(0.0)).show_separator_line(false).frame(egui::Frame::NONE.fill(th().bg)).show(ctx, |_| {});
            if let Some(r) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) {
                if r.rect.bottom() > keyboard_top(ctx) - 8.0 {
                    r.scroll_to_me(Some(egui::Align::Center));
                }
            }
        }
        if portrait {
            // it floats over the list, which scrolls under it — and steps aside while typing.
            // A pushed detail page (the inspector) hides it, iOS-style, so it can't cover or block the bottom
            // fields; the page's own back button returns to the list.
            if kb == 0.0 && !self.show_inspector {
                let safe = ctx.content_rect();
                egui::Area::new(egui::Id::new("tab-bar")).order(egui::Order::Middle).fixed_pos(pos2(safe.left(), safe.bottom() - TAB_BAR_H)).show(ctx, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(safe.width(), TAB_BAR_H), Sense::hover());
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
                    let d = self.store.derive(now);
                    if self.select_mode {
                        self.select_bar(&mut child);
                    } else {
                        self.tab_bar(&mut child, &d);
                    }
                });
            }
        } else {
            egui::SidePanel::left("rail")
                .exact_width(RAIL_W)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE.fill(th().rail))
                .show(ctx, |ui| {
                    // re-derive per panel: an earlier panel or a shortcut may have added or removed tasks this frame
                    let d = self.store.derive(now);
                    // scrolls, no bar
                    egui::ScrollArea::vertical().id_salt("rail-scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| self.rail(ui, &d));
                    // the subsystems' lights, at the rail's foot (always on screen; the iPhone upright has them in its tab bar)
                    let foot = ui.max_rect();
                    self.status_dots(ui, pos2(foot.center().x, foot.bottom() - 12.0));
                });
        }
        // Panels slide in and out (egui's animated side panels).
        // Phones (and narrow windows): the perspective rail and one pane — the list, or the inspector as a full page
        let compact = ctx.content_rect().width() < COMPACT_W;
        // (and the iPhone on its side: no inspector column; it opens as a page in the list's place)
        let paged = compact || landscape;
        self.paged = paged;
        if paged && !self.compact_seen {
            self.compact_seen = true;
            self.show_inspector &= self.focus_field.is_some(); // start on the list (unless `--date` opened a field)
        }
        // sideways the sidebar takes about 4/9 of what the rail leaves
        let column_w = if landscape { ((ctx.content_rect().width() - RAIL_W) * 0.42).clamp(COLUMN_W, 320.0) } else { COLUMN_W };
        egui::SidePanel::left("column")
            .exact_width(column_w)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(th().column).stroke(Stroke::NONE))
            .show_animated(ctx, !compact && self.show_sidebar && self.view.has_column(), |ui| {
                let d = self.store.derive(now);
                self.column(ui, &d, now)
            });
        egui::SidePanel::right("inspector")
            .exact_width(INSPECTOR_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(th().inspector).inner_margin(egui::Margin::symmetric(18, 14)))
            .show_animated(ctx, !paged && self.show_inspector, |ui| {
                let d = self.store.derive(now);
                self.inspector(ui, &d, now)
            });
        if self.list_page && !(compact && !landscape && self.view.has_column() && matches!(self.view, View::Projects | View::Tags)) {
            self.list_page = false;
        }
        if self.list_page && !self.show_inspector {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(th().column)).show(ctx, |ui| {
                let r = ui.max_rect();
                // ‹ and the perspective's name, then the sidebar's list full width; picking one shows its items
                let (head, body) = r.split_top_bottom_at_y(r.top() + 56.0);
                let k = pos2(head.left() + 34.0, head.center().y);
                let resp = ui.interact(Rect::from_center_size(k, vec2(40.0, 40.0)), ui.id().with("lp-back"), Sense::click());
                ui.painter().circle_filled(k, 20.0, mix(th().column, th().text, 0.10));
                ui.painter().line_segment([k + vec2(3.0, -7.0), k + vec2(-4.0, 0.0)], Stroke::new(2.2, th().text));
                ui.painter().line_segment([k + vec2(-4.0, 0.0), k + vec2(3.0, 7.0)], Stroke::new(2.2, th().text));
                ui.painter().text(pos2(head.left() + 66.0, head.center().y), Align2::LEFT_CENTER, self.pname(self.view), bold(22.0), self.pcolor(self.view));
                if resp.clicked() {
                    self.list_page = false;
                }
                let before = self.filter;
                let d = self.store.derive(now);
                self.column(&mut ui.new_child(egui::UiBuilder::new().max_rect(body)), &d, now);
                if self.filter != before {
                    self.list_page = false;
                }
            });
        } else if paged && self.show_inspector {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(th().inspector).inner_margin(egui::Margin::symmetric(14, 10))).show(ctx, |ui| {
                    // a round ‹ back button
                let (b, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                ui.painter().circle_filled(b.center(), 20.0, mix(th().inspector, th().text, if resp.hovered() { 0.16 } else { 0.10 }));
                let k = b.center();
                ui.painter().line_segment([k + vec2(3.0, -7.0), k + vec2(-4.0, 0.0)], Stroke::new(2.2, th().text));
                ui.painter().line_segment([k + vec2(-4.0, 0.0), k + vec2(3.0, 7.0)], Stroke::new(2.2, th().text));
                if resp.clicked() {
                    self.show_inspector = false;
                }
                ui.add_space(4.0);
                let d = self.store.derive(now);
                self.inspector(ui, &d, now)
            });
        } else {
            let main = egui::CentralPanel::default().frame(egui::Frame::NONE.fill(th().bg)).show(ctx, |ui| {
                let d = self.store.derive(now);
                if landscape {
                    // the list's own top bar (eye · title · i ⋯), then the list
                    let r = ui.max_rect();
                    let (bar, body) = r.split_top_bottom_at_y(r.top() + TOOLBAR_H);
                    self.toolbar_phone(&mut ui.new_child(egui::UiBuilder::new().max_rect(bar)));
                    self.outline(&mut ui.new_child(egui::UiBuilder::new().max_rect(body)), &d, now);
                } else {
                    self.outline(ui, &d, now)
                }
            });
            if landscape && kb == 0.0 {
                // floating capsules at the list's bottom corners (out of the way while typing): ⚡ and Search at the left, + and Quick Entry at the right
                let r = main.response.rect;
                egui::Area::new(egui::Id::new("land-caps")).order(egui::Order::Middle).fixed_pos(r.left_bottom()).show(ctx, |ui| {
                    let y = r.bottom() - 26.0;
                    self.capsule(ui, Rect::from_min_max(pos2(r.left() + 28.0, y - 22.0), pos2(r.left() + 120.0, y + 22.0)), &[PhoneCmd::QuickOpen, PhoneCmd::Search], "lc-left");
                    self.capsule(ui, Rect::from_min_max(pos2(r.right() - 120.0, y - 22.0), pos2(r.right() - 28.0, y + 22.0)), &[PhoneCmd::New, PhoneCmd::QuickEntry], "lc-right");
                });
            }
        }
        if let Some((old, mut name)) = self.renaming_folder.take() {
            let mut done = None;
            egui::Modal::new(egui::Id::new("rename-folder")).show(ctx, |ui| {
                ui.set_width(280.0);
                ui.label(egui::RichText::new("Folder name").strong());
                let r = ui.add(crate::nativetext::Edit::singleline(&mut name).desired_width(f32::INFINITY));
                if !r.has_focus() && !ui.input(|i| i.pointer.any_pressed()) {
                    r.request_focus(); // type the name straight away
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        done = Some(false);
                    }
                    if ui.button("Rename").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                });
            });
            match done {
                Some(true) => {
                    // the field holds the folder's own name; a nested folder keeps its parent
                    let full = match model::folder_parent(&old) {
                        Some(parent) if !name.contains(model::FOLDER_SEP) => format!("{parent}{}{}", model::FOLDER_SEP, name.trim()),
                        _ => name,
                    };
                    self.store.rename_folder(&old, &full);
                    self.touch();
                }
                Some(false) => {}
                None => self.renaming_folder = Some((old, name)),
            }
        }
        if self.perspectives_page && (portrait || landscape) {
            self.perspectives_page_ui(ctx);
        }
        if self.show_settings {
            self.settings_window(ctx);
        }
        if self.drop_prompt.is_some() {
            self.drop_prompt_window(ctx);
        }
        self.view_options(ctx);
        self.field_popup(ctx, now);
        self.quick_open(ctx);
        self.perspective_editor(ctx);
        if self.quick.is_some() {
            self.quick_entry(ctx);
        }

        // save debounced after edits
        if let Some(t) = self.dirty_at {
            if time - t > 0.4 {
                self.save();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(450));
            }
        }
        if self.shot.is_some() || cfg!(target_os = "ios") {
            // one window only (screenshots; iPhone and iPad apps): draw Focus and Quick Entry inside it
            ctx.set_embed_viewports(true);
        }
        self.focus_window(ctx, now);
        self.sync_tick(ctx);
        self.status_tick(ctx);
        self.git_tick(ctx);
        self.db_windows(ctx);
        if !portrait && !landscape {
            version_label(ctx); // (iPhone: the bottom bar or buttons are there; Settings shows the version)
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1)); // file poll + clock
        #[cfg(target_os = "ios")]
        nativetext::end_frame();
        self.screenshot(ctx);
    }

    /// Menu bar clicks enter here, before the frame, so shortcut items act exactly like their keys.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.shot.is_some() {
            // screenshot runs ignore the real mouse and keyboard (before egui sees them) so a stray click or key
            // can't change what's captured; `--press` / `--type` inject their own events later
            raw.events.retain(|e| !matches!(e, egui::Event::PointerButton { .. } | egui::Event::PointerMoved(_) | egui::Event::PointerGone | egui::Event::MouseWheel { .. } | egui::Event::Key { .. } | egui::Event::Text(_)));
        }
        self.menu_input(raw);
        // iPhone/iPad: Return in a native text field reaches egui as Enter (fields that submit on Enter keep working)
        #[cfg(target_os = "ios")]
        if nativetext::take_return() {
            for pressed in [true, false] {
                raw.events.push(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
            }
        }
    }

    #[cfg(target_os = "ios")]
    fn on_exit(&mut self) {
        if self.dirty_at.is_some() {
            self.save();
        }
        self.sync_on_quit();
    }

    #[cfg(not(target_os = "ios"))]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.dirty_at.is_some() {
            self.save();
        }
        self.sync_on_quit();
        self.commit_now("Autosave on quit");
    }
}

impl Gtd {
    /// Picks the theme (fixed, or the light/dark pair when following macOS) and applies it when it changes.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        let st = &self.store.settings;
        let want = if st.follow_system {
            match ctx.system_theme() {
                Some(egui::Theme::Light) => st.light_theme.clone(),
                _ => st.dark_theme.clone(),
            }
        } else {
            st.theme.clone()
        };
        if want == self.applied_theme {
            return;
        }
        let t = self.themes.iter().find(|t| t.name == want).or(self.themes.first()).cloned();
        if let Some(t) = t {
            theme::set(t.colors);
            apply_visuals(ctx);
        }
        self.applied_theme = want;
    }

    fn poll_geocode(&mut self) {
        while let Ok(msg) = self.geo_rx.try_recv() {
            match msg {
                GeoMsg::Found(_, Err(e)) => self.geo_msg = e,
                GeoMsg::Found(GeoFor::Here, Ok((src, v))) => {
                    let Some(f) = v.first() else { continue };
                    self.map.lat = f.lat;
                    self.map.lon = f.lon;
                    self.geo_msg = format!("Found {} ({src})", f.name);
                    self.store.settings.here = Some(Place { label: "My location".into(), ..f.place(None) });
                    self.touch();
                }
                GeoMsg::Found(GeoFor::Tag(id), Ok((src, v))) => {
                    self.geo_msg = format!("{} from {src}", if v.len() == 1 { "1 place".to_string() } else { format!("{} places", v.len()) });
                    self.place_results = Some((id, v));
                }
                GeoMsg::Address(id, lat, lon, address) => {
                    let g = self.store.tags.iter_mut().find(|g| g.id == id);
                    // only if the tag is still at that spot
                    if let Some(p) = g.and_then(|g| g.place.as_mut()).filter(|p| p.lat == lat && p.lon == lon) {
                        p.address = address;
                        self.touch_tag(id);
                    }
                }
            }
        }
    }

    /// Searches for places in the background (Apple Maps, near you, on Mac and iPhone; OpenStreetMap elsewhere).
    fn geocode(&mut self, q: String, target: GeoFor, address_only: bool) {
        if q.trim().is_empty() {
            return;
        }
        self.geo_msg = "Searching\u{2026}".into();
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let gps = places::cl::current();
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let gps = None;
        let near = places::search_bias(gps.as_ref(), self.store.settings.here.as_ref());
        let tx = self.geo_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(GeoMsg::Found(target, places::search(&q, near, address_only)));
        });
    }

    /// Moves a tag to a spot (keeping its size and notification) and looks up the street address there.
    fn place_tag_at(&mut self, id: Id, label: String, lat: f64, lon: f64) {
        let Some(g) = self.store.tags.iter_mut().find(|g| g.id == id) else { return };
        g.place = Some(Place::relocated(g.place.as_ref(), label, String::new(), lat, lon));
        self.touch_tag(id);
        let tx = self.geo_tx.clone();
        std::thread::spawn(move || {
            if let Some(a) = places::reverse(lat, lon) {
                let _ = tx.send(GeoMsg::Address(id, lat, lon, a));
            }
        });
    }

    /// A tag changed: stamp it for sync and save.
    fn touch_tag(&mut self, id: Id) {
        if let Some(g) = self.store.tags.iter_mut().find(|g| g.id == id) {
            g.modified = Some(now());
        }
        self.touch();
    }

    /// Date/time reminders (by default due on, defer and planned off). On the Mac they're checked
    /// each second while the app is open and each time fires once, when the clock passes it. On iPhone/iPad the
    /// next week's are handed to iOS as scheduled notifications, so they arrive while GTDCraft is closed.
    fn reminders(&mut self, d: &Derived, now: Dt) {
        if now <= self.notified_until {
            return;
        }
        let from = self.notified_until;
        self.notified_until = now;
        if self.shot.is_some() || !self.store.settings.notify {
            return;
        }
        #[cfg(not(target_os = "ios"))]
        for (_, id, title, body) in self.alarms(d, from, now) {
            places::notify_task(id, &title, &body);
        }
        #[cfg(target_os = "ios")]
        {
            let _ = from;
            let ahead = self.alarms(d, now, now + Duration::days(7));
            // Location reminders: a geofence for each tag with a place whose Notification isn't None and that has
            // an available action (the nearest 20 to "here", `schedule_location`)
            let here = self.store.settings.here.clone();
            let loc_places: Vec<_> = self.store.tags.iter()
                .filter_map(|g| {
                    let place = g.place.as_ref().filter(|p| p.notify != Notify::None && here.is_some())?;
                    let task = self.store.tasks.iter().find(|t| t.tags.contains(&g.id) && d.st(t.id).available())?;
                    let what = if place.notify == Notify::Leaving { "Leaving" } else { "Near" };
                    Some((place.clone(), task.id, task.name.clone(), format!("{what} {}", g.name)))
                })
                .collect();
            // hand iOS the lot only when something changed (not every second, which re-armed every geofence)
            let hash = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                ahead.hash(&mut h);
                format!("{here:?}{loc_places:?}").hash(&mut h);
                h.finish()
            };
            if hash != self.alarm_hash {
                self.alarm_hash = hash;
                // replaces every pending request, so places that stopped notifying lose theirs too
                places::schedule(&ahead.iter().map(|(at, id, t, b)| ((*at - now).num_seconds() as f64, *id, t.clone(), b.clone())).collect::<Vec<_>>());
                if let Some(here) = &here {
                    places::schedule_location(here, &loc_places);
                }
            }
        }
    }

    /// The reminders that fall in (from, to], earliest first: (when, task, task name, what).
    /// Defer → effective defer date, latest time to start →
    /// effective due − estimated minutes, planned → effective planned date, due → effective due.
    fn alarms(&self, d: &Derived, from: Dt, to: Dt) -> Vec<(Dt, Id, String, String)> {
        let st = &self.store.settings;
        let mut out = vec![];
        for t in &self.store.tasks {
            if !d.status.get(&t.id).is_some_and(|s| s.remaining()) {
                continue;
            }
            let mut add = |x: Option<Dt>, body: String| {
                if let Some(x) = x.filter(|x| *x > from && *x <= to) {
                    out.push((x, t.id, t.name.clone(), body));
                }
            };
            let due = d.eff_due.get(&t.id).copied();
            if st.notify_due {
                add(due, format!("Due · {}", due.map(|x| fmt_time(x.time())).unwrap_or_default()));
            }
            if st.notify_start {
                if let (Some(dd), Some(m)) = (due, t.duration_min) {
                    add(Some(dd - Duration::minutes(m as i64)), format!("Time to start; item estimated to take {m} minutes"));
                }
            }
            if st.notify_defer {
                add(d.eff_defer.get(&t.id).copied(), "Available now".into());
            }
            if st.notify_planned {
                add(t.planned, match t.duration_min {
                    Some(m) => format!("Planned for now ({m} min) · Clock in?"),
                    None => "Planned for now · Clock in?".into(),
                });
            }
            // a heads-up before planned and due times, to wrap up and get ready
            let lead = st.notify_before_min as i64;
            if lead > 0 {
                // relative notification labels: "Before Due: " with an interval like "15 Minutes"
                if st.notify_planned {
                    add(t.planned.map(|x| x - Duration::minutes(lead)), format!("Before Planned: {}", interval_label(lead)));
                }
                if st.notify_due {
                    add(due.map(|x| x - Duration::minutes(lead)), format!("Before Due: {}", interval_label(lead)));
                }
            }
        }
        out.sort_by_key(|a| a.0);
        out
    }

    /// The automatic reminders still ahead for a task, as (kind, "Tomorrow, 5:00 PM").
    fn upcoming_reminders(&self, d: &Derived, id: Id, now: Dt) -> Vec<(String, String)> {
        let st = &self.store.settings;
        let Some(t) = self.store.task(id) else { return vec![] };
        if !st.notify || !d.status.get(&id).is_some_and(|s| s.remaining()) {
            return vec![];
        }
        let when = |x: Dt| {
            let day = match (x.date() - now.date()).num_days() {
                0 => "Today".to_string(),
                1 => "Tomorrow".to_string(),
                n if (2..7).contains(&n) => x.format("%A").to_string(),
                _ => x.format("%b %-d").to_string(),
            };
            format!("{day}, {}", fmt_time(x.time()))
        };
        let mut out = vec![];
        let due = d.eff_due.get(&id).copied();
        if let (true, Some(x)) = (st.notify_defer, d.eff_defer.get(&id).copied().filter(|x| *x > now)) {
            out.push(("Defer".to_string(), when(x)));
        }
        if let (true, Some(x)) = (st.notify_planned, t.planned.filter(|x| *x > now)) {
            out.push(("Planned".to_string(), when(x)));
        }
        if let (true, Some(x), Some(m)) = (st.notify_start, due, t.duration_min) {
            let s = x - Duration::minutes(m as i64);
            if s > now {
                out.push(("Time to start".to_string(), when(s)));
            }
        }
        // the heads-up reminders, labelled "Before Due: 15 Minutes" and so on
        let lead = st.notify_before_min as i64;
        if lead > 0 {
            if let (true, Some(x)) = (st.notify_planned, t.planned.map(|x| x - Duration::minutes(lead)).filter(|x| *x > now)) {
                out.push((format!("Before Planned ({})", interval_label(lead)), when(x)));
            }
            if let (true, Some(x)) = (st.notify_due, due.map(|x| x - Duration::minutes(lead)).filter(|x| *x > now)) {
                out.push((format!("Before Due ({})", interval_label(lead)), when(x)));
            }
        }
        if let (true, Some(x)) = (st.notify_due, due.filter(|x| *x > now)) {
            out.push(("Due".to_string(), when(x)));
        }
        out
    }

    fn pcfg(&self, v: View) -> Option<&PerspectiveCfg> {
        let k = v.key();
        self.store.settings.perspectives.iter().find(|p| p.key == k)
    }
    fn pname(&self, v: View) -> String {
        self.pcfg(v).map(|p| p.name.clone()).unwrap_or_else(|| format!("{v:?}"))
    }
    fn pcolor(&self, v: View) -> Color32 {
        hue(self.pcfg(v).map(|p| p.hue.as_str()).unwrap_or("blue"))
    }
    fn visible_views(&self) -> Vec<View> {
        self.store.settings.perspectives.iter().filter(|p| p.visible).filter_map(|p| View::from_key(&p.key)).collect()
    }

    /// Attach File… / Attach Folder…: the Finder picker, then the paths go on each target's Attachments.
    #[cfg(not(target_os = "ios"))]
    fn attach_picked(&mut self, targets: &[Sel], folder: bool) {
        let dlg = rfd::FileDialog::new().set_title(if folder { "Attach Folder" } else { "Attach File" });
        let picked: Vec<String> = if folder { dlg.pick_folder().into_iter().collect() } else { dlg.pick_files().unwrap_or_default() }.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        if attach_to(&mut self.store, targets, &picked, chrono::Local::now().naive_local()) {
            self.touch();
        }
    }
    #[cfg(target_os = "ios")]
    fn attach_picked(&mut self, _targets: &[Sel], _folder: bool) {}

    fn touch(&mut self) {
        self.dirty_at = Some(f64::MIN_POSITIVE.max(self.last_poll));
        // ponytail: whole-store snapshots; fine for personal databases, diff-based undo if they grow large
        if self.time - self.undo_at > 1.0 {
            if let Some(prev) = self.snapshot.take() {
                self.undo.push(prev);
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
            }
            self.redo.clear();
        }
        self.undo_at = self.time;
        self.snapshot = Some(self.store.clone());
    }
    fn save(&mut self) {
        self.dirty_at = None;
        if !self.persist {
            return;
        }
        // perspectives / section order changed here: stamp them so sync carries them to the other devices
        let print = self.store.layout_fingerprint();
        if print != self.layout_print {
            self.store.settings.layout_modified = Some(now());
            self.layout_print = print;
        }
        let print = self.store.connections_fingerprint();
        if print != self.connections_print {
            self.store.settings.connections_modified = Some(now());
            self.connections_print = print;
        }
        let print = self.store.queue_fingerprint();
        if print != self.queue_print {
            self.store.queue_modified = Some(now());
            self.queue_print = print;
        }
        if let Err(e) = self.store.save(&self.path) {
            eprintln!("gtdcraft: save failed: {e}");
        } else {
            self.after_save();
            // send changes to the sync server a few seconds after an edit
            if !self.store.settings.sync_url.trim().is_empty() && self.sync_rx.is_none() {
                self.sync_due = Some(self.sync_due.unwrap_or(f64::MAX).min(self.time + 1.0));
            }
        }
        self.mtime = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
    }
    /// Picks up changes an agent made through the MCP server.
    fn poll_file(&mut self, time: f64) {
        if time - self.last_poll < 1.0 {
            return;
        }
        self.last_poll = time;
        if !self.persist || self.dirty_at.is_some() {
            return;
        }
        let m = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if m.is_some() && m != self.mtime {
            if let Ok(mut s) = Store::load(&self.path) {
                s.id_base = self.store.id_base;
                self.snapshot = Some(s.clone());
                self.store = s;
                self.adopt_prints();
                self.mtime = m;
            }
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        ctx.request_repaint();
        if self.frames >= 120 && !self.shot_sent {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.shot_sent = true;
        }
        let img = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = img {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match image::save_buffer(&path, &raw, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                Ok(()) => eprintln!("saved {}", path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn go(&mut self, v: View) {
        if v != self.view {
            self.back.push(self.view);
            self.fwd.clear();
            self.switch(v);
        }
    }
    fn switch(&mut self, v: View) {
        self.view = v;
        self.filter = Filter::All;
        self.sel = None;
        self.picked.clear();
        self.editing = None;
        self.session_resolved.clear(); // "clean up when changing perspectives"
    }

    // ---------- actions ----------
    fn new_action(&mut self) {
        let (project, parent) = match (self.sel, self.filter) {
            (Some(Sel::Task(id)), _) => (self.store.task(id).and_then(|t| t.project), self.store.task(id).and_then(|t| t.parent)),
            (Some(Sel::Project(p)), _) | (_, Filter::Project(p)) => (Some(p), None),
            _ => (None, None),
        };
        let project = if self.view == View::Inbox { None } else { project };
        let id = self.store.add_task("", project, parent);
        // place right after the selected task
        if let Some(Sel::Task(after)) = self.sel {
            if let (Some(a), Some(n)) = (self.store.tasks.iter().position(|t| t.id == after), self.store.tasks.iter().position(|t| t.id == id)) {
                let t = self.store.tasks.remove(n);
                self.store.tasks.insert(a + 1, t);
            }
        }
        let n = now();
        let st = self.store.settings.clone();
        if let Some(t) = self.store.task_mut(id) {
            match self.view {
                View::Flagged => t.flagged = true,
                View::Forecast => {
                    if let Filter::Day(day) = self.filter {
                        t.due = Some(day.and_time(st.default_due_time));
                    } else {
                        t.due = Some(n.date().and_time(st.default_due_time));
                    }
                }
                View::Tags => {
                    if let Filter::Tag(Some(g)) = self.filter {
                        t.tags = vec![g];
                    }
                }
                _ => {}
            }
        }
        if !matches!(self.view, View::Inbox | View::Projects | View::Flagged | View::Forecast | View::Tags | View::Changed) {
            self.go(View::Inbox);
        }
        self.sel = Some(Sel::Task(id));
        self.editing = Some(id);
        self.edit_buf.clear();
        self.edit_focus = true;
        if self.paged {
            self.show_inspector = false; // type the new item in the list, where it goes (the person: "jarring")
        }
        self.touch();
    }

    fn toggle_done(&mut self, id: Id) {
        let n = now();
        let Some(t) = self.store.task(id) else { return };
        if t.completed.is_some() || t.dropped.is_some() {
            self.store.reactivate(id, n);
        } else {
            self.store.complete(id, n);
            self.mark_resolved(id);
        }
        self.touch();
    }
    /// Dropping a repeating action asks first: skip this occurrence or drop it completely.
    fn toggle_drop(&mut self, id: Id) {
        let n = now();
        let Some(t) = self.store.task(id) else { return };
        if t.dropped.is_some() {
            self.store.reactivate(id, n);
        } else if t.repeat.is_some() && t.completed.is_none() {
            self.drop_prompt = Some(id);
            return;
        } else {
            self.store.drop_task(id, n);
            self.mark_resolved(id);
        }
        self.touch();
    }
    fn mark_resolved(&mut self, id: Id) {
        self.session_resolved.insert(id);
        self.hide_queue.push((id, self.time));
    }
    fn drop_prompt_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.drop_prompt else { return };
        let mut choice = None;
        egui::Window::new("This item is configured to repeat.").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.label("Do you want to skip this occurrence and schedule the next one, or drop the item completely?");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    choice = Some(0);
                }
                if ui.button("Drop Completely").clicked() {
                    choice = Some(2);
                }
                if ui.add(egui::Button::new(egui::RichText::new("Skip Occurrence").color(Color32::WHITE)).fill(th().accent)).clicked() {
                    choice = Some(1);
                }
            });
        });
        if let Some(c) = choice {
            let n = now();
            match c {
                1 => self.store.drop_task(id, n),
                2 => self.store.drop_completely(id, n),
                _ => {}
            }
            if c != 0 {
                self.mark_resolved(id);
                self.touch();
            }
            self.drop_prompt = None;
        }
    }
    fn toggle_flag(&mut self, sel: Sel) {
        match sel {
            Sel::Task(id) => {
                if let Some(t) = self.store.task_mut(id) {
                    t.flagged = !t.flagged;
                    t.modified = now();
                }
            }
            Sel::Project(id) => {
                if let Some(p) = self.store.project_mut(id) {
                    p.flagged = !p.flagged;
                    p.modified = now();
                }
            }
            Sel::Tag(_) => return,
        }
        self.touch();
    }
    fn commit_edit(&mut self) {
        if let Some(id) = self.editing.take() {
            let name = self.edit_buf.trim().to_string();
            if let Some(t) = self.store.task_mut(id) {
                if name.is_empty() && t.name.is_empty() {
                    self.store.delete_task(id);
                    self.sel = None;
                } else if !name.is_empty() {
                    t.name = name;
                    t.modified = now();
                }
            }
            self.touch();
        }
    }

    // keys(), run(), visible(), push_tree() live in actions.rs

    fn rows(&self, d: &Derived, now: Dt) -> Vec<Row> {
        let mut rows = vec![];
        let today = now.date();
        let flat = |rows: &mut Vec<Row>, v: Vec<&Task>| rows.extend(v.into_iter().map(|t| Row::Task { id: t.id, depth: 0, show_project: true }));
        match self.view {
            View::Inbox => {
                let kept = self.inbox_kept.clone();
                self.push_tree(d, &mut rows, None, 0, &|t: &Task| t.project.is_none() || (kept.contains(&t.id) && t.parent.is_none()))
            }
            View::Projects => {
                let mut folders: Vec<&str> = vec![];
                for p in &self.store.projects {
                    if !folders.contains(&p.folder.as_str()) {
                        folders.push(&p.folder);
                    }
                }
                for f in folders {
                    let ps: Vec<&Project> = self
                        .store
                        .projects
                        .iter()
                        .filter(|p| p.folder == f && matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold))
                        .filter(|p| self.show_mode() == "remaining" || self.show_mode() == "everything" || p.status == ProjectStatus::Active)
                        .filter(|p| self.filter == Filter::All || self.filter == Filter::Project(p.id))
                        .collect();
                    if ps.is_empty() {
                        continue;
                    }
                    if !f.is_empty() && self.filter == Filter::All {
                        rows.push(Row::Header(f.to_string()));
                        if self.collapsed_headers.contains(f) {
                            continue;
                        }
                    }
                    for p in ps {
                        rows.push(Row::Project(p.id));
                        if !self.collapsed.contains(&p.id) {
                            let pid = p.id;
                            let at = rows.len();
                            self.push_tree(d, &mut rows, None, 1, &|t: &Task| t.project == Some(pid));
                            let _ = at;
                        }
                    }
                }
            }
            View::Tags => {
                let mut groups: Vec<(String, Option<Id>)> = self.store.tag_tree().into_iter().map(|(g, _)| (self.store.tag_path(g), Some(g))).collect();
                groups.push(("Untagged".into(), None));
                // a parent tag shows its own actions and its children's
                let family: Option<Vec<Id>> = if let Filter::Tag(Some(p)) = self.filter { Some(self.store.tag_family(p)) } else { None };
                for (name, g) in groups {
                    let shown = match (&family, g) {
                        (Some(f), Some(g)) => f.contains(&g),
                        (Some(_), None) => false,
                        (None, _) => self.filter == Filter::All || self.filter == Filter::Tag(g),
                    };
                    if !shown {
                        continue;
                    }
                    let v: Vec<&Task> = self
                        .store
                        .tasks
                        .iter()
                        .filter(|t| match g {
                            Some(g) => t.tags.contains(&g),
                            None => t.tags.is_empty(),
                        })
                        .filter(|t| self.visible(d, t))
                        .collect();
                    if v.is_empty() {
                        continue;
                    }
                    rows.push(Row::Header(name.clone()));
                    if !self.collapsed_headers.contains(&name) {
                        flat(&mut rows, v);
                    }
                }
            }
            View::Flagged => {
                let v: Vec<&Task> = self
                    .store
                    .tasks
                    .iter()
                    .filter(|t| t.flagged || t.project.and_then(|p| self.store.project(p)).is_some_and(|p| p.flagged))
                    .filter(|t| self.visible(d, t))
                    .collect();
                flat(&mut rows, v);
            }
            View::Forecast => {
                let mut sections: Vec<(String, Filter, Vec<&Task>)> = vec![];
                let rem: Vec<&Task> = self.store.tasks.iter().filter(|t| self.visible(d, t)).collect();
                let past: Vec<&Task> = rem.iter().copied().filter(|t| t.due.is_some_and(|x| x.date() < today)).collect();
                sections.push(("Past".into(), Filter::Past, past));
                let mut days: Vec<NaiveDate> = (0..7).map(|i| today + Duration::days(i)).collect();
                if let Filter::Day(d) = self.filter {
                    if !days.contains(&d) {
                        days.push(d); // a day picked further out on the calendar
                    }
                }
                for day in days {
                    let v: Vec<&Task> = rem.iter().copied().filter(|t| self.forecast_includes(t, day)).collect();
                    let title = if day == today { format!("Today — {}", day.format("%a, %b %-d")) } else { day.format("%a, %b %-d").to_string() };
                    sections.push((title, Filter::Day(day), v));
                }
                let last = today + Duration::days(6);
                let fut: Vec<&Task> = rem.iter().copied().filter(|t| t.due.or(t.planned).is_some_and(|x| x.date() > last)).collect();
                sections.push(("Future".into(), Filter::Future, fut));
                for (title, f, mut v) in sections {
                    if self.filter != Filter::All && self.filter != f {
                        continue;
                    }
                    // Calendar events first on each day, in time order
                    let events: Vec<(usize, NaiveDate)> = match f {
                        Filter::Day(day) => self.cal.iter().enumerate().filter(|(_, e)| calendar::on_day(e, day)).map(|(i, _)| (i, day)).collect(),
                        _ => vec![],
                    };
                    if v.is_empty() && events.is_empty() && !matches!(f, Filter::Day(x) if x == today) && self.filter == Filter::All {
                        continue;
                    }
                    v.sort_by_key(|t| t.due.or(t.planned));
                    rows.push(Row::Header(title.clone()));
                    if !self.collapsed_headers.contains(&title) {
                        // the day's timeline when it has events or planned blocks (an action planned with a duration)
                        if let Filter::Day(day) = f {
                            if !events.is_empty() || v.iter().any(|t| t.duration_min.is_some() && t.planned.is_some_and(|p| p.date() == day)) {
                                rows.push(Row::Timeline(day));
                            }
                        }
                        rows.extend(events.into_iter().map(|(i, day)| Row::Event(i, day)));
                        flat(&mut rows, v);
                    }
                }
            }
            View::Review => {
                let mut ps: Vec<&Project> = self
                    .store
                    .projects
                    .iter()
                    .filter(|p| p.next_review <= today && matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold))
                    .collect();
                ps.sort_by_key(|p| p.next_review); // manual: oldest Next review date first
                rows.extend(ps.into_iter().map(|p| Row::Project(p.id)));
            }
            View::Stats => {}
            View::Agents => {
                // actions with threads, by project; the view's Show rule applies (finished threads' actions are
                // archived out of sight unless it shows Everything)
                let everything = self.show_mode() == "everything";
                let mut by_project: Vec<(String, Vec<&Task>)> = vec![];
                let mut seen = HashSet::new();
                let mut threads: Vec<&agent::Thread> = self.agentui.threads.iter().collect();
                threads.sort_by_key(|t| std::cmp::Reverse(t.created));
                for th in threads {
                    let Some(t) = self.store.task(th.task) else { continue };
                    if !seen.insert(t.id) || !self.visible(d, t) {
                        continue;
                    }
                    let latest_active = self.agentui.latest(t.id).is_some_and(|x| x.status.active());
                    if !latest_active && !everything {
                        continue;
                    }
                    let proj = t.project.and_then(|p| self.store.project(p)).map(|p| self.store.project_path(p.id)).unwrap_or_else(|| "Inbox".into());
                    match by_project.iter_mut().find(|x| x.0 == proj) {
                        Some(x) => x.1.push(t),
                        None => by_project.push((proj, vec![t])),
                    }
                }
                for (proj, v) in by_project {
                    rows.push(Row::Header(proj));
                    rows.extend(v.into_iter().map(|t| Row::Task { id: t.id, depth: 0, show_project: false }));
                }
            }
            View::Nearby => {
                // Places sorted by distance from your location; available actions tagged with each place.
                let here = self.store.settings.here.clone();
                let mut placed: Vec<(&Tag, f64)> = self
                    .store
                    .tags
                    .iter()
                    .filter_map(|g| g.place.as_ref().map(|p| (g, here.as_ref().map(|h| h.km_to(p)).unwrap_or(0.0))))
                    .filter(|(g, _)| self.filter == Filter::All || self.filter == Filter::Tag(Some(g.id)))
                    .collect();
                placed.sort_by(|a, b| a.1.total_cmp(&b.1));
                for (g, km) in placed {
                    let v: Vec<&Task> = self.store.tasks.iter().filter(|t| t.tags.contains(&g.id) && d.st(t.id).available()).collect();
                    if v.is_empty() {
                        continue;
                    }
                    let label = g.place.as_ref().map(|p| p.label.clone()).unwrap_or_default();
                    let inside = here.is_some() && km * 1000.0 <= g.place.as_ref().map(|p| p.radius()).unwrap_or(1000.0);
                    let title = match (&here, inside) {
                        (Some(_), true) => format!("{} — {label} · you're here", g.name),
                        (Some(_), false) => format!("{} — {label} · {km:.1} km", g.name),
                        _ => format!("{} — {label}", g.name),
                    };
                    rows.push(Row::Header(title.clone()));
                    if !self.collapsed_headers.contains(&title) {
                        flat(&mut rows, v);
                    }
                }
            }
            View::Custom(_) => {
                // custom perspective: rule tree, then Structure (grouping) and initial order
                let cfg = self.pcfg(self.view).cloned().unwrap_or_else(PerspectiveCfg::blank);
                let rule = cfg.rule();
                let q = self.search.trim().to_lowercase();
                let mut v: Vec<&Task> = self
                    .store
                    .tasks
                    .iter()
                    .filter(|t| rule.matches(&self.store, d, t, now) || self.session_resolved.contains(&t.id))
                    .filter(|t| q.is_empty() || t.name.to_lowercase().contains(&q) || t.note.to_lowercase().contains(&q))
                    .collect();
                let far = NaiveDate::MAX.and_time(NaiveTime::MIN);
                match cfg.sort.as_deref().unwrap_or("none") {
                    "due" => v.sort_by_key(|t| d.eff_due.get(&t.id).copied().unwrap_or(far)),
                    "defer" => v.sort_by_key(|t| d.eff_defer.get(&t.id).copied().unwrap_or(far)),
                    "planned" => v.sort_by_key(|t| t.planned.unwrap_or(far)),
                    "added" => v.sort_by_key(|t| std::cmp::Reverse(t.added)),
                    "modified" => v.sort_by_key(|t| std::cmp::Reverse(t.modified)),
                    "name" => v.sort_by_key(|t| t.name.to_lowercase()),
                    "flagged" => v.sort_by_key(|t| !(t.flagged)),
                    _ => {}
                }
                let group = cfg.group_by();
                if group == "none" {
                    flat(&mut rows, v);
                } else {
                    let mut groups: Vec<(String, Vec<&Task>)> = vec![];
                    for t in v {
                        let key = match group.as_str() {
                            "tag" => t.tags.first().and_then(|g| self.store.tag(*g)).map(|g| g.name.clone()).unwrap_or_else(|| "Untagged".into()),
                            "folder" => t.project.and_then(|p| self.store.project(p)).map(|p| p.folder.clone()).filter(|f| !f.is_empty()).unwrap_or_else(|| "No Folder".into()),
                            "due" => match d.eff_due.get(&t.id) {
                                Some(x) if x.date() < today => "Overdue".into(),
                                Some(x) if x.date() == today => "Today".into(),
                                Some(x) if x.date() == today + Duration::days(1) => "Tomorrow".into(),
                                Some(x) if (x.date() - today).num_days() < 7 => "Within a week".into(),
                                Some(_) => "Later".into(),
                                None => "No Due Date".into(),
                            },
                            "flagged" => if self.store.effective_flagged(t.id) != (false, false) { "Flagged".into() } else { "Unflagged".into() },
                            _ => t.project.and_then(|p| self.store.project(p)).map(|p| p.name.clone()).unwrap_or_else(|| "No Project".into()),
                        };
                        match groups.iter_mut().find(|g| g.0 == key) {
                            Some(g) => g.1.push(t),
                            None => groups.push((key, vec![t])),
                        }
                    }
                    for (k, v) in groups {
                        rows.push(Row::Header(k.clone()));
                        if !self.collapsed_headers.contains(&k) {
                            flat(&mut rows, v);
                        }
                    }
                }
            }
            View::Completed | View::Changed => {
                let completed = self.view == View::Completed;
                let key = |t: &Task| if completed { t.completed } else { Some(t.modified) };
                let tag_ok = |t: &Task| match self.filter {
                    Filter::Tag(Some(g)) => self.store.tag_family(g).iter().any(|x| t.tags.contains(x)),
                    Filter::Tag(None) => t.tags.is_empty(),
                    _ => true,
                };
                let q = self.search.trim().to_lowercase();
                let mut v: Vec<&Task> = self
                    .store
                    .tasks
                    .iter()
                    .filter(|t| key(t).is_some() && tag_ok(t))
                    .filter(|t| q.is_empty() || t.name.to_lowercase().contains(&q))
                    .collect();
                v.sort_by_key(|t| std::cmp::Reverse(key(t)));
                let word = if completed { "Completed" } else { "Changed" };
                let buckets = [
                    (format!("{word} today"), 0i64),
                    (format!("{word} within the last week"), 7),
                    (format!("{word} within the last month"), 31),
                    (format!("{word} earlier"), i64::MAX),
                ];
                let mut cur: Option<usize> = None;
                for t in v {
                    let age = (today - key(t).unwrap().date()).num_days();
                    let b = buckets.iter().position(|(_, days)| age <= *days).unwrap_or(3);
                    if cur != Some(b) {
                        cur = Some(b);
                        rows.push(Row::Header(buckets[b].0.clone()));
                    }
                    if !self.collapsed_headers.contains(&buckets[b].0) {
                        rows.push(Row::Task { id: t.id, depth: 0, show_project: true });
                    }
                }
            }
        }
        rows
    }

    // ---------- toolbar ----------
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        if phone_portrait(ui.ctx()) {
            return self.toolbar_phone(ui);
        }
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let cy = r.center().y + 2.0;
        let button = |ui: &mut egui::Ui, center: Pos2, w: f32, id: &str| -> (egui::Response, Rect) {
            let rect = Rect::from_center_size(center, vec2(w, 34.0));
            let resp = ui.interact(rect, ui.id().with(id), Sense::click());
            let fill = if resp.hovered() { th().pill_hover } else { th().pill };
            p.rect_filled(rect, 17.0, fill);
            (resp, rect)
        };
        let ic = mix(th().text, th().dim, 0.15);
        let s = Stroke::new(1.5, ic);
        // Narrow (phone): no sidebar toggle (one pane), everything packed from the left; on iOS there are no window
        // buttons to leave room for.
        let compact = r.width() < COMPACT_W;
        let lights = if cfg!(target_os = "ios") { 0.0 } else { 84.0 };
        let left = r.left() + lights;
        let (bf_x, eye_x) = if compact { (left + 46.0, left + 112.0) } else { (left + 97.0, left + 163.0) };
        // sidebar toggle
        if !compact {
            let (b, rect) = button(ui, pos2(left + 30.0, cy), 34.0, "sb");
            let c = rect.center();
            p.rect_stroke(Rect::from_center_size(c, vec2(16.0, 13.0)), 2.5, s, StrokeKind::Middle);
            p.line_segment([c + vec2(-3.0, -6.5), c + vec2(-3.0, 6.5)], s);
            if b.on_hover_text("Toggle sidebar").clicked() {
                self.show_sidebar = !self.show_sidebar;
            }
        }
        // back / forward
        let rect = Rect::from_center_size(pos2(bf_x, cy), vec2(80.0, 34.0));
        p.rect_filled(rect, 17.0, th().pill);
        let bk = ui.interact(Rect::from_min_max(rect.min, pos2(rect.center().x, rect.max.y)), ui.id().with("bk"), Sense::click());
        let fw = ui.interact(Rect::from_min_max(pos2(rect.center().x, rect.min.y), rect.max), ui.id().with("fw"), Sense::click());
        let bc = if self.back.is_empty() { th().faint } else { ic };
        let fc = if self.fwd.is_empty() { th().faint } else { ic };
        let l = pos2(rect.left() + 20.0, cy);
        p.line_segment([l + vec2(4.0, -7.0), l + vec2(-3.0, 0.0)], Stroke::new(1.8, bc));
        p.line_segment([l + vec2(-3.0, 0.0), l + vec2(4.0, 7.0)], Stroke::new(1.8, bc));
        let rr = pos2(rect.right() - 20.0, cy);
        p.line_segment([rr + vec2(-4.0, -7.0), rr + vec2(3.0, 0.0)], Stroke::new(1.8, fc));
        p.line_segment([rr + vec2(3.0, 0.0), rr + vec2(-4.0, 7.0)], Stroke::new(1.8, fc));
        if bk.clicked() {
            if let Some(v) = self.back.pop() {
                self.fwd.push(self.view);
                self.switch(v);
            }
        }
        if fw.clicked() {
            if let Some(v) = self.fwd.pop() {
                self.back.push(self.view);
                self.switch(v);
            }
        }
        // view options (eye): opens the per-perspective options popover
        let (b, rect) = button(ui, pos2(eye_x, cy), 34.0, "eye");
        let c = rect.center();
        let eye_c = if self.view_opts { th().accent } else { ic };
        // almond outline: two arcs meeting at the corners (no repeated end point, so no miter spike)
        let mut pts: Vec<Pos2> = (0..=16).map(|i| {
            let x = -9.5 + 19.0 * i as f32 / 16.0;
            c + vec2(x, -6.0 * (1.0 - (x / 9.5).powi(2)))
        }).collect();
        pts.extend((1..16).rev().map(|i| {
            let x = -9.5 + 19.0 * i as f32 / 16.0;
            c + vec2(x, 6.0 * (1.0 - (x / 9.5).powi(2)))
        }));
        p.add(Shape::closed_line(pts, Stroke::new(1.6, eye_c)));
        p.circle_stroke(c, 3.6, Stroke::new(1.6, eye_c));
        p.circle_filled(c, 1.6, eye_c);
        self.eye_rect = rect;
        if b.on_hover_text("View Options").clicked() {
            self.view_opts = !self.view_opts;
        }
        // center group: new action, quick entry, clean up
        let mid = if compact { pos2(eye_x + 90.0, cy) } else { pos2(r.left() + 655.0, cy).min(pos2(r.center().x, cy)).max(pos2(r.left() + 420.0, cy)) };
        let rect = Rect::from_center_size(mid, vec2(110.0, 34.0));
        p.rect_filled(rect, 17.0, th().pill);
        let cells: Vec<Rect> = (0..3).map(|i| Rect::from_min_size(rect.min + vec2(i as f32 * 110.0 / 3.0, 0.0), vec2(110.0 / 3.0, 34.0))).collect();
        let c0 = cells[0].center();
        p.line_segment([c0 + vec2(-7.0, 0.0), c0 + vec2(7.0, 0.0)], Stroke::new(1.8, ic));
        p.line_segment([c0 + vec2(0.0, -7.0), c0 + vec2(0.0, 7.0)], Stroke::new(1.8, ic));
        let c1 = cells[1].center();
        p.rect_filled(Rect::from_center_size(c1, vec2(16.0, 14.0)), 3.0, ic);
        p.line_segment([c1 + vec2(-4.0, -1.0), c1 + vec2(4.0, -1.0)], Stroke::new(1.6, th().pill));
        p.line_segment([c1 + vec2(0.0, -5.0), c1 + vec2(0.0, 3.0)], Stroke::new(1.6, th().pill));
        let c2 = cells[2].center();
        for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let k = c2 + vec2(7.0 * dx, 7.0 * dy);
            p.line_segment([k, k - vec2(3.5 * dx, 0.0)], Stroke::new(1.4, th().faint.gamma_multiply(1.6)));
            p.line_segment([k, k - vec2(0.0, 3.5 * dy)], Stroke::new(1.4, th().faint.gamma_multiply(1.6)));
        }
        p.circle_filled(c2, 2.0, th().faint.gamma_multiply(1.6));
        if ui.interact(cells[0], ui.id().with("new"), Sense::click()).on_hover_text("New Action (⌘N)").clicked() {
            self.commit_edit();
            self.new_action();
        }
        if ui.interact(cells[1], ui.id().with("quick"), Sense::click()).on_hover_text("Quick Entry (⌃⌥Space)").clicked() {
            self.quick = Some(QuickEntry::default());
        }
        if ui.interact(cells[2], ui.id().with("clean"), Sense::click()).on_hover_text("Clean Up").clicked() {
            self.session_resolved.clear();
        }
        // search
        // narrow: a Settings button (no keyboard or sidebar gear on a phone), then whatever search fits before it
        if compact {
            let (b, rect) = button(ui, pos2(r.right() - 80.0, cy), 34.0, "settings");
            let c = rect.center();
            p.circle_stroke(c, 6.0, s);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 6.0, c + d * 9.0], Stroke::new(2.0, ic));
            }
            if b.on_hover_text("Settings").clicked() {
                self.show_settings = true;
            }
        }
        let sw = if compact { (r.right() - 125.0 - (mid.x + 55.0 + 10.0)).clamp(0.0, 165.0) } else { 165.0 };
        let sr = Rect::from_min_size(pos2(r.right() - if compact { 125.0 } else { 81.0 } - sw, cy - 17.0), vec2(sw, 34.0));
        let pill_at = rect.right_center() + vec2(12.0, 0.0);
        self.timer_pill(ui, pill_at, sr.left() - 10.0 - pill_at.x, now());
        if sw >= 60.0 {
            p.rect_filled(sr, 17.0, th().pill);
            let mc = sr.left_center() + vec2(20.0, -1.0);
            p.circle_stroke(mc, 5.5, Stroke::new(1.5, th().dim));
            p.line_segment([mc + vec2(4.0, 4.0), mc + vec2(8.0, 8.0)], Stroke::new(1.6, th().dim));
            let te = crate::nativetext::Edit::singleline(&mut self.search).hint_text("Search Here").frame(false).font(FontId::proportional(14.0)).text_color(th().text);
            let sresp = ui.put(Rect::from_min_max(sr.min + vec2(36.0, 8.0), sr.max - vec2(10.0, 8.0)), te);
            if self.focus_search {
                sresp.request_focus();
                self.focus_search = false;
            }
        }
        // inspector toggle
        let (b, rect) = button(ui, pos2(r.right() - 36.0, cy), 34.0, "insp");
        p.text(rect.center(), Align2::CENTER_CENTER, "i", FontId::proportional(20.0), if self.show_inspector { ic } else { th().dim });
        if b.on_hover_text("Inspector").clicked() {
            self.show_inspector = !self.show_inspector;
        }
        let _ = Vec2::ZERO;
    }

    // ---------- left rail ----------
    /// The Inbox badge: inbox actions still to do (leaves only).
    fn inbox_count(&self, d: &Derived) -> usize {
        self.store
            .tasks
            .iter()
            .filter(|t| t.project.is_none() && d.st(t.id).remaining() && d.st(t.id) != Status::Blocked || (t.project.is_none() && d.st(t.id) == Status::Blocked && t.defer.is_none() && !d.children.contains_key(&Some(t.id))))
            .filter(|t| !d.children.get(&Some(t.id)).is_some_and(|c| !c.is_empty()))
            .count()
    }

    /// iPhone top bar. Upright: round buttons — the perspectives page (▯), View Options (eye) —
    /// and at the right the inspector (i, when something is selected) and ⋯ (Clean Up, Go Back, Sync, Settings); once the
    /// list has scrolled its large title away, the title shows small in the middle. Sideways it heads the list: ▯ only while the sidebar is hidden, the eye, the title
    /// in the middle, and i and ⋯ in one capsule. Search, from the bottom bar, takes the middle with a Cancel; a blue
    /// ✓ ends editing an action's name.
    fn toolbar_phone(&mut self, ui: &mut egui::Ui) {
        let c = th();
        let r = ui.max_rect();
        let cy = r.center().y + 2.0;
        let ic = mix(c.text, c.dim, 0.15);
        let landscape = phone_landscape(ui.ctx());
        let round = |ui: &mut egui::Ui, x: f32, id: &str, fill: bool| -> (egui::Response, Pos2) {
            let rect = egui::Rect::from_center_size(pos2(x, cy), vec2(40.0, 40.0));
            let resp = ui.interact(rect, ui.id().with(id), Sense::click());
            if fill {
                ui.painter().circle_filled(rect.center(), 20.0, if resp.hovered() { c.pill_hover } else { c.pill });
            }
            (resp, rect.center())
        };
        // ▯ → upright: the perspectives page; sideways: the sidebar back (or the perspectives page, with no sidebar)
        let side_shown = landscape && self.show_sidebar && self.view.has_column();
        let mut x = r.left() + 34.0;
        if !side_shown {
            let (b, k) = round(ui, x, "ph-side", true);
            ui.painter().rect_stroke(egui::Rect::from_center_size(k, vec2(18.0, 15.0)), 3.0, Stroke::new(1.6, ic), egui::StrokeKind::Middle);
            ui.painter().line_segment([k + vec2(-3.0, -7.5), k + vec2(-3.0, 7.5)], Stroke::new(1.6, ic));
            if b.clicked() {
                if landscape && self.view.has_column() {
                    self.show_sidebar = true;
                } else {
                    self.perspectives_page = true;
                }
            }
            x += 48.0;
        }
        // eye → View Options
        let (b, k) = round(ui, x, "ph-eye", true);
        eye_glyph(ui.painter(), k, if self.view_opts { c.accent } else { ic });
        self.eye_rect = b.rect; // View Options opens under it (it opened off-screen: the person's "eye does nothing")
        if b.clicked() {
            self.view_opts = !self.view_opts;
        }
        // undo / redo (the person: "need undo and redo buttons" — a phone has no ⌘Z), once there's something to undo
        if !self.undo.is_empty() || !self.redo.is_empty() {
            for (redo, id) in [(false, "ph-undo"), (true, "ph-redo")] {
                x += 48.0;
                let (b, k) = round(ui, x, id, true);
                let on = if redo { !self.redo.is_empty() } else { !self.undo.is_empty() };
                let col = if on { ic } else { c.faint };
                // a curved arrow: an arc and its head
                let dir = if redo { 1.0 } else { -1.0 };
                let pts: Vec<Pos2> = (0..=10).map(|i| {
                    let a = std::f32::consts::PI * (0.15 + 0.85 * i as f32 / 10.0);
                    k + vec2(-dir * 6.5 * a.cos(), 2.5 - 6.5 * a.sin())
                }).collect();
                let tip = pts[0];
                ui.painter().add(Shape::line(pts, Stroke::new(1.8, col)));
                ui.painter().line_segment([tip, tip + vec2(-dir * 1.0, -5.0)], Stroke::new(1.8, col));
                ui.painter().line_segment([tip, tip + vec2(-dir * 5.0, 1.0)], Stroke::new(1.8, col));
                if b.clicked() && on {
                    self.undo_step(redo);
                }
            }
        }
        let left = x + 26.0;
        // sideways: i and ⋯ share a capsule
        let insp_x = r.right() - 82.0;
        if landscape {
            ui.painter().rect_filled(egui::Rect::from_min_max(pos2(insp_x - 22.0, cy - 20.0), pos2(r.right() - 12.0, cy + 20.0)), 20.0, c.pill);
        }
        // ⋯ menu
        let (more, k) = round(ui, r.right() - 34.0, "ph-more", !landscape);
        for dx in [-6.0, 0.0, 6.0] {
            ui.painter().circle_filled(k + vec2(dx, 0.0), 2.0, ic);
        }
        egui::Popup::menu(&more).show(|ui| {
            let item = |ui: &mut egui::Ui, label: &str| {
                let clicked = ui.button(label).clicked();
                if clicked {
                    ui.close();
                }
                clicked
            };
            if item(ui, if self.select_mode { "Done Selecting" } else { "Select" }) {
                self.select_mode = !self.select_mode;
                self.show_inspector = false;
            }
            if item(ui, "Clean Up") {
                self.db.run.push(keys::Action::CleanUp);
            }
            if !self.back.is_empty() && item(ui, "Go Back") {
                self.db.run.push(keys::Action::Back);
            }
            // every row, recursively
            if item(ui, "Expand All") {
                self.collapsed.clear();
                self.collapsed_headers.clear();
            }
            if item(ui, "Collapse All") {
                let parents: Vec<Id> = self.store.tasks.iter().filter_map(|t| t.parent).chain(self.store.projects.iter().map(|p| p.id)).collect();
                self.collapsed.extend(parents);
                self.collapsed_headers.extend(self.store.folder_names());
            }
            if !self.store.settings.sync_url.trim().is_empty() && item(ui, "Sync") {
                self.sync_due = Some(0.0);
            }
            if item(ui, "Settings") {
                self.show_settings = true;
            }
            if item(ui, "Report a Bug") {
                self.report_bug();
            }
        });
        // i → the inspector, when something is selected (sideways it's always there, dimmed without a selection)
        let mut right = r.right() - 60.0;
        if self.sel.is_some() || landscape {
            let (b, k) = round(ui, insp_x, "ph-insp", !landscape);
            let col = if self.sel.is_none() { c.faint } else if self.show_inspector { c.accent } else { ic };
            ui.painter().text(k, Align2::CENTER_CENTER, "i", FontId::proportional(20.0), col);
            if b.clicked() && self.sel.is_some() {
                self.show_inspector = !self.show_inspector;
            }
            right = insp_x - 26.0;
        }
        // ✓ ends editing a name (picture: the blue check while an action is being typed)
        if self.editing.is_some() {
            let k = pos2(right - 22.0, cy);
            let resp = ui.interact(egui::Rect::from_center_size(k, vec2(40.0, 40.0)), ui.id().with("ph-done"), Sense::click());
            ui.painter().circle_filled(k, 20.0, c.accent);
            ui.painter().line_segment([k + vec2(-7.0, 0.5), k + vec2(-2.0, 5.5)], Stroke::new(2.4, Color32::WHITE));
            ui.painter().line_segment([k + vec2(-2.0, 5.5), k + vec2(7.5, -5.5)], Stroke::new(2.4, Color32::WHITE));
            if resp.clicked() {
                self.commit_edit();
            }
            right -= 48.0;
        }
        // search (from the bottom bar), or the title and the clock / Pomodoro pill
        if self.phone_search || !self.search.is_empty() {
            let field = egui::Rect::from_min_max(pos2(left, cy - 18.0), pos2(right - 64.0, cy + 18.0));
            ui.painter().rect_filled(field, 18.0, c.pill);
            let te = crate::nativetext::Edit::singleline(&mut self.search).hint_text("Search").frame(false).font(FontId::proportional(15.0)).text_color(c.text);
            let resp = ui.put(field.shrink2(vec2(14.0, 8.0)), te);
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            let cancel = egui::Rect::from_min_max(pos2(right - 60.0, cy - 18.0), pos2(right, cy + 18.0));
            let cr = ui.interact(cancel, ui.id().with("ph-search-cancel"), Sense::click());
            ui.painter().text(cancel.center(), Align2::CENTER_CENTER, "Cancel", FontId::proportional(15.0), c.accent);
            if cr.clicked() {
                self.search.clear();
                self.phone_search = false;
            }
        } else {
            let t = if landscape { 1.0 } else { ui.ctx().animate_bool_with_time(egui::Id::new("ph-small-title"), self.list_scroll > TITLE_SCROLL, 0.2) };
            if t < 1.0 {
                let mut pill = ui.new_child(egui::UiBuilder::new().max_rect(r));
                pill.multiply_opacity(1.0 - t);
                self.timer_pill(&mut pill, pos2(left, cy), right - left, now());
            }
            if t > 0.0 {
                let mid = pos2(r.center().x, cy + 4.0 * (1.0 - t));
                ui.painter().text(mid, Align2::CENTER_CENTER, self.pname(self.view), bold(17.0), c.text.gamma_multiply(t));
            }
        }
    }

    /// iPhone portrait: the perspectives page — every perspective with its icon, the favourite star, and
    /// ‹ back. Tapping one opens it.
    fn perspectives_page_ui(&mut self, ctx: &egui::Context) {
        let c = th();
        let full = ctx.viewport_rect();
        let safe = ctx.content_rect();
        let mut under = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("persp-page")));
        under.set_clip_rect(full);
        under.rect_filled(full, 0.0, c.bg);
        let mut close = false;
        egui::Area::new(egui::Id::new("persp-page")).order(egui::Order::Foreground).fixed_pos(safe.min).show(ctx, |ui| {
            ui.set_min_size(safe.size());
            ui.set_max_width(safe.width());
            let (bar, _) = ui.allocate_exact_size(vec2(safe.width(), 56.0), Sense::hover());
            let b = egui::Rect::from_center_size(pos2(bar.left() + 34.0, bar.center().y), vec2(40.0, 40.0));
            let r = ui.interact(b, ui.id().with("pp-back"), Sense::click());
            ui.painter().circle_filled(b.center(), 20.0, mix(c.bg, c.text, 0.12));
            let k = b.center();
            ui.painter().line_segment([k + vec2(3.0, -7.0), k + vec2(-4.0, 0.0)], Stroke::new(2.2, c.text));
            ui.painter().line_segment([k + vec2(-4.0, 0.0), k + vec2(3.0, 7.0)], Stroke::new(2.2, c.text));
            close |= r.clicked();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(egui::RichText::new("Perspectives").font(bold(32.0)).color(c.text));
            });
            ui.add_space(10.0);
            let views: Vec<(View, bool, usize)> = self.store.settings.perspectives.iter().enumerate().filter(|(_, p)| p.visible).filter_map(|(i, p)| View::from_key(&p.key).map(|v| (v, p.is_favorite(), i))).collect();
            for (v, fav, i) in views {
                let (row, resp) = ui.allocate_exact_size(vec2(safe.width(), 48.0), Sense::click());
                if self.view == v {
                    ui.painter().rect_filled(row.shrink2(vec2(10.0, 2.0)), 10.0, mix(c.bg, c.text, 0.08));
                }
                draw_icon(ui.painter(), pos2(row.left() + 40.0, row.center().y), v, self.pcolor(v));
                ui.painter().text(pos2(row.left() + 72.0, row.center().y), Align2::LEFT_CENTER, self.pname(v), FontId::proportional(17.0), c.text);
                let star = egui::Rect::from_center_size(pos2(row.right() - 34.0, row.center().y), vec2(40.0, 40.0));
                let sr = ui.interact(star, ui.id().with(("pp-star", i)), Sense::click());
                ui.painter().text(star.center(), Align2::CENTER_CENTER, if fav { "★" } else { "☆" }, FontId::proportional(20.0), if fav { hue("yellow") } else { c.dim });
                if sr.clicked() {
                    self.store.settings.perspectives[i].favorite = Some(!fav);
                    self.touch();
                } else if resp.clicked() {
                    self.go(v);
                    close = true;
                }
            }
        });
        if close {
            self.perspectives_page = false;
        }
    }

    /// iPhone portrait bottom bar: a rounded, raised block with a row of commands —
    /// Quick Open (⚡), Search, New Action (+), Quick Entry (inbox +) — over four perspective tabs (the first four
    /// shown in the sidebar), each with its icon, name and badge; the current one sits on a lighter pill. While the
    /// list scrolls down it shrinks to a round button with the perspective's icon (a tap brings the bar back) and a
    /// capsule with + and Quick Entry; the two cross-fade.
    fn tab_bar(&mut self, ui: &mut egui::Ui, d: &Derived) {
        // One glass shape that splits, like iOS 26's tab bar minimising: the bar's rounded block divides into the
        // round perspective button (left) and the + capsule (right) with a soft spring, rather than two bars
        // cross-fading. The block's halves are drawn as two blobs in the same colour, so mid-way they read as one
        // shape pulling apart; the contents fade across the first and last 60% of the move.
        let raw = ui.ctx().animate_bool_with_time(egui::Id::new("tab-bar-collapse"), self.bar_collapsed, 0.42);
        let e = liquid(raw, self.bar_collapsed);
        let full = ui.max_rect();
        let c = th();
        let fill = mix(c.bg, c.text, if raw < 0.5 { 0.11 } else { 0.14 });
        let bar = Rect::from_min_max(pos2(full.left() + 14.0, full.top() + 4.0), pos2(full.right() - 14.0, full.bottom() - 6.0));
        let y = full.bottom() - 32.0;
        let k = pos2(full.left() + 40.0, y);
        let round = Rect::from_center_size(k, vec2(48.0, 48.0));
        let caps = Rect::from_min_max(pos2(full.right() - 116.0, y - 24.0), pos2(full.right() - 16.0, y + 24.0));
        let lerp_rect = |a: Rect, b: Rect| Rect::from_min_max(a.min + (b.min - a.min) * e, a.max + (b.max - a.max) * e);
        let (left, right) = (lerp_rect(bar, round), lerp_rect(bar, caps));
        let radius = 30.0 + (24.0 - 30.0) * e.clamp(0.0, 1.0);
        let p = ui.painter().clone();
        for r in [left, right] {
            p.rect_filled(r.translate(vec2(0.0, 2.0)), radius, egui::Color32::from_black_alpha(if raw > 0.0 && raw < 1.0 { 45 } else { 70 }));
        }
        for r in [left, right] {
            p.rect_filled(r, radius, fill);
        }
        if raw < 0.6 {
            let mut big = ui.new_child(egui::UiBuilder::new().max_rect(full));
            big.set_clip_rect(left.union(right).intersect(full));
            big.multiply_opacity(1.0 - raw / 0.6);
            if self.bar_collapsed {
                big.disable();
            }
            self.tab_bar_full(&mut big, d);
        }
        if raw > 0.4 {
            let mut small = ui.new_child(egui::UiBuilder::new().max_rect(full));
            small.multiply_opacity((raw - 0.4) / 0.6);
            if !self.bar_collapsed {
                small.disable();
            }
            let resp = small.interact(round, small.id().with("tb-restore"), Sense::click());
            draw_icon(small.painter(), left.center(), self.view, self.pcolor(self.view));
            if resp.clicked() {
                self.bar_collapsed = false;
            }
            self.capsule_cmds(&mut small, right, &[PhoneCmd::New, PhoneCmd::QuickEntry], "tb-small", fill);
        }
    }

    /// iPhone/iPad: an action's commands as a sheet from the bottom (touch and hold a row) — the Mac shortcut menu's
    /// commands with Type, Organize and Schedule listed in full under their headings, big enough to tap; scrolls when
    /// it's taller than the screen. Tapping outside closes it.
    fn row_sheet_ui(&mut self, ctx: &egui::Context) {
        let Some(id) = self.row_sheet else { return };
        let Some(t) = self.store.task(id).cloned() else {
            self.row_sheet = None;
            return;
        };
        let has_kids = self.store.tasks.iter().any(|c| c.parent == Some(id));
        let items = row_menu_items(id, &t, has_kids, self.agentui.available());
        let c = th();
        let safe = ctx.content_rect();
        let w = safe.width().min(520.0) - 16.0;
        let area = egui::Modal::default_area(egui::Id::new("row-sheet")).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -(ctx.viewport_rect().bottom() - safe.bottom()) - 8.0));
        let mut chosen = None;
        let resp = egui::Modal::new(egui::Id::new("row-sheet")).area(area).frame(egui::Frame::new().fill(mix(c.bg, c.text, 0.10)).corner_radius(22).inner_margin(egui::Margin::symmetric(0, 10))).show(ctx, |ui| {
            ui.set_width(w);
            let n = self.selected_tasks().len();
            let title = if n > 1 { format!("{n} items") } else if t.name.is_empty() { "Untitled Item".into() } else { t.name.clone() };
            ui.add_space(2.0);
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(title).strong().size(15.0).color(c.text)));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(safe.height() * 0.72).show(ui, |ui| {
                let mut last = "";
                for (i, (sec, label, _)) in items.iter().enumerate() {
                    if *sec != last {
                        ui.add_space(4.0);
                        if matches!(*sec, "Type" | "Organize" | "Schedule") {
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                ui.label(egui::RichText::new(sec.to_uppercase()).size(11.5).color(c.dim));
                            });
                        } else {
                            let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
                            ui.painter().line_segment([r.left_center() + vec2(16.0, 0.0), r.right_center() - vec2(16.0, 0.0)], Stroke::new(1.0, mix(c.bg, c.text, 0.18)));
                        }
                        last = sec;
                    }
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
                    if resp.is_pointer_button_down_on() || resp.hovered() && !cfg!(target_os = "ios") {
                        ui.painter().rect_filled(r.shrink2(vec2(8.0, 2.0)), 10.0, mix(c.bg, c.text, 0.16));
                    }
                    let col = if *sec == "Delete" { c.overdue } else { c.text };
                    ui.painter().text(r.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(16.0), col);
                    if resp.clicked() {
                        chosen = Some(i);
                    }
                }
            });
        });
        if let Some(i) = chosen {
            let act = items.into_iter().nth(i).map(|x| x.2);
            self.row_sheet = None;
            if let Some(a) = act {
                self.apply(vec![a]);
            }
        } else if resp.should_close() {
            self.row_sheet = None;
        }
    }

    /// Select mode's bar (in place of the tab bar): how many are picked and Done, over the commands that act on all of
    /// them — Copy, Copy with Context, Flag, Complete, Delete.
    fn select_bar(&mut self, ui: &mut egui::Ui) {
        let c = th();
        let full = ui.max_rect();
        let bar = Rect::from_min_max(pos2(full.left() + 14.0, full.top() + 4.0), pos2(full.right() - 14.0, full.bottom() - 6.0));
        let fill = mix(c.bg, c.text, 0.11);
        let p = ui.painter().clone();
        p.rect_filled(bar.translate(vec2(0.0, 2.0)), 30.0, egui::Color32::from_black_alpha(70));
        p.rect_filled(bar, 30.0, fill);
        let n = self.selected_tasks().len();
        let top = bar.top() + 26.0;
        p.text(pos2(bar.left() + 24.0, top), Align2::LEFT_CENTER, if n == 0 { "Tap items to select".to_string() } else { format!("{n} Selected") }, FontId::proportional(15.0), if n == 0 { c.dim } else { c.text });
        let done = Rect::from_center_size(pos2(bar.right() - 46.0, top), vec2(72.0, 34.0));
        p.rect_filled(done, 17.0, c.accent);
        p.text(done.center(), Align2::CENTER_CENTER, "Done", FontId::proportional(15.0), c.on_accent);
        if ui.interact(done, ui.id().with("sel-done"), Sense::click()).clicked() {
            self.select_mode = false;
        }
        let cmds: [(&str, u8); 5] = [("Copy", 0), ("Copy+Context", 1), ("Flag", 2), ("Complete", 3), ("Delete", 4)];
        let row = Rect::from_min_max(pos2(bar.left() + 8.0, bar.bottom() - 58.0), pos2(bar.right() - 8.0, bar.bottom() - 10.0));
        let w = row.width() / cmds.len() as f32;
        for (i, (label, k)) in cmds.into_iter().enumerate() {
            let cell = Rect::from_min_size(pos2(row.left() + w * i as f32, row.top()), vec2(w, row.height()));
            let resp = ui.interact(cell, ui.id().with(("sel-cmd", i)), Sense::click());
            let on = n > 0;
            if resp.is_pointer_button_down_on() && on {
                p.rect_filled(cell.shrink(3.0), 14.0, mix(fill, c.text, 0.10));
            }
            let col = if !on { c.faint } else if k == 4 { c.overdue } else { c.text };
            p.text(cell.center(), Align2::CENTER_CENTER, label, FontId::proportional(if label.len() > 8 { 12.0 } else { 14.0 }), col);
            if resp.clicked() && on {
                match k {
                    0 | 1 => {
                        copy_text(&copy_list(&self.store, &self.selected_tasks(), k == 1));
                        self.toast = Some((format!("Copied {n} item{}", if n == 1 { "" } else { "s" }), None));
                    }
                    2 => self.db.run.push(keys::Action::ToggleFlag),
                    3 => self.db.run.push(keys::Action::ToggleComplete),
                    _ => {
                        self.db.run.push(keys::Action::Delete);
                        self.select_mode = false;
                    }
                }
            }
        }
    }

    fn tab_bar_full(&mut self, ui: &mut egui::Ui, d: &Derived) {
        let c = th();
        let full = ui.max_rect();
        let bar = egui::Rect::from_min_max(pos2(full.left() + 14.0, full.top() + 4.0), pos2(full.right() - 14.0, full.bottom() - 6.0));
        let p = ui.painter().clone();
        let ink = mix(c.text, c.dim, 0.15);
        // commands
        let row1 = bar.top() + 26.0;
        for (i, cmd) in [PhoneCmd::QuickOpen, PhoneCmd::Search, PhoneCmd::New, PhoneCmd::QuickEntry].into_iter().enumerate() {
            let k = pos2(bar.left() + bar.width() * (i as f32 + 0.5) / 4.0, row1);
            cmd_glyph(&p, k, cmd, ink, mix(c.bg, c.text, 0.11));
            if ui.interact(egui::Rect::from_center_size(k, vec2(56.0, 40.0)), ui.id().with(("tb-cmd", i)), Sense::click()).clicked() {
                self.phone_cmd(cmd);
            }
        }
        // the subsystems' lights, small along the bar's top edge
        self.status_dots(ui, pos2(bar.center().x, bar.top() + 7.0));
        // perspective tabs: the favourites (starred), four across; more scroll sideways
        let inbox = self.inbox_count(d);
        let favs: Vec<View> = self.store.settings.perspectives.iter().filter(|p| p.visible && p.is_favorite()).filter_map(|p| View::from_key(&p.key)).filter(|v| self.visible_views().contains(v)).collect();
        let cell_w = bar.width() / 4.0;
        let strip = egui::Rect::from_min_max(pos2(bar.left(), bar.bottom() - 64.0), pos2(bar.right(), bar.bottom() - 4.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip));
        child.set_clip_rect(strip.shrink2(vec2(6.0, 0.0)));
        egui::ScrollArea::horizontal().id_salt("tab-strip").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            let (row, _) = ui.allocate_exact_size(vec2(cell_w * favs.len().max(4) as f32, strip.height()), Sense::hover());
            let p = ui.painter().clone();
            for (i, v) in favs.iter().copied().enumerate() {
                let cell = egui::Rect::from_center_size(pos2(row.left() + cell_w * (i as f32 + 0.5), row.center().y), vec2(cell_w - 8.0, 56.0));
                let resp = ui.interact(cell, ui.id().with(("tab", i)), Sense::click());
                if self.view == v {
                    p.rect_filled(cell, 22.0, mix(c.bg, c.text, 0.20));
                    // a newly opened perspective's tab scrolls into view
                    let seen = egui::Id::new("tab-strip-seen");
                    if ui.data(|d| d.get_temp::<View>(seen)) != Some(v) {
                        ui.data_mut(|d| d.insert_temp(seen, v));
                        ui.scroll_to_rect(cell, Some(egui::Align::Max));
                    }
                }
                draw_icon(&p, cell.center() - vec2(0.0, 8.0), v, self.pcolor(v));
                p.text(cell.center() + vec2(0.0, 17.0), Align2::CENTER_CENTER, self.pname(v), FontId::proportional(10.5), mix(c.text, c.dim, 0.25));
                if v == View::Inbox && inbox > 0 {
                    let bc = cell.center() + vec2(14.0, -24.0);
                    let txt = inbox.to_string();
                    p.rect_filled(egui::Rect::from_center_size(bc, vec2((10.0 + 7.0 * txt.len() as f32).max(20.0), 17.0)), 9.0, c.badge);
                    p.text(bc, Align2::CENTER_CENTER, txt, FontId::proportional(11.0), c.text);
                }
                if resp.clicked() {
                    self.go(v);
                }
            }
        });
    }

    /// A floating capsule of phone commands (the shrunk portrait bar; landscape's bottom corners).
    fn capsule(&mut self, ui: &mut egui::Ui, rect: Rect, cmds: &[PhoneCmd], id: &str) {
        let c = th();
        let bg = mix(c.bg, c.text, 0.14);
        let p = ui.painter().clone();
        p.rect_filled(rect.translate(vec2(0.0, 2.0)), rect.height() / 2.0, egui::Color32::from_black_alpha(70));
        p.rect_filled(rect, rect.height() / 2.0, bg);
        self.capsule_cmds(ui, rect, cmds, id, bg);
    }

    /// A capsule's commands, evenly across `rect` (its background drawn by the caller).
    fn capsule_cmds(&mut self, ui: &mut egui::Ui, rect: Rect, cmds: &[PhoneCmd], id: &str, bg: egui::Color32) {
        let c = th();
        let p = ui.painter().clone();
        let w = rect.width() / cmds.len() as f32;
        for (i, cmd) in cmds.iter().copied().enumerate() {
            let cell = Rect::from_min_size(rect.min + vec2(w * i as f32, 0.0), vec2(w, rect.height()));
            cmd_glyph(&p, cell.center(), cmd, mix(c.text, c.dim, 0.15), bg);
            if ui.interact(cell, ui.id().with((id, i)), Sense::click()).clicked() {
                self.phone_cmd(cmd);
            }
        }
    }

    fn phone_cmd(&mut self, cmd: PhoneCmd) {
        match cmd {
            PhoneCmd::QuickOpen => self.quick_open = Some(String::new()),
            PhoneCmd::Search => {
                self.phone_search = true;
                self.focus_search = true;
            }
            PhoneCmd::New => self.new_action(),
            PhoneCmd::QuickEntry => self.quick = Some(QuickEntry::default()),
        }
    }

    fn rail(&mut self, ui: &mut egui::Ui, d: &Derived) {
        let inbox_badge = self.inbox_count(d);
        let views = self.visible_views();
        // Full content height, so the scroll area can reach the last perspective on short windows.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0 + views.len() as f32 * 64.0 + 12.0), Sense::hover());
        // Selection highlight slides between perspectives (animated relative to the top, so scrolling doesn't animate it).
        let sel_i = views.iter().position(|v| *v == self.view);
        let ctx = ui.ctx().clone();
        if let Some(i) = sel_i {
            let y = r.top() + ctx.animate_value_with_time(ui.id().with("rail-sel"), 16.0 + 31.0 + i as f32 * 64.0, 0.16);
            let cell = Rect::from_center_size(pos2(r.left() + 40.0, y + 7.0), vec2(66.0, 62.0));
            ui.painter().rect_filled(cell, RAIL_SEL_RADIUS, th().rail_sel);
            let key = (self.view, ui.clip_rect().height() as u32);
            if self.rail_seen != Some(key) {
                // keep asking until it's on screen: the first frames don't know the final height yet
                let home = Rect::from_center_size(pos2(r.left() + 40.0, r.top() + 16.0 + 38.0 + i as f32 * 64.0), vec2(66.0, 62.0));
                if ui.clip_rect().contains_rect(home) {
                    self.rail_seen = Some(key);
                } else {
                    ui.scroll_to_rect(home, None);
                }
            }
        }
        for (i, v) in views.into_iter().enumerate() {
            let c = pos2(r.left() + 40.0, r.top() + 16.0 + 31.0 + i as f32 * 64.0);
            let cell = Rect::from_center_size(c + vec2(0.0, 7.0), vec2(66.0, 62.0));
            let resp = ui.interact(cell, ui.id().with(("rail", i)), Sense::click());
            if self.view != v && resp.hovered() {
                ui.painter().rect_filled(cell, RAIL_SEL_RADIUS, mix(th().rail, th().rail_sel, 0.5));
            }
            draw_icon(ui.painter(), c - vec2(0.0, 6.0), v, self.pcolor(v));
            let name = self.pname(v);
            ui.painter().text(c + vec2(0.0, 22.0), Align2::CENTER_CENTER, &name, FontId::proportional(11.5), mix(th().text, th().dim, 0.25));
            if v == View::Inbox && inbox_badge > 0 {
                let bc = c + vec2(14.0, -21.0);
                let txt = inbox_badge.to_string();
                let w = 10.0 + 7.0 * txt.len() as f32;
                ui.painter().rect_filled(Rect::from_center_size(bc, vec2(w.max(22.0), 18.0)), 9.0, th().badge);
                ui.painter().text(bc, Align2::CENTER_CENTER, txt, FontId::proportional(12.0), th().text);
            }
            let resp = resp.on_hover_text(match v.shortcut() {
                Some(k) => format!("{name}  ⌘{}", k.name()),
                None => name.clone(),
            });
            if resp.clicked() {
                self.go(v);
            }
            resp.context_menu(|ui| {
                if ui.button("Hide from sidebar").clicked() {
                    let k = v.key();
                    if let Some(p) = self.store.settings.perspectives.iter_mut().find(|p| p.key == k) {
                        p.visible = false;
                    }
                    self.touch();
                    ui.close();
                }
                if matches!(v, View::Custom(_)) && ui.button("Edit Perspective…").clicked() {
                    self.editor = Some(v.key());
                    ui.close();
                }
                if ui.button("Edit Perspectives…").clicked() {
                    self.show_settings = true;
                    self.settings_tab = 2;
                    ui.close();
                }
            });
        }
    }

    // ---------- middle column ----------
    fn column(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt) {
        let full = ui.max_rect();
        ui.painter().line_segment([full.right_top(), full.right_bottom()], Stroke::new(1.0, th().line));
        let mut acts: Vec<Act> = vec![];
        // iPhone on its side: the sidebar heads itself with ▯ · ☰ · Edit, and its + floats
        let landscape = phone_landscape(ui.ctx());
        let list = if landscape {
            Rect::from_min_max(full.min + vec2(14.0, TOOLBAR_H + 4.0), full.max - vec2(10.0, 0.0))
        } else {
            Rect::from_min_max(full.min + vec2(14.0, 10.0), full.max - vec2(10.0, 44.0))
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
        // drag a project onto a folder (or a project inside it) to move it there; onto "No Folder" to take it out
        let mut folder_drop: Option<(Id, String)> = None;
        egui::ScrollArea::vertical().id_salt("colscroll").show(&mut child, |ui| {
            ui.set_width(list.width());
            match self.view {
                View::Projects => {
                    let ps: Vec<&Project> = self.store.projects.iter().filter(|p| matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).collect();
                    let folders = self.store.folder_names();
                    let dropped: Vec<String> = self.store.folders.iter().filter(|f| f.dropped.is_some()).map(|f| f.name.clone()).collect();
                    project_list(ui, ps, &folders, &dropped, self.filter, &mut acts, &mut folder_drop);
                }
                View::Tags | View::Completed | View::Changed => {
                    for (gid, depth) in self.store.tag_tree() {
                        let Some(g) = self.store.tag(gid) else { continue };
                        let on = self.filter == Filter::Tag(Some(g.id));
                        let label = if g.on_hold { format!("{} (on hold)", g.name) } else { g.name.clone() };
                        let r = column_item(ui, &label, on, 16.0 * depth as f32, ColIcon::Tag, hue("violet"));
                        if r.clicked() {
                            acts.push(Act::Filter(if on { Filter::All } else { Filter::Tag(Some(g.id)) }));
                            acts.push(Act::Select(Sel::Tag(g.id)));
                        }
                    }
                    let on = self.filter == Filter::Tag(None);
                    if column_item(ui, "Untagged", on, 0.0, ColIcon::Tag, hue("violet")).clicked() {
                        acts.push(Act::Filter(if on { Filter::All } else { Filter::Tag(None) }));
                    }
                }
                View::Forecast => self.calendar(ui, d, now, &mut acts),
                View::Nearby => {
                    ui.label(egui::RichText::new("My location").font(bold(13.5)).color(th().text));
                    let here = self.store.settings.here.as_ref().map(|h| h.label.clone()).unwrap_or_else(|| "Not set".into());
                    ui.label(egui::RichText::new(here).color(th().dim).size(12.5));
                    let r = ui.add(crate::nativetext::Edit::singleline(&mut self.place_query).hint_text("Address or place").desired_width(f32::INFINITY));
                    if (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) || ui.button("Set My Location").clicked() {
                        let q = self.place_query.clone();
                        self.geocode(q, GeoFor::Here, false);
                    }
                    if !self.geo_msg.is_empty() {
                        ui.label(egui::RichText::new(&self.geo_msg).color(th().dim).size(12.0));
                    }
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("Places").font(bold(13.5)).color(th().text));
                    let placed: Vec<(Id, String, Place)> = self.store.tags.iter().filter_map(|g| g.place.clone().map(|p| (g.id, g.name.clone(), p))).collect();
                    if placed.is_empty() {
                        ui.label(egui::RichText::new("Give a tag a place in its inspector, or select a tag and click the map.").color(th().dim).size(12.5));
                    }
                    for (gid, name, place) in placed {
                        let on = self.filter == Filter::Tag(Some(gid));
                        if column_item(ui, &name, on, 0.0, ColIcon::Tag, hue("green")).on_hover_text(&place.label).clicked() {
                            acts.push(Act::Filter(if on { Filter::All } else { Filter::Tag(Some(gid)) }));
                            acts.push(Act::Select(Sel::Tag(gid)));
                            self.map.lat = place.lat;
                            self.map.lon = place.lon;
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Other tags").font(bold(13.0)).color(th().dim));
                    for g in self.store.tags.iter().filter(|g| g.place.is_none()) {
                        if column_item(ui, &g.name, self.sel == Some(Sel::Tag(g.id)), 0.0, ColIcon::Tag, th().dim).on_hover_text("Select, then click the map to place it").clicked() {
                            acts.push(Act::Select(Sel::Tag(g.id)));
                        }
                    }
                }
                _ => {}
            }
        });
        if landscape {
            self.column_bar(ui, Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR_H)));
        }
        // footer: + and gear (sideways: a floating round +, where there's something to add; Settings is in ⋯)
        let fy = full.bottom() - 22.0;
        let plus = if landscape { Rect::from_center_size(pos2(full.left() + 40.0, full.bottom() - 26.0), vec2(44.0, 44.0)) } else { Rect::from_center_size(pos2(full.left() + 33.0, fy), vec2(24.0, 24.0)) };
        let gear = Rect::from_center_size(pos2(full.left() + 64.0, fy), vec2(24.0, 24.0));
        let can_add = !landscape || matches!(self.view, View::Projects | View::Tags | View::Completed | View::Changed);
        if landscape {
            if can_add {
                ui.painter().circle_filled(plus.center() + vec2(0.0, 2.0), 22.0, Color32::from_black_alpha(70));
                ui.painter().circle_filled(plus.center(), 22.0, mix(th().bg, th().text, 0.14));
                cmd_glyph(ui.painter(), plus.center(), PhoneCmd::New, mix(th().text, th().dim, 0.15), th().bg);
            }
        } else {
            ui.painter().text(plus.center(), Align2::CENTER_CENTER, "+", FontId::proportional(20.0), th().dim);
            draw_gear(ui.painter(), gear.center(), th().dim);
        }
        if can_add && ui.interact(plus, ui.id().with("colplus"), Sense::click()).on_hover_text("Add").clicked() {
            match self.view {
                View::Tags | View::Completed | View::Changed => {
                    let id = self.store.new_id();
                    self.store.tags.push(Tag { id, name: "New Tag".into(), on_hold: false, place: None, parent: None, modified: None });
                    self.sel = Some(Sel::Tag(id));
                }
                _ => {
                    let id = self.store.add_project("New Project", ProjectKind::Parallel);
                    self.go(View::Projects);
                    self.sel = Some(Sel::Project(id));
                }
            }
            self.touch();
        }
        if !landscape && ui.interact(gear, ui.id().with("gear"), Sense::click()).on_hover_text("Settings and agent connection").clicked() {
            self.show_settings = !self.show_settings;
        }
        if let Some((pid, folder)) = folder_drop {
            if let Some(p) = self.store.project_mut(pid) {
                if p.folder != folder {
                    p.folder = folder;
                    p.modified = now;
                    self.touch();
                }
            }
        }
        self.apply(acts);
    }

    /// iPhone on its side: the sidebar's own top buttons — ▯ hides the sidebar, ☰ opens the
    /// perspectives page, Edit opens the inspector for the selected project or tag (dimmed when there's none).
    fn column_bar(&mut self, ui: &mut egui::Ui, r: Rect) {
        let c = th();
        let cy = r.center().y + 2.0;
        let ic = mix(c.text, c.dim, 0.15);
        let button = |ui: &mut egui::Ui, rect: Rect, id: &str| -> egui::Response {
            let resp = ui.interact(rect, ui.id().with(id), Sense::click());
            ui.painter().rect_filled(rect, 20.0, if resp.hovered() { c.pill_hover } else { c.pill });
            resp
        };
        let k = pos2(r.left() + 34.0, cy);
        if button(ui, Rect::from_center_size(k, vec2(40.0, 40.0)), "cb-side").clicked() {
            self.show_sidebar = false;
        }
        ui.painter().rect_stroke(Rect::from_center_size(k, vec2(18.0, 15.0)), 3.0, Stroke::new(1.6, ic), StrokeKind::Middle);
        ui.painter().line_segment([k + vec2(-3.0, -7.5), k + vec2(-3.0, 7.5)], Stroke::new(1.6, ic));
        let edit = Rect::from_min_max(pos2(r.right() - 76.0, cy - 20.0), pos2(r.right() - 12.0, cy + 20.0));
        let editable = matches!(self.sel, Some(Sel::Project(_) | Sel::Tag(_)));
        if button(ui, edit, "cb-edit").clicked() && editable {
            self.show_inspector = true;
        }
        ui.painter().text(edit.center(), Align2::CENTER_CENTER, "Edit", FontId::proportional(16.0), if editable { c.text } else { c.faint });
        let k = pos2(edit.left() - 28.0, cy);
        if button(ui, Rect::from_center_size(k, vec2(40.0, 40.0)), "cb-list").clicked() {
            self.perspectives_page = true;
        }
        for i in -1..=1 {
            let y = k.y + i as f32 * 5.5;
            ui.painter().circle_filled(pos2(k.x - 7.0, y), 1.3, ic);
            ui.painter().line_segment([pos2(k.x - 3.5, y), pos2(k.x + 8.0, y)], Stroke::new(1.6, ic));
        }
    }

    fn calendar(&self, ui: &mut egui::Ui, d: &Derived, now: Dt, acts: &mut Vec<Act>) {
        let today = now.date();
        let w = ui.available_width();
        let count = |f: &dyn Fn(&Task) -> bool| self.store.tasks.iter().filter(|t| d.st(t.id).remaining() && f(t)).count();
        let tile = |ui: &mut egui::Ui, label: &str, n: usize, on: bool| {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 48.0), Sense::click());
            ui.painter().rect_filled(rect, 6.0, if on { th().sel_row } else { th().field });
            ui.painter().text(rect.center() - vec2(0.0, 9.0), Align2::CENTER_CENTER, label, FontId::proportional(15.0), th().dim);
            ui.painter().text(rect.center() + vec2(0.0, 11.0), Align2::CENTER_CENTER, n.to_string(), FontId::proportional(15.0), th().text);
            resp
        };
        let past = count(&|t| t.due.is_some_and(|x| x.date() < today));
        if tile(ui, "Past", past, self.filter == Filter::Past).clicked() {
            acts.push(Act::Filter(if self.filter == Filter::Past { Filter::All } else { Filter::Past }));
        }
        ui.add_space(8.0);
        let cw = w / 7.0;
        let (hdr, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
        for (i, n) in ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"].iter().enumerate() {
            ui.painter().text(pos2(hdr.left() + cw * (i as f32 + 0.5), hdr.center().y), Align2::CENTER_CENTER, *n, FontId::proportional(11.0), th().dim);
        }
        let start = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
        let (grid, _) = ui.allocate_exact_size(vec2(w, 5.0 * 51.0), Sense::hover());
        for k in 0..35 {
            let day = start + Duration::days(k);
            let cell = Rect::from_min_size(pos2(grid.left() + cw * (k % 7) as f32, grid.top() + 51.0 * (k / 7) as f32), vec2(cw - 1.0, 50.0));
            let resp = ui.interact(cell, ui.id().with(("cal", k)), Sense::click());
            let on = self.filter == Filter::Day(day);
            let fill = if day < today { mix(th().bg, th().field, 0.4) } else if on { th().sel_row } else { th().field };
            ui.painter().rect_filled(cell, 0.0, fill);
            let label = if day == today { "Today".to_string() } else if day.day() == 1 { day.format("%b").to_string() } else { day.day().to_string() };
            ui.painter().text(cell.right_top() + vec2(-4.0, 9.0), Align2::RIGHT_CENTER, label, FontId::proportional(11.0), th().dim);
            let n = count(&|t| self.forecast_includes(t, day));
            if n > 0 && day >= today {
                ui.painter().text(cell.center() + vec2(0.0, 5.0), Align2::CENTER_CENTER, n.to_string(), FontId::proportional(15.0), th().text);
            }
            // Calendar events that day: one dot per calendar colour (up to 4)
            let mut dots: Vec<Color32> = vec![];
            for e in self.cal.iter().filter(|e| calendar::on_day(e, day)) {
                let c = e.color.map(|[r, g, b]| Color32::from_rgb(r, g, b)).unwrap_or(th().accent);
                if !dots.contains(&c) && dots.len() < 4 {
                    dots.push(c);
                }
            }
            for (j, c) in dots.iter().enumerate() {
                let x = cell.center().x + (j as f32 - (dots.len() as f32 - 1.0) / 2.0) * 7.0;
                ui.painter().circle_filled(pos2(x, cell.bottom() - 7.0), 2.5, *c);
            }
            if resp.clicked() && day >= today {
                acts.push(Act::Filter(if on { Filter::All } else { Filter::Day(day) }));
            }
        }
        ui.add_space(8.0);
        let last = today + Duration::days(6);
        let fut = count(&|t| t.due.or(t.planned).is_some_and(|x| x.date() > last));
        if tile(ui, "Future", fut, self.filter == Filter::Future).clicked() {
            acts.push(Act::Filter(if self.filter == Filter::Future { Filter::All } else { Filter::Future }));
        }
        // Calendar events in Forecast need macOS permission
        if self.store.settings.calendar_events && self.shot.is_none() {
            ui.add_space(10.0);
            match self.cal_access {
                calendar::Access::NotAsked => {
                    ui.label(egui::RichText::new("See your Calendar events alongside your actions.").color(th().dim).size(12.0));
                    if ui.button("Allow Calendar Access").clicked() {
                        acts.push(Act::AllowCalendar);
                    }
                }
                calendar::Access::Denied => {
                    ui.label(egui::RichText::new("GTDCraft requires system permissions.").color(th().dim).size(12.0));
                    if ui.button("Open System Settings").clicked() {
                        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars").spawn();
                    }
                }
                calendar::Access::Granted => {
                    if !self.cal_msg.is_empty() {
                        ui.label(egui::RichText::new(&self.cal_msg).color(th().dim).size(12.0));
                    }
                }
            }
        }
    }

    fn apply(&mut self, acts: Vec<Act>) {
        for a in acts {
            match a {
                Act::Select(s) => {
                    if self.editing.is_some() {
                        self.commit_edit();
                    }
                    // an in-row note being typed is kept when another row is tapped
                    if let Some((nid, text, _)) = self.note_inline.take() {
                        if let Some(t) = self.store.task_mut(nid).filter(|t| t.note != text) {
                            t.note = text;
                            t.modified = now();
                            self.touch();
                        }
                    }
                    self.sel = Some(s);
                    self.picked.clear();
                }
                Act::PickToggle(id) => self.pick_toggle(id),
                Act::AgentSend(id) => self.agent_compose(id, None),
                Act::RowSheet(id) => {
                    if !(self.sel == Some(Sel::Task(id)) || self.picked.contains(&id)) {
                        self.sel = Some(Sel::Task(id));
                        self.picked.clear();
                    }
                    self.row_sheet = Some(id);
                }
                Act::Copy(s, meta) => {
                    let inside = self.sel == Some(s) || matches!(s, Sel::Task(id) if self.picked.contains(&id));
                    let ids: Vec<Id> = match s {
                        _ if inside => self.selected_tasks(),
                        Sel::Task(id) => vec![id],
                        _ => vec![],
                    };
                    copy_text(&copy_list(&self.store, &ids, meta));
                    self.toast = Some((format!("Copied {} item{}", ids.len(), if ids.len() == 1 { "" } else { "s" }), None));
                }
                Act::Attach(s, folder) => {
                    // the clicked row, or every selected row
                    // when it's one of them
                    let inside = self.sel == Some(s) || matches!(s, Sel::Task(id) if self.picked.contains(&id));
                    let mut targets = vec![s];
                    if inside {
                        targets.extend(self.sel.filter(|x| *x != s));
                        targets.extend(self.picked.iter().map(|&id| Sel::Task(id)).filter(|x| *x != s));
                    } else {
                        self.sel = Some(s);
                        self.picked.clear();
                    }
                    self.attach_picked(&targets, folder);
                }
                Act::AllowCalendar => self.cal_ask = true,
                Act::Menu(s, a) => {
                    // right-clicking inside the selection keeps it (the command applies to all of it)
                    let inside = self.sel == Some(s) || matches!(s, Sel::Task(id) if self.picked.contains(&id));
                    if !inside {
                        self.sel = Some(s);
                        self.picked.clear();
                    } else if let Sel::Task(id) = s {
                        if self.sel != Some(s) {
                            // make the clicked row the main one, keep the rest picked
                            if let Some(Sel::Task(cur)) = self.sel {
                                self.picked.retain(|x| *x != id);
                                self.picked.push(cur);
                            }
                            self.sel = Some(s);
                        }
                    }
                    let n = now();
                    let d = self.store.derive(n);
                    self.run(a, &d, n);
                }
                Act::Schedule(id, k) => {
                    let n = now();
                    let st = self.store.settings.clone();
                    let ids: Vec<Id> = if self.sel == Some(Sel::Task(id)) || self.picked.contains(&id) { self.selected_tasks() } else { vec![id] };
                    for i in ids {
                        let Some(t) = self.store.task_mut(i) else { continue };
                        let (today, tomorrow) = (n.date(), n.date() + Duration::days(1));
                        match k {
                            Schedule::DueToday => t.due = Some(today.and_time(st.default_due_time)),
                            Schedule::DueTomorrow => t.due = Some(tomorrow.and_time(st.default_due_time)),
                            Schedule::DeferTomorrow => t.defer = Some(tomorrow.and_time(st.default_defer_time)),
                            Schedule::PlanToday => t.planned = Some(today.and_time(st.default_planned_time)),
                            Schedule::PlanTomorrow => t.planned = Some(tomorrow.and_time(st.default_planned_time)),
                        }
                        t.modified = n;
                    }
                    self.touch();
                }
                Act::GroupType(id, sequential) => {
                    if let Some(t) = self.store.task_mut(id) {
                        t.sequential = sequential;
                        t.modified = now();
                    }
                    self.touch();
                }
                Act::CompleteWithLast(id) => {
                    if let Some(t) = self.store.task_mut(id) {
                        t.complete_with_last = !t.complete_with_last;
                        t.modified = now();
                    }
                    self.touch();
                }
                Act::AddToProject(pid) => {
                    let id = self.store.add_task("", Some(pid), None);
                    self.collapsed.remove(&pid);
                    self.sel = Some(Sel::Task(id));
                    self.picked.clear();
                    if self.paged {
                        self.show_inspector = false;
                    }
                    self.edit_title(id);
                    self.touch();
                }
                Act::PickRange(id) => {
                    let d = self.store.derive(now());
                    let rows = self.row_ids(&d, now());
                    self.pick_range(id, &rows);
                }
                Act::ToggleDone(id) => self.toggle_done(id),
                Act::ToggleDrop(id) => self.toggle_drop(id),
                Act::ToggleFlag(id) => self.toggle_flag(Sel::Task(id)),
                Act::Collapse(id) => {
                    if !self.collapsed.remove(&id) {
                        self.collapsed.insert(id);
                    }
                }
                Act::CollapseHeader(h) => {
                    if !self.collapsed_headers.remove(&h) {
                        self.collapsed_headers.insert(h);
                    }
                }
                Act::Edit(id) => {
                    self.sel = Some(Sel::Task(id));
                    self.editing = Some(id);
                    self.edit_buf = self.store.task(id).map(|t| t.name.clone()).unwrap_or_default();
                    self.edit_focus = true;
                }
                Act::ProjectStatus(id, st) => {
                    self.store.set_project_status(id, st, now());
                    self.touch();
                }
                Act::NewFolder(after) => {
                    let after = after.or_else(|| self.selected_folder());
                    let name = self.store.add_folder(after.as_deref());
                    self.touch();
                    self.renaming_folder = Some((name.clone(), name));
                }
                Act::RenameFolder(f) => {
                    let short = f.rsplit(model::FOLDER_SEP).next().unwrap_or(&f).to_string();
                    self.renaming_folder = Some((f.clone(), short));
                }
                Act::DropFolder(f, drop) => {
                    self.store.set_folder_dropped(&f, drop);
                    self.touch();
                }
                Act::NewSubfolder(parent) => {
                    let name = self.store.add_subfolder(&parent);
                    self.touch();
                    self.renaming_folder = Some((name, "Untitled Folder".into()));
                }
                Act::NewProject => {
                    let id = self.store.add_project("New Project", ProjectKind::Parallel);
                    self.sel = Some(Sel::Project(id));
                    self.show_inspector = true;
                    self.touch();
                }
                Act::DeleteFolder(f) => {
                    self.store.delete_folder(&f);
                    self.sel = self.sel.filter(|s| !matches!(s, Sel::Project(p) if self.store.project(*p).is_none()));
                    self.filter = if matches!(self.filter, Filter::Project(p) if self.store.project(p).is_none()) { Filter::All } else { self.filter };
                    self.touch();
                }
                Act::Filter(f) => self.filter = f,
                Act::OpenField(id, f) => self.open_field(id, f),
                Act::EditNote(id) if self.paged => {
                    // iPhone: edit the note right in the row, rather than switching to the inspector page
                    self.sel = Some(Sel::Task(id));
                    self.note_inline = Some((id, self.store.task(id).map(|t| t.note.clone()).unwrap_or_default(), true));
                }
                Act::EditNote(id) => {
                    self.sel = Some(Sel::Task(id));
                    self.show_inspector = true;
                    self.note_edit = Some(id);
                    self.focus_field = Some(keys::Action::EditNote);
                }
            }
        }
    }

    // ---------- outline ----------
    /// The list's header: the large title over a count line (or Forecast's day strip), Review's Mark Reviewed, and a
    /// rule. iPhone landscape has no large title (the top bar shows it). Returns where the list starts.
    fn outline_header(&mut self, ui: &mut egui::Ui, full: Rect, rows: &[Row], d: &Derived, now: Dt, landscape: bool) -> f32 {
        let p = ui.painter().clone();
        let left = full.left() + 22.0;
        let mut header_bottom = if landscape { full.top() } else { full.top() + 60.0 };
        if !landscape {
            p.text(pos2(left, full.top() + 24.0), Align2::LEFT_CENTER, self.pname(self.view), bold(27.0), self.pcolor(self.view));
            if self.view == View::Forecast {
                self.forecast_strip(ui, d, now, Rect::from_min_max(pos2(full.left() + 10.0, full.top() + 44.0), pos2(full.right() - 10.0, full.top() + 90.0)));
                header_bottom = full.top() + 92.0;
            } else if matches!(self.view, View::Tags | View::Projects) && phone_portrait(ui.ctx()) {
                // iPhone portrait has no sidebar: the tags (or projects) as a row of chips to pick from, with ☰ for
                // the whole list as a page (the person: "no way to display the tags/projects list")
                self.tag_chips(ui, Rect::from_min_max(pos2(full.left() + 12.0, full.top() + 44.0), pos2(full.right() - 8.0, full.top() + 82.0)));
                header_bottom = full.top() + 88.0;
            } else {
                p.text(pos2(left, full.top() + 46.0), Align2::LEFT_CENTER, self.subtitle(rows), FontId::proportional(13.0), mix(th().text, th().dim, 0.35));
            }
        }
        if self.view == View::Review {
            // the Review Guru: a slacker-dude icon that launches an assisted grooming pass over the whole database.
            // It's greyed during the zen cooldown (Settings › Organization), with a tool tip saying how long is left.
            let gtop = if landscape { full.top() + 8.0 } else { full.top() + 10.0 };
            let gb = Rect::from_min_size(pos2(full.right() - 42.0, gtop), vec2(34.0, 34.0));
            let left = self.store.settings.guru_days_left(now);
            let ready = left <= 0;
            let gresp = ui.interact(gb, ui.id().with("guru"), Sense::click());
            let gcol = if !ready { th().dim } else if gresp.hovered() { th().accent } else { mix(th().accent, th().text, 0.15) };
            if gresp.hovered() && ready {
                p.circle_filled(gb.center(), 17.0, mix(th().bg, th().accent, 0.14));
            }
            draw_guru(&p, gb.center(), gcol);
            let gresp = if ready {
                gresp.on_hover_text("Review Guru — an assisted grooming pass over your whole database")
            } else {
                gresp.on_hover_text(format!("Relax man, zen is now. You have {left} more day{} before your next guru sesh.", if left == 1 { "" } else { "s" }))
            };
            if ready && gresp.clicked() {
                self.guru_compose();
            }
            if let Some(Sel::Project(pid)) = self.sel {
                let top = if landscape {
                    header_bottom = full.top() + 44.0;
                    full.top() + 8.0
                } else {
                    full.top() + 16.0
                };
                let br = Rect::from_min_size(pos2(full.right() - 150.0 - 48.0, top), vec2(130.0, 28.0));
                let resp = ui.interact(br, ui.id().with("markrev"), Sense::click());
                p.rect_filled(br, 7.0, if resp.hovered() { th().accent.gamma_multiply(1.15) } else { th().accent });
                p.text(br.center(), Align2::CENTER_CENTER, "Mark Reviewed", FontId::proportional(13.5), Color32::WHITE);
                if resp.clicked() {
                    self.store.mark_reviewed(pid, now);
                    self.touch();
                    // manual: "then presents the next project in the list"
                    self.sel = self.rows(d, now).iter().find_map(|r| match r {
                        Row::Project(x) if *x != pid => Some(Sel::Project(*x)),
                        _ => None,
                    });
                }
            }
        }
        if header_bottom > full.top() {
            p.line_segment([pos2(full.left() + 10.0, header_bottom), pos2(full.right() - 8.0, header_bottom)], Stroke::new(1.0, th().line));
        }
        header_bottom
    }

    /// Tags in iPhone portrait: All, every tag (in the sidebar's order), Untagged — a sideways-scrolling row of chips
    /// that filters the list like the sidebar does.
    fn tag_chips(&mut self, ui: &mut egui::Ui, r: Rect) {
        let c = th();
        // None: ☰, the list page
        let mut chips: Vec<(String, Option<Filter>)> = vec![("☰".into(), None), ("All".into(), Some(Filter::All))];
        if self.view == View::Projects {
            let mut ps: Vec<(Id, String)> = self.store.projects.iter().filter(|p| matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).map(|p| (p.id, p.name.clone())).collect();
            self.store.by_recent_use(&mut ps, |t| t.project.into_iter().collect());
            chips.extend(ps.into_iter().map(|(id, n)| (n, Some(Filter::Project(id)))));
        } else {
            chips.extend(self.store.tag_tree().into_iter().filter_map(|(g, _)| self.store.tag(g).map(|t| (t.name.clone(), Some(Filter::Tag(Some(t.id)))))));
            chips.push(("Untagged".into(), Some(Filter::Tag(None))));
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
        child.set_clip_rect(r);
        egui::ScrollArea::horizontal().id_salt("tag-chips").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (label, f) in chips {
                    let on = f == Some(self.filter);
                    let g = ui.painter().layout_no_wrap(label, FontId::proportional(14.0), if on { c.on_accent } else { c.text });
                    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 26.0, 32.0), Sense::click());
                    ui.painter().rect_filled(rect, 16.0, if on { hue("violet") } else { mix(c.bg, c.text, 0.10) });
                    ui.painter().galley(rect.center() - g.size() / 2.0, g, c.text);
                    if on && ui.data(|d| d.get_temp::<bool>(egui::Id::new("tag-chips-seen"))) != Some(true) {
                        ui.data_mut(|d| d.insert_temp(egui::Id::new("tag-chips-seen"), true));
                        ui.scroll_to_rect(rect, None);
                    }
                    if resp.clicked() {
                        match f {
                            Some(f) => self.filter = f,
                            None => self.list_page = true,
                        }
                    }
                }
            });
        });
    }

    fn outline(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        let rows = self.rows(d, now);
        let portrait = phone_portrait(ui.ctx());
        let in_list = !(self.view == View::Nearby || self.view == View::Stats || self.board_on() || rows.is_empty());
        // iPhone portrait: the large title scrolls away with the list, to show small in the top bar
        let header_in_list = portrait && in_list;
        let header_bottom = if header_in_list {
            full.top()
        } else {
            self.list_scroll = 0.0;
            self.bar_collapsed = false;
            self.outline_header(ui, full, &rows, d, now, phone_landscape(ui.ctx()))
        };

        let mut body = Rect::from_min_max(pos2(full.left(), header_bottom + 1.0), full.max);
        if self.view == View::Nearby {
            let mh = (body.height() * 0.55).clamp(220.0, 520.0);
            let mrect = Rect::from_min_size(body.min, vec2(body.width(), mh));
            let pins: Vec<places::Pin> = self
                .store
                .tags
                .iter()
                .filter_map(|g| {
                    let count = self.store.tasks.iter().filter(|t| t.tags.contains(&g.id) && d.st(t.id).available()).count();
                    g.place.clone().map(|place| places::Pin { place, color: hue("green"), count })
                })
                .collect();
            let here = self.store.settings.here.clone();
            let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(mrect));
            if let Some((lat, lon)) = places::map(&mut mui, &mut self.map, mrect.size(), &pins, here.as_ref()) {
                // a pin for the selected tag (its inspector's Change Location › Pin brings you here)
                if let Some(Sel::Tag(gid)) = self.sel {
                    if let Some(name) = self.store.tags.iter().find(|g| g.id == gid).map(|g| g.name.clone()) {
                        self.geo_msg.clear();
                        self.place_tag_at(gid, name, lat, lon);
                    }
                }
            }
            body.min.y = mrect.max.y + 1.0;
        }
        if self.view == View::Stats {
            self.stats_view(ui, body, now);
            return;
        }
        if self.board_on() {
            self.board(ui, d, now, body);
            return;
        }
        if rows.is_empty() {
            draw_empty(&p, body, self.view, self.pcolor(self.view));
            return;
        }
        let mut acts = vec![];
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        let area = egui::ScrollArea::vertical().id_salt(("outline", self.view.key())).auto_shrink([false, false]);
        // `--scroll` (screenshots) scrolls the list too
        let area = if self.scroll != 0.0 { area.vertical_scroll_offset(self.scroll) } else { area };
        let out = area.show(&mut child, |ui| {
            if header_in_list {
                let w = ui.available_width();
                let h = match self.view {
                    View::Forecast => 92.0,
                    View::Tags | View::Projects => 88.0, // the chips (portrait)
                    _ => 60.0,
                };
                let (hr, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                self.outline_header(ui, Rect::from_min_size(hr.min, vec2(w, full.height())), &rows, d, now, false);
            }
            ui.add_space(10.0);
            // An action can appear under several headers (one per tag); salt each row with its header so the copies
            // get their own widget IDs.
            let mut header: Option<&String> = None;
            for row in &rows {
                match row {
                    Row::Header(h) => {
                        header = Some(h);
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                        let collapsed = self.collapsed_headers.contains(h);
                        chevron(ui.painter(), pos2(rect.left() + 32.0, rect.center().y), !collapsed, th().dim);
                        ui.painter().text(pos2(rect.left() + 55.0, rect.center().y), Align2::LEFT_CENTER, h, bold(14.5), th().text);
                        if resp.clicked() {
                            acts.push(Act::CollapseHeader(h.clone()));
                        }
                    }
                    Row::Task { id, depth, show_project } => {
                        ui.push_id((header, *id), |ui| self.task_row(ui, d, now, *id, *depth, *show_project, &mut acts));
                    }
                    Row::Project(pid) => self.project_row(ui, d, now, *pid, &mut acts),
                    Row::Event(i, day) => {
                        if let Some(e) = self.cal.get(*i) {
                            event_row(ui, e, *day);
                        }
                    }
                    Row::Timeline(day) => {
                        let spans = self.day_spans(d, *day);
                        day_timeline(ui, &spans, (*day == now.date()).then(|| now.time()), self.store.settings.forecast_vertical, now);
                    }
                }
            }
            // (iPhone: room to scroll the last rows above the floating bar or buttons)
            ui.add_space(if portrait {
                TAB_BAR_H + 20.0
            } else if phone_landscape(ui.ctx()) {
                76.0
            } else {
                40.0
            });
            // click empty space to deselect
            let rest = ui.allocate_response(vec2(ui.available_width(), ui.available_height().max(0.0)), Sense::click());
            if rest.clicked() {
                acts.push(Act::Filter(self.filter));
                self.sel = None;
            }
        });
        if portrait {
            let offset = out.state.offset.y;
            self.bar_collapsed = bar_collapsed(self.bar_collapsed, self.list_scroll, offset);
            self.list_scroll = offset;
        }
        self.apply(acts);
    }

    fn subtitle(&self, rows: &[Row]) -> String {
        let (mut inbox, mut actions, mut projects) = (0, 0, 0);
        for r in rows {
            match r {
                Row::Task { id, .. } => {
                    if self.store.task(*id).is_some_and(|t| t.project.is_none()) { inbox += 1 } else { actions += 1 }
                }
                Row::Project(_) => projects += 1,
                _ => {}
            }
        }
        let pl = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
        match self.view {
            View::Inbox => if inbox == 0 { "no items".into() } else { pl(inbox, "inbox item") },
            View::Stats => "time from your clock-ins and Pomodoros".into(),
            View::Agents if !self.agentui.available() => "turn on Agents for your sync server in Settings › Sync".into(),
            View::Review => if projects == 0 { "no projects to review".into() } else { pl(projects, "project") + " to review" },
            _ => {
                let mut parts = vec![];
                if inbox > 0 {
                    parts.push(pl(inbox, "inbox item"));
                }
                if actions > 0 {
                    parts.push(pl(actions, "action"));
                }
                if projects > 0 {
                    parts.push(pl(projects, "project"));
                }
                if parts.is_empty() { "no items".into() } else { parts.join(", ") }
            }
        }
    }

    fn forecast_strip(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt, rect: Rect) {
        let today = now.date();
        let n = 10;
        let w = rect.width() / n as f32;
        let cutoff = self.store.due_soon_cutoff(now);
        for i in 0..n {
            let cell = Rect::from_min_size(rect.min + vec2(i as f32 * w, 0.0), vec2(w, rect.height()));
            let (label, f, items): (String, Filter, Vec<&Task>) = match i {
                0 => ("Past".into(), Filter::Past, self.store.tasks.iter().filter(|t| t.due.is_some_and(|x| x.date() < today)).collect()),
                9 => {
                    let last = today + Duration::days(7);
                    ("Future".into(), Filter::Future, self.store.tasks.iter().filter(|t| t.due.or(t.planned).is_some_and(|x| x.date() > last)).collect())
                }
                _ => {
                    let day = today + Duration::days(i as i64 - 1);
                    let l = if i == 1 { "Today".into() } else if w < 62.0 { day.format("%-d").to_string() } else { day.format("%a %-d").to_string() };
                    (l, Filter::Day(day), self.store.tasks.iter().filter(|t| self.forecast_includes(t, day)).collect())
                }
            };
            let items: Vec<&Task> = items.into_iter().filter(|t| d.st(t.id).remaining()).collect();
            let on = self.filter == f;
            let resp = ui.interact(cell, ui.id().with(("strip", i)), Sense::click());
            if on {
                ui.painter().rect_filled(cell.shrink(2.0), 6.0, th().rail_sel);
            }
            let lc = if i == 1 || on { th().text } else { th().dim };
            ui.painter().text(cell.center_top() + vec2(0.0, 10.0), Align2::CENTER_CENTER, label, if i == 1 { bold(12.0) } else { FontId::proportional(12.0) }, lc);
            ui.painter().text(cell.center_top() + vec2(0.0, 28.0), Align2::CENTER_CENTER, items.len().to_string(), FontId::proportional(15.0), if items.is_empty() { th().faint } else { th().text });
            // manual: red dots overdue, yellow due soon, grey other due items; none on Future
            if i != 9 {
                let dots: Vec<Color32> = items
                    .iter()
                    .filter_map(|t| t.due)
                    .map(|x| if x < now { th().overdue } else if x <= cutoff { th().due_soon } else { th().dim })
                    .take(6)
                    .collect();
                let x0 = cell.center().x - (dots.len() as f32 - 1.0) * 2.5;
                for (k, c) in dots.iter().enumerate() {
                    ui.painter().circle_filled(pos2(x0 + k as f32 * 5.0, cell.top() + 40.0), 1.8, *c);
                }
            }
            if resp.clicked() {
                self.filter = if on { Filter::All } else { f };
            }
        }
    }

    fn task_row(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt, id: Id, depth: usize, show_project: bool, acts: &mut Vec<Act>) {
        let Some(t) = self.store.task(id).cloned() else { return };
        let st = d.st(id);
        let today = now.date();
        let selected = self.sel == Some(Sel::Task(id));
        let picked = !selected && self.picked.contains(&id);
        let has_kids = d.children.get(&Some(id)).is_some_and(|c| !c.is_empty());
        let proj_name = t.project.and_then(|p| self.store.project(p)).map(|p| p.name.clone());
        let tag_names: Vec<String> = t.tags.iter().filter_map(|g| self.store.tag(*g)).map(|g| g.name.clone()).collect();
        // effective due (inherited dates show in italics)
        let eff_due = d.eff_due.get(&id).copied();
        let inherited_due = eff_due.is_some() && eff_due != t.due;
        let date: Option<(String, Color32)> = eff_due.map(|x| {
            let c = match st {
                Status::Overdue => th().overdue,
                Status::DueSoon => th().due_soon,
                _ => th().dim,
            };
            (short_date(x, today), c)
        });
        let (own_flag, inh_flag) = self.store.effective_flagged(id);
        let flagged = own_flag || inh_flag;
        let issue = t.linear.as_ref().and_then(|k| self.store.linear.get(k)).cloned();
        let defer_future = t.defer.filter(|x| *x > now);
        // Forecast: time until the planned time, then the block's time left (planned → planned + duration)
        let block = if self.view == View::Forecast { t.planned.and_then(|pl| block_countdown(pl, t.duration_min, now)) } else { None };
        if block.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        let agent_badge = self.agentui.latest(id).is_some_and(|th| th.status.active() || self.view == View::Agents);
        let has_meta = agent_badge || block.is_some() || (show_project && proj_name.is_some()) || !tag_names.is_empty() || date.is_some() || defer_future.is_some() || t.repeat.is_some() || issue.is_some();
        let note_lines: Vec<&str> = if selected { t.note.lines().filter(|l| !l.trim().is_empty()).take(3).collect() } else { vec![] };
        // the note being typed in the row: as many lines as it wraps to, so the row grows with it (the person: "notes
        // field can exceed its container")
        let note_w = (ui.available_width() - (52.0 + depth as f32 * INDENT + 18.0) - 30.0).max(60.0);
        let inline_note = self.note_inline.as_ref().filter(|n| selected && n.0 == id).map(|n| {
            let g = ui.fonts_mut(|f| f.layout(format!("{} ", n.1), FontId::proportional(13.0), th().text, note_w));
            ((g.size().y / 17.0).ceil() as usize).clamp(1, 40)
        });
        let mut h = ROW_H + if has_meta && !selected { META_H } else { 0.0 };
        if selected {
            // field strip + note lines ("Add Note" counts as one line) + bottom padding: everything stays inside the selection
            h += META_H + 2.0 + inline_note.unwrap_or(note_lines.len().max(1)) as f32 * 17.0 + 10.0;
        }
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        let p = ui.painter().clone();
        let p = &p;
        let x0 = rect.left() + 52.0 + depth as f32 * INDENT;
        let cy = rect.top() + ROW_H / 2.0;
        if selected || picked {
            p.rect_filled(Rect::from_min_max(rect.min + vec2(11.0, 1.0), rect.max - vec2(11.0, 1.0)), 13.0, th().sel_row);
        }
        if has_kids {
            let cc = pos2(x0 - 41.0, cy);
            chevron(p, cc, !self.collapsed.contains(&id), th().dim);
            if ui.interact(Rect::from_center_size(cc, vec2(20.0, 24.0)), ui.id().with(("chev", id)), Sense::click()).clicked() {
                acts.push(Act::Collapse(id));
            }
        }
        // status circle (manual: yellow due soon, red overdue, orange flagged, dots = repeat, dash = dropped)
        let cc = pos2(x0, cy);
        let ring = board::row_ring(st, flagged);
        let done = ui.ctx().animate_bool_with_time(ui.id().with(("done", id)), matches!(st, Status::Completed | Status::Dropped), 0.22);
        status_circle_anim(p, cc, ring, st, t.repeat.is_some(), flagged, done);
        let circle = ui.interact(Rect::from_center_size(cc, vec2(24.0, 24.0)), ui.id().with(("circle", id)), Sense::click());
        if circle.clicked() {
            if ui.input(|i| i.modifiers.alt) { acts.push(Act::ToggleDrop(id)) } else { acts.push(Act::ToggleDone(id)) }
        }
        // title
        let name_x = x0 + 18.0;
        let right_pad = 74.0;
        let name_rect = Rect::from_min_max(pos2(name_x, cy - 10.0), pos2(rect.right() - right_pad, cy + 10.0));
        let color = board::title_color(st, has_kids);
        if self.editing == Some(id) {
            let te = crate::nativetext::Edit::singleline(&mut self.edit_buf).frame(false).hint_text("Untitled Item").font(FontId::proportional(14.5)).text_color(th().text).desired_width(name_rect.width());
            // a child ui, so the title editor doesn't move the outline's layout cursor (rows below stay put)
            let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(name_rect));
            let r = field_ui.add(te);
            if self.edit_focus {
                r.request_focus();
                ui.scroll_to_rect(rect, Some(egui::Align::Center)); // a new row may be below the fold
                self.edit_focus = false;
            }
            if r.lost_focus() {
                self.commit_edit();
            }
        } else {
            let mut job = egui::text::LayoutJob::default();
            let font = if has_kids { bold(14.5) } else { FontId::proportional(14.5) };
            let mut fmt = TextFormat::simple(font, color);
            if matches!(st, Status::Completed | Status::Dropped) {
                fmt.strikethrough = Stroke::new(1.0, th().dim);
            }
            let name = if t.name.is_empty() { "Untitled Item" } else { &t.name };
            job.append(name, 0.0, fmt);
            job.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width());
            let g = p.layout_job(job);
            p.galley(pos2(name_x, cy - g.size().y / 2.0), g, color);
        }
        // right icons: note, flag
        let fx = rect.right() - 36.0;
        draw_flag(p, pos2(fx, cy), if own_flag { 2 } else if inh_flag { 1 } else { 0 });
        if ui.interact(Rect::from_center_size(pos2(fx, cy), vec2(22.0, 22.0)), ui.id().with(("flag", id)), Sense::click()).clicked() {
            acts.push(Act::ToggleFlag(id));
        }
        if !t.note.is_empty() {
            let nc = pos2(fx - 30.0, cy);
            let nr = Rect::from_center_size(nc, vec2(14.0, 12.0));
            p.rect_filled(nr, 2.0, if selected { th().text } else { th().dim });
            for k in 0..3 {
                let y = nr.top() + 3.5 + k as f32 * 2.6;
                p.line_segment([pos2(nr.left() + 3.0, y), pos2(nr.right() - 3.0, y)], Stroke::new(1.0, if selected { th().sel_row } else { th().bg }));
            }
        }
        // meta line
        let my = cy + 20.0;
        if selected {
            // field strip on the selected row: each field opens its editor when clicked
            let mut x = name_x;
            let right = rect.right() - 30.0;
            let labels: [(fields::Field, String, bool); 7] = [
                (fields::Field::Project, proj_name.clone().unwrap_or_else(|| "Project".into()), proj_name.is_some()),
                (fields::Field::Tags, if tag_names.is_empty() { "Tags".into() } else { tag_names.join(", ") }, !tag_names.is_empty()),
                (fields::Field::Duration, t.duration_min.map(|m| format!("{m} min")).unwrap_or_else(|| "Duration".into()), t.duration_min.is_some()),
                (fields::Field::Defer, t.defer.map(|x| format!("Defer {}", short_date(x, today))).unwrap_or_else(|| "Defer".into()), t.defer.is_some()),
                (fields::Field::Planned, t.planned.map(|x| format!("Planned {}", short_date(x, today))).unwrap_or_else(|| "Planned".into()), t.planned.is_some()),
                (fields::Field::Due, t.due.map(|x| format!("Due {}", short_date(x, today))).unwrap_or_else(|| "Due".into()), t.due.is_some()),
                (fields::Field::Repeat, t.repeat.map(|r| r.label()).unwrap_or_else(|| "Repeat".into()), t.repeat.is_some()),
            ];
            for (f, label, set) in labels {
                if x > right - 30.0 {
                    break;
                }
                let open = self.field_open == Some((id, f));
                let c = if set || open { mix(th().text, th().sel_row, 0.12) } else { mix(th().dim, th().sel_row, 0.25) };
                let mut job = egui::text::LayoutJob::simple_singleline(label, FontId::proportional(12.5), c);
                job.wrap = egui::text::TextWrapping::truncate_at_width(right - x);
                let g = p.layout_job(job);
                let r = Rect::from_min_size(pos2(x - 4.0, my - 10.0), vec2(g.size().x + 8.0, 20.0));
                let resp = ui.interact(r, ui.id().with(("strip", id, f)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.hovered() || open {
                    p.rect_filled(r, 5.0, mix(th().sel_row, th().text, 0.12));
                }
                let wdt = g.size().x;
                p.galley(pos2(x, my - g.size().y / 2.0), g, c);
                self.strip_rects.insert((id, f), r);
                if resp.clicked() {
                    acts.push(Act::OpenField(id, f));
                }
                x += wdt + 16.0;
            }
            let mut ny = my + 19.0;
            let note_c = mix(th().text, th().sel_row, 0.15);
            // the note area (or "Add Note") opens the note editor
            let note_area = Rect::from_min_max(pos2(name_x - 4.0, ny - 9.0), pos2(rect.right() - 30.0, rect.bottom() - 4.0));
            if inline_note.is_some() {
                // iPhone: the note edits in place; it saves when the field loses focus
                let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(note_area));
                let Some((_, buf, focus)) = self.note_inline.as_mut() else { return };
                let r = field_ui.add(crate::nativetext::Edit::multiline(buf).frame(false).hint_text("Add Note").font(FontId::proportional(13.0)).text_color(note_c).desired_width(note_area.width()).desired_rows(1));
                if *focus {
                    r.request_focus();
                    r.scroll_to_me(Some(egui::Align::Center));
                    *focus = false;
                } else if !r.has_focus() {
                    let (_, text, _) = self.note_inline.take().unwrap_or_default();
                    if let Some(t) = self.store.task_mut(id).filter(|t| t.note != text) {
                        t.note = text;
                        t.modified = now;
                        self.touch();
                    }
                }
                return;
            }
            if ui.interact(note_area, ui.id().with(("note", id)), Sense::click()).on_hover_cursor(egui::CursorIcon::Text).clicked() {
                acts.push(Act::EditNote(id));
            }
            if t.note.is_empty() {
                p.text(pos2(name_x, ny), Align2::LEFT_CENTER, "Add Note", FontId::proportional(13.0), mix(th().dim, th().sel_row, 0.25));
            }
            for l in note_lines {
                let mut job = egui::text::LayoutJob::simple_singleline(notes::plain(l), FontId::proportional(13.0), note_c);
                job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - 30.0 - name_x);
                let g = p.layout_job(job);
                p.galley(pos2(name_x, ny - g.size().y / 2.0), g, note_c);
                ny += 17.0;
            }
        } else if has_meta {
            let mut x = name_x;
            if let (true, Some(pn)) = (show_project, proj_name) {
                project_glyph(p, pos2(x + 5.0, my), 0.55, th().dim);
                let g = p.layout_no_wrap(pn, FontId::proportional(12.5), mix(th().text, th().dim, 0.45));
                let wdt = g.size().x;
                p.galley(pos2(x + 12.0, my - g.size().y / 2.0), g, th().dim);
                x += wdt + 22.0;
            }
            for tn in &tag_names {
                let g = p.layout_no_wrap(tn.clone(), FontId::proportional(12.0), th().tag_text);
                let r = Rect::from_min_size(pos2(x, my - 9.0), vec2(g.size().x + 16.0, 18.0));
                p.rect_filled(r, 6.0, th().tag_pill);
                p.galley(pos2(x + 8.0, my - g.size().y / 2.0), g, th().text);
                x = r.right() + 6.0;
            }
            if let Some(df) = defer_future {
                let s = format!("Defer {}", short_date(df, today));
                let g = p.layout_no_wrap(s, FontId::proportional(12.5), th().dim);
                let wdt = g.size().x;
                p.galley(pos2(x + 4.0, my - g.size().y / 2.0), g, th().dim);
                x += wdt + 14.0;
            }
            if let Some((s, c)) = date {
                // due soon / overdue dates sit in a tinted pill (reference: the person's Forecast picture); inherited = italic
                let urgent = matches!(st, Status::DueSoon | Status::Overdue);
                let mut job = egui::text::LayoutJob::default();
                let mut f = TextFormat::simple(FontId::proportional(12.5), c);
                f.italics = inherited_due;
                job.append(&s, 0.0, f);
                let g = p.layout_job(job);
                let pill = Rect::from_min_size(pos2(x + 2.0, my - 9.0), vec2(g.size().x + 26.0, 18.0));
                if urgent {
                    p.rect_filled(pill, 4.0, mix(th().bg, c, 0.22));
                }
                cal_glyph(p, pos2(pill.left() + 10.0, my), c);
                let wdt = g.size().x;
                p.galley(pos2(pill.left() + 18.0, my - g.size().y / 2.0), g, c);
                x += wdt + 34.0;
            }
            if agent_badge {
                x = self.agent_row_badge(ui, rect, x, my, id, self.view == View::Agents);
            }
            if let Some((s, phase)) = &block {
                // a ticking pill, coloured by phase: dim ahead, green as it's about to start, accent in it, orange
                // as it ends
                let c = phase.color();
                let g = p.layout_no_wrap(s.clone(), FontId::monospace(12.0), c);
                let pill = Rect::from_min_size(pos2(x + 2.0, my - 9.0), vec2(g.size().x + 26.0, 18.0));
                p.rect_filled(pill, 9.0, mix(th().bg, c, if *phase == BlockPhase::Ahead { 0.10 } else { 0.22 }));
                let k = pos2(pill.left() + 10.0, my);
                p.circle_stroke(k, 5.0, Stroke::new(1.2, c));
                p.line_segment([k, k - vec2(0.0, 3.0)], Stroke::new(1.2, c));
                p.line_segment([k, k + vec2(2.5, 0.0)], Stroke::new(1.2, c));
                p.galley(pos2(pill.left() + 18.0, my - g.size().y / 2.0), g, c);
                x = pill.right() + 8.0;
            }
            let clocked = self.store.clocked_minutes(id, true, now);
            let running = self.store.active_clock().is_some_and(|c| c.0 == id);
            if clocked > 0 || running {
                let c = if running { th().accent } else { th().dim };
                p.circle_stroke(pos2(x + 9.0, my), 5.0, Stroke::new(1.2, c));
                p.line_segment([pos2(x + 9.0, my), pos2(x + 9.0, my - 3.0)], Stroke::new(1.2, c));
                p.line_segment([pos2(x + 9.0, my), pos2(x + 11.5, my)], Stroke::new(1.2, c));
                let g = p.layout_no_wrap(stats::hm(clocked), FontId::proportional(12.0), c);
                let wdt = g.size().x;
                p.galley(pos2(x + 17.0, my - g.size().y / 2.0), g, c);
                x += wdt + 26.0;
            }
            if let Some(i) = &issue {
                let sc = linear::color(&i.state.color);
                editor::state_glyph(p, pos2(x + 8.0, my), &i.state.kind, sc);
                let g = p.layout_no_wrap(format!("{} {}", i.identifier, i.state.name), FontId::proportional(12.0), mix(th().text, th().dim, 0.4));
                let wdt = g.size().x;
                p.galley(pos2(x + 17.0, my - g.size().y / 2.0), g, th().dim);
                x += wdt + 26.0;
            }
            if t.repeat.is_some() {
                p.text(pos2(x + 4.0, my), Align2::LEFT_CENTER, "⟳", FontId::proportional(12.5), th().dim);
            }
        }
        if resp.double_clicked() {
            acts.push(Act::Edit(id));
        } else if resp.clicked() {
            let m = ui.input(|i| i.modifiers);
            // Control-click opens the shortcut menu on Mac, like a right-click
            if !m.ctrl {
                acts.push(if m.command || self.select_mode { Act::PickToggle(id) } else if m.shift { Act::PickRange(id) } else { Act::Select(Sel::Task(id)) });
            }
        }
        let ctrl_click = resp.clicked() && ui.input(|i| i.modifiers.ctrl && !i.modifiers.command);
        task_menu(&resp, id, &t, has_kids, ctrl_click, self.agentui.available(), acts);
    }

    fn project_row(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt, pid: Id, acts: &mut Vec<Act>) {
        let Some(pr) = self.store.project(pid) else { return };
        let selected = self.sel == Some(Sel::Project(pid));
        let (rem, avail) = self.store.tasks.iter().filter(|t| t.project == Some(pid)).fold((0, 0), |(r, a), t| {
            let s = d.st(t.id);
            (r + s.remaining() as usize, a + s.available() as usize)
        });
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H + META_H + 2.0), Sense::click());
        let p = ui.painter();
        if selected {
            p.rect_filled(Rect::from_min_max(rect.min + vec2(11.0, 1.0), rect.max - vec2(11.0, 1.0)), 13.0, th().sel_row);
        }
        let cy = rect.top() + ROW_H / 2.0;
        let x0 = rect.left() + 52.0;
        if self.view == View::Projects {
            let cc = pos2(x0 - 41.0 + 8.0, cy);
            chevron(p, cc, !self.collapsed.contains(&pid), th().dim);
            if ui.interact(Rect::from_center_size(cc, vec2(20.0, 24.0)), ui.id().with(("pchev", pid)), Sense::click()).clicked() {
                acts.push(Act::Collapse(pid));
            }
        }
        let st = match pr.status {
            ProjectStatus::Completed => Status::Completed,
            ProjectStatus::Dropped => Status::Dropped,
            ProjectStatus::OnHold => Status::Blocked,
            _ => Status::Available,
        };
        status_circle(p, pos2(x0, cy), if pr.flagged { th().flag } else { mix(th().text, th().dim, 0.6) }, st, false, pr.flagged);
        if ui.interact(Rect::from_center_size(pos2(x0, cy), vec2(24.0, 24.0)), ui.id().with(("pcircle", pid)), Sense::click()).clicked() {
            let next = if pr.status == ProjectStatus::Completed { ProjectStatus::Active } else { ProjectStatus::Completed };
            acts.push(Act::ProjectStatus(pid, next));
        }
        let c = if pr.status == ProjectStatus::OnHold { th().dim } else { th().text };
        p.text(pos2(x0 + 18.0, cy), Align2::LEFT_CENTER, &pr.name, bold(14.5), c);
        let kind = match pr.kind {
            ProjectKind::Parallel => "Parallel",
            ProjectKind::Sequential => "Sequential",
            ProjectKind::SingleActions => "Single Actions",
        };
        let mut meta = format!("{kind} · {rem} remaining");
        if pr.status == ProjectStatus::Active && avail == 0 && rem > 0 || pr.status == ProjectStatus::Active && rem == 0 {
            meta.push_str(" · Stalled");
        }
        if pr.status == ProjectStatus::OnHold {
            meta.push_str(" · On Hold");
        }
        if self.view == View::Review {
            let ago = (now.date() - pr.next_review).num_days();
            meta.push_str(&format!(" · review every {} days · due {}", pr.review_every_days, if ago == 0 { "today".into() } else { format!("{ago} days ago") }));
        }
        p.text(pos2(x0 + 18.0, cy + 20.0), Align2::LEFT_CENTER, meta, FontId::proportional(12.5), th().dim);
        draw_flag(p, pos2(rect.right() - 36.0, cy), if pr.flagged { 2 } else { 0 });
        if resp.clicked() {
            acts.push(Act::Select(Sel::Project(pid)));
        }
        // shortcut menu (manual, Shortcut Menus: projects)
        resp.context_menu(|ui| {
            let s = Sel::Project(pid);
            let mut item = |ui: &mut egui::Ui, label: &str, act: Act| {
                if ui.button(label).clicked() {
                    acts.push(act);
                    ui.close();
                }
            };
            item(ui, "Add Inside", Act::AddToProject(pid));
            item(ui, if pr.flagged { "Unflag" } else { "Flag" }, Act::Menu(s, keys::Action::ToggleFlag));
            item(ui, "Mark Reviewed", Act::Menu(s, keys::Action::MarkReviewed));
            item(ui, "Attach File\u{2026}", Act::Attach(s, false));
            item(ui, "Attach Folder\u{2026}", Act::Attach(s, true));
            ui.separator();
            item(ui, "Complete", Act::ProjectStatus(pid, ProjectStatus::Completed));
            item(ui, "Drop", Act::ProjectStatus(pid, ProjectStatus::Dropped));
            item(ui, "On Hold", Act::ProjectStatus(pid, ProjectStatus::OnHold));
            item(ui, "Active", Act::ProjectStatus(pid, ProjectStatus::Active));
            ui.separator();
            item(ui, "Inspect", Act::Select(s));
            item(ui, "Delete", Act::Menu(s, keys::Action::Delete));
        });
    }

    // ---------- inspector ----------
    fn inspector(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt) {
        let Some(sel) = self.sel else {
            let r = ui.max_rect();
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "No Items Selected", FontId::proportional(14.5), th().dim);
            return;
        };
        egui::ScrollArea::vertical().id_salt("insp").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width()); // the side panel sets the Mac width; on iPhone the page is the screen
            if !self.picked.is_empty() && matches!(sel, Sel::Task(_)) {
                let n = self.selected_tasks().len();
                ui.label(egui::RichText::new(format!("{n} actions selected. Status, flag, delete and the row's field editors change all of them; this shows the first.")).color(th().dim).size(12.0));
                ui.add_space(6.0);
            }
            if self.arrange_sections && !matches!(sel, Sel::Tag(_)) {
                self.arrange_panel(ui, if matches!(sel, Sel::Task(_)) { &TASK_SECTIONS } else { &PROJECT_SECTIONS });
                return;
            }
            match sel {
                Sel::Task(id) => self.task_inspector(ui, d, now, id),
                Sel::Project(id) => self.project_inspector(ui, now, id),
                Sel::Tag(id) => self.tag_inspector(ui, id),
            }
            // room to scroll the last field clear of the on-screen keyboard (iPhone/iPad); 0 when it's hidden
            ui.add_space(keyboard::height());
        });
    }

    /// Rearrange mode: the inspector's sections as a list you drag into order.
    fn arrange_panel(&mut self, ui: &mut egui::Ui, keys: &[&str]) {
        ui.horizontal(|ui| {
            caption(ui, "Rearrange Sections");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    self.arrange_sections = false;
                }
            });
        });
        ui.label(egui::RichText::new("Drag a section by its grip. The order applies to every action (or project).").color(th().dim).size(12.0));
        ui.add_space(8.0);
        let order: Vec<String> = self.store.settings.inspector_order().into_iter().filter(|k| keys.contains(&k.as_str())).collect();
        let w = ui.available_width();
        let moved = drag_list(ui, egui::Id::new("arrange-sections"), order.len(), |ui, i| {
            let k = order[i].as_str();
            let (r, _) = ui.allocate_exact_size(vec2(w - 30.0, 34.0), Sense::hover());
            ui.painter().rect_filled(r, 7.0, th().field);
            let mut x = r.left() + 12.0;
            if k == "linear" {
                linear::logo(ui.painter(), pos2(x + 7.0, r.center().y), 7.0);
                x += 22.0;
            }
            let label = INSPECTOR_SECTIONS.iter().find(|s| s.0 == k).map(|s| s.1).unwrap_or(k);
            ui.painter().text(pos2(x, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), th().text);
        });
        if let Some((from, to)) = moved {
            self.store.settings.place_section(&order[from], to, keys);
            self.touch();
        }
        ui.add_space(10.0);
        if ui.small_button("Restore Default Order").clicked() {
            self.store.settings.inspector_order.clear();
            self.touch();
        }
    }

    fn task_inspector(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt, id: Id) {
        let mut projects: Vec<(Id, String)> = self.store.projects.iter().filter(|p| matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).map(|p| (p.id, p.name.clone())).collect();
        let mut tags: Vec<(Id, String)> = self.store.tags.iter().map(|g| (g.id, g.name.clone())).collect();
        // the pickers list the most recently used first
        self.store.by_recent_use(&mut projects, |t| t.project.into_iter().collect());
        self.store.by_recent_use(&mut tags, |t| t.tags.clone());
        let st = self.store.settings.clone();
        let status = d.status.get(&id).copied();
        let note_cards: std::collections::BTreeMap<String, LinkCard> = self
            .store
            .task(id)
            .map(|t| t.note.lines().filter_map(|l| self.store.link_cards.get_key_value(l.trim())).map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let w = ui.available_width();
        let mut changed = false;
        let mut move_to: Option<Option<Id>> = None;
        let mut resolve: Option<u8> = None;
        let has_kids = d.children.get(&Some(id)).is_some_and(|c| !c.is_empty());
        let focus = self.focus_field.take();
        let reminders = self.upcoming_reminders(d, id, now);
        let linear_cur = self.store.task(id).and_then(|t| t.linear.clone());
        let status_name = match status {
            Some(Status::Blocked) => "Blocked",
            Some(Status::Available) => "Available",
            Some(Status::Next) => "Next",
            Some(Status::Completed) => "Completed",
            Some(Status::DueSoon) => "Due Soon",
            Some(Status::Overdue) => "Overdue",
            Some(Status::Dropped) => "Dropped",
            None => "",
        };
        let jump = |ui: &mut egui::Ui, want: keys::Action| {
            if focus == Some(want) {
                ui.scroll_to_cursor(Some(egui::Align::Center));
            }
        };
        {
            let Some(t) = self.store.task_mut(id) else { return };
            // title
            let r = ui.add(crate::nativetext::Edit::multiline(&mut t.name).desired_rows(2).desired_width(w).hint_text("Untitled Item").font(FontId::proportional(14.5)).margin(egui::Margin::same(10)));
            changed |= r.changed();
        }
        section(ui);
        // Sections in the person's order (Settings › Appearance, or the ⋯ menu on each section).
        let mut op: Option<(String, SecMove)> = None;
        let order: Vec<String> = self.store.settings.inspector_order().into_iter().filter(|k| TASK_SECTIONS.contains(&k.as_str())).collect();
        for key in order {
            match key.as_str() {
                "status" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Status").font(bold(14.0)).color(th().text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            sec_menu(ui, "status", &mut op);
                            ui.label(egui::RichText::new(status_name).color(th().dim));
                        });
                    });
                    let cur = if t.completed.is_some() { 1 } else if t.dropped.is_some() { 2 } else { 0 };
                    ui.horizontal(|ui| {
                        if let Some(i) = segmented(ui, &["▶", "✔", "–"], w - 44.0, Some(cur), true) {
                            if i != cur {
                                resolve = Some(i as u8);
                            }
                        }
                        let (fr, fresp) = ui.allocate_exact_size(vec2(36.0, 28.0), Sense::click());
                        ui.painter().rect_filled(fr, 6.0, if t.flagged { th().flag.gamma_multiply(0.35) } else { th().pill });
                        draw_flag(ui.painter(), fr.center(), if t.flagged { 2 } else { 0 });
                        if fresp.clicked() {
                            t.flagged = !t.flagged;
                            changed = true;
                        }
                    });
                    if has_kids {
                        section(ui);
                        // "Group Type" + "Complete with last action" for action groups
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Group Type").font(bold(14.0)).color(th().text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(if t.sequential { "Sequential" } else { "Parallel" }).color(th().dim));
                            });
                        });
                        if let Some(i) = segmented(ui, &["Parallel", "Sequential"], w, Some(t.sequential as usize), true) {
                            t.sequential = i == 1;
                            changed = true;
                        }
                        changed |= ui.checkbox(&mut t.complete_with_last, "Complete with last action").changed();
                    }
                }
                "project" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    jump(ui, keys::Action::EditProject);
                    sec_caption(ui, "Project", "project", None, &mut op);
                    let cur_name = t.project.and_then(|p| projects.iter().find(|x| x.0 == p)).map(|x| x.1.clone()).unwrap_or_else(|| "None".into());
                    egui::ComboBox::from_id_salt(("proj", id)).selected_text(cur_name).width(w - 8.0).show_ui(ui, |ui| {
                        if ui.selectable_label(t.project.is_none(), "None (Inbox)").clicked() {
                            move_to = Some(None);
                        }
                        for (pid, pn) in &projects {
                            if ui.selectable_label(t.project == Some(*pid), pn).clicked() {
                                move_to = Some(Some(*pid));
                            }
                        }
                    });
                }
                "tags" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    jump(ui, keys::Action::EditTags);
                    sec_caption(ui, "Tags", "tags", None, &mut op);
                    egui::Frame::new().fill(th().field).corner_radius(6).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.set_width(w - 16.0);
                        ui.horizontal_wrapped(|ui| {
                            let mut remove = None;
                            for g in &t.tags {
                                if let Some((_, n)) = tags.iter().find(|x| x.0 == *g) {
                                    if ui.add(egui::Button::new(egui::RichText::new(format!("{n}  ×")).size(12.5)).fill(th().tag_pill).corner_radius(6)).on_hover_text("Remove tag").clicked() {
                                        remove = Some(*g);
                                    }
                                }
                            }
                            if let Some(g) = remove {
                                t.tags.retain(|x| *x != g);
                                changed = true;
                            }
                            ui.menu_button(egui::RichText::new("+").size(15.0), |ui| {
                                for (gid, gn) in &tags {
                                    let mut on = t.tags.contains(gid);
                                    if ui.checkbox(&mut on, gn).changed() {
                                        if on { t.tags.push(*gid) } else { t.tags.retain(|x| x != gid) }
                                        changed = true;
                                    }
                                }
                            });
                        });
                    });
                }
                "duration" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    jump(ui, keys::Action::EditDuration);
                    sec_caption(ui, "Duration", "duration", None, &mut op);
                    let dtext = t.duration_min.map(|m| if m >= 60 && m % 60 == 0 { format!("{} hour{}", m / 60, if m == 60 { "" } else { "s" }) } else { format!("{m} minutes") }).unwrap_or_else(|| "None".into());
                    field_text(ui, &dtext, w, false);
                    if let Some(i) = segmented(ui, &["+1m", "+5m", "+15m", "+1h", "×"], w, None, false) {
                        t.duration_min = match i {
                            4 => None,
                            _ => Some(t.duration_min.unwrap_or(0) + [1, 5, 15, 60][i]),
                        };
                        changed = true;
                    }
                }
                "dates" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    for (k, label, default_time) in [(0usize, "Defer", st.default_defer_time), (1, "Planned", st.default_planned_time), (2, "Due", st.default_due_time)] {
                        if k == 0 {
                            sec_caption(ui, label, "dates", None, &mut op);
                        } else {
                            section(ui);
                            caption(ui, label);
                        }
                        let field = match k {
                            0 => &mut t.defer,
                            1 => &mut t.planned,
                            _ => &mut t.due,
                        };
                        let buf = &mut self.date_buf[k];
                        let dirty = &mut self.date_dirty[k];
                        let formatted = field.map(|x| x.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
                        let want = [keys::Action::EditDefer, keys::Action::EditPlanned, keys::Action::EditDue][k];
                        if cfg!(target_os = "ios") {
                            // iOS's own date picker in a sheet
                            let shown = field.map(long_date).unwrap_or_else(|| "None".into());
                            let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
                            ui.painter().rect_filled(r, 6.0, th().field);
                            ui.painter().text(r.left_center() + vec2(9.0, 0.0), Align2::LEFT_CENTER, shown, FontId::proportional(14.0), if field.is_some() { th().text } else { th().dim });
                            #[cfg(target_os = "ios")]
                            if resp.clicked() || focus == Some(want) {
                                datepick::present(&format!("{label} Date"), k, id, *field, default_time);
                            }
                            let _ = resp;
                        } else {
                            let r = ui.add(crate::nativetext::Edit::singleline(&mut buf.1).hint_text("None").desired_width(w).margin(egui::Margin::symmetric(8, 5)));
                            if focus == Some(want) {
                                r.request_focus();
                                r.scroll_to_me(Some(egui::Align::Center));
                            }
                            if r.changed() {
                                *dirty = true;
                            }
                            if r.has_focus() && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.contains('\n')))) {
                                r.surrender_focus(); // a soft keyboard's Return can arrive as a newline
                            }
                            if !r.has_focus() {
                                if *dirty && buf.0 == Some(id) {
                                    // focus left (Return, a click or tap elsewhere): keep the entry when it reads as a date
                                    if let Ok(v) = mcp::parse_date(&buf.1.replace('\n', " "), default_time) {
                                        if v != *field {
                                            *field = v;
                                            changed = true;
                                        }
                                    }
                                }
                                *dirty = false;
                                let formatted = field.map(|x| x.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
                                *buf = (Some(id), formatted);
                            }
                        }
                        let _ = formatted;
                        if let Some(i) = segmented(ui, &["Today", "+1d", "+1w", "+1m", "×"], w, None, false) {
                            *field = datepick::quick(i, *field, now, default_time);
                            buf.0 = None;
                            changed = true;
                        }
                    }
                }
                "notifications" => {
                    // lists the automatic ones, e.g. "Due: Tomorrow, 5:00 PM"
                    sec_caption(ui, "Notifications", "notifications", None, &mut op);
                    if reminders.is_empty() {
                        ui.label(egui::RichText::new("None").color(th().dim));
                    }
                    for (kind, when) in &reminders {
                        egui::Frame::new().fill(th().field).corner_radius(6).inner_margin(egui::Margin::symmetric(9, 5)).show(ui, |ui| {
                            ui.set_width(w - 18.0);
                            ui.label(egui::RichText::new(format!("{kind}:  {when}")).color(th().text));
                        });
                    }
                }
                "completed" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    jump(ui, keys::Action::EditCompleted);
                    sec_caption(ui, "Completed", "completed", None, &mut op);
                    field_text(ui, &t.completed.map(long_date).unwrap_or_else(|| "None".into()), w, true);
                    section(ui);
                    caption(ui, "Dropped");
                    field_text(ui, &t.dropped.map(long_date).unwrap_or_else(|| "None".into()), w, true);
                }
                "repeat" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    jump(ui, keys::Action::EditRepeat);
                    sec_caption(ui, "Repeat", "repeat", None, &mut op);
                    match t.repeat.as_mut() {
                        None => {
                            if ui.add_sized([w, 26.0], egui::Button::new("Add Repetition")).clicked() {
                                t.repeat = Some(Repeat::default());
                                changed = true;
                            }
                        }
                        Some(r) => {
                            ui.horizontal(|ui| {
                                ui.label("Every");
                                changed |= ui.add(egui::DragValue::new(&mut r.every).range(1..=999)).changed();
                                egui::ComboBox::from_id_salt(("unit", id)).selected_text(format!("{}s", r.unit_name())).width(90.0).show_ui(ui, |ui| {
                                    for u in [RepeatUnit::Minute, RepeatUnit::Hour, RepeatUnit::Day, RepeatUnit::Week, RepeatUnit::Month, RepeatUnit::Year] {
                                        let label = format!("{}s", Repeat { unit: u, ..*r }.unit_name());
                                        changed |= ui.selectable_value(&mut r.unit, u, label).changed();
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                changed |= ui.selectable_value(&mut r.schedule, RepeatSchedule::Regularly, "Regularly").changed();
                                changed |= ui.selectable_value(&mut r.schedule, RepeatSchedule::FromCompletion, "From Completion").changed();
                            });
                            if r.schedule == RepeatSchedule::Regularly {
                                changed |= ui.checkbox(&mut r.catch_up, "Catch up automatically").changed();
                            }
                            ui.horizontal(|ui| {
                                ui.label("Based on");
                                changed |= ui.selectable_value(&mut r.based_on, BasedOn::Defer, "Defer").changed();
                                changed |= ui.selectable_value(&mut r.based_on, BasedOn::Planned, "Planned").changed();
                                changed |= ui.selectable_value(&mut r.based_on, BasedOn::Due, "Due").changed();
                            });
                            ui.horizontal(|ui| {
                                let mut ends = r.end_after.is_some();
                                if ui.checkbox(&mut ends, "End after").changed() {
                                    r.end_after = if ends { Some(5) } else { None };
                                    changed = true;
                                }
                                if let Some(n) = r.end_after.as_mut() {
                                    changed |= ui.add(egui::DragValue::new(n).range(1..=999)).changed();
                                    ui.label("times");
                                }
                            });
                            ui.label(egui::RichText::new(r.label()).color(th().dim).size(12.5));
                            if ui.add_sized([w, 26.0], egui::Button::new("Remove Repetition")).clicked() {
                                t.repeat = None;
                                changed = true;
                            }
                        }
                    }
                }
                "note" => {
                    let Some(t) = self.store.task_mut(id) else { return };
                    // Rendered Markdown with image embeds and link cards; click Edit (or double-click) to change it.
                    jump(ui, keys::Action::EditNote);
                    ui.horizontal(|ui| {
                        caption(ui, "Note");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            sec_menu(ui, "note", &mut op);
                            let editing = self.note_edit == Some(id);
                            if ui.small_button(if editing { "Done" } else { "Edit" }).clicked() {
                                self.note_edit = if editing { None } else { Some(id) };
                            }
                        });
                    });
                    if self.note_edit == Some(id) {
                        let r = ui.add(
                            crate::nativetext::Edit::multiline(&mut t.note)
                                .hint_text("Markdown: **bold**, *italic*, - [ ] lists, # headings. Paste a link on its own line for a card.")
                                .desired_rows(6)
                                .desired_width(w)
                                .margin(egui::Margin::same(8)),
                        );
                        changed |= r.changed();
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            self.note_edit = None;
                        }
                        self.notes.show_cards(ui, &t.note, &note_cards);
                    } else if t.note.trim().is_empty() {
                        let r = ui.add(egui::Label::new(egui::RichText::new("Add a note…").color(th().faint)).sense(Sense::click()));
                        if r.clicked() {
                            self.note_edit = Some(id);
                        }
                    } else {
                        let resp = egui::Frame::new()
                            .fill(th().field)
                            .corner_radius(6)
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_width(w - 16.0);
                                self.notes.show(ui, id, &t.note, &note_cards);
                            })
                            .response
                            .interact(Sense::click());
                        if resp.double_clicked() {
                            self.note_edit = Some(id);
                        }
                    }
                }
                "time" => self.time_section(ui, id, now, &mut op),
                "linear" => {
                    if let Some(v) = self.linear_section(ui, linear_cur.clone(), &mut op) {
                        if let Some(t) = self.store.task_mut(id) {
                            t.linear = v;
                        }
                        changed = true;
                    }
                }
                "attachments" => {
                    let cur = self.store.task(id).map(|t| t.attachments.clone()).unwrap_or_default();
                    if let Some(v) = self.attachments_section(ui, cur, &mut op, attachpick::Target::Task(id)) {
                        if let Some(t) = self.store.task_mut(id) {
                            t.attachments = v;
                            t.modified = now;
                        }
                        changed = true;
                    }
                }
                "refs" => {
                    let cur = self.store.task(id).map(|t| t.refs.clone()).unwrap_or_default();
                    if let Some(v) = self.refs_section(ui, cur, &mut op) {
                        if let Some(t) = self.store.task_mut(id) {
                            t.refs = v;
                            t.modified = now;
                        }
                        changed = true;
                    }
                }
                _ => continue,
            }
            section(ui);
        }
        if let Some(t) = self.store.task_mut(id) {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Added:").color(th().dim).size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(long_date(t.added)).color(th().dim).size(12.0)));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Changed:").color(th().dim).size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(long_date(t.modified)).color(th().dim).size(12.0)));
            });
            if changed {
                t.modified = now;
            }
        }
        if let Some((k, m)) = op {
            if m == SecMove::Arrange {
                self.arrange_sections = true;
            } else {
                self.store.settings.move_section(&k, m, &TASK_SECTIONS);
                changed = true;
            }
        }
        if let Some(p) = move_to {
            self.store.set_project(id, p);
            changed = true;
        }
        if let Some(i) = resolve {
            self.store.reactivate(id, now);
            match i {
                1 => self.store.complete(id, now),
                2 => self.store.drop_task(id, now),
                _ => {}
            }
            self.mark_resolved(id);
            changed = true;
        }
        if changed {
            self.touch();
        }
    }

    fn project_inspector(&mut self, ui: &mut egui::Ui, now: Dt, id: Id) {
        let w = ui.available_width();
        let mut changed = false;
        let mut new_status = None;
        let mut reviewed = false;
        let linear_cur = self.store.project(id).and_then(|p| p.linear.clone());
        {
            let Some(p) = self.store.project_mut(id) else { return };
            changed |= ui.add(crate::nativetext::Edit::multiline(&mut p.name).desired_rows(1).desired_width(w).font(bold(15.0)).margin(egui::Margin::same(10))).changed();
        }
        section(ui);
        let mut op: Option<(String, SecMove)> = None;
        let order: Vec<String> = self.store.settings.inspector_order().into_iter().filter(|k| PROJECT_SECTIONS.contains(&k.as_str())).collect();
        for key in order {
            if key == "linear" {
                if let Some(v) = self.linear_section(ui, linear_cur.clone(), &mut op) {
                    if let Some(p) = self.store.project_mut(id) {
                        p.linear = v;
                    }
                    changed = true;
                }
                section(ui);
                continue;
            }
            if key == "attachments" {
                let cur = self.store.project(id).map(|p| p.attachments.clone()).unwrap_or_default();
                if let Some(v) = self.attachments_section(ui, cur, &mut op, attachpick::Target::Project(id)) {
                    if let Some(p) = self.store.project_mut(id) {
                        p.attachments = v;
                        p.modified = now;
                    }
                    changed = true;
                }
                section(ui);
                continue;
            }
            if key == "refs" {
                let cur = self.store.project(id).map(|p| p.refs.clone()).unwrap_or_default();
                if let Some(v) = self.refs_section(ui, cur, &mut op) {
                    if let Some(p) = self.store.project_mut(id) {
                        p.refs = v;
                        p.modified = now;
                    }
                    changed = true;
                }
                section(ui);
                continue;
            }
            let Some(p) = self.store.project_mut(id) else { return };
            match key.as_str() {
                "status" => {
                    sec_caption(ui, "Status", "status", None, &mut op);
                    let cur = match p.status {
                        ProjectStatus::Active => 0,
                        ProjectStatus::OnHold => 1,
                        ProjectStatus::Completed => 2,
                        ProjectStatus::Dropped => 3,
                    };
                    if let Some(i) = segmented(ui, &["Active", "On Hold", "Done", "Dropped"], w, Some(cur), true) {
                        new_status = Some([ProjectStatus::Active, ProjectStatus::OnHold, ProjectStatus::Completed, ProjectStatus::Dropped][i]);
                    }
                    changed |= ui.checkbox(&mut p.flagged, "Flagged").changed();
                }
                "type" => {
                    sec_caption(ui, "Type", "type", None, &mut op);
                    let cur = match p.kind {
                        ProjectKind::Parallel => 0,
                        ProjectKind::Sequential => 1,
                        ProjectKind::SingleActions => 2,
                    };
                    if let Some(i) = segmented(ui, &["Parallel", "Sequential", "Single Actions"], w, Some(cur), true) {
                        p.kind = [ProjectKind::Parallel, ProjectKind::Sequential, ProjectKind::SingleActions][i];
                        changed = true;
                    }
                }
                "folder" => {
                    sec_caption(ui, "Folder", "folder", None, &mut op);
                    changed |= ui.add(crate::nativetext::Edit::singleline(&mut p.folder).hint_text("None").desired_width(w).margin(egui::Margin::symmetric(8, 5))).changed();
                }
                "review" => {
                    sec_caption(ui, "Review", "review", None, &mut op);
                    ui.horizontal(|ui| {
                        ui.label("Review every");
                        changed |= ui.add(egui::DragValue::new(&mut p.review_every_days).range(1..=3650)).changed();
                        ui.label("days");
                    });
                    ui.label(egui::RichText::new(format!("Next review: {}", p.next_review.format("%a, %b %-d, %Y"))).color(th().dim));
                    ui.label(egui::RichText::new(format!("Last reviewed: {}", p.last_review.map(long_date).unwrap_or_else(|| "Never".into()))).color(th().dim));
                    if ui.add_sized([w, 26.0], egui::Button::new("Mark Reviewed")).clicked() {
                        reviewed = true;
                    }
                }
                "note" => {
                    sec_caption(ui, "Note", "note", None, &mut op);
                    changed |= ui.add(crate::nativetext::Edit::multiline(&mut p.note).hint_text("Note").desired_rows(5).desired_width(w).margin(egui::Margin::same(8))).changed();
                }
                _ => continue,
            }
            section(ui);
        }
        if changed {
            if let Some(p) = self.store.project_mut(id) {
                p.modified = now;
            }
        }
        if let Some(s) = new_status {
            self.store.set_project_status(id, s, now);
            changed = true;
        }
        if let Some((k, m)) = op {
            if m == SecMove::Arrange {
                self.arrange_sections = true;
            } else {
                self.store.settings.move_section(&k, m, &PROJECT_SECTIONS);
                changed = true;
            }
        }
        if reviewed {
            self.store.mark_reviewed(id, now);
            changed = true;
        }
        if changed {
            self.touch();
        }
    }

    fn tag_inspector(&mut self, ui: &mut egui::Ui, id: Id) {
        let w = ui.available_width();
        let mut changed = false;
        let Some(g) = self.store.tags.iter_mut().find(|g| g.id == id) else { return };
        changed |= ui.add(crate::nativetext::Edit::singleline(&mut g.name).desired_width(w).font(bold(15.0)).margin(egui::Margin::same(10))).changed();
        section(ui);
        caption(ui, "Status");
        if let Some(i) = segmented(ui, &["Active", "On Hold"], w, Some(g.on_hold as usize), true) {
            g.on_hold = i == 1;
            changed = true;
        }
        ui.label(egui::RichText::new("Actions with an on-hold tag are blocked.").color(th().dim).size(12.0));
        section(ui);
        self.tag_location(ui, id, w);
        section(ui);
        if ui.add_sized([w, 26.0], egui::Button::new("Delete Tag")).clicked() {
            self.store.delete_tag(id);
            self.sel = None;
            self.filter = Filter::All;
            self.touch();
        } else if changed {
            self.touch_tag(id);
        }
    }

    /// A tag's Location, like the iPhone inspector: a card with the address and a red ✕ to clear
    /// it, "Change Location" (Here, Anywhere, Search, Address, Pin), then Notification and Distance.
    fn tag_location(&mut self, ui: &mut egui::Ui, id: Id, w: f32) {
        let Some((name, place)) = self.store.tags.iter().find(|g| g.id == id).map(|g| (g.name.clone(), g.place.clone())) else { return };
        let mut set: Option<Option<Place>> = None;
        caption(ui, "Location");
        ui.add_space(4.0);
        let red = hue("red");
        // a shade lighter than the fields, so the search field inside it shows
        egui::Frame::new().fill(mix(th().field, th().text, 0.05)).corner_radius(12).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            let iw = ui.available_width();
            ui.set_width(iw);
            if let Some(p) = &place {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(iw - 34.0);
                        // the place's own name (a business) above its address; not the tag's name or the street again
                        if !p.label.is_empty() && p.label != name && p.address.lines().next() != Some(p.label.as_str()) {
                            ui.label(egui::RichText::new(&p.label).font(bold(15.0)).color(th().text));
                        }
                        let addr = if p.address.is_empty() { format!("{:.5}, {:.5}", p.lat, p.lon) } else { p.address.clone() };
                        ui.label(egui::RichText::new(addr).size(15.0).color(th().text));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                        let b = Rect::from_center_size(r.center(), vec2(18.0, 18.0));
                        let s = Stroke::new(if resp.hovered() { 2.0 } else { 1.5 }, red);
                        ui.painter().rect_stroke(b, 2.0, s, StrokeKind::Middle);
                        ui.painter().line_segment([b.min + vec2(5.0, 5.0), b.max - vec2(5.0, 5.0)], s);
                        ui.painter().line_segment([b.left_bottom() + vec2(5.0, -5.0), b.right_top() + vec2(-5.0, 5.0)], s);
                        if resp.on_hover_text("Clear Location").clicked() {
                            set = Some(None);
                        }
                    });
                });
                ui.add_space(6.0);
                let (r, _) = ui.allocate_exact_size(vec2(iw, 1.0), Sense::hover());
                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, th().line));
            }
            // "Change Location" (or "Add Location"), the location arrow before it, opens the menu
            let (r, resp) = ui.allocate_exact_size(vec2(iw, 34.0), Sense::click());
            let col = if resp.hovered() { mix(th().accent, th().text, 0.25) } else { th().accent };
            let k = r.left_center() + vec2(9.0, 0.0);
            ui.painter().add(Shape::closed_line(vec![k + vec2(7.0, -7.0), k + vec2(-7.0, -1.0), k + vec2(-1.0, 1.0), k + vec2(1.0, 7.0)], Stroke::new(1.5, col)));
            let label = if place.is_some() { "Change Location" } else { "Add Location" };
            ui.painter().text(r.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(15.0), col);
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(170.0);
                let item = |ui: &mut egui::Ui, l: &str| {
                    let c = ui.add(egui::Button::new(egui::RichText::new(l).size(15.0)).min_size(vec2(160.0, 30.0))).clicked();
                    if c {
                        ui.close();
                    }
                    c
                };
                if item(ui, "Here") {
                    #[cfg(any(target_os = "macos", target_os = "ios"))]
                    let gps = places::cl::current();
                    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
                    let gps: Option<Place> = None;
                    match gps.or(self.store.settings.here.clone()) {
                        Some(h) => set = Some(Some(Place::relocated(place.as_ref(), name.clone(), String::new(), h.lat, h.lon))),
                        None => self.geo_msg = "Your location isn't known yet (Location Services).".into(),
                    }
                }
                if item(ui, "Anywhere") {
                    set = Some(None);
                }
                for (l, addr) in [("Search", false), ("Address", true)] {
                    if item(ui, l) {
                        self.place_find = Some((id, addr));
                        self.place_results = None;
                        self.place_query.clear();
                        self.geo_msg.clear();
                    }
                }
                if item(ui, "Pin") {
                    // drop it on the Nearby map: select the tag there and click (or tap) the spot
                    self.go(View::Nearby);
                    self.sel = Some(Sel::Tag(id));
                    self.show_inspector = !phone_portrait(ui.ctx());
                    if let Some(p) = &place {
                        (self.map.lat, self.map.lon) = (p.lat, p.lon);
                    }
                    self.geo_msg = format!("Click the map where \u{201c}{name}\u{201d} is.");
                }
            });
            if let Some((_, addr)) = self.place_find.filter(|f| f.0 == id) {
                ui.add_space(4.0);
                let hint = if addr { "Street, city" } else { "Search for a place or business" };
                let r = ui.add(crate::nativetext::Edit::singleline(&mut self.place_query).hint_text(hint).desired_width(iw));
                if !r.has_focus() && self.place_query.is_empty() && self.place_results.is_none() {
                    r.request_focus();
                }
                let mut go = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    go |= ui.button(if addr { "Find Address" } else { "Search" }).clicked();
                    if ui.button("Cancel").clicked() {
                        self.place_find = None;
                        self.place_results = None;
                        self.geo_msg.clear();
                    }
                });
                if go {
                    let q = self.place_query.clone();
                    self.geocode(q, GeoFor::Tag(id), addr);
                }
                let results = self.place_results.as_ref().filter(|r| r.0 == id).map(|r| r.1.clone()).unwrap_or_default();
                for f in results {
                    let row = egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 5)).show(ui, |ui| {
                        ui.set_width(iw - 8.0);
                        if f.distinct_name() {
                            ui.label(egui::RichText::new(&f.name).font(bold(14.0)).color(th().text));
                        }
                        ui.label(egui::RichText::new(f.address.replace('\n', ", ")).size(12.5).color(th().dim));
                    });
                    let resp = row.response.interact(Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(row.response.rect, 6.0, th().text.gamma_multiply(0.06));
                    }
                    if resp.clicked() {
                        set = Some(Some(f.place(place.as_ref())));
                        self.place_find = None;
                        self.place_results = None;
                        self.geo_msg.clear();
                    }
                }
            }
        });
        if !self.geo_msg.is_empty() {
            ui.label(egui::RichText::new(&self.geo_msg).color(th().dim).size(12.0));
        }
        let mut changed = false;
        if let Some(p) = &place {
            ui.add_space(8.0);
            let notify = [Notify::None, Notify::Arriving, Notify::Leaving];
            let cur = notify.iter().position(|n| *n == p.notify).unwrap_or(1);
            caption_value(ui, "Notification", ["None", "When Arriving", "When Leaving"][cur]);
            if let Some(i) = segmented(ui, &["notify-none", "notify-arrive", "notify-leave"], w, Some(cur), true) {
                let mut p = p.clone();
                p.notify = notify[i];
                set = set.or(Some(Some(p)));
                changed = true;
            }
            if !cfg!(target_os = "ios") && p.notify != Notify::None {
                ui.label(egui::RichText::new("Location notifications arrive on iPhone and iPad.").color(th().dim).size(12.0));
            }
            ui.add_space(8.0);
            let tier = p.tier();
            caption_value(ui, "Distance", radius_label(tier, places::imperial()));
            if let Some(i) = segmented(ui, &["dist0", "dist1", "dist2"], w, Some(tier), true) {
                let mut p = p.clone();
                p.radius_m = Some(PLACE_RADII[i]);
                set = set.or(Some(Some(p)));
                changed = true;
            }
        }
        if let Some(new) = set {
            // a new spot without an address (Here) gets one looked up; picked results already have theirs
            let lookup = new.as_ref().filter(|p| p.address.is_empty() && !changed).map(|p| (p.label.clone(), p.lat, p.lon));
            match lookup {
                Some((label, lat, lon)) => self.place_tag_at(id, label, lat, lon),
                None => {
                    if let Some(g) = self.store.tags.iter_mut().find(|g| g.id == id) {
                        g.place = new;
                    }
                    self.touch_tag(id);
                }
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if cfg!(target_os = "ios") {
            return self.settings_page(ctx);
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Settings").open(&mut open).resizable(true).default_size([560.0, 520.0]).collapsible(false).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (t, i) in SETTINGS_PAGES.iter().chain([("Agent", 5u8)].iter()) {
                    ui.selectable_value(&mut self.settings_tab, *i, *t);
                }
            });
            ui.separator();
            // `--scroll` also scrolls Settings, for screenshots of what's below the fold
            let area = egui::ScrollArea::vertical().auto_shrink([false, true]);
            let area = if self.scroll != 0.0 { area.vertical_scroll_offset(self.scroll) } else { area };
            area.show(ui, |ui| changed |= self.settings_page_body(ui));
        });
        if changed {
            self.touch();
        }
        self.show_settings = open;
    }

    /// The store as loaded, synced or reopened: its layout and connections count as adopted, not as changes made here.
    fn adopt_prints(&mut self) {
        self.layout_print = self.store.layout_fingerprint();
        self.connections_print = self.store.connections_fingerprint();
        self.queue_print = self.store.queue_fingerprint();
    }

    /// Help › Report a Bug: GitHub's bug form for GTDCraft's repo, with this copy's version, device and screen filled in.
    pub(crate) fn report_bug(&mut self) {
        let screen = if self.show_settings { "Settings".to_string() } else { self.pname(self.view) };
        refs::open(&refs::bug_report_url(REPO_URL, &refs::environment(&screen)));
    }

    /// The folder of the selected project or action (where a new folder goes after).
    fn selected_folder(&self) -> Option<String> {
        let pid = match self.sel {
            Some(Sel::Project(p)) => Some(p),
            Some(Sel::Task(t)) => self.store.task(t).and_then(|t| t.project),
            _ => None,
        };
        pid.and_then(|p| self.store.project(p)).map(|p| p.folder.clone()).filter(|f| !f.is_empty())
    }

    /// iPhone/iPad Settings: a full page over the app, not a floating window. The home page
    /// is a large title with grouped rows (›); a row opens its page, with ‹ back; the blue ✓ closes Settings.
    fn settings_page(&mut self, ctx: &egui::Context) {
        // (+ Connections: the Linear, GitHub and AI keys — the Mac has them on its Agent page, which iPhone doesn't show)
        const PAGES: [(&str, u8); 10] = {
            let mut p = [("Connections", 10u8); 10];
            let mut i = 0;
            while i < 9 {
                p[i] = SETTINGS_PAGES[i];
                i += 1;
            }
            p
        };
        let rect = ctx.content_rect();
        let c = th();
        let mut changed = false;
        let mut close = false;
        // the page covers the whole screen, under the status bar and home indicator too
        let mut under = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("settings-page")));
        under.set_clip_rect(ctx.viewport_rect());
        under.rect_filled(ctx.viewport_rect(), 0.0, c.bg);
        egui::Area::new(egui::Id::new("settings-page")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_width(rect.width());
            let home = self.settings_tab == SETTINGS_HOME;
            // header: ‹ back (on a page) · title · ✓ done
            let (bar, _) = ui.allocate_exact_size(vec2(rect.width(), 52.0), Sense::hover());
            if !home {
                let b = egui::Rect::from_center_size(pos2(bar.left() + 34.0, bar.center().y), vec2(40.0, 40.0));
                let r = ui.interact(b, ui.id().with("back"), Sense::click());
                ui.painter().circle_filled(b.center(), 20.0, mix(c.bg, c.text, if r.hovered() { 0.16 } else { 0.10 }));
                let k = b.center();
                ui.painter().line_segment([k + vec2(3.0, -7.0), k + vec2(-4.0, 0.0)], Stroke::new(2.2, c.text));
                ui.painter().line_segment([k + vec2(-4.0, 0.0), k + vec2(3.0, 7.0)], Stroke::new(2.2, c.text));
                if r.clicked() {
                    self.settings_tab = SETTINGS_HOME;
                }
                let title = PAGES.iter().find(|p| p.1 == self.settings_tab).map_or("Settings", |p| p.0);
                ui.painter().text(bar.center(), Align2::CENTER_CENTER, title, bold(17.0), c.text);
            }
            let d = egui::Rect::from_center_size(pos2(bar.right() - 34.0, bar.center().y), vec2(40.0, 40.0));
            let r = ui.interact(d, ui.id().with("done"), Sense::click());
            ui.painter().circle_filled(d.center(), 20.0, if r.hovered() { mix(c.accent, c.text, 0.12) } else { c.accent });
            let k = d.center();
            ui.painter().line_segment([k + vec2(-7.0, 0.5), k + vec2(-2.0, 6.0)], Stroke::new(2.4, c.on_accent));
            ui.painter().line_segment([k + vec2(-2.0, 6.0), k + vec2(8.0, -6.5)], Stroke::new(2.4, c.on_accent));
            if r.clicked() {
                close = true;
            }
            let body = egui::Rect::from_min_max(pos2(rect.left() + 16.0, bar.bottom()), pos2(rect.right() - 16.0, rect.bottom().min(keyboard_top(ctx))));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            let area = egui::ScrollArea::vertical().id_salt(("settings-page", self.settings_tab)).auto_shrink([false, false]);
            let area = if self.scroll != 0.0 { area.vertical_scroll_offset(self.scroll) } else { area };
            area.show(&mut child, |ui| {
                ui.set_width(body.width());
                if home {
                    ui.label(egui::RichText::new("Settings").font(bold(32.0)).color(c.text));
                    ui.add_space(10.0);
                    if let Some(p) = grouped_rows(ui, &PAGES.map(|p| p.0)) {
                        self.settings_tab = PAGES[p].1;
                    }
                    ui.add_space(14.0);
                    // footer under the list
                    let (np, na) = (self.store.projects.len(), self.store.tasks.len());
                    ui.label(egui::RichText::new(format!("{np} projects; {na} actions · GTDCraft {}", env!("CARGO_PKG_VERSION"))).color(c.dim).size(12.0));
                    return;
                }
                changed |= self.settings_page_body(ui);
                ui.add_space(30.0);
            });
        });
        if changed {
            self.touch();
        }
        if close {
            self.show_settings = false;
            self.settings_tab = SETTINGS_HOME;
        }
    }

    /// The current Settings page (`settings_tab`), on the Mac's window or the iPhone's full page.
    fn settings_page_body(&mut self, ui: &mut egui::Ui) -> bool {
        match self.settings_tab {
            0 => self.settings_general(ui),
            1 => self.settings_appearance(ui),
            2 => self.settings_perspectives(ui),
            3 => self.settings_notifications(ui),
            4 => self.settings_keyboard(ui),
            5 if !cfg!(target_os = "ios") => self.settings_agent(ui),
            6 => self.settings_sync(ui),
            7 => self.settings_calendar(ui),
            10 => self.settings_connections(ui),
            8 => self.settings_organization(ui),
            9 => self.settings_dates(ui),
            status::STATUS_TAB => self.settings_status(ui),
            _ => false,
        }
    }

    /// Settings › Dates & Times.
    fn settings_dates(&mut self, ui: &mut egui::Ui) -> bool {
        let st = &mut self.store.settings;
        let mut ch = false;
        ui.heading("Dates & Times");
        ui.horizontal(|ui| {
            ui.label("\u{201c}Due Soon\u{201d} means due within");
            ch |= ui.add(egui::DragValue::new(&mut st.due_soon_days).range(1..=30)).changed();
            ui.label("days");
        });
        let mut time_field = |ui: &mut egui::Ui, label: &str, t: &mut NaiveTime| {
            ui.horizontal(|ui| {
                ui.label(label);
                let mut h = t.format("%H").to_string().parse::<u32>().unwrap_or(0);
                let mut m = t.format("%M").to_string().parse::<u32>().unwrap_or(0);
                let a = ui.add(egui::DragValue::new(&mut h).range(0..=23)).changed();
                ui.label(":");
                let b = ui.add(egui::DragValue::new(&mut m).range(0..=59)).changed();
                if a || b {
                    *t = NaiveTime::from_hms_opt(h, m, 0).unwrap_or(*t);
                    ch = true;
                }
            });
        };
        time_field(ui, "Default defer time", &mut st.default_defer_time);
        time_field(ui, "Default planned time", &mut st.default_planned_time);
        time_field(ui, "Default due time", &mut st.default_due_time);
        ch
    }

    /// Settings › Organization: review and completed items.
    fn settings_organization(&mut self, ui: &mut egui::Ui) -> bool {
        let st = &mut self.store.settings;
        let mut ch = false;
        ui.heading("Review");
        ui.horizontal(|ui| {
            ui.label("New projects are reviewed every");
            ch |= ui.add(egui::DragValue::new(&mut st.review_every_days).range(1..=365)).changed();
            ui.label("days");
        });
        ui.add_space(4.0);
        ch |= ui.checkbox(&mut st.guru_cadence, "Limit the Review Guru to one grooming pass every").changed();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(st.guru_cadence, |ui| {
                ch |= ui.add(egui::DragValue::new(&mut st.guru_every_days).range(1..=365)).changed();
            });
            ui.label("days");
        });
        ui.label(egui::RichText::new("Off: the Guru is always available. Otherwise it greys out between sessions with a zen reminder.").color(th().dim).size(12.0));
        ui.separator();
        ui.heading("Organization");
        ch |= ui.checkbox(&mut st.auto_hide_completed, "Hide completed items automatically").changed();
        ui.label(egui::RichText::new("Otherwise they stay until you use Clean Up or leave the perspective.").color(th().dim).size(12.0));
        ch
    }

    /// Settings › General (ordering 0): sample items, and on the Mac where the database lives.
    fn settings_general(&mut self, ui: &mut egui::Ui) -> bool {
        // (its buttons touch() the store themselves; nothing here is a plain setting)
        let ch = false;
        // a new database starts empty; the sample projects and actions are there to try things on, and come out again
        ui.heading("Sample Items");
        let (t, p, g) = self.store.sample_items();
        let found = t.len() + p.len() + g.len();
        if self.store.tasks.is_empty() && self.store.projects.is_empty() {
            ui.label(egui::RichText::new("Example projects, tags and actions to try GTDCraft with. Remove them here when you're done.").color(th().dim).size(12.0));
            if ui.button("Add Sample Items").clicked() {
                self.store.fill_sample(now());
                self.touch();
            }
        } else if found > 0 {
            ui.label(egui::RichText::new(format!("{} sample actions, {} projects and {} tags are in this database. Removing them also removes them from your other devices at the next sync.", t.len(), p.len(), g.len())).color(th().dim).size(12.0));
            if ui.button("Remove Sample Items").clicked() {
                let n = self.store.remove_sample();
                self.touch();
                self.sync_msg = format!("Removed {n} sample items (Edit › Undo brings them back)");
            }
        } else {
            ui.label(egui::RichText::new("No sample items in this database.").color(th().dim).size(12.0));
        }
        if !cfg!(target_os = "ios") {
            ui.separator();
            ui.label(egui::RichText::new(format!("Database: {}", self.path.display())).color(th().dim).size(12.0));
        }
        ch
    }

    fn settings_appearance(&mut self, ui: &mut egui::Ui) -> bool {
        let mut ch = false;
        let names: Vec<(String, bool)> = self.themes.iter().map(|t| (t.name.clone(), t.dark)).collect();
        let st = &mut self.store.settings;
        ui.heading("Theme");
        ch |= ui.checkbox(&mut st.follow_system, if cfg!(target_os = "ios") { "Match the system's light/dark appearance" } else { "Match macOS light/dark appearance" }).changed();
        let mut pick = |ui: &mut egui::Ui, label: &str, cur: &mut String, filter: Option<bool>| {
            ui.horizontal(|ui| {
                ui.label(label);
                egui::ComboBox::from_id_salt(label).selected_text(cur.clone()).width(230.0).show_ui(ui, |ui| {
                    for (n, dark) in &names {
                        if filter.is_none_or(|f| f == *dark) {
                            ch |= ui.selectable_value(cur, n.clone(), n).changed();
                        }
                    }
                });
            });
        };
        if st.follow_system {
            pick(ui, "Light theme", &mut st.light_theme, Some(false));
            pick(ui, "Dark theme", &mut st.dark_theme, Some(true));
        } else {
            pick(ui, "Theme", &mut st.theme, None);
        }
        ui.add_space(6.0);
        // swatches of the current theme
        let cs = th();
        ui.horizontal_wrapped(|ui| {
            for (n, col) in [("bg", cs.bg), ("text", cs.text), ("accent", cs.accent), ("overdue", cs.overdue), ("due soon", cs.due_soon), ("flag", cs.flag), ("next", cs.next)]
                .into_iter()
                .chain(theme::HUES.iter().map(|h| (*h, hue(h))))
            {
                let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                ui.painter().rect_filled(r, 6.0, col);
                ui.painter().rect_stroke(r, 6.0, Stroke::new(1.0, cs.line), StrokeKind::Inside);
                resp.on_hover_text(n);
            }
        });
        ui.separator();
        ui.heading("Your own themes");
        ui.label("Drop a .json file following theme.schema.json into the themes folder. A theme names a palette and points UI slots at it, like an nvim colorscheme; perspective colours come from its hue slots and are adjusted so they always read on the background.");
        ui.horizontal(|ui| {
            if !cfg!(target_os = "ios") && ui.button("Open Themes Folder").clicked() {
                if let Ok(dir) = theme::seed_user_dir() {
                    let _ = std::process::Command::new("open").arg(dir).spawn();
                }
            }
            if ui.button("Reload Themes").clicked() {
                let (t, e) = theme::load_all();
                self.themes = t;
                self.theme_errors = e;
                self.applied_theme.clear();
            }
        });
        for e in &self.theme_errors {
            ui.label(egui::RichText::new(e).color(th().overdue).size(12.0));
        }
        if ch {
            self.applied_theme.clear();
        }
        ui.separator();
        ui.heading("Inspector Sections");
        ui.label(egui::RichText::new("Drag the grip to reorder (top to bottom). In the inspector, each section's ⋯ menu has Rearrange Sections too.").color(th().dim).size(12.0));
        let order = self.store.settings.inspector_order();
        let all: Vec<&str> = order.iter().map(String::as_str).collect();
        let moved = drag_list(ui, egui::Id::new("settings-sections"), order.len(), |ui, i| {
            let k = order[i].as_str();
            if k == "linear" {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                linear::logo(ui.painter(), r.center(), 7.0);
            }
            ui.label(INSPECTOR_SECTIONS.iter().find(|s| s.0 == k).map(|s| s.1).unwrap_or(k));
        });
        if let Some((from, to)) = moved {
            self.store.settings.place_section(&order[from], to, &all);
            ch = true;
        }
        if ui.button("Restore Default Order").clicked() {
            self.store.settings.inspector_order.clear();
            ch = true;
        }
        ch
    }

    fn settings_notifications(&mut self, ui: &mut egui::Ui) -> bool {
        let st = &mut self.store.settings;
        let mut ch = false;
        ui.heading("Reminders");
        ch |= ui.checkbox(&mut st.notify, "Show date and time reminders").changed();
        ui.add_enabled_ui(st.notify, |ui| {
            ch |= ui.checkbox(&mut st.notify_due, "When an item is due").changed();
            ch |= ui.checkbox(&mut st.notify_defer, "When a deferred item becomes available").changed();
            ch |= ui.checkbox(&mut st.notify_planned, "At an item's planned time").changed();
            ch |= ui.checkbox(&mut st.notify_start, "At the latest time to start (due minus estimated duration)").changed();
            // the person: "configure notification for before planned time/due time so I can wrap up other work"
            ui.horizontal_wrapped(|ui| {
                ui.label("Also remind me before planned and due times:");
                for m in [0u32, 5, 10, 15, 30, 60] {
                    let l = if m == 0 { "Off".to_string() } else { interval_label(m as i64) };
                    ch |= ui.selectable_value(&mut st.notify_before_min, m, l).changed();
                }
            });
        });
        #[cfg(target_os = "ios")]
        {
            // iOS decides whether they show: say so, and offer the way to change it
            if ui.ctx().cumulative_frame_nr() % 60 == 0 {
                places::check_permission();
            }
            match places::permission() {
                Some(true) => {
                    ui.label(egui::RichText::new("Notifications are allowed. Reminders are scheduled with iOS for the next week, so they arrive even when GTDCraft is closed.").color(th().dim).size(12.0));
                }
                Some(false) => {
                    ui.label(egui::RichText::new("iOS isn't letting GTDCraft show notifications. Turn on Allow Notifications in GTDCraft's iOS Settings.").color(th().overdue).size(12.5));
                    if ui.button("Open iOS Settings").clicked() {
                        refs::open("app-settings:");
                    }
                }
                None => {
                    if ui.button("Allow Notifications…").clicked() {
                        places::check_permission();
                        places::notify("GTDCraft reminders", "Notifications are on");
                    }
                }
            }
        }
        #[cfg(not(target_os = "ios"))]
        ui.label(egui::RichText::new("Reminders appear in macOS Notification Center while GTDCraft is open. The first one may ask you to allow notifications.").color(th().dim).size(12.0));
        if ui.button("Send a Test Reminder").clicked() {
            places::notify("Test reminder", "Reminders are working");
        }
        ch
    }

    /// Quick Entry: the card with its title, Project / Tags / Due / Repeat fields and Add
    /// Note; Save adds one action to the chosen project (or the Inbox) and closes the card.
    fn quick_entry(&mut self, ctx: &egui::Context) {
        if self.quick.is_none() {
            return;
        }
        let projects: Vec<(Id, String)> = self.store.projects.iter().filter(|p| p.status == ProjectStatus::Active).map(|p| (p.id, p.name.clone())).collect();
        let tags: Vec<(Id, String)> = self.store.tags.iter().map(|t| (t.id, self.store.tag_path(t.id))).collect();
        let c = th();
        // The Quick Entry card is its own small window, about 713 × 196 pt, floating above other windows
        // (window level 8): it moves anywhere on screen, apart from the main window, is dragged by its background and
        // resized from its edges, and keeps its frame (MECHANICS.md rounds 21 and 22).
        let today = chrono::Local::now().date_naive();
        let min = vec2(ctx.fonts_mut(|f| quick_min_width(|t| f.layout_no_wrap(t.into(), FontId::proportional(12.5), c.text).size().x, today)), 150.0);
        let saved = self.store.settings.quick_size;
        let mut q = self.quick.take().expect("quick entry");
        let (mut save, mut close) = (false, false);
        let card = egui::Frame::new().fill(mix(c.bg, c.text, 0.02)).inner_margin(egui::Margin { left: 20, right: 20, top: 30, bottom: 12 });
        if phone_portrait(ctx) || phone_landscape(ctx) {
            // iPhone: a full-screen sheet with Cancel · eye · Save at the top
            let full = ctx.viewport_rect();
            let safe = ctx.content_rect();
            let mut under = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("quick-sheet")));
            under.set_clip_rect(full);
            under.rect_filled(full, 0.0, c.bg);
            egui::Area::new(egui::Id::new("quick-sheet")).order(egui::Order::Foreground).fixed_pos(safe.min).show(ctx, |ui| {
                ui.set_min_size(safe.size());
                ui.set_max_width(safe.width());
                let (bar, _) = ui.allocate_exact_size(vec2(safe.width(), 56.0), Sense::hover());
                let pill = |ui: &mut egui::Ui, r: egui::Rect, label: &str, id: &str| {
                    let resp = ui.interact(r, ui.id().with(id), Sense::click());
                    ui.painter().rect_filled(r, r.height() / 2.0, mix(c.bg, c.text, if resp.hovered() { 0.18 } else { 0.12 }));
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(16.0), c.text);
                    resp.clicked()
                };
                if pill(ui, egui::Rect::from_min_size(pos2(bar.left() + 14.0, bar.top() + 8.0), vec2(84.0, 40.0)), "Cancel", "qs-cancel") {
                    close = true;
                }
                let eye = egui::Rect::from_center_size(pos2(bar.left() + 128.0, bar.center().y), vec2(40.0, 40.0));
                let er = ui.interact(eye, ui.id().with("qs-eye"), Sense::click());
                ui.painter().circle_filled(eye.center(), 20.0, mix(c.bg, c.text, 0.12));
                eye_glyph(ui.painter(), eye.center(), c.text);
                if er.clicked() {
                    q.show_note = !q.show_note;
                }
                if pill(ui, egui::Rect::from_min_size(pos2(bar.right() - 84.0, bar.top() + 8.0), vec2(70.0, 40.0)), "Save", "qs-save") {
                    save = true;
                }
                let body = egui::Rect::from_min_max(pos2(safe.left() + 4.0, bar.bottom() + 4.0), pos2(safe.right() - 4.0, safe.bottom()));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                quick_card(&mut child, &mut q, &projects, &tags, &mut save, &mut close, true);
            });
            close |= ctx.input(|i| i.key_pressed(Key::Escape));
        } else if ctx.embed_viewports() || cfg!(target_os = "ios") || self.shot.is_some() {
            // one window (screenshots, iPhone/iPad): show the card inside it, centred
            let vp = ctx.viewport_rect();
            let frame = card.stroke(Stroke::new(1.0, mix(c.line, c.text, 0.08))).corner_radius(14);
            let size = vec2(saved[0], saved[1]).min(vp.size() - vec2(24.0, 24.0));
            egui::Window::new("quick-entry")
                .title_bar(false)
                .collapsible(false)
                .resizable(true)
                .current_pos(pos2((vp.width() - size.x) * 0.5, (vp.height() - size.y) * 0.5))
                .default_size((size - frame.total_margin().sum()).max(min))
                .min_size(min)
                .frame(frame)
                .show(ctx, |ui| quick_card(ui, &mut q, &projects, &tags, &mut save, &mut close, false));
            close |= ctx.input(|i| i.key_pressed(Key::Escape));
        } else {
            // its own window: placed where it was left, else centred on the screen; pulled back if that's off screen
            let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(vec2(1440.0, 900.0));
            let size = vec2(saved[0], saved[1]).max(min).min(monitor - vec2(24.0, 24.0));
            let centred = pos2((monitor.x - size.x) * 0.5, (monitor.y - size.y) * 0.5);
            // ponytail: "on screen" is the main monitor only; check every monitor if people place it on a second one
            let at = self.store.settings.quick_pos.map(|p| pos2(p[0], p[1])).filter(|p| egui::Rect::from_min_size(Pos2::ZERO, monitor).contains_rect(egui::Rect::from_min_size(*p, size))).unwrap_or(centred);
            let builder = egui::ViewportBuilder::default()
                .with_title("Quick Entry")
                .with_inner_size(size)
                .with_min_inner_size(min)
                .with_position(at)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_titlebar_shown(false)
                .with_title_shown(false)
                .with_titlebar_buttons_shown(false)
                .with_fullsize_content_view(true)
                .with_resizable(true)
                .with_active(true);
            let mut frame_now = None;
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("gtdcraft-quick-entry"), builder, |ctx, _| {
                egui::CentralPanel::default().frame(card).show(ctx, |ui| {
                    // dragging the card's background moves the window (widgets drawn after this take their own presses)
                    let bg = ui.interact(ui.max_rect().expand(30.0), ui.id().with("quick-bg"), Sense::drag());
                    if bg.drag_started() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    quick_card(ui, &mut q, &projects, &tags, &mut save, &mut close, false);
                });
                close |= ctx.input(|i| i.key_pressed(Key::Escape) || i.viewport().close_requested());
                quick_window_level();
                frame_now = ctx.input(|i| Some((i.viewport().outer_rect?.min, i.viewport().inner_rect?.size())));
            });
            // remember the window's frame (place and size) when the person has moved or resized it
            if let Some((pos, now)) = frame_now {
                let (pos, now) = ([pos.x.round(), pos.y.round()], [now.x.round(), now.y.round()]);
                if self.store.settings.quick_pos != Some(pos) || self.store.settings.quick_size != now {
                    self.store.settings.quick_pos = Some(pos);
                    self.store.settings.quick_size = now;
                    self.dirty_at = Some(f64::MIN_POSITIVE.max(self.last_poll));
                }
            }
        }
        if save && !q.name.trim().is_empty() {
            let id = self.store.add_task(q.name.trim(), q.project, None);
            let due = mcp::parse_date(&q.due, self.store.settings.default_due_time).ok().flatten();
            let (tags, repeat, note, flagged) = (q.tags.clone(), q.repeat.clone(), q.note.clone(), q.flagged);
            if let Some(t) = self.store.task_mut(id) {
                t.due = due;
                t.flagged = flagged;
                t.tags = tags;
                t.repeat = repeat;
                t.note = note;
            }
            self.touch();
            close = true;
        }
        self.quick = if close { None } else { Some(q) };
    }
    }

impl Gtd {
    /// Manual, Forecast: an item shows on a day for its due date, and (view options) planned / defer date.
    /// The day's timeline: calendar events (all-day ones only with that option) and the actions planned that day with
    /// a duration, as blocks.
    fn day_spans(&self, d: &Derived, day: NaiveDate) -> Vec<Span> {
        use chrono::Timelike;
        let hrs = |t: Dt, from_day: NaiveDate| if t.date() < from_day { 0.0 } else if t.date() > from_day { 24.0 } else { t.hour() as f32 + t.minute() as f32 / 60.0 };
        let all_day = self.store.settings.forecast_all_day_blocks;
        let mut out: Vec<Span> = self
            .cal
            .iter()
            .filter(|e| calendar::on_day(e, day) && (all_day || !e.all_day))
            .map(|e| {
                let (from, to) = if e.all_day { (0.0, 24.0) } else { (hrs(e.start, day), hrs(e.end, day)) };
                Span { from, to, title: e.title.clone(), color: e.color.map(|[r, g, b]| Color32::from_rgb(r, g, b)).unwrap_or(th().accent), block: None }
            })
            .collect();
        for t in self.store.tasks.iter().filter(|t| self.visible(d, t)) {
            if let (Some(pl), Some(m)) = (t.planned.filter(|p| p.date() == day), t.duration_min.filter(|m| *m > 0)) {
                let from = hrs(pl, day);
                out.push(Span { from, to: (from + m as f32 / 60.0).min(24.0), title: t.name.clone(), color: th().accent.gamma_multiply(0.85), block: Some((pl, Some(m))) });
            }
        }
        out
    }

    pub(crate) fn forecast_includes(&self, t: &Task, day: NaiveDate) -> bool {
        let st = &self.store.settings;
        t.due.is_some_and(|x| x.date() == day) || (st.forecast_planned && t.planned.is_some_and(|x| x.date() == day)) || (st.forecast_defer && t.defer.is_some_and(|x| x.date() == day))
    }
}


#[derive(Clone, Copy)]
enum QeGlyph {
    Project,
    Tag,
    Due,
    Repeat,
}

/// One field in Quick Entry's field line: a small glyph, then the field's name or value.
/// Quick Entry floats at window level 8, above floating panels (3, winit's "always on top"): set it on the
/// AppKit window.
fn quick_window_level() {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        for w in objc2_app_kit::NSApplication::sharedApplication(mtm).windows().iter() {
            if w.level() != 8 && w.title().to_string() == "Quick Entry" {
                w.setLevel(8);
            }
        }
    }
}

/// The Quick Entry card's contents: the editing block, a field's list when one is open, and the eye /
/// Cancel / Save row on the card's floor.
fn quick_card(ui: &mut egui::Ui, q: &mut QuickEntry, projects: &[(Id, String)], tags: &[(Id, String)], save: &mut bool, close: &mut bool, sheet: bool) {
    let c = th();
    // the iPhone sheet reads at phone size
    let fs = if sheet { 1.15 } else { 1.0 };
    let inner = ui.available_width();
    ui.set_width(inner);
    // the editing block: title, fields and note, inside the card
    egui::Frame::new()
        .fill(mix(c.bg, c.sel_row, 0.08))
        .stroke(Stroke::new(1.2, c.sel_row))
        .corner_radius(9)
        .inner_margin(egui::Margin { left: 28, right: 14, top: 7, bottom: 7 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // the item's title, with the note and flag buttons at the right
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                ui.painter().circle_stroke(r.center(), 7.0, Stroke::new(1.6, mix(c.dim, c.faint, 0.25)));
                ui.add_space(7.0);
                let te = ui.add(
                    crate::nativetext::Edit::singleline(&mut q.name)
                        .frame(false)
                        .hint_text("Untitled Item")
                        .desired_width((ui.available_width() - 62.0).max(60.0))
                        .font(FontId::proportional(15.0 * fs))
                        .margin(egui::Margin::symmetric(0, 2)),
                );
                if q.focus {
                    te.request_focus();
                    q.focus = false;
                }
                if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    *save = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(2.0);
                    let (fr, frp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    if frp.hovered() {
                        ui.painter().rect_filled(fr, 5.0, mix(c.field, c.text, 0.10));
                    }
                    draw_flag(ui.painter(), fr.center(), if q.flagged { 2 } else { 0 });
                    if frp.on_hover_text(if q.flagged { "Unflag" } else { "Flag" }).clicked() {
                        q.flagged = !q.flagged;
                    }
                    ui.add_space(10.0);
                    let (nr, nrp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    let ncol = if q.show_note || !q.note.is_empty() { mix(c.text, c.dim, 0.35) } else { mix(c.dim, c.faint, 0.30) };
                    if nrp.hovered() {
                        ui.painter().rect_filled(nr, 5.0, mix(c.field, c.text, 0.10));
                    }
                    note_glyph(ui.painter(), nr.center(), ncol);
                    if nrp.on_hover_text("Note").clicked() {
                        q.show_note = true;
                    }
                });
            });
            // the field line: Project, Tags, Due, Repeat — each opens its list under the line
            ui.add_space(if sheet { 12.0 } else { 1.0 });
            // iPhone sheet: one field per line; the Mac card: one line of fields
            let gap = if sheet { 18.0 } else { 4.0 };
            let fields = |ui: &mut egui::Ui| {
                if !sheet {
                    ui.add_space(27.0);
                }
                let pname = q.project.and_then(|p| projects.iter().find(|x| x.0 == p)).map(|x| x.1.clone());
                if qe_chip_sized(ui, fs, QeGlyph::Project, pname.as_deref().unwrap_or("Project"), pname.is_some()).clicked() {
                    q.open = if q.open == Some("project") { None } else { Some("project") };
                }
                ui.add_space(gap);
                let tnames: Vec<&str> = q.tags.iter().filter_map(|id| tags.iter().find(|t| t.0 == *id)).map(|t| t.1.as_str()).collect();
                let tlabel = if tnames.is_empty() { "Tags".to_string() } else { tnames.join(", ") };
                if qe_chip_sized(ui, fs, QeGlyph::Tag, &tlabel, !tnames.is_empty()).clicked() {
                    q.open = if q.open == Some("tags") { None } else { Some("tags") };
                }
                ui.add_space(gap);
                let dlabel = if q.due.trim().is_empty() { "Due".to_string() } else { q.due.trim().to_string() };
                if qe_chip_sized(ui, fs, QeGlyph::Due, &dlabel, !q.due.trim().is_empty()).clicked() {
                    q.open = if q.open == Some("due") { None } else { Some("due") };
                }
                ui.add_space(gap);
                let rlabel = q.repeat.as_ref().map(|r| r.label()).unwrap_or_else(|| "Repeat".into());
                if qe_chip_sized(ui, fs, QeGlyph::Repeat, &rlabel, q.repeat.is_some()).clicked() {
                    q.open = if q.open == Some("repeat") { None } else { Some("repeat") };
                }
            };
            // (`horizontal` / `vertical` take only the rows they need; `with_layout` would take all the height)
            if sheet {
                ui.vertical(fields);
            } else {
                ui.horizontal(fields);
            }
            // the note, or the Add Note line
            ui.add_space(3.0);
            if q.show_note {
                ui.add(
                    crate::nativetext::Edit::multiline(&mut q.note)
                        .frame(false)
                        .hint_text("Note")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .font(FontId::proportional(12.5 * fs))
                        .margin(egui::Margin { left: 34, right: 6, top: 2, bottom: 2 }),
                );
            } else {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
                let col = if resp.hovered() { mix(c.dim, c.bg, 0.10) } else { mix(c.faint, c.bg, 0.30) };
                ui.painter().text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, "Add Note", FontId::proportional(12.5 * fs), col);
                if resp.on_hover_cursor(egui::CursorIcon::Text).clicked() {
                    q.show_note = true;
                }
            }
        });
    // a field's list opens under the block; otherwise the block is followed by the empty area the
    // empty area between the fields and the Cancel / Save row
    if let Some(kind) = q.open {
        egui::Frame::new()
            .fill(c.bg)
            .stroke(Stroke::new(1.0, c.line))
            .corner_radius(9)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match kind {
                    "project" => {
                        egui::ScrollArea::vertical().max_height(190.0).show(ui, |ui| {
                            if ui.selectable_label(q.project.is_none(), "Inbox").clicked() {
                                q.project = None;
                                q.open = None;
                            }
                            for (id, n) in projects {
                                if ui.selectable_label(q.project == Some(*id), n).clicked() {
                                    q.project = Some(*id);
                                    q.open = None;
                                }
                            }
                        });
                    }
                    "tags" => {
                        egui::ScrollArea::vertical().max_height(190.0).show(ui, |ui| {
                            if tags.is_empty() {
                                ui.label(egui::RichText::new("No tags yet.").color(c.dim).size(12.5));
                            }
                            for (id, n) in tags {
                                let mut on = q.tags.contains(id);
                                if ui.checkbox(&mut on, n).changed() {
                                    if on {
                                        q.tags.push(*id);
                                    } else {
                                        q.tags.retain(|x| x != id);
                                    }
                                }
                            }
                        });
                    }
                    "due" => {
                        ui.horizontal(|ui| {
                            ui.add(
                                crate::nativetext::Edit::singleline(&mut q.due)
                                    .hint_text("today, tomorrow 5pm, fri, nov 15")
                                    .desired_width(300.0)
                                    .font(FontId::proportional(13.5)),
                            );
                            for (label, text) in [("Today", "today"), ("Tomorrow", "tomorrow"), ("Next Week", "next week")] {
                                if ui.small_button(label).clicked() {
                                    q.due = text.into();
                                }
                            }
                            if ui.small_button("Clear").clicked() {
                                q.due.clear();
                            }
                        });
                    }
                    _ => {
                        let presets: [(Option<Repeat>, &str); 5] = [
                            (None, "Never"),
                            (Some(Repeat { every: 1, unit: RepeatUnit::Day, ..Default::default() }), "Every Day"),
                            (Some(Repeat { every: 1, unit: RepeatUnit::Week, ..Default::default() }), "Every Week"),
                            (Some(Repeat { every: 2, unit: RepeatUnit::Week, ..Default::default() }), "Every 2 Weeks"),
                            (Some(Repeat { every: 1, unit: RepeatUnit::Month, ..Default::default() }), "Every Month"),
                        ];
                        for (preset, label) in presets {
                            let on = match (&q.repeat, &preset) {
                                (None, None) => true,
                                (Some(a), Some(b)) => a.every == b.every && a.unit == b.unit,
                                _ => false,
                            };
                            if ui.selectable_label(on, label).clicked() {
                                q.repeat = preset;
                                q.open = None;
                            }
                        }
                    }
                }
            });
        ui.add_space(8.0);
    }
    if sheet {
        return; // the sheet's Cancel · eye · Save are in its top bar
    }
    // the bottom of the card: the eye at the left, Cancel and Save at the right, on the card's floor
    // (not in egui's first, unbounded sizing pass, or the card would grow to the height of the screen)
    // a 26 pt row on the content's floor (after the fields in egui's first, unbounded sizing pass)
    let max = ui.max_rect();
    let y = if ui.is_sizing_pass() { ui.cursor().top() } else { (max.bottom() - 26.0).max(ui.cursor().top()) };
    let row = egui::Rect::from_min_max(pos2(max.left(), y), pos2(max.right(), y + 26.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let (er, erp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
        if erp.hovered() {
            ui.painter().rect_filled(er, 6.0, mix(c.field, c.text, 0.10));
        }
        eye_glyph(ui.painter(), er.center(), mix(c.dim, c.faint, 0.3));
        if erp.on_hover_text(if q.show_note { "Hide Note" } else { "Show Note" }).clicked() {
            q.show_note = !q.show_note;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if qe_button(ui, "Save", c.accent, c.on_accent).clicked() {
                *save = true;
            }
            ui.add_space(9.0);
            if qe_button(ui, "Cancel", mix(c.bg, c.text, 0.09), mix(c.text, c.dim, 0.10)).clicked() {
                *close = true;
            }
        });
    });
}

/// The iPhone/iPad Settings home page (its list of pages), as `settings_tab`.
const SETTINGS_HOME: u8 = 255;

/// Settings pages in sidebar order: Calendar sits after Notifications, Keyboard last.
/// The numbers are `settings_tab` (kept stable: `--settings N` and links into Settings use them).
const SETTINGS_PAGES: [(&str, u8); 10] = [
    ("General", 0),
    ("Organization", 8),
    ("Appearance", 1),
    ("Perspectives", 2),
    ("Dates & Times", 9),
    ("Notifications", 3),
    ("Calendar", 7),
    ("Sync", 6),
    ("Keyboard", 4),
    ("Status", status::STATUS_TAB),
];

/// iOS-style grouped rows: one rounded block, 44 pt rows, a hairline between rows from the
/// text's left edge, a dim › at the right. Returns the tapped row.
fn grouped_rows(ui: &mut egui::Ui, labels: &[&str]) -> Option<usize> {
    let c = th();
    let w = ui.available_width();
    let (block, _) = ui.allocate_exact_size(vec2(w, 44.0 * labels.len() as f32), Sense::hover());
    ui.painter().rect_filled(block, 12.0, mix(c.bg, c.text, 0.07));
    let mut hit = None;
    for (i, l) in labels.iter().enumerate() {
        let row = egui::Rect::from_min_size(block.min + vec2(0.0, 44.0 * i as f32), vec2(w, 44.0));
        let r = ui.interact(row, ui.id().with(("grouped-row", i)), Sense::click());
        if r.is_pointer_button_down_on() || r.hovered() {
            let corners = egui::CornerRadius { nw: if i == 0 { 12 } else { 0 }, ne: if i == 0 { 12 } else { 0 }, sw: if i + 1 == labels.len() { 12 } else { 0 }, se: if i + 1 == labels.len() { 12 } else { 0 } };
            ui.painter().rect_filled(row, corners, mix(c.bg, c.text, 0.12));
        }
        ui.painter().text(pos2(row.left() + 16.0, row.center().y), Align2::LEFT_CENTER, *l, FontId::proportional(16.0), c.text);
        let k = pos2(row.right() - 20.0, row.center().y);
        ui.painter().line_segment([k + vec2(-3.0, -5.5), k + vec2(2.5, 0.0)], Stroke::new(1.8, c.dim));
        ui.painter().line_segment([k + vec2(2.5, 0.0), k + vec2(-3.0, 5.5)], Stroke::new(1.8, c.dim));
        if i + 1 < labels.len() {
            ui.painter().line_segment([pos2(row.left() + 16.0, row.bottom()), pos2(row.right(), row.bottom())], Stroke::new(0.6, mix(c.bg, c.text, 0.16)));
        }
        if r.clicked() {
            hit = Some(i);
        }
    }
    hit
}

/// Inbox items that gained a project since last frame join `kept` (they stay in the Inbox until Clean Up); ones that
/// lost it again, or are gone, leave it.
fn track_inbox(prev: &HashSet<Id>, now: &HashSet<Id>, has_project: impl Fn(Id) -> bool, kept: &mut HashSet<Id>) {
    for id in prev.difference(now) {
        if has_project(*id) {
            kept.insert(*id);
        }
    }
    kept.retain(|id| has_project(*id));
}

/// A repo's open issues and PRs as fetched: (repo url, the list or why not).
type RefsList = (String, Result<Vec<refs::Ref>, String>);

/// Where the on-screen keyboard begins (the screen's bottom when it's hidden or there is none).
pub(crate) fn keyboard_top(ctx: &egui::Context) -> f32 {
    ctx.viewport_rect().bottom() - keyboard::height()
}

/// iPhone/iPad: the on-screen keyboard's height, from UIKit's `UIKeyboardWillChangeFrameNotification`
/// (`UIKeyboardFrameEndUserInfoKey`: when it's hidden its top is the screen's bottom, so the height is 0).
pub(crate) mod keyboard {
    #[cfg(target_os = "ios")]
    static HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    pub fn height() -> f32 {
        #[cfg(target_os = "ios")]
        {
            install();
            f32::from_bits(HEIGHT.load(std::sync::atomic::Ordering::Relaxed))
        }
        #[cfg(not(target_os = "ios"))]
        0.0
    }

    #[cfg(target_os = "ios")]
    fn install() {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::{NSRect, NSString};
        #[link(name = "UIKit", kind = "framework")]
        extern "C" {
            static UIKeyboardWillChangeFrameNotification: &'static NSString;
            static UIKeyboardFrameEndUserInfoKey: &'static NSString;
        }
        static ONCE: std::sync::Once = std::sync::Once::new();
        if objc2::MainThreadMarker::new().is_none() {
            return;
        }
        ONCE.call_once(|| unsafe {
            let block = block2::RcBlock::new(|n: *mut AnyObject| {
                let info: *mut AnyObject = msg_send![n, userInfo];
                if info.is_null() {
                    return;
                }
                let v: *mut AnyObject = msg_send![info, objectForKey: UIKeyboardFrameEndUserInfoKey];
                if v.is_null() {
                    return;
                }
                let frame: NSRect = msg_send![v, CGRectValue];
                let screen: *mut AnyObject = msg_send![class!(UIScreen), mainScreen];
                let bounds: NSRect = msg_send![screen, bounds];
                let h = (bounds.size.height - frame.origin.y).max(0.0) as f32;
                HEIGHT.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
            });
            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let queue: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
            let none: *mut AnyObject = std::ptr::null_mut();
            let _observer: *mut AnyObject = msg_send![center, addObserverForName: UIKeyboardWillChangeFrameNotification, object: none, queue: queue, usingBlock: &*block];
            std::mem::forget(block);
        });
    }
}

/// Height of the iPhone portrait bottom bar (commands over perspective tabs).
const TAB_BAR_H: f32 = 132.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phone {
    Portrait,
    Landscape,
}

/// The iPhone layouts, by the safe area's size in points: upright and narrow → the bottom bar;
/// sideways and short → rail, sidebar and list with floating buttons. iPad and Mac: neither.
fn phone_layout(ios: bool, size: Vec2) -> Option<Phone> {
    if !ios {
        None
    } else if size.y > size.x && size.x < COMPACT_W {
        Some(Phone::Portrait)
    } else if size.x > size.y && size.y < 600.0 {
        Some(Phone::Landscape)
    } else {
        None
    }
}

/// An iPhone held upright: a bottom bar replaces the perspective sidebar there.
fn phone_portrait(ctx: &egui::Context) -> bool {
    phone_layout(cfg!(target_os = "ios"), ctx.content_rect().size()) == Some(Phone::Portrait)
}

/// An iPhone on its side: the rail, a sidebar with its own buttons, and the list with its own top
/// bar and floating capsules; no inspector column (it opens as a page).
fn phone_landscape(ctx: &egui::Context) -> bool {
    phone_layout(cfg!(target_os = "ios"), ctx.content_rect().size()) == Some(Phone::Landscape)
}

/// iPhone portrait: how far the list scrolls before the large title has gone under the top bar (and shows there
/// small) and the bottom bar may shrink.
const TITLE_SCROLL: f32 = 44.0;

/// iPhone portrait bottom bar, after the list's scroll offset moved from `prev` to `offset`: it shrinks to a round
/// perspective button and a + capsule while the list scrolls down, and comes back when it scrolls up or reaches the
/// top (like iOS's tab bar "minimize on scroll down").
/// Forecast's countdown for a planned action: before it, the time until it ("in 2h 05m", then "in 12:34" in the last
/// hour, ticking); from the planned time until the duration has passed, the block's time left ("12:34 left of 45m").
/// The planned date and duration are the block, so there's no need for a due date or a calendar event. None more
/// than a day ahead (the date shows instead), after the block, or at the planned time when there's no duration.
/// The bool is true while inside the block.
/// A time interval for a relative notification's label (e.g. "1 Day" or "5 Minutes"): the largest whole
/// unit, capitalised, plural past one.
fn interval_label(minutes: i64) -> String {
    let (n, unit) = if minutes > 0 && minutes % 1440 == 0 { (minutes / 1440, "Day") } else if minutes > 0 && minutes % 60 == 0 { (minutes / 60, "Hour") } else { (minutes, "Minute") };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// Where now falls against a planned block — each its own colour on the countdown pill and the timeline (the person:
/// "time blocks should be colour coded distinctly to show beginning and ending").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BlockPhase {
    /// more than 5 minutes before it
    Ahead,
    /// the last 5 minutes before it starts
    Starting,
    /// in it
    Active,
    /// the last 5 minutes (or last fifth) of it
    Ending,
}

impl BlockPhase {
    fn color(self) -> Color32 {
        match self {
            BlockPhase::Ahead => mix(th().text, th().dim, 0.3),
            BlockPhase::Starting => hue("green"),
            BlockPhase::Active => th().accent,
            BlockPhase::Ending => hue("orange"),
        }
    }
}

fn block_countdown(planned: Dt, duration_min: Option<u32>, now: Dt) -> Option<(String, BlockPhase)> {
    let clock = |secs: i64| {
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
    };
    let until = (planned - now).num_seconds();
    if until > 0 {
        if until > 24 * 3600 {
            return None;
        }
        let mins = (until + 59) / 60; // whole minutes, rounded up
        let text = if until >= 3600 { format!("in {}h {:02}m", mins / 60, mins % 60) } else { format!("in {}", clock(until)) };
        return Some((text, if until <= 300 { BlockPhase::Starting } else { BlockPhase::Ahead }));
    }
    let len = i64::from(duration_min.filter(|m| *m > 0)?) * 60;
    let left = len + until; // until <= 0: seconds since the block began
    let total = if len >= 3600 { format!("{}h{}", len / 3600, if len % 3600 > 0 { format!(" {}m", len % 3600 / 60) } else { String::new() }) } else { format!("{}m", len / 60) };
    let phase = if left <= 300.max(len / 5) { BlockPhase::Ending } else { BlockPhase::Active };
    (left > 0).then(|| (format!("{} left of {total}", clock(left)), phase))
}

/// The bar's shape progress for linear animation time `t` (0 = full bar, 1 = split): a spring that settles with a
/// slight overshoot (about 4%) in the direction it's heading, so collapsing and expanding both feel elastic.
fn liquid(t: f32, collapsing: bool) -> f32 {
    let back_out = |x: f32| {
        let (c1, x1) = (0.9, x - 1.0);
        1.0 + (c1 + 1.0) * x1.powi(3) + c1 * x1.powi(2)
    };
    let t = t.clamp(0.0, 1.0);
    if collapsing {
        back_out(t)
    } else {
        1.0 - back_out(1.0 - t)
    }
}

fn bar_collapsed(was: bool, prev: f32, offset: f32) -> bool {
    if offset < TITLE_SCROLL {
        false
    } else if offset > prev + 1.0 {
        true
    } else if offset < prev - 6.0 {
        false
    } else {
        was
    }
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    #[test]
    fn layout_by_size() {
        // iPhone 17's safe area upright and sideways, iPad upright and sideways, a Mac window
        assert_eq!(phone_layout(true, vec2(402.0, 791.0)), Some(Phone::Portrait));
        assert_eq!(phone_layout(true, vec2(750.0, 381.0)), Some(Phone::Landscape));
        assert_eq!(phone_layout(true, vec2(820.0, 1160.0)), None);
        assert_eq!(phone_layout(true, vec2(1180.0, 800.0)), None);
        assert_eq!(phone_layout(false, vec2(402.0, 791.0)), None);
        assert_eq!(phone_layout(false, vec2(750.0, 381.0)), None);
    }

    #[test]
    fn bar_shrinks_scrolling_down_and_returns_scrolling_up() {
        assert!(!bar_collapsed(false, 0.0, 20.0)); // still near the top
        assert!(bar_collapsed(false, 40.0, 60.0)); // scrolling down past the title
        assert!(bar_collapsed(true, 300.0, 300.0)); // resting: stays as it was
        assert!(bar_collapsed(true, 300.0, 297.0)); // a small wobble up doesn't bring it back
        assert!(!bar_collapsed(true, 300.0, 280.0)); // scrolling up does
        assert!(!bar_collapsed(true, 60.0, 10.0)); // and so does the top
        assert!(bar_collapsed(false, 0.0, 300.0)); // opened already scrolled (`--scroll`)
        let at = |h: u32, m: u32, s: u32| NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().and_hms_opt(h, m, s).unwrap();
        let pl = at(14, 0, 0);
        assert_eq!(block_countdown(pl, Some(45), at(11, 50, 0)), Some(("in 2h 10m".into(), BlockPhase::Ahead)));
        assert_eq!(block_countdown(pl, Some(45), at(13, 47, 26)), Some(("in 12:34".into(), BlockPhase::Ahead)));
        assert_eq!(block_countdown(pl, Some(45), at(14, 0, 0)), Some(("45:00 left of 45m".into(), BlockPhase::Active)));
        assert_eq!(block_countdown(pl, Some(45), at(14, 32, 26)), Some(("12:34 left of 45m".into(), BlockPhase::Active)));
        assert_eq!(block_countdown(pl, Some(150), at(14, 0, 1)), Some(("2:29:59 left of 2h 30m".into(), BlockPhase::Active)));
        assert_eq!(block_countdown(pl, Some(45), at(14, 45, 0)), None); // the block is over
        assert_eq!(block_countdown(pl, Some(45), at(11, 0, 30)), Some(("in 3h 00m".into(), BlockPhase::Ahead))); // 2h 59m 30s rounds up
        assert_eq!(block_countdown(pl, None, at(14, 0, 1)), None); // no duration, no block
        assert_eq!([interval_label(5), interval_label(60), interval_label(90), interval_label(120), interval_label(1440)], ["5 Minutes", "1 Hour", "90 Minutes", "2 Hours", "1 Day"]);
        // the phases: starting in the last 5 minutes before, ending in the last 5 minutes (or fifth) of the block
        assert_eq!(block_countdown(pl, Some(45), at(13, 56, 0)).map(|x| x.1), Some(BlockPhase::Starting));
        assert_eq!(block_countdown(pl, Some(45), at(14, 36, 0)).map(|x| x.1), Some(BlockPhase::Ending)); // 9 min left = last fifth
        assert_eq!(block_countdown(pl, Some(45), at(14, 35, 0)).map(|x| x.1), Some(BlockPhase::Active));
        assert_eq!(block_countdown(pl, Some(45), at(13, 0, 0) - Duration::days(1)), None); // more than a day ahead
        // the shape starts and ends exactly, overshoots a little on the way, and never jumps
        for dir in [true, false] {
            assert_eq!((liquid(0.0, dir), liquid(1.0, dir)), (0.0, 1.0));
            let pts: Vec<f32> = (0..=50).map(|i| liquid(i as f32 / 50.0, dir)).collect();
            assert!(pts.windows(2).all(|w| (w[1] - w[0]).abs() < 0.08), "{dir}: {pts:?}");
            // collapsing overshoots past 1, expanding past 0 (beyond the full bar)
            let over = if dir { pts.iter().cloned().fold(f32::MIN, f32::max) - 1.0 } else { -pts.iter().cloned().fold(f32::MAX, f32::min) };
            assert!(over > 0.01 && over < 0.08, "{dir}: overshoot {over}");
        }
    }
}

/// `--landscape`: ask iOS to turn the window scene sideways (UIWindowScene requestGeometryUpdate, iOS 16+).
#[cfg(target_os = "ios")]
fn rotate_to_landscape() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        let scenes: *mut AnyObject = msg_send![app, connectedScenes];
        let scene: *mut AnyObject = msg_send![scenes, anyObject];
        if scene.is_null() {
            return;
        }
        let prefs: *mut AnyObject = msg_send![class!(UIWindowSceneGeometryPreferencesIOS), alloc];
        // UIInterfaceOrientationMaskLandscapeRight
        let prefs: *mut AnyObject = msg_send![prefs, initWithInterfaceOrientations: 1usize << 3];
        let none: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![scene, requestGeometryUpdateWithPreferences: prefs, errorHandler: none];
    }
}

/// iPhone/iPad: panels sit inside the safe area (egui's content_rect); paint what's around it — under the status bar,
/// the home indicator and the notch sides — in the colours that meet it, so the app fills the screen with no black bars.
fn paint_safe_areas(ctx: &egui::Context) {
    let (full, safe) = (ctx.viewport_rect(), ctx.content_rect());
    if full == safe {
        return;
    }
    let mut p = ctx.layer_painter(egui::LayerId::background());
    p.set_clip_rect(full);
    let c = th();
    p.rect_filled(egui::Rect::from_min_max(full.min, pos2(full.max.x, safe.min.y)), 0.0, c.toolbar);
    p.rect_filled(egui::Rect::from_min_max(pos2(full.min.x, safe.max.y), full.max), 0.0, c.bg);
    if !phone_portrait(ctx) {
        p.rect_filled(egui::Rect::from_min_max(pos2(full.min.x, safe.max.y), pos2(safe.min.x + RAIL_W, full.max.y)), 0.0, c.rail); // the rail runs on down
    }
    p.rect_filled(egui::Rect::from_min_max(pos2(full.min.x, safe.min.y), pos2(safe.min.x, safe.max.y)), 0.0, c.rail);
    p.rect_filled(egui::Rect::from_min_max(pos2(safe.max.x, safe.min.y), pos2(full.max.x, safe.max.y)), 0.0, c.bg);
}

/// The narrowest Quick Entry card for the inline layout: the metadata font's width of "Project Tags D D"
/// (D = the widest short-style date of today, yesterday, tomorrow and 20 Dec 2022) + 4 field icons + 23 + 58 +
/// 18 points.
fn quick_min_width(mut measure: impl FnMut(&str) -> f32, today: NaiveDate) -> f32 {
    let dates = [today, today - Duration::days(1), today + Duration::days(1), NaiveDate::from_ymd_opt(2022, 12, 20).unwrap()];
    let widest = dates.iter().map(|d| d.format("%-m/%-d/%y").to_string()).map(|t| (measure(&t), t)).max_by(|a, b| a.0.total_cmp(&b.0)).unwrap().1;
    const ICON: f32 = 12.0; // GTDCraft's field glyphs
    measure(&format!("Project Tags {widest} {widest}")) + 4.0 * ICON + 23.0 + 58.0 + 18.0
}

#[cfg(test)]
mod quick_tests {
    use super::*;

    /// Quick Entry saves its frame on the frame the pointer is released: check egui moves a title-less, resizable
    /// window by its background and reports the new place on that frame.
    #[test]
    fn card_drags_by_background_and_reports_on_release() {
        let ctx = egui::Context::default();
        let btn = |p: Pos2, down| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: down, modifiers: Default::default() };
        let (a, b) = (pos2(250.0, 170.0), pos2(310.0, 220.0));
        let frames = vec![vec![], vec![], vec![egui::Event::PointerMoved(a)], vec![btn(a, true)], vec![egui::Event::PointerMoved(pos2(270.0, 190.0))], vec![egui::Event::PointerMoved(b)], vec![btn(b, false)]];
        let mut seen = vec![];
        for (n, events) in frames.into_iter().enumerate() {
            let raw = egui::RawInput { events, time: Some(n as f64 * 0.1), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                let r = egui::Window::new("quick-entry").title_bar(false).collapsible(false).resizable(true).default_size(vec2(300.0, 120.0)).default_pos(pos2(100.0, 100.0)).show(ctx, |ui| {
                    ui.allocate_space(vec2(300.0, 120.0)); // the card's empty background (labels would select text instead)
                });
                seen.push((r.unwrap().response.rect.min, ctx.input(|i| i.pointer.any_released())));
            });
        }
        let (last, released) = *seen.last().unwrap();
        assert!(released, "release is seen on the last frame");
        assert_eq!(last, seen[0].0 + (b - a), "the card moved with the pointer: {seen:?}");
    }

    #[test]
    fn min_width_follows_the_original_formula() {
        // 1 pt per character: "12/20/22" is the widest date, so "Project Tags 12/20/22 12/20/22" = 30 chars
        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(super::quick_min_width(|t| t.chars().count() as f32, today), 30.0 + 48.0 + 99.0);
    }
}

/// A Quick Entry field chip at `scale` (1 on the Mac card; the iPhone sheet's fields are larger).
fn qe_chip_sized(ui: &mut egui::Ui, scale: f32, glyph: QeGlyph, label: &str, set: bool) -> egui::Response {
    let c = th();
    let col = if set { mix(c.text, c.field, 0.30) } else { mix(c.dim, c.faint, 0.30) };
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5 * scale * scale), col);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0 * scale, 18.0 * scale), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 6.0, mix(c.field, c.text, 0.10));
    }
    let gc = pos2(rect.left() + 7.0 * scale, rect.center().y);
    match glyph {
        QeGlyph::Project => project_glyph(p, gc, 0.80, col),
        QeGlyph::Tag => tag_glyph(p, gc, 0.95, col),
        QeGlyph::Due => cal_glyph(p, gc, col),
        QeGlyph::Repeat => repeat_glyph(p, gc, col),
    }
    p.galley(pos2(rect.left() + 15.0 * scale * scale, rect.center().y - galley.size().y / 2.0), galley, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn qe_button(ui: &mut egui::Ui, label: &str, fill: Color32, text: Color32) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(label).size(13.0).color(text)).fill(fill).corner_radius(6).min_size(vec2(80.0, 24.0)))
}

fn tag_glyph(p: &egui::Painter, c: Pos2, scale: f32, col: Color32) {
    let pts = vec![c + vec2(-5.5, 1.2) * scale, c + vec2(0.0, -4.3) * scale, c + vec2(4.8, -4.3) * scale, c + vec2(4.8, 0.6) * scale, c + vec2(-0.7, 5.0) * scale];
    p.add(Shape::closed_line(pts, Stroke::new(1.2, col)));
    p.circle_filled(c + vec2(2.5, -2.4) * scale, 0.85 * scale, col);
}

/// The repeat icon: two arcs with arrow heads chasing each other.
fn repeat_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.1, col);
    for k in 0..2 {
        let base = k as f32 * std::f32::consts::PI;
        let pts: Vec<Pos2> = (0..=10).map(|i| {
            let a = base + 0.45 + i as f32 / 10.0 * 2.1;
            c + vec2(5.2 * a.cos(), 5.2 * a.sin())
        }).collect();
        let end = pts[pts.len() - 1];
        let prev = pts[pts.len() - 2];
        let dir = (end - prev).normalized();
        let n = vec2(-dir.y, dir.x);
        p.add(Shape::line(pts, s));
        p.add(Shape::convex_polygon(vec![end + dir * 2.4, end - dir * 1.2 + n * 1.9, end - dir * 1.2 - n * 1.9], col, Stroke::NONE));
    }
}

fn note_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(15.0, 12.0));
    p.rect_stroke(r, 2.0, Stroke::new(1.2, col), StrokeKind::Middle);
    for k in 0..3 {
        let y = r.top() + 3.5 + k as f32 * 2.6;
        p.line_segment([pos2(r.left() + 3.0, y), pos2(r.right() - 3.0 - (k == 2) as i32 as f32 * 3.0, y)], Stroke::new(1.0, col));
    }
}

/// The iPhone's command buttons (bottom bar, floating capsules).
#[derive(Clone, Copy)]
enum PhoneCmd {
    /// ⚡ over a card: jump to any perspective, project or tag
    QuickOpen,
    Search,
    /// + : New Action
    New,
    /// an inbox with + : Quick Entry
    QuickEntry,
}

fn cmd_glyph(p: &egui::Painter, k: Pos2, cmd: PhoneCmd, ink: Color32, bg: Color32) {
    match cmd {
        PhoneCmd::QuickOpen => {
            p.rect_stroke(Rect::from_center_size(k + vec2(0.0, 1.0), vec2(20.0, 18.0)), 4.0, Stroke::new(1.6, ink), StrokeKind::Middle);
            p.add(Shape::convex_polygon(vec![k + vec2(1.5, -7.0), k + vec2(-4.0, 2.0), k + vec2(0.0, 2.0), k + vec2(-1.5, 8.0), k + vec2(4.5, -1.0), k + vec2(0.5, -1.0)], ink, Stroke::NONE));
        }
        PhoneCmd::Search => {
            p.circle_stroke(k + vec2(-2.0, -2.0), 7.0, Stroke::new(1.8, ink));
            p.line_segment([k + vec2(3.0, 3.0), k + vec2(8.0, 8.0)], Stroke::new(2.0, ink));
        }
        PhoneCmd::New => {
            p.line_segment([k + vec2(-9.0, 0.0), k + vec2(9.0, 0.0)], Stroke::new(2.0, ink));
            p.line_segment([k + vec2(0.0, -9.0), k + vec2(0.0, 9.0)], Stroke::new(2.0, ink));
        }
        PhoneCmd::QuickEntry => {
            p.rect_filled(Rect::from_center_size(k, vec2(22.0, 20.0)), 5.0, ink);
            p.line_segment([k + vec2(-5.0, 0.0), k + vec2(5.0, 0.0)], Stroke::new(2.0, bg));
            p.line_segment([k + vec2(0.0, -5.0), k + vec2(0.0, 5.0)], Stroke::new(2.0, bg));
        }
    }
}

fn eye_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    p.add(Shape::closed_line(
        vec![c + vec2(-9.0, 0.0), c + vec2(-5.0, -4.2), c + vec2(0.0, -5.2), c + vec2(5.0, -4.2), c + vec2(9.0, 0.0), c + vec2(5.0, 4.2), c + vec2(0.0, 5.2), c + vec2(-5.0, 4.2)],
        Stroke::new(1.3, col),
    ));
    p.circle_filled(c, 2.6, col);
}

// ---------- small widgets ----------
fn section(ui: &mut egui::Ui) {
    ui.add_space(10.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, th().line));
    ui.add_space(10.0);
}
fn caption(ui: &mut egui::Ui, s: &str) {
    ui.label(egui::RichText::new(s).font(bold(14.0)).color(th().text));
}
/// A caption with its current value at the right ("Distance  ·····  ~¼ mi").
fn caption_value(ui: &mut egui::Ui, s: &str, value: &str) {
    ui.horizontal(|ui| {
        caption(ui, s);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(value).color(th().dim)));
    });
}

/// The Projects sidebar list: folders, projects (click to filter), and dragging a project onto a folder,
/// a project in a folder, or "No Folder" to move it. `folder_drop` gets (project, new folder) on a drop.
fn project_list(ui: &mut egui::Ui, mut ps: Vec<&Project>, folders: &[String], dropped: &[String], filter: Filter, acts: &mut Vec<Act>, folder_drop: &mut Option<(Id, String)>) {
    let is_dropped = |f: &str| dropped.iter().any(|d| model::in_folder(f, d));
    ps.retain(|p| !is_dropped(&p.folder)); // a dropped folder's projects are dropped too
    let mut last_folder: Option<String> = None;
    ps.sort_by(|a, b| a.folder.cmp(&b.folder));
    // folders without projects still get their row, in name order among the others
    let mut rows: Vec<(String, Option<&Project>)> = ps.iter().map(|p| (p.folder.clone(), Some(*p))).collect();
    rows.extend(folders.iter().filter(|f| !ps.iter().any(|p| &p.folder == *f)).map(|f| (f.clone(), None)));
    // loose projects first, then folders in their placed order (`folders` is `Store::folder_names`)
    let rank = |f: &str| if f.is_empty() { 0 } else { 1 + folders.iter().position(|x| x == f).unwrap_or(folders.len()) };
    rows.sort_by_key(|r| rank(&r.0));
    let dragging = egui::DragAndDrop::payload::<ProjectDrag>(ui.ctx()).map(|p| p.0);
    let target = |ui: &egui::Ui, r: &egui::Response, folder: &str, drop: &mut Option<(Id, String)>| {
        if r.dnd_hover_payload::<ProjectDrag>().is_some() {
            ui.painter().rect_stroke(r.rect.shrink(1.0), 6.0, Stroke::new(1.5, th().accent), egui::StrokeKind::Inside);
        }
        if let Some(p) = r.dnd_release_payload::<ProjectDrag>() {
            *drop = Some((p.0, folder.to_string()));
        }
    };
    for (folder, p) in rows {
        if last_folder.as_deref() != Some(&folder) && !folder.is_empty() {
            // nested folders ("Work : Clients") show their own name, indented under their parent
            let depth = folder.matches(model::FOLDER_SEP).count() as f32;
            let short = folder.rsplit(model::FOLDER_SEP).next().unwrap_or(&folder).to_string();
            let label = if is_dropped(&folder) { format!("{short} (dropped)") } else { short };
            let fr = column_item(ui, &label, false, 16.0 * depth, ColIcon::Folder, hue("blue")).interact(Sense::click());
            target(ui, &fr, &folder, folder_drop);
            fr.context_menu(|ui| {
                let drop = if dropped.contains(&folder) { ("Mark Folder Active", Act::DropFolder(folder.clone(), false)) } else { ("Drop Folder", Act::DropFolder(folder.clone(), true)) };
                for (label, act) in [("New Folder", Act::NewFolder(Some(folder.clone()))), ("New Folder Inside", Act::NewSubfolder(folder.clone())), ("Rename Folder…", Act::RenameFolder(folder.clone())), drop, ("Delete Folder and Its Projects", Act::DeleteFolder(folder.clone()))] {
                    if ui.button(label).clicked() {
                        acts.push(act);
                        ui.close();
                    }
                }
            });
        }
        last_folder = Some(folder.clone());
        let Some(p) = p else { continue };
        let indent = if p.folder.is_empty() { 0.0 } else { 16.0 * (1.0 + p.folder.matches(model::FOLDER_SEP).count() as f32) };
        let on = filter == Filter::Project(p.id);
        let label = if p.status == ProjectStatus::OnHold { format!("{} (on hold)", p.name) } else { p.name.clone() };
        // the row itself takes both clicks (filter + select) and drags (move between folders)
        let r = column_item(ui, &label, on, indent, ColIcon::Project, hue("blue")).interact(Sense::click_and_drag());
        if r.clicked() {
            acts.push(Act::Filter(if on { Filter::All } else { Filter::Project(p.id) }));
            acts.push(Act::Select(Sel::Project(p.id)));
        }
        if r.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), ProjectDrag(p.id));
        }
        r.context_menu(|ui| {
            for (label, st) in [("Complete", ProjectStatus::Completed), ("Drop", ProjectStatus::Dropped), ("On Hold", ProjectStatus::OnHold), ("Active", ProjectStatus::Active)] {
                if ui.button(label).clicked() {
                    acts.push(Act::ProjectStatus(p.id, st));
                    ui.close();
                }
            }
            ui.separator();
            for (label, folder) in [("Attach File\u{2026}", false), ("Attach Folder\u{2026}", true)] {
                if ui.button(label).clicked() {
                    acts.push(Act::Attach(Sel::Project(p.id), folder));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Delete").clicked() {
                acts.push(Act::Menu(Sel::Project(p.id), keys::Action::Delete));
                ui.close();
            }
        });
        if dragging == Some(p.id) {
            ui.painter().rect_filled(r.rect, 6.0, th().pill.gamma_multiply(0.6));
        } else {
            target(ui, &r, &p.folder, folder_drop);
        }
    }
    // the empty space below the list: right-click for a new project or folder
    if dragging.is_none() {
        let rest = (ui.clip_rect().bottom() - ui.cursor().top()).clamp(60.0, 2000.0); // (a scroll area's height is unbounded)
        let (_, bg) = ui.allocate_exact_size(vec2(ui.available_width(), rest), Sense::click());
        bg.context_menu(|ui| {
            for (label, act) in [("New Project", Act::NewProject), ("New Folder", Act::NewFolder(None))] {
                if ui.button(label).clicked() {
                    acts.push(act);
                    ui.close();
                }
            }
        });
    }
    if let Some(pid) = dragging {
        ui.add_space(6.0);
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        ui.data_mut(|d| d.insert_temp(egui::Id::new("no-folder-box"), r)); // where it is (tests drop onto it)
        ui.painter().rect_stroke(r, 6.0, Stroke::new(1.0, th().faint), egui::StrokeKind::Inside);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "No Folder", FontId::proportional(13.0), th().dim);
        target(ui, &resp, "", folder_drop);
        // the dragged project follows the pointer
        if let (Some(pos), Some(p)) = (ui.ctx().pointer_latest_pos(), ps.iter().find(|p| p.id == pid)) {
            egui::Area::new(egui::Id::new("project-drag-ghost")).order(egui::Order::Tooltip).fixed_pos(pos + vec2(12.0, 6.0)).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(&p.name));
            });
        }
    }
}

/// The action shortcut menu (right-click or Control-click):
/// top-level commands, then Edit, Type, Organize and Schedule groups. Commands go through the same actions as the
/// menu bar and keys, so they apply to every selected row.
/// Copy as List (`meta` false) or Copy as List with Context (true), in TaskPaper format:
/// only the top items of the selection (a picked child of a picked parent comes
/// with its parent), each as "- name" with its sub-actions tab-indented under it and
/// notes as indented lines; with context, TaskPaper's tags follow the name — @project, @tags, @defer, @planned, @due,
/// @estimate, @flagged, @done.
fn copy_list(store: &Store, ids: &[Id], meta: bool) -> String {
    let mut out = String::new();
    let fmt = |d: Dt| d.format("%Y-%m-%d %H:%M").to_string();
    fn write(store: &Store, id: Id, depth: usize, meta: bool, top: bool, out: &mut String, fmt: &dyn Fn(Dt) -> String) {
        let Some(t) = store.task(id) else { return };
        let tabs = "\t".repeat(depth);
        out.push_str(&format!("{tabs}- {}", t.name.trim()));
        if meta {
            let mut tags = vec![];
            if top {
                if let Some(p) = t.project.and_then(|p| store.project(p)) {
                    tags.push(format!("@project({})", p.name));
                }
            }
            let names: Vec<String> = t.tags.iter().filter_map(|g| store.tag(*g)).map(|g| g.name.clone()).collect();
            if !names.is_empty() {
                tags.push(format!("@tags({})", names.join(", ")));
            }
            for (k, v) in [("defer", t.defer), ("planned", t.planned), ("due", t.due)] {
                if let Some(v) = v {
                    tags.push(format!("@{k}({})", fmt(v)));
                }
            }
            if let Some(m) = t.duration_min {
                tags.push(format!("@estimate({m}m)"));
            }
            if t.flagged {
                tags.push("@flagged".into());
            }
            if let Some(c) = t.completed {
                tags.push(format!("@done({})", fmt(c)));
            }
            for g in tags {
                out.push(' ');
                out.push_str(&g);
            }
        }
        out.push('\n');
        for l in t.note.lines().filter(|l| !l.trim().is_empty()) {
            out.push_str(&format!("{tabs}\t{}\n", l.trim_end()));
        }
        for c in store.tasks.iter().filter(|c| c.parent == Some(id) && c.dropped.is_none()) {
            write(store, c.id, depth + 1, meta, false, out, fmt);
        }
    }
    let set: HashSet<Id> = ids.iter().copied().collect();
    let has_picked_ancestor = |id: Id| {
        let mut p = store.task(id).and_then(|t| t.parent);
        while let Some(x) = p {
            if set.contains(&x) {
                return true;
            }
            p = store.task(x).and_then(|t| t.parent);
        }
        false
    };
    for &id in ids.iter().filter(|id| !has_picked_ancestor(**id)) {
        write(store, id, 0, meta, true, &mut out, &fmt);
    }
    out
}

/// Puts text on the clipboard: the Mac's pasteboard (`pbcopy`), iOS's UIPasteboard.
fn copy_text(text: &str) {
    #[cfg(target_os = "macos")]
    if let Ok(mut c) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
        use std::io::Write;
        if let Some(mut i) = c.stdin.take() {
            let _ = i.write_all(text.as_bytes());
        }
        let _ = c.wait();
    }
    #[cfg(target_os = "ios")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let pb: *mut AnyObject = msg_send![class!(UIPasteboard), generalPasteboard];
        let _: () = msg_send![pb, setString: &*objc2_foundation::NSString::from_str(text)];
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = text;
}

/// Adds `paths` to each task's or project's Attachments (skipping ones it already has); true when anything changed.
/// Attachments are links to the file, so each selected item gets the link and there's nothing to ask.
#[cfg_attr(target_os = "ios", allow(dead_code))]
fn attach_to(store: &mut Store, targets: &[Sel], paths: &[String], now: chrono::NaiveDateTime) -> bool {
    let mut changed = false;
    for &s in targets {
        let item = match s {
            Sel::Task(id) => store.task_mut(id).map(|t| (&mut t.attachments, &mut t.modified)),
            Sel::Project(id) => store.project_mut(id).map(|p| (&mut p.attachments, &mut p.modified)),
            _ => None,
        };
        let Some((list, modified)) = item else { continue };
        let before = list.len();
        for p in paths {
            if !list.contains(p) {
                list.push(p.clone());
            }
        }
        if list.len() != before {
            *modified = now;
            changed = true;
        }
    }
    changed
}

/// An action's shortcut-menu commands, by section: the Mac menu shows Type, Organize and Schedule as submenus and
/// the rest as items; the iPhone sheet lists every section in full (submenus that open on hover don't work by touch).
fn row_menu_items(id: Id, t: &Task, has_kids: bool, agents: bool) -> Vec<(&'static str, String, Act)> {
    use keys::Action as A;
    let s = Sel::Task(id);
    let mut v: Vec<(&'static str, String, Act)> = vec![
        ("", (if t.flagged { "Unflag" } else { "Flag" }).into(), Act::Menu(s, A::ToggleFlag)),
        ("", (if t.completed.is_some() { "Mark Incomplete" } else { "Complete" }).into(), Act::Menu(s, A::ToggleComplete)),
        ("", (if t.dropped.is_some() { "Mark Active" } else { "Drop" }).into(), Act::Menu(s, A::ToggleDrop)),
        ("", "Inspect".into(), Act::Select(s)),
    ];
    if !cfg!(target_os = "ios") {
        v.push(("", "Attach File\u{2026}".into(), Act::Attach(s, false)));
        v.push(("", "Attach Folder\u{2026}".into(), Act::Attach(s, true)));
    }
    if agents {
        v.push(("", "Send to Agent\u{2026}".into(), Act::AgentSend(id)));
    }
    v.push(("", "Copy as List".into(), Act::Copy(s, false)));
    v.push(("", "Copy as List with Context".into(), Act::Copy(s, true)));
    v.push(("-", "Duplicate".into(), Act::Menu(s, A::Duplicate)));
    v.push(("-", "Add to Up Next".into(), Act::Menu(s, A::QueueAdd)));
    v.push(("-", "Start Pomodoro".into(), Act::Menu(s, A::Pomodoro)));
    if has_kids {
        for (label, on, act) in [("Parallel", !t.sequential, Act::GroupType(id, false)), ("Sequential", t.sequential, Act::GroupType(id, true)), ("Complete with Last Action", t.complete_with_last, Act::CompleteWithLast(id))] {
            v.push(("Type", format!("{}{label}", if on { "✓ " } else { "" }), act));
        }
    }
    let mut org = vec![("Add Inside", A::AddInside), ("Add Action", A::NewBelow), ("Group", A::Group)];
    if has_kids {
        org.push(("Ungroup", A::Ungroup));
    }
    org.extend([("Indent", A::Indent), ("Outdent", A::Outdent), ("Move Up", A::MoveUp), ("Move Down", A::MoveDown), ("Convert to Project", A::ConvertToProject)]);
    v.extend(org.into_iter().map(|(l, a)| ("Organize", l.to_string(), Act::Menu(s, a))));
    for (label, k) in [("Due Today", Schedule::DueToday), ("Due Tomorrow", Schedule::DueTomorrow), ("Defer until Tomorrow", Schedule::DeferTomorrow), ("Plan for Today", Schedule::PlanToday), ("Plan for Tomorrow", Schedule::PlanTomorrow)] {
        v.push(("Schedule", label.into(), Act::Schedule(id, k)));
    }
    v.push(("Delete", "Delete".into(), Act::Menu(s, A::Delete)));
    v
}

fn task_menu(resp: &egui::Response, id: Id, t: &Task, has_kids: bool, open_now: bool, agents: bool, acts: &mut Vec<Act>) {
    // iPhone/iPad: touch and hold (or a pointer's secondary click) opens the commands as a sheet
    if cfg!(target_os = "ios") {
        if resp.secondary_clicked() || resp.long_touched() || open_now {
            acts.push(Act::RowSheet(id));
        }
        return;
    }
    // like egui's context menu, but Control-click (the Mac shortcut-menu gesture) opens it too
    let open = if resp.secondary_clicked() || open_now {
        Some(egui::SetOpenCommand::Bool(true))
    } else if resp.clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    let popup = egui::Popup::menu(resp).open_memory(open);
    // at the pointer, like egui's context menu; under the row when there's no pointer (opened from code)
    let popup = if resp.ctx.pointer_latest_pos().is_some() { popup.at_pointer_fixed() } else { popup };
    popup.show(|ui| {
        let mut items = row_menu_items(id, t, has_kids, agents);
        // separators between groups, as before: commands · Duplicate… · the submenus · Delete
        fn group(sec: &str) -> &str {
            if matches!(sec, "Type" | "Organize" | "Schedule") { "sub" } else { sec }
        }
        let mut last = "";
        let mut picked = None;
        let mut i = 0;
        while i < items.len() {
            let sec = items[i].0;
            if group(sec) != group(last) {
                ui.separator();
            }
            last = sec;
            if matches!(sec, "Type" | "Organize" | "Schedule") {
                let n = items[i..].iter().take_while(|x| x.0 == sec).count();
                ui.menu_button(sec, |ui| {
                    for (j, it) in items[i..i + n].iter().enumerate() {
                        if menu_item(ui, &it.1) {
                            picked = Some(i + j);
                        }
                    }
                });
                i += n;
                continue;
            }
            if menu_item(ui, &items[i].1) {
                picked = Some(i);
            }
            i += 1;
        }
        if let Some(i) = picked {
            acts.push(items.swap_remove(i).2);
        }
    });
}

fn menu_item(ui: &mut egui::Ui, label: &str) -> bool {
    let c = ui.button(label).clicked();
    if c {
        ui.close();
    }
    c
}

/// Something on a day's timeline: hours from midnight, title, colour; `block` for an action's planned block.
struct Span {
    from: f32,
    to: f32,
    title: String,
    color: Color32,
    block: Option<(Dt, Option<u32>)>,
}

/// A day's timeline (Forecast): calendar events in their calendars' colours and planned blocks (planned time + duration)
/// across the day's hours — or down them, hours as rows, with Forecast's view option "Vertical" (the person: "allow
/// horizontal or vertical time block display"). A block's start edge is green and its end edge orange; the one you're
/// in takes its phase's colour. Now is the red line. All-day events only fill it with "All-day events block time".
fn day_timeline(ui: &mut egui::Ui, spans: &[Span], now_t: Option<NaiveTime>, vertical: bool, now: Dt) {
    use chrono::Timelike;
    let nowh = now_t.map(|t| t.hour() as f32 + t.minute() as f32 / 60.0);
    let fill = |s: &Span| match s.block {
        Some((pl, dur)) => block_countdown(pl, dur, now).map(|x| x.1).filter(|p| matches!(p, BlockPhase::Active | BlockPhase::Ending)).map(|p| p.color()).unwrap_or(s.color),
        None => s.color,
    };
    if !vertical {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let x0 = r.left() + 52.0;
        let w = (r.right() - 40.0 - x0) / 24.0;
        let p = ui.painter();
        p.rect_filled(Rect::from_min_size(pos2(x0, r.top() + 4.0), vec2(24.0 * w, 20.0)), 3.0, th().field);
        for (s, lane) in spans.iter().map(|s| (s, if s.block.is_some() { 1.0 } else { 0.0 })) {
            let bar = Rect::from_min_max(pos2(x0 + s.from * w, r.top() + 4.0 + lane * 10.0), pos2(x0 + s.to * w, r.top() + 13.0 + lane * 10.0));
            p.rect_filled(bar, 2.0, fill(s).gamma_multiply(0.9));
            if s.block.is_some() {
                p.rect_filled(Rect::from_min_size(bar.min, vec2(2.5, bar.height())), 1.0, hue("green"));
                p.rect_filled(Rect::from_min_size(pos2(bar.right() - 2.5, bar.top()), vec2(2.5, bar.height())), 1.0, hue("orange"));
            }
        }
        for (h, l) in [(0, "12a"), (6, "6a"), (12, "12p"), (18, "6p")] {
            p.text(pos2(x0 + h as f32 * w, r.top() + 33.0), Align2::LEFT_CENTER, l, FontId::proportional(10.0), th().faint);
        }
        if let Some(h) = nowh {
            p.line_segment([pos2(x0 + h * w, r.top() + 1.0), pos2(x0 + h * w, r.top() + 27.0)], Stroke::new(2.0, th().overdue));
        }
        return;
    }
    // vertical: the hours that matter (8 AM–6 PM at least, widened to everything on the day and now)
    let lo = spans.iter().map(|s| s.from).chain(nowh).fold(8.0f32, f32::min).floor().max(0.0);
    let hi = spans.iter().map(|s| s.to).chain(nowh).fold(18.0f32, f32::max).ceil().min(24.0);
    let hh = 34.0;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), (hi - lo) * hh + 8.0), Sense::hover());
    let p = ui.painter();
    let x0 = r.left() + 92.0;
    let right = r.right() - 30.0;
    let y = |h: f32| r.top() + 4.0 + (h - lo) * hh;
    for h in lo as u32..=hi as u32 {
        let yy = y(h as f32);
        p.line_segment([pos2(x0, yy), pos2(right, yy)], Stroke::new(1.0, th().line));
        if h < 24 {
            let label = NaiveTime::from_hms_opt(h, 0, 0).map(|t| t.format("%-I %p").to_string()).unwrap_or_default();
            p.text(pos2(x0 - 8.0, yy), Align2::RIGHT_CENTER, label, FontId::proportional(11.0), th().faint);
        }
    }
    // events in the left half and blocks in the right — or the whole width when there's only one kind
    let both = spans.iter().any(|s| s.block.is_some()) && spans.iter().any(|s| s.block.is_none());
    let mid = (x0 + right) / 2.0;
    for s in spans {
        let (l, rr) = match (both, s.block.is_some()) {
            (false, _) => (x0 + 2.0, right),
            (true, true) => (mid + 2.0, right),
            (true, false) => (x0 + 2.0, mid - 2.0),
        };
        let rect = Rect::from_min_max(pos2(l, y(s.from) + 1.0), pos2(rr, y(s.to).max(y(s.from) + 22.0) - 1.0));
        let c = fill(s);
        p.rect_filled(rect, 5.0, mix(th().bg, c, 0.35));
        p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 2.0, c);
        if s.block.is_some() {
            p.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 2.5)), 1.0, hue("green"));
            p.rect_filled(Rect::from_min_size(pos2(rect.left(), rect.bottom() - 2.5), vec2(rect.width(), 2.5)), 1.0, hue("orange"));
        }
        let mut job = egui::text::LayoutJob::simple_singleline(s.title.clone(), FontId::proportional(12.0), th().text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 12.0);
        let g = p.layout_job(job);
        p.galley(pos2(rect.left() + 8.0, rect.top() + 3.0), g, th().text);
    }
    if let Some(h) = nowh {
        p.line_segment([pos2(x0 - 4.0, y(h)), pos2(right, y(h))], Stroke::new(2.0, th().overdue));
        p.circle_filled(pos2(x0 - 4.0, y(h)), 3.5, th().overdue);
    }
}

/// A Calendar event in Forecast: the calendar's colour bar, its time, title and calendar (location on hover).
fn event_row(ui: &mut egui::Ui, e: &calendar::Event, day: NaiveDate) {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let c = e.color.map(|[r, g, b]| Color32::from_rgb(r, g, b)).unwrap_or(th().accent);
    let x0 = r.left() + 52.0;
    let bar = Rect::from_min_size(pos2(x0 - 22.0, r.top() + 5.0), vec2(4.0, r.height() - 10.0));
    ui.painter().rect_filled(bar, 2.0, c);
    let time = calendar::time_label(e, day);
    let g = ui.painter().layout_no_wrap(time, FontId::proportional(12.5), th().dim);
    let tw = g.size().x.max(118.0);
    ui.painter().galley(pos2(x0, r.center().y - g.size().y / 2.0), g, th().dim);
    let title = ui.painter().layout_no_wrap(e.title.clone(), FontId::proportional(14.5), th().text);
    let tx = x0 + tw + 14.0;
    let ttw = title.size().x;
    ui.painter().galley(pos2(tx, r.center().y - title.size().y / 2.0), title, th().text);
    ui.painter().text(pos2(tx + ttw + 10.0, r.center().y), Align2::LEFT_CENTER, &e.calendar, FontId::proportional(11.5), th().faint);
    if !e.location.is_empty() {
        resp.on_hover_text(&e.location);
    }
}

/// A list you reorder by dragging the grip at the left of each row. Returns (from, to) when a drop moves a row.
fn drag_list(ui: &mut egui::Ui, id: egui::Id, n: usize, mut row: impl FnMut(&mut egui::Ui, usize)) -> Option<(usize, usize)> {
    let held: Option<usize> = ui.data(|d| d.get_temp(id));
    let (mut started, mut stopped) = (None, false);
    // iPhone/iPad: ↑ ↓ on every row too (a finger on a small grip inside a scrolling page is fiddly)
    let mut nudged: Option<(usize, usize)> = None;
    let mut rects = Vec::with_capacity(n);
    for i in 0..n {
        let r = ui
            .horizontal(|ui| {
                let (g, resp) = ui.allocate_exact_size(vec2(18.0, 22.0), Sense::drag());
                let c = if resp.hovered() || held == Some(i) { th().text } else { th().faint };
                for dy in [-4.0, 0.0, 4.0] {
                    ui.painter().line_segment([g.center() + vec2(-5.0, dy), g.center() + vec2(5.0, dy)], Stroke::new(1.5, c));
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if resp.drag_started() {
                    started = Some(i);
                }
                stopped |= resp.drag_stopped();
                row(ui, i);
                if cfg!(target_os = "ios") {
                    if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                        nudged = Some((i, i - 1));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("↓")).clicked() {
                        nudged = Some((i, i + 1));
                    }
                }
            })
            .response
            .rect;
        rects.push(r);
    }
    if nudged.is_some() {
        return nudged;
    }
    if let Some(i) = started {
        ui.data_mut(|d| d.insert_temp(id, i));
    }
    let from = started.or(held)?;
    // where the pointer is — or was: a lifted finger ends the touch (no position) in the same frame as the release
    let y_id = id.with("y");
    let y = match ui.ctx().pointer_latest_pos() {
        Some(p) => {
            ui.data_mut(|d| d.insert_temp(y_id, p.y));
            p.y
        }
        None => ui.data(|d| d.get_temp::<f32>(y_id)).unwrap_or(rects[from].center().y),
    };
    // slot = the gap the row would drop into (0 = above the first row, n = below the last)
    let slot = rects.iter().position(|r| y < r.center().y).unwrap_or(n);
    let line_y = if slot < n { rects[slot].top() - 2.0 } else { rects[n - 1].bottom() + 2.0 };
    let x = rects[from].x_range();
    ui.painter().rect_stroke(rects[from].expand(2.0), 5.0, Stroke::new(1.0, th().accent), egui::StrokeKind::Outside);
    ui.painter().line_segment([pos2(x.min, line_y), pos2(x.max, line_y)], Stroke::new(2.5, th().accent));
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    if stopped || !ui.input(|i| i.pointer.any_down()) {
        ui.data_mut(|d| {
            d.remove::<usize>(id);
            d.remove::<f32>(y_id);
        });
        let to = if slot > from { slot - 1 } else { slot };
        return (to != from).then_some((from, to));
    }
    None
}

/// A section's ⋯ menu: move it within the inspector (the order is saved in Settings).
fn sec_menu(ui: &mut egui::Ui, key: &str, op: &mut Option<(String, SecMove)>) {
    // three painted dots (the "⋯" glyph isn't in the UI font)
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 18.0), Sense::click());
    let c = if resp.hovered() { th().text } else { th().faint };
    if resp.hovered() {
        ui.painter().rect_filled(r, 5.0, th().pill);
    }
    for dx in [-5.0, 0.0, 5.0] {
        ui.painter().circle_filled(r.center() + vec2(dx, 0.0), 1.6, c);
    }
    let resp = resp.on_hover_text("Move this section");
    egui::Popup::menu(&resp).show(|ui| {
        for (label, m) in [("Move Up", SecMove::Up), ("Move Down", SecMove::Down), ("Move to Top", SecMove::Top), ("Move to Bottom", SecMove::Bottom)] {
            if ui.button(label).clicked() {
                *op = Some((key.to_string(), m));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Rearrange Sections…").clicked() {
            *op = Some((key.to_string(), SecMove::Arrange));
            ui.close();
        }
    });
}

/// Section caption with its ⋯ menu; integrations (Linear, later GitHub…) pass a logo drawn before the name.
fn sec_caption(ui: &mut egui::Ui, title: &str, key: &str, logo: Option<fn(&egui::Painter, Pos2, f32)>, op: &mut Option<(String, SecMove)>) {
    ui.horizontal(|ui| {
        if let Some(draw) = logo {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            draw(ui.painter(), r.center(), 8.0);
        }
        caption(ui, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| sec_menu(ui, key, op));
    });
}
fn field_text(ui: &mut egui::Ui, s: &str, w: f32, cal: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
    ui.painter().rect_filled(r, 6.0, th().field);
    ui.painter().text(r.left_center() + vec2(9.0, 0.0), Align2::LEFT_CENTER, s, FontId::proportional(14.0), if s == "None" { th().dim } else { th().text });
    if cal {
        cal_glyph(ui.painter(), r.right_center() - vec2(14.0, 0.0), th().dim);
    }
}

/// Segmented control; returns the clicked cell.
fn segmented(ui: &mut egui::Ui, labels: &[&str], w: f32, selected: Option<usize>, highlight: bool) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 7.0, th().pill);
    let n = labels.len() as f32;
    let cw = rect.width() / n;
    let mut out = None;
    for (i, l) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(rect.min + vec2(i as f32 * cw, 0.0), vec2(cw, 28.0));
        let resp = ui.interact(cell, ui.id().with(("seg", l, i, rect.min.y as i32)), Sense::click());
        let on = highlight && selected == Some(i);
        if on {
            p.rect_filled(cell, 7.0, th().accent);
        } else if resp.hovered() {
            p.rect_filled(cell.shrink(2.0), 5.0, th().pill_hover);
        }
        if i > 0 && !on && selected != Some(i - 1) {
            p.line_segment([cell.left_top() + vec2(0.0, 7.0), cell.left_bottom() - vec2(0.0, 7.0)], Stroke::new(1.0, mix(th().pill, th().text, 0.2)));
        }
        let c = if on { Color32::WHITE } else { th().text };
        match *l {
            "▶" => {
                p.circle_stroke(cell.center(), 7.5, Stroke::new(1.4, c));
                p.add(Shape::convex_polygon(vec![cell.center() + vec2(-2.5, -4.0), cell.center() + vec2(4.0, 0.0), cell.center() + vec2(-2.5, 4.0)], c, Stroke::NONE));
            }
            "✔" => {
                p.circle_stroke(cell.center(), 7.5, Stroke::new(1.4, c));
                check(p, cell.center(), 3.5, Stroke::new(1.5, c));
            }
            "–" => {
                p.circle_stroke(cell.center(), 7.5, Stroke::new(1.4, c));
                p.line_segment([cell.center() - vec2(3.5, 0.0), cell.center() + vec2(3.5, 0.0)], Stroke::new(1.5, c));
            }
            "Today" => {
                // sunrise glyph for "today"
                let b = cell.center() + vec2(0.0, 4.0);
                p.line_segment([b - vec2(8.0, 0.0), b + vec2(8.0, 0.0)], Stroke::new(1.3, c));
                p.add(Shape::line(
                    (0..=10).map(|k| {
                        let a = std::f32::consts::PI * (k as f32 / 10.0);
                        b + vec2(-4.5 * a.cos(), -4.5 * a.sin())
                    }).collect(),
                    Stroke::new(1.3, c),
                ));
                for k in 0..5 {
                    let a = std::f32::consts::PI * (k as f32 / 4.0);
                    let dir = vec2(-a.cos(), -a.sin());
                    p.line_segment([b + dir * 6.5, b + dir * 8.5], Stroke::new(1.2, c));
                }
            }
            // tag location: Notification (none / arriving / leaving) and Distance (small / medium / large)
            "notify-none" => {
                p.circle_stroke(cell.center(), 7.5, Stroke::new(1.4, c));
                p.line_segment([cell.center() + vec2(-5.3, -5.3), cell.center() + vec2(5.3, 5.3)], Stroke::new(1.4, c));
            }
            "notify-arrive" | "notify-leave" => {
                // the area as a circle, and an arrow going into it or coming out of it
                let k = cell.center();
                let (ring, from, to) = if *l == "notify-arrive" { (k + vec2(2.5, 0.0), k + vec2(-9.0, 0.0), k + vec2(3.0, 0.0)) } else { (k + vec2(-2.5, 0.0), k + vec2(-3.0, 0.0), k + vec2(9.5, 0.0)) };
                p.circle_stroke(ring, 7.0, Stroke::new(1.4, c));
                p.line_segment([from, to], Stroke::new(1.4, c));
                p.line_segment([to, to + vec2(-3.5, -3.5)], Stroke::new(1.4, c));
                p.line_segment([to, to + vec2(-3.5, 3.5)], Stroke::new(1.4, c));
            }
            "dist0" | "dist1" | "dist2" => {
                let r = [3.5, 5.5, 7.5][(l.as_bytes()[4] - b'0') as usize];
                let n = (r * 2.0) as usize;
                for k in 0..n {
                    let a = std::f32::consts::TAU * k as f32 / n as f32;
                    p.circle_filled(cell.center() + vec2(a.cos(), a.sin()) * r, 0.8, c);
                }
                p.circle_filled(cell.center(), 1.4, c);
            }
            "×" => {
                let r = Rect::from_center_size(cell.center(), vec2(11.0, 11.0));
                p.rect_stroke(r, 2.0, Stroke::new(1.0, th().dim), StrokeKind::Middle);
                p.line_segment([r.min + vec2(3.0, 3.0), r.max - vec2(3.0, 3.0)], Stroke::new(1.0, th().dim));
                p.line_segment([r.left_bottom() + vec2(3.0, -3.0), r.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.0, th().dim));
            }
            _ => {
                p.text(cell.center(), Align2::CENTER_CENTER, *l, FontId::proportional(14.0), c);
            }
        }
        if resp.clicked() {
            out = Some(i);
        }
    }
    out
}

fn check(p: &egui::Painter, c: Pos2, s: f32, stroke: Stroke) {
    p.add(Shape::line(vec![c + vec2(-s, 0.0), c + vec2(-s * 0.3, s * 0.8), c + vec2(s, -s * 0.8)], stroke));
}

fn status_circle(p: &egui::Painter, c: Pos2, ring: Color32, st: Status, repeat: bool, flagged: bool) {
    status_circle_anim(p, c, ring, st, repeat, flagged, if matches!(st, Status::Completed | Status::Dropped) { 1.0 } else { 0.0 });
}

/// `done` runs 0→1 as an item is completed: a soft fill grows in and the mark scales up.
fn status_circle_anim(p: &egui::Painter, c: Pos2, ring: Color32, st: Status, repeat: bool, flagged: bool, done: f32) {
    if done > 0.0 && done < 1.0 {
        p.circle_filled(c, STATUS_CIRCLE_R * (0.4 + 0.6 * done), ring.gamma_multiply(0.35 * (1.0 - done)));
    }
    p.circle_stroke(c, STATUS_CIRCLE_R, Stroke::new(1.7, ring));
    match st {
        Status::Completed => check(p, c, 4.0 * (0.5 + 0.5 * done), Stroke::new(1.7, ring)),
        Status::Dropped => {
            p.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], Stroke::new(1.7, ring));
        }
        _ if repeat => {
            for k in -1..=1 {
                p.circle_filled(c + vec2(k as f32 * 3.2, 0.0), 1.1, ring);
            }
        }
        _ => {}
    }
    if flagged {
        // small flag notch on the circle's top-left (reference: due soon + flagged rows in the person's Forecast picture)
        let o = c + vec2(-STATUS_CIRCLE_R * 0.72, -STATUS_CIRCLE_R * 0.72);
        p.circle_filled(o, 4.2, th().bg);
        p.line_segment([o + vec2(-2.0, -3.2), o + vec2(-2.0, 3.6)], Stroke::new(1.3, th().flag));
        p.add(Shape::convex_polygon(vec![o + vec2(-2.0, -3.2), o + vec2(3.2, -1.8), o + vec2(-2.0, -0.2)], th().flag, Stroke::NONE));
    }
}

fn chevron(p: &egui::Painter, c: Pos2, open: bool, col: Color32) {
    let s = Stroke::new(1.5, col);
    if open {
        p.add(Shape::line(vec![c + vec2(-5.0, -2.5), c + vec2(0.0, 2.5), c + vec2(5.0, -2.5)], s));
    } else {
        p.add(Shape::line(vec![c + vec2(-2.5, -5.0), c + vec2(2.5, 0.0), c + vec2(-2.5, 5.0)], s));
    }
}

/// 0 = not flagged, 1 = inherited (orange outline), 2 = flagged (filled).
fn draw_flag(p: &egui::Painter, c: Pos2, state: u8) {
    let on = state == 2;
    let col = if state > 0 { th().flag } else { mix(th().dim, th().faint, 0.5) };
    p.line_segment([c + vec2(-5.0, -7.0), c + vec2(-5.0, 7.0)], Stroke::new(1.3, col));
    let pts = vec![c + vec2(-5.0, -7.0), c + vec2(1.0, -5.5), c + vec2(6.0, -7.0), c + vec2(6.0, 1.0), c + vec2(1.0, 2.5), c + vec2(-5.0, 1.0)];
    if on {
        p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
    } else {
        p.add(Shape::closed_line(pts, Stroke::new(1.2, col)));
    }
}

fn cal_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(11.0, 10.0));
    p.rect_stroke(r, 1.5, Stroke::new(1.0, col), StrokeKind::Middle);
    p.line_segment([r.left_top() + vec2(0.0, 3.0), r.right_top() + vec2(0.0, 3.0)], Stroke::new(1.0, col));
    for k in 0..3 {
        p.circle_filled(pos2(r.left() + 3.0 + k as f32 * 2.5, r.top() + 6.5), 0.7, col);
    }
}

fn project_glyph(p: &egui::Painter, c: Pos2, scale: f32, col: Color32) {
    for (dx, dy, r) in [(-6.0, -6.0, 3.0), (6.0, -6.0, 2.6), (0.0, 0.0, 3.6), (-6.0, 6.0, 2.6), (6.0, 6.0, 3.0)] {
        p.circle_filled(c + vec2(dx, dy) * scale, r * scale, col);
    }
}

fn draw_gear(p: &egui::Painter, c: Pos2, col: Color32) {
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU;
        let dir = vec2(a.cos(), a.sin());
        p.line_segment([c + dir * 5.0, c + dir * 8.0], Stroke::new(2.4, col));
    }
    p.circle_filled(c, 6.0, col);
    p.circle_filled(c, 2.4, th().bg);
}

/// The Review Guru: a slacker dude sitting in a calm meditation pose (shades on, zen aura overhead), drawn in the
/// house glyph style. ~24 px tall, centred on `c`.
fn draw_guru(p: &egui::Painter, c: Pos2, col: Color32) {
    // the mat he sits on
    p.line_segment([c + vec2(-12.0, 10.0), c + vec2(12.0, 10.0)], Stroke::new(1.3, mix(col, th().bg, 0.45)));
    // seated meditation silhouette: narrow shoulders over wide crossed legs (hands rest on the knees)
    p.add(Shape::convex_polygon(
        vec![c + vec2(0.0, -5.0), c + vec2(7.0, 1.0), c + vec2(11.0, 9.0), c + vec2(-11.0, 9.0), c + vec2(-7.0, 1.0)],
        col,
        Stroke::NONE,
    ));
    p.circle_filled(c + vec2(-10.5, 8.5), 2.0, col);
    p.circle_filled(c + vec2(10.5, 8.5), 2.0, col);
    // head, sat just above the shoulders
    p.circle_filled(c + vec2(0.0, -9.0), 4.0, col);
    // the Dude's shades: a short bar at the eye line
    p.line_segment([c + vec2(-2.6, -9.0), c + vec2(2.6, -9.0)], Stroke::new(1.4, th().bg));
}

enum ColIcon {
    Folder,
    Project,
    Tag,
}

fn column_item(ui: &mut egui::Ui, label: &str, on: bool, indent: f32, icon: ColIcon, col: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 5.0, th().sel_row);
    } else if resp.hovered() {
        p.rect_filled(rect, 5.0, mix(th().column, th().pill, 0.5));
    }
    let ic = pos2(rect.left() + 18.0 + indent, rect.center().y);
    match icon {
        ColIcon::Folder => {
            p.rect_stroke(Rect::from_center_size(ic + vec2(0.0, 1.0), vec2(14.0, 10.0)), 2.0, Stroke::new(1.3, col), StrokeKind::Middle);
            p.line_segment([ic + vec2(-7.0, -4.0), ic + vec2(-2.0, -4.0)], Stroke::new(1.3, col));
        }
        ColIcon::Project => project_glyph(p, ic, 0.5, col),
        ColIcon::Tag => {
            let pts = vec![ic + vec2(-6.0, -1.0), ic + vec2(0.0, -7.0), ic + vec2(6.0, -7.0), ic + vec2(6.0, -1.0), ic + vec2(0.0, 5.0)];
            p.add(Shape::closed_line(pts, Stroke::new(1.3, col)));
            p.circle_filled(ic + vec2(3.0, -4.0), 1.0, col);
        }
    }
    p.text(pos2(rect.left() + 32.0 + indent, rect.center().y), Align2::LEFT_CENTER, label, if matches!(icon, ColIcon::Folder) { bold(14.0) } else { FontId::proportional(14.0) }, th().text);
    ui.add_space(8.0);
    resp
}

fn draw_empty(p: &egui::Painter, body: Rect, v: View, pcol: Color32) {
    let c = body.center();
    let col = mix(th().bg, th().text, 0.08);
    match v {
        View::Flagged => {
            p.rect_filled(Rect::from_center_size(c + vec2(-90.0, 0.0), vec2(18.0, 220.0)), 9.0, col);
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let x = i as f32 / 20.0;
                    c + vec2(-70.0 + 160.0 * x, -85.0 + 12.0 * (x * std::f32::consts::TAU).sin())
                })
                .chain((0..=20).rev().map(|i| {
                    let x = i as f32 / 20.0;
                    c + vec2(-70.0 + 160.0 * x, 35.0 + 12.0 * (x * std::f32::consts::TAU).sin())
                }))
                .collect();
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        }
        View::Nearby => {
            p.text(c, Align2::CENTER_CENTER, "No available nearby items", FontId::proportional(15.0), th().dim);
        }
        _ => {
            draw_icon(p, c, v, mix(pcol, th().bg, 0.75));
        }
    }
}

/// Perspective icons, drawn as shapes in the perspective's theme colour.
fn draw_icon(p: &egui::Painter, c: Pos2, v: View, col: Color32) {
    let deep = mix(col, th().rail, 0.6);
    let light = mix(col, Color32::WHITE, 0.25);
    match v {
        View::Inbox => {
            let r = Rect::from_center_size(c, vec2(26.0, 22.0));
            p.rect_filled(r, 5.0, col);
            p.rect_filled(Rect::from_min_max(r.min + vec2(4.0, 3.0), pos2(r.max.x - 4.0, r.center().y + 2.0)), 2.0, th().rail);
            p.rect_filled(Rect::from_center_size(pos2(c.x, r.center().y + 2.0), vec2(9.0, 4.0)), 2.0, col);
        }
        View::Forecast => {
            let r = Rect::from_center_size(c, vec2(26.0, 24.0));
            p.rect_filled(r, 5.0, col);
            p.rect_filled(Rect::from_min_max(r.min + vec2(3.0, 6.0), r.max - vec2(3.0, 3.0)), 3.0, deep);
            for (i, j) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)] {
                p.circle_filled(pos2(r.left() + 8.0 + i as f32 * 5.0, r.top() + 12.0 + j as f32 * 5.0), 1.5, light);
            }
        }
        View::Review => {
            let r = Rect::from_center_size(c - vec2(3.0, 0.0), vec2(22.0, 20.0));
            p.rect_stroke(r, CornerRadius { nw: 3, ne: 3, sw: 9, se: 9 }, Stroke::new(2.0, col), StrokeKind::Middle);
            p.rect_filled(Rect::from_min_max(pos2(r.left() + 2.0, r.center().y), r.max - vec2(2.0, 2.0)), CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 }, col);
            p.circle_stroke(pos2(r.right() + 3.0, r.top() + 7.0), 4.0, Stroke::new(2.0, col));
        }
        View::Flagged => {
            p.line_segment([c + vec2(-10.0, -12.0), c + vec2(-10.0, 13.0)], Stroke::new(2.5, mix(col, Color32::BLACK, 0.2)));
            p.add(Shape::convex_polygon(
                vec![c + vec2(-8.0, -10.0), c + vec2(1.0, -8.0), c + vec2(11.0, -10.0), c + vec2(11.0, 3.0), c + vec2(1.0, 4.0), c + vec2(-8.0, 3.0)],
                col,
                Stroke::NONE,
            ));
        }
        View::Projects => {
            project_glyph(p, c, 1.15, mix(col, th().rail, 0.25));
            p.circle_filled(c, 4.0, light);
        }
        View::Tags => {
            let pts = vec![c + vec2(-13.0, 3.0), c + vec2(1.0, -11.0), c + vec2(12.0, -11.0), c + vec2(12.0, -1.0), c + vec2(-2.0, 13.0)];
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
            p.circle_filled(c + vec2(6.0, -5.0), 2.2, th().rail);
        }
        View::Nearby => {
            p.circle_filled(c - vec2(0.0, 4.0), 9.0, col);
            p.add(Shape::convex_polygon(vec![c + vec2(-8.0, 0.0), c + vec2(8.0, 0.0), c + vec2(0.0, 14.0)], col, Stroke::NONE));
            p.circle_filled(c - vec2(0.0, 4.0), 3.0, deep);
        }
        View::Completed => {
            p.circle_filled(c, 12.0, deep);
            check(p, c, 6.0, Stroke::new(2.6, light));
        }
        View::Changed => {
            p.add(Shape::line(
                (0..=30)
                    .map(|k| {
                        let a = -1.2 + (k as f32 / 30.0) * 5.4;
                        c + vec2(11.0 * a.cos(), 11.0 * a.sin())
                    })
                    .collect(),
                Stroke::new(2.0, col),
            ));
            p.line_segment([c, c + vec2(0.0, -6.0)], Stroke::new(2.0, col));
            p.line_segment([c, c + vec2(-4.0, 1.0)], Stroke::new(2.0, col));
        }
        View::Stats => {
            for (i, hgt) in [10.0f32, 18.0, 13.0, 22.0].iter().enumerate() {
                let x = c.x - 10.5 + i as f32 * 7.0;
                p.rect_filled(Rect::from_min_max(pos2(x - 2.6, c.y + 11.0 - hgt), pos2(x + 2.6, c.y + 11.0)), CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, if i == 3 { light } else { col });
            }
            p.line_segment([c + vec2(-14.0, 11.5), c + vec2(14.0, 11.5)], Stroke::new(1.5, deep));
        }
        View::Agents => {
            // a speech bubble with a spark: an agent thread
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, -2.0), vec2(24.0, 17.0)), 6.0, col);
            p.add(Shape::convex_polygon(vec![c + vec2(-6.0, 5.0), c + vec2(-1.0, 5.0), c + vec2(-7.0, 11.0)], col, Stroke::NONE));
            for dx in [-6.0, 0.0, 6.0] {
                p.circle_filled(c + vec2(dx, -2.0), 1.8, light);
            }
        }
        View::Custom(_) => {
            // a gear-less "lens": ring with a dot, in the perspective colour
            p.circle_stroke(c, 10.5, Stroke::new(2.4, col));
            p.circle_filled(c, 4.5, light);
        }
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;

    /// Drives `drag_list` with real pointer events in a headless egui: press a grip, move down, release.
    fn drag(from_y: f32, to_y: f32) -> Option<(usize, usize)> {
        let ctx = egui::Context::default();
        let mut out = None;
        let grip_x = 17.0;
        let steps: Vec<Vec<egui::Event>> = vec![
            vec![egui::Event::PointerMoved(pos2(grip_x, from_y))],
            vec![egui::Event::PointerButton { pos: pos2(grip_x, from_y), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }],
            vec![egui::Event::PointerMoved(pos2(grip_x, from_y + 12.0))],
            vec![egui::Event::PointerMoved(pos2(grip_x, to_y))],
            vec![egui::Event::PointerMoved(pos2(grip_x, to_y))],
            vec![egui::Event::PointerButton { pos: pos2(grip_x, to_y), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
            vec![],
        ];
        for (n, events) in steps.into_iter().enumerate() {
            let raw = egui::RawInput { events, time: Some(n as f64 * 0.1), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(m) = drag_list(ui, egui::Id::new("t"), 4, |ui, i| {
                        ui.label(format!("row {i}"));
                    }) {
                        out = Some(m);
                    }
                });
            });
        }
        out
    }

    /// A finger: the release frame also ends the pointer (PointerGone), so there's no position left to read.
    #[test]
    fn touch_drag_reorders_even_though_the_finger_lifts() {
        let ctx = egui::Context::default();
        let mut out = None;
        let (x, from_y, to_y) = (17.0, 11.0 + 2.0 * 0.0, 90.0);
        let btn = |y: f32, down| egui::Event::PointerButton { pos: pos2(x, y), button: egui::PointerButton::Primary, pressed: down, modifiers: Default::default() };
        let steps: Vec<Vec<egui::Event>> = vec![
            vec![egui::Event::PointerMoved(pos2(x, from_y))],
            vec![btn(from_y, true)],
            vec![egui::Event::PointerMoved(pos2(x, from_y + 12.0))],
            vec![egui::Event::PointerMoved(pos2(x, to_y))],
            vec![btn(to_y, false), egui::Event::PointerGone],
            vec![],
        ];
        for (n, events) in steps.into_iter().enumerate() {
            let raw = egui::RawInput { events, time: Some(n as f64 * 0.1), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(m) = drag_list(ui, egui::Id::new("t"), 4, |ui, i| {
                        ui.label(format!("row {i}"));
                    }) {
                        out = Some(m);
                    }
                });
            });
        }
        assert_eq!(out.map(|m| m.0), Some(0), "the first row moved: {out:?}");
        assert!(out.is_some_and(|m| m.1 > 0));
    }

    /// Runs the Projects sidebar headlessly through a list of input frames; returns the drop of the last one that had any.
    fn run_projects(ctx: &egui::Context, ps: &[Project], frames: Vec<Vec<egui::Event>>) -> (Option<(Id, String)>, Vec<Act>) {
        let (mut drop, mut acts) = (None, vec![]);
        for (n, events) in frames.into_iter().enumerate() {
            let raw = egui::RawInput { events, time: Some(n as f64 * 0.1), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 600.0))), ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut d = None;
                    project_list(ui, ps.iter().collect(), &[], &[], Filter::All, &mut acts, &mut d);
                    if d.is_some() {
                        drop = d;
                    }
                });
            });
        }
        (drop, acts)
    }

    fn press(p: Pos2, down: bool) -> egui::Event {
        egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: down, modifiers: Default::default() }
    }

    #[test]
    fn projects_drag_into_and_out_of_folders() {
        let mut s = Store::default();
        let loose = s.add_project("Loose", ProjectKind::Parallel);
        let filed = s.add_project("Filed", ProjectKind::Parallel);
        s.project_mut(filed).unwrap().folder = "Work".into();
        let ps = s.projects.clone();
        let ctx = egui::Context::default();
        setup_fonts(&ctx); // the app's font families (the list uses the "bold" one)
        run_projects(&ctx, &ps, vec![vec![]]); // lay out once
        // rows are 22 pt tall plus item spacing, inside the panel margin: Loose, the Work folder, then Filed
        let pitch = 22.0 + ctx.style().spacing.item_spacing.y;
        let row = |i: f32| pos2(60.0, 8.0 + 11.0 + i * pitch);
        let (a, b) = (row(0.0), row(2.0));
        // drag "Loose" onto "Filed" (inside Work): it moves into Work
        let (drop, _) = run_projects(&ctx, &ps, vec![vec![egui::Event::PointerMoved(a)], vec![press(a, true)], vec![egui::Event::PointerMoved(a + vec2(0.0, 12.0))], vec![egui::Event::PointerMoved(b)], vec![egui::Event::PointerMoved(b)], vec![press(b, false)], vec![]]);
        assert_eq!(drop, Some((loose, "Work".to_string())));
        // drag "Filed" onto the "No Folder" box that appears below the list: it leaves the folder
        let _ = run_projects(&ctx, &ps, vec![vec![egui::Event::PointerMoved(b)], vec![press(b, true)], vec![egui::Event::PointerMoved(b + vec2(0.0, 12.0))]]);
        let below = ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("no-folder-box"))).expect("No Folder box shows while dragging").center();
        let (drop, _) = run_projects(&ctx, &ps, vec![vec![egui::Event::PointerMoved(below)], vec![egui::Event::PointerMoved(below)], vec![press(below, false)], vec![]]);
        assert_eq!(drop, Some((filed, String::new())));
        // a plain click still selects the project and moves nothing
        let (drop, acts) = run_projects(&ctx, &ps, vec![vec![egui::Event::PointerMoved(a)], vec![press(a, true)], vec![press(a, false)], vec![]]);
        assert_eq!(drop, None);
        assert!(acts.iter().any(|x| matches!(x, Act::Select(Sel::Project(id)) if *id == loose)));
    }

    /// Right-click and Control-click both open an action's shortcut menu.
    #[test]
    fn row_shortcut_menu_opens() {
        let mut st = Store::default();
        let id = st.add_task("Nest something", None, None);
        let t = st.task(id).unwrap().clone();
        for (button, mods) in [(egui::PointerButton::Secondary, egui::Modifiers::NONE), (egui::PointerButton::Primary, egui::Modifiers::CTRL)] {
            let ctx = egui::Context::default();
            setup_fonts(&ctx);
            let p = pos2(100.0, 20.0);
            let frames = vec![
                vec![egui::Event::PointerMoved(p)],
                vec![egui::Event::PointerButton { pos: p, button, pressed: true, modifiers: mods }],
                vec![egui::Event::PointerButton { pos: p, button, pressed: false, modifiers: mods }],
                vec![],
            ];
            let mut opened = false;
            for (n, events) in frames.into_iter().enumerate() {
                let raw = egui::RawInput { events, modifiers: mods, time: Some(n as f64 * 0.1), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() };
                let _ = ctx.run(raw, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (_, resp) = ui.allocate_exact_size(vec2(300.0, 40.0), Sense::click());
                        let ctrl_click = resp.clicked() && ui.input(|i| i.modifiers.ctrl && !i.modifiers.command);
                        let mut acts = vec![];
                        task_menu(&resp, id, &t, false, ctrl_click, false, &mut acts);
                    });
                });
                opened |= egui::Popup::is_any_open(&ctx);
            }
            assert!(opened, "{button:?} {mods:?}");
        }
    }

    #[test]
    fn copies_a_selection_as_a_list() {
        let mut st = Store::default();
        let a = st.add_task("Multi select not working", None, None);
        let b = st.add_task("Child step", None, Some(a));
        let c = st.add_task("Notification for planned item", None, None);
        st.task_mut(a).unwrap().note = "on iPhone\n\nand iPad".into();
        st.task_mut(c).unwrap().flagged = true;
        st.task_mut(c).unwrap().duration_min = Some(15);
        // a picked child of a picked parent comes once, under its parent
        assert_eq!(copy_list(&st, &[a, b, c], false), "- Multi select not working\n\ton iPhone\n\tand iPad\n\t- Child step\n- Notification for planned item\n");
        let ctx = copy_list(&st, &[c], true);
        assert_eq!(ctx, "- Notification for planned item @estimate(15m) @flagged\n");
    }

    #[test]
    fn attaching_goes_on_every_target_once() {
        let mut st = Store::default();
        let (a, b) = (st.add_task("A", None, None), st.add_task("B", None, None));
        let now = chrono::Local::now().naive_local();
        let paths = vec!["/Users/me/Projects".to_string()];
        assert!(attach_to(&mut st, &[Sel::Task(a), Sel::Task(b)], &paths, now));
        assert_eq!(st.task(a).unwrap().attachments, paths);
        assert_eq!(st.task(b).unwrap().attachments, paths);
        assert!(!attach_to(&mut st, &[Sel::Task(a)], &paths, now), "already attached");
        assert!(!attach_to(&mut st, &[Sel::Task(a)], &[], now), "picker cancelled");
    }

    /// The shortcut menu offers Attach File… and Attach Folder….
    #[test]
    fn row_menu_offers_attach() {
        let mut st = Store::default();
        let id = st.add_task("Nest something", None, None);
        let t = st.task(id).unwrap().clone();
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        let mut texts = vec![];
        for n in 0..3 {
            let raw = egui::RawInput { time: Some(n as f64 * 0.1), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))), ..Default::default() };
            let out = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (_, resp) = ui.allocate_exact_size(vec2(300.0, 40.0), Sense::click());
                    task_menu(&resp, id, &t, false, n == 0, false, &mut vec![]);
                });
            });
            for s in out.shapes {
                if let egui::epaint::Shape::Text(ts) = s.shape {
                    texts.push(ts.galley.text().to_string());
                }
            }
        }
        for want in ["Attach File\u{2026}", "Attach Folder\u{2026}"] {
            assert!(texts.iter().any(|t| t == want), "{want} missing from {texts:?}");
        }
    }

    #[test]
    fn dragging_a_grip_reorders() {
        // rows are ~25 pt apart starting near y = 19
        assert_eq!(drag(19.0, 95.0), Some((0, 3))); // first row dropped below the last
        assert_eq!(drag(70.0, 10.0), Some((2, 0))); // third row dropped above the first
        assert_eq!(drag(19.0, 22.0), None); // dropped where it was
    }
}

#[cfg(test)]
mod inbox_tests {
    use super::*;

    #[test]
    fn processed_inbox_items_wait_for_clean_up() {
        let mut kept = HashSet::new();
        let prev: HashSet<Id> = [1, 2, 3].into();
        let now: HashSet<Id> = [1, 3].into(); // 2 got a project
        track_inbox(&prev, &now, |id| id == 2, &mut kept);
        assert_eq!(kept, [2].into());
        track_inbox(&now, &now, |_| false, &mut kept); // its project was taken away again
        assert!(kept.is_empty());
    }
}
