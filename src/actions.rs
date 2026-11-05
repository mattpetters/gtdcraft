//! Keyboard dispatch, the actions behind every shortcut, undo/redo, and what each perspective shows.
use crate::keys::{self, Action};
use crate::*;
use eframe::egui::{self, Key, Modifiers};

impl Gtd {
    pub(crate) fn keys(&mut self, ctx: &egui::Context, d: &Derived, now: Dt) {
        let events: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        // Settings › Keyboard is recording a new shortcut
        if let Some((action, add)) = self.recording {
            if let Some((key, m)) = events.into_iter().find(|(k, _)| *k != Key::Escape) {
                let b = keys::Binding { key, cmd: m.command, shift: m.shift, alt: m.alt, ctrl: m.ctrl };
                let mut list: Vec<String> = if add {
                    self.store.settings.keymap.get(action.id()).cloned().unwrap_or_else(|| action.defaults().iter().map(|s| s.to_string()).collect())
                } else {
                    vec![]
                };
                list.push(b.to_config());
                self.store.settings.keymap.insert(action.id().into(), list);
                self.recording = None;
                self.touch();
            } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.recording = None;
            }
            return;
        }
        if self.field_open.is_some() {
            return; // the field editor popover owns the keyboard (Enter applies, Esc closes)
        }
        let typing = ctx.wants_keyboard_input();
        let binds = keys::bindings(&self.store.settings.keymap);
        for (key, m) in events {
            // Return / Esc end title editing
            if self.editing.is_some() && (key == Key::Enter || key == Key::Escape) && !m.command {
                self.commit_edit();
                continue;
            }
            let hit = binds.iter().find(|(_, b)| b.matches(key, m) && (!typing || b.works_while_typing())).map(|x| x.0);
            if let Some(a) = hit {
                if typing && self.editing.is_some() {
                    self.commit_edit();
                }
                self.run(a, d, now);
            }
        }
    }

    /// Every selected action: the current one plus ⌘/⇧-clicked ones, in that order.
    pub(crate) fn selected_tasks(&self) -> Vec<Id> {
        let mut out: Vec<Id> = self.selected_task().into_iter().collect();
        for &id in &self.picked {
            if self.store.task(id).is_some() && !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    /// ⌘-click: toggles a row in the selection (the first selected row stays the one the inspector shows).
    pub(crate) fn pick_toggle(&mut self, id: Id) {
        match self.sel {
            Some(Sel::Task(cur)) if cur == id => self.sel = self.picked.pop().map(Sel::Task),
            Some(Sel::Task(_)) => {
                if let Some(i) = self.picked.iter().position(|x| *x == id) {
                    self.picked.remove(i);
                } else {
                    self.picked.push(id);
                }
            }
            _ => {
                self.sel = Some(Sel::Task(id));
                self.picked.clear();
            }
        }
    }

    /// ⇧-click: selects the rows between the current one and `id`, in the order they're shown.
    pub(crate) fn pick_range(&mut self, id: Id, rows: &[Sel]) {
        let anchor = if let Some(Sel::Task(a)) = self.sel { Some(a) } else { None };
        match anchor.and_then(|a| range_pick(a, id, rows)) {
            Some(p) => self.picked = p,
            None => {
                self.sel = Some(Sel::Task(id));
                self.picked.clear();
            }
        }
    }

    fn selected_task(&self) -> Option<Id> {
        match self.sel {
            Some(Sel::Task(id)) if self.store.task(id).is_some() => Some(id),
            _ => None,
        }
    }

    pub(crate) fn row_ids(&self, d: &Derived, now: Dt) -> Vec<Sel> {
        self.rows(d, now)
            .iter()
            .filter_map(|r| match r {
                Row::Task { id, .. } => Some(Sel::Task(*id)),
                Row::Project(p) => Some(Sel::Project(*p)),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn back(&mut self) {
        if let Some(v) = self.back.pop() {
            self.fwd.push(self.view);
            self.switch(v);
        }
    }
    pub(crate) fn forward(&mut self) {
        if let Some(v) = self.fwd.pop() {
            self.back.push(self.view);
            self.switch(v);
        }
    }

    pub(crate) fn undo_step(&mut self, redo: bool) {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        if let Some(prev) = from.pop() {
            to.push(self.store.clone());
            self.store = prev.clone();
            self.snapshot = Some(prev);
            self.undo_at = self.time;
            self.dirty_at = Some(f64::MIN_POSITIVE.max(self.last_poll));
            if self.sel.is_some_and(|s| matches!(s, Sel::Task(id) if self.store.task(id).is_none())) {
                self.sel = None;
            }
        }
    }

    pub(crate) fn edit_title(&mut self, id: Id) {
        self.editing = Some(id);
        self.edit_buf = self.store.task(id).map(|t| t.name.clone()).unwrap_or_default();
        self.edit_focus = true;
    }

    pub(crate) fn run(&mut self, a: Action, d: &Derived, now: Dt) {
        use Action::*;
        let sel = self.selected_task();
        match a {
            QuickOpen => self.quick_open = Some(String::new()),
            OpenInbox => self.go(View::Inbox),
            OpenProjects => self.go(View::Projects),
            OpenTags => self.go(View::Tags),
            OpenForecast => self.go(View::Forecast),
            OpenFlagged => self.go(View::Flagged),
            OpenNearby => self.go(View::Nearby),
            OpenReview => self.go(View::Review),
            OpenCompleted => self.go(View::Completed),
            OpenChanged => self.go(View::Changed),
            OpenStats => self.go(View::Stats),
            ClockToggle => self.toggle_clock(now),
            Pomodoro => {
                if self.pomo.is_some() {
                    self.stop_pomodoro(now);
                } else {
                    self.start_pomodoro(now);
                }
            }
            FocusWindow => self.focus_win = !self.focus_win,
            QueueAdd => {
                for id in self.selected_tasks() {
                    self.store.queue_add(id);
                }
                self.touch();
            }
            Back => self.back(),
            Forward => self.forward(),
            Today | PrevDay | NextDay => {
                if self.view != View::Forecast {
                    self.go(View::Forecast);
                }
                let today = now.date();
                let cur = match self.filter {
                    Filter::Day(x) => x,
                    _ => today,
                };
                let next = match a {
                    PrevDay => cur - Duration::days(1),
                    NextDay => cur + Duration::days(1),
                    _ => today,
                };
                self.filter = Filter::Day(next.max(today));
            }
            PerspectivesList => {
                self.show_settings = true;
                self.settings_tab = 2;
            }
            QuickEntry => self.quick = Some(crate::QuickEntry::default()),
            NewAction | NewBelow => {
                self.commit_edit();
                self.new_action();
            }
            NewAbove => {
                self.commit_edit();
                let before = sel;
                self.new_action();
                if let (Some(b), Some(n)) = (before, self.selected_task()) {
                    let t = self.store.tasks.remove(self.store.tasks.iter().position(|x| x.id == n).unwrap());
                    let at = self.store.tasks.iter().position(|x| x.id == b).unwrap_or(0);
                    self.store.tasks.insert(at, t);
                }
            }
            AddInside => {
                if let Some(p) = sel {
                    let proj = self.store.task(p).and_then(|t| t.project);
                    let id = self.store.add_task("", proj, Some(p));
                    self.collapsed.remove(&p);
                    self.sel = Some(Sel::Task(id));
                    self.edit_title(id);
                    self.touch();
                }
            }
            AddOutside => {
                if let Some(p) = sel.and_then(|s| self.store.task(s)).and_then(|t| t.parent) {
                    self.sel = Some(Sel::Task(p));
                    self.new_action();
                }
            }
            NewFolder => {
                // a folder of its own, empty until projects are moved in; its name is edited right away
                let after = self.selected_folder();
                let name = self.store.add_folder(after.as_deref());
                self.touch();
                self.go(View::Projects);
                self.renaming_folder = Some((name.clone(), name));
            }
            NewProject => {
                let id = self.store.add_project("New Project", ProjectKind::Parallel);
                self.touch();
                self.go(View::Projects);
                self.sel = Some(Sel::Project(id));
                self.show_inspector = true;
            }
            NewTag => {
                let id = self.store.new_id();
                self.store.tags.push(Tag { id, name: "New Tag".into(), on_hold: false, place: None, parent: None, modified: None });
                self.touch();
                self.go(View::Tags);
                self.sel = Some(Sel::Tag(id));
                self.show_inspector = true;
            }
            SelectNext | SelectPrev => {
                self.picked.clear();
                let ids = self.row_ids(d, now);
                let pos = self.sel.and_then(|s| ids.iter().position(|x| *x == s));
                self.sel = if a == SelectNext {
                    ids.get(pos.map(|p| p + 1).unwrap_or(0)).copied().or(self.sel)
                } else {
                    pos.and_then(|p| p.checked_sub(1)).and_then(|p| ids.get(p)).copied().or(self.sel)
                };
            }
            ToggleComplete => match self.sel {
                Some(Sel::Task(_)) => {
                    for id in self.selected_tasks() {
                        self.toggle_done(id);
                    }
                }
                Some(Sel::Project(p)) => {
                    let done = self.store.project(p).is_some_and(|x| x.status == ProjectStatus::Completed);
                    self.store.set_project_status(p, if done { ProjectStatus::Active } else { ProjectStatus::Completed }, now);
                    self.touch();
                }
                _ => {}
            },
            ToggleDrop => match self.sel {
                Some(Sel::Task(_)) => {
                    for id in self.selected_tasks() {
                        self.toggle_drop(id);
                    }
                }
                Some(Sel::Project(p)) => {
                    let dropped = self.store.project(p).is_some_and(|x| x.status == ProjectStatus::Dropped);
                    self.store.set_project_status(p, if dropped { ProjectStatus::Active } else { ProjectStatus::Dropped }, now);
                    self.touch();
                }
                _ => {}
            },
            ToggleFlag => {
                if let Some(s) = self.sel {
                    self.toggle_flag(s);
                    for id in self.picked.clone() {
                        self.toggle_flag(Sel::Task(id));
                    }
                }
            }
            EditTitle => {
                if let Some(id) = sel {
                    self.edit_title(id);
                }
            }
            EditNote => {
                if let Some(id) = sel {
                    self.show_inspector = true;
                    self.note_edit = Some(id);
                    self.focus_field = Some(EditNote);
                }
            }
            EditProject | EditTags | EditDefer | EditPlanned | EditDue | EditRepeat | EditDuration => {
                // quick edit opens the field's editor right on the selected row
                if let (Some(id), Some(f)) = (sel, fields::Field::from_action(a)) {
                    self.open_field(id, f);
                }
            }
            EditCompleted => {
                if sel.is_some() {
                    self.show_inspector = true;
                    self.focus_field = Some(a);
                }
            }
            Undo => self.undo_step(false),
            Redo => self.undo_step(true),
            // No confirmation (Undo brings it back), no repeating-action prompt, and the previous row is
            // selected afterwards.
            Delete if sel.is_none() && matches!(self.sel, Some(Sel::Project(_))) => {
                let Some(Sel::Project(pid)) = self.sel else { return };
                let ids = self.row_ids(d, now);
                let pos = ids.iter().position(|x| *x == Sel::Project(pid));
                self.store.delete_project(pid);
                if self.filter == crate::Filter::Project(pid) {
                    self.filter = crate::Filter::All;
                }
                self.sel = pos.and_then(|p| p.checked_sub(1)).and_then(|p| ids.get(p)).copied().filter(|s| !matches!(s, Sel::Task(t) if self.store.task(*t).is_none()));
                self.touch();
            }
            Delete => {
                if let Some(id) = sel {
                    // delete the selected row, then select the previous row
                    let ids = self.row_ids(d, now);
                    let pos = ids.iter().position(|x| *x == Sel::Task(id));
                    for other in std::mem::take(&mut self.picked) {
                        self.store.delete_task(other);
                    }
                    self.store.delete_task(id);
                    self.sel = pos.and_then(|p| p.checked_sub(1)).and_then(|p| ids.get(p)).copied();
                    self.touch();
                }
            }
            Duplicate => {
                if let Some(n) = sel.and_then(|id| self.store.duplicate(id)) {
                    self.sel = Some(Sel::Task(n));
                    self.touch();
                }
            }
            ConvertToProject => {
                if let Some(p) = sel.and_then(|id| self.store.convert_to_project(id)) {
                    self.sel = Some(Sel::Project(p));
                    self.touch();
                }
            }
            MarkReviewed => {
                let p = match self.sel {
                    Some(Sel::Project(p)) => Some(p),
                    Some(Sel::Task(t)) => self.store.task(t).and_then(|t| t.project),
                    _ => None,
                };
                if let Some(p) = p {
                    self.store.mark_reviewed(p, now);
                    self.touch();
                }
            }
            CleanUp => {
                self.session_resolved.clear();
                self.hide_queue.clear();
                self.inbox_kept.clear(); // processed inbox items (with a project) leave the Inbox now
            }
            MoveUp | MoveDown => {
                if sel.is_some_and(|id| self.store.move_sibling(id, a == MoveDown)) {
                    self.touch();
                }
            }
            Indent => {
                if let Some(id) = sel {
                    let parent_before = self.store.task(id).and_then(|t| t.parent);
                    if self.store.indent(id) {
                        if let Some(p) = self.store.task(id).and_then(|t| t.parent).filter(|p| Some(*p) != parent_before) {
                            self.collapsed.remove(&p);
                        }
                        self.touch();
                    }
                }
            }
            Outdent => {
                if sel.is_some_and(|id| self.store.outdent(id)) {
                    self.touch();
                }
            }
            Group => {
                if let Some(g) = sel.and_then(|id| self.store.group(id)) {
                    self.sel = Some(Sel::Task(g));
                    self.edit_title(g);
                    self.touch();
                }
            }
            Ungroup => {
                if sel.is_some_and(|id| self.store.ungroup(id)) {
                    self.sel = None;
                    self.touch();
                }
            }
            Settings => self.show_settings = !self.show_settings,
            ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            ToggleInspector => self.show_inspector = !self.show_inspector,
            ToggleBoard => {
                let k = self.view.key();
                if let Some(p) = self.store.settings.perspectives.iter_mut().find(|p| p.key == k) {
                    p.board = !p.board;
                    self.touch();
                }
            }
            Search => self.focus_search = true,
        }
    }

    // ---------- what each perspective shows (view options: the eye button) ----------
    /// first | available | remaining | everything
    pub(crate) fn show_mode(&self) -> String {
        let default = match self.view {
            View::Flagged | View::Nearby => "available",
            _ => "remaining",
        };
        self.pcfg(self.view).and_then(|p| p.show.clone()).unwrap_or_else(|| default.into())
    }

    pub(crate) fn visible(&self, d: &Derived, t: &Task) -> bool {
        let s = d.st(t.id);
        let base = match self.show_mode().as_str() {
            "first" => s == Status::Next || (t.project.is_none() && s.available()),
            "available" => s.available(),
            "everything" => true,
            _ => s.remaining(),
        };
        let q = self.search.trim().to_lowercase();
        (base || self.session_resolved.contains(&t.id)) && (q.is_empty() || t.name.to_lowercase().contains(&q) || t.note.to_lowercase().contains(&q))
    }

    /// Task by id through the snapshot's index, checked against the live list (never panics on a stale index).
    pub(crate) fn task_at(&self, d: &Derived, id: Id) -> Option<&Task> {
        d.idx.get(&id).and_then(|&i| self.store.tasks.get(i)).filter(|t| t.id == id).or_else(|| self.store.task(id))
    }

    /// A group stays visible while anything inside it is.
    pub(crate) fn tree_visible(&self, d: &Derived, t: &Task) -> bool {
        self.visible(d, t)
            || d.children.get(&Some(t.id)).is_some_and(|kids| kids.iter().any(|k| self.task_at(d, *k).is_some_and(|c| self.tree_visible(d, c))))
    }

    pub(crate) fn push_tree(&self, d: &Derived, rows: &mut Vec<Row>, parent: Option<Id>, depth: usize, keep: &dyn Fn(&Task) -> bool) {
        let Some(kids) = d.children.get(&parent) else { return };
        for &id in kids {
            let Some(t) = self.task_at(d, id) else { continue };
            if !keep(t) || !self.tree_visible(d, t) {
                continue;
            }
            rows.push(Row::Task { id, depth, show_project: false });
            if !self.collapsed.contains(&id) {
                self.push_tree(d, rows, Some(id), depth + 1, &|c: &Task| c.parent == Some(id));
            }
        }
    }
}

/// The rows from `anchor` to `id` in display order, without the anchor (it stays the main selection).
pub(crate) fn range_pick(anchor: Id, id: Id, rows: &[Sel]) -> Option<Vec<Id>> {
    let tasks: Vec<Id> = rows.iter().filter_map(|r| if let Sel::Task(t) = r { Some(*t) } else { None }).collect();
    let a = tasks.iter().position(|x| *x == anchor)?;
    let b = tasks.iter().position(|x| *x == id)?;
    let (lo, hi) = (a.min(b), a.max(b));
    Some(tasks[lo..=hi].iter().copied().filter(|x| *x != anchor).collect())
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    #[test]
    fn shift_click_selects_the_range_in_display_order() {
        let rows = [Sel::Task(5), Sel::Project(1), Sel::Task(9), Sel::Task(2), Sel::Task(7)];
        assert_eq!(range_pick(9, 7, &rows), Some(vec![2, 7])); // downward
        assert_eq!(range_pick(7, 5, &rows), Some(vec![5, 9, 2])); // upward, project rows skipped
        assert_eq!(range_pick(9, 9, &rows), Some(vec![]));
        assert_eq!(range_pick(9, 42, &rows), None); // not on screen
    }
}
