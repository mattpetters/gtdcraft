//! Send to Agent on the Mac, iPhone and iPad: the sync server runs the agents (agent.rs); this side asks it which
//! agents it offers, starts threads, shows their progress on the action's row, and opens a thread to read, reply,
//! answer a permission request, and approve or reject the staged changes.
use crate::agent::{self, Status, Thread};
use crate::model::Id;
use crate::{mix, th, Gtd};
use eframe::egui::{self, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};
use serde_json::{json, Value};
use std::sync::mpsc;

pub enum Got {
    /// (agents offered; empty when the server has them off or has none)
    Agents(Result<Vec<String>, String>),
    Threads(Result<Vec<Thread>, String>),
    Thread(Result<Box<Thread>, String>),
    Done(Result<Value, String>),
}

pub struct Compose {
    pub task: Id,
    pub agent: String,
    pub text: String,
    /// continue from this thread's summary
    pub from: Option<String>,
    /// the prompt template chosen (index into Settings' templates) and its fields as edited here
    pub template: Option<usize>,
    pub fields: Vec<(String, String)>,
}

#[derive(Default)]
pub struct State {
    /// None until the server has answered; Some(empty) = not available
    pub agents: Option<Vec<String>>,
    pub threads: Vec<Thread>,
    pub compose: Option<Compose>,
    /// the thread window
    pub open: Option<String>,
    /// the thread window shown full-screen (slide-over) rather than as a centered popup
    pub expanded: bool,
    pub reply: String,
    pub msg: String,
    pub busy: bool,
    last_poll: f64,
    rx: Option<mpsc::Receiver<Got>>,
    tx: Option<mpsc::Sender<Got>>,
}

impl State {
    pub fn available(&self) -> bool {
        self.agents.as_ref().is_some_and(|a| !a.is_empty())
    }

    /// The task's most recent thread.
    pub fn latest(&self, task: Id) -> Option<&Thread> {
        self.threads.iter().filter(|t| t.task == task).max_by_key(|t| t.created)
    }

    pub fn thread(&self, id: &str) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }
}

/// A thread's state as one colour: working (accent), needs you (orange), review (green), finished (dim).
pub fn status_color(s: Status) -> Color32 {
    match s {
        Status::Working => th().accent,
        Status::Waiting | Status::Permission => crate::hue("orange"),
        Status::Review => crate::hue("green"),
        Status::Failed => th().overdue,
        Status::Done | Status::Expired => th().dim,
    }
}

/// An indeterminate progress bar: a segment sliding across `r`, for an agent that's working.
pub fn progress(p: &egui::Painter, r: Rect, t: f64, c: Color32) {
    p.rect_filled(r, r.height() / 2.0, mix(th().bg, c, 0.18));
    let w = r.width() * 0.3;
    let x = r.left() - w + ((t * 0.6).fract() as f32) * (r.width() + w);
    let seg = Rect::from_min_max(egui::pos2(x.max(r.left()), r.top()), egui::pos2((x + w).min(r.right()), r.bottom()));
    if seg.width() > 0.0 {
        p.rect_filled(seg, r.height() / 2.0, c);
    }
}

impl Gtd {
    fn agent_call(&mut self, method: &'static str, path: String, body: Option<Value>, then: fn(Result<Value, String>) -> Got) {
        let (url, token) = (self.store.settings.sync_url.clone(), self.store.settings.sync_token.clone());
        if url.trim().is_empty() {
            return;
        }
        if self.agentui.tx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.agentui.tx = Some(tx);
            self.agentui.rx = Some(rx);
        }
        let Some(tx) = self.agentui.tx.clone() else { return };
        std::thread::spawn(move || {
            let _ = tx.send(then(agent::request(&url, &token, method, &path, body)));
        });
    }

    fn agent_refresh(&mut self) {
        self.agent_call("GET", "/v1/agent/agents".into(), None, |r| Got::Agents(r.map(|v| if v["enabled"].as_bool() == Some(true) { v["agents"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect() } else { vec![] })));
        self.agent_call("GET", "/v1/agent/threads".into(), None, |r| Got::Threads(r.and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))));
    }

    /// Each frame: take answers in; ask again every 30 s, or every 2 s while something is running.
    pub(crate) fn agent_poll(&mut self, ctx: &egui::Context) {
        let mut got = vec![];
        if let Some(rx) = &self.agentui.rx {
            got.extend(rx.try_iter());
        }
        for g in got {
            match g {
                Got::Agents(r) => self.agentui.agents = Some(r.unwrap_or_default()),
                Got::Threads(Ok(v)) => self.agentui.threads = v,
                Got::Threads(Err(_)) => {}
                Got::Thread(Ok(t)) => {
                    self.agentui.busy = false;
                    self.agentui.open = Some(t.id.clone());
                    self.agentui.threads.retain(|x| x.id != t.id);
                    self.agentui.threads.push(*t);
                }
                Got::Thread(Err(e)) | Got::Done(Err(e)) => {
                    self.agentui.busy = false;
                    self.agentui.msg = e;
                }
                Got::Done(Ok(_)) => {
                    self.agentui.busy = false;
                    self.agentui.last_poll = 0.0; // refresh now
                }
            }
        }
        if self.store.settings.sync_url.trim().is_empty() {
            return;
        }
        let t = ctx.input(|i| i.time);
        let running = self.agentui.threads.iter().any(|x| x.status == Status::Working);
        let every = if running || self.agentui.open.is_some() { 2.0 } else { 30.0 };
        if self.agentui.last_poll == 0.0 || t - self.agentui.last_poll > every {
            self.agentui.last_poll = t.max(f64::MIN_POSITIVE);
            self.agent_refresh();
        }
        if running {
            ctx.request_repaint_after(std::time::Duration::from_millis(50)); // the progress bars move
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
        }
    }

    /// Ask the server again on the next frame (after the opt-in changed).
    #[cfg_attr(target_os = "ios", allow(dead_code))]
    pub(crate) fn agentui_refresh_soon(&mut self) {
        self.agentui.last_poll = 0.0;
    }

    pub(crate) fn agent_compose(&mut self, task: Id, from: Option<String>) {
        let agent = self.agentui.agents.as_ref().and_then(|a| a.first().cloned()).unwrap_or_default();
        let text = from.as_ref().map(|_| "Continue where the earlier thread left off.".to_string()).unwrap_or_default();
        // the template for this action: one whose project: or tag: fits it
        let t = self.store.task(task);
        let project = t.and_then(|t| t.project).map(|p| self.store.project_path(p)).unwrap_or_default();
        let tags: Vec<String> = t.map(|t| t.tags.iter().map(|g| self.store.tag_path(*g)).collect()).unwrap_or_default();
        let template = self.store.settings.agent_templates.iter().position(|tp| agent::template_matches(&agent::parse_fields(&tp.text), &project, &tags));
        let fields = template.map(|i| agent::parse_fields(&self.store.settings.agent_templates[i].text)).unwrap_or_default();
        self.agentui.compose = Some(Compose { task, agent, text, from, template, fields });
        self.agentui.msg.clear();
    }

    /// Open the compose sheet for a Review Guru pass: task 0 (the whole database), prefilled with the grooming brief.
    pub(crate) fn guru_compose(&mut self) {
        let agent = self.agentui.agents.as_ref().and_then(|a| a.first().cloned()).unwrap_or_default();
        self.agentui.compose = Some(Compose { task: 0, agent, text: agent::GURU_BRIEF.to_string(), from: None, template: None, fields: vec![] });
        self.agentui.msg.clear();
    }

    fn agent_action(&mut self, id: &str, action: &str, body: Value) {
        self.agentui.busy = true;
        self.agent_call("POST", format!("/v1/agent/threads/{id}/{action}"), Some(body), Got::Done);
    }

    /// The sheet that starts a thread: the action, which agent, and what to do.
    pub(crate) fn agent_compose_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = self.agentui.compose.as_mut() else { return };
        let name = if c.task == 0 { "Review Guru — your whole database".to_string() } else { self.store.task(c.task).map(|t| t.name.clone()).unwrap_or_default() };
        let agents = self.agentui.agents.clone().unwrap_or_default();
        let templates = self.store.settings.agent_templates.clone();
        let msg = self.agentui.msg.clone();
        let w = (ctx.content_rect().width() - 32.0).min(520.0);
        let mut send = false;
        let mut cancel = false;
        let r = egui::Modal::new(egui::Id::new("agent-compose")).show(ctx, |ui| {
            ui.set_width(w);
            ui.label(egui::RichText::new("Send to Agent").strong().size(17.0));
            ui.label(egui::RichText::new(&name).color(th().dim));
            ui.add_space(8.0);
            if agents.is_empty() {
                ui.label(egui::RichText::new("The sync server offers no agents: turn Agents on in Settings › Sync on the Mac running it, and install one (Claude Code, Codex, Gemini CLI…).").color(th().overdue));
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Agent:");
                    for a in &agents {
                        ui.selectable_value(&mut c.agent, a.clone(), a);
                    }
                });
            }
            ui.add_space(6.0);
            ui.add(crate::nativetext::Edit::multiline(&mut c.text).hint_text("What should the agent do? It can read everything and stage changes for you to approve.").desired_rows(3).desired_width(w));
            // prompt templates: fixed labelled fields with their defaults, edited for this request
            if !templates.is_empty() {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Template:");
                    let mut pick: Option<Option<usize>> = None;
                    if ui.selectable_label(c.template.is_none(), "None").clicked() {
                        pick = Some(None);
                    }
                    for (i, tp) in templates.iter().enumerate() {
                        if ui.selectable_label(c.template == Some(i), &tp.name).clicked() {
                            pick = Some(Some(i));
                        }
                    }
                    if let Some(p) = pick {
                        c.template = p;
                        c.fields = p.map(|i| agent::parse_fields(&templates[i].text)).unwrap_or_default();
                    }
                });
                egui::ScrollArea::vertical().id_salt("tpl-fields").max_height(260.0).show(ui, |ui| {
                    egui::Grid::new("tpl-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        for (k, v) in c.fields.iter_mut() {
                            ui.label(egui::RichText::new(k.as_str()).color(th().dim));
                            ui.add(crate::nativetext::Edit::multiline(v).desired_rows(1).desired_width(w - 110.0).hint_text(if k.eq_ignore_ascii_case("followUp") { "anything to do last (optional)" } else { "" }));
                            ui.end_row();
                        }
                    });
                });
            }
            if c.from.is_some() {
                ui.label(egui::RichText::new("Continues from a summary of the earlier thread.").color(th().dim).size(12.0));
            }
            if !msg.is_empty() {
                ui.label(egui::RichText::new(&msg).color(th().overdue).size(12.0));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                // a template's fields can stand in for the request
                let ok = !agents.is_empty() && (!c.text.trim().is_empty() || c.fields.iter().any(|(_, v)| !v.trim().is_empty()));
                if ui.add_enabled(ok, egui::Button::new(egui::RichText::new("Send").color(th().on_accent)).fill(th().accent)).clicked() {
                    send = true;
                }
            });
        });
        if cancel || r.should_close() {
            self.agentui.compose = None;
            return;
        }
        if send {
            // the fields' {variables} filled from the action; an unknown one stops here with its message
            let env = agent::template_env(&self.store, self.agentui.compose.as_ref().map(|c| c.task).unwrap_or(0));
            let mut filled: Vec<[String; 2]> = vec![];
            for (k, v) in self.agentui.compose.as_ref().map(|c| c.fields.clone()).unwrap_or_default() {
                match agent::fill_vars(v.trim(), &env) {
                    Ok(v) if !v.trim().is_empty() => filled.push([k, v]),
                    Ok(_) => {}
                    Err(e) => {
                        self.agentui.msg = format!("{k}: {e}");
                        return;
                    }
                }
            }
            let c = self.agentui.compose.take().expect("compose");
            // a Review Guru pass (whole database) starts its zen cooldown now
            if c.task == 0 {
                self.store.settings.last_guru = Some(crate::model::now());
                self.touch();
            }
            self.agentui.busy = true;
            // the server needs the action: sync first, then start
            self.sync_due = Some(0.0);
            let fields = filled;
            let request = if c.text.trim().is_empty() { "Do what the template above describes for this action." } else { c.text.trim() };
            let body = json!({"task": c.task, "agent": c.agent, "request": request, "from": c.from, "db": self.store.db_id, "fields": fields});
            self.agent_call("POST", "/v1/agent/threads".into(), Some(body), |r| Got::Thread(r.and_then(|v| serde_json::from_value(v).map(Box::new).map_err(|e| e.to_string()))));
        }
    }

    /// The thread window: what was said, what it's doing, its questions and permission requests, and the staged
    /// changes to approve or reject.
    pub(crate) fn agent_thread_ui(&mut self, ctx: &egui::Context) {
        let Some(id) = self.agentui.open.clone() else { return };
        let Some(t) = self.agentui.thread(&id).cloned() else { return };
        let now_t = ctx.input(|i| i.time);
        let (close, toggle, act, cont) = if self.agentui.expanded {
            // full-screen slide-over: the page covers the whole screen, like the settings and perspective pages
            let full = ctx.viewport_rect();
            let safe = ctx.content_rect();
            let mut under = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("agent-thread-page")));
            under.set_clip_rect(full);
            under.rect_filled(full, 0.0, th().bg);
            let w = safe.width() - 24.0;
            let h = safe.height();
            egui::Area::new(egui::Id::new("agent-thread-page")).order(egui::Order::Foreground).fixed_pos(safe.min).show(ctx, |ui| {
                ui.set_min_size(safe.size());
                ui.set_max_width(safe.width());
                self.agent_thread_body(ui, &t, w, h, now_t)
            }).inner
        } else {
            let screen = ctx.content_rect();
            let w = (screen.width() - 24.0).min(640.0);
            let h = (screen.height() - 120.0).min(720.0);
            let r = egui::Modal::new(egui::Id::new("agent-thread")).show(ctx, |ui| self.agent_thread_body(ui, &t, w, h, now_t));
            let backdrop = r.should_close();
            let (c, tg, a, co) = r.inner;
            (c || backdrop, tg, a, co)
        };
        if let Some((a, body)) = act {
            self.agent_action(&id, a, body);
        }
        if toggle {
            self.agentui.expanded = !self.agentui.expanded;
        }
        if cont {
            self.agentui.open = None;
            self.agentui.expanded = false;
            self.agent_compose(t.task, Some(t.id.clone()));
        } else if close {
            self.agentui.open = None;
            self.agentui.expanded = false;
        }
    }

    /// The thread window's contents, shared by the popup and the full-screen slide-over.
    /// Returns (close, toggle_expand, action, continue).
    fn agent_thread_body(&mut self, ui: &mut egui::Ui, t: &Thread, w: f32, h: f32, now_t: f64) -> (bool, bool, Option<(&'static str, Value)>, bool) {
        let mut close = false;
        let mut toggle = false;
        let mut act: Option<(&'static str, Value)> = None;
        let mut cont = false;
        let expanded = self.agentui.expanded;
        {
            ui.set_width(w);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&t.agent).strong().size(16.0));
                let c = status_color(t.status);
                ui.label(egui::RichText::new(t.status.label()).color(c));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Minimize").clicked() {
                        close = true;
                    }
                    if ui.button(if expanded { "Shrink" } else { "Full Screen" }).clicked() {
                        toggle = true;
                    }
                });
            });
            ui.label(egui::RichText::new(&t.task_name).color(th().dim).size(12.5));
            if t.status == Status::Working {
                let (r, _) = ui.allocate_exact_size(vec2(w, 4.0), Sense::hover());
                progress(ui.painter(), r, now_t, th().accent);
                ui.label(egui::RichText::new(format!("{} · step {}", if t.activity.is_empty() { "Working…" } else { &t.activity }, t.steps)).color(th().dim).size(12.0));
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(h - 260.0).stick_to_bottom(true).show(ui, |ui| {
                for m in &t.messages {
                    let (fill, align, who) = match m.role.as_str() {
                        "you" => (mix(th().bg, th().accent, 0.28), egui::Align::Max, "You"),
                        "agent" => (mix(th().bg, th().text, 0.08), egui::Align::Min, t.agent.as_str()),
                        _ => (Color32::TRANSPARENT, egui::Align::Center, ""),
                    };
                    ui.with_layout(egui::Layout::top_down(align), |ui| {
                        if !who.is_empty() {
                            ui.label(egui::RichText::new(format!("{who} · {}", m.at.format("%-I:%M %p"))).color(th().faint).size(11.0));
                        }
                        egui::Frame::new().fill(fill).corner_radius(12).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                            ui.set_max_width(w * 0.82);
                            ui.label(egui::RichText::new(&m.text).color(if who.is_empty() { th().dim } else { th().text }));
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            if let Some((what, opts)) = &t.permission {
                ui.separator();
                ui.label(egui::RichText::new(format!("{} asks to use {what}", t.agent)).strong());
                ui.horizontal_wrapped(|ui| {
                    for (oid, name) in opts {
                        if ui.button(name).clicked() {
                            act = Some(("permission", json!({"option": oid})));
                        }
                    }
                });
            }
            if !t.changes.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(format!("Staged changes ({})", t.changes.len())).strong());
                egui::ScrollArea::vertical().id_salt("changes").max_height(120.0).show(ui, |ui| {
                    for c in &t.changes {
                        ui.label(format!("• {c}"));
                    }
                });
                if t.status == Status::Review {
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("Approve").color(th().on_accent)).fill(crate::hue("green"))).clicked() {
                            act = Some(("approve", json!({})));
                        }
                        if ui.button("Reject").clicked() {
                            act = Some(("reject", json!({})));
                        }
                    });
                }
            }
            ui.separator();
            match t.status {
                Status::Waiting | Status::Review => {
                    ui.add(crate::nativetext::Edit::multiline(&mut self.agentui.reply).hint_text("Reply to the agent").desired_rows(2).desired_width(w));
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.agentui.reply.trim().is_empty(), egui::Button::new("Send")).clicked() {
                            act = Some(("reply", json!({"text": self.agentui.reply.trim()})));
                            self.agentui.reply.clear();
                        }
                        if t.status == Status::Waiting && ui.button("Finish").clicked() {
                            act = Some(("finish", json!({})));
                        }
                    });
                }
                Status::Working | Status::Permission => {
                    if ui.button("Stop").clicked() {
                        act = Some(("cancel", json!({})));
                    }
                }
                Status::Expired | Status::Failed | Status::Done => {
                    if ui.button("Continue in a New Thread").clicked() {
                        cont = true;
                    }
                }
            }
            if !self.agentui.msg.is_empty() {
                ui.label(egui::RichText::new(&self.agentui.msg).color(th().overdue).size(12.0));
            }
        }
        (close, toggle, act, cont)
    }

    /// On an action's row: its thread's state as a small pill (tap to open), with a moving bar while it works.
    pub(crate) fn agent_row_badge(&mut self, ui: &mut egui::Ui, row: Rect, x: f32, my: f32, task: Id, in_agents_view: bool) -> f32 {
        let Some(t) = self.agentui.latest(task) else { return x };
        if !(t.status.active() || in_agents_view) {
            return x;
        }
        let (id, status, steps) = (t.id.clone(), t.status, t.steps);
        let c = status_color(status);
        let label = if status == Status::Working { format!("Agent · step {steps}") } else { format!("Agent · {}", status.label()) };
        let p = ui.painter().clone();
        let g = p.layout_no_wrap(label, FontId::proportional(12.0), c);
        let pill = Rect::from_min_size(egui::pos2(x + 2.0, my - 9.0), vec2(g.size().x + 16.0, 18.0));
        p.rect_filled(pill, 9.0, mix(th().bg, c, 0.18));
        p.rect_stroke(pill, 9.0, Stroke::new(1.0, mix(th().bg, c, 0.5)), egui::StrokeKind::Inside);
        p.galley(pill.left_center() + vec2(8.0, -g.size().y / 2.0), g, c);
        if ui.interact(pill, ui.id().with(("agent-pill", task)), Sense::click()).clicked() {
            self.agentui.open = Some(id);
        }
        if status == Status::Working {
            let bar = Rect::from_min_max(egui::pos2(row.left() + 52.0, row.bottom() - 4.0), egui::pos2(row.right() - 30.0, row.bottom() - 2.0));
            progress(&p, bar, ui.input(|i| i.time), c);
        }
        let _ = Align2::LEFT_CENTER;
        pill.right() + 8.0
    }
}
