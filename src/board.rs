//! Kanban board view, the eye-button view options popover, and Quick Open (⌘O).
use crate::*;
use eframe::egui::{self, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};
use std::sync::Arc;

/// What a column stands for; dropping a card on a column changes the task to match.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ColKey {
    Project(Option<Id>),
    Tag(Option<Id>),
    Day(NaiveDate),
    Todo,
    Waiting,
    Done,
    Fixed,
    /// A stage of a status set (tag group): (set, stage); None = no stage yet.
    Stage(Id, Option<Id>),
}

pub(crate) struct Column {
    title: String,
    key: ColKey,
    color: Color32,
    tasks: Vec<Id>,
}

impl Gtd {
    pub(crate) fn board_supported(&self) -> bool {
        !matches!(self.view, View::Review | View::Completed | View::Changed | View::Stats)
    }

    pub(crate) fn board_on(&self) -> bool {
        self.board_supported() && self.pcfg(self.view).is_some_and(|p| p.board)
    }

    /// The board's columns here: the selected project's own choice in Projects, else the perspective's (None = usual).
    pub(crate) fn column_mode(&self) -> Option<String> {
        if let (View::Projects, Filter::Project(p)) = (self.view, self.filter) {
            if let Some(c) = self.store.project(p).and_then(|p| p.columns.clone()) {
                return Some(c);
            }
        }
        self.pcfg(self.view).and_then(|p| p.columns.clone())
    }

    fn set_column_mode(&mut self, m: Option<String>) {
        if let (View::Projects, Filter::Project(p)) = (self.view, self.filter) {
            if let Some(p) = self.store.project_mut(p) {
                p.columns = m;
            }
            return;
        }
        let k = self.view.key();
        if let Some(p) = self.store.settings.perspectives.iter_mut().find(|p| p.key == k) {
            p.columns = m;
        }
    }

    fn columns(&self, d: &Derived, now: Dt) -> Vec<Column> {
        let usual = self.usual_columns(d, now);
        let Some(mode) = self.column_mode() else { return usual };
        // the same actions the perspective's usual columns hold, laid out another way
        let mut scope: Vec<Id> = vec![];
        for c in &usual {
            for id in &c.tasks {
                if !scope.contains(id) {
                    scope.push(*id);
                }
            }
        }
        let tasks: Vec<&Task> = scope.iter().filter_map(|id| self.store.task(*id)).collect();
        if let Some(g) = mode.strip_prefix("group:").and_then(|g| g.parse::<Id>().ok()).filter(|g| self.store.tag(*g).is_some()) {
            let stages = self.store.tag_children(g);
            let hues = ["blue", "yellow", "orange", "purple", "green", "cyan", "indigo", "red"];
            let set_name = self.store.tag(g).map(|t| t.name.clone()).unwrap_or_default();
            let mut cols = vec![Column {
                title: format!("No {set_name}"),
                key: ColKey::Stage(g, None),
                color: th().dim,
                tasks: tasks.iter().filter(|t| !t.tags.iter().any(|x| stages.contains(x))).map(|t| t.id).collect(),
            }];
            for (i, st) in stages.iter().enumerate() {
                cols.push(Column {
                    title: self.store.tag(*st).map(|t| t.name.clone()).unwrap_or_default(),
                    key: ColKey::Stage(g, Some(*st)),
                    color: hue(hues[i % hues.len()]),
                    tasks: tasks.iter().filter(|t| t.tags.contains(st)).map(|t| t.id).collect(),
                });
            }
            if cols[0].tasks.is_empty() {
                cols.remove(0); // everything has a stage: no need for the catch-all
            }
            return cols;
        }
        match mode.as_str() {
            "project" => {
                let mut cols: Vec<Column> = vec![];
                for t in &tasks {
                    let key = ColKey::Project(t.project);
                    match cols.iter_mut().find(|c| c.key == key) {
                        Some(c) => c.tasks.push(t.id),
                        None => cols.push(Column { title: t.project.map(|p| self.store.project_path(p)).unwrap_or_else(|| "Inbox".into()), key, color: hue("blue"), tasks: vec![t.id] }),
                    }
                }
                cols
            }
            "tag" => {
                let mut cols: Vec<Column> = self
                    .store
                    .tag_tree()
                    .into_iter()
                    .map(|(g, _)| Column { title: self.store.tag_path(g), key: ColKey::Tag(Some(g)), color: hue("violet"), tasks: tasks.iter().filter(|t| t.tags.contains(&g)).map(|t| t.id).collect() })
                    .filter(|c| !c.tasks.is_empty())
                    .collect();
                cols.push(Column { title: "Untagged".into(), key: ColKey::Tag(None), color: th().dim, tasks: tasks.iter().filter(|t| t.tags.is_empty()).map(|t| t.id).collect() });
                cols
            }
            "status" => {
                let today = now.date();
                let (mut todo, mut waiting, mut done) = (vec![], vec![], vec![]);
                for t in &tasks {
                    let st = d.st(t.id);
                    if !st.remaining() {
                        if self.session_resolved.contains(&t.id) || t.completed.or(t.dropped).is_some_and(|x| (today - x.date()).num_days() < 7) {
                            done.push(t.id);
                        }
                    } else if st.available() {
                        todo.push(t.id);
                    } else {
                        waiting.push(t.id);
                    }
                }
                vec![
                    Column { title: "To Do".into(), key: ColKey::Todo, color: th().accent, tasks: todo },
                    Column { title: "Waiting".into(), key: ColKey::Waiting, color: th().dim, tasks: waiting },
                    Column { title: "Done".into(), key: ColKey::Done, color: hue("green"), tasks: done },
                ]
            }
            _ => usual,
        }
    }

    /// The columns a perspective's board uses by default.
    fn usual_columns(&self, d: &Derived, now: Dt) -> Vec<Column> {
        let today = now.date();
        let vis = |t: &&Task| self.visible(d, t);
        let leaf = |t: &&Task| !d.children.get(&Some(t.id)).is_some_and(|c| c.iter().any(|k| d.st(*k).remaining()));
        let status_cols = |scope: &dyn Fn(&Task) -> bool| {
            let mut todo = vec![];
            let mut waiting = vec![];
            let mut done = vec![];
            for t in self.store.tasks.iter().filter(|t| scope(t)).filter(leaf) {
                let st = d.st(t.id);
                if !st.remaining() {
                    if self.session_resolved.contains(&t.id) || t.completed.or(t.dropped).is_some_and(|x| (today - x.date()).num_days() < 7) {
                        done.push(t.id);
                    }
                } else if st.available() {
                    todo.push(t.id);
                } else {
                    waiting.push(t.id);
                }
            }
            vec![
                Column { title: "To Do".into(), key: ColKey::Todo, color: th().accent, tasks: todo },
                Column { title: "Waiting".into(), key: ColKey::Waiting, color: th().dim, tasks: waiting },
                Column { title: "Done".into(), key: ColKey::Done, color: hue("green"), tasks: done },
            ]
        };
        let per_tag = |only_places: bool| {
            let mut cols: Vec<Column> = self
                .store
                .tags
                .iter()
                .filter(|g| !only_places || g.place.is_some())
                .filter(|g| self.filter == Filter::All || self.filter == Filter::Tag(Some(g.id)))
                .map(|g| Column {
                    title: g.name.clone(),
                    key: ColKey::Tag(Some(g.id)),
                    color: hue("violet"),
                    tasks: self.store.tasks.iter().filter(|t| t.tags.contains(&g.id)).filter(vis).filter(leaf).map(|t| t.id).collect(),
                })
                .collect();
            if !only_places && matches!(self.filter, Filter::All | Filter::Tag(None)) {
                cols.push(Column {
                    title: "Untagged".into(),
                    key: ColKey::Tag(None),
                    color: th().dim,
                    tasks: self.store.tasks.iter().filter(|t| t.tags.is_empty()).filter(vis).filter(leaf).map(|t| t.id).collect(),
                });
            }
            cols
        };
        match self.view {
            View::Projects => self
                .store
                .projects
                .iter()
                .filter(|p| matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold))
                .filter(|p| self.filter == Filter::All || self.filter == Filter::Project(p.id))
                .map(|p| Column {
                    title: p.name.clone(),
                    key: ColKey::Project(Some(p.id)),
                    color: hue("blue"),
                    tasks: self.store.tasks.iter().filter(|t| t.project == Some(p.id)).filter(vis).filter(leaf).map(|t| t.id).collect(),
                })
                .collect(),
            View::Tags => per_tag(false),
            View::Nearby => per_tag(true),
            View::Forecast => {
                let mut cols = vec![Column {
                    title: "Past".into(),
                    key: ColKey::Fixed,
                    color: th().overdue,
                    tasks: self.store.tasks.iter().filter(|t| t.due.is_some_and(|x| x.date() < today)).filter(vis).map(|t| t.id).collect(),
                }];
                for i in 0..7 {
                    let day = today + Duration::days(i);
                    let title = if i == 0 { "Today".to_string() } else { day.format("%a %-d").to_string() };
                    cols.push(Column {
                        title,
                        key: ColKey::Day(day),
                        color: if i == 0 { hue("red") } else { th().dim },
                        tasks: self.store.tasks.iter().filter(|t| self.forecast_includes(t, day)).filter(vis).map(|t| t.id).collect(),
                    });
                }
                cols
            }
            View::Custom(_) => {
                let cfg = self.pcfg(self.view).cloned().unwrap_or_else(PerspectiveCfg::blank);
                let rule = cfg.rule();
                let matching = |t: &Task| rule.matches(&self.store, d, t, now) || self.session_resolved.contains(&t.id);
                match cfg.group_by().as_str() {
                    "project" => {
                        let mut cols: Vec<Column> = vec![];
                        for t in self.store.tasks.iter().filter(|t| matching(t)).filter(leaf) {
                            let key = ColKey::Project(t.project);
                            match cols.iter_mut().find(|c| c.key == key) {
                                Some(c) => c.tasks.push(t.id),
                                None => cols.push(Column {
                                    title: t.project.and_then(|p| self.store.project(p)).map(|p| p.name.clone()).unwrap_or_else(|| "Inbox".into()),
                                    key,
                                    color: hue("blue"),
                                    tasks: vec![t.id],
                                }),
                            }
                        }
                        cols
                    }
                    "tag" => {
                        let mut cols = per_tag(false);
                        for c in cols.iter_mut() {
                            c.tasks.retain(|id| self.store.task(*id).is_some_and(matching));
                        }
                        cols.retain(|c| !c.tasks.is_empty());
                        cols
                    }
                    _ => status_cols(&|t: &Task| {
                        // "To Do / Waiting / Done" ignores the availability rule so items can move between columns
                        let mut r = rule.clone();
                        if let crate::rules::Rule::All { rules } = &mut r {
                            rules.retain(|x| !matches!(x, crate::rules::Rule::Availability { .. }));
                        }
                        r.matches(&self.store, d, t, now)
                    }),
                }
            }
            View::Flagged => status_cols(&|t: &Task| {
                let (own, inh) = self.store.effective_flagged(t.id);
                own || inh
            }),
            _ => status_cols(&|t: &Task| t.project.is_none()),
        }
    }

    /// Dropping a card on a column (kanban): moves, retags, reschedules, defers or completes it.
    fn drop_card(&mut self, id: Id, from: &ColKey, to: &ColKey, now: Dt) {
        if from == to {
            return;
        }
        let st = self.store.settings.clone();
        match to {
            ColKey::Project(p) => self.store.set_project(id, *p),
            ColKey::Tag(g) => {
                if let Some(t) = self.store.task_mut(id) {
                    if let ColKey::Tag(Some(old)) = from {
                        t.tags.retain(|x| x != old);
                    }
                    match g {
                        Some(g) if !t.tags.contains(g) => t.tags.push(*g),
                        None => t.tags.clear(),
                        _ => {}
                    }
                    t.modified = now;
                }
            }
            ColKey::Day(day) => {
                if let Some(t) = self.store.task_mut(id) {
                    let time = t.due.map(|x| x.time()).unwrap_or(st.default_due_time);
                    t.due = Some(day.and_time(time));
                    t.modified = now;
                }
            }
            ColKey::Done => {
                self.store.complete(id, now);
                self.mark_resolved(id);
            }
            ColKey::Todo => {
                self.store.reactivate(id, now);
                if let Some(t) = self.store.task_mut(id) {
                    if t.defer.is_some_and(|x| x > now) {
                        t.defer = None;
                    }
                }
            }
            ColKey::Waiting => {
                self.store.reactivate(id, now);
                if let Some(t) = self.store.task_mut(id) {
                    t.defer = Some((now.date() + Duration::days(1)).and_time(st.default_defer_time));
                    t.modified = now;
                }
            }
            ColKey::Stage(g, stage) => {
                self.store.set_stage(id, *g, *stage);
                if let Some(t) = self.store.task_mut(id) {
                    t.modified = now;
                }
            }
            ColKey::Fixed => return,
        }
        self.touch();
    }

    pub(crate) fn board(&mut self, ui: &mut egui::Ui, d: &Derived, now: Dt, body: Rect) {
        let cols = self.columns(d, now);
        let mut dropped: Option<(Id, ColKey, ColKey)> = None;
        let mut select: Option<Id> = None;
        let mut toggle: Option<Id> = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(vec2(12.0, 8.0))));
        egui::ScrollArea::horizontal().id_salt(("board", self.view.key())).show(&mut child, |ui| {
            ui.horizontal_top(|ui| {
                for col in &cols {
                    ui.vertical(|ui| {
                        ui.set_width(270.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.5, col.color);
                            ui.label(egui::RichText::new(&col.title).font(bold(14.0)).color(th().text));
                            ui.label(egui::RichText::new(col.tasks.len().to_string()).color(th().dim));
                        });
                        let frame = egui::Frame::new().fill(mix(th().bg, th().field, 0.55)).corner_radius(10).inner_margin(egui::Margin::same(8));
                        let (_, payload) = ui.dnd_drop_zone::<(Id, ColKey), ()>(frame, |ui| {
                            ui.set_min_size(vec2(254.0, (body.height() - 80.0).max(120.0)));
                            egui::ScrollArea::vertical().id_salt(("col", col.title.clone())).show(ui, |ui| {
                                for &id in &col.tasks {
                                    let payload = (id, col.key.clone());
                                    // the same action can sit in several columns (one per tag): salt with the column
                                    let r = ui.dnd_drag_source(egui::Id::new(("card", id, col.title.as_str())), payload, |ui| ui.push_id(("card", id), |ui| self.card(ui, d, now, id)).inner).inner;
                                    if r.0 {
                                        select = Some(id);
                                    }
                                    if r.1 {
                                        toggle = Some(id);
                                    }
                                    ui.add_space(6.0);
                                }
                            });
                        });
                        if let Some(p) = payload {
                            let p: Arc<(Id, ColKey)> = p;
                            dropped = Some((p.0, p.1.clone(), col.key.clone()));
                        }
                    });
                    ui.add_space(10.0);
                }
            });
        });
        if let Some(id) = select {
            self.sel = Some(Sel::Task(id));
        }
        if let Some(id) = toggle {
            self.toggle_done(id);
        }
        if let Some((id, from, to)) = dropped {
            self.drop_card(id, &from, &to, now);
        }
    }

    /// One card: (clicked, circle clicked).
    fn card(&self, ui: &mut egui::Ui, d: &Derived, now: Dt, id: Id) -> (bool, bool) {
        let Some(t) = self.store.task(id) else { return (false, false) };
        let st = d.st(id);
        let selected = self.sel == Some(Sel::Task(id));
        let (own_flag, inh_flag) = self.store.effective_flagged(id);
        let mut circle_hit = false;
        let resp = egui::Frame::new()
            .fill(if selected { th().sel_row } else { th().field })
            .stroke(Stroke::new(1.0, if selected { th().accent } else { th().line }))
            .corner_radius(8)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(232.0);
                ui.horizontal_top(|ui| {
                    let (r, cr) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    let ring = row_ring(st, own_flag || inh_flag);
                    status_circle(ui.painter(), r.center(), ring, st, t.repeat.is_some(), own_flag || inh_flag);
                    circle_hit = cr.clicked();
                    let color = title_color(st, false);
                    let mut text = egui::RichText::new(if t.name.is_empty() { "Untitled Item" } else { &t.name }).color(color).size(14.0);
                    if !st.remaining() {
                        text = text.strikethrough();
                    }
                    ui.add(egui::Label::new(text).wrap());
                });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if let Some(p) = t.project.and_then(|p| self.store.project(p)) {
                        ui.label(egui::RichText::new(&p.name).color(th().dim).size(12.0));
                    }
                    if let Some(due) = d.eff_due.get(&id) {
                        let c = match st {
                            Status::Overdue => th().overdue,
                            Status::DueSoon => th().due_soon,
                            _ => th().dim,
                        };
                        ui.label(egui::RichText::new(short_date(*due, now.date())).color(c).size(12.0).background_color(c.gamma_multiply(0.15)));
                    }
                    for g in t.tags.iter().filter_map(|g| self.store.tag(*g)) {
                        ui.label(egui::RichText::new(&g.name).color(th().tag_text).size(11.5).background_color(th().tag_pill));
                    }
                    if let Some(issue) = t.linear.as_ref().and_then(|k| self.store.linear.get(k)) {
                        ui.label(egui::RichText::new(format!("{} · {}", issue.identifier, issue.state.name)).color(linear::color(&issue.state.color)).size(11.5));
                    }
                    if own_flag || inh_flag {
                        ui.label(egui::RichText::new("⚑").color(th().flag).size(12.0));
                    }
                });
            })
            .response;
        (resp.interact(Sense::click()).clicked() && !circle_hit, circle_hit)
    }

    /// The eye popover: what this perspective shows, and List / Board.
    pub(crate) fn view_options(&mut self, ctx: &egui::Context) {
        if !self.view_opts {
            return;
        }
        // under the eye, kept on screen (a phone is narrower than the popover's 330 pt + margins)
        let screen = ctx.content_rect();
        let pos = self.eye_rect.left_bottom() + vec2(-18.0, 10.0);
        let pos = pos2(pos.x.clamp(screen.left() + 8.0, (screen.right() - 370.0).max(screen.left() + 8.0)), pos.y);
        let name = self.pname(self.view);
        let mut changed = false;
        let area = egui::Area::new(egui::Id::new("view-options")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(th().inspector).corner_radius(14).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.set_width(330.0f32.min(screen.width() - 50.0));
                let key = self.view.key();
                match self.view {
                    View::Forecast => {
                        ui.label(egui::RichText::new("In Forecast, include:").font(bold(14.0)));
                        changed |= ui.checkbox(&mut self.store.settings.forecast_planned, "Items on planned date").changed();
                        changed |= ui.checkbox(&mut self.store.settings.forecast_defer, "Items on defer date").changed();
                        ui.separator();
                        ui.label(egui::RichText::new("Day timeline:").font(bold(14.0)));
                        ui.horizontal(|ui| {
                            changed |= ui.radio_value(&mut self.store.settings.forecast_vertical, false, "Horizontal").changed();
                            changed |= ui.radio_value(&mut self.store.settings.forecast_vertical, true, "Vertical").changed();
                        });
                        changed |= ui.checkbox(&mut self.store.settings.forecast_all_day_blocks, "All-day events block time").changed();
                    }
                    View::Review | View::Completed | View::Changed | View::Stats => {
                        ui.label(egui::RichText::new("This perspective has no view options.").color(th().dim));
                    }
                    _ => {
                        ui.label(egui::RichText::new(format!("In {}, show:", name)).font(bold(14.0)));
                        let cur = self.show_mode();
                        let mut opts = vec![
                            ("available", "Available", "Items not blocked or deferred"),
                            ("remaining", "Remaining", "Items not completed or dropped"),
                            ("everything", "Everything", "All items, including completed and dropped"),
                        ];
                        if self.view != View::Inbox {
                            opts.insert(0, ("first", "First Available", "Only the first available item per project"));
                        }
                        for (k, title, desc) in opts {
                            if ui.radio(cur == k, title).clicked() {
                                if let Some(p) = self.store.settings.perspectives.iter_mut().find(|p| p.key == key) {
                                    p.show = Some(k.into());
                                    changed = true;
                                }
                            }
                            ui.label(egui::RichText::new(desc).color(th().dim).size(12.0));
                        }
                    }
                }
                if self.board_supported() {
                    ui.separator();
                    ui.label(egui::RichText::new("Layout:").font(bold(14.0)));
                    let mut board = self.board_on();
                    ui.horizontal(|ui| {
                        changed |= ui.radio_value(&mut board, false, "List").changed();
                        changed |= ui.radio_value(&mut board, true, "Board").changed();
                    });
                    if let Some(p) = self.store.settings.perspectives.iter_mut().find(|p| p.key == key) {
                        p.board = board;
                    }
                    if board {
                        changed |= self.columns_picker(ui);
                    }
                }
                if matches!(self.view, View::Custom(_)) {
                    ui.separator();
                    if ui.button("Edit Perspective…").clicked() {
                        self.editor = Some(key);
                        self.view_opts = false;
                    }
                }
            });
        });
        if changed {
            self.touch();
        }
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !self.eye_rect.contains(p));
        if clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.view_opts = false;
        }
    }

    /// Board › Columns: the usual ones, To Do / Waiting / Done, by project, by tag, or a status set (a tag group).
    fn columns_picker(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let cur = self.column_mode();
        let label = |m: &Option<String>| -> String {
            match m.as_deref() {
                None => "Usual columns".into(),
                Some("status") => "To Do / Waiting / Done".into(),
                Some("project") => "By project".into(),
                Some("tag") => "By tag".into(),
                Some(g) => g.strip_prefix("group:").and_then(|g| g.parse::<Id>().ok()).map(|g| format!("Stages of {}", self.store.tag_path(g))).unwrap_or_else(|| "Usual columns".into()),
            }
        };
        let for_project = if let (View::Projects, Filter::Project(p)) = (self.view, self.filter) { self.store.project(p).map(|p| p.name.clone()) } else { None };
        ui.add_space(6.0);
        ui.label(egui::RichText::new(match &for_project {
            Some(n) => format!("Columns for {n}:"),
            None => "Columns:".into(),
        }).font(bold(14.0)));
        let mut pick: Option<Option<String>> = None;
        let mut options: Vec<Option<String>> = vec![None, Some("status".into()), Some("project".into()), Some("tag".into())];
        options.extend(self.store.tag_groups().into_iter().map(|g| Some(format!("group:{g}"))));
        egui::ComboBox::from_id_salt("board-columns").selected_text(label(&cur)).width(300.0).show_ui(ui, |ui| {
            for o in &options {
                if ui.selectable_label(*o == cur, label(o)).clicked() {
                    pick = Some(o.clone());
                }
            }
        });
        ui.label(egui::RichText::new("A status set is a tag with child tags (Status : Backlog, Status : In Progress…). Dragging a card to a column gives it that stage.").color(th().dim).size(12.0));
        if !self.store.tag_groups().iter().any(|g| self.store.tag(*g).is_some_and(|t| t.name == "Status")) && ui.button("Add a Status set (Backlog, To Do, In Progress, Waiting for Review)").clicked() {
            if let Some(g) = self.store.default_status_set() {
                pick = Some(Some(format!("group:{g}")));
            }
        }
        if let Some(m) = pick {
            self.set_column_mode(m);
            changed = true;
        }
        changed
    }

    /// Quick Open (⌘O): jump to a perspective, project or tag by typing.
    pub(crate) fn quick_open(&mut self, ctx: &egui::Context) {
        let Some(q) = self.quick_open.as_mut() else { return };
        let mut items: Vec<(String, String, View, Filter, Option<Sel>)> = vec![];
        for p in self.store.settings.perspectives.iter().filter(|p| p.visible) {
            if let Some(v) = View::from_key(&p.key) {
                items.push((p.name.clone(), "Perspective".into(), v, Filter::All, None));
            }
        }
        for p in self.store.projects.iter().filter(|p| matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)) {
            items.push((p.name.clone(), "Project".into(), View::Projects, Filter::Project(p.id), Some(Sel::Project(p.id))));
        }
        for g in &self.store.tags {
            items.push((g.name.clone(), "Tag".into(), View::Tags, Filter::Tag(Some(g.id)), Some(Sel::Tag(g.id))));
        }
        let ql = q.to_lowercase();
        items.retain(|i| ql.is_empty() || i.0.to_lowercase().contains(&ql));
        items.truncate(12);
        let mut pick = None;
        let mut close = false;
        egui::Window::new("Quick Open").collapsible(false).resizable(false).title_bar(false).anchor(Align2::CENTER_TOP, vec2(0.0, 110.0)).fixed_size([460.0, 0.0]).show(ctx, |ui| {
            let r = ui.add(crate::nativetext::Edit::singleline(q).hint_text("Go to perspective, project or tag").desired_width(f32::INFINITY).font(FontId::proportional(17.0)));
            r.request_focus();
            for (i, it) in items.iter().enumerate() {
                let resp = ui.horizontal(|ui| {
                    let l = ui.selectable_label(i == 0 && !ql.is_empty(), egui::RichText::new(&it.0).size(14.5));
                    ui.label(egui::RichText::new(&it.1).color(th().dim).size(12.0));
                    l
                });
                if resp.inner.clicked() {
                    pick = Some(i);
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !items.is_empty() {
                pick = Some(0);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if let Some(i) = pick {
            let (_, _, v, f, s) = items[i].clone();
            self.go(v);
            self.filter = f;
            if s.is_some() {
                self.sel = s;
            }
            close = true;
        }
        if close {
            self.quick_open = None;
        }
    }
}

/// Status circle colour: yellow due soon, red overdue, orange flagged.
pub(crate) fn row_ring(st: Status, flagged: bool) -> Color32 {
    match st {
        Status::Overdue => th().overdue,
        Status::DueSoon => th().due_soon,
        _ if flagged => th().flag,
        Status::Completed | Status::Dropped => th().dim,
        Status::Blocked => th().faint.gamma_multiply(1.3),
        _ => mix(th().text, th().dim, 0.6),
    }
}

/// Title colour: due soon / overdue titles take the status colour (reference: the person's Forecast picture).
pub(crate) fn title_color(st: Status, group: bool) -> Color32 {
    match st {
        Status::Overdue => th().overdue,
        Status::DueSoon => th().due_soon,
        Status::Completed | Status::Dropped => th().dim,
        Status::Blocked if !group => th().dim,
        Status::Next => th().next,
        _ => th().text,
    }
}
