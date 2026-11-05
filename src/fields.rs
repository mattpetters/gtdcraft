//! Click-to-edit fields on the selected row (Project, Tags, Duration, Defer, Planned, Due, Repeat) as a
//! field strip; the quick-edit keys (P, T, ⇧T, ⇧D, ⇧P, D, R) open the same editors.
use crate::*;
use eframe::egui::{self, vec2, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Field {
    Project,
    Tags,
    Duration,
    Defer,
    Planned,
    Due,
    Repeat,
}

impl Field {
    pub(crate) fn from_action(a: keys::Action) -> Option<Field> {
        use keys::Action::*;
        Some(match a {
            EditProject => Field::Project,
            EditTags => Field::Tags,
            EditDuration => Field::Duration,
            EditDefer => Field::Defer,
            EditPlanned => Field::Planned,
            EditDue => Field::Due,
            EditRepeat => Field::Repeat,
            _ => return None,
        })
    }
}

impl Gtd {
    pub(crate) fn open_field(&mut self, id: Id, f: Field) {
        self.sel = Some(Sel::Task(id));
        // iPhone/iPad: dates use iOS's own picker sheet, as in the inspector (the person: "still not able to save
        // planned fields" — the typed popover saved only on Return)
        #[cfg(target_os = "ios")]
        if let Some(slot) = [Field::Defer, Field::Planned, Field::Due].iter().position(|x| *x == f) {
            let st = &self.store.settings;
            let t = self.store.task(id);
            let (cur, dt, label) = match slot {
                0 => (t.and_then(|t| t.defer), st.default_defer_time, "Defer"),
                1 => (t.and_then(|t| t.planned), st.default_planned_time, "Planned"),
                _ => (t.and_then(|t| t.due), st.default_due_time, "Due"),
            };
            crate::datepick::present(&format!("{label} Date"), slot, id, cur, dt);
            return;
        }
        let t = self.store.task(id);
        self.field_buf = match f {
            Field::Defer => t.and_then(|t| t.defer).map(|x| x.format("%Y-%m-%d %H:%M").to_string()),
            Field::Planned => t.and_then(|t| t.planned).map(|x| x.format("%Y-%m-%d %H:%M").to_string()),
            Field::Due => t.and_then(|t| t.due).map(|x| x.format("%Y-%m-%d %H:%M").to_string()),
            Field::Duration => t.and_then(|t| t.duration_min).map(|m| format!("{m}m")),
            _ => None,
        }
        .unwrap_or_default();
        self.field_open = Some((id, f));
        self.field_focus = true;
    }

    /// The editor popover under the clicked field.
    pub(crate) fn field_popup(&mut self, ctx: &egui::Context, now: Dt) {
        let Some((id, f)) = self.field_open else { return };
        if self.store.task(id).is_none() {
            self.field_open = None;
            return;
        }
        let anchor = self.strip_rects.get(&(id, f)).copied().unwrap_or_else(|| Rect::from_center_size(ctx.content_rect().center() - vec2(0.0, 120.0), vec2(1.0, 1.0)));
        let mut close = false;
        let mut changed = false;
        let tags_before: Vec<Id> = self.store.task(id).map(|t| t.tags.clone()).unwrap_or_default();
        let st = self.store.settings.clone();
        // listed by colon path ("Folder : Project", "Parent : Child")
        let mut projects: Vec<(Id, String)> = self.store.projects.iter().filter(|p| matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).map(|p| (p.id, self.store.project_path(p.id))).collect();
        let mut tags: Vec<(Id, String)> = self.store.tags.iter().map(|g| (g.id, self.store.tag_path(g.id))).collect();
        tags.sort_by_key(|a| a.1.to_lowercase());
        // most recently used first until something is typed (then the matches, alphabetically)
        if self.field_buf.trim().is_empty() {
            self.store.by_recent_use(&mut projects, |t| t.project.into_iter().collect());
            self.store.by_recent_use(&mut tags, |t| t.tags.clone());
        }
        // under the field — or, when the iPhone keyboard would cover it, sitting just above the keyboard (the person:
        // "keyboard covering tag selector dropdown")
        let kb_top = crate::keyboard_top(ctx);
        let last_h = ctx.data(|d| d.get_temp::<f32>(egui::Id::new("field-popup-h"))).unwrap_or(300.0);
        let below = anchor.left_bottom() + vec2(-8.0, 6.0);
        let (pos, pivot) = if crate::keyboard::height() > 0.0 && below.y + last_h > kb_top - 8.0 {
            (pos2(below.x, kb_top - 8.0), egui::Align2::LEFT_BOTTOM)
        } else {
            (below, egui::Align2::LEFT_TOP)
        };
        let max_h = (if pivot == egui::Align2::LEFT_BOTTOM { kb_top - ctx.content_rect().top() - 60.0 } else { 400.0f32 }).max(160.0);
        let area = egui::Area::new(egui::Id::new("field-popup")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            ui.set_max_height(max_h);
            egui::Frame::popup(ui.style()).fill(th().inspector).corner_radius(10).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(270.0);
                // Return, or a soft keyboard's newline
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.contains('\n'))));
                self.field_buf.retain(|c| c != '\n');
                let focus = std::mem::take(&mut self.field_focus);
                match f {
                    Field::Project | Field::Tags => {
                        let r = ui.add(crate::nativetext::Edit::singleline(&mut self.field_buf).hint_text(if f == Field::Project { "Project" } else { "Tag" }).desired_width(f32::INFINITY));
                        if focus {
                            r.request_focus();
                        }
                        let typed = self.field_buf.trim().to_string();
                        let names_match = |path: &str| path_eq(path, &typed) || path_parts(path).last().is_some_and(|n| n.eq_ignore_ascii_case(&typed));
                        egui::ScrollArea::vertical().max_height((max_h - 70.0).clamp(120.0, 220.0)).show(ui, |ui| {
                            if f == Field::Project {
                                let mut list: Vec<(Option<Id>, String)> = vec![(None, "Inbox (no project)".into())];
                                list.extend(projects.iter().map(|(i, n)| (Some(*i), n.clone())));
                                list.retain(|x| typed.is_empty() || path_matches(&x.1, &typed));
                                let exact = list.iter().any(|x| x.0.is_some() && names_match(&x.1));
                                let cur = self.store.task(id).and_then(|t| t.project);
                                // a name that isn't there yet: create it ("Folder : Project" files it in a folder)
                                if !typed.is_empty() && !exact {
                                    let label = format!("New project “{}”", path_parts(&typed).join(" : "));
                                    if ui.add(egui::Button::new(egui::RichText::new(label).color(th().accent))).clicked() || (enter && list.is_empty()) {
                                        let pid = self.store.project_for_path(&typed);
                                        self.store.set_project(id, pid);
                                        changed = true;
                                        close = true;
                                    }
                                }
                                for (k, (pid, name)) in list.iter().enumerate() {
                                    if ui.selectable_label(cur == *pid, name).clicked() || (enter && k == 0 && !close) {
                                        self.store.set_project(id, *pid);
                                        changed = true;
                                        close = true;
                                    }
                                }
                            } else {
                                let shown: Vec<&(Id, String)> = tags.iter().filter(|x| typed.is_empty() || path_matches(&x.1, &typed)).collect();
                                let exact = shown.iter().find(|x| names_match(&x.1)).map(|x| x.0);
                                if let Some(t) = self.store.task_mut(id) {
                                    for (gid, name) in &shown {
                                        let mut on = t.tags.contains(gid);
                                        if ui.checkbox(&mut on, name.as_str()).changed() {
                                            if on { t.tags.push(*gid) } else { t.tags.retain(|x| x != gid) }
                                            t.modified = now;
                                            changed = true;
                                        }
                                    }
                                }
                                let mut add: Option<Id> = None;
                                if !typed.is_empty() && exact.is_none() {
                                    let label = format!("New tag “{}”", path_parts(&typed).join(" : "));
                                    if ui.add(egui::Button::new(egui::RichText::new(label).color(th().accent))).clicked() || enter {
                                        add = self.store.tag_for_path(&typed); // "Parent : Child" nests it
                                    }
                                } else if enter {
                                    match exact {
                                        Some(g) => add = Some(g),
                                        None => close = true, // Enter on an empty field closes
                                    }
                                }
                                if let Some(g) = add {
                                    if let Some(t) = self.store.task_mut(id) {
                                        if !t.tags.contains(&g) {
                                            t.tags.push(g);
                                        }
                                        t.modified = now;
                                    }
                                    self.field_buf.clear();
                                    changed = true;
                                }
                            }
                        });
                    }
                    Field::Duration => {
                        let r = ui.add(crate::nativetext::Edit::singleline(&mut self.field_buf).hint_text("15m, 1h, 1h30m").desired_width(f32::INFINITY));
                        if focus {
                            r.request_focus();
                        }
                        let parsed = dates::parse_minutes(&self.field_buf);
                        match &parsed {
                            Ok(Some(m)) => ui.label(egui::RichText::new(format!("= {}", stats::hm(*m as i64))).color(th().dim)),
                            Ok(None) => ui.label(egui::RichText::new("= None").color(th().dim)),
                            Err(e) => ui.label(egui::RichText::new(e).color(th().overdue).size(12.0)),
                        };
                        let mut pick: Option<Option<u32>> = None;
                        ui.horizontal_wrapped(|ui| {
                            for (l, m) in [("5m", 5), ("15m", 15), ("30m", 30), ("1h", 60), ("2h", 120)] {
                                if ui.button(l).clicked() {
                                    pick = Some(Some(m));
                                }
                            }
                            if ui.button("Clear").clicked() {
                                pick = Some(None);
                            }
                        });
                        if pick.is_none() && enter {
                            if let Ok(v) = parsed {
                                pick = Some(v);
                            }
                        }
                        if let (Some(v), Some(t)) = (pick, self.store.task_mut(id)) {
                            t.duration_min = v;
                            t.modified = now;
                            changed = true;
                            close = true;
                        }
                    }
                    Field::Defer | Field::Planned | Field::Due => {
                        let default_time = match f {
                            Field::Defer => st.default_defer_time,
                            Field::Planned => st.default_planned_time,
                            _ => st.default_due_time,
                        };
                        let r = ui.add(crate::nativetext::Edit::singleline(&mut self.field_buf).hint_text("today, tonight, tomorrow 5pm, next fri, +3h, nov 15").desired_width(f32::INFINITY));
                        if focus {
                            r.request_focus();
                        }
                        let parsed = dates::parse_when(&self.field_buf, now, default_time);
                        match &parsed {
                            Ok(Some(x)) => ui.label(egui::RichText::new(format!("= {}", x.format("%a, %b %-d, %Y  %-I:%M %p"))).color(th().dim)),
                            Ok(None) => ui.label(egui::RichText::new("= None").color(th().dim)),
                            Err(e) => ui.label(egui::RichText::new(e).color(th().overdue).size(12.0)),
                        };
                        let mut pick: Option<Option<Dt>> = None;
                        ui.horizontal_wrapped(|ui| {
                            for (l, days) in [("Today", 0), ("Tomorrow", 1), ("Next Week", 7)] {
                                if ui.button(l).clicked() {
                                    pick = Some(Some((now.date() + Duration::days(days)).and_time(default_time)));
                                }
                            }
                            if ui.button("Clear").clicked() {
                                pick = Some(None);
                            }
                        });
                        if pick.is_none() && enter {
                            if let Ok(v) = parsed {
                                pick = Some(v);
                            }
                        }
                        if let (Some(v), Some(t)) = (pick, self.store.task_mut(id)) {
                            match f {
                                Field::Defer => t.defer = v,
                                Field::Planned => t.planned = v,
                                _ => t.due = v,
                            }
                            t.modified = now;
                            changed = true;
                            close = true;
                            self.date_buf = Default::default();
                        }
                    }
                    Field::Repeat => {
                        if let Some(t) = self.store.task_mut(id) {
                            let mut from_completion = t.repeat.is_some_and(|r| r.schedule == RepeatSchedule::FromCompletion);
                            for (l, unit) in [("Every day", Some(RepeatUnit::Day)), ("Every week", Some(RepeatUnit::Week)), ("Every month", Some(RepeatUnit::Month)), ("Every year", Some(RepeatUnit::Year)), ("Don't repeat", None)] {
                                let on = match (t.repeat, unit) {
                                    (Some(r), Some(u)) => r.unit == u && r.every == 1,
                                    (None, None) => true,
                                    _ => false,
                                };
                                if ui.selectable_label(on, l).clicked() {
                                    t.repeat = unit.map(|u| Repeat { unit: u, schedule: if from_completion { RepeatSchedule::FromCompletion } else { RepeatSchedule::Regularly }, ..Repeat::default() });
                                    t.modified = now;
                                    changed = true;
                                    close = true;
                                }
                            }
                            if ui.checkbox(&mut from_completion, "Count from completion").changed() {
                                if let Some(r) = t.repeat.as_mut() {
                                    r.schedule = if from_completion { RepeatSchedule::FromCompletion } else { RepeatSchedule::Regularly };
                                    t.modified = now;
                                    changed = true;
                                }
                            }
                            ui.label(egui::RichText::new("More options in the inspector's Repeat section.").color(th().dim).size(11.5));
                        }
                    }
                }
            });
        });
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("field-popup-h"), area.response.rect.height()));
        let outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p));
        // a tap elsewhere keeps a typed date or duration that reads, as ending editing does
        if outside && !close && !self.field_buf.trim().is_empty() {
            let st = self.store.settings.clone();
            let typed = self.field_buf.clone();
            if let Some(t) = self.store.task_mut(id) {
                let dt = match f {
                    Field::Defer => Some(st.default_defer_time),
                    Field::Planned => Some(st.default_planned_time),
                    Field::Due => Some(st.default_due_time),
                    _ => None,
                };
                match (f, dt.map(|d| dates::parse_when(&typed, now, d))) {
                    (Field::Defer, Some(Ok(v))) => t.defer = v,
                    (Field::Planned, Some(Ok(v))) => t.planned = v,
                    (Field::Due, Some(Ok(v))) => t.due = v,
                    (Field::Duration, _) => {
                        if let Ok(v) = dates::parse_minutes(&typed) {
                            t.duration_min = v;
                        }
                    }
                    _ => {}
                }
                t.modified = now;
                changed = true;
                self.date_buf = Default::default();
            }
        }
        if close || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.field_open = None;
        }
        if changed {
            self.copy_field_to_picked(id, f, &tags_before, now);
            self.touch();
        }
    }

    /// With several actions selected, an edit made in the field editor applies to all of them
    /// (tags: the ones added or removed, so each keeps its other tags).
    fn copy_field_to_picked(&mut self, from: Id, f: Field, tags_before: &[Id], now: Dt) {
        let Some(src) = self.store.task(from).cloned() else { return };
        let others: Vec<Id> = self.picked.iter().copied().filter(|x| *x != from).collect();
        for other in others {
            if f == Field::Project {
                self.store.set_project(other, src.project);
                continue;
            }
            let Some(t) = self.store.task_mut(other) else { continue };
            match f {
                Field::Tags => {
                    for g in src.tags.iter().filter(|g| !tags_before.contains(g)) {
                        if !t.tags.contains(g) {
                            t.tags.push(*g);
                        }
                    }
                    t.tags.retain(|g| !tags_before.contains(g) || src.tags.contains(g)); // drop only the removed ones
                }
                Field::Duration => t.duration_min = src.duration_min,
                Field::Defer => t.defer = src.defer,
                Field::Planned => t.planned = src.planned,
                Field::Due => t.due = src.due,
                Field::Repeat => t.repeat = src.repeat,
                Field::Project => {}
            }
            t.modified = now;
        }
    }
}
