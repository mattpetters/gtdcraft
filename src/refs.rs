//! References on tasks and projects: links to where the work lives (a GitHub repo, issue or pull request) or where it
//! came from (a Gmail or Mail message, a Messages conversation, any web page). One list per task or project; each kind
//! is recognised from its URL, so another service is one more arm in `kind` / `label` (and `fetch` when it has an API).
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ref {
    pub url: String,
    /// What the link is about (a GitHub issue's title, …), filled in by `fetch`; empty until then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// GitHub: open | closed | merged | draft. Empty for links without a state.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub state: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Repo,
    Issue,
    PullRequest,
    Email,
    Message,
    Video,
    Link,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Repo => "Repo",
            Kind::Issue => "Issue",
            Kind::PullRequest => "PR",
            Kind::Email => "Email",
            Kind::Message => "Messages",
            Kind::Video => "Video",
            Kind::Link => "Link",
        }
    }
}

/// A pasted link, cleaned up: `github.com/…` gets https://, `owner/repo#12` becomes the issue's URL. None if it isn't
/// a link GTDCraft can open.
pub fn normalize(input: &str) -> Option<String> {
    let s = input.trim().trim_matches(['<', '>']);
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let lower = s.to_lowercase();
    if ["https://", "http://", "message:", "sms:", "imessage:", "mailto:"].iter().any(|p| lower.starts_with(p)) {
        return Some(s.to_string());
    }
    if lower.starts_with("github.com/") || lower.starts_with("www.github.com/") {
        return Some(format!("https://{s}"));
    }
    // owner/repo#12 shorthand
    if let Some((repo, n)) = s.split_once('#') {
        if repo.matches('/').count() == 1 && n.parse::<u64>().is_ok() {
            return Some(format!("https://github.com/{repo}/issues/{n}"));
        }
    }
    None
}

/// GitHub URL parts: owner, repo, and the issue or pull request number.
type GitHubParts = (String, String, Option<(Kind, u64)>);

pub fn github(url: &str) -> Option<GitHubParts> {
    let rest = url.split_once("://").map_or(url, |x| x.1);
    let rest = rest.strip_prefix("www.").unwrap_or(rest).strip_prefix("github.com/")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let (owner, repo) = (parts.first()?.to_string(), parts.get(1)?.trim_end_matches(".git").to_string());
    let num = match (parts.get(2).copied(), parts.get(3).and_then(|n| n.parse().ok())) {
        (Some("issues"), Some(n)) => Some((Kind::Issue, n)),
        (Some("pull"), Some(n)) => Some((Kind::PullRequest, n)),
        _ => None,
    };
    Some((owner, repo, num))
}

pub fn kind(url: &str) -> Kind {
    let lower = url.to_lowercase();
    if let Some((_, _, n)) = github(url) {
        return n.map_or(Kind::Repo, |n| n.0);
    }
    if lower.contains("mail.google.com/") || lower.starts_with("message:") || lower.starts_with("mailto:") {
        return Kind::Email;
    }
    if lower.starts_with("sms:") || lower.starts_with("imessage:") {
        return Kind::Message;
    }
    if crate::webact::youtube_id(url).is_some() {
        return Kind::Video;
    }
    Kind::Link
}

/// A short name for a link when it has no fetched title: "owner/repo #12", "Gmail message", "Messages · Ana".
pub fn label(url: &str) -> String {
    if let Some((o, r, n)) = github(url) {
        return match n {
            Some((_, n)) => format!("{o}/{r} #{n}"),
            None => format!("{o}/{r}"),
        };
    }
    let lower = url.to_lowercase();
    if lower.contains("mail.google.com/") {
        return "Gmail message".into();
    }
    if lower.starts_with("message:") {
        return "Mail message".into();
    }
    if let Some(to) = lower.strip_prefix("mailto:") {
        return format!("Email · {}", to.split('?').next().unwrap_or(to));
    }
    for p in ["sms:", "imessage:"] {
        if let Some(who) = url.get(p.len()..).filter(|_| lower.starts_with(p)) {
            let who = who.trim_start_matches('/').split(['?', '&']).next().unwrap_or("");
            return if who.is_empty() { "Messages".into() } else { format!("Messages · {who}") };
        }
    }
    let host = url.split_once("://").map_or(url, |x| x.1);
    host.split('/').next().unwrap_or(host).trim_start_matches("www.").to_string()
}

/// Links whose details come from an API (GitHub, YouTube), and haven't been fetched yet.
pub fn needs_fetch(r: &Ref) -> bool {
    r.title.is_empty() && (github(&r.url).is_some() || crate::webact::youtube_id(&r.url).is_some())
}

/// The GitHub token: Settings › Agent › GitHub, else GITHUB_TOKEN (optional: public repos work without one, at
/// GitHub's lower unauthenticated rate).
pub fn token(setting: &str) -> Option<String> {
    Some(setting.trim().to_string()).filter(|t| !t.is_empty()).or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty()))
}

/// The video's title and channel from YouTube's oEmbed endpoint (no key needed). "Title · Channel".
fn fetch_youtube(url: &str) -> Result<Ref, String> {
    let api = format!("https://www.youtube.com/oembed?url={}&format=json", url_encode(url));
    let resp = ureq::get(&api).set("User-Agent", crate::notes::UA).timeout(std::time::Duration::from_secs(10)).call().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&resp.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let title = v["title"].as_str().unwrap_or("").trim().to_string();
    let author = v["author_name"].as_str().unwrap_or("").trim();
    let title = match (title.is_empty(), author.is_empty()) {
        (false, false) => format!("{title} · {author}"),
        (true, false) => author.to_string(),
        _ => title,
    };
    Ok(Ref { url: url.to_string(), title, state: String::new() })
}

/// The link's details from an API: a YouTube video's title and channel, or GitHub's repo description / issue title.
pub fn fetch(url: &str, token: Option<&str>) -> Result<Ref, String> {
    if crate::webact::youtube_id(url).is_some() {
        return fetch_youtube(url);
    }
    let (o, r, n) = github(url).ok_or("not a GitHub link")?;
    let api = match n {
        Some((Kind::PullRequest, n)) => format!("https://api.github.com/repos/{o}/{r}/pulls/{n}"),
        Some((_, n)) => format!("https://api.github.com/repos/{o}/{r}/issues/{n}"),
        None => format!("https://api.github.com/repos/{o}/{r}"),
    };
    let mut req = ureq::get(&api).set("User-Agent", "GTDCraft").set("Accept", "application/vnd.github+json").timeout(std::time::Duration::from_secs(15));
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let v: serde_json::Value = match req.call() {
        Ok(resp) => serde_json::from_str(&resp.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(404, _)) => return Err(format!("GitHub couldn't find {}{}", label(url), if token.is_none() { " (private? add a GitHub token in Settings)" } else { "" })),
        Err(ureq::Error::Status(c, _)) => return Err(format!("GitHub answered {c} for {}", label(url))),
        Err(e) => return Err(e.to_string()),
    };
    Ok(from_api(url, &v))
}

/// A repo's open issues and pull requests (newest first, up to 30), as References ready to add.
pub fn list_open(repo_url: &str, token: Option<&str>) -> Result<Vec<Ref>, String> {
    let (o, r, _) = github(repo_url).ok_or("not a GitHub repo link")?;
    let api = format!("https://api.github.com/repos/{o}/{r}/issues?state=open&per_page=30");
    let mut req = ureq::get(&api).set("User-Agent", "GTDCraft").set("Accept", "application/vnd.github+json").timeout(std::time::Duration::from_secs(15));
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let v: serde_json::Value = match req.call() {
        Ok(resp) => serde_json::from_str(&resp.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(c, _)) => return Err(format!("GitHub answered {c} for {o}/{r}")),
        Err(e) => return Err(e.to_string()),
    };
    Ok(from_list(&v))
}

/// References from GitHub's issue list JSON (pull requests come back as issues with a `pull_request` key and a /pull/
/// html_url; drafts are marked).
fn from_list(v: &serde_json::Value) -> Vec<Ref> {
    v.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|i| {
                    let url = i["html_url"].as_str()?.to_string();
                    let state = if i["draft"].as_bool() == Some(true) { "draft" } else { i["state"].as_str().unwrap_or("open") };
                    Some(Ref { url, title: i["title"].as_str().unwrap_or("").to_string(), state: state.to_string() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A Ref from GitHub's JSON for a repo, issue or pull request.
fn from_api(url: &str, v: &serde_json::Value) -> Ref {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let (title, state) = match kind(url) {
        Kind::Repo => (if s("description").is_empty() { s("full_name") } else { s("description") }, if v["archived"].as_bool() == Some(true) { "archived".into() } else { String::new() }),
        Kind::PullRequest => {
            let state = if v["merged"].as_bool() == Some(true) || !v["merged_at"].is_null() {
                "merged".to_string()
            } else if v["draft"].as_bool() == Some(true) && s("state") == "open" {
                "draft".to_string()
            } else {
                s("state")
            };
            (s("title"), state)
        }
        _ => (s("title"), s("state")),
    };
    Ref { url: url.to_string(), title, state }
}

/// The "Report a Bug" link: GitHub's new-issue page with the bug_report issue form (.github/ISSUE_TEMPLATE) and its
/// Environment field filled in — GitHub fills issue-form fields from same-named URL parameters, so no token or library
/// is needed; the browser's (or GitHub app's) sign-in is used.
pub fn bug_report_url(repo: &str, environment: &str) -> String {
    // `body` too: GitHub uses it when the form isn't on the default branch yet (forms ignore it)
    let body = format!("**What happened**\n\n\n**Steps to reproduce**\n1. \n\n**Environment**\n```\n{environment}\n```\n");
    format!("{repo}/issues/new?template=bug_report.yml&labels=bug&title={}&environment={}&body={}", url_encode("[Bug] "), url_encode(environment), url_encode(&body))
}

/// What a bug report should know about this copy of GTDCraft.
pub fn environment(screen: &str) -> String {
    let os = os_version();
    format!(
        "GTDCraft {} (build {}, {})\n{} {} · {}\nScreen: {screen}",
        env!("CARGO_PKG_VERSION"),
        env!("GTDCRAFT_BUILD"),
        env!("GTDCRAFT_SHA"),
        std::env::consts::OS,
        os,
        std::env::consts::ARCH
    )
}

fn os_version() -> String {
    #[cfg(target_os = "macos")]
    return std::process::Command::new("sw_vers").arg("-productVersion").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    #[cfg(target_os = "ios")]
    {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let (v, m): (Option<Retained<objc2_foundation::NSString>>, Option<Retained<objc2_foundation::NSString>>) = unsafe {
            let d: *mut AnyObject = msg_send![class!(UIDevice), currentDevice];
            (msg_send![d, systemVersion], msg_send![d, model])
        };
        return format!("{} ({})", v.map(|s| s.to_string()).unwrap_or_default(), m.map(|s| s.to_string()).unwrap_or_default());
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    String::new()
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Opens a link in its app: the browser, Mail, Messages.
pub fn open(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "ios")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        if objc2::MainThreadMarker::new().is_none() {
            return;
        }
        unsafe {
            let u: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*objc2_foundation::NSString::from_str(url)];
            if u.is_null() {
                return;
            }
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let opts: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![app, openURL: u, options: opts, completionHandler: none];
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = url;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_and_labels_links() {
        let cases = [
            ("https://github.com/mattpetters/gtdcraft", Kind::Repo, "mattpetters/gtdcraft"),
            ("https://github.com/o/r/issues/12", Kind::Issue, "o/r #12"),
            ("https://github.com/o/r/pull/34/files", Kind::PullRequest, "o/r #34"),
            ("https://mail.google.com/mail/u/0/#inbox/FMfcgz", Kind::Email, "Gmail message"),
            ("message://%3cabc@example.com%3e", Kind::Email, "Mail message"),
            ("sms:+15551234567", Kind::Message, "Messages · +15551234567"),
            ("https://youtu.be/dQw4w9WgXcQ", Kind::Video, "youtu.be"),
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", Kind::Video, "youtube.com"),
            ("https://www.example.com/a/b", Kind::Link, "example.com"),
        ];
        for (u, k, l) in cases {
            assert_eq!((kind(u), label(u).as_str()), (k, l), "{u}");
        }
    }

    #[test]
    fn cleans_up_pasted_links() {
        assert_eq!(normalize(" github.com/o/r "), Some("https://github.com/o/r".into()));
        assert_eq!(normalize("o/r#7"), Some("https://github.com/o/r/issues/7".into()));
        assert_eq!(normalize("<https://x.dev/y>"), Some("https://x.dev/y".into()));
        assert_eq!(normalize("not a link"), None);
        assert_eq!(normalize("hello"), None);
    }

    #[test]
    fn youtube_links_want_a_title() {
        assert!(needs_fetch(&Ref { url: "https://youtu.be/dQw4w9WgXcQ".into(), ..Default::default() }));
        assert!(!needs_fetch(&Ref { url: "https://youtu.be/dQw4w9WgXcQ".into(), title: "Known".into(), ..Default::default() }));
        assert!(!needs_fetch(&Ref { url: "https://www.example.com/a".into(), ..Default::default() }));
    }

    /// Hits YouTube: `cargo test -- --ignored fetches_a_real_video`.
    #[test]
    #[ignore]
    fn fetches_a_real_video() {
        let r = fetch_youtube("https://www.youtube.com/watch?v=dQw4w9WgXcQ").expect("YouTube oEmbed");
        assert!(r.title.contains("Never Gonna Give You Up") && r.title.contains("Rick Astley"));
    }

    /// Hits GitHub: `cargo test -- --ignored lists_a_real_repo`.
    #[test]
    #[ignore]
    fn lists_a_real_repo() {
        let items = list_open("https://github.com/emilk/egui", token("").as_deref()).expect("GitHub list");
        assert!(!items.is_empty() && items.iter().all(|r| matches!(kind(&r.url), Kind::Issue | Kind::PullRequest) && !r.title.is_empty()));
    }

    #[test]
    fn bug_reports_open_the_issue_form_filled_in() {
        let u = bug_report_url("https://github.com/o/r", "GTDCraft 1.0 (build 3)\nScreen: Inbox");
        assert!(u.starts_with("https://github.com/o/r/issues/new?template=bug_report.yml&labels=bug&title=%5BBug%5D%20&environment="));
        assert!(u.contains("&environment=GTDCraft%201.0%20%28build%203%29%0AScreen%3A%20Inbox&body="));
        assert!(environment("Inbox").contains(env!("CARGO_PKG_VERSION")) && environment("Inbox").ends_with("Screen: Inbox"));
    }

    #[test]
    fn reads_a_repos_open_list() {
        let v = serde_json::json!([
            {"html_url": "https://github.com/o/r/issues/5", "title": "Crash", "state": "open"},
            {"html_url": "https://github.com/o/r/pull/6", "title": "Fix", "state": "open", "pull_request": {}, "draft": true},
            {"title": "no url"}
        ]);
        let refs = from_list(&v);
        assert_eq!(refs.len(), 2);
        assert_eq!((kind(&refs[0].url), refs[1].state.as_str()), (Kind::Issue, "draft"));
        assert_eq!(kind(&refs[1].url), Kind::PullRequest);
    }

    #[test]
    fn reads_github_answers() {
        let pr = serde_json::json!({"title": "Fix sync", "state": "closed", "merged": true});
        assert_eq!(from_api("https://github.com/o/r/pull/3", &pr).state, "merged");
        let draft = serde_json::json!({"title": "WIP", "state": "open", "draft": true, "merged_at": null});
        assert_eq!(from_api("https://github.com/o/r/pull/4", &draft).state, "draft");
        let issue = serde_json::json!({"title": "Crash", "state": "open"});
        assert_eq!(from_api("https://github.com/o/r/issues/5", &issue), Ref { url: "https://github.com/o/r/issues/5".into(), title: "Crash".into(), state: "open".into() });
        let repo = serde_json::json!({"full_name": "o/r", "description": "", "archived": false});
        assert_eq!(from_api("https://github.com/o/r", &repo).title, "o/r");
    }
}
