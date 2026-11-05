//! Calendar events from macOS (EventKit), shown in Forecast, the Focus window's countdown, and to agents.
//!
//! macOS asks the *responsible* app for Calendar permission. When GTDCraft is started by another app (a launcher,
//! a terminal, an agent running `--mcp`) that app is responsible, and if it doesn't declare Calendar use macOS refuses
//! silently. So all EventKit work happens in a tiny helper app, "GTDCraft Calendar" (src/bin/gtdcraft-calendar.rs),
//! started through LaunchServices (`open`), which makes it its own responsible app: it shows the prompt and keeps the
//! permission however GTDCraft is launched. It's installed once in Application Support and only replaced when
//! `HELPER_VERSION` changes, so the permission isn't lost on every rebuild. Answers come back as a JSON file.
//!
//! Rules for Forecast:
//! - cancelled events (status 3) are dropped;
//! - an event belongs to a day if it starts, ends or spans inside [start of day, next day − 60 s];
//! - all-day events only when "include all-day events" is on (`isAllDay == FALSE` otherwise);
//! - hidden calendars are skipped;
//! - events are read 35 days ahead;
//! - Siri-suggested, natural-language-suggested and Facebook birthday calendars are left out
//!   (asking EventKit's own isNaturalLanguageSuggestedEventCalendar / isSuggestedEventCalendar /
//!   isFacebookBirthdayCalendar when present);
//! - an event fills every hour it touches, used for the timeline.
use block2::RcBlock;
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use objc2::runtime::Bool;
use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStatus, EKEventStore};
use objc2_foundation::{NSDate, NSError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub calendar: String,
    #[serde(default)]
    pub calendar_id: String,
    /// The calendar's colour (sRGB).
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    #[serde(default)]
    pub location: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarInfo {
    pub id: String,
    pub title: String,
    pub color: Option<[u8; 3]>,
}

/// How many days ahead events are read.
pub const DAYS_AHEAD: i64 = 35;
/// How often events are re-read.
pub const REFRESH: Duration = Duration::minutes(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Access {
    NotAsked,
    Denied,
    Granted,
}

// ---------- EventKit (runs inside the helper) ----------

pub fn ek_access() -> Access {
    match unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) } {
        EKAuthorizationStatus::FullAccess => Access::Granted,
        EKAuthorizationStatus::NotDetermined => Access::NotAsked,
        _ => Access::Denied,
    }
}

/// Shows the macOS permission prompt and waits for the answer: Ok(granted) or the error EventKit gave.
pub fn ek_request() -> Result<bool, String> {
    let store = unsafe { EKEventStore::new() };
    let (tx, rx) = std::sync::mpsc::channel();
    let block = RcBlock::new(move |granted: Bool, err: *mut NSError| {
        let e = unsafe { err.as_ref() }.map(|e| format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code()));
        let _ = tx.send((granted.as_bool(), e));
    });
    // macOS stages the permission dialog ("staged prompting request" in tccd's log) until the asking app has finished
    // launching and is in front. Spinning a bare NSRunLoop never got there: `lsappinfo` showed the helper as
    // "!cgsConnection !signalled" (no window-server connection, launch never checked in), so the dialog never showed.
    // Become a regular app for the moment (a Dock icon while asking), finish launching, come forward, and run AppKit's
    // own event loop (nextEvent / sendEvent) while the dialog is up.
    #[cfg(target_os = "macos")]
    let app = objc2::MainThreadMarker::new().map(objc2_app_kit::NSApplication::sharedApplication);
    #[cfg(target_os = "macos")]
    if let Some(app) = &app {
        app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
        app.finishLaunching();
        // macOS 14+ activation is cooperative: GTDCraft yields to the helper first (`yield_to_helper`), then this takes it
        app.activate();
    }
    #[cfg(not(target_os = "macos"))]
    let app = objc2::MainThreadMarker::new();
    unsafe { store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&block)) };
    let t0 = std::time::Instant::now();
    loop {
        match rx.try_recv() {
            Ok((g, None)) => return Ok(g),
            Ok((_, Some(e))) => return Err(e),
            Err(_) if t0.elapsed() > std::time::Duration::from_secs(300) => return Err("no answer to the Calendar prompt within 5 minutes".into()),
            Err(_) => {}
        }
        #[cfg(target_os = "macos")]
        if let Some(app) = &app {
            let until = NSDate::dateWithTimeIntervalSinceNow(0.1);
            if let Some(ev) = unsafe { app.nextEventMatchingMask_untilDate_inMode_dequeue(objc2_app_kit::NSEventMask::Any, Some(&until), objc2_foundation::NSDefaultRunLoopMode, true) } {
                app.sendEvent(&ev);
            }
            continue;
        }
        #[cfg(not(target_os = "macos"))]
        if app.is_some() {
            objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn ns_date(t: NaiveDateTime) -> objc2::rc::Retained<NSDate> {
    let secs = Local.from_local_datetime(&t).earliest().map(|x| x.timestamp() as f64).unwrap_or(0.0);
    NSDate::dateWithTimeIntervalSince1970(secs)
}

fn local(d: &NSDate) -> NaiveDateTime {
    let secs = d.timeIntervalSince1970();
    Local.timestamp_opt(secs.floor() as i64, 0).single().map(|x| x.naive_local()).unwrap_or_default()
}

fn rgb(c: Option<objc2::rc::Retained<objc2_core_graphics::CGColor>>) -> Option<[u8; 3]> {
    let c = c?;
    let n = objc2_core_graphics::CGColor::number_of_components(Some(&c));
    let p = objc2_core_graphics::CGColor::components(Some(&c));
    if p.is_null() || n < 3 {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(p, n) };
    Some([0, 1, 2].map(|i| (v[i].clamp(0.0, 1.0) * 255.0).round() as u8))
}

/// Calendars left out of Forecast automatically: suggestions and Facebook birthdays. It asks EventKit's own (private)
/// checks only when the calendar answers to them.
fn auto_excluded(c: &objc2_event_kit::EKCalendar) -> bool {
    use objc2::runtime::MessageReceiver;
    use objc2::{msg_send, sel};
    [sel!(isNaturalLanguageSuggestedEventCalendar), sel!(isSuggestedEventCalendar), sel!(isFacebookBirthdayCalendar)].into_iter().any(|s| unsafe {
        let responds: bool = msg_send![c, respondsToSelector: s];
        responds && c.send_message::<(), Bool>(s, ()).as_bool()
    })
}

/// The event calendars on this Mac.
pub fn ek_calendars() -> Vec<CalendarInfo> {
    if ek_access() != Access::Granted {
        return vec![];
    }
    let store = unsafe { EKEventStore::new() };
    let list = unsafe { store.calendarsForEntityType(EKEntityType::Event) };
    list.iter().filter(|c| !auto_excluded(c)).map(|c| unsafe { CalendarInfo { id: c.calendarIdentifier().to_string(), title: c.title().to_string(), color: rgb(c.CGColor()) } }).collect()
}

/// Events from the start of `from` for `days` days, skipping hidden calendars, cancelled and (unless asked) all-day ones.
pub fn ek_events(from: NaiveDate, days: i64, hidden: &[String], all_day: bool) -> Result<Vec<Event>, String> {
    match ek_access() {
        Access::Granted => {}
        Access::NotAsked => return Err("Calendar access not set up yet".into()),
        Access::Denied => return Err("GTDCraft isn't allowed to read Calendar. Turn it on in System Settings › Privacy & Security › Calendars.".into()),
    }
    let store = unsafe { EKEventStore::new() };
    let start = from.and_hms_opt(0, 0, 0).unwrap();
    let end = start + Duration::days(days) - Duration::seconds(60);
    let pred = unsafe { store.predicateForEventsWithStartDate_endDate_calendars(&ns_date(start), &ns_date(end), None) };
    let list = unsafe { store.eventsMatchingPredicate(&pred) };
    let mut out: Vec<Event> = list
        .iter()
        .filter_map(|e| unsafe {
            if e.status() == EKEventStatus::Canceled || (e.isAllDay() && !all_day) {
                return None;
            }
            let cal = e.calendar();
            let calendar_id = cal.as_ref().map(|c| c.calendarIdentifier().to_string()).unwrap_or_default();
            if hidden.contains(&calendar_id) || cal.as_ref().is_some_and(|c| auto_excluded(c)) {
                return None;
            }
            Some(Event {
                title: e.title().to_string(),
                start: local(&e.startDate()),
                end: local(&e.endDate()),
                all_day: e.isAllDay(),
                calendar: cal.as_ref().map(|c| c.title().to_string()).unwrap_or_default(),
                calendar_id,
                color: cal.and_then(|c| rgb(c.CGColor())),
                location: e.location().map(|l| l.to_string()).unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by_key(|e| (e.start, e.end));
    Ok(out)
}

// ---------- the helper app, as GTDCraft uses it ----------

/// Bump when the helper's commands or output change (reinstalls it, which asks for permission again).
pub const HELPER_VERSION: u32 = 6;
const HELPER_ID: &str = "local.gtdcraft.calendar";

fn helper_app() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Application Support/GTDCraft/GTDCraft Calendar.app")
}

/// The first code-signing identity on this Mac ("Apple Development: …"), if the person has one.
fn signing_identity() -> Option<String> {
    let out = std::process::Command::new("security").args(["find-identity", "-v", "-p", "codesigning"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let (a, b) = (l.find('"')?, l.rfind('"')?);
        (b > a).then(|| l[a + 1..b].to_string())
    })
}

/// Signs `app` with the developer identity when there is one, ad-hoc otherwise.
fn code_sign(app: &std::path::Path, ident: &str) -> bool {
    let mut cmd = std::process::Command::new("codesign");
    cmd.args(["--force", "--identifier", ident]);
    match signing_identity() {
        Some(name) => {
            cmd.args(["--sign", &name]);
        }
        None => {
            cmd.args(["--sign", "-"]);
        }
    }
    cmd.arg(app).output().is_ok_and(|o| o.status.success())
}

/// Puts the helper app in Application Support (from the `gtdcraft-calendar` built next to this program) unless the
/// same version is already there.
fn install_helper() -> Result<PathBuf, String> {
    let app = helper_app();
    let exe = app.join("Contents/MacOS/gtdcraft-calendar");
    let ver = app.join("Contents/Resources/version");
    if exe.exists() && std::fs::read_to_string(&ver).is_ok_and(|v| v.trim() == HELPER_VERSION.to_string()) {
        return Ok(app);
    }
    let here = std::env::current_exe().map_err(|e| e.to_string())?;
    let src = here.parent().map(|d| d.join("gtdcraft-calendar")).filter(|p| p.exists()).ok_or("The Calendar helper (gtdcraft-calendar) isn't next to GTDCraft; build it with cargo build.")?;
    let _ = std::fs::remove_dir_all(&app);
    std::fs::create_dir_all(app.join("Contents/MacOS")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app.join("Contents/Resources")).map_err(|e| e.to_string())?;
    std::fs::copy(&src, &exe).map_err(|e| e.to_string())?;
    let usage = "GTDCraft shows your Calendar events in Forecast and counts down to the next one in the Focus window.";
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>GTDCraft Calendar</string>
  <key>CFBundleDisplayName</key><string>GTDCraft</string>
  <key>CFBundleIdentifier</key><string>{HELPER_ID}</string>
  <key>CFBundleExecutable</key><string>gtdcraft-calendar</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{HELPER_VERSION}</string>
  <key>LSUIElement</key><true/>
  <key>NSCalendarsFullAccessUsageDescription</key><string>{usage}</string>
  <key>NSCalendarsUsageDescription</key><string>{usage}</string>
</dict></plist>
"#
    );
    std::fs::write(app.join("Contents/Info.plist"), plist).map_err(|e| e.to_string())?;
    std::fs::write(&ver, HELPER_VERSION.to_string()).map_err(|e| e.to_string())?;
    // macOS ties the Calendar permission to the helper's signature: sign it with this Mac's Apple Development
    // identity when there is one, so the grant survives a rebuild (an ad-hoc signature changes with every build,
    // which makes macOS forget the permission and ask again)
    if !code_sign(&app, HELPER_ID) {
        return Err("Couldn't sign the Calendar helper (codesign).".into());
    }
    Ok(app)
}

/// Runs one helper command through LaunchServices and returns its JSON answer.
#[cfg(not(target_os = "ios"))]
fn helper(args: &[String]) -> Result<serde_json::Value, String> {
    let app = install_helper()?;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let out = std::env::temp_dir().join(format!("gtdcraft-calendar-{}-{}.json", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_file(&out);
    // -n a fresh instance, -g stay in the background. Not -W: the helper often exits before `open` can watch it
    // ("Unable to block on application"), so wait for its answer file instead.
    let asking = args.first().is_some_and(|a| a == "request");
    let st = std::process::Command::new("/usr/bin/open")
        .args(if asking { &["-n", "-a"][..] } else { &["-n", "-g", "-a"][..] })
        .arg(&app)
        .arg("--args")
        .arg(&out)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Couldn't start the Calendar helper: {e}"))?;
    if !st.success() {
        return Err(format!("Couldn't start the Calendar helper ({st})."));
    }
    // the permission prompt waits for you; everything else answers in well under a second
    let limit = std::time::Duration::from_secs(if asking { 310 } else { 20 });
    let t0 = std::time::Instant::now();
    while !out.exists() {
        if t0.elapsed() > limit {
            return Err("The Calendar helper didn't answer.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let text = std::fs::read_to_string(&out).map_err(|e| format!("The Calendar helper's answer: {e}"))?;
    let _ = std::fs::remove_file(&out);
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    match v.get("error").and_then(|e| e.as_str()) {
        Some(e) => Err(e.to_string()),
        None => Ok(v),
    }
}

/// Lets the helper come to the front when it asks (macOS 14+ only lets an app activate if the active one yields to
/// it, and the permission dialog waits until the asking app is in front). Call on the main thread before asking.
#[cfg(target_os = "macos")]
pub fn yield_to_helper() {
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).yieldActivationToApplicationWithBundleIdentifier(&objc2_foundation::NSString::from_str(HELPER_ID));
    }
}

/// Starts Calendar permission over: stops helpers still waiting on a dialog, clears macOS's answer for the helper
/// (`tccutil reset Calendar local.gtdcraft.calendar`, no admin needed) and removes the helper so the next request
/// installs and signs it fresh.
#[cfg(target_os = "macos")]
pub fn reset() -> Result<(), String> {
    let _ = std::process::Command::new("/usr/bin/pkill").args(["-f", "GTDCraft Calendar.app/Contents/MacOS/gtdcraft-calendar"]).status();
    let out = std::process::Command::new("/usr/bin/tccutil").args(["reset", "Calendar", HELPER_ID]).output().map_err(|e| format!("tccutil: {e}"))?;
    if !out.status.success() {
        return Err(format!("tccutil: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    match std::fs::remove_dir_all(helper_app()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Calendar permission, as the helper app has it.
#[cfg(not(target_os = "ios"))]
pub fn access() -> Access {
    helper(&["status".into()]).ok().and_then(|v| serde_json::from_value(v["access"].clone()).ok()).unwrap_or(Access::NotAsked)
}

/// Shows the macOS permission prompt (from the helper) and waits for the answer.
#[cfg(not(target_os = "ios"))]
pub fn request_access() -> bool {
    helper(&["request".into()]).ok().and_then(|v| serde_json::from_value::<Access>(v["access"].clone()).ok()) == Some(Access::Granted)
}

/// Events from the start of `from` for `days` days (see `ek_events`).
#[cfg(not(target_os = "ios"))]
pub fn events(from: NaiveDate, days: i64, hidden: &[String], all_day: bool) -> Result<Vec<Event>, String> {
    let v = helper(&["events".into(), from.to_string(), days.to_string(), (all_day as u8).to_string(), hidden.join("\n")])?;
    serde_json::from_value(v["events"].clone()).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "ios"))]
pub fn calendars() -> Vec<CalendarInfo> {
    helper(&["calendars".into()]).ok().and_then(|v| serde_json::from_value(v["calendars"].clone()).ok()).unwrap_or_default()
}

// iPhone/iPad: no helper apps there; GTDCraft asks EventKit itself (Info.plist NSCalendarsFullAccessUsageDescription).
#[cfg(target_os = "ios")]
pub fn access() -> Access {
    ek_access()
}

#[cfg(target_os = "ios")]
pub fn request_access() -> bool {
    ek_request() == Ok(true)
}

#[cfg(target_os = "ios")]
pub fn events(from: NaiveDate, days: i64, hidden: &[String], all_day: bool) -> Result<Vec<Event>, String> {
    ek_events(from, days, hidden, all_day)
}

#[cfg(target_os = "ios")]
pub fn calendars() -> Vec<CalendarInfo> {
    ek_calendars()
}

/// The helper's side: one command, answer written to `out` as JSON.
pub fn helper_main(args: &[String]) -> i32 {
    let (Some(out), Some(cmd)) = (args.first(), args.get(1)) else { return 2 };
    let status = |a: Access| serde_json::json!({ "access": a });
    let answer = match cmd.as_str() {
        "status" => status(ek_access()),
        "request" => {
            let t0 = std::time::Instant::now();
            let r = if ek_access() == Access::NotAsked { ek_request() } else { Ok(true) };
            let mut v = status(ek_access());
            v["seconds"] = serde_json::json!(t0.elapsed().as_secs_f64());
            if let Err(e) = r {
                v["request_error"] = serde_json::json!(e);
            }
            v
        }
        "calendars" => serde_json::json!({ "calendars": ek_calendars() }),
        "events" => {
            let from = args.get(2).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).unwrap_or_else(|| Local::now().date_naive());
            let days = args.get(3).and_then(|d| d.parse().ok()).unwrap_or(1).clamp(1, DAYS_AHEAD);
            let all_day = args.get(4).is_some_and(|x| x == "1");
            let hidden: Vec<String> = args.get(5).map(|h| h.split('\n').filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default();
            match ek_events(from, days, &hidden, all_day) {
                Ok(evs) => serde_json::json!({ "events": evs }),
                Err(e) => serde_json::json!({ "error": e }),
            }
        }
        _ => serde_json::json!({ "error": format!("unknown command {cmd}") }),
    };
    let tmp = format!("{out}.tmp");
    match std::fs::write(&tmp, answer.to_string()).and_then(|_| std::fs::rename(&tmp, out)) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// Does an event show on this day? It starts, ends or spans inside [start of day, next day − 60 s].
pub fn on_day(e: &Event, day: NaiveDate) -> bool {
    let s = day.and_hms_opt(0, 0, 0).unwrap();
    let end = s + Duration::days(1) - Duration::seconds(60);
    (e.start >= s && e.start <= end) || (e.end >= s && e.end <= end) || (e.start < s && e.end > end)
}

/// The hours of `day` (0–23) an event fills:
/// start = hour + minute/60 if it starts that day, else 0; end = hour + minute/60 if it ends before the next day, else 24;
/// it fills hours floor(start) .. floor(start) + max(ceil(end) − floor(start), 1); all-day events fill all 24.
pub fn hours(e: &Event, day: NaiveDate) -> std::ops::Range<u32> {
    use chrono::Timelike;
    if e.all_day {
        return 0..24;
    }
    let s = day.and_hms_opt(0, 0, 0).unwrap();
    let next = s + Duration::days(1);
    let frac = |t: NaiveDateTime| t.hour() as f64 + t.minute() as f64 / 60.0;
    let start = if e.start >= s { frac(e.start) } else { 0.0 };
    let end = if e.end < next { frac(e.end) } else { 24.0 };
    let first = start.floor();
    let count = (end.ceil() - first).max(1.0);
    first as u32..((first + count) as u32).min(24)
}

/// Busy hours of a day from all its events.
pub fn busy(events: &[Event], day: NaiveDate, all_day: bool) -> [bool; 24] {
    let mut out = [false; 24];
    // all-day events are listed on the day but only fill the timeline when asked (the person: "show all-day events
    // but not as time-blocked")
    for e in events.iter().filter(|e| on_day(e, day) && (all_day || !e.all_day)) {
        for h in hours(e, day) {
            out[h as usize] = true;
        }
    }
    out
}

/// What's happening now (a timed event in progress) and what's next, for the Focus countdown.
pub fn now_and_next(events: &[Event], now: NaiveDateTime) -> (Option<&Event>, Option<&Event>) {
    let timed = || events.iter().filter(|e| !e.all_day);
    let current = timed().filter(|e| e.start <= now && e.end > now).min_by_key(|e| e.end);
    // only later today: the countdown is about the rest of this day
    let next = timed().filter(|e| e.start > now && e.start.date() == now.date()).min_by_key(|e| e.start);
    (current, next)
}

/// "9:30 – 10:00 AM", "all-day", or "until 11:00 AM" for an event that began on an earlier day.
pub fn time_label(e: &Event, day: NaiveDate) -> String {
    if e.all_day {
        return "all-day".into();
    }
    let f = |t: NaiveDateTime| t.format("%-I:%M %p").to_string();
    match (e.start.date() == day, e.end.date() == day) {
        (true, true) => format!("{} – {}", f(e.start), f(e.end)),
        (true, false) => format!("from {}", f(e.start)),
        (false, true) => format!("until {}", f(e.end)),
        (false, false) => "all day".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }
    fn ev(title: &str, start: NaiveDateTime, end: NaiveDateTime) -> Event {
        Event { title: title.into(), start, end, all_day: false, calendar: "Work".into(), calendar_id: "w".into(), color: None, location: String::new() }
    }

    #[test]
    fn days_countdown_and_labels() {
        let standup = ev("Standup", at(2, 9, 30), at(2, 9, 45));
        let review = ev("Design review", at(2, 14, 0), at(2, 15, 0));
        let trip = ev("Trip", at(1, 18, 0), at(4, 9, 0)); // spans Nov 2 and 3 entirely
        let late = ev("Late call", at(2, 23, 30), at(3, 0, 30)); // ends next day
        let d2 = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let d3 = d2.succ_opt().unwrap();
        assert!(on_day(&standup, d2) && !on_day(&standup, d3));
        assert!(on_day(&trip, d2) && on_day(&trip, d3));
        assert!(on_day(&late, d2) && on_day(&late, d3));
        let all = vec![standup.clone(), review.clone(), trip.clone()];
        let (cur, next) = now_and_next(&all, at(2, 9, 40));
        assert_eq!(cur.map(|e| e.title.as_str()), Some("Standup")); // the one ending soonest
        assert_eq!(next.map(|e| e.title.as_str()), Some("Design review"));
        let tomorrow = ev("Dentist", at(3, 11, 0), at(3, 12, 0));
        assert!(now_and_next(&[tomorrow], at(2, 22, 0)).1.is_none()); // tomorrow isn't "next" today
        assert_eq!(time_label(&standup, d2), "9:30 AM – 9:45 AM");
        assert_eq!(time_label(&late, d3), "until 12:30 AM");
        assert_eq!(time_label(&trip, d2), "all day");
        // hour collation: 9:30–9:45 fills 9; 14:00–15:00 fills 14; the overnight call fills 23 then 0
        assert_eq!(hours(&standup, d2), 9..10);
        assert_eq!(hours(&review, d2), 14..15);
        assert_eq!(hours(&late, d2), 23..24);
        assert_eq!(hours(&late, d3), 0..1);
        assert_eq!(hours(&trip, d2), 0..24);
        let b = busy(&[standup.clone(), review.clone()], d2, true);
        assert_eq!((0..24).filter(|h| b[*h]).collect::<Vec<_>>(), vec![9, 14]);
    }

    /// Talks to the real EventKit in this process: never prompts.
    #[test]
    fn access_query_does_not_prompt() {
        if ek_access() != Access::Granted {
            assert!(ek_events(NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), 1, &[], false).is_err());
        }
    }

    #[test]
    fn events_round_trip_through_the_helper_format() {
        let e = Event { title: "Standup".into(), start: at(2, 9, 30), end: at(2, 9, 45), all_day: false, calendar: "Work".into(), calendar_id: "w".into(), color: Some([1, 2, 3]), location: "Room 4".into() };
        let v = serde_json::json!({ "events": [e.clone()] });
        let back: Vec<Event> = serde_json::from_value(v["events"].clone()).unwrap();
        assert_eq!(back, vec![e]);
        let a: Access = serde_json::from_value(serde_json::json!("Granted")).unwrap();
        assert_eq!(a, Access::Granted);
    }
}

/// iPhone/iPad: open GTDCraft's page in the Settings app (where Calendars access is turned back on).
#[cfg(target_os = "ios")]
pub fn open_app_settings() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    let Some(_mtm) = objc2::MainThreadMarker::new() else { return };
    unsafe {
        let url: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*objc2_foundation::NSString::from_str("app-settings:")];
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        let opts: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let none: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![app, openURL: url, options: opts, completionHandler: none];
    }
}
