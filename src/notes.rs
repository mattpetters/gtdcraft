//! Notes: Markdown rendering, image embeds, and link cards for URLs on their own line.
use crate::model::LinkCard;
use crate::theme::c;
use eframe::egui::{self, vec2, Sense};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::sync::mpsc::{channel, Receiver, Sender};

pub const UA: &str = concat!("GTDCraft/", env!("CARGO_PKG_VERSION"), " (desktop task manager)");

pub struct Notes {
    pub cache: CommonMarkCache,
    pending: HashSet<String>,
    tx: Sender<(String, LinkCard)>,
    rx: Receiver<(String, LinkCard)>,
    pub offline: bool,
}

impl Notes {
    pub fn new(offline: bool) -> Notes {
        let (tx, rx) = channel();
        Notes { cache: CommonMarkCache::default(), pending: HashSet::new(), tx, rx, offline }
    }

    /// Moves finished previews into the store; true when something arrived.
    pub fn poll(&mut self, cards: &mut BTreeMap<String, LinkCard>) -> bool {
        let mut got = false;
        while let Ok((url, card)) = self.rx.try_recv() {
            self.pending.remove(&url);
            cards.insert(url, card);
            got = true;
        }
        got
    }

    fn request(&mut self, url: &str) {
        if self.offline || !self.pending.insert(url.to_string()) {
            return;
        }
        let (tx, url) = (self.tx.clone(), url.to_string());
        std::thread::spawn(move || {
            let card = fetch_card(&url).unwrap_or_else(|_| LinkCard { title: host(&url), site: host(&url), ..Default::default() });
            let _ = tx.send((url, card));
        });
    }

    /// Renders a note: Markdown text, images for image links, cards for other bare links.
    pub fn show(&mut self, ui: &mut egui::Ui, id: u64, text: &str, cards: &BTreeMap<String, LinkCard>) {
        let w = ui.available_width();
        let mut md = String::new();
        let mut seg = 0;
        let flush = |ui: &mut egui::Ui, md: &mut String, cache: &mut CommonMarkCache, seg: &mut usize| {
            if !md.trim().is_empty() {
                ui.push_id((id, *seg), |ui| {
                    CommonMarkViewer::new().max_image_width(Some(w as usize)).show(ui, cache, md);
                });
                *seg += 1;
            }
            md.clear();
        };
        for line in text.lines() {
            let l = line.trim();
            if is_file_link(l) {
                flush(ui, &mut md, &mut self.cache, &mut seg);
                file_link_row(ui, l, w);
                ui.add_space(4.0);
            } else if is_url(l) {
                flush(ui, &mut md, &mut self.cache, &mut seg);
                if is_image_url(l) {
                    ui.add(egui::Image::from_uri(l.to_string()).max_width(w).corner_radius(6));
                } else {
                    if !cards.contains_key(l) {
                        self.request(l);
                    }
                    link_card(ui, l, cards.get(l), w);
                }
                ui.add_space(4.0);
            } else {
                md.push_str(line);
                md.push('\n');
            }
        }
        flush(ui, &mut md, &mut self.cache, &mut seg);
    }

    /// Cards for bare links only (shown under the editor so a pasted link previews right away).
    pub fn show_cards(&mut self, ui: &mut egui::Ui, text: &str, cards: &BTreeMap<String, LinkCard>) {
        let w = ui.available_width();
        for l in text.lines().map(str::trim).filter(|l| is_url(l) && !is_image_url(l)) {
            if !cards.contains_key(l) {
                self.request(l);
            }
            link_card(ui, l, cards.get(l), w);
            ui.add_space(4.0);
        }
    }
}

/// One note line as plain text for the outline preview (Markdown markers removed).
pub fn plain(line: &str) -> String {
    let t = line.trim_start();
    let t = t.trim_start_matches('#').trim_start();
    let t = if let Some(r) = t.strip_prefix("- [ ] ").or_else(|| t.strip_prefix("* [ ] ")) {
        format!("☐ {r}")
    } else if let Some(r) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
        format!("☑ {r}")
    } else if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        format!("• {r}")
    } else {
        t.to_string()
    };
    t.replace("**", "").replace("__", "").replace('`', "").replace('*', "")
}

pub fn is_url(s: &str) -> bool {
    (s.starts_with("https://") || s.starts_with("http://")) && !s.contains(char::is_whitespace) && s.len() > 10
}

/// Common macOS/Linux path prefixes that are unambiguously file system paths.
const PATH_PREFIXES: &[&str] = &[
    "/Users/", "/Volumes/", "/Applications/", "/Library/", "/System/",
    "/home/", "/tmp/", "/var/", "/etc/", "/opt/", "/usr/",
];

/// Recognises `file://…` URLs and plain local paths on their own line.
pub fn is_file_link(s: &str) -> bool {
    let s = strip_trailing_punct(s);
    if s.is_empty() || s.contains(char::is_whitespace) {
        return false;
    }
    if s.starts_with("file://") && s.len() > 7 {
        return true;
    }
    if s.starts_with("~/") && s.len() > 2 {
        return true;
    }
    PATH_PREFIXES.iter().any(|p| s.starts_with(p))
}

/// Strips a trailing `.`, `,`, or `;` that is almost certainly punctuation, not part of the path.
pub fn strip_trailing_punct(s: &str) -> &str {
    s.strip_suffix(['.', ',', ';']).unwrap_or(s)
}

/// Expands `~` to `$HOME` and converts `file://` URLs to paths.
pub fn expand_path(s: &str) -> String {
    let s = strip_trailing_punct(s);
    if let Some(rest) = s.strip_prefix("file://") {
        rest.replace("%20", " ")
    } else if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{home}/{rest}")
    } else {
        s.to_string()
    }
}

/// Display info for a file link: (file name, parent path).
pub fn file_display(s: &str) -> (String, String) {
    let clean = strip_trailing_punct(s);
    let expanded = expand_path(clean);
    let p = std::path::Path::new(&expanded);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| expanded.clone());
    // Show the unexpanded form for ~/… paths
    let parent = if clean.starts_with("~/") {
        std::path::Path::new(clean).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default()
    };
    (name, parent)
}

/// Opens a file path in Finder (macOS) or via URL (iOS).
pub fn open_file(s: &str) {
    let expanded = expand_path(s);
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&expanded).spawn();
    #[cfg(target_os = "ios")]
    {
        // a file in the app: Quick Look (another app can't open it by URL); anything else (a link): its app
        if expanded.starts_with('/') {
            crate::attachpick::preview(&expanded);
        } else {
            crate::refs::open(&expanded);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = expanded;
}

/// Normalises a pasted attachment path: strips `file://`, trims whitespace.
pub fn normalize_attachment(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("file://") {
        rest.replace("%20", " ")
    } else {
        s.to_string()
    }
}

pub fn is_image_url(s: &str) -> bool {
    let path = s.split(['?', '#']).next().unwrap_or(s).to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| path.ends_with(e))
}

pub fn host(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").trim_start_matches("www.").to_string()
}

/// A clickable file/folder link row in a note.
fn file_link_row(ui: &mut egui::Ui, raw: &str, w: f32) {
    let cs = c();
    let clean = strip_trailing_punct(raw);
    let (name, parent) = file_display(clean);
    let expanded = expand_path(clean);
    let is_dir = std::path::Path::new(&expanded).is_dir();
    let icon = if is_dir { "\u{1F4C1}" } else { "\u{1F4CE}" }; // 📁 / 📎
    let resp = egui::Frame::new()
        .fill(cs.field)
        .stroke(egui::Stroke::new(1.0, cs.line))
        .corner_radius(8)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(w - 18.0);
            ui.horizontal(|ui| {
                ui.label(icon);
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&name).strong().color(cs.text)).truncate());
                    if !parent.is_empty() {
                        ui.label(egui::RichText::new(&parent).color(cs.dim).size(11.5));
                    }
                });
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(clean);
    if resp.clicked() {
        open_file(clean);
    }
}

/// A clickable attachment row with a remove button. Returns true if the × was clicked.
pub fn attachment_row(ui: &mut egui::Ui, path: &str, w: f32) -> bool {
    let cs = c();
    let (name, parent) = file_display(path);
    let expanded = expand_path(path);
    let is_dir = std::path::Path::new(&expanded).is_dir();
    let icon = if is_dir { "\u{1F4C1}" } else { "\u{1F4CE}" };
    let mut removed = false;
    let resp = egui::Frame::new()
        .fill(cs.field)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(w - 20.0);
            ui.horizontal(|ui| {
                ui.label(icon);
                let click = ui.add(egui::Label::new(egui::RichText::new(&name).strong()).wrap().sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(path);
                if click.clicked() {
                    open_file(path);
                }
                if !parent.is_empty() {
                    ui.label(egui::RichText::new(&parent).color(cs.dim).size(11.5));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Remove this attachment").clicked() {
                        removed = true;
                    }
                });
            });
        })
        .response;
    let _ = resp;
    removed
}

/// A pasted YouTube link shows as a video card: the 16:9 thumbnail on top, then title and channel.
fn youtube_card(ui: &mut egui::Ui, url: &str, id: &str, card: Option<&LinkCard>, w: f32) {
    let cs = c();
    let resp = egui::Frame::new()
        .fill(cs.field)
        .stroke(egui::Stroke::new(1.0, cs.line))
        .corner_radius(8)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(w - 18.0);
            ui.vertical(|ui| {
                let tw = (w - 18.0).min(360.0);
                let (tr, _) = ui.allocate_exact_size(vec2(tw, tw * 9.0 / 16.0), Sense::hover());
                match crate::webact::thumb(id) {
                    Some(path) => {
                        egui::Image::from_uri(format!("file://{}", path.display())).corner_radius(6).paint_at(ui, tr);
                    }
                    None => {
                        ui.painter().rect_filled(tr, 6.0, cs.pill);
                        ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "▶", egui::FontId::proportional(20.0), cs.faint);
                    }
                }
                ui.add_space(6.0);
                let title = card.map(|c| c.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "YouTube video".to_string());
                ui.add(egui::Label::new(egui::RichText::new(title).strong().color(cs.text)).wrap());
                match card.map(|c| c.description.clone()).filter(|d| !d.is_empty()) {
                    Some(desc) => {
                        ui.add(egui::Label::new(egui::RichText::new(desc).color(cs.dim).size(12.0)).truncate());
                    }
                    None if card.is_none() => {
                        ui.label(egui::RichText::new("Loading preview…").color(cs.faint).size(12.0));
                    }
                    None => {}
                }
                ui.label(egui::RichText::new("YouTube").color(cs.link).size(11.5));
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(url);
    if resp.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

fn link_card(ui: &mut egui::Ui, url: &str, card: Option<&LinkCard>, w: f32) {
    if let Some(id) = crate::webact::youtube_id(url) {
        youtube_card(ui, url, &id, card, w);
        return;
    }
    let cs = c();
    let resp = egui::Frame::new()
        .fill(cs.field)
        .stroke(egui::Stroke::new(1.0, cs.line))
        .corner_radius(8)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(w - 18.0);
            ui.horizontal(|ui| {
                if let Some(img) = card.and_then(|c| c.image.clone()) {
                    ui.add(egui::Image::from_uri(img).fit_to_exact_size(vec2(52.0, 52.0)).corner_radius(6));
                }
                ui.vertical(|ui| {
                    let title = card.map(|c| c.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| url.to_string());
                    ui.add(egui::Label::new(egui::RichText::new(title).strong().color(cs.text)).truncate());
                    match card {
                        None => {
                            ui.label(egui::RichText::new("Loading preview…").color(cs.faint).size(12.0));
                        }
                        Some(c) if !c.description.is_empty() => {
                            ui.add(egui::Label::new(egui::RichText::new(&c.description).color(cs.dim).size(12.0)).truncate());
                        }
                        _ => {}
                    }
                    let site = card.map(|c| c.site.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| host(url));
                    ui.label(egui::RichText::new(site).color(cs.link).size(11.5));
                });
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(url);
    if resp.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

/// Fetches a page (first 512 KB) and reads its Open Graph / title tags.
pub fn fetch_card(url: &str) -> Result<LinkCard, String> {
    let resp = ureq::get(url).set("User-Agent", UA).timeout(std::time::Duration::from_secs(8)).call().map_err(|e| e.to_string())?;
    let ctype = resp.header("content-type").unwrap_or("").to_string();
    if ctype.starts_with("image/") {
        return Ok(LinkCard { title: host(url), site: host(url), image: Some(url.to_string()), ..Default::default() });
    }
    let mut body = String::new();
    resp.into_reader().take(512 * 1024).read_to_string(&mut body).ok();
    Ok(parse_card(url, &body))
}

pub fn parse_card(url: &str, html: &str) -> LinkCard {
    let mut metas: Vec<(String, String)> = vec![];
    let lower = html.to_lowercase();
    let mut i = 0;
    while let Some(p) = lower[i..].find("<meta") {
        let start = i + p;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        let key = attr(tag, "property").or_else(|| attr(tag, "name")).map(|k| k.to_lowercase());
        if let (Some(k), Some(v)) = (key, attr(tag, "content")) {
            metas.push((k, decode(&v)));
        }
        i = end;
    }
    let get = |k: &str| metas.iter().find(|(m, _)| m == k).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    let title_tag = lower.find("<title").and_then(|s| {
        let a = s + lower[s..].find('>')? + 1;
        let b = a + lower[a..].find("</title")?;
        Some(decode(html[a..b].trim()))
    });
    let image = get("og:image").or_else(|| get("twitter:image")).map(|img| absolute(url, &img));
    LinkCard {
        title: get("og:title").or_else(|| get("twitter:title")).or(title_tag).unwrap_or_else(|| host(url)),
        description: get("og:description").or_else(|| get("description")).or_else(|| get("twitter:description")).unwrap_or_default(),
        image,
        site: get("og:site_name").unwrap_or_else(|| host(url)),
    }
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find(name) {
        let at = from + p;
        let rest = lower[at + name.len()..].trim_start();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        if before_ok && rest.starts_with('=') {
            let off = tag.len() - rest.len() + 1;
            let v = tag[off..].trim_start();
            let q = v.chars().next()?;
            return if q == '"' || q == '\'' {
                v[1..].find(q).map(|e| v[1..1 + e].to_string())
            } else {
                Some(v.split(|ch: char| ch.is_whitespace() || ch == '/').next().unwrap_or("").to_string())
            };
        }
        from = at + name.len();
    }
    None
}

fn decode(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
}

fn absolute(base: &str, href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else if let Some(r) = href.strip_prefix("//") {
        format!("https://{r}")
    } else {
        let scheme = base.split("://").next().unwrap_or("https");
        format!("{scheme}://{}/{}", base.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or(""), href.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_link_detection() {
        // file:// URLs
        assert!(is_file_link("file:///Users/matt/doc.pdf"));
        assert!(is_file_link("file:///Volumes/USB/data"));
        assert!(!is_file_link("file://")); // too short
        // ~/… paths
        assert!(is_file_link("~/Documents/report.pdf"));
        assert!(is_file_link("~/code/project"));
        assert!(!is_file_link("~/")); // too short (nothing after ~/)
        // Absolute paths with known prefixes
        assert!(is_file_link("/Users/matt/Desktop/notes.txt"));
        assert!(is_file_link("/Volumes/External/backup"));
        assert!(is_file_link("/Applications/Safari.app"));
        assert!(is_file_link("/tmp/scratch.rs"));
        assert!(is_file_link("/home/user/file.txt"));
        // Not file links
        assert!(!is_file_link("https://example.com"));
        assert!(!is_file_link("just some text"));
        assert!(!is_file_link("/random/unknown/path")); // no known prefix
        assert!(!is_file_link("")); // empty
        assert!(!is_file_link("/Users/matt/my file.txt")); // has whitespace
        // Trailing punctuation stripped
        assert!(is_file_link("~/Documents/report.pdf."));
        assert!(is_file_link("/Users/matt/file.txt,"));
        assert!(is_file_link("file:///tmp/data;"));
    }

    #[test]
    fn file_link_display_info() {
        let (name, parent) = file_display("~/Documents/report.pdf");
        assert_eq!(name, "report.pdf");
        assert_eq!(parent, "~/Documents");

        let (name, parent) = file_display("/Users/matt/Desktop/notes.txt");
        assert_eq!(name, "notes.txt");
        assert_eq!(parent, "/Users/matt/Desktop");

        let (name, parent) = file_display("file:///Volumes/USB/data.zip");
        assert_eq!(name, "data.zip");
        assert_eq!(parent, "/Volumes/USB");

        // Trailing punctuation stripped for display
        let (name, _) = file_display("~/Documents/report.pdf.");
        assert_eq!(name, "report.pdf");
    }

    #[test]
    fn expand_path_works() {
        assert_eq!(expand_path("file:///Users/matt/f.txt"), "/Users/matt/f.txt");
        assert_eq!(expand_path("file:///path/with%20spaces/f.txt"), "/path/with spaces/f.txt");
        assert!(expand_path("~/code/x").ends_with("/code/x"));
        assert_eq!(expand_path("/tmp/file.rs"), "/tmp/file.rs");
        // Trailing punctuation
        assert_eq!(expand_path("/tmp/file.rs."), "/tmp/file.rs");
    }

    #[test]
    fn normalize_attachment_paths() {
        assert_eq!(normalize_attachment("file:///Users/matt/f.txt"), "/Users/matt/f.txt");
        assert_eq!(normalize_attachment("  file:///path/with%20spaces/f  "), "/path/with spaces/f");
        assert_eq!(normalize_attachment("/Users/matt/doc.pdf"), "/Users/matt/doc.pdf");
        assert_eq!(normalize_attachment("~/code"), "~/code");
    }

    #[test]
    fn attachment_add_remove() {
        let mut list: Vec<String> = vec!["/tmp/a.txt".into(), "~/b.rs".into()];
        // Add a new one (dedup)
        let new = "/tmp/c.txt".to_string();
        if !list.contains(&new) { list.push(new); }
        assert_eq!(list.len(), 3);
        // Adding a duplicate does nothing
        let dup = "/tmp/a.txt".to_string();
        if !list.contains(&dup) { list.push(dup); }
        assert_eq!(list.len(), 3);
        // Remove by index
        list.remove(1);
        assert_eq!(list, vec!["/tmp/a.txt", "/tmp/c.txt"]);
    }

    #[test]
    fn card_from_html() {
        let html = r#"<html><head><title>Plain &amp; Simple</title>
            <meta property="og:title" content="Kanagawa Paper">
            <meta name='description' content='Ink on paper'>
            <meta property="og:image" content="/img/cover.png"/></head></html>"#;
        let c = parse_card("https://example.com/post/1", html);
        assert_eq!(c.title, "Kanagawa Paper");
        assert_eq!(c.description, "Ink on paper");
        assert_eq!(c.image.as_deref(), Some("https://example.com/img/cover.png"));
        assert_eq!(c.site, "example.com");
        let t = parse_card("https://www.x.org", "<title>Only Title</title>");
        assert_eq!(t.title, "Only Title");
        assert_eq!(t.site, "x.org");
        assert!(is_url("https://example.com/a") && !is_url("see https://x.com"));
        assert!(is_image_url("https://a.com/p.JPG?x=1"));
        assert_eq!(plain("**Why:** go"), "Why: go");
        assert_eq!(plain("- [ ] skim"), "☐ skim");
        assert_eq!(plain("- [ ] write down *one* idea"), "☐ write down one idea");
    }
}
