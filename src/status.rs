//! Status: the health of GTDCraft's own subsystems (sync, and the server's decisions and agents), as small lights in
//! the bottom bar (tap for a summary), and Settings › Status, which adds the connections (Linear, GitHub, AI,
//! decisions provider) with Check, Reset and Troubleshoot.
//!
//! The lights read state the app already has (the last sync's result) plus the server's `GET /v1/health` (polled
//! every `POLL_SECS` while sync is set up). Connection checks only run when you press Check.
use crate::model::Dt;
use eframe::egui::{self, Color32};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};

const POLL_SECS: f64 = 30.0;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Light {
    /// Not set up, or not reported: not a problem.
    Off,
    Ok,
    Warn,
    Down,
}

impl Light {
    pub fn color(self) -> Color32 {
        match self {
            Light::Off => crate::th().faint,
            Light::Ok => crate::hue("green"),
            Light::Warn => crate::hue("orange"),
            Light::Down => crate::th().overdue,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Light::Off => "Off",
            Light::Ok => "OK",
            Light::Warn => "Trouble",
            Light::Down => "Down",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: &'static str,
    pub light: Light,
    pub detail: String,
}

/// What the app knows about its own sync, for `sync_row`.
pub struct SyncState<'a> {
    pub configured: bool,
    pub paused: bool,
    pub failures: u32,
    pub msg: &'a str,
    pub synced_at: Option<Dt>,
}

/// Sync, from this device's side: set up, paused, failing, or when it last worked. A server that doesn't answer
/// `/v1/health` is down even before a sync fails.
pub fn sync_row(s: &SyncState, health: Option<&Result<Value, String>>) -> Row {
    let (light, detail) = if !s.configured {
        (Light::Off, "Not set up (Settings › Sync)".to_string())
    } else if s.paused {
        (Light::Warn, "Paused: this database isn't the one this device syncs".into())
    } else if s.failures > 0 {
        (if s.failures >= 3 { Light::Down } else { Light::Warn }, format!("{} failed in a row: {}", s.failures, s.msg))
    } else if let Some(Err(e)) = health {
        (Light::Down, e.clone())
    } else if let Some(at) = s.synced_at {
        (Light::Ok, format!("Synced at {}", at.format("%-I:%M %p")))
    } else {
        (Light::Ok, "Waiting for the first sync".into())
    };
    Row { name: "Sync", light, detail }
}

/// The server's subsystems from its `/v1/health` (with the token it lists each one).
pub fn server_rows(health: Option<&Result<Value, String>>) -> Vec<Row> {
    let row = |name: &'static str, key: &str| {
        let (light, detail) = match health {
            None => (Light::Off, "Not checked yet".to_string()),
            Some(Err(_)) => (Light::Off, "Unknown: the server isn't answering".into()),
            Some(Ok(v)) => match v["subsystems"][key]["state"].as_str() {
                Some("up") => (Light::Ok, "Up".into()),
                Some("off") => (Light::Off, "Off on the server".into()),
                Some("unknown") => (Light::Off, "Not probed yet".into()),
                Some("down") => (Light::Down, v["subsystems"][key]["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("Down").to_string()),
                _ => (Light::Off, "The server doesn't report it (older server?)".into()),
            },
        };
        Row { name, light, detail }
    };
    vec![row("Decisions", "decisions"), row("Agents", "agents")]
}

/// The connection checks Settings › Status runs.
pub const CONNECTIONS: [&str; 4] = ["Linear", "GitHub", "AI", "Decisions provider"];

type Probe = (&'static str, Light, String);

/// Status state on the app: the last health answer and connection checks, and their pending requests.
pub struct Monitor {
    pub health: Option<Result<Value, String>>,
    pub health_at: Option<Dt>,
    health_rx: Option<Receiver<Result<Value, String>>>,
    polled: f64,
    pub synced_at: Option<Dt>,
    pub probes: BTreeMap<&'static str, (Light, String, Dt)>,
    probe_tx: Sender<Probe>,
    probe_rx: Receiver<Probe>,
    busy: std::collections::HashSet<&'static str>,
    diag_rx: Option<Receiver<Vec<String>>>,
    pub diag: Vec<String>,
}

impl Default for Monitor {
    fn default() -> Self {
        let (probe_tx, probe_rx) = channel();
        Monitor { health: None, health_at: None, health_rx: None, polled: f64::NEG_INFINITY, synced_at: None, probes: BTreeMap::new(), probe_tx, probe_rx, busy: Default::default(), diag_rx: None, diag: vec![] }
    }
}

impl Monitor {
    /// Folds in answers; asks the server again every `POLL_SECS` (or now, with `force`).
    pub fn tick(&mut self, ctx: &egui::Context, url: &str, token: &str, time: f64, force: bool) {
        if let Some(r) = self.health_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.health = Some(r);
            self.health_at = Some(crate::model::now());
            self.health_rx = None;
        }
        for (name, light, detail) in self.probe_rx.try_iter() {
            self.busy.remove(name);
            self.probes.insert(name, (light, detail, crate::model::now()));
        }
        if let Some(d) = self.diag_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.diag = d;
            self.diag_rx = None;
        }
        if url.trim().is_empty() {
            self.health = None;
            return;
        }
        if self.health_rx.is_none() && (force || time - self.polled > POLL_SECS) {
            self.polled = time;
            let (tx, rx) = channel();
            let (u, t, ctx) = (url.to_string(), token.to_string(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(crate::agent::request(&u, &t, "GET", "/v1/health", None));
                ctx.request_repaint();
            });
            self.health_rx = Some(rx);
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(POLL_SECS));
    }

    fn probe(&mut self, ctx: &egui::Context, name: &'static str, st: &crate::model::Settings) {
        if !self.busy.insert(name) {
            return;
        }
        let (tx, ctx, st) = (self.probe_tx.clone(), ctx.clone(), st.clone());
        std::thread::spawn(move || {
            let (light, detail) = check(name, &st);
            let _ = tx.send((name, light, detail));
            ctx.request_repaint();
        });
    }

    fn troubleshoot(&mut self, ctx: &egui::Context, url: &str, token: &str) {
        let (tx, rx) = channel();
        let (u, t, ctx) = (url.to_string(), token.to_string(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(diagnose(&u, &t));
            ctx.request_repaint();
        });
        self.diag = vec!["Checking…".into()];
        self.diag_rx = Some(rx);
    }
}

/// One connection check: a cheap authenticated call that says whether the key works.
fn check(name: &str, st: &crate::model::Settings) -> (Light, String) {
    let call = |req: ureq::Request| match req.timeout(std::time::Duration::from_secs(15)).call() {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(c, r)) => Err(format!("answered {c}: {}", r.into_string().unwrap_or_default().chars().take(160).collect::<String>())),
        Err(e) => Err(format!("unreachable: {e}")),
    };
    let res = |r: Result<(), String>, ok: &str| match r {
        Ok(()) => (Light::Ok, ok.to_string()),
        Err(e) => (Light::Down, e),
    };
    match name {
        "Linear" => match crate::linear::api_key(&st.linear_key) {
            None => (Light::Off, "No key (Settings › Agent, or LINEAR_API_KEY)".into()),
            Some(k) => match crate::linear::gql(&k, "{ viewer { name } }", serde_json::json!({})) {
                Ok(v) => (Light::Ok, format!("Signed in as {}", v["viewer"]["name"].as_str().unwrap_or("?"))),
                Err(e) => (Light::Down, e),
            },
        },
        "GitHub" => match crate::refs::token(&st.github_token) {
            None => (Light::Off, "No token: public repos only".into()),
            Some(t) => res(call(ureq::get("https://api.github.com/user").set("User-Agent", "GTDCraft").set("Authorization", &format!("Bearer {t}"))), "Token works"),
        },
        "AI" => {
            let env = |var: &str| Some(st.ai_key.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| std::env::var(var).ok().filter(|k| !k.is_empty()));
            if st.ai_provider == "openai" {
                let base = Some(st.ai_base_url.trim().trim_end_matches('/')).filter(|b| !b.is_empty()).unwrap_or("https://api.openai.com/v1").to_string();
                let mut req = ureq::get(&format!("{base}/models"));
                if let Some(k) = env("OPENAI_API_KEY") {
                    req = req.set("Authorization", &format!("Bearer {k}"));
                }
                res(call(req), &format!("{base} answers"))
            } else {
                match env("ANTHROPIC_API_KEY") {
                    None => (Light::Off, "No key (Settings › Agent, or ANTHROPIC_API_KEY)".into()),
                    Some(k) => res(call(ureq::get("https://api.anthropic.com/v1/models").set("x-api-key", &k).set("anthropic-version", "2023-06-01")), "Key works"),
                }
            }
        }
        "Decisions provider" => {
            if !st.decisions_on {
                return (Light::Off, "Decisions are off".into());
            }
            match crate::systemone::route(st).and_then(|r| crate::systemone::test(&r)) {
                Ok(m) => (Light::Ok, m),
                Err(e) => (Light::Down, e),
            }
        }
        _ => (Light::Off, String::new()),
    }
}

/// Sync troubleshooting, step by step: the address resolves, the server answers, the token is accepted. Each line
/// says what was tried and what came back (the token itself is never shown).
pub fn diagnose(url: &str, token: &str) -> Vec<String> {
    let mut out = vec![format!("Server: {url}")];
    let base = url.trim().trim_end_matches('/');
    let host_port = base.split("://").nth(1).unwrap_or(base).split('/').next().unwrap_or("");
    let with_port = if host_port.contains(':') { host_port.to_string() } else { format!("{host_port}:80") };
    match std::net::ToSocketAddrs::to_socket_addrs(&with_port) {
        Ok(a) => out.push(format!("✓ {host_port} resolves to {}", a.map(|a| a.ip().to_string()).collect::<Vec<_>>().join(", "))),
        Err(e) => {
            out.push(format!("✗ {host_port} doesn't resolve: {e}"));
            if host_port.contains(".lan") {
                out.push("  Try this Mac's .local name instead (Settings › Sync on the Mac, or scan its pairing code).".into());
            }
            return out;
        }
    }
    match ureq::get(&format!("{base}/v1/health")).timeout(std::time::Duration::from_secs(8)).call() {
        Ok(_) => out.push("✓ the server answers".into()),
        Err(e) => {
            out.push(format!("✗ no answer from the server: {e}"));
            out.push("  Is the sync server running on that machine, and is this device on the same network?".into());
            return out;
        }
    }
    match crate::agent::request(base, token, "GET", "/v1/databases", None) {
        Ok(v) => out.push(format!("✓ the token is accepted; the server has {} database(s)", v.as_array().map_or(0, |a| a.len()))),
        Err(e) => out.push(format!("✗ {e} (check the token in Settings › Sync)")),
    }
    out
}

fn row_ui(ui: &mut egui::Ui, r: &Row) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, r.light.color());
        ui.label(egui::RichText::new(r.name).strong());
        ui.label(egui::RichText::new(r.light.word()).color(crate::th().dim).size(12.0));
    });
    if !r.detail.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(&r.detail).color(crate::th().dim).size(12.0)).wrap());
    }
}

impl crate::Gtd {
    /// The lights' rows: sync and the server's subsystems.
    pub(crate) fn status_rows(&self) -> Vec<Row> {
        let st = &self.store.settings;
        let s = SyncState { configured: !st.sync_url.trim().is_empty(), paused: self.sync_paused, failures: self.sync_failures, msg: &self.sync_msg, synced_at: self.status.synced_at };
        let mut rows = vec![sync_row(&s, self.status.health.as_ref())];
        rows.extend(server_rows(self.status.health.as_ref()));
        rows
    }

    pub(crate) fn status_tick(&mut self, ctx: &egui::Context) {
        let (url, token) = (self.store.settings.sync_url.clone(), self.store.settings.sync_token.clone());
        self.status.tick(ctx, &url, &token, self.time, false);
    }

    fn status_recheck(&mut self, ctx: &egui::Context) {
        let (url, token) = (self.store.settings.sync_url.clone(), self.store.settings.sync_token.clone());
        self.status.tick(ctx, &url, &token, self.time, true);
        self.sync_due = Some(0.0);
    }

    /// The bottom bar's lights, one dot per subsystem, centred on `at`; a tap opens the summary.
    pub(crate) fn status_dots(&mut self, ui: &mut egui::Ui, at: egui::Pos2) {
        let rows = self.status_rows();
        let gap = 9.0;
        let w = gap * rows.len() as f32;
        let hit = egui::Rect::from_center_size(at, egui::vec2(w + 16.0, 22.0));
        let resp = ui.interact(hit, ui.id().with("status-dots"), egui::Sense::click()).on_hover_text("Status");
        for (i, r) in rows.iter().enumerate() {
            let c = egui::pos2(at.x - w / 2.0 + gap * (i as f32 + 0.5), at.y);
            // subtle: soft when all is well, full strength when something needs you
            let a = if matches!(r.light, Light::Warn | Light::Down) { 1.0 } else { 0.55 };
            ui.painter().circle_filled(c, 2.6, r.light.color().gamma_multiply(a));
        }
        let mut open_page = false;
        let mut retry = false;
        egui::Popup::from_toggle_button_response(&resp)
            .align(egui::RectAlign::TOP)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(280.0)
            .show(|ui| {
                ui.label(egui::RichText::new("Status").strong());
                for r in &rows {
                    ui.add_space(4.0);
                    row_ui(ui, r);
                }
                if let Some(at) = self.status.health_at {
                    ui.label(egui::RichText::new(format!("Server checked at {}", at.format("%-I:%M:%S %p"))).color(crate::th().faint).size(11.0));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    retry = ui.button("Retry").clicked();
                    open_page = ui.button("Status Page…").clicked();
                });
            });
        if retry {
            self.status_recheck(ui.ctx());
        }
        if open_page {
            egui::Popup::close_all(ui.ctx());
            self.settings_tab = STATUS_TAB;
            self.show_settings = true;
        }
    }

    /// Settings › Status: every subsystem, then the connections, each with Check / Reset / Troubleshoot.
    pub(crate) fn settings_status(&mut self, ui: &mut egui::Ui) -> bool {
        let mut ch = false;
        let ctx = ui.ctx().clone();
        let th = crate::th();
        ui.heading("GTDCraft");
        for r in self.status_rows() {
            ui.add_space(4.0);
            row_ui(ui, &r);
        }
        if let Some(at) = self.status.health_at {
            ui.label(egui::RichText::new(format!("Server checked at {}", at.format("%-I:%M:%S %p"))).color(th.faint).size(11.0));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Check Now").on_hover_text("Ask the server again and sync").clicked() {
                self.status_recheck(&ctx);
            }
            if ui.button("Reset Sync").on_hover_text("Forget past failures and the last revision, then do a full sync").clicked() {
                (self.sync_failures, self.sync_fail_hidden) = (0, false);
                self.last_rev.clear();
                self.status_recheck(&ctx);
            }
            if ui.button("Troubleshoot Sync").on_hover_text("Check the address, the server and the token, step by step").clicked() {
                let (u, t) = (self.store.settings.sync_url.clone(), self.store.settings.sync_token.clone());
                self.status.troubleshoot(&ctx, &u, &t);
            }
        });
        if !self.status.diag.is_empty() {
            let text = self.status.diag.join("\n");
            egui::Frame::new().fill(th.field).corner_radius(8).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                ui.label(egui::RichText::new(&text).monospace().size(12.0));
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(text.clone());
                }
            });
        }
        ui.separator();
        ui.heading("Connections");
        ui.label(egui::RichText::new("Checked only when you ask. Reset clears the saved key on every device (keys from environment variables stay).").color(th.dim).size(12.0));
        for name in CONNECTIONS {
            ui.add_space(6.0);
            let (light, detail) = match self.status.probes.get(name) {
                Some((l, d, at)) => (*l, format!("{d} · checked {}", at.format("%-I:%M %p"))),
                None => (Light::Off, "Not checked".into()),
            };
            row_ui(ui, &Row { name, light, detail });
            ui.horizontal(|ui| {
                if self.status.busy.contains(name) {
                    ui.spinner();
                } else if ui.small_button("Check").clicked() {
                    self.status.probe(&ctx, name, &self.store.settings);
                }
                if ui.small_button("Reset").clicked() {
                    let st = &mut self.store.settings;
                    match name {
                        "Linear" => st.linear_key.clear(),
                        "GitHub" => st.github_token.clear(),
                        "AI" => st.ai_key.clear(),
                        _ => st.decisions_key.clear(),
                    }
                    st.connections_modified = Some(crate::model::now());
                    self.status.probes.remove(name);
                    ch = true;
                }
                if ui.small_button("Troubleshoot").on_hover_text("Where its settings are").clicked() {
                    self.settings_tab = if cfg!(target_os = "ios") { 10 } else { 5 };
                }
            });
        }
        ch
    }
}

/// Settings › Status's `settings_tab` number.
pub const STATUS_TAB: u8 = 11;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn st(configured: bool, failures: u32) -> SyncState<'static> {
        SyncState { configured, paused: false, failures, msg: "boom", synced_at: None }
    }

    #[test]
    fn sync_light_follows_state() {
        assert_eq!(sync_row(&st(false, 0), None).light, Light::Off);
        assert_eq!(sync_row(&st(true, 0), None).light, Light::Ok);
        assert_eq!(sync_row(&st(true, 1), None).light, Light::Warn);
        assert_eq!(sync_row(&st(true, 3), None).light, Light::Down);
        let unreachable = Err("Couldn't reach the sync server".to_string());
        assert_eq!(sync_row(&st(true, 0), Some(&unreachable)).light, Light::Down);
        let paused = SyncState { paused: true, ..st(true, 0) };
        assert_eq!(sync_row(&paused, None).light, Light::Warn);
    }

    #[test]
    fn server_rows_read_health() {
        let h = Ok(json!({"ok": true, "subsystems": {"decisions": {"state": "down", "error": "unreachable: clef"}, "agents": {"state": "up"}}}));
        let rows = server_rows(Some(&h));
        assert_eq!((rows[0].light, rows[0].detail.as_str()), (Light::Down, "unreachable: clef"));
        assert_eq!(rows[1].light, Light::Ok);
        // an older server (no subsystems) and an unreachable one aren't alarms by themselves
        assert!(server_rows(Some(&Ok(json!({"ok": true})))).iter().all(|r| r.light == Light::Off));
        assert!(server_rows(Some(&Err("x".into()))).iter().all(|r| r.light == Light::Off));
    }

    #[test]
    fn diagnose_stops_at_an_unresolvable_host() {
        let d = diagnose("http://no-such-host.invalid:8765", "t");
        assert!(d[1].starts_with('✗'), "{d:?}");
        assert_eq!(d.len(), 2);
    }
}
