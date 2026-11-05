//! Perspective editor (name + colour, "Contents" rule tree, Structure, order),
//! "Describe it" AI drafting, Settings › Perspectives / Keyboard / Agent tabs, and the Linear issue card.
use crate::keys::{self, Action};
use crate::rules::{Rule, LEAF_KINDS};
use crate::*;
use eframe::egui::{self, vec2, Color32, Sense, Stroke};

struct Names {
    tags: Vec<(Id, String)>,
    projects: Vec<(Id, String)>,
    folders: Vec<String>,
}

enum RuleOp {
    Keep,
    Delete,
    Negate,
    Toggle,
}

fn rule_brief(r: &Rule) -> String {
    match r {
        Rule::All { .. } => "All of the following".into(),
        Rule::Any { .. } => "Any of the following".into(),
        Rule::None { .. } => "None of the following".into(),
        other => format!("{other:?}").split(['{', '(']).next().unwrap_or("").trim().to_string(),
    }
}

const FIELDS: [&str; 6] = ["due", "defer", "planned", "added", "modified", "completed"];

fn rule_ui(ui: &mut egui::Ui, r: &mut Rule, names: &Names, path: &mut Vec<usize>, root: bool, changed: &mut bool) -> RuleOp {
    let mut op = RuleOp::Keep;
    let id = egui::Id::new(("rule", path.clone()));
    ui.horizontal(|ui| {
        let combo = |ui: &mut egui::Ui, salt: &str, cur: &mut String, opts: &[&str], changed: &mut bool| {
            egui::ComboBox::from_id_salt((id, salt)).selected_text(cur.replace('_', " ")).show_ui(ui, |ui| {
                for o in opts {
                    *changed |= ui.selectable_value(cur, o.to_string(), o.replace('_', " ")).changed();
                }
            });
        };
        match r {
            Rule::All { .. } | Rule::Any { .. } | Rule::None { .. } => {
                let mut which = match r {
                    Rule::All { .. } => 0,
                    Rule::Any { .. } => 1,
                    _ => 2,
                };
                let before = which;
                egui::ComboBox::from_id_salt((id, "grp")).selected_text(["All of the following:", "Any of the following:", "None of the following:"][which]).show_ui(ui, |ui| {
                    ui.selectable_value(&mut which, 0, "All of the following:");
                    ui.selectable_value(&mut which, 1, "Any of the following:");
                    ui.selectable_value(&mut which, 2, "None of the following:");
                });
                if which != before {
                    let rules = r.children_mut().map(std::mem::take).unwrap_or_default();
                    *r = match which {
                        0 => Rule::All { rules },
                        1 => Rule::Any { rules },
                        _ => Rule::None { rules },
                    };
                    *changed = true;
                }
            }
            Rule::Availability { value } => {
                ui.label("Availability:");
                combo(ui, "av", value, &["first", "available", "remaining", "completed", "dropped"], changed);
            }
            Rule::Status { value } => {
                ui.label("Status:");
                combo(ui, "st", value, &["flagged", "due_soon", "overdue", "due_soon_or_flagged"], changed);
            }
            Rule::Tag { id: tag } => {
                ui.label("Has tag:");
                let cur = tag.and_then(|t| names.tags.iter().find(|x| x.0 == t)).map(|x| x.1.clone()).unwrap_or_else(|| "Untagged".into());
                egui::ComboBox::from_id_salt((id, "tag")).selected_text(cur).show_ui(ui, |ui| {
                    *changed |= ui.selectable_value(tag, None, "Untagged").changed();
                    for (tid, n) in &names.tags {
                        *changed |= ui.selectable_value(tag, Some(*tid), n).changed();
                    }
                });
            }
            Rule::Project { id: proj } => {
                ui.label("Contained within project:");
                let cur = proj.and_then(|t| names.projects.iter().find(|x| x.0 == t)).map(|x| x.1.clone()).unwrap_or_else(|| "Inbox".into());
                egui::ComboBox::from_id_salt((id, "proj")).selected_text(cur).show_ui(ui, |ui| {
                    *changed |= ui.selectable_value(proj, None, "Inbox").changed();
                    for (pid, n) in &names.projects {
                        *changed |= ui.selectable_value(proj, Some(*pid), n).changed();
                    }
                });
            }
            Rule::Folder { name } => {
                ui.label("Contained within folder:");
                let opts: Vec<&str> = names.folders.iter().map(String::as_str).collect();
                combo(ui, "folder", name, &opts, changed);
            }
            Rule::DateWithin { field, days } => {
                ui.label("Has a");
                combo(ui, "f", field, &FIELDS, changed);
                ui.label("date within");
                *changed |= ui.add(egui::DragValue::new(days).range(-3650..=3650)).changed();
                ui.label(egui::RichText::new("days (− = past)").color(th().dim));
            }
            Rule::HasDate { field, has } => {
                let mut h = if *has { "has".to_string() } else { "has no".to_string() };
                combo(ui, "h", &mut h, &["has", "has no"], changed);
                *has = h == "has";
                combo(ui, "f", field, &FIELDS, changed);
                ui.label("date");
            }
            Rule::Duration { max_min } => {
                ui.label("Estimated duration");
                let mut unest = max_min.is_none();
                if ui.checkbox(&mut unest, "unestimated").changed() {
                    *max_min = if unest { None } else { Some(15) };
                    *changed = true;
                }
                if let Some(m) = max_min.as_mut() {
                    ui.label("≤");
                    *changed |= ui.add(egui::DragValue::new(m).range(1..=1440).suffix(" min")).changed();
                }
            }
            Rule::Text { contains } => {
                ui.label("Name or note contains");
                *changed |= ui.add(crate::nativetext::Edit::singleline(contains).desired_width(140.0)).changed();
            }
            Rule::Repeats { value } => {
                *changed |= ui.checkbox(value, "Is repeating").changed();
            }
            Rule::HasNote { value } => {
                *changed |= ui.checkbox(value, "Has a note").changed();
            }
            Rule::Tags { ids, all } => {
                let mut mode = if *all { "all".to_string() } else { "any".to_string() };
                ui.label("Has");
                combo(ui, "tagsmode", &mut mode, &["any", "all"], changed);
                *all = mode == "all";
                ui.label("of tags:");
                ui.menu_button(if ids.is_empty() { "choose…".to_string() } else { ids.iter().filter_map(|g| names.tags.iter().find(|x| x.0 == *g)).map(|x| x.1.clone()).collect::<Vec<_>>().join(", ") }, |ui| {
                    for (tid, n) in &names.tags {
                        let mut on = ids.contains(tid);
                        if ui.checkbox(&mut on, n).changed() {
                            if on { ids.push(*tid) } else { ids.retain(|x| x != tid) }
                            *changed = true;
                        }
                    }
                });
            }
            Rule::TagStatus { status } => {
                ui.label("Has a tag that is");
                combo(ui, "ts", status, &["active", "on_hold"], changed);
            }
            Rule::ProjectStatus { status } => {
                ui.label("Has a project that is");
                combo(ui, "ps", status, &["active", "on_hold", "completed", "dropped"], changed);
            }
            Rule::SingleActions => {
                ui.label("Is in a single actions list");
            }
            Rule::ProjectOrGroup => {
                ui.label("Is a project or action group");
            }
            Rule::Leaf => {
                ui.label("Is an action (no sub-actions)");
            }
            Rule::DateOn { field, day } => {
                ui.label("Has a");
                combo(ui, "f", field, &FIELDS, changed);
                ui.label("date of");
                let mut w = ["yesterday", "today", "tomorrow"][(*day + 1).clamp(0, 2) as usize].to_string();
                combo(ui, "day", &mut w, &["yesterday", "today", "tomorrow"], changed);
                *day = match w.as_str() { "yesterday" => -1, "tomorrow" => 1, _ => 0 };
            }
            Rule::DateBefore { field, date } | Rule::DateAfter { field, date } => {
                ui.label("Has a");
                combo(ui, "f", field, &FIELDS, changed);
                ui.label("date before/after");
                let mut txt = date.to_string();
                if ui.add(crate::nativetext::Edit::singleline(&mut txt).desired_width(96.0)).changed() {
                    if let Ok(nd) = chrono::NaiveDate::parse_from_str(&txt, "%Y-%m-%d") {
                        *date = nd;
                        *changed = true;
                    }
                }
            }
            Rule::Off { rule } => {
                ui.label(egui::RichText::new(format!("Off: {}", rule_brief(rule))).color(th().dim).italics());
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("⋯", |ui| {
                if !root && ui.button("Delete").clicked() {
                    op = RuleOp::Delete;
                    ui.close();
                }
                if !root && ui.button("Exclude matches (None of)").clicked() {
                    op = RuleOp::Negate;
                    ui.close();
                }
                if !root && ui.button(if matches!(r, Rule::Off { .. }) { "Turn On" } else { "Turn Off" }).clicked() {
                    op = RuleOp::Toggle;
                    ui.close();
                }
                if let Rule::DateBefore { field, date } = r {
                    if ui.button("Switch to “after”").clicked() {
                        *r = Rule::DateAfter { field: field.clone(), date: *date };
                        *changed = true;
                        ui.close();
                    }
                } else if let Rule::DateAfter { field, date } = r {
                    if ui.button("Switch to “before”").clicked() {
                        *r = Rule::DateBefore { field: field.clone(), date: *date };
                        *changed = true;
                        ui.close();
                    }
                }
            });
        });
    });
    if let Some(children) = r.children_mut() {
        ui.indent(id, |ui| {
            let mut remove = None;
            let mut negate = None;
            let mut toggle = None;
            for (i, c) in children.iter_mut().enumerate() {
                path.push(i);
                match rule_ui(ui, c, names, path, false, changed) {
                    RuleOp::Delete => remove = Some(i),
                    RuleOp::Negate => negate = Some(i),
                    RuleOp::Toggle => toggle = Some(i),
                    RuleOp::Keep => {}
                }
                path.pop();
            }
            if let Some(i) = remove {
                children.remove(i);
                *changed = true;
            }
            if let Some(i) = toggle {
                let r = children.remove(i);
                children.insert(i, match r {
                    Rule::Off { rule } => *rule,
                    other => Rule::Off { rule: Box::new(other) },
                });
                *changed = true;
            }
            if let Some(i) = negate {
                let inner = children.remove(i);
                children.insert(i, Rule::None { rules: vec![inner] });
                *changed = true;
            }
            ui.menu_button(egui::RichText::new("Add a new rule").color(th().dim), |ui| {
                for (k, label) in LEAF_KINDS {
                    if ui.button(label).clicked() {
                        children.push(Rule::new_leaf(k));
                        *changed = true;
                        ui.close();
                    }
                }
            });
        });
    }
    op
}

impl Gtd {
    fn names(&self) -> Names {
        let mut folders: Vec<String> = self.store.projects.iter().map(|p| p.folder.clone()).filter(|f| !f.is_empty()).collect();
        folders.sort();
        folders.dedup();
        Names {
            tags: self.store.tags.iter().map(|g| (g.id, g.name.clone())).collect(),
            projects: self.store.projects.iter().map(|p| (p.id, p.name.clone())).collect(),
            folders,
        }
    }

    pub(crate) fn new_custom_perspective(&mut self) -> String {
        let next = self.store.settings.perspectives.iter().filter_map(|p| p.key.strip_prefix("custom-")?.parse::<u32>().ok()).max().map(|m| m + 1).unwrap_or(1);
        let key = format!("custom-{next}");
        self.store.settings.perspectives.push(PerspectiveCfg {
            key: key.clone(),
            name: "New Perspective".into(),
            hue: "cyan".into(),
            filter: Some(Rule::default_root()),
            group: Some("project".into()),
            ..PerspectiveCfg::blank()
        });
        self.touch();
        key
    }

    fn poll_ai(&mut self) {
        let Some(rx) = &self.ai_rx else { return };
        let Ok(res) = rx.try_recv() else { return };
        self.ai_rx = None;
        let Some(key) = self.editor.clone() else { return };
        match res {
            Ok(draft) => {
                let rule = draft.to_rule(&self.store);
                if let Some(p) = self.store.settings.perspectives.iter_mut().find(|p| p.key == key) {
                    if p.name == "New Perspective" || p.name.trim().is_empty() {
                        p.name = draft.name.clone();
                    }
                    p.hue = draft.hue.clone();
                    p.filter = Some(rule);
                    p.rules = None;
                    p.group = Some(draft.group.clone());
                    p.sort = Some(draft.sort.clone());
                    p.board = draft.board;
                }
                self.ai_msg = format!("Drafted “{}”. Check the rules below.", draft.name);
                self.touch();
            }
            Err(e) => self.ai_msg = e,
        }
    }

    pub(crate) fn perspective_editor(&mut self, ctx: &egui::Context) {
        self.poll_ai();
        let Some(key) = self.editor.clone() else { return };
        let names = self.names();
        let Some(idx) = self.store.settings.perspectives.iter().position(|p| p.key == key) else {
            self.editor = None;
            return;
        };
        let provider = match self.store.settings.ai_provider.as_str() {
            "openai" => "OpenAI-compatible model",
            "acp" => "ACP agent",
            _ => "Claude",
        };
        let mut open = true;
        let mut changed = false;
        let mut delete = false;
        let mut generate = false;
        egui::Window::new("Edit Perspective").open(&mut open).collapsible(false).default_width(560.0).default_pos([180.0, 90.0]).show(ctx, |ui| {
            let p = &mut self.store.settings.perspectives[idx];
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                ui.painter().rect_filled(r, 7.0, hue(&p.hue));
                changed |= ui.add(crate::nativetext::Edit::singleline(&mut p.name).desired_width(330.0).font(egui::FontId::proportional(16.0))).changed();
                for h in theme::HUES {
                    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 7.0, hue(h));
                    if p.hue == h {
                        ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, th().text));
                    }
                    if resp.on_hover_text(h).clicked() {
                        p.hue = h.into();
                        changed = true;
                    }
                }
            });
            if p.is_custom() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Describe it").font(bold(14.0)));
                ui.add(crate::nativetext::Edit::multiline(&mut self.ai_text).hint_text("e.g. “flagged or due this week in my Work folder, quick ones under 15 minutes, as a board by project”").desired_rows(2).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    let busy = self.ai_rx.is_some();
                    if ui.add_enabled(!busy && !self.ai_text.trim().is_empty(), egui::Button::new(format!("Draft rules with {provider}"))).clicked() {
                        generate = true;
                    }
                    if busy {
                        ui.spinner();
                    }
                    if !self.ai_msg.is_empty() {
                        ui.label(egui::RichText::new(&self.ai_msg).color(th().dim).size(12.0));
                    }
                });
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Contents:").font(bold(14.0)));
                let mut root = p.rule();
                egui::Frame::new().fill(th().field).corner_radius(10).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut c = false;
                    rule_ui(ui, &mut root, &names, &mut vec![], true, &mut c);
                    if c {
                        changed = true;
                    }
                });
                if changed || p.filter.is_none() {
                    p.filter = Some(root);
                    p.rules = None;
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Structure:").font(bold(14.0)));
                let mut g = p.group_by();
                ui.horizontal(|ui| {
                    let flexible = g == "none";
                    if ui.selectable_label(flexible, "Flexible").clicked() && !flexible {
                        g = "none".into();
                    }
                    if ui.selectable_label(!flexible, "Organized").clicked() && flexible {
                        g = "project".into();
                    }
                    if g != "none" {
                        ui.label("by");
                        egui::ComboBox::from_id_salt("group").selected_text(g.clone()).show_ui(ui, |ui| {
                            for o in ["project", "tag", "folder", "due", "flagged"] {
                                ui.selectable_value(&mut g, o.to_string(), o);
                            }
                        });
                    }
                });
                if Some(&g) != p.group.as_ref() {
                    p.group = Some(g);
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Initial order");
                    let mut s = p.sort.clone().unwrap_or_else(|| "none".into());
                    egui::ComboBox::from_id_salt("sort").selected_text(s.clone()).show_ui(ui, |ui| {
                        for o in ["none", "due", "defer", "planned", "added", "modified", "name", "flagged"] {
                            changed |= ui.selectable_value(&mut s, o.to_string(), o).changed();
                        }
                    });
                    p.sort = Some(s);
                });
            }
            ui.horizontal(|ui| {
                ui.label("Layout");
                changed |= ui.radio_value(&mut p.board, false, "List").changed();
                changed |= ui.radio_value(&mut p.board, true, "Board").changed();
            });
            ui.separator();
            ui.horizontal(|ui| {
                if p.is_custom() && ui.button("Delete Perspective").clicked() {
                    delete = true;
                }
                if ui.button("Show It").clicked() {
                    if let Some(v) = View::from_key(&p.key) {
                        self.view_after_editor = Some(v);
                    }
                }
            });
        });
        if let Some(v) = self.view_after_editor.take() {
            self.go(v);
        }
        if generate {
            let (tx, rx) = std::sync::mpsc::channel();
            self.ai_rx = Some(rx);
            self.ai_msg = format!("Asking {provider}…");
            let store = self.store.clone();
            let text = self.ai_text.clone();
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(ai::draft(&store, &text));
                ctx2.request_repaint();
            });
        }
        if delete {
            self.store.settings.perspectives.remove(idx);
            if self.view.key() == key {
                self.switch(View::Inbox);
            }
            open = false;
            changed = true;
        }
        if changed {
            self.touch();
        }
        if !open {
            self.editor = None;
        }
    }

    pub(crate) fn settings_perspectives(&mut self, ui: &mut egui::Ui) -> bool {
        let mut ch = false;
        let n = self.store.settings.perspectives.len();
        let mut edit = None;
        ui.label(egui::RichText::new("Show, hide, reorder and colour perspectives. Built-in ones can be hidden but not deleted.").color(th().dim));
        let summaries: Vec<Option<String>> = self.store.settings.perspectives.iter().map(|p| p.filter.as_ref().filter(|_| p.is_custom()).map(|r| rule_summary(r, &self.store))).collect();
        ui.label(egui::RichText::new("Drag the grip to reorder; the sidebar follows this order.").color(th().dim).size(12.0));
        let list = &mut self.store.settings.perspectives;
        let moved = drag_list(ui, egui::Id::new("settings-perspectives"), n, |ui, i| {
            let p = &mut list[i];
            ch |= ui.checkbox(&mut p.visible, "").on_hover_text("Show in sidebar").changed();
            // ☆ / ★: favourites fill the iPhone's bottom bar
            let fav = p.is_favorite();
            if ui.add(egui::Button::new(egui::RichText::new(if fav { "★" } else { "☆" }).color(if fav { hue("yellow") } else { th().dim })).frame(false)).on_hover_text(if fav { "Favorite: in the iPhone's bottom bar" } else { "Make a favorite" }).clicked() {
                p.favorite = Some(!fav);
                ch = true;
            }
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 6.0, hue(&p.hue));
            ch |= ui.add(crate::nativetext::Edit::singleline(&mut p.name).desired_width(160.0)).changed();
            if ui.button("Edit…").clicked() {
                edit = Some(p.key.clone());
            }
            if let Some(sum) = &summaries[i] {
                ui.label(egui::RichText::new(sum).color(th().dim).size(12.0));
            }
        });
        if let Some((from, to)) = moved {
            let p = self.store.settings.perspectives.remove(from);
            self.store.settings.perspectives.insert(to, p);
            ch = true;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Add Perspective").clicked() {
                edit = Some(self.new_custom_perspective());
                self.ai_msg.clear();
            }
            if ui.button("Restore Built-in Perspectives").clicked() {
                let custom: Vec<PerspectiveCfg> = self.store.settings.perspectives.iter().filter(|p| p.is_custom()).cloned().collect();
                self.store.settings.perspectives = default_perspectives();
                self.store.settings.perspectives.extend(custom);
                ch = true;
            }
        });
        if edit.is_some() {
            self.editor = edit;
        }
        if !self.visible_views().contains(&self.view) {
            if let Some(v) = self.visible_views().first().copied() {
                self.switch(v);
            }
        }
        ch
    }

    pub(crate) fn settings_keyboard(&mut self, ui: &mut egui::Ui) -> bool {
        let mut ch = false;
        let binds = keys::bindings(&self.store.settings.keymap);
        ui.label(egui::RichText::new("Click a shortcut to replace it, + to add another, ↺ to reset.").color(th().dim));
        if let Some((a, _)) = self.recording {
            ui.label(egui::RichText::new(format!("Press the new keys for “{}” (Esc cancels)…", a.label())).color(th().accent).strong());
        }
        let mut group = "";
        for &a in Action::ALL {
            if a.group() != group {
                group = a.group();
                ui.add_space(6.0);
                ui.label(egui::RichText::new(group).font(bold(14.0)));
            }
            ui.horizontal(|ui| {
                ui.add_sized([230.0, 20.0], egui::Label::new(a.label()).truncate());
                let mine: Vec<keys::Binding> = binds.iter().filter(|(x, _)| *x == a).map(|(_, b)| *b).collect();
                for b in &mine {
                    let clash = binds.iter().any(|(x, o)| *x != a && o == b);
                    let txt = egui::RichText::new(b.format()).monospace().color(if clash { th().overdue } else { th().text });
                    let r = ui.button(txt);
                    let r = if clash { r.on_hover_text("Also used by another action") } else { r };
                    if r.clicked() {
                        self.recording = Some((a, false));
                    }
                }
                if ui.small_button("+").on_hover_text("Add another shortcut").clicked() {
                    self.recording = Some((a, true));
                }
                if self.store.settings.keymap.contains_key(a.id()) && ui.small_button("↺").on_hover_text("Reset to default").clicked() {
                    self.store.settings.keymap.remove(a.id());
                    ch = true;
                }
            });
        }
        ui.add_space(8.0);
        if ui.button("Reset All Shortcuts").clicked() {
            self.store.settings.keymap.clear();
            ch = true;
        }
        ch
    }

    pub(crate) fn settings_agent(&mut self, ui: &mut egui::Ui) -> bool {
        let mut ch = false;
        // Send to Agent's prompt templates (agent.rs): labelled fields with defaults, chosen per action
        ui.heading("Send to Agent templates");
        ui.label(egui::RichText::new("One field per line, front-matter style: `key: value` (a line without a key continues the one above). project: or tag: picks the template for actions in that project or with that tag; followUp: is done last. A value can use the action's {task}, {project}, {tags}, {note}, {defer}, {planned}, {due}, {estimate} and {link}. The fields appear, filled in, when you Send to Agent, and go to the agent with the action's details.").color(th().dim).size(12.0));
        let mut remove = None;
        for (i, tp) in self.store.settings.agent_templates.iter_mut().enumerate() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ch |= ui.add(crate::nativetext::Edit::singleline(&mut tp.name).hint_text("Template name").desired_width(260.0)).changed();
                if ui.small_button("Delete").clicked() {
                    remove = Some(i);
                }
            });
            ch |= ui.add(crate::nativetext::Edit::multiline(&mut tp.text).desired_rows(5).desired_width(f32::INFINITY).font(egui::FontId::monospace(12.5))).changed();
        }
        if let Some(i) = remove {
            self.store.settings.agent_templates.remove(i);
            ch = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Add Template").clicked() {
                self.store.settings.agent_templates.push(crate::agent::Template { name: "New template".into(), text: "task: \nproject: \npath: \nctx: \nfollowUp: \n".into() });
                ch = true;
            }
            if ui.button("Add the Example").clicked() {
                self.store.settings.agent_templates.extend(crate::agent::default_templates());
                ch = true;
            }
        });
        ui.separator();
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "gtdcraft".into());
        ui.heading("Agents can use GTDCraft (MCP)");
        ui.label("Any MCP client can read and change your tasks and create perspectives. Add it to Claude Code with:");
        let cmd = format!("claude mcp add gtdcraft -- \"{exe}\" --mcp");
        ui.add(egui::TextEdit::multiline(&mut cmd.as_str()).desired_rows(2).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        let json = format!("{{\"mcpServers\": {{\"gtdcraft\": {{\"command\": {:?}, \"args\": [\"--mcp\"]}}}}}}", exe);
        ui.horizontal(|ui| {
            if ui.button("Copy command").clicked() {
                ui.ctx().copy_text(cmd.clone());
            }
            if ui.button("Copy JSON config").clicked() {
                ui.ctx().copy_text(json.clone());
            }
        });
        ui.separator();
        ui.heading("AI helper (Describe a perspective)");
        let st = &mut self.store.settings;
        ui.horizontal(|ui| {
            ch |= ui.radio_value(&mut st.ai_provider, "claude".to_string(), "Claude").changed();
            ch |= ui.radio_value(&mut st.ai_provider, "openai".to_string(), "OpenAI-compatible").changed();
            ch |= ui.radio_value(&mut st.ai_provider, "acp".to_string(), "ACP agent").changed();
        });
        let field = |ui: &mut egui::Ui, label: &str, v: &mut String, hint: &str, secret: bool| {
            ui.horizontal(|ui| {
                ui.add_sized([110.0, 20.0], egui::Label::new(label));
                ui.add(crate::nativetext::Edit::singleline(v).hint_text(hint).password(secret).desired_width(f32::INFINITY)).changed()
            })
            .inner
        };
        match st.ai_provider.as_str() {
            "openai" => {
                ui.label(egui::RichText::new("OpenAI, or Hermes and other open models through any OpenAI-compatible server (Ollama, LM Studio, vLLM, OpenRouter…).").color(th().dim).size(12.0));
                ch |= field(ui, "Base URL", &mut st.ai_base_url, "https://api.openai.com/v1  ·  http://localhost:11434/v1", false);
                ch |= field(ui, "Model", &mut st.ai_model, "model name as your server lists it", false);
                ch |= field(ui, "API key", &mut st.ai_key, "or set OPENAI_API_KEY; local servers may need none", true);
            }
            "acp" => {
                ui.label(egui::RichText::new("Any Agent Client Protocol agent, started as a command. GTDCraft offers it no file or terminal access and declines permission requests.").color(th().dim).size(12.0));
                // installed agents, found on your shell's PATH (checked once in the background)
                if self.acp_found.is_none() && self.acp_rx.is_none() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(acpfind::find());
                        ctx.request_repaint();
                    });
                    self.acp_rx = Some(rx);
                }
                if let Some(found) = self.acp_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                    self.acp_found = Some(found);
                    self.acp_rx = None;
                }
                match &self.acp_found {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Looking for installed agents…");
                        });
                    }
                    Some(found) if found.is_empty() => {
                        ui.label(egui::RichText::new("No ACP agents found on this Mac. Install one (Claude Code, Codex, Gemini CLI, Hermes, goose, OpenCode…) or type its command below.").color(th().dim).size(12.0));
                    }
                    Some(found) => {
                        ui.label(egui::RichText::new("Installed agents").font(bold(13.0)));
                        for f in found {
                            let on = st.acp_command.trim() == f.command;
                            ui.horizontal(|ui| {
                                if ui.radio(on, f.name).on_hover_text(&f.command).clicked() && !on {
                                    st.acp_command = f.command.clone();
                                    ch = true;
                                }
                            });
                        }
                    }
                }
                if ui.small_button("Look again").clicked() {
                    self.acp_found = None;
                }
                ch |= field(ui, "Command", &mut st.acp_command, "the agent's ACP command line", false);
            }
            _ => {
                ui.label(egui::RichText::new(format!("Uses {} through the Claude API.", ai::MODEL)).color(th().dim).size(12.0));
                ch |= field(ui, "API key", &mut st.ai_key, "or set ANTHROPIC_API_KEY", true);
            }
        }
        ui.label(egui::RichText::new("Keys are stored in your GTDCraft database file on this Mac.").color(th().dim).size(12.0));
        ui.separator();
        ui.heading("Decisions (System One)");
        ui.label(egui::RichText::new("A decision model answers typed questions (yes/no, one of several, a score) with a probability for each answer, in a fraction of a second. Agents use it through gtd_decide.").color(th().dim).size(12.0));
        ch |= ui.checkbox(&mut st.decisions_on, "Use a decision model").changed();
        if st.decisions_on {
            ui.horizontal_wrapped(|ui| {
                for p in &systemone::PROVIDERS {
                    let on = systemone::provider(&st.decisions_provider).key == p.key;
                    if ui.radio(on, p.name).clicked() && !on {
                        st.decisions_provider = p.key.to_string();
                        ch = true;
                    }
                }
            });
            let p = systemone::provider(&st.decisions_provider);
            ui.label(egui::RichText::new(p.about).color(th().dim).size(12.0));
            let url_hint = if p.url.is_empty() { "http://other-mac.local:8001" } else { p.url };
            ch |= field(ui, "URL", &mut st.decisions_url, url_hint, false);
            ch |= field(ui, "Model", &mut st.decisions_model, if p.model.is_empty() { "the model name the server expects" } else { p.model }, false);
            if p.key == "cloudflare" {
                ch |= field(ui, "Account ID", &mut st.decisions_account, "or set CLOUDFLARE_ACCOUNT_ID", false);
            }
            if p.key_env.is_empty() {
                ch |= field(ui, "API key", &mut st.decisions_key, "optional", true);
            } else {
                ch |= field(ui, "API key", &mut st.decisions_key, &format!("or set {}", p.key_env), true);
            }
            if let Some(r) = self.decide_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                self.decide_test = Some(r);
                self.decide_rx = None;
            }
            ui.horizontal(|ui| {
                if self.decide_rx.is_some() {
                    ui.spinner();
                    ui.label("Asking…");
                } else if ui.button("Test").clicked() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let ctx = ui.ctx().clone();
                    let route = systemone::route(st);
                    std::thread::spawn(move || {
                        let _ = tx.send(route.and_then(|r| systemone::test(&r)));
                        ctx.request_repaint();
                    });
                    self.decide_rx = Some(rx);
                }
                match &self.decide_test {
                    Some(Ok(msg)) => ui.label(egui::RichText::new(msg).color(th().hues[3]).size(12.0)),
                    Some(Err(e)) => ui.label(egui::RichText::new(e).color(th().overdue).size(12.0)),
                    None => ui.label(""),
                };
            });
        }
        ui.separator();
        ui.heading("Linear");
        ch |= field(ui, "API key", &mut st.linear_key, "personal API key, or set LINEAR_API_KEY", true);
        ui.label(egui::RichText::new("Link an issue from a task's or project's inspector to see its status and change it here.").color(th().dim).size(12.0));
        ui.separator();
        ui.heading("GitHub");
        ch |= field(ui, "Token", &mut st.github_token, "optional; or set GITHUB_TOKEN", true);
        ui.label(egui::RichText::new("References to GitHub repos, issues and pull requests show their title and state. Public repos work without a token; private ones need one with read access.").color(th().dim).size(12.0));
        ch
    }

    /// Settings › Connections (iPhone/iPad): the keys GTDCraft's connections use. They sync between your devices, so
    /// setting one on the Mac is enough.
    pub(crate) fn settings_connections(&mut self, ui: &mut egui::Ui) -> bool {
        let st = &mut self.store.settings;
        let mut ch = false;
        let field = |ui: &mut egui::Ui, label: &str, v: &mut String, hint: &str| {
            ui.label(egui::RichText::new(label).strong());
            ui.add(crate::nativetext::Edit::singleline(v).hint_text(hint).password(true).desired_width(f32::INFINITY)).changed()
        };
        ui.label(egui::RichText::new("These sync with your other devices: set them once on any of them.").color(th().dim).size(12.0));
        ui.add_space(8.0);
        ch |= field(ui, "Linear API key", &mut st.linear_key, "lin_api_…");
        ui.add_space(6.0);
        ch |= field(ui, "GitHub token", &mut st.github_token, "optional; needed for private repos");
        ui.add_space(6.0);
        let label = if st.ai_provider == "openai" { "AI key (OpenAI-compatible)" } else { "AI key (Claude)" };
        ch |= field(ui, label, &mut st.ai_key, "for Describe a perspective");
        ch
    }

    // ---------- References ----------
    /// Fetched GitHub details arrive here; every task and project with that link gets them.
    pub(crate) fn poll_refs(&mut self) {
        if let Some(rx) = &self.refs_list_rx {
            for (repo, res) in rx.try_iter().collect::<Vec<_>>() {
                self.refs_busy.remove(&format!("list:{repo}"));
                match res {
                    Ok(list) => {
                        self.refs_lists.insert(repo, list);
                    }
                    Err(e) => self.refs_msg = e,
                }
            }
        }
        let Some(rx) = &self.refs_rx else { return };
        let got: Vec<(String, Result<refs::Ref, String>)> = rx.try_iter().collect();
        for (url, res) in got {
            self.refs_busy.remove(&url);
            match res {
                Ok(r) => {
                    let lists = self.store.tasks.iter_mut().map(|t| &mut t.refs).chain(self.store.projects.iter_mut().map(|p| &mut p.refs));
                    for list in lists {
                        for x in list.iter_mut().filter(|x| x.url == url) {
                            (x.title, x.state) = (r.title.clone(), r.state.clone());
                        }
                    }
                    self.touch();
                }
                Err(e) => {
                    self.refs_failed.insert(url);
                    self.refs_msg = e;
                }
            }
        }
    }

    fn refs_list_fetch(&mut self, ctx: &egui::Context, repo: String) {
        if !self.refs_busy.insert(format!("list:{repo}")) {
            return;
        }
        if self.refs_list_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            (self.refs_list_rx, self.refs_list_tx) = (Some(rx), Some(tx));
        }
        let (tx, ctx, token) = (self.refs_list_tx.clone().unwrap(), ctx.clone(), refs::token(&self.store.settings.github_token));
        std::thread::spawn(move || {
            let res = refs::list_open(&repo, token.as_deref());
            let _ = tx.send((repo, res));
            ctx.request_repaint();
        });
    }

    fn refs_fetch(&mut self, ctx: &egui::Context, url: String) {
        if self.refs_failed.contains(&url) || !self.refs_busy.insert(url.clone()) {
            return;
        }
        if self.refs_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            (self.refs_rx, self.refs_tx) = (Some(rx), Some(tx));
        }
        let (tx, ctx, token) = (self.refs_tx.clone().unwrap(), ctx.clone(), refs::token(&self.store.settings.github_token));
        std::thread::spawn(move || {
            let res = refs::fetch(&url, token.as_deref());
            let _ = tx.send((url, res));
            ctx.request_repaint();
        });
    }

    /// Inspector › References: one row per link (kind, title, state; click opens it in its app, × removes it) and a
    /// field to paste another. Returns the new list when it changed.
    pub(crate) fn refs_section(&mut self, ui: &mut egui::Ui, current: Vec<refs::Ref>, op: &mut Option<(String, SecMove)>) -> Option<Vec<refs::Ref>> {
        sec_caption(ui, "References", "refs", None, op);
        let mut list = current.clone();
        let mut remove = None;
        let w = ui.available_width();
        for (i, r) in current.iter().enumerate() {
            if refs::needs_fetch(r) {
                self.refs_fetch(&ui.ctx().clone(), r.url.clone());
            }
            let kind = refs::kind(&r.url);
            egui::Frame::new().fill(th().field).corner_radius(8).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                ui.set_width(w - 20.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(kind.name()).size(11.0).color(th().dim).background_color(mix(th().field, th().text, 0.08)));
                    if !r.state.is_empty() {
                        let col = match r.state.as_str() {
                            "open" => hue("green"),
                            "merged" => hue("purple"),
                            "closed" => if kind == refs::Kind::PullRequest { hue("red") } else { hue("purple") },
                            _ => th().dim,
                        };
                        ui.label(egui::RichText::new(&r.state).size(11.0).color(col).strong());
                    }
                    if self.refs_busy.contains(&r.url) {
                        ui.spinner();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Remove this reference").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                let title = if r.title.is_empty() { refs::label(&r.url) } else { r.title.clone() };
                let resp = ui.add(egui::Label::new(egui::RichText::new(title).strong()).wrap().sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&r.url);
                if !r.title.is_empty() {
                    ui.label(egui::RichText::new(refs::label(&r.url)).color(th().dim).size(11.5));
                }
                // A YouTube link shows its thumbnail, so the card reads like the video it points to.
                if let Some(id) = crate::webact::youtube_id(&r.url) {
                    let tw = (w - 20.0).min(240.0);
                    let (tr, tresp) = ui.allocate_exact_size(vec2(tw, tw * 9.0 / 16.0), Sense::click());
                    match crate::webact::thumb(&id) {
                        Some(path) => {
                            egui::Image::from_uri(format!("file://{}", path.display())).corner_radius(6).paint_at(ui, tr);
                        }
                        None => {
                            ui.painter().rect_filled(tr, 6.0, th().pill);
                            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "▶", egui::FontId::proportional(18.0), th().faint);
                        }
                    }
                    if tresp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        refs::open(&r.url);
                    }
                }
                if resp.clicked() {
                    refs::open(&r.url);
                }
                // a repo: its open issues and pull requests, to link any of them here
                if kind == refs::Kind::Repo {
                    let busy = self.refs_busy.contains(&format!("list:{}", r.url));
                    match self.refs_lists.get(&r.url).cloned() {
                        None => {
                            if ui.add_enabled(!busy, egui::Button::new("Issues & PRs").small()).on_hover_text("List this repo's open issues and pull requests").clicked() {
                                self.refs_list_fetch(&ui.ctx().clone(), r.url.clone());
                            }
                        }
                        Some(items) => {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("{} open", items.len())).color(th().dim).size(11.5));
                                if ui.small_button("Hide").clicked() {
                                    self.refs_lists.remove(&r.url);
                                }
                            });
                            for it in items {
                                let have = list.iter().any(|x| x.url == it.url);
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(!have, egui::Button::new(if have { "✓" } else { "+" }).small()).on_hover_text("Add as a reference").clicked() {
                                        list.push(it.clone());
                                    }
                                    ui.label(egui::RichText::new(refs::kind(&it.url).name()).size(10.5).color(th().dim));
                                    ui.add(egui::Label::new(egui::RichText::new(&it.title).size(12.5)).truncate());
                                });
                            }
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            list.remove(i);
        }
        ui.horizontal(|ui| {
            let r = ui.add(crate::nativetext::Edit::singleline(&mut self.refs_buf).kind(crate::nativetext::Kind::Url).hint_text("Paste a GitHub, Gmail, Messages or web link").desired_width((w - 56.0).max(80.0)));
            if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button("Add").clicked() {
                match refs::normalize(&self.refs_buf) {
                    Some(url) if !list.iter().any(|x| x.url == url) => {
                        list.push(refs::Ref { url, ..Default::default() });
                        self.refs_buf.clear();
                        self.refs_msg.clear();
                    }
                    Some(_) => self.refs_buf.clear(),
                    None if !self.refs_buf.trim().is_empty() => self.refs_msg = "That isn't a link GTDCraft can open (https://…, github.com/…, owner/repo#12, sms:, message:).".into(),
                    None => {}
                }
            }
        });
        if !self.refs_msg.is_empty() {
            ui.label(egui::RichText::new(&self.refs_msg).color(th().overdue).size(11.5));
        }
        (list != current).then_some(list)
    }

    /// Inspector › Attachments: file/folder links added via picker or paste, separate from notes.
    /// Returns the new list when it changed.
    pub(crate) fn attachments_section(&mut self, ui: &mut egui::Ui, current: Vec<String>, op: &mut Option<(String, SecMove)>, target: crate::attachpick::Target) -> Option<Vec<String>> {
        sec_caption(ui, "Attachments", "attachments", None, op);
        let mut list = current.clone();
        let mut remove = None;
        let w = ui.available_width();
        for (i, path) in current.iter().enumerate() {
            if notes::attachment_row(ui, path, w) {
                remove = Some(i);
            }
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            list.remove(i);
        }
        // File picker (macOS only; rfd not available on iOS)
        #[cfg(not(target_os = "ios"))]
        ui.horizontal(|ui| {
            if ui.button("Add File\u{2026}").clicked() {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    for p in paths {
                        let s = p.to_string_lossy().to_string();
                        if !list.contains(&s) {
                            list.push(s);
                        }
                    }
                    self.att_msg.clear();
                }
            }
            if ui.button("Add Folder\u{2026}").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    let s = p.to_string_lossy().to_string();
                    if !list.contains(&s) {
                        list.push(s);
                    }
                    self.att_msg.clear();
                }
            }
        });
        // iPhone/iPad: from Files or Photos (each copied into the app; added on the next frame)
        #[cfg(target_os = "ios")]
        ui.horizontal(|ui| {
            if ui.button("Add from Files\u{2026}").clicked() {
                crate::attachpick::present_files(target);
            }
            if ui.button("Add Photo\u{2026}").clicked() {
                crate::attachpick::present_photos(target);
            }
        });
        #[cfg(not(target_os = "ios"))]
        let _ = target;
        // Paste field (all platforms)
        ui.horizontal(|ui| {
            let r = ui.add(crate::nativetext::Edit::singleline(&mut self.att_buf).kind(crate::nativetext::Kind::Url).hint_text("Paste a path or file:// URL").desired_width((w - 56.0).max(80.0)));
            if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button("Add").clicked() {
                let s = self.att_buf.trim().to_string();
                if !s.is_empty() {
                    let normalized = notes::normalize_attachment(&s);
                    if !list.contains(&normalized) {
                        list.push(normalized);
                        self.att_buf.clear();
                        self.att_msg.clear();
                    } else {
                        self.att_buf.clear();
                    }
                }
            }
        });
        if !self.att_msg.is_empty() {
            ui.label(egui::RichText::new(&self.att_msg).color(th().overdue).size(11.5));
        }
        (list != current).then_some(list)
    }

    // ---------- Linear ----------
    pub(crate) fn poll_linear(&mut self) {
        let Some(rx) = &self.linear_rx else { return };
        let got: Vec<(String, Result<linear::Issue, String>)> = rx.try_iter().collect();
        for (id, res) in got {
            self.linear_busy.remove(&id);
            match res {
                Ok(issue) => {
                    self.store.linear.insert(issue.identifier.clone(), issue);
                    self.linear_msg.clear();
                    self.touch();
                }
                Err(e) => self.linear_msg = e,
            }
        }
    }

    fn linear_request(&mut self, ctx: &egui::Context, id: String, state: Option<String>) {
        let Some(key) = linear::api_key(&self.store.settings.linear_key) else {
            self.linear_msg = "Add a Linear API key in Settings › Agent.".into();
            return;
        };
        if !self.linear_busy.insert(id.clone()) {
            return;
        }
        if self.linear_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.linear_rx = Some(rx);
            self.linear_tx = Some(tx);
        }
        let tx = self.linear_tx.clone().unwrap();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = match state {
                Some(s) => linear::set_state(&key, &id, &s),
                None => linear::fetch(&key, &id),
            };
            let _ = tx.send((id, res));
            ctx.request_repaint();
        });
    }

    /// Inspector section: link field, or a card with status, details, Open in Linear and a status picker.
    /// Returns the new link value when it changed.
    pub(crate) fn linear_section(&mut self, ui: &mut egui::Ui, current: Option<String>, op: &mut Option<(String, SecMove)>) -> Option<Option<String>> {
        let mut out = None;
        sec_caption(ui, "Linear", "linear", Some(linear::logo), op);
        let w = ui.available_width();
        match current {
            None => {
                let r = ui.add(crate::nativetext::Edit::singleline(&mut self.link_buf).kind(crate::nativetext::Kind::Url).hint_text("Issue ID or linear.app link").desired_width(w));
                if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button("Link Issue").clicked() {
                    match linear::identifier(&self.link_buf) {
                        Some(id) => {
                            out = Some(Some(id.clone()));
                            self.link_buf.clear();
                            self.linear_request(&ui.ctx().clone(), id, None);
                        }
                        None => self.linear_msg = "That doesn't look like a Linear issue (e.g. ENG-123).".into(),
                    }
                }
            }
            Some(id) => {
                let issue = self.store.linear.get(&id).cloned();
                if issue.is_none() && !self.linear_busy.contains(&id) && self.linear_msg.is_empty() {
                    self.linear_request(&ui.ctx().clone(), id.clone(), None);
                }
                egui::Frame::new().fill(th().field).stroke(Stroke::new(1.0, th().line)).corner_radius(8).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.set_width(w - 20.0);
                    match &issue {
                        None => {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&id).strong());
                                if self.linear_busy.contains(&id) {
                                    ui.spinner();
                                }
                            });
                        }
                        Some(i) => {
                            ui.horizontal(|ui| {
                                let sc = linear::color(&i.state.color);
                                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                state_glyph(ui.painter(), r.center(), &i.state.kind, sc);
                                ui.label(egui::RichText::new(&i.identifier).color(th().dim).monospace());
                                ui.label(egui::RichText::new(&i.state.name).color(sc).strong());
                            });
                            ui.add(egui::Label::new(egui::RichText::new(&i.title).strong().size(14.0)).wrap());
                            ui.horizontal_wrapped(|ui| {
                                let mut bits = vec![];
                                if !i.priority_label.is_empty() && i.priority_label != "No priority" {
                                    bits.push(i.priority_label.clone());
                                }
                                if !i.assignee.is_empty() {
                                    bits.push(i.assignee.clone());
                                }
                                if !i.due.is_empty() {
                                    bits.push(format!("Due {}", i.due));
                                }
                                if let Some(e) = i.estimate {
                                    bits.push(format!("{e} pts"));
                                }
                                ui.label(egui::RichText::new(bits.join(" · ")).color(th().dim).size(12.0));
                                for (n, c) in &i.labels {
                                    ui.label(egui::RichText::new(n).size(11.5).color(linear::color(c)).background_color(linear::color(c).gamma_multiply(0.15)));
                                }
                            });
                            ui.horizontal(|ui| {
                                if !i.states.is_empty() {
                                    let mut pick: Option<String> = None;
                                    egui::ComboBox::from_id_salt(("lin-state", &i.identifier)).selected_text("Change Status").show_ui(ui, |ui| {
                                        for s in &i.states {
                                            if ui.selectable_label(s.id == i.state.id, &s.name).clicked() {
                                                pick = Some(s.id.clone());
                                            }
                                        }
                                    });
                                    if let Some(s) = pick.filter(|s| *s != i.state.id) {
                                        self.linear_request(&ui.ctx().clone(), i.identifier.clone(), Some(s));
                                    }
                                }
                                if ui.button("Open in Linear").clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(&i.url));
                                }
                                if ui.small_button("↻").on_hover_text("Refresh").clicked() {
                                    self.linear_request(&ui.ctx().clone(), i.identifier.clone(), None);
                                }
                                if self.linear_busy.contains(&i.identifier) {
                                    ui.spinner();
                                }
                            });
                        }
                    }
                    if ui.small_button("Unlink").clicked() {
                        out = Some(None);
                    }
                });
            }
        }
        if !self.linear_msg.is_empty() {
            ui.label(egui::RichText::new(&self.linear_msg).color(th().dim).size(12.0));
        }
        out
    }
}

/// Linear's state icons, simplified: hollow (backlog/todo), half (started), filled check (done), cross (canceled).
pub(crate) fn state_glyph(p: &egui::Painter, c: egui::Pos2, kind: &str, col: Color32) {
    match kind {
        "started" => {
            p.circle_stroke(c, 5.5, Stroke::new(1.5, col));
            p.add(egui::Shape::convex_polygon(
                (0..=12).map(|k| c + vec2(0.0, 0.0) + 3.5 * egui::Vec2::angled(-std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / 12.0)).chain(std::iter::once(c)).collect(),
                col,
                Stroke::NONE,
            ));
        }
        "completed" => {
            p.circle_filled(c, 6.0, col);
            check(p, c, 3.0, Stroke::new(1.4, Color32::WHITE));
        }
        "canceled" => {
            p.circle_filled(c, 6.0, col);
            p.line_segment([c + vec2(-2.5, -2.5), c + vec2(2.5, 2.5)], Stroke::new(1.4, Color32::WHITE));
            p.line_segment([c + vec2(2.5, -2.5), c + vec2(-2.5, 2.5)], Stroke::new(1.4, Color32::WHITE));
        }
        "backlog" => {
            p.circle_stroke(c, 5.5, Stroke::new(1.3, col.gamma_multiply(0.7)));
        }
        _ => {
            p.circle_stroke(c, 5.5, Stroke::new(1.5, col));
        }
    }
}

fn rule_summary(r: &Rule, s: &Store) -> String {
    match r {
        Rule::All { rules } | Rule::Any { rules } | Rule::None { rules } => {
            let parts: Vec<String> = rules.iter().take(3).map(|x| x.label(s)).collect();
            let more = if rules.len() > 3 { format!(" +{}", rules.len() - 3) } else { String::new() };
            format!("{}{more}", parts.join(" · "))
        }
        other => other.label(s),
    }
}
