//! The app side of the menu bar and of database files: New / Open / Save As / Export, Sync Now, git version history.
use crate::*;
use eframe::egui;

/// Quiet time after a save before it's committed, so a burst of edits becomes one version.
const GIT_COMMIT_DELAY: f64 = 30.0;

pub(crate) struct DbUi {
    pub bar: Option<menu::Bar>,
    pub rx: Option<std::sync::mpsc::Receiver<String>>,
    /// Menu items handled by `update` (database, view, help).
    pub queue: Vec<String>,
    pub run: Vec<keys::Action>,
    pub git: bool,
    pub git_due: Option<f64>,
    pub recent: Vec<PathBuf>,
    pub history: Option<Vec<dbfile::Version>>,
    pub msg: String,
}

impl DbUi {
    pub fn new(path: &std::path::Path) -> DbUi {
        DbUi { bar: None, rx: None, queue: vec![], run: vec![], git: dbfile::git_tracked(path), git_due: None, recent: dbfile::recent(), history: None, msg: String::new() }
    }
}

/// File dialogs (macOS); on iPhone and iPad there are none yet, so these return nothing.
#[cfg(not(target_os = "ios"))]
fn save_file(title: &str, start: &std::path::Path, name: &str) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title).add_filter("GTDCraft database", &["jsonl", "json", "jsonc"]).set_file_name(name);
    if let Some(dir) = start.parent() {
        d = d.set_directory(dir);
    }
    d.save_file()
}

#[cfg(not(target_os = "ios"))]
fn pick_file(title: &str, start: &std::path::Path) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title).add_filter("GTDCraft database", &["jsonl", "json", "jsonc"]);
    if let Some(dir) = start.parent() {
        d = d.set_directory(dir);
    }
    d.pick_file()
}

#[cfg(target_os = "ios")]
fn save_file(_title: &str, _start: &std::path::Path, _name: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "ios")]
fn pick_file(_title: &str, _start: &std::path::Path) -> Option<PathBuf> {
    None
}

impl Gtd {
    fn menu_shape(&self) -> menu::Shape {
        menu::Shape {
            keymap: keys::bindings(&self.store.settings.keymap),
            perspectives: self.store.settings.perspectives.iter().filter(|p| p.visible).map(|p| (p.key.clone(), p.name.clone())).collect(),
            recent: self.db.recent.clone(),
            git: self.db.git,
            sidebar: self.show_sidebar,
            inspector: self.show_inspector,
            board: self.pcfg(self.view).is_some_and(|p| p.board),
        }
    }

    /// Builds the menu bar once the app is running and rebuilds it when perspectives, shortcuts or state change.
    pub(crate) fn menu_tick(&mut self, ctx: &egui::Context) {
        if self.db.rx.is_none() {
            self.db.rx = Some(menu::listen(ctx));
        }
        let shape = self.menu_shape();
        if self.db.bar.as_ref().is_none_or(|b| b.shape != shape) {
            self.db.bar = Some(menu::build(&shape));
        }
    }

    /// Menu clicks become input before the frame runs: shortcut items replay their key; the rest are queued.
    pub(crate) fn menu_input(&mut self, raw: &mut egui::RawInput) {
        let Some(rx) = &self.db.rx else { return };
        let km = keys::bindings(&self.store.settings.keymap);
        for id in rx.try_iter() {
            match menu::decode(&id, &km) {
                menu::Cmd::Event(e) => raw.events.push(e),
                menu::Cmd::Run(a) => self.db.run.push(a),
                menu::Cmd::Other(s) if !s.is_empty() => self.db.queue.push(s),
                menu::Cmd::Other(_) => {}
            }
        }
    }

    pub(crate) fn menu_commands(&mut self, ctx: &egui::Context, d: &Derived, now: Dt) {
        for a in std::mem::take(&mut self.db.run) {
            self.run(a, d, now);
        }
        for id in std::mem::take(&mut self.db.queue) {
            match id.as_str() {
                "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                "db:new" => {
                    if let Some(p) = save_file("New Database", &self.path, "GTDCraft.jsonl") {
                        // a fresh, empty database that keeps your settings (theme, shortcuts, perspectives, agents)
                        let mut s = Store { settings: self.store.settings.clone(), ..Default::default() };
                        s.settings.here = None;
                        self.switch_db(p, s);
                    }
                }
                "db:open" => {
                    if let Some(p) = pick_file("Open Database", &self.path) {
                        self.open_db(p);
                    }
                }
                "db:save_as" => {
                    let name = self.path.file_stem().map(|s| format!("{} copy.jsonl", s.to_string_lossy())).unwrap_or_default();
                    if let Some(p) = save_file("Save Database As", &self.path, &name) {
                        let s = self.store.clone();
                        self.switch_db(p, s);
                    }
                }
                "db:export" => {
                    let name = self.path.file_stem().map(|s| format!("{}.jsonc", s.to_string_lossy())).unwrap_or_default();
                    if let Some(p) = save_file("Export", &self.path, &name) {
                        self.db.msg = match self.store.save(&p) {
                            Ok(()) => format!("Exported to {}", p.display()),
                            Err(e) => format!("Export failed: {e}"),
                        };
                    }
                }
                "db:sync" => self.sync_now(),
                "db:git" => {
                    if self.db.git {
                        self.db.msg = "Version history stays in the folder's git repository; GTDCraft just stops adding versions.".into();
                        self.db.git = false;
                    } else {
                        self.save();
                        match dbfile::git_init(&self.path) {
                            Ok(()) => {
                                self.db.git = true;
                                self.db.msg = "Version history is on: changes are committed after 30 seconds of quiet.".into();
                            }
                            Err(e) => self.db.msg = format!("Couldn't start version history: {e}"),
                        }
                    }
                }
                "db:history" => self.load_history(),
                "db:reveal" => {
                    let _ = std::process::Command::new("open").arg("-R").arg(&self.path).spawn();
                }
                "view:options" => self.view_opts = !self.view_opts,
                "persp:add" => {
                    let k = self.new_custom_perspective();
                    self.editor = Some(k);
                }
                "help:keys" => {
                    self.show_settings = true;
                    self.settings_tab = 4;
                }
                "help:agent" => {
                    self.show_settings = true;
                    self.settings_tab = 5;
                }
                "help:bug" => self.report_bug(),
                "help:mcp_copy" => {
                    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "gtdcraft".into());
                    ctx.copy_text(format!("{exe} --mcp"));
                }
                s => {
                    if let Some(i) = s.strip_prefix("recent:").and_then(|i| i.parse::<usize>().ok()) {
                        if let Some(p) = self.db.recent.get(i).cloned() {
                            self.open_db(p);
                        }
                    } else if let Some(v) = s.strip_prefix("persp:").and_then(View::from_key) {
                        self.go(v);
                    }
                }
            }
        }
    }

    fn open_db(&mut self, p: PathBuf) {
        match Store::load(&p) {
            Ok(s) => self.switch_db(p, s),
            Err(e) => self.db.msg = format!("Couldn't open {}: {e}", p.display()),
        }
    }

    /// Makes `p` the open database (saving and committing the current one first). Undo history starts fresh.
    fn switch_db(&mut self, p: PathBuf, mut s: Store) {
        s.id_base = self.store.id_base;
        if self.dirty_at.is_some() {
            self.save();
        }
        self.commit_now("Autosave before switching databases");
        if let Err(e) = s.save(&p) {
            self.db.msg = format!("Couldn't write {}: {e}", p.display());
            return;
        }
        self.path = p;
        self.store = s;
        self.adopt_prints();
        self.persist = true;
        self.mtime = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        self.undo.clear();
        self.redo.clear();
        self.snapshot = Some(self.store.clone());
        self.sel = None;
        self.editing = None;
        self.field_open = None;
        self.db.git = dbfile::git_tracked(&self.path);
        self.db.git_due = None;
        self.db.history = None;
        dbfile::set_current(&self.path);
        self.db.recent = dbfile::recent();
        self.db.msg = format!("Opened {}", self.path.display());
        self.switch(View::Inbox);
    }

    /// Called after every save: schedule a commit.
    pub(crate) fn after_save(&mut self) {
        if self.db.git && self.db.git_due.is_none() {
            self.db.git_due = Some(self.time + GIT_COMMIT_DELAY);
        }
    }

    pub(crate) fn git_tick(&mut self, ctx: &egui::Context) {
        let Some(due) = self.db.git_due else { return };
        if self.time >= due {
            self.db.git_due = None;
            let (p, msg) = (self.path.clone(), self.version_message("Autosave"));
            std::thread::spawn(move || {
                if let Err(e) = dbfile::git_commit(&p, &msg) {
                    eprintln!("gtdcraft: git commit failed: {e}");
                }
            });
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - self.time));
        }
    }

    fn version_message(&self, what: &str) -> String {
        let open = self.store.tasks.iter().filter(|t| t.completed.is_none() && t.dropped.is_none()).count();
        // Conventional Commit form, so repositories with a commit-message hook accept it
        format!("chore(gtdcraft): {} - {open} open actions, {} projects, {} tags", what.to_lowercase(), self.store.projects.len(), self.store.tags.len())
    }

    /// Commits right away (quit, switching databases, Sync Now).
    pub(crate) fn commit_now(&mut self, what: &str) {
        if !self.db.git || !self.persist {
            return;
        }
        self.db.git_due = None;
        if let Err(e) = dbfile::git_commit(&self.path, &self.version_message(what)) {
            self.db.msg = format!("git commit failed: {e}");
        }
    }

    /// Sync Now: save, commit, and push when the repository has a remote.
    fn sync_now(&mut self) {
        self.save();
        // the sync server first (other devices), then git history
        if !self.store.settings.sync_url.trim().is_empty() {
            self.sync_due = Some(0.0);
        }
        if !self.db.git {
            self.db.msg = if self.store.settings.sync_url.trim().is_empty() { "Saved. Set a sync server in Settings › General, or turn on File › Keep Version History.".into() } else { "Syncing…".into() };
            return;
        }
        self.commit_now("Sync");
        self.db.msg = match dbfile::git_push(&self.path) {
            Ok(true) => "Saved, committed and pushed.".into(),
            Ok(false) => "Saved and committed (no git remote to push to).".into(),
            Err(e) => format!("Committed, but push failed: {e}"),
        };
    }

    fn load_history(&mut self) {
        if !self.db.git {
            self.db.msg = "Turn on File › Keep Version History first.".into();
            self.db.history = Some(vec![]);
            return;
        }
        self.commit_now("Autosave");
        match dbfile::git_log(&self.path, 200) {
            Ok(v) => self.db.history = Some(v),
            Err(e) => self.db.msg = format!("Couldn't read history: {e}"),
        }
    }

    /// Version History window, and the database status line.
    pub(crate) fn db_windows(&mut self, ctx: &egui::Context) {
        if let Some(list) = self.db.history.clone() {
            let mut open = true;
            let mut restore: Option<String> = None;
            egui::Window::new("Version History").open(&mut open).default_size([460.0, 420.0]).collapsible(false).show(ctx, |ui| {
                ui.label(egui::RichText::new(self.path.display().to_string()).color(th().dim).size(12.0));
                if !self.db.msg.is_empty() {
                    ui.label(egui::RichText::new(&self.db.msg).size(12.0));
                }
                ui.separator();
                if list.is_empty() {
                    ui.label("No versions yet.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, v) in list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(&v.when);
                                ui.label(egui::RichText::new(v.message.strip_prefix("chore(gtdcraft): ").unwrap_or(&v.message)).color(th().dim).size(12.0));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if i == 0 {
                                    ui.label(egui::RichText::new("Current").color(th().dim));
                                } else if ui.button("Restore").on_hover_text("Replace what's open with this version (you can undo it)").clicked() {
                                    restore = Some(v.hash.clone());
                                }
                            });
                        });
                        ui.separator();
                    }
                });
            });
            if let Some(h) = restore {
                match dbfile::git_show(&self.path, &h) {
                    Ok(s) => {
                        self.store = s;
                        self.adopt_prints();
                        self.touch(); // undoable, and saved like any edit
                        self.db.msg = format!("Restored the version from {}.", list.iter().find(|v| v.hash == h).map(|v| v.when.as_str()).unwrap_or("?"));
                        self.db.history = None;
                        return;
                    }
                    Err(e) => self.db.msg = format!("Couldn't restore: {e}"),
                }
            }
            if !open {
                self.db.history = None;
            }
        }
        if !self.db.msg.is_empty() && self.db.history.is_none() {
            let mut close = false;
            egui::Area::new(egui::Id::new("db-msg")).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -18.0)).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(&self.db.msg);
                        close = ui.small_button("×").clicked();
                    });
                });
            });
            if close {
                self.db.msg.clear();
            }
        }
    }
}
