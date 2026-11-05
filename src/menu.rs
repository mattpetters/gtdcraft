//! The macOS menu bar, laid out as File, Edit, Organize, View, Perspectives,
//! Window, Help, plus GTDCraft's database items in File. A menu item with a ⌘/⌃ shortcut replays that key into
//! the app (`inject`), so menus, keys and custom shortcuts all go through the same `keys()` dispatch.
use crate::keys::{Action, Binding};
use eframe::egui::{self, Key};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

/// What the menu shows; the menu is rebuilt when this changes.
#[derive(Clone, PartialEq, Default)]
pub struct Shape {
    pub keymap: Vec<(Action, Binding)>,
    pub perspectives: Vec<(String, String)>, // (key, name)
    pub recent: Vec<PathBuf>,
    pub git: bool,
    pub sidebar: bool,
    pub inspector: bool,
    pub board: bool,
}

pub struct Bar {
    /// Held so the menu stays alive while it's installed.
    _menu: Menu,
    pub shape: Shape,
}

/// Menu clicks arrive here; each one also asks egui for a frame.
pub fn listen(ctx: &egui::Context) -> Receiver<String> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = tx.send(e.id().0.clone());
        ctx.request_repaint();
    }));
    rx
}

fn code(k: Key) -> Option<Code> {
    use Code::*;
    Some(match k {
        Key::A => KeyA, Key::B => KeyB, Key::C => KeyC, Key::D => KeyD, Key::E => KeyE, Key::F => KeyF, Key::G => KeyG,
        Key::H => KeyH, Key::I => KeyI, Key::J => KeyJ, Key::K => KeyK, Key::L => KeyL, Key::M => KeyM, Key::N => KeyN,
        Key::O => KeyO, Key::P => KeyP, Key::Q => KeyQ, Key::R => KeyR, Key::S => KeyS, Key::T => KeyT, Key::U => KeyU,
        Key::V => KeyV, Key::W => KeyW, Key::X => KeyX, Key::Y => KeyY, Key::Z => KeyZ,
        Key::Num0 => Digit0, Key::Num1 => Digit1, Key::Num2 => Digit2, Key::Num3 => Digit3, Key::Num4 => Digit4,
        Key::Num5 => Digit5, Key::Num6 => Digit6, Key::Num7 => Digit7, Key::Num8 => Digit8, Key::Num9 => Digit9,
        Key::OpenBracket => BracketLeft, Key::CloseBracket => BracketRight, Key::Comma => Comma, Key::Period => Period,
        Key::Quote => Quote, Key::Semicolon => Semicolon, Key::Slash => Slash, Key::Minus => Minus, Key::Equals => Equal,
        Key::Backspace => Backspace, Key::Delete => Delete, Key::Enter => Enter, Key::Space => Space, Key::Tab => Tab,
        Key::ArrowUp => ArrowUp, Key::ArrowDown => ArrowDown, Key::ArrowLeft => ArrowLeft, Key::ArrowRight => ArrowRight,
        _ => return None,
    })
}

fn accel(b: &Binding) -> Option<Accelerator> {
    let mut m = Modifiers::empty();
    if b.cmd {
        m |= Modifiers::META;
    }
    if b.ctrl {
        m |= Modifiers::CONTROL;
    }
    if b.alt {
        m |= Modifiers::ALT;
    }
    if b.shift {
        m |= Modifiers::SHIFT;
    }
    Some(Accelerator::new(m, code(b.key)?))
}

/// The binding a menu shows for an action: the first one with ⌘ or ⌃ (bare keys would steal typing).
pub fn menu_binding(keymap: &[(Action, Binding)], a: Action) -> Option<Binding> {
    keymap.iter().find(|(x, b)| *x == a && b.works_while_typing()).map(|x| x.1)
}

fn fixed(s: &str) -> Option<Accelerator> {
    Binding::parse(s).as_ref().and_then(accel)
}

pub fn build(s: &Shape) -> Bar {
    let act = |a: Action, text: &str| MenuItem::with_id(format!("act:{}", a.id()), text, true, menu_binding(&s.keymap, a).as_ref().and_then(accel));
    let check = |a: Action, text: &str, on: bool| CheckMenuItem::with_id(format!("act:{}", a.id()), text, true, on, menu_binding(&s.keymap, a).as_ref().and_then(accel));
    let item = |id: &str, text: &str, key: Option<&str>| MenuItem::with_id(id, text, true, key.and_then(fixed));
    let sep = PredefinedMenuItem::separator;

    let app = Submenu::with_items(
        "GTDCraft",
        true,
        &[
            &PredefinedMenuItem::about(Some("About GTDCraft"), Some(AboutMetadata { name: Some("GTDCraft".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() })),
            &sep(),
            &act(Action::Settings, "Settings…"),
            &sep(),
            &PredefinedMenuItem::services(None),
            &sep(),
            &PredefinedMenuItem::hide(Some("Hide GTDCraft")),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &sep(),
            // our own Quit, so unsaved changes are written and committed first
            &item("quit", "Quit GTDCraft", Some("cmd+q")),
        ],
    )
    .expect("app menu");

    let recent = Submenu::with_id("recent", "Open Recent", !s.recent.is_empty());
    for (i, p) in s.recent.iter().enumerate() {
        let _ = recent.append(&MenuItem::with_id(format!("recent:{i}"), p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), true, None));
    }
    let file = Submenu::with_items(
        "File",
        true,
        &[
            &act(Action::NewAction, "New Action"),
            &act(Action::NewProject, "New Project"),
            &act(Action::NewFolder, "New Folder"),
            &act(Action::NewTag, "New Tag"),
            &sep(),
            &act(Action::QuickOpen, "Quick Open…"),
            &PredefinedMenuItem::close_window(Some("Close")),
            &sep(),
            &item("db:new", "New Database…", Some("alt+cmd+n")),
            &item("db:open", "Open Database…", Some("shift+cmd+o")),
            &recent,
            &item("db:save_as", "Save Database As…", Some("shift+cmd+s")),
            &item("db:export", "Export…", Some("shift+cmd+e")),
            &sep(),
            &item("db:sync", "Sync Now", Some("cmd+s")),
            &CheckMenuItem::with_id("db:git", "Keep Version History (git)", true, s.git, None),
            &item("db:history", "Version History…", None),
            &sep(),
            &item("db:reveal", "Show Database in Finder", None),
        ],
    )
    .expect("file menu");

    let status = Submenu::with_items("Status", true, &[&act(Action::ToggleComplete, "Completed"), &act(Action::ToggleDrop, "Dropped")]).expect("status");
    let find = Submenu::with_items("Find", true, &[&act(Action::Search, "Go to Search Field")]).expect("find");
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &act(Action::Undo, "Undo"),
            &act(Action::Redo, "Redo"),
            &sep(),
            &item("edit:cut", "Cut", Some("cmd+x")),
            &item("edit:copy", "Copy", Some("cmd+c")),
            &item("edit:paste", "Paste", Some("cmd+v")),
            &act(Action::Delete, "Delete"),
            &item("edit:select_all", "Select All", Some("cmd+a")),
            &act(Action::Duplicate, "Duplicate"),
            &sep(),
            &act(Action::EditNote, "Edit Note"),
            &sep(),
            &act(Action::ConvertToProject, "Convert to Project"),
            &sep(),
            &status,
            &act(Action::MarkReviewed, "Mark Reviewed"),
            &act(Action::QueueAdd, "Add to Up Next"),
            &act(Action::ToggleFlag, "Set Flag"),
            &sep(),
            &find,
        ],
    )
    .expect("edit menu");

    let mv = Submenu::with_items("Move", true, &[&act(Action::MoveDown, "Move Down"), &act(Action::MoveUp, "Move Up")]).expect("move");
    let organize = Submenu::with_items(
        "Organize",
        true,
        &[
            &act(Action::CleanUp, "Clean Up"),
            &sep(),
            &act(Action::AddInside, "Add Inside"),
            &act(Action::AddOutside, "Add Outside"),
            &sep(),
            &mv,
            &act(Action::Indent, "Indent"),
            &act(Action::Outdent, "Outdent"),
            &act(Action::Group, "Group"),
            &act(Action::Ungroup, "Ungroup"),
        ],
    )
    .expect("organize menu");

    let view = Submenu::with_items(
        "View",
        true,
        &[
            &act(Action::Back, "Go Back"),
            &act(Action::Forward, "Go Forward"),
            &sep(),
            &check(Action::ToggleSidebar, "Show Sidebar", s.sidebar),
            &check(Action::ToggleInspector, "Show Inspector", s.inspector),
            &sep(),
            &item("view:options", "Show View Options", Some("shift+cmd+v")),
            &check(Action::ToggleBoard, "Show as Board", s.board),
            &sep(),
            &PredefinedMenuItem::fullscreen(Some("Enter Full Screen")),
        ],
    )
    .expect("view menu");

    let persp = Submenu::with_items("Perspectives", true, &[&act(Action::PerspectivesList, "Show Perspectives List"), &item("persp:add", "Add Perspective…", None), &sep()]).expect("perspectives");
    for (key, name) in &s.perspectives {
        let a = Action::ALL.iter().copied().find(|a| a.id() == format!("open_{key}"));
        let acc = a.and_then(|a| menu_binding(&s.keymap, a)).as_ref().and_then(accel);
        let _ = persp.append(&MenuItem::with_id(format!("persp:{key}"), name, true, acc));
    }

    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(Some("Minimize Window")),
            &PredefinedMenuItem::maximize(Some("Zoom")),
            &sep(),
            &act(Action::QuickEntry, "Quick Entry"),
            &act(Action::FocusWindow, "Focus Window"),
            &sep(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ],
    )
    .expect("window menu");
    let help = Submenu::with_items(
        "Help",
        true,
        &[&item("help:keys", "Keyboard Shortcuts…", None), &item("help:agent", "Agent & MCP Setup…", None), &sep(), &item("help:mcp_copy", "Copy MCP Server Command", None), &sep(), &item("help:bug", "Report a Bug…", None)],
    )
    .expect("help menu");

    let menu = Menu::with_items(&[&app, &file, &edit, &organize, &view, &persp, &window, &help]).expect("menu bar");
    menu.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    help.set_as_help_menu_for_nsapp();
    Bar { _menu: menu, shape: s.clone() }
}

/// What a menu item does once it reaches the app.
#[derive(Debug, PartialEq)]
pub enum Cmd {
    /// Replay this key so `keys()` (and text fields) handle it exactly like a key press.
    Event(egui::Event),
    /// An action without a ⌘/⌃ shortcut runs directly.
    Run(Action),
    /// Database, view and help items, handled by the app.
    Other(String),
}

pub fn decode(id: &str, keymap: &[(Action, Binding)]) -> Cmd {
    let key_event = |b: Binding| {
        Cmd::Event(egui::Event::Key {
            key: b.key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { alt: b.alt, ctrl: b.ctrl, shift: b.shift, mac_cmd: b.cmd, command: b.cmd },
        })
    };
    if let Some(aid) = id.strip_prefix("act:") {
        if let Some(a) = Action::ALL.iter().copied().find(|a| a.id() == aid) {
            return match menu_binding(keymap, a) {
                Some(b) => key_event(b),
                None => Cmd::Run(a),
            };
        }
    }
    match id {
        "edit:cut" => Cmd::Event(egui::Event::Cut),
        "edit:copy" => Cmd::Event(egui::Event::Copy),
        "edit:paste" => match std::process::Command::new("pbpaste").output() {
            Ok(o) if !o.stdout.is_empty() => Cmd::Event(egui::Event::Paste(String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"))),
            _ => Cmd::Other(String::new()),
        },
        "edit:select_all" => key_event(Binding::parse("cmd+a").expect("cmd+a")),
        _ => Cmd::Other(id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;
    use std::collections::BTreeMap;

    #[test]
    fn menu_items_replay_their_shortcut() {
        let km = keys::bindings(&BTreeMap::new());
        // ⌘N → the same key event the keyboard sends
        match decode("act:new_action", &km) {
            Cmd::Event(egui::Event::Key { key: Key::N, modifiers, .. }) => assert!(modifiers.command && !modifiers.shift),
            c => panic!("{c:?}"),
        }
        // Edit Note's only shortcut is the bare "n", which a menu can't own: it runs directly
        assert_eq!(decode("act:edit_note", &km), Cmd::Run(Action::EditNote));
        // Set Flag shows ⇧⌘L, not the bare "f"
        assert_eq!(menu_binding(&km, Action::ToggleFlag).map(|b| b.to_config()), Some("shift+cmd+l".into()));
        // a remapped shortcut is what the menu replays
        let mut over = BTreeMap::new();
        over.insert("new_action".to_string(), vec!["ctrl+cmd+j".to_string()]);
        let km2 = keys::bindings(&over);
        match decode("act:new_action", &km2) {
            Cmd::Event(egui::Event::Key { key: Key::J, modifiers, .. }) => assert!(modifiers.ctrl && modifiers.command),
            c => panic!("{c:?}"),
        }
        assert_eq!(decode("db:save_as", &km), Cmd::Other("db:save_as".into()));
        assert!(accel(&Binding::parse("alt+cmd+[").unwrap()).is_some());
    }
}
