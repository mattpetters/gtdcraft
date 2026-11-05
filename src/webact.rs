//! Web activity from the GTDCraft Chrome extension (extension/chrome): sites with time spent (and whether they're on
//! task), searches and YouTube videos. The extension posts new records to http://127.0.0.1:8766/v1/activity while
//! GTDCraft runs; they're appended to web-activity.jsonl in Application Support. Nothing leaves this computer.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

pub const PORT: u16 = 8766;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Rec {
    /// A stretch on one page (ms since 1970).
    Visit { start: i64, end: i64, url: String, domain: String, #[serde(default)] title: String, #[serde(default)] category: String },
    Search { at: i64, engine: String, query: String },
}

pub fn file() -> PathBuf {
    crate::dbfile::app_dir().join("web-activity.jsonl")
}

/// Receives the extension's records on 127.0.0.1 only (another GTDCraft already listening is fine).
#[cfg(not(target_os = "ios"))]
pub fn listen() {
    std::thread::spawn(|| {
        let Ok(server) = tiny_http::Server::http(("127.0.0.1", PORT)) else { return };
        for mut req in server.incoming_requests() {
            let ok = req.method() == &tiny_http::Method::Post && req.url() == "/v1/activity" && {
                let mut body = String::new();
                std::io::Read::read_to_string(req.as_reader(), &mut body).is_ok() && append(&body).is_ok()
            };
            let _ = req.respond(tiny_http::Response::from_string(if ok { "{\"ok\":true}" } else { "{\"ok\":false}" }).with_status_code(if ok { 200 } else { 400 }));
        }
    });
}

#[cfg(target_os = "ios")]
pub fn listen() {}

/// Appends a JSON array of records (unknown or malformed entries are skipped).
pub fn append(body: &str) -> std::io::Result<()> {
    let recs: Vec<serde_json::Value> = serde_json::from_str(body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let _ = std::fs::create_dir_all(crate::dbfile::app_dir());
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file())?;
    for r in recs {
        if serde_json::from_value::<Rec>(r.clone()).is_ok() {
            writeln!(f, "{r}")?;
        }
    }
    Ok(())
}

pub fn load() -> Vec<Rec> {
    std::fs::read_to_string(file()).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Video {
    pub id: String,
    pub title: String,
    /// Where clicking it goes: the video itself, watched from the start.
    pub url: String,
    pub seconds: f64,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub seconds: f64,
    pub focus: f64,
    pub distraction: f64,
    pub neutral: f64,
    pub sites: Vec<(String, f64)>,
    pub searches: Vec<(String, String)>,
    /// Watch time per video, most time first.
    pub videos: Vec<Video>,
}

pub fn watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// Where the app keeps a video's thumbnail: one small JPEG per id, fetched once and read from disk after.
pub fn thumbs_dir() -> PathBuf {
    crate::dbfile::app_dir().join("thumbs")
}

/// A video's thumbnail from YouTube (320×180), fetched in the background the first time it's needed.
/// None until it's on disk; the caller shows a placeholder and repaints (the app repaints every second).
pub fn thumb(id: &str) -> Option<PathBuf> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    let path = thumbs_dir().join(format!("{id}.jpg"));
    match std::fs::metadata(&path) {
        // an empty file means "asked, YouTube had no picture": don't ask again this run or any other
        Ok(m) if m.len() > 0 => return Some(path),
        Ok(_) => return None,
        Err(_) => {}
    }
    let pending = PENDING.get_or_init(Default::default);
    if pending.lock().map(|mut p| p.insert(id.to_string())).unwrap_or(false) {
        let (id, path) = (id.to_string(), path.clone());
        std::thread::spawn(move || {
            let got = ureq::get(&format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"))
                .set("User-Agent", crate::notes::UA)
                .timeout(std::time::Duration::from_secs(8))
                .call()
                .ok()
                .and_then(|r| {
                    let mut body = Vec::new();
                    std::io::Read::read_to_end(&mut r.into_reader(), &mut body).ok().map(|_| body)
                })
                .filter(|b| b.len() > 512);
            if let Some(body) = got {
                let _ = std::fs::create_dir_all(thumbs_dir());
                let part = path.with_extension("jpg.part");
                if std::fs::write(&part, &body).is_ok() {
                    let _ = std::fs::rename(&part, &path);
                }
            } else {
                // remember the miss so a deleted or private video isn't asked for on every repaint
                let _ = std::fs::create_dir_all(thumbs_dir());
                let _ = std::fs::write(&path, b"");
            }
            if let Ok(mut p) = pending.lock() {
                p.remove(&id);
            }
        });
    }
    None
}

/// The tab title without Chrome's unread count and YouTube's site suffix: "(3) Talk - YouTube" → "Talk".
fn clean_title(t: &str) -> String {
    let t = t.trim_end_matches(" - YouTube").trim();
    let t = match t.strip_prefix('(').and_then(|r| r.split_once(") ")) {
        Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest.trim(),
        _ => t,
    };
    t.to_string()
}

pub fn youtube_id(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.trim_start_matches("www.").trim_start_matches("m.");
    if host == "youtube.com" && path.starts_with("watch?") {
        return path.split(['?', '&']).find_map(|p| p.strip_prefix("v=")).map(String::from);
    }
    if host == "youtube.com" && path.starts_with("shorts/") {
        return path.split('/').nth(1).map(String::from);
    }
    (host == "youtu.be").then(|| path.split('?').next().unwrap_or("").to_string()).filter(|s| !s.is_empty())
}

/// Totals for records overlapping [from, to) (ms), like the extension's own summary.
pub fn summarize(recs: &[Rec], from: i64, to: i64) -> Summary {
    let mut s = Summary::default();
    let mut sites: BTreeMap<String, f64> = BTreeMap::new();
    let mut videos: Vec<Video> = vec![];
    for r in recs {
        match r {
            Rec::Visit { start, end, url, domain, title, category } => {
                let secs = ((*end).min(to) - (*start).max(from)) as f64 / 1000.0;
                if secs <= 0.0 {
                    continue;
                }
                s.seconds += secs;
                match category.as_str() {
                    "focus" => s.focus += secs,
                    "distraction" => s.distraction += secs,
                    _ => s.neutral += secs,
                }
                *sites.entry(domain.clone()).or_default() += secs;
                if let Some(id) = youtube_id(url) {
                    match videos.iter_mut().find(|v| v.id == id) {
                        Some(v) => {
                            v.seconds += secs;
                            if v.title.is_empty() {
                                v.title = clean_title(title);
                            }
                        }
                        None => videos.push(Video { url: watch_url(&id), title: clean_title(title), id, seconds: secs }),
                    }
                }
            }
            Rec::Search { at, engine, query } if *at >= from && *at < to => s.searches.push((query.clone(), engine.clone())),
            Rec::Search { .. } => {}
        }
    }
    let mut v: Vec<(String, f64)> = sites.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    s.sites = v;
    videos.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    s.videos = videos;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_parse_and_summarize() {
        let body = r#"[
          {"kind":"visit","start":0,"end":1800000,"url":"https://github.com/x","domain":"github.com","title":"x","category":"focus"},
          {"kind":"visit","start":1800000,"end":2400000,"url":"https://www.youtube.com/watch?v=v1&t=3","domain":"youtube.com","title":"Talk - YouTube","category":"distraction"},
          {"kind":"search","at":100,"engine":"Google","query":"egui ios"},
          {"kind":"bogus"}
        ]"#;
        let recs: Vec<Rec> = serde_json::from_str::<Vec<serde_json::Value>>(body).unwrap().into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
        assert_eq!(recs.len(), 3); // the unknown one is skipped
        let s = summarize(&recs, 0, 3_600_000);
        assert_eq!((s.focus, s.distraction), (1800.0, 600.0));
        assert_eq!(s.sites[0].0, "github.com");
        assert_eq!(s.videos.len(), 1);
        assert_eq!(s.videos[0].title, "Talk");
        assert_eq!(s.videos[0].url, "https://www.youtube.com/watch?v=v1");
        assert_eq!(s.videos[0].seconds, 600.0);
        assert_eq!(s.searches, vec![("egui ios".to_string(), "Google".to_string())]);
        assert_eq!(youtube_id("https://youtu.be/abc?t=1").as_deref(), Some("abc"));
        assert_eq!(youtube_id("https://www.youtube.com/watch?v=dQw4&t=3").as_deref(), Some("dQw4"));
        assert_eq!(youtube_id("https://github.com/o/r"), None);
        assert_eq!(clean_title("(12) My favorite method #Mix - YouTube"), "My favorite method #Mix");
        assert_eq!(clean_title("Plain title"), "Plain title");
        assert_eq!(thumb(""), None);
        assert_eq!(thumb("../etc/passwd"), None); // only real ids touch the disk
        // an empty file means YouTube had no picture: None, and no new request
        let path = thumbs_dir().join("gtdcraft_test_no_picture.jpg");
        let _ = std::fs::create_dir_all(thumbs_dir());
        std::fs::write(&path, b"").unwrap();
        assert_eq!(thumb("gtdcraft_test_no_picture"), None);
        let _ = std::fs::remove_file(&path);
    }
}
