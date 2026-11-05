//! Places for the Nearby perspective: map (Apple MapKit on macOS/iOS, OpenStreetMap fallback),
//! address search (Nominatim), Core Location for "here", location-triggered notifications (iOS),
//! and reminders through UserNotifications.
use crate::model::Place;
use crate::notes::UA;
use crate::theme::c;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};
use std::f64::consts::PI;

pub struct MapView {
    pub lat: f64,
    pub lon: f64,
    pub zoom: u8,
    pub tiles: Tiles,
    /// Apple MapKit snapshot cache (macOS/iOS).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub apple: apple_map::AppleMap,
}

impl MapView {
    pub fn new(lat: f64, lon: f64, zoom: u8) -> Self {
        Self {
            lat,
            lon,
            zoom,
            tiles: Tiles::default(),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            apple: apple_map::AppleMap::default(),
        }
    }
}

/// OpenStreetMap tiles fetched with our own User-Agent, cached on disk (tile usage policy asks for both),
/// tinted to the theme. At most 4 downloads at a time.
#[derive(Default)]
pub struct Tiles {
    ready: std::collections::HashMap<(u8, i64, i64, bool), egui::TextureHandle>,
    pending: std::collections::HashSet<(u8, i64, i64)>,
    failed: std::collections::HashSet<(u8, i64, i64)>,
    rx: Option<std::sync::mpsc::Receiver<((u8, i64, i64), Option<Vec<u8>>)>>,
    tx: Option<std::sync::mpsc::Sender<((u8, i64, i64), Option<Vec<u8>>)>>,
    raw: std::collections::HashMap<(u8, i64, i64), egui::ColorImage>,
}

impl Tiles {
    fn get(&mut self, ctx: &egui::Context, k: (u8, i64, i64), dark: bool, bg: Color32) -> Option<egui::TextureHandle> {
        if self.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
        while let Some(Ok((key, bytes))) = self.rx.as_ref().map(|r| r.try_recv()) {
            self.pending.remove(&key);
            match bytes.and_then(|b| image::load_from_memory(&b).ok()) {
                Some(img) => {
                    let rgba = img.to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    self.raw.insert(key, ci);
                }
                None => {
                    self.failed.insert(key);
                }
            }
        }
        if let Some(t) = self.ready.get(&(k.0, k.1, k.2, dark)) {
            return Some(t.clone());
        }
        if let Some(ci) = self.raw.get(&k) {
            let mut img = ci.clone();
            if dark {
                // dark map from light tiles, pulled toward the theme background
                for px in img.pixels.iter_mut() {
                    // invert, then rotate hue 180deg (2*mean - c) so water stays blue and parks green
                    let inv = [255 - px.r() as i32, 255 - px.g() as i32, 255 - px.b() as i32];
                    let m2 = (inv[0] + inv[1] + inv[2]) * 2 / 3;
                    let ch = |v: i32| (m2 - v).clamp(0, 255) as u8;
                    let rot = Color32::from_rgb(ch(inv[0]), ch(inv[1]), ch(inv[2]));
                    *px = crate::theme::mix(rot, bg, 0.3);
                }
            } else {
                for px in img.pixels.iter_mut() {
                    *px = crate::theme::mix(*px, bg, 0.15);
                }
            }
            let t = ctx.load_texture(format!("tile-{k:?}-{dark}"), img, egui::TextureOptions::LINEAR);
            self.ready.insert((k.0, k.1, k.2, dark), t.clone());
            return Some(t);
        }
        if !self.pending.contains(&k) && !self.failed.contains(&k) && self.pending.len() < 4 {
            self.pending.insert(k);
            let tx = self.tx.clone().unwrap();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((k, fetch_tile(k)));
                ctx.request_repaint();
            });
        }
        None
    }
}

fn tile_cache_path(k: (u8, i64, i64)) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(format!("Library/Caches/GTDCraft/tiles/{}/{}/{}.png", k.0, k.1, k.2))
}

fn fetch_tile(k: (u8, i64, i64)) -> Option<Vec<u8>> {
    let path = tile_cache_path(k);
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 30 * 86400);
    if fresh {
        if let Ok(b) = std::fs::read(&path) {
            return Some(b);
        }
    }
    let url = format!("https://tile.openstreetmap.org/{}/{}/{}.png", k.0, k.1, k.2);
    let resp = ureq::get(&url).set("User-Agent", UA).timeout(std::time::Duration::from_secs(10)).call().ok()?;
    let mut b = vec![];
    use std::io::Read;
    resp.into_reader().take(2 * 1024 * 1024).read_to_end(&mut b).ok()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, &b);
    }
    Some(b)
}

pub struct Pin {
    pub place: Place,
    pub color: Color32,
    pub count: usize,
}

fn world(z: u8) -> f64 {
    256.0 * 2f64.powi(z as i32)
}
pub fn project(lat: f64, lon: f64, z: u8) -> (f64, f64) {
    let w = world(z);
    let s = lat.to_radians();
    ((lon + 180.0) / 360.0 * w, (1.0 - (s.tan() + 1.0 / s.cos()).ln() / PI) / 2.0 * w)
}
pub fn unproject(x: f64, y: f64, z: u8) -> (f64, f64) {
    let w = world(z);
    let lon = x / w * 360.0 - 180.0;
    let n = PI - 2.0 * PI * y / w;
    (n.sinh().atan().to_degrees(), lon)
}

/// The visible region as (lat, lon, lat_span, lon_span) for the given map center and zoom.
pub fn visible_region(lat: f64, lon: f64, z: u8, w_px: f64, h_px: f64) -> (f64, f64, f64, f64) {
    let (cx, cy) = project(lat, lon, z);
    let (lat0, lon0) = unproject(cx - w_px / 2.0, cy - h_px / 2.0, z);
    let (lat1, lon1) = unproject(cx + w_px / 2.0, cy + h_px / 2.0, z);
    (lat, lon, (lat0 - lat1).abs(), (lon1 - lon0).abs())
}

/// Pick the nearest N places from `here`, sorted by distance.
/// Used by iOS location notifications and tests; on macOS the caller is in main.rs (future).
#[allow(dead_code)]
pub fn nearest_n(here: &Place, places: &[(Place, u64)], n: usize) -> Vec<(Place, u64)> {
    let mut sorted: Vec<_> = places.iter().map(|(p, id)| (here.km_to(p), p.clone(), *id)).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    sorted.into_iter().take(n).map(|(_, p, id)| (p, id)).collect()
}

/// Cache key for a map snapshot: (lat truncated to ~100m, lon truncated to ~100m, zoom, width, height, dark).
pub fn snap_cache_key(lat: f64, lon: f64, z: u8, w: u32, h: u32, dark: bool) -> (i64, i64, u8, u32, u32, bool) {
    // Truncate to ~0.001 degrees (~100m) so small pans reuse the cache
    ((lat * 1000.0) as i64, (lon * 1000.0) as i64, z, w, h, dark)
}

/// Draws the map; returns the place clicked (lat, lon), if any.
pub fn map(ui: &mut egui::Ui, view: &mut MapView, size: egui::Vec2, pins: &[Pin], here: Option<&Place>) -> Option<(f64, f64)> {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let cs = c();
    let z = view.zoom.clamp(2, 18);
    let (cx, cy) = project(view.lat, view.lon, z);
    if resp.dragged() {
        let d = resp.drag_delta();
        let (lat, lon) = unproject(cx - d.x as f64, cy - d.y as f64, z);
        view.lat = lat.clamp(-84.0, 84.0);
        view.lon = lon;
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll > 8.0 && view.zoom < 18 {
            view.zoom += 1;
        } else if scroll < -8.0 && view.zoom > 2 {
            view.zoom -= 1;
        }
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    child.painter().rect_filled(rect, 0.0, cs.field);

    // Try Apple MapKit snapshot on macOS/iOS; fall back to OSM tiles
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let apple_ok = {
        let region = visible_region(view.lat, view.lon, z, rect.width() as f64, rect.height() as f64);
        view.apple.draw(ui.ctx(), &mut child, rect, region, cs.map_dark)
    };
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let apple_ok = false;

    if !apple_ok {
        // OSM tile fallback
        let n = 1i64 << z;
        let x0 = ((cx - rect.width() as f64 / 2.0) / 256.0).floor() as i64;
        let x1 = ((cx + rect.width() as f64 / 2.0) / 256.0).floor() as i64;
        let y0 = ((cy - rect.height() as f64 / 2.0) / 256.0).floor() as i64;
        let y1 = ((cy + rect.height() as f64 / 2.0) / 256.0).floor() as i64;
        for ty in y0.max(0)..=y1.min(n - 1) {
            for tx in x0..=x1 {
                let wx = tx.rem_euclid(n);
                let min = rect.center() + vec2((tx as f64 * 256.0 - cx) as f32, (ty as f64 * 256.0 - cy) as f32);
                let r = Rect::from_min_size(min, vec2(256.0, 256.0));
                if let Some(tex) = view.tiles.get(ui.ctx(), (z, wx, ty), cs.map_dark, cs.bg) {
                    child.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
        }
    }

    let to_screen = |p: &Place| {
        let (x, y) = project(p.lat, p.lon, z);
        rect.center() + vec2((x - cx) as f32, (y - cy) as f32)
    };
    let painter = child.painter();
    if let Some(h) = here {
        let p = to_screen(h);
        painter.circle_filled(p, 9.0, Color32::WHITE);
        painter.circle_filled(p, 6.5, cs.accent);
    }
    // metres per screen point at this latitude and zoom (Web Mercator, 256-pt tiles)
    let mpp = 156_543.033_92 * view.lat.to_radians().cos() / 2f64.powi(z as i32);
    for pin in pins {
        let p = to_screen(&pin.place);
        let r = (pin.place.radius() / mpp) as f32;
        if r > 6.0 {
            painter.circle(p, r, pin.color.gamma_multiply(0.12), Stroke::new(1.0, pin.color.gamma_multiply(0.6)));
        }
        draw_pin(painter, p, pin.color);
        let label = if pin.count > 0 { format!("{} \u{00b7} {}", pin.place.label, pin.count) } else { pin.place.label.clone() };
        let g = painter.layout_no_wrap(label, FontId::proportional(12.5), cs.text);
        let r = Rect::from_min_size(p + vec2(12.0, -30.0), g.size() + vec2(10.0, 6.0));
        painter.rect_filled(r, 5.0, cs.rail.gamma_multiply(0.92));
        painter.galley(r.min + vec2(5.0, 3.0), g, cs.text);
    }
    // Attribution: Apple Maps snapshot includes its own legal text; show OSM credit only for OSM
    if !apple_ok {
        painter.text(rect.left_bottom() + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, "\u{00a9} OpenStreetMap contributors", FontId::proportional(10.5), cs.dim);
    }
    // zoom buttons
    let zr = Rect::from_min_size(rect.right_bottom() - vec2(40.0, 76.0), vec2(30.0, 62.0));
    painter.rect_filled(zr, 8.0, cs.rail.gamma_multiply(0.95));
    let plus = Rect::from_min_size(zr.min, vec2(30.0, 31.0));
    let minus = Rect::from_min_size(zr.min + vec2(0.0, 31.0), vec2(30.0, 31.0));
    painter.text(plus.center(), Align2::CENTER_CENTER, "+", FontId::proportional(18.0), cs.text);
    painter.text(minus.center(), Align2::CENTER_CENTER, "\u{2212}", FontId::proportional(18.0), cs.text);
    if ui.interact(plus, ui.id().with("zin"), Sense::click()).clicked() && view.zoom < 18 {
        view.zoom += 1;
    }
    if ui.interact(minus, ui.id().with("zout"), Sense::click()).clicked() && view.zoom > 2 {
        view.zoom -= 1;
    }
    if resp.clicked() {
        let p = resp.interact_pointer_pos()?;
        let d = p - rect.center();
        return Some(unproject(cx + d.x as f64, cy + d.y as f64, z));
    }
    None
}

fn draw_pin(p: &egui::Painter, at: Pos2, col: Color32) {
    p.add(egui::Shape::convex_polygon(vec![at + vec2(-6.0, -12.0), at + vec2(6.0, -12.0), at], col, Stroke::NONE));
    p.circle_filled(at - vec2(0.0, 16.0), 8.5, col);
    p.circle_filled(at - vec2(0.0, 16.0), 3.2, Color32::from_black_alpha(140));
}

/// A place search result: a business or address the person picks from a list.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    /// One line per row, as `address_lines` writes it.
    pub address: String,
    pub lat: f64,
    pub lon: f64,
}

impl Found {
    /// False when the name only repeats the address's street line (address results), so it isn't shown twice.
    pub fn distinct_name(&self) -> bool {
        !self.name.is_empty() && self.address.lines().next() != Some(self.name.as_str())
    }
    /// The tag's place for this result, keeping the old place's size and notification.
    pub fn place(&self, old: Option<&Place>) -> Place {
        let label = if self.name.is_empty() { self.address.lines().next().unwrap_or_default().to_string() } else { self.name.clone() };
        Place::relocated(old, label, self.address.clone(), self.lat, self.lon)
    }
}

/// A postal address as the iPhone inspector shows it, from (number, street, city, state, postcode, country):
/// "7535 Torrey Santa Fe Rd" / "San Diego CA 92129" / "United States". Empty parts are skipped.
pub fn address_lines(p: &[&str; 6]) -> String {
    let join = |xs: &[&str]| xs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    [join(&p[0..2]), join(&p[2..5]), join(&p[5..6])].into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Where a search should look first: the device's location, else the saved "My location".
pub fn search_bias(current: Option<&Place>, here: Option<&Place>) -> Option<(f64, f64)> {
    current.or(here).map(|p| (p.lat, p.lon))
}

/// Place search: Apple Maps (`MKLocalSearch`) on Mac and iPhone, near `near` when known; OpenStreetMap Nominatim
/// elsewhere, and if Apple Maps fails. `address` limits Apple's results to addresses (the "Address" option).
/// Returns where the results came from ("Apple Maps" / "OpenStreetMap") too.
/// Blocks: call it off the main thread (MapKit answers on the main queue).
pub fn search(query: &str, near: Option<(f64, f64)>, address: bool) -> Result<(&'static str, Vec<Found>), String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    match mk::search(query, near, address) {
        Ok(v) if !v.is_empty() => return Ok(("Apple Maps", v)),
        Ok(_) => {}
        Err(e) => eprintln!("Apple Maps search failed ({e}); trying OpenStreetMap"),
    }
    let _ = (near, address);
    nominatim(query).map(|v| ("OpenStreetMap", v))
}

/// The postal address at a spot (Apple's `CLGeocoder`), for "Here" and pins; None off Apple or when unknown.
pub fn reverse(lat: f64, lon: f64) -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return mk::reverse(lat, lon);
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = (lat, lon);
        None
    }
}

/// Whether distances read in miles and feet here (the locale's measurement system).
pub fn imperial() -> bool {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return !objc2_foundation::NSLocale::currentLocale().usesMetricSystem();
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    false
}

/// Address search via OpenStreetMap Nominatim (one request per search, as its usage policy asks).
fn nominatim(query: &str) -> Result<Vec<Found>, String> {
    let resp = ureq::get("https://nominatim.openstreetmap.org/search")
        .query("format", "json")
        .query("limit", "8")
        .query("q", query)
        .set("User-Agent", UA)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?;
    let v: Vec<serde_json::Value> = serde_json::from_str(&resp.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let s = |r: &serde_json::Value, k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let found: Vec<Found> = v
        .iter()
        .filter_map(|r| Some(Found { name: s(r, "name"), address: s(r, "display_name"), lat: s(r, "lat").parse().ok()?, lon: s(r, "lon").parse().ok()? }))
        .collect();
    if found.is_empty() {
        return Err(format!("No place found for \u{201c}{query}\u{201d}"));
    }
    Ok(found)
}

/// Shows a macOS notification (Notification Center, via the system's AppleScript runner).
/// A notification, the way Apple platform apps send them (UserNotifications: GTDCraft's own name and icon, Notification
/// Center, banners while GTDCraft is in front too). Outside an app bundle (the bare `target/release` binary has no
/// bundle for UserNotifications to belong to) it falls back to AppleScript's `display notification` on the Mac.
pub fn notify(title: &str, body: &str) {
    notify_for(None, title, body);
}

/// A reminder for an action: it carries Complete and Snooze buttons (see `un`). (iPhone/iPad
/// schedule theirs ahead with `schedule` instead.)
#[cfg(not(target_os = "ios"))]
pub fn notify_task(task: u64, title: &str, body: &str) {
    notify_for(Some(task), title, body);
}

fn notify_for(task: Option<u64>, title: &str, body: &str) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if un::available() {
        un::send(title, body, None, task);
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = task;
    // Linux (and other non-Apple desktops): libnotify's notify-send, as desktop apps there do
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = std::process::Command::new("notify-send").args(["--app-name=GTDCraft", title, body]).spawn();
    #[cfg(target_os = "macos")]
    {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"GTDCraft\" subtitle \"{}\" sound name \"default\"", esc(body), esc(title));
        let _ = std::process::Command::new("osascript").arg("-e").arg(script).spawn();
    }
}

/// iPhone/iPad: replace the scheduled reminders with these (seconds from now, task, title, body), so they arrive
/// while GTDCraft is closed — iOS keeps at most 64 pending.
#[cfg(target_os = "ios")]
pub fn schedule(alarms: &[(f64, u64, String, String)]) {
    if un::available() {
        un::schedule(alarms);
    }
}

/// iPhone/iPad: schedule location-triggered notifications for the nearest places (call right after `schedule`,
/// which clears the old ones). iOS allows at most
/// 64 pending notifications total, but only ~20 location regions can be monitored concurrently.
/// We take the nearest 20 places from `here` and schedule a UNLocationNotificationTrigger for each.
#[cfg(target_os = "ios")]
pub fn schedule_location(here: &Place, places: &[(Place, u64, String, String)]) {
    if un::available() {
        un::schedule_location(here, places);
    }
}

/// Whether iOS lets GTDCraft show notifications: None until asked, then Some(allowed). Refreshed by `check_permission`.
#[cfg(target_os = "ios")]
pub fn permission() -> Option<bool> {
    match un::PERMISSION.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// Ask iOS for the current notification permission (it arrives a moment later in `permission`); asking also shows the
/// system's prompt the first time.
#[cfg(target_os = "ios")]
pub fn check_permission() {
    if un::available() {
        un::check_permission();
    }
}

/// What was tapped on a reminder, for the app to act on: ("complete" | "clockin" | "open", task).
pub fn take_notification_actions() -> Vec<(String, u64)> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return un::ACTIONS.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    vec![]
}

/// Snooze: the next whole minute after now (`nextDateAfterDate:matchingUnit:NSCalendarUnitSecond value:0`), plus 15
/// minutes. `secs_into_minute` is now's seconds past the minute, 0 <= s < 60.
pub fn snooze_delay(secs_into_minute: f64) -> f64 {
    const DEFAULT_SNOOZED_ALARM_MINUTES: f64 = 15.0;
    (60.0 - secs_into_minute) + DEFAULT_SNOOZED_ALARM_MINUTES * 60.0
}

// ─── Apple MapKit snapshot (macOS / iOS) ───────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod apple_map {
    use eframe::egui::{self, Color32, Rect};
    use objc2::AllocAnyThread;
    use objc2_core_location::CLLocationCoordinate2D;
    use objc2_map_kit::*;
    use std::sync::mpsc;

    type SnapKey = (i64, i64, u8, u32, u32, bool);

    /// Manages MKMapSnapshotter results: one pending snapshot at a time, cached by region.
    #[derive(Default)]
    pub struct AppleMap {
        cache: std::collections::HashMap<SnapKey, egui::TextureHandle>,
        pending: Option<SnapKey>,
        rx: Option<mpsc::Receiver<(SnapKey, Option<egui::ColorImage>)>>,
        tx: Option<mpsc::Sender<(SnapKey, Option<egui::ColorImage>)>>,
        failed: bool,
    }

    impl AppleMap {
        /// Draw the Apple Maps snapshot into `child` at `rect`. Returns true if drawn.
        /// `region` = (lat, lon, lat_span, lon_span) from `visible_region`.
        pub fn draw(
            &mut self,
            ctx: &egui::Context,
            child: &mut egui::Ui,
            rect: Rect,
            region: (f64, f64, f64, f64),
            dark: bool,
        ) -> bool {
            if self.failed {
                return false;
            }
            if self.tx.is_none() {
                let (tx, rx) = mpsc::channel();
                self.tx = Some(tx);
                self.rx = Some(rx);
            }
            // Drain completed snapshots
            while let Some(Ok((key, img))) = self.rx.as_ref().map(|r| r.try_recv()) {
                if self.pending.as_ref() == Some(&key) {
                    self.pending = None;
                }
                match img {
                    Some(ci) => {
                        let tex = ctx.load_texture(format!("apple-map-{key:?}"), ci, egui::TextureOptions::LINEAR);
                        self.cache.insert(key, tex);
                    }
                    None => {
                        self.failed = true;
                        return false;
                    }
                }
            }

            let key = super::snap_cache_key(region.0, region.1, 0, rect.width() as u32, rect.height() as u32, dark);

            if let Some(tex) = self.cache.get(&key) {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                child.painter().image(tex.id(), rect, uv, Color32::WHITE);
                return true;
            }

            // Request a new snapshot if none pending
            if self.pending.is_none() {
                self.pending = Some(key);
                let tx = self.tx.clone().unwrap();
                let ctx2 = ctx.clone();
                let w = rect.width() as f64;
                let h = rect.height() as f64;
                let (lat, lon, lat_span, lon_span) = region;
                std::thread::spawn(move || {
                    let img = take_snapshot(lat, lon, lat_span, lon_span, w, h, dark);
                    let _ = tx.send((key, img));
                    ctx2.request_repaint();
                });
            }

            // Show a stale cached frame if we have one nearby (any key)
            if let Some((_, tex)) = self.cache.iter().next() {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                child.painter().image(tex.id(), rect, uv, Color32::WHITE);
                return true;
            }

            false
        }
    }

    /// Take a synchronous MKMapSnapshotter snapshot. Returns None on failure.
    fn take_snapshot(lat: f64, lon: f64, lat_span: f64, lon_span: f64, w: f64, h: f64, dark: bool) -> Option<egui::ColorImage> {
        unsafe {
            let opts = MKMapSnapshotOptions::new();
            let coord = CLLocationCoordinate2D { latitude: lat, longitude: lon };
            let span = MKCoordinateSpan { latitudeDelta: lat_span, longitudeDelta: lon_span };
            let region = MKCoordinateRegion { center: coord, span };
            opts.setRegion(region);
            opts.setSize(objc2_foundation::NSSize { width: w, height: h });

            // Dark appearance
            if dark {
                #[cfg(target_os = "macos")]
                {
                    use objc2_app_kit::NSAppearance;
                    let name = objc2_foundation::NSString::from_str("NSAppearanceNameDarkAqua");
                    if let Some(appearance) = NSAppearance::appearanceNamed(&name) {
                        opts.setAppearance(Some(&appearance));
                    }
                }
                // iOS: use a dark map configuration
                #[cfg(target_os = "ios")]
                {
                    // MKStandardMapConfiguration with emphasis style .muted for dark
                    let config = MKStandardMapConfiguration::new();
                    // emphasisStyle 1 = muted (darker), via msg_send since the typed API may not expose it
                    let _: () = objc2::msg_send![&config, setEmphasisStyle: 1i64];
                    opts.setPreferredConfiguration(&config);
                }
            }

            let snapshotter = MKMapSnapshotter::initWithOptions(MKMapSnapshotter::alloc(), &opts);

            // Synchronous wait via a semaphore
            let (tx, rx) = std::sync::mpsc::channel::<Option<egui::ColorImage>>();
            let block = block2::RcBlock::new(move |snapshot: *mut MKMapSnapshot, error: *mut objc2_foundation::NSError| {
                if !error.is_null() || snapshot.is_null() {
                    let _ = tx.send(None);
                    return;
                }
                let snap = &*snapshot;
                let img = snapshot_to_image(snap);
                let _ = tx.send(img);
            });
            snapshotter.startWithCompletionHandler(&*block as *const _ as *mut _);
            rx.recv_timeout(std::time::Duration::from_secs(10)).ok().flatten()
        }
    }

    /// Convert an MKMapSnapshot to an egui::ColorImage.
    #[cfg(target_os = "macos")]
    fn snapshot_to_image(snap: &MKMapSnapshot) -> Option<egui::ColorImage> {
        unsafe {
            let ns_image = snap.image();
            let tiff = ns_image.TIFFRepresentation()?;
            let bytes = tiff.to_vec();
            let img = image::load_from_memory(&bytes).ok()?;
            let rgba = img.to_rgba8();
            Some(egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            ))
        }
    }

    /// Convert an MKMapSnapshot to an egui::ColorImage (iOS: snapshot.image is a UIImage).
    #[cfg(target_os = "ios")]
    fn snapshot_to_image(snap: &MKMapSnapshot) -> Option<egui::ColorImage> {
        unsafe {
            // On iOS, MKMapSnapshot.image returns a UIImage (not in objc2-map-kit typed API). Use msg_send.
            let ui_image: *mut objc2::runtime::NSObject = objc2::msg_send![snap, image];
            if ui_image.is_null() {
                return None;
            }
            // UIImagePNGRepresentation
            extern "C" {
                fn UIImagePNGRepresentation(image: *mut objc2::runtime::NSObject) -> *mut objc2_foundation::NSData;
            }
            let data = UIImagePNGRepresentation(ui_image);
            if data.is_null() {
                return None;
            }
            let bytes = (*data).to_vec();
            let img = image::load_from_memory(&bytes).ok()?;
            let rgba = img.to_rgba8();
            Some(egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            ))
        }
    }
}

// ─── Core Location ("here") ────────────────────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod cl {
    use crate::model::Place;
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, AllocAnyThread, msg_send};
    use objc2_core_location::*;
    use objc2_foundation::NSArray;

    static LOCATION: std::sync::Mutex<Option<(f64, f64)>> = std::sync::Mutex::new(None);
    static SETUP: std::sync::Once = std::sync::Once::new();

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "GTDCraftLocationDelegate"]
        struct LocDelegate;

        unsafe impl NSObjectProtocol for LocDelegate {}

        unsafe impl CLLocationManagerDelegate for LocDelegate {
            #[unsafe(method(locationManager:didUpdateLocations:))]
            fn did_update(&self, _mgr: &CLLocationManager, locs: &NSArray<CLLocation>) {
                if let Some(loc) = locs.lastObject() {
                    let coord = unsafe { loc.coordinate() };
                    if let Ok(mut g) = LOCATION.lock() {
                        *g = Some((coord.latitude, coord.longitude));
                    }
                }
            }

            #[unsafe(method(locationManager:didFailWithError:))]
            fn did_fail(&self, _mgr: &CLLocationManager, _err: &objc2_foundation::NSError) {
                // Location failed; leave LOCATION as-is (None or stale)
            }

            #[unsafe(method(locationManagerDidChangeAuthorization:))]
            fn did_change_auth(&self, mgr: &CLLocationManager) {
                let status = unsafe { mgr.authorizationStatus() };
                if status == CLAuthorizationStatus::AuthorizedWhenInUse
                    || status == CLAuthorizationStatus::AuthorizedAlways
                {
                    unsafe { mgr.requestLocation() };
                }
            }
        }
    );

    /// Start Core Location. Call once at launch. The delegate is retained globally.
    pub fn start() {
        SETUP.call_once(|| {
            // GTDCRAFT_NO_NOTIFY suppresses location too (screenshot mode)
            if std::env::var_os("GTDCRAFT_NO_NOTIFY").is_some() {
                return;
            }
            let mgr = unsafe { CLLocationManager::new() };
            let del: Retained<LocDelegate> = unsafe { msg_send![LocDelegate::alloc(), init] };
            unsafe { mgr.setDelegate(Some(ProtocolObject::from_ref(&*del))) };
            unsafe { mgr.requestWhenInUseAuthorization() };
            unsafe { mgr.requestLocation() };
            // Keep the manager and delegate alive
            std::mem::forget(del);
            std::mem::forget(mgr);
        });
    }

    /// Returns the last known location, if any. Non-blocking.
    pub fn current() -> Option<Place> {
        LOCATION.lock().ok().and_then(|g| g.map(|(lat, lon)| Place { label: "My location".into(), lat, lon, ..Place::default() }))
    }
}

// ─── Apple place search (MapKit) and addresses (Core Location) ─────────────────

/// Both answer on the main queue: the app's main thread runs it (eframe's event loop); `--find-place` spins it.
/// `placemark` / `CLGeocoder` are deprecated in macOS/iOS 26, but their replacements (`MKMapItem.address`,
/// `MKReverseGeocodingRequest`) don't exist on iOS 17, GTDCraft's oldest.
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[allow(deprecated)]
mod mk {
    use super::Found;
    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_core_location::{CLGeocoder, CLLocation, CLLocationCoordinate2D, CLPlacemark};
    use objc2_foundation::{NSArray, NSError, NSString};
    use objc2_map_kit::*;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(15);

    fn s(x: Option<Retained<NSString>>) -> String {
        x.map(|x| x.to_string()).unwrap_or_default()
    }

    fn address(p: &CLPlacemark) -> String {
        unsafe { super::address_lines(&[&s(p.subThoroughfare()), &s(p.thoroughfare()), &s(p.locality()), &s(p.administrativeArea()), &s(p.postalCode()), &s(p.country())]) }
    }

    /// `MKLocalSearch` for a natural-language query, within ~50 km of `near` when given.
    pub fn search(query: &str, near: Option<(f64, f64)>, address_only: bool) -> Result<Vec<Found>, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let search = unsafe {
            let req = MKLocalSearchRequest::new();
            req.setNaturalLanguageQuery(Some(&NSString::from_str(query)));
            if let Some((lat, lon)) = near {
                let center = CLLocationCoordinate2D { latitude: lat, longitude: lon };
                req.setRegion(MKCoordinateRegion { center, span: MKCoordinateSpan { latitudeDelta: 0.5, longitudeDelta: 0.5 } });
            }
            if address_only {
                req.setResultTypes(MKLocalSearchResultType::Address);
            }
            MKLocalSearch::initWithRequest(MKLocalSearch::alloc(), &req)
        };
        let done = block2::RcBlock::new(move |resp: *mut MKLocalSearchResponse, err: *mut NSError| {
            let out = match unsafe { resp.as_ref() } {
                Some(r) => Ok(unsafe { r.mapItems() }
                    .iter()
                    .filter_map(|item| unsafe {
                        let pm = item.placemark();
                        let c = pm.location()?.coordinate();
                        Some(Found { name: s(item.name()), address: address(&pm), lat: c.latitude, lon: c.longitude })
                    })
                    .collect()),
                None => Err(unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default()),
            };
            let _ = tx.send(out);
        });
        unsafe { search.startWithCompletionHandler(&*done as *const _ as *mut _) };
        rx.recv_timeout(WAIT).map_err(|_| "Apple Maps didn't answer".to_string())?
    }

    /// The postal address nearest a spot (`CLGeocoder reverseGeocodeLocation`).
    pub fn reverse(lat: f64, lon: f64) -> Option<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let geo = unsafe { CLGeocoder::new() };
        let loc = unsafe { CLLocation::initWithLatitude_longitude(CLLocation::alloc(), lat, lon) };
        let done = block2::RcBlock::new(move |marks: *mut NSArray<CLPlacemark>, _err: *mut NSError| {
            let _ = tx.send(unsafe { marks.as_ref() }.and_then(|m| m.firstObject()).map(|p| address(&p)));
        });
        unsafe { geo.reverseGeocodeLocation_completionHandler(&loc, &*done as *const _ as *mut _) };
        rx.recv_timeout(WAIT).ok().flatten().filter(|a| !a.is_empty())
    }
}

// ─── UserNotifications ─────────────────────────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod un {
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread};
    use objc2_foundation::{NSArray, NSBundle, NSError, NSSet, NSString};
    use objc2_user_notifications::*;

    /// iOS's notification permission: 0 not asked yet, 1 denied, 2 allowed (provisional and ephemeral count as allowed).
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    pub static PERMISSION: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    pub fn check_permission() {
        let block = block2::RcBlock::new(|s: std::ptr::NonNull<UNNotificationSettings>| {
            let st = unsafe { s.as_ref() }.authorizationStatus();
            let v = if st == UNAuthorizationStatus::Denied { 1 } else if st == UNAuthorizationStatus::NotDetermined { 0 } else { 2 };
            PERMISSION.store(v, std::sync::atomic::Ordering::Relaxed);
        });
        center().getNotificationSettingsWithCompletionHandler(&block);
    }

    /// Reminder buttons tapped, waiting for the app (it completes or selects the action on its next frame).
    pub static ACTIONS: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());
    /// The category of a reminder for an action, and its buttons' identifiers "complete" and "snooze" with the titles
    /// "Complete" and "Snooze".
    const TASK_CATEGORY: &str = "task-alarm";

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop.
        #[unsafe(super(NSObject))]
        #[name = "GTDCraftNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            /// Show banners (and list them, with sound) while GTDCraft is the app in front, too.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(&self, _c: &UNUserNotificationCenter, _n: &UNNotification, done: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
                done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List | UNNotificationPresentationOptions::Sound,));
            }

            /// Complete / Snooze / a tap on the reminder itself.
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(&self, _c: &UNUserNotificationCenter, r: &UNNotificationResponse, done: &block2::DynBlock<dyn Fn()>) {
                let action = r.actionIdentifier().to_string();
                let req = r.notification().request();
                let task = task_of(&req.identifier().to_string());
                if let Some(task) = task {
                    if action == "snooze" {
                        let n = req.content();
                        let secs = chrono::Local::now().format("%S%.f").to_string().parse::<f64>().unwrap_or(0.0);
                        send(&n.title().to_string(), &n.body().to_string(), Some(super::snooze_delay(secs)), Some(task));
                    } else if let Ok(mut a) = ACTIONS.lock() {
                        a.push((match action.as_str() { "complete" => "complete", "clockin" => "clockin", _ => "open" }.to_string(), task));
                    }
                }
                done.call(());
            }
        }
    );

    /// Only an app bundle has a notification identity.
    pub fn available() -> bool {
        // GTDCRAFT_NO_NOTIFY: screenshot and simulator runs (the permission prompt would cover the screen)
        if std::env::var_os("GTDCRAFT_NO_NOTIFY").is_some() {
            return false;
        }
        let b = NSBundle::mainBundle();
        b.bundleIdentifier().is_some() && b.bundlePath().to_string().ends_with(".app")
    }

    /// The center, with GTDCraft's delegate set and permission asked for (once per launch; the system asks once ever).
    fn center() -> Retained<UNUserNotificationCenter> {
        static SETUP: std::sync::Once = std::sync::Once::new();
        let c = UNUserNotificationCenter::currentNotificationCenter();
        SETUP.call_once(|| {
            let d: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
            c.setDelegate(Some(ProtocolObject::from_ref(&*d)));
            let complete = UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str("complete"), &NSString::from_str("Complete"), UNNotificationActionOptions::AuthenticationRequired);
            let snooze = UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str("snooze"), &NSString::from_str("Snooze"), UNNotificationActionOptions::empty());
            // GTDCraft's own: start the clock on it (opens the app, which keeps the clock and the Live Activity)
            let clock = UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str("clockin"), &NSString::from_str("Clock In"), UNNotificationActionOptions::Foreground);
            let cat = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(&NSString::from_str(TASK_CATEGORY), &NSArray::from_retained_slice(&[clock, complete, snooze]), &NSArray::new(), UNNotificationCategoryOptions::empty());
            c.setNotificationCategories(&NSSet::from_retained_slice(&[cat]));
            std::mem::forget(d); // the center keeps its delegate weakly
            let answer = block2::RcBlock::new(|_ok: Bool, _e: *mut NSError| {});
            c.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &answer);
        });
        c
    }

    fn content(title: &str, body: &str) -> Retained<UNMutableNotificationContent> {
        let n = UNMutableNotificationContent::new();
        n.setTitle(&NSString::from_str(title));
        n.setBody(&NSString::from_str(body));
        n.setSound(Some(&UNNotificationSound::defaultSound()));
        n
    }

    /// The task a reminder is for, from its request identifier ("gtdcraft-task-<id>-<n>").
    fn task_of(id: &str) -> Option<u64> {
        id.strip_prefix("gtdcraft-task-")?.split('-').next()?.parse().ok()
    }

    pub fn send(title: &str, body: &str, after_secs: Option<f64>, task: Option<u64>) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let c = center();
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let content = content(title, body);
        let id = match task {
            Some(t) => {
                content.setCategoryIdentifier(&NSString::from_str(TASK_CATEGORY));
                format!("gtdcraft-task-{t}-{}{n}", std::process::id())
            }
            None => format!("gtdcraft-{}-{n}", std::process::id()),
        };
        let trigger = after_secs.map(|s| UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(s.max(1.0), false));
        let req = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&id), &content, trigger.as_deref().map(|t| &**t));
        c.addNotificationRequest_withCompletionHandler(&req, None);
    }

    #[cfg(target_os = "ios")]
    pub fn schedule(alarms: &[(f64, u64, String, String)]) {
        center().removeAllPendingNotificationRequests();
        for (secs, task, title, body) in alarms.iter().take(64) {
            send(title, body, Some(*secs), Some(*task));
        }
    }

    /// Schedule location-triggered notifications for the nearest ~20 places from `here`.
    /// iOS monitors at most ~20 regions concurrently. Each region fires on entry or exit, as its place asks.
    #[cfg(target_os = "ios")]
    pub fn schedule_location(here: &crate::model::Place, places: &[(crate::model::Place, u64, String, String)]) {
        use objc2_core_location::*;
        let c = center();

        let nearest = super::nearest_n(here, &places.iter().map(|(p, id, _, _)| (p.clone(), *id)).collect::<Vec<_>>(), 20);
        let nearest_ids: std::collections::HashSet<u64> = nearest.iter().map(|(_, id)| *id).collect();

        // `schedule` has just removed every pending request, the old geofences with them
        for (place, task_id, title, body) in places.iter() {
            if !nearest_ids.contains(task_id) {
                continue;
            }
            let loc_id = format!("gtdcraft-loc-{task_id}");

            // Create a CLCircularRegion
            #[allow(deprecated)] // CLCircularRegion is deprecated but UNLocationNotificationTrigger needs it
            let region = unsafe {
                let coord = CLLocationCoordinate2D::new(place.lat, place.lon);
                CLCircularRegion::initWithCenter_radius_identifier(
                    CLCircularRegion::alloc(),
                    coord,
                    place.radius(),
                    &NSString::from_str(&loc_id),
                )
            };
            // the tag's Notification: When Arriving / When Leaving (None is left out by the caller, `reminders` in main.rs)
            unsafe {
                region.setNotifyOnEntry(place.notify == crate::model::Notify::Arriving);
                region.setNotifyOnExit(place.notify == crate::model::Notify::Leaving);
            };

            // Create the trigger
            let trigger = UNLocationNotificationTrigger::triggerWithRegion_repeats(
                &region,
                true, // repeats: fire every time user enters
            );

            let content = content(title, body);
            content.setCategoryIdentifier(&NSString::from_str(TASK_CATEGORY));

            let req = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&loc_id),
                &content,
                Some(&trigger),
            );
            c.addNotificationRequest_withCompletionHandler(&req, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snooze_is_the_next_whole_minute_plus_15() {
        assert_eq!(snooze_delay(0.0), 60.0 + 900.0); // at :00 the next whole minute is a minute away
        assert_eq!(snooze_delay(45.0), 15.0 + 900.0);
    }
    #[test]
    fn projection_roundtrip() {
        let (x, y) = project(45.52, -122.67, 12);
        let (lat, lon) = unproject(x, y, 12);
        assert!((lat - 45.52).abs() < 1e-9 && (lon + 122.67).abs() < 1e-9);
        let a = Place { label: "a".into(), lat: 45.5231, lon: -122.6765, ..Place::default() };
        let b = Place { label: "b".into(), lat: 45.5152, lon: -122.6784, radius_m: Some(0.0), ..Place::default() };
        assert_eq!(b.radius(), 1000.0);
        let km = a.km_to(&b);
        assert!(km > 0.8 && km < 1.0, "{km}");
    }
    #[test]
    fn visible_region_sane() {
        let (lat, lon, lat_span, lon_span) = visible_region(45.52, -122.67, 14, 600.0, 400.0);
        assert_eq!(lat, 45.52);
        assert_eq!(lon, -122.67);
        assert!(lat_span > 0.0 && lat_span < 1.0, "lat_span={lat_span}");
        assert!(lon_span > 0.0 && lon_span < 1.0, "lon_span={lon_span}");
    }
    #[test]
    fn nearest_n_picks_closest() {
        let here = Place { label: "here".into(), lat: 0.0, lon: 0.0, ..Place::default() };
        let places = vec![
            (Place { label: "far".into(), lat: 10.0, lon: 10.0, ..Place::default() }, 1),
            (Place { label: "near".into(), lat: 0.01, lon: 0.01, ..Place::default() }, 2),
            (Place { label: "mid".into(), lat: 1.0, lon: 1.0, ..Place::default() }, 3),
        ];
        let result = nearest_n(&here, &places, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].1, 2); // nearest
        assert_eq!(result[1].1, 3); // next nearest
    }
    #[test]
    fn snap_cache_key_stable() {
        let k1 = snap_cache_key(45.5201, -122.6742, 14, 600, 400, true);
        let k2 = snap_cache_key(45.5205, -122.6748, 14, 600, 400, true);
        assert_eq!(k1, k2, "small movements should share cache key");
        let k3 = snap_cache_key(45.5301, -122.6742, 14, 600, 400, true);
        assert_ne!(k1, k3, "large movements should differ");
    }
    #[test]
    fn address_lines_like_the_iphone_card() {
        let a = address_lines(&["7535", "Torrey Santa Fe Rd", "San Diego", "CA", "92129", "United States"]);
        assert_eq!(a, "7535 Torrey Santa Fe Rd\nSan Diego CA 92129\nUnited States");
        // missing parts leave no gaps or blank lines
        assert_eq!(address_lines(&["", "", "Portland", "OR", "", "United States"]), "Portland OR\nUnited States");
        assert_eq!(address_lines(&["", "", "", "", "", ""]), "");
    }
    #[test]
    fn found_becomes_a_place_named_for_the_business_or_its_street() {
        let f = Found { name: "Coava Coffee".into(), address: "1300 SE Grand Ave\nPortland OR 97214\nUnited States".into(), lat: 45.51, lon: -122.66 };
        let p = f.place(None);
        assert_eq!((p.label.as_str(), p.lat, p.lon), ("Coava Coffee", 45.51, -122.66));
        assert_eq!(p.address, f.address);
        // an address result's name is its street line, so the card shows the address only
        let a = Found { name: "1300 SE Grand Ave".into(), ..f.clone() };
        assert!(!a.distinct_name());
        assert!(f.distinct_name());
        let nameless = Found { name: String::new(), ..f };
        assert_eq!(nameless.place(None).label, "1300 SE Grand Ave");
    }
    #[test]
    fn search_is_biased_to_where_you_are() {
        let gps = Place { label: "My location".into(), lat: 45.52, lon: -122.68, ..Place::default() };
        let saved = Place { label: "Home".into(), lat: 32.7, lon: -117.1, ..Place::default() };
        assert_eq!(search_bias(Some(&gps), Some(&saved)), Some((45.52, -122.68)));
        assert_eq!(search_bias(None, Some(&saved)), Some((32.7, -117.1)));
        assert_eq!(search_bias(None, None), None);
    }
    /// MapKit search, for real: `cargo test --release mapkit_search -- --ignored --nocapture`.
    /// MapKit answers on the main queue, which no test thread runs, so this goes through `--find-place`
    /// (the binary spins the main run loop while it waits).
    #[test]
    #[ignore]
    fn mapkit_search_finds_coffee_in_portland() {
        // the app binary next to this test's (target/release/deps/..): `cargo build --release` first
        let exe = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("gtdcraft");
        let out = std::process::Command::new(exe).args(["--find-place", "coffee", "--near", "45.52,-122.68"]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        println!("{text}");
        assert!(text.contains("apple:"), "{text}");
        assert!(text.lines().filter(|l| l.contains("Portland")).count() >= 2, "{text}");
    }
    #[test]
    fn nearest_n_empty() {
        let here = Place { label: "here".into(), lat: 0.0, lon: 0.0, ..Place::default() };
        assert!(nearest_n(&here, &[], 5).is_empty());
    }
    #[test]
    fn nearest_n_more_than_available() {
        let here = Place { label: "here".into(), lat: 0.0, lon: 0.0, ..Place::default() };
        let places = vec![
            (Place { label: "a".into(), lat: 1.0, lon: 1.0, ..Place::default() }, 1),
        ];
        assert_eq!(nearest_n(&here, &places, 20).len(), 1);
    }
}
