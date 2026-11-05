//! The Focus window: a small window (optionally floating above everything) with what you're working on, its timer,
//! how long until the next timed item today, and Up Next — the actions you mean to do, in order.
use crate::*;
use eframe::egui::{self, vec2, Align2, FontId, Sense};

impl Gtd {
    /// What's being worked on: the Pomodoro's action, else the clocked-in one, else the first in Up Next.
    pub(crate) fn focus_task(&self) -> Option<Id> {
        self.pomo.and_then(|p| p.task).or_else(|| self.store.active_clock().map(|c| c.0)).or_else(|| self.store.queue_live().first().copied())
    }

    /// "Project : Action" style label for the focus displays.
    pub(crate) fn focus_label(&self, id: Id) -> (String, Option<String>) {
        let t = self.store.task(id);
        (t.map(|t| t.name.clone()).unwrap_or_default(), t.and_then(|t| t.project).map(|p| self.store.project_path(p)))
    }

    /// Re-reads today's Calendar events every few minutes while the Focus window is open and events are on.
    /// Keeps Calendar events current: re-reads them every few minutes (35 days ahead, like Forecast),
    /// and after "Allow Calendar Access" asks macOS. A request made while a read is running waits for it.
    pub(crate) fn poll_calendar(&mut self, ctx: &egui::Context, now: Dt, ask: bool) {
        self.cal_ask |= ask;
        // Re-read the permission whenever GTDCraft becomes the active app (back from System Settings, or from the
        // helper's dialog). No timer while access is off.
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused && self.cal_access != calendar::Access::Granted {
            self.cal_at = None;
        }
        self.was_focused = focused;
        if let Some(rx) = &self.cal_rx {
            match rx.try_recv() {
                Ok((access, r, cals)) => {
                    self.cal_rx = None;
                    self.cal_access = access;
                    // Ask on our own once per launch while access is undecided (full access already granted
                    // needs no request).
                    if access == calendar::Access::NotAsked && !self.cal_auto_asked {
                        self.cal_auto_asked = true;
                        self.cal_ask = true;
                    }
                    if !cals.is_empty() {
                        self.cal_list = Some(cals);
                    }
                    match r {
                        Ok(evs) => {
                            self.cal = evs;
                            self.cal_msg.clear();
                        }
                        Err(e) => {
                            self.cal.clear();
                            self.cal_msg = e;
                        }
                    }
                }
                Err(_) => return,
            }
        }
        let ask = std::mem::take(&mut self.cal_ask);
        // granted: refresh events every REFRESH; otherwise only on launch, when asked, or on becoming active (above)
        let due = ask || self.cal_at.is_none_or(|t| self.cal_access == calendar::Access::Granted && now - t >= calendar::REFRESH);
        if !self.store.settings.calendar_events || !due || self.shot.is_some() {
            return;
        }
        self.cal_at = Some(now);
        #[cfg(target_os = "macos")]
        if ask {
            calendar::yield_to_helper();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let (hidden, all_day, from) = (self.store.settings.hidden_calendars.clone(), self.store.settings.calendar_all_day, now.date());
        std::thread::spawn(move || {
            // everything goes through the helper app (it may take a moment, and the prompt waits for you)
            let mut access = calendar::access();
            if ask && access == calendar::Access::NotAsked {
                calendar::request_access();
                access = calendar::access();
            }
            let (events, cals) = if access == calendar::Access::Granted {
                (calendar::events(from, calendar::DAYS_AHEAD, &hidden, all_day), calendar::calendars())
            } else {
                (Err(if access == calendar::Access::Denied { "Calendar access is off for GTDCraft.".into() } else { "Calendar access not set up yet".into() }), vec![])
            };
            let _ = tx.send((access, events, cals));
            ctx.request_repaint();
        });
        self.cal_rx = Some(rx);
    }

    pub(crate) fn focus_window(&mut self, ctx: &egui::Context, now: Dt) {
        if !self.focus_win {
            return;
        }
        let level = if self.focus_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
        let builder = egui::ViewportBuilder::default().with_title("GTDCraft Focus").with_inner_size([340.0, 420.0]).with_min_inner_size([260.0, 180.0]).with_window_level(level);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("gtdcraft-focus"), builder, |ctx, class| {
            if class == egui::ViewportClass::Embedded {
                // no separate windows here (screenshots, some setups): show it as a window inside the app
                let mut open = true;
                egui::Window::new("GTDCraft Focus").open(&mut open).default_size([340.0, 420.0]).default_pos([560.0, 140.0]).collapsible(false).show(ctx, |ui| self.focus_panel(ui, now));
                self.focus_win &= open;
                return;
            }
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(th().inspector).inner_margin(egui::Margin::same(14))).show(ctx, |ui| self.focus_panel(ui, now));
            if ctx.input(|i| i.viewport().close_requested()) {
                self.focus_win = false;
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn focus_panel(&mut self, ui: &mut egui::Ui, now: Dt) {
        let w = ui.available_width();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Focus").font(bold(15.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let on_top = self.focus_on_top;
                if ui.selectable_label(on_top, "Float on top").on_hover_text("Keep this window above other apps").clicked() {
                    self.focus_on_top = !on_top;
                    let level = if self.focus_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                }
            });
        });
        ui.add_space(6.0);
        // what you're doing
        let cur = self.focus_task();
        egui::Frame::new().fill(th().field).corner_radius(10).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(w - 24.0);
            match cur {
                Some(id) => {
                    let (name, project) = self.focus_label(id);
                    if let Some(p) = project {
                        ui.label(egui::RichText::new(p).color(th().dim).size(12.0));
                    }
                    ui.label(egui::RichText::new(name).font(bold(16.0)).color(th().text));
                }
                None => {
                    ui.label(egui::RichText::new("Nothing in focus. Select an action and start a Pomodoro, or add actions to Up Next.").color(th().dim));
                }
            }
            ui.add_space(6.0);
            // the timer: Pomodoro countdown, else time clocked in
            let (big, color) = match (self.pomo, self.store.active_clock()) {
                (Some(p), _) => {
                    let left = (p.ends - now).num_seconds().max(0);
                    (format!("{} {:02}:{:02}", if p.work { "Focus" } else { "Break" }, left / 60, left % 60), if p.work { th().overdue } else { hue("green") })
                }
                (None, Some((_, start))) => {
                    let s = (now - start).num_seconds().max(0);
                    (format!("Clocked in {}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60), th().accent)
                }
                _ => ("Not running".into(), th().dim),
            };
            ui.label(egui::RichText::new(big).font(FontId::monospace(20.0)).color(color));
            ui.horizontal(|ui| {
                if self.pomo.is_some() {
                    if ui.button("Stop").clicked() {
                        self.stop_pomodoro(now);
                    }
                } else if cur.is_some() && ui.button("Start Pomodoro").clicked() {
                    if let Some(id) = cur {
                        self.sel = Some(Sel::Task(id));
                    }
                    self.start_pomodoro(now);
                }
                if let Some(id) = cur {
                    if ui.button("Done").on_hover_text("Complete it and move to the next in Up Next").clicked() {
                        self.finish_focus(id, now);
                    }
                }
            });
        });
        // how long until the next thing with a time today: a Calendar event or a due / planned action
        ui.add_space(8.0);
        let (in_event, next_event) = calendar::now_and_next(&self.cal, now);
        if let Some(e) = in_event {
            ui.label(egui::RichText::new(format!("In {} until {}", e.title, e.end.format("%-I:%M %p"))).color(th().accent));
        }
        let next_task = self.store.next_timed(now).map(|(at, id)| (at, self.focus_label(id).0));
        let next = match (next_event.map(|e| (e.start, e.title.clone())), next_task) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (a, b) => a.or(b),
        };
        match next {
            Some((at, name)) => {
                let mins = (at - now).num_minutes();
                let c = if mins <= 15 { th().overdue } else if mins <= 45 { th().due_soon } else { th().dim };
                ui.label(egui::RichText::new(format!("{} until {} · {}", stats::hm(mins), at.format("%-I:%M %p"), name)).color(c));
            }
            None => {
                ui.label(egui::RichText::new("Nothing else timed today.").color(th().dim).size(12.5));
            }
        }
        ui.horizontal(|ui| {
            let mut on = self.store.settings.calendar_events;
            if ui.checkbox(&mut on, egui::RichText::new("Include Calendar events").size(12.5)).changed() {
                self.store.settings.calendar_events = on;
                self.cal_at = None;
                if !on {
                    self.cal.clear();
                    self.cal_msg.clear();
                }
                self.touch();
            }
            if self.store.settings.calendar_events && self.cal_access == calendar::Access::NotAsked && ui.small_button("Allow Calendar Access").clicked() {
                self.poll_calendar(ui.ctx(), now, true);
            }
        });
        if self.store.settings.calendar_events && !self.cal_msg.is_empty() {
            ui.label(egui::RichText::new(&self.cal_msg).color(th().dim).size(11.5));
        }
        ui.separator();
        // Up Next
        ui.label(egui::RichText::new("Up Next").font(bold(14.0)));
        let queue = self.store.queue_live();
        if queue.is_empty() {
            ui.label(egui::RichText::new("Add actions with Add to Up Next (inspector Time section, or ⌃⌘U).").color(th().dim).size(12.5));
        }
        let mut remove = None;
        let mut start = None;
        egui::ScrollArea::vertical().max_height(ui.available_height() - 4.0).show(ui, |ui| {
            let moved = drag_list(ui, egui::Id::new("up-next"), queue.len(), |ui, i| {
                let id = queue[i];
                let (name, project) = self.focus_label(id);
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 30.0, 34.0), Sense::click());
                ui.painter().rect_filled(r, 7.0, if Some(id) == cur { th().sel_row } else { th().field });
                ui.painter().text(r.left_center() + vec2(10.0, -6.0), Align2::LEFT_CENTER, &name, FontId::proportional(13.5), th().text);
                ui.painter().text(r.left_center() + vec2(10.0, 9.0), Align2::LEFT_CENTER, project.unwrap_or_else(|| "Inbox".into()), FontId::proportional(11.0), th().dim);
                if resp.on_hover_text("Click to work on this next").clicked() {
                    start = Some(id);
                }
                if ui.small_button("×").on_hover_text("Remove from Up Next").clicked() {
                    remove = Some(id);
                }
            });
            if let Some((from, to)) = moved {
                let mut q = queue.clone();
                let x = q.remove(from);
                q.insert(to, x);
                self.store.queue = q;
                self.touch();
            }
        });
        if let Some(id) = remove {
            self.store.queue.retain(|x| *x != id);
            self.touch();
        }
        if let Some(id) = start {
            // move it to the front so it's what's in focus
            self.store.queue.retain(|x| *x != id);
            self.store.queue.insert(0, id);
            self.sel = Some(Sel::Task(id));
            self.touch();
        }
    }

    /// Done: complete the focused action, stop its timer, and line up the next one.
    fn finish_focus(&mut self, id: Id, now: Dt) {
        if self.pomo.is_some_and(|p| p.task == Some(id)) {
            self.stop_pomodoro(now);
        } else if self.store.active_clock().is_some_and(|c| c.0 == id) {
            self.store.clock_out(now);
        }
        self.store.complete(id, now);
        self.mark_resolved(id);
        self.store.queue.retain(|x| *x != id);
        self.sel = self.store.queue_live().first().map(|x| Sel::Task(*x));
        self.touch();
    }

    /// Settings › General › Calendar: show events, access, all-day events, and which calendars.
    pub(crate) fn settings_calendar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut ch = false;
        ui.heading("Calendar");
        ch |= ui.checkbox(&mut self.store.settings.calendar_events, "Show Calendar events in Forecast, the Focus window and to agents").changed();
        if !self.store.settings.calendar_events {
            return ch;
        }
        if self.cal_rx.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new("Checking Calendar…").color(th().dim).size(12.0));
            });
            if self.cal_access == calendar::Access::NotAsked && !cfg!(target_os = "ios") {
                ui.label(egui::RichText::new("No dialog after a few seconds? Click the GTDCraft Calendar icon in the Dock to bring it forward.").color(th().dim).size(11.5));
            }
        }
        match self.cal_access {
            calendar::Access::NotAsked => {
                let system = if cfg!(target_os = "ios") { "iOS" } else { "macOS" };
                ui.label(egui::RichText::new(format!("{system} asks once whether GTDCraft may read your calendars.")).color(th().dim).size(12.0));
                if ui.button("Allow Calendar Access").clicked() {
                    self.cal_ask = true;
                }
                if !cfg!(target_os = "ios") {
                    ui.label(egui::RichText::new("The request comes from \u{201c}GTDCraft Calendar\u{201d}, a small helper GTDCraft uses so the permission works however GTDCraft is opened.").color(th().dim).size(11.5));
                }
            }
            calendar::Access::Denied => {
                #[cfg(target_os = "ios")]
                {
                    ui.label(egui::RichText::new("Calendar access is off for GTDCraft. Turn it on in Settings › Apps › GTDCraft › Calendars.").color(th().dim).size(12.0));
                    if ui.button("Open Settings").clicked() {
                        calendar::open_app_settings();
                    }
                }
                #[cfg(not(target_os = "ios"))]
                {
                    ui.label(egui::RichText::new("GTDCraft requires system permissions. Turn on GTDCraft in System Settings › Privacy & Security › Calendars.").color(th().dim).size(12.0));
                    if ui.button("Open System Settings").clicked() {
                        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars").spawn();
                    }
                }
            }
            calendar::Access::Granted => {
                if ui.checkbox(&mut self.store.settings.calendar_all_day, "Include all-day events").changed() {
                    ch = true;
                    self.cal_at = None;
                }
                ui.label(egui::RichText::new("Calendars:").font(bold(13.0)));
                for c in self.cal_list.clone().unwrap_or_default() {
                    let mut on = !self.store.settings.hidden_calendars.contains(&c.id);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 5.0, c.color.map(|[r, g, b]| egui::Color32::from_rgb(r, g, b)).unwrap_or(th().accent));
                        if ui.checkbox(&mut on, &c.title).changed() {
                            if on {
                                self.store.settings.hidden_calendars.retain(|x| *x != c.id);
                            } else {
                                self.store.settings.hidden_calendars.push(c.id.clone());
                            }
                            ch = true;
                            self.cal_at = None;
                        }
                    });
                }
                ui.label(egui::RichText::new(format!("{} events in the next {} days.", self.cal.len(), calendar::DAYS_AHEAD)).color(th().dim).size(12.0));
            }
        }
        #[cfg(target_os = "macos")]
        {
            ui.add_space(6.0);
            if ui.button("Reset Calendar Permission and Ask Again").on_hover_text("Stops a stuck request, clears macOS's answer for GTDCraft Calendar, reinstalls the helper and asks again").clicked() {
                match calendar::reset() {
                    Ok(()) => {
                        self.cal_rx = None; // drop the stuck request's answer
                        self.cal_access = calendar::Access::NotAsked;
                        self.cal_list = None;
                        self.cal.clear();
                        self.cal_msg.clear();
                        self.cal_ask = true;
                    }
                    Err(e) => self.cal_msg = format!("Couldn't reset Calendar permission: {e}"),
                }
            }
            if !self.cal_msg.is_empty() && self.cal_access != calendar::Access::Granted {
                ui.label(egui::RichText::new(&self.cal_msg).color(th().dim).size(11.5));
            }
        }
        if ch {
            self.cal_at = None; // re-read with the new choices
        }
        ch
    }
}
