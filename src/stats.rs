//! Time tracking (org-mode style clock in / out), Pomodoro timer, and the Stats perspective.
//! Charts follow the dataviz method: one axis, thin marks with rounded data ends, a 2px gap between fills,
//! fixed-order categorical colours (validated: all checks pass on dark surfaces; light surfaces need the
//! visible labels and table these charts carry), a legend for >= 2 series, hover readouts on every mark.
use crate::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

/// Categorical slots 1-6 from the dataviz reference palette (light, dark), in fixed order.
const SERIES: [(&str, &str); 6] = [("#2a78d6", "#3987e5"), ("#eb6834", "#d95926"), ("#1baf7a", "#199e70"), ("#eda100", "#c98500"), ("#e87ba4", "#d55181"), ("#008300", "#008300")];

fn series(i: usize) -> Color32 {
    let (l, d) = SERIES[i % SERIES.len()];
    theme::hex(if th().dark { d } else { l }).unwrap()
}

fn other() -> Color32 {
    mix(th().dim, th().bg, 0.25)
}

pub(crate) fn hm(mins: i64) -> String {
    if mins >= 60 { format!("{}:{:02}", mins / 60, mins % 60) } else { format!("0:{mins:02}") }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Pomo {
    pub work: bool,
    pub ends: Dt,
    pub task: Option<Id>,
    /// work intervals finished in this run
    pub count: u32,
}

impl Gtd {
    pub(crate) fn toggle_clock(&mut self, now: Dt) {
        match (self.store.active_clock(), self.sel) {
            (Some((running, _)), Some(Sel::Task(id))) if running != id => self.store.clock_in(id, now, false),
            (Some(_), _) => {
                self.store.clock_out(now);
            }
            (None, Some(Sel::Task(id))) => self.store.clock_in(id, now, false),
            _ => return,
        }
        match self.store.active_clock() {
            Some((id, start)) => live::start(&self.store.task(id).map(|t| t.name.clone()).unwrap_or_default(), start, None),
            None => live::end(),
        }
        self.touch();
    }

    pub(crate) fn start_pomodoro(&mut self, now: Dt) {
        // the selected action, else the clocked-in one, else the first in Up Next
        let task = match self.sel {
            Some(Sel::Task(id)) => Some(id),
            _ => self.store.active_clock().map(|c| c.0).or_else(|| self.store.queue_live().first().copied()),
        };
        let work = self.store.settings.pomodoro[0].max(1) as i64;
        if let Some(t) = task {
            self.store.clock_in(t, now, true);
        }
        self.pomo = Some(Pomo { work: true, ends: now + Duration::minutes(work), task, count: self.pomo.map(|p| p.count).unwrap_or(0) });
        let title = task.and_then(|t| self.store.task(t)).map(|t| t.name.clone()).unwrap_or_else(|| "Pomodoro".into());
        live::start(&title, now, Some(now + Duration::minutes(work)));
        self.touch();
    }

    /// Focus started or stopped on another device (it arrives as a synced clock entry): follow it here — a Pomodoro
    /// clock gets this device's countdown (its start + the work length), a stopped one ends it, and on iPhone the Live
    /// Activity and the widgets show the new state at once (the person: "enabling focus on one platform should
    /// instantly get the same state on the other linked devices… and widgets should update").
    pub(crate) fn follow_focus(&mut self) {
        let clock = self.store.active_clock();
        if clock == self.last_clock {
            return;
        }
        self.last_clock = clock;
        let pomo_clock = clock.filter(|(id, _)| self.store.task(*id).is_some_and(|t| t.clock.iter().any(|c| c.end.is_none() && c.pomodoro)));
        match (self.pomo, pomo_clock) {
            (None, Some((id, start))) => {
                let work = self.store.settings.pomodoro[0].max(1) as i64;
                self.pomo = Some(Pomo { work: true, ends: start + Duration::minutes(work), task: Some(id), count: 0 });
            }
            (Some(p), None) if p.work && clock.is_none() => self.pomo = None,
            _ => {}
        }
        match clock {
            Some((id, start)) => live::start(&self.store.task(id).map(|t| t.name.clone()).unwrap_or_default(), start, self.pomo.filter(|p| p.work).map(|p| p.ends)),
            None if self.pomo.is_none() => live::end(),
            None => {}
        }
        #[cfg(target_os = "ios")]
        live::reload_widgets();
    }

    pub(crate) fn stop_pomodoro(&mut self, now: Dt) {
        live::end();
        if let Some(p) = self.pomo.take() {
            if p.work && p.task.is_some() {
                self.store.clock_out(now);
                self.touch();
            }
        }
    }

    /// Work → break → stop, with a reminder at each change (break is long every Nth interval).
    pub(crate) fn tick_pomodoro(&mut self, now: Dt) {
        let Some(p) = self.pomo else { return };
        if now < p.ends {
            return;
        }
        let [_, short, long, every] = self.store.settings.pomodoro;
        if p.work {
            if p.task.is_some() {
                self.store.clock_out(p.ends);
            }
            self.store.pomodoros.push((p.ends, p.task));
            let count = p.count + 1;
            let long_break = every > 0 && count % every == 0;
            let mins = if long_break { long } else { short };
            if self.shot.is_none() {
                places::notify("Pomodoro done", &format!("Take a {mins}-minute break."));
            }
            self.pomo = Some(Pomo { work: false, ends: p.ends + Duration::minutes(mins.max(1) as i64), task: p.task, count });
        } else {
            if self.shot.is_none() {
                places::notify("Break over", "Ready for the next Pomodoro?");
            }
            self.pomo = None;
        }
        self.touch();
    }

    /// Toolbar pill: running clock and/or Pomodoro countdown; click stops it.
    pub(crate) fn timer_pill(&mut self, ui: &mut egui::Ui, at: egui::Pos2, max_w: f32, now_s: Dt) {
        let clock = self.store.active_clock();
        if clock.is_none() && self.pomo.is_none() {
            return;
        }
        let (label, color) = match (self.pomo, clock) {
            (Some(p), _) => {
                let left = (p.ends - now_s).num_seconds().max(0);
                let what = if p.work { "Focus" } else { "Break" };
                // the action being worked on, so the pill says what you're meant to be doing
                let name = p.task.and_then(|t| self.store.task(t)).map(|t| t.name.chars().take(24).collect::<String>()).map(|n| format!("  {n}")).unwrap_or_default();
                (format!("{what} {:02}:{:02}{name}", left / 60, left % 60), if p.work { th().overdue } else { hue("green") })
            }
            (None, Some((id, start))) => {
                let secs = (now_s - start).num_seconds().max(0);
                let name = self.store.task(id).map(|t| t.name.clone()).unwrap_or_default();
                let short: String = name.chars().take(18).collect();
                (format!("{short}  {}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60), th().accent)
            }
            _ => return,
        };
        // fit between the toolbar buttons and the search field: cut the action's name short if needed
        let fits = |s: &str| ui.painter().layout_no_wrap(s.to_string(), FontId::monospace(12.5), th().text).size().x + 44.0 <= max_w;
        let mut label = label;
        while !fits(&label) && label.chars().count() > 12 {
            label = label.chars().take(label.chars().count() - 2).collect::<String>().trim_end_matches('…').to_string() + "…";
        }
        let g = ui.painter().layout_no_wrap(label, FontId::monospace(12.5), th().text);
        let r = Rect::from_min_size(at - vec2(0.0, 15.0), vec2(g.size().x + 44.0, 30.0));
        let resp = ui.interact(r, ui.id().with("timer-pill"), Sense::click());
        ui.painter().rect_filled(r, 15.0, if resp.hovered() { th().pill_hover } else { th().pill });
        ui.painter().circle_filled(r.left_center() + vec2(16.0, 0.0), 5.0, color);
        ui.painter().galley(r.left_center() + vec2(28.0, -g.size().y / 2.0), g, th().text);
        let what = self.focus_task().map(|id| self.focus_label(id)).map(|(n, p)| match p {
            Some(p) => format!("{p} : {n}\n"),
            None => format!("{n}\n"),
        });
        let tip = format!("{}{}", what.unwrap_or_default(), if self.pomo.is_some() { "Click to stop the Pomodoro · ⌃⌘M opens the Focus window" } else { "Click to clock out · ⌃⌘M opens the Focus window" });
        if resp.on_hover_text(tip).clicked() {
            if self.pomo.is_some() {
                self.stop_pomodoro(now_s);
            } else {
                self.store.clock_out(now_s);
                self.touch();
            }
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }

    /// Inspector "Time" section: total, clock in/out, Pomodoro, and the LOGBOOK.
    pub(crate) fn time_section(&mut self, ui: &mut egui::Ui, id: Id, now: Dt, op: &mut Option<(String, SecMove)>) {
        sec_caption(ui, "Time", "time", None, op);
        let own = self.store.clocked_minutes(id, false, now);
        let all = self.store.clocked_minutes(id, true, now);
        let running = self.store.active_clock().is_some_and(|c| c.0 == id);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(hm(all)).font(bold(18.0)));
            if all != own {
                ui.label(egui::RichText::new(format!("({} here, rest in sub-actions)", hm(own))).color(th().dim).size(12.0));
            }
        });
        ui.horizontal(|ui| {
            if ui.button(if running { "Clock Out" } else { "Clock In" }).clicked() {
                if running {
                    self.store.clock_out(now);
                } else {
                    self.store.clock_in(id, now, false);
                }
                self.touch();
            }
            let pomo_here = self.pomo.is_some_and(|p| p.task == Some(id));
            if pomo_here {
                if ui.button("Stop Pomodoro").clicked() {
                    self.stop_pomodoro(now);
                }
            } else if ui.button(format!("Start Pomodoro ({} min)", self.store.settings.pomodoro[0])).clicked() {
                self.sel = Some(Sel::Task(id));
                self.start_pomodoro(now);
            }
        });
        ui.horizontal(|ui| {
            let queued = self.store.queue.contains(&id);
            if ui.button(if queued { "Remove from Up Next" } else { "Add to Up Next" }).clicked() {
                if queued {
                    self.store.queue.retain(|x| *x != id);
                } else {
                    self.store.queue_add(id);
                }
                self.touch();
            }
            if ui.button("Focus Window").on_hover_text("A small window with the timer, this action and Up Next (⌃⌘M)").clicked() {
                self.focus_win = true;
            }
        });
        let entries: Vec<Clock> = self.store.task(id).map(|t| t.clock.iter().rev().take(6).copied().collect()).unwrap_or_default();
        if !entries.is_empty() {
            egui::CollapsingHeader::new(egui::RichText::new("LOGBOOK").monospace().color(th().dim)).id_salt(("logbook", id)).show(ui, |ui| {
                for c in entries {
                    let end = c.end.map(|e| format!("[{}]", e.format("%Y-%m-%d %a %H:%M"))).unwrap_or_else(|| "running".into());
                    let line = format!("CLOCK: [{}]--{} => {}{}", c.start.format("%Y-%m-%d %a %H:%M"), end, hm(c.minutes(now)), if c.pomodoro { " 🍅" } else { "" });
                    ui.label(egui::RichText::new(line).monospace().size(11.0).color(th().dim));
                }
            });
        }
    }

    // ---------- Stats perspective ----------
    pub(crate) fn stats_view(&mut self, ui: &mut egui::Ui, body: Rect, now: Dt) {
        // the Chrome extension's records, re-read at most every 30 s
        if self.web.0 == 0.0 || self.time - self.web.0 > 30.0 {
            self.web = (self.time.max(0.001), crate::webact::load());
        }
        let days = self.stats_days.max(1) as i64;
        let from = (now.date() - Duration::days(days - 1)).and_time(NaiveTime::MIN);
        // clock entries in range, cut to the range start
        let mut entries: Vec<(Id, Dt, i64)> = vec![];
        for t in &self.store.tasks {
            for c in &t.clock {
                let end = c.end.unwrap_or(now);
                if end > from {
                    let start = c.start.max(from);
                    entries.push((t.id, start, (end - start).num_minutes().max(0)));
                }
            }
        }
        let total: i64 = entries.iter().map(|e| e.2).sum();
        let pomos = self.store.pomodoros.iter().filter(|p| p.0 >= from).count();
        let proj_of = |id: Id| self.store.task(id).and_then(|t| t.project).and_then(|p| self.store.project(p)).map(|p| p.name.clone()).unwrap_or_else(|| "Inbox".into());
        let mut by_project: Vec<(String, i64)> = vec![];
        let mut by_tag: Vec<(String, i64)> = vec![];
        let mut by_task: Vec<(Id, i64, usize)> = vec![];
        for (id, _, m) in &entries {
            let p = proj_of(*id);
            match by_project.iter_mut().find(|x| x.0 == p) {
                Some(x) => x.1 += m,
                None => by_project.push((p, *m)),
            }
            let tags: Vec<String> = self.store.task(*id).map(|t| t.tags.iter().filter_map(|g| self.store.tag(*g)).map(|g| g.name.clone()).collect()).unwrap_or_default();
            for tg in if tags.is_empty() { vec!["Untagged".to_string()] } else { tags } {
                match by_tag.iter_mut().find(|x| x.0 == tg) {
                    Some(x) => x.1 += m,
                    None => by_tag.push((tg, *m)),
                }
            }
            match by_task.iter_mut().find(|x| x.0 == *id) {
                Some(x) => {
                    x.1 += m;
                    x.2 += 1;
                }
                None => by_task.push((*id, *m, 1)),
            }
        }
        by_project.sort_by(|a, b| b.1.cmp(&a.1));
        by_tag.sort_by(|a, b| b.1.cmp(&a.1));
        by_task.sort_by(|a, b| b.1.cmp(&a.1));
        let per_day: Vec<(NaiveDate, i64)> = (0..days)
            .map(|k| {
                let day = from.date() + Duration::days(k);
                (day, entries.iter().filter(|e| e.1.date() == day).map(|e| e.2).sum())
            })
            .collect();
        let weeks: Vec<(NaiveDate, i64)> = (0..12)
            .rev()
            .map(|k| {
                let start = now.date() - Duration::days(now.date().weekday().num_days_from_monday() as i64) - Duration::weeks(k);
                let end = start + Duration::days(7);
                let m = self.store.tasks.iter().flat_map(|t| t.clock.iter()).filter(|c| c.start.date() >= start && c.start.date() < end).map(|c| c.minutes(now)).sum();
                (start, m)
            })
            .collect();

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(vec2(22.0, 10.0))));
        // `--scroll`: a screenshot of what's below the fold (the Stats page is taller than a window)
        let area = egui::ScrollArea::vertical().id_salt("stats").auto_shrink([false, false]);
        let area = if self.scroll != 0.0 { area.vertical_scroll_offset(self.scroll) } else { area };
        area.show(&mut child, |ui| {
            let w = ui.available_width();
            // filters in one row above the charts
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Range").color(th().dim));
                for (n, l) in [(7u32, "7 days"), (30, "30 days"), (90, "90 days")] {
                    if ui.selectable_label(self.stats_days == n, l).clicked() {
                        self.stats_days = n;
                    }
                }
            });
            ui.add_space(8.0);
            // stat tiles
            // phones: two tiles a row and the charts stacked; side by side they'd be too narrow
            let narrow = w < 560.0;
            let cols = if narrow { 2.0 } else { 4.0 };
            let tile_w = (w - (cols - 1.0) * ui.spacing().item_spacing.x) / cols - 24.0;
            let busiest = by_project.first().map(|p| p.0.clone()).unwrap_or_else(|| "—".into());
            let tiles = [("Time tracked", hm(total)), ("Daily average", hm(total / days)), ("Pomodoros", pomos.to_string()), ("Most time on", busiest)];
            for row in tiles.chunks(cols as usize) {
                ui.horizontal(|ui| {
                    for (label, value) in row {
                        egui::Frame::new().fill(th().field).corner_radius(10).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(tile_w);
                                ui.label(egui::RichText::new(*label).color(th().dim).size(12.0));
                                ui.add(egui::Label::new(egui::RichText::new(value.as_str()).font(bold(22.0)).color(th().text)).truncate());
                            });
                        });
                    }
                });
            }
            ui.add_space(14.0);
            if narrow {
                chart_title(ui, "Time by project");
                donut(ui, &by_project, total, w);
                ui.add_space(14.0);
                chart_title(ui, "Time by tag");
                ranked_bars(ui, &by_tag, w);
            } else {
                ui.horizontal_top(|ui| {
                    let half = (w - 16.0) / 2.0;
                    ui.vertical(|ui| {
                        ui.set_width(half);
                        chart_title(ui, "Time by project");
                        donut(ui, &by_project, total, half);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(half);
                        chart_title(ui, "Time by tag");
                        ranked_bars(ui, &by_tag, half);
                    });
                });
            }
            ui.add_space(14.0);
            chart_title(ui, "Time per day");
            day_bars(ui, &per_day, w);
            ui.add_space(14.0);
            chart_title(ui, "Weekly trend");
            trend(ui, &weeks, w);
            ui.add_space(14.0);
            chart_title(ui, "Top tasks");
            egui::Grid::new("top-tasks").striped(true).num_columns(4).spacing([18.0, 6.0]).show(ui, |ui| {
                for h in ["Task", "Project", "Time", "Sessions"] {
                    ui.label(egui::RichText::new(h).color(th().dim).size(12.0));
                }
                ui.end_row();
                for (id, m, n) in by_task.iter().take(10) {
                    let name = self.store.task(*id).map(|t| t.name.clone()).unwrap_or_default();
                    ui.label(name);
                    ui.label(egui::RichText::new(proj_of(*id)).color(th().dim));
                    ui.label(egui::RichText::new(hm(*m)).monospace());
                    ui.label(n.to_string());
                    ui.end_row();
                }
            });
            ui.add_space(14.0);
            web_section(ui, &self.web.1, from, now, w);
            ui.add_space(30.0);
        });
    }
}

/// Web activity from the Chrome extension for the same range: on task vs distraction, sites, searches, videos.
fn web_section(ui: &mut egui::Ui, recs: &[crate::webact::Rec], from: Dt, now: Dt, w: f32) {
    {
        use chrono::TimeZone;
        chart_title(ui, "Web activity");
        let ms = |t: Dt| chrono::Local.from_local_datetime(&t).earliest().map(|x| x.timestamp_millis()).unwrap_or(0);
        let _ = now;
        let s = crate::webact::summarize(recs, ms(from), chrono::Local::now().timestamp_millis() + 1); // precise "now" (Dt is whole seconds)
        if s.seconds < 1.0 {
            ui.label(egui::RichText::new("Install the GTDCraft Chrome extension (extension/chrome in the GTDCraft folder: chrome://extensions › Load unpacked) to see where your browsing time goes, on task or off.").color(th().dim).size(12.5));
            return;
        }
        let mins = |x: f64| (x / 60.0).round() as i64;
        ui.label(format!(
            "{} browsing · {}% on task · {} distraction",
            hm(mins(s.seconds)),
            (100.0 * s.focus / s.seconds).round(),
            hm(mins(s.distraction))
        ));
        let (bar, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
        let mut x = bar.left();
        for (secs, c) in [(s.focus, series(0)), (s.neutral, other()), (s.distraction, series(1))] {
            let wd = bar.width() * (secs / s.seconds) as f32;
            ui.painter().rect_filled(Rect::from_min_size(pos2(x, bar.top()), vec2(wd, bar.height())), 3.0, c);
            x += wd;
        }
        ui.label(egui::RichText::new("■ on task  ■ neutral  ■ distraction").color(th().dim).size(11.5));
        ui.add_space(8.0);
        ui.columns(3, |cols| {
            cols[0].label(egui::RichText::new("Sites").font(bold(13.0)));
            for (d, secs) in s.sites.iter().take(8) {
                web_row(&mut cols[0], d, &hm(mins(*secs)), &format!("https://{d}"), None);
            }
            cols[1].label(egui::RichText::new("Searches").font(bold(13.0)));
            for (q, e) in s.searches.iter().rev().take(8) {
                web_row(&mut cols[1], q, e, &search_url(e, q), None);
            }
            cols[2].label(egui::RichText::new("Videos").font(bold(13.0)));
            if s.videos.is_empty() {
                cols[2].label(egui::RichText::new("No YouTube videos in this range").color(th().dim).size(12.5));
            }
            for v in s.videos.iter().take(6) {
                web_row(&mut cols[2], &v.title, &hm(mins(v.seconds)), &v.url, Some(&v.id));
            }
        });
    }
}

/// A search on the engine it was made on.
fn search_url(engine: &str, query: &str) -> String {
    let base = match engine.to_lowercase().as_str() {
        "duckduckgo" | "ddg" => "https://duckduckgo.com/?q=",
        "bing" => "https://www.bing.com/search?q=",
        "yandex" => "https://yandex.com/search/?text=",
        _ => "https://www.google.com/search?q=",
    };
    let words: Vec<String> = query.split_whitespace().map(|w| w.replace(['&', '#', '?', '%', '+'], " ").trim().replace(' ', "%20")).collect();
    format!("{base}{}", words.join("+"))
}

/// One line of web activity, clickable: a video's thumbnail (when it has one), its title, and the time on the
/// right. Hovering draws a link underline; clicking opens the page in the browser.
fn web_row(ui: &mut egui::Ui, label: &str, right: &str, url: &str, video: Option<&str>) {
    let cs = th();
    let w = ui.available_width();
    let thumb = video.and_then(crate::webact::thumb);
    let h = if video.is_some() { 52.0 } else { 22.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 5.0, cs.field);
    }
    let mut x = rect.left() + 4.0;
    if video.is_some() {
        // 16:9, big enough to tell one video from another at a glance
        let tr = Rect::from_min_size(pos2(x, rect.center().y - 20.5), vec2(72.0, 41.0));
        match &thumb {
            Some(path) => {
                egui::Image::from_uri(format!("file://{}", path.display())).corner_radius(4).paint_at(ui, tr);
            }
            None => {
                ui.painter().rect_filled(tr, 4.0, cs.pill);
                ui.painter().text(tr.center(), Align2::CENTER_CENTER, "▶", FontId::proportional(12.0), cs.faint);
            }
        }
        x = tr.right() + 8.0;
    }
    // the time or engine, flush right
    let p = ui.painter();
    let g_right = p.layout_no_wrap(right.to_string(), FontId::proportional(11.5), cs.dim);
    let right_x = rect.right() - 4.0 - g_right.size().x;
    p.galley(pos2(right_x, rect.center().y - g_right.size().y / 2.0), g_right, cs.dim);
    // the title, one line, cut to the space left
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), FontId::proportional(12.5), cs.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right_x - 8.0 - x).max(16.0));
    let g = p.layout_job(job);
    let y = rect.center().y - g.size().y / 2.0;
    p.galley(pos2(x, y), g.clone(), cs.text);
    if hovered {
        p.line_segment([pos2(x, y + g.size().y), pos2(x + g.size().x, y + g.size().y)], Stroke::new(1.0, cs.link));
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{label}\n{url}")).clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

fn chart_title(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t).font(bold(15.0)).color(th().text));
    ui.add_space(4.0);
}

/// Donut (asked for as a pie): <= 6 slices + "Other", legend of names with time and share beside it.
fn donut(ui: &mut egui::Ui, parts: &[(String, i64)], total: i64, w: f32) {
    let mut slices: Vec<(String, i64, Color32)> = parts.iter().take(6).enumerate().map(|(i, p)| (p.0.clone(), p.1, series(i))).collect();
    let rest: i64 = parts.iter().skip(6).map(|p| p.1).sum();
    if rest > 0 {
        slices.push(("Other".into(), rest, other()));
    }
    // the legend needs ~200 pt beside the 200 pt ring; narrower, it goes under the ring
    let stacked = w < 400.0;
    let h = if stacked && total > 0 { 200.0 + slices.len() as f32 * 24.0 + 6.0 } else { 200.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let p = ui.painter();
    if total == 0 {
        p.text(rect.center(), Align2::CENTER_CENTER, "No time tracked in this range", FontId::proportional(13.0), th().dim);
        return;
    }
    let c = if stacked { pos2(rect.center().x, rect.top() + 100.0) } else { pos2(rect.left() + 100.0, rect.center().y) };
    let (r_out, r_in) = (88.0f32, 56.0f32);
    let mut a0 = -std::f32::consts::FRAC_PI_2;
    let hover = resp.hover_pos();
    let mut hovered: Option<usize> = None;
    for (i, (_, m, col)) in slices.iter().enumerate() {
        let sweep = std::f32::consts::TAU * (*m as f32 / total as f32);
        let gap = 0.025f32.min(sweep / 3.0); // surface gap between slices
        let steps = ((sweep / 0.05) as usize).max(2);
        let (s0, s1) = (a0 + gap / 2.0, a0 + sweep - gap / 2.0);
        if let Some(h) = hover {
            let d = h - c;
            let ang = d.y.atan2(d.x);
            let ang = if ang < -std::f32::consts::FRAC_PI_2 { ang + std::f32::consts::TAU } else { ang };
            if d.length() >= r_in && d.length() <= r_out && ang >= a0 && ang < a0 + sweep {
                hovered = Some(i);
            }
        }
        let grow = if hovered == Some(i) { 4.0 } else { 0.0 };
        for k in 0..steps {
            let t0 = s0 + (s1 - s0) * k as f32 / steps as f32;
            let t1 = s0 + (s1 - s0) * (k + 1) as f32 / steps as f32;
            let quad = vec![
                c + (r_out + grow) * egui::Vec2::angled(t0),
                c + (r_out + grow) * egui::Vec2::angled(t1),
                c + r_in * egui::Vec2::angled(t1),
                c + r_in * egui::Vec2::angled(t0),
            ];
            p.add(egui::Shape::convex_polygon(quad, *col, Stroke::NONE));
        }
        a0 += sweep;
    }
    p.text(c - vec2(0.0, 8.0), Align2::CENTER_CENTER, hm(total), bold(17.0), th().text);
    p.text(c + vec2(0.0, 12.0), Align2::CENTER_CENTER, "total", FontId::proportional(11.5), th().dim);
    // legend: swatch + name (text ink) + time and share
    let (lx, ly) = if stacked { (rect.left() + 8.0, rect.top() + 212.0) } else { (c.x + r_out + 26.0, rect.top() + 18.0) };
    for (i, (n, m, col)) in slices.iter().enumerate() {
        let y = ly + i as f32 * 24.0;
        p.rect_filled(Rect::from_center_size(pos2(lx, y), vec2(10.0, 10.0)), 3.0, *col);
        let mut job = egui::text::LayoutJob::simple_singleline(n.clone(), FontId::proportional(13.0), th().text);
        job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - lx - 120.0).max(40.0));
        let g = p.layout_job(job);
        p.galley(pos2(lx + 12.0, y - g.size().y / 2.0), g, th().text);
        p.text(pos2(rect.right() - 4.0, y), Align2::RIGHT_CENTER, format!("{}  {:>3.0}%", hm(*m), *m as f32 * 100.0 / total as f32), FontId::monospace(12.0), th().dim);
    }
    if let Some(i) = hovered {
        let (n, m, _) = &slices[i];
        resp.on_hover_text_at_pointer(format!("{n}: {} ({:.0}%)", hm(*m), *m as f32 * 100.0 / total as f32));
    }
}

/// Ranked horizontal bars, largest first, one hue, values labelled.
fn ranked_bars(ui: &mut egui::Ui, parts: &[(String, i64)], w: f32) {
    let max = parts.iter().map(|p| p.1).max().unwrap_or(0).max(1);
    let shown: Vec<&(String, i64)> = parts.iter().take(7).collect();
    let (rect, _) = ui.allocate_exact_size(vec2(w, (shown.len().max(1) as f32) * 26.0 + 6.0), Sense::hover());
    if shown.is_empty() {
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, "No time tracked in this range", FontId::proportional(13.0), th().dim);
        return;
    }
    let label_w = 110.0;
    for (i, (n, m)) in shown.iter().enumerate() {
        let y = rect.top() + 4.0 + i as f32 * 26.0;
        let row = Rect::from_min_size(pos2(rect.left(), y), vec2(w, 24.0));
        let resp = ui.interact(row, ui.id().with(("rb", i)), Sense::hover());
        let mut job = egui::text::LayoutJob::simple_singleline(n.clone(), FontId::proportional(13.0), th().text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(label_w - 8.0);
        let g = ui.painter().layout_job(job);
        ui.painter().galley(pos2(rect.left(), y + 12.0 - g.size().y / 2.0), g, th().text);
        let bw = (w - label_w - 60.0) * (*m as f32 / max as f32);
        let bar = Rect::from_min_size(pos2(rect.left() + label_w, y + 5.0), vec2(bw.max(2.0), 14.0));
        // rounded data end (4px), square at the baseline
        ui.painter().rect_filled(bar, egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 }, if resp.hovered() { mix(series(0), th().text, 0.15) } else { series(0) });
        ui.painter().text(pos2(bar.right() + 6.0, y + 12.0), Align2::LEFT_CENTER, hm(*m), FontId::monospace(12.0), th().dim);
        resp.on_hover_text_at_pointer(format!("{n}: {}", hm(*m)));
    }
}

/// Every how many points to write a "10/29" date label so they never touch: each needs ~38 pt.
fn label_step(slot: f32) -> usize {
    (38.0 / slot.max(1.0)).ceil().max(1.0) as usize
}

/// Daily bars (single series, so no legend), recessive grid, hover readout per bar.
fn day_bars(ui: &mut egui::Ui, data: &[(NaiveDate, i64)], w: f32) {
    let h = 170.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h + 24.0), Sense::hover());
    let p = ui.painter();
    let plot = Rect::from_min_max(rect.min + vec2(44.0, 6.0), pos2(rect.right() - 6.0, rect.top() + h));
    let max_h = ((data.iter().map(|d| d.1).max().unwrap_or(0) as f32 / 60.0).ceil()).max(1.0);
    for k in 0..=max_h as i32 {
        let y = plot.bottom() - plot.height() * k as f32 / max_h;
        p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, th().line));
        p.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{k}h"), FontId::proportional(11.0), th().dim);
    }
    let n = data.len().max(1) as f32;
    let slot = plot.width() / n;
    let bw = (slot - 2.0).clamp(2.0, 28.0); // 2px surface gap between bars
    let label_every = label_step(slot);
    for (i, (day, m)) in data.iter().enumerate() {
        let x = plot.left() + slot * (i as f32 + 0.5);
        let bh = plot.height() * (*m as f32 / 60.0 / max_h);
        let bar = Rect::from_min_max(pos2(x - bw / 2.0, plot.bottom() - bh), pos2(x + bw / 2.0, plot.bottom()));
        let hit = Rect::from_min_max(pos2(x - slot / 2.0, plot.top()), pos2(x + slot / 2.0, plot.bottom()));
        let resp = ui.interact(hit, ui.id().with(("db", i)), Sense::hover());
        if *m > 0 {
            ui.painter().rect_filled(bar, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, if resp.hovered() { mix(series(0), th().text, 0.15) } else { series(0) });
        }
        if i % label_every == 0 {
            ui.painter().text(pos2(x, plot.bottom() + 12.0), Align2::CENTER_CENTER, day.format("%-m/%-d").to_string(), FontId::proportional(11.0), th().dim);
        }
        resp.on_hover_text_at_pointer(format!("{}: {}", day.format("%a, %b %-d"), hm(*m)));
    }
    p.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0, th().dim));
}

/// Weekly trend line: 2px line, 8px markers, crosshair + readout on hover.
fn trend(ui: &mut egui::Ui, data: &[(NaiveDate, i64)], w: f32) {
    let h = 150.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + 24.0), Sense::hover());
    let p = ui.painter();
    let plot = Rect::from_min_max(rect.min + vec2(44.0, 8.0), pos2(rect.right() - 12.0, rect.top() + h));
    let max_h = ((data.iter().map(|d| d.1).max().unwrap_or(0) as f32 / 60.0 / 4.0).ceil() * 4.0).max(4.0);
    for k in 0..=4 {
        let v = max_h * k as f32 / 4.0;
        let y = plot.bottom() - plot.height() * k as f32 / 4.0;
        p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, th().line));
        p.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{v:.0}h"), FontId::proportional(11.0), th().dim);
    }
    let n = data.len().max(2) as f32 - 1.0;
    let pts: Vec<egui::Pos2> = data.iter().enumerate().map(|(i, (_, m))| pos2(plot.left() + plot.width() * i as f32 / n, plot.bottom() - plot.height() * (*m as f32 / 60.0 / max_h))).collect();
    p.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, series(0))));
    for (i, pt) in pts.iter().enumerate() {
        p.circle_filled(*pt, 4.0, series(0));
        p.circle_stroke(*pt, 4.0, Stroke::new(2.0, th().bg)); // surface ring
        if i % label_step(plot.width() / n) == 0 || i + 1 == pts.len() {
            p.text(pos2(pt.x, plot.bottom() + 12.0), Align2::CENTER_CENTER, data[i].0.format("%-m/%-d").to_string(), FontId::proportional(11.0), th().dim);
        }
    }
    if let Some(hp) = resp.hover_pos().filter(|hp| plot.expand(10.0).contains(*hp)) {
        let i = (((hp.x - plot.left()) / plot.width() * n).round() as usize).min(data.len() - 1);
        p.line_segment([pos2(pts[i].x, plot.top()), pos2(pts[i].x, plot.bottom())], Stroke::new(1.0, th().dim));
        p.circle_filled(pts[i], 5.0, series(0));
        resp.on_hover_text_at_pointer(format!("Week of {}: {}", data[i].0.format("%b %-d"), hm(data[i].1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click on a web-activity row asks the system to open its URL (what "the links are clickable" means).
    #[test]
    fn clicking_a_web_row_opens_its_url() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0));
        let click_at = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let draw = |raw| {
            ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    web_row(ui, "youtube.com", "0:42", "https://youtube.com", None);
                });
            })
        };
        // one frame to lay the row out, then press and release inside it
        draw(click_at(vec![]));
        let at = egui::pos2(200.0, 20.0);
        for pressed in [true, false] {
            let out = draw(click_at(vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
            ]));
            let opened: Vec<String> = out
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::OpenUrl(u) => Some(u.url.clone()),
                    _ => None,
                })
                .collect();
            if !pressed {
                assert_eq!(opened, vec!["https://youtube.com".to_string()]);
            }
        }
    }

    #[test]
    fn searches_link_to_their_engine() {
        assert_eq!(search_url("Google", "egui ios"), "https://www.google.com/search?q=egui+ios");
        assert_eq!(search_url("DuckDuckGo", "a b"), "https://duckduckgo.com/?q=a+b");
        assert_eq!(search_url("Bing", "x"), "https://www.bing.com/search?q=x");
    }
}

/// iPhone/iPad: the Focus Live Activity (Lock Screen, Dynamic Island) — ActivityKit through ios/LiveActivity; nothing
/// elsewhere.
pub(crate) mod live {
    use crate::model::Dt;
    use chrono::TimeZone;

    #[cfg(target_os = "ios")]
    extern "C" {
        fn gtd_live_start(title: *const std::os::raw::c_char, start: f64, end: f64, pomodoro: bool);
        fn gtd_live_end();
        fn gtd_widgets_reload();
    }

    /// iPhone/iPad: ask WidgetKit to refresh the Home Screen widgets now.
    #[cfg(target_os = "ios")]
    pub fn reload_widgets() {
        unsafe { gtd_widgets_reload() }
    }

    fn unix(t: Dt) -> f64 {
        chrono::Local.from_local_datetime(&t).earliest().map_or(0.0, |x| x.timestamp() as f64)
    }

    /// A Pomodoro (`end` set, counting down) or a clock-in (counting up).
    pub fn start(title: &str, start: Dt, end: Option<Dt>) {
        let (_s, _e) = (unix(start), end.map_or(0.0, unix));
        #[cfg(target_os = "ios")]
        {
            let t = std::ffi::CString::new(title.replace('\0', "")).unwrap_or_default();
            unsafe { gtd_live_start(t.as_ptr(), _s, _e, end.is_some()) };
        }
        #[cfg(not(target_os = "ios"))]
        let _ = title;
    }

    pub fn end() {
        #[cfg(target_os = "ios")]
        unsafe {
            gtd_live_end()
        };
    }
}
