//! Keyboard shortcuts: every action with its default keys, plus user overrides stored in Settings as strings like "cmd+shift+n".
use eframe::egui::{Key, Modifiers};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub key: Key,
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Binding {
    pub fn parse(s: &str) -> Option<Binding> {
        let mut b = Binding { key: Key::A, cmd: false, shift: false, alt: false, ctrl: false };
        let mut key = None;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_lowercase().as_str() {
                "cmd" | "command" | "⌘" => b.cmd = true,
                "shift" | "⇧" => b.shift = true,
                "alt" | "option" | "opt" | "⌥" => b.alt = true,
                "ctrl" | "control" | "⌃" => b.ctrl = true,
                k => key = key_from(k),
            }
        }
        b.key = key?;
        Some(b)
    }
    pub fn format(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push('⌃');
        }
        if self.alt {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        if self.cmd {
            s.push('⌘');
        }
        s + &key_label(self.key)
    }
    pub fn to_config(&self) -> String {
        let mut parts = vec![];
        if self.ctrl {
            parts.push("ctrl".to_string());
        }
        if self.alt {
            parts.push("alt".into());
        }
        if self.shift {
            parts.push("shift".into());
        }
        if self.cmd {
            parts.push("cmd".into());
        }
        parts.push(self.key.name().to_lowercase());
        parts.join("+")
    }
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        self.key == key && self.cmd == m.command && self.shift == m.shift && self.alt == m.alt && self.ctrl == m.ctrl
    }
    /// Shortcuts with ⌘ or ⌃ also work while typing in a field; bare keys don't.
    pub fn works_while_typing(&self) -> bool {
        self.cmd || self.ctrl
    }
}

fn key_from(k: &str) -> Option<Key> {
    let alias = match k {
        "space" => "Space",
        "return" | "enter" => "Enter",
        "delete" | "backspace" => "Backspace",
        "forwarddelete" | "del" => "Delete",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "[" => "OpenBracket",
        "]" => "CloseBracket",
        "," => "Comma",
        "esc" | "escape" => "Escape",
        "tab" => "Tab",
        other => other,
    };
    Key::from_name(alias).or_else(|| Key::from_name(&alias.to_uppercase())).or_else(|| {
        let mut c = alias.chars();
        let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        Key::from_name(&cap)
    })
}

fn key_label(k: Key) -> String {
    match k {
        Key::Space => "Space".into(),
        Key::Enter => "↩".into(),
        Key::Backspace => "⌫".into(),
        Key::Delete => "⌦".into(),
        Key::ArrowUp => "↑".into(),
        Key::ArrowDown => "↓".into(),
        Key::ArrowLeft => "←".into(),
        Key::ArrowRight => "→".into(),
        Key::OpenBracket => "[".into(),
        Key::CloseBracket => "]".into(),
        Key::Comma => ",".into(),
        Key::Escape => "Esc".into(),
        k => k.symbol_or_name().to_string(),
    }
}

macro_rules! actions {
    ($($id:ident, $key:literal, $label:literal, $group:literal, [$($b:literal),*];)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Action { $($id),* }
        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$id),*];
            pub fn id(self) -> &'static str { match self { $(Action::$id => $key),* } }
            pub fn label(self) -> &'static str { match self { $(Action::$id => $label),* } }
            pub fn group(self) -> &'static str { match self { $(Action::$id => $group),* } }
            pub fn defaults(self) -> &'static [&'static str] { match self { $(Action::$id => &[$($b),*]),* } }
        }
    };
}

// Default keys per action; the last group is GTDCraft-specific.
actions! {
    QuickOpen, "quick_open", "Quick Open…", "Navigating", ["cmd+o"];
    OpenInbox, "open_inbox", "Open Inbox", "Navigating", ["cmd+1"];
    OpenProjects, "open_projects", "Open Projects", "Navigating", ["cmd+2"];
    OpenTags, "open_tags", "Open Tags", "Navigating", ["cmd+3"];
    OpenForecast, "open_forecast", "Open Forecast", "Navigating", ["cmd+4"];
    OpenFlagged, "open_flagged", "Open Flagged", "Navigating", ["cmd+5"];
    OpenNearby, "open_nearby", "Open Nearby", "Navigating", ["cmd+6"];
    OpenReview, "open_review", "Open Review", "Navigating", ["cmd+7"];
    Back, "back", "Go Back in History", "Navigating", ["alt+cmd+["];
    Forward, "forward", "Go Forward in History", "Navigating", ["alt+cmd+]"];
    PrevDay, "prev_day", "Select Previous Day", "Navigating", ["cmd+left"];
    Today, "today", "Select Today", "Navigating", ["cmd+t"];
    NextDay, "next_day", "Select Next Day", "Navigating", ["cmd+right"];
    PerspectivesList, "perspectives_list", "Show Perspectives List", "Navigating", ["ctrl+cmd+p"];
    QuickEntry, "quick_entry", "Quick Entry", "Creating Items", ["ctrl+alt+space"];
    NewAction, "new_action", "New Action", "Creating Items", ["cmd+n"];
    NewProject, "new_project", "New Project", "Creating Items", ["shift+cmd+n"];
    NewFolder, "new_folder", "New Folder", "Creating Items", ["shift+alt+cmd+n"];
    NewTag, "new_tag", "New Tag", "Creating Items", ["ctrl+cmd+n"];
    AddInside, "add_inside", "Add Inside", "Creating Items", ["shift+cmd+]"];
    AddOutside, "add_outside", "Add Outside", "Creating Items", ["shift+cmd+["];
    SelectNext, "select_next", "Select the next row", "Editing Items", ["down"];
    SelectPrev, "select_prev", "Select the previous row", "Editing Items", ["up"];
    NewBelow, "new_below", "Add new item below / end editing", "Editing Items", ["return"];
    NewAbove, "new_above", "Add new item above", "Editing Items", ["shift+return"];
    ToggleComplete, "toggle_complete", "Mark Completed or Active", "Editing Items", ["space"];
    ToggleDrop, "toggle_drop", "Mark Dropped or Active", "Editing Items", ["alt+space"];
    ToggleFlag, "toggle_flag", "Mark Flagged or Unflagged", "Editing Items", ["f", "shift+cmd+l"];
    EditTitle, "edit_title", "Edit Title", "Editing Items", ["e", "cmd+return"];
    EditNote, "edit_note", "Edit Note", "Editing Items", ["n"];
    EditProject, "edit_project", "Edit Project", "Editing Items", ["p"];
    EditTags, "edit_tags", "Edit Tags", "Editing Items", ["t"];
    EditDefer, "edit_defer", "Edit Defer Date", "Editing Items", ["shift+d"];
    EditPlanned, "edit_planned", "Edit Planned Date", "Editing Items", ["shift+p"];
    EditDue, "edit_due", "Edit Due Date", "Editing Items", ["d"];
    EditRepeat, "edit_repeat", "Edit Repeat", "Editing Items", ["r"];
    EditDuration, "edit_duration", "Edit Estimated Duration", "Editing Items", ["shift+t"];
    EditCompleted, "edit_completed", "Edit Completed Date", "Editing Items", ["c"];
    Undo, "undo", "Undo", "Editing Items", ["cmd+z"];
    Redo, "redo", "Redo", "Editing Items", ["shift+cmd+z"];
    Delete, "delete", "Delete", "Editing Items", ["cmd+backspace"];
    Duplicate, "duplicate", "Duplicate", "Editing Items", ["cmd+d"];
    ConvertToProject, "convert_to_project", "Convert Action to Project", "Editing Items", ["shift+cmd+1"];
    MarkReviewed, "mark_reviewed", "Mark Project Reviewed", "Editing Items", ["shift+cmd+r"];
    CleanUp, "clean_up", "Clean Up", "Moving Items", ["cmd+k"];
    MoveUp, "move_up", "Move Up", "Moving Items", ["ctrl+cmd+up"];
    MoveDown, "move_down", "Move Down", "Moving Items", ["ctrl+cmd+down"];
    Indent, "indent", "Indent (Move Right)", "Moving Items", ["cmd+]", "ctrl+cmd+right"];
    Outdent, "outdent", "Outdent (Move Left)", "Moving Items", ["cmd+[", "ctrl+cmd+left"];
    Group, "group", "Group", "Moving Items", ["alt+cmd+g"];
    Ungroup, "ungroup", "Ungroup", "Moving Items", ["alt+cmd+u"];
    Settings, "settings", "Settings…", "GTDCraft", ["cmd+,"];
    ToggleSidebar, "toggle_sidebar", "Show or Hide Sidebar", "GTDCraft", ["alt+cmd+s"];
    ToggleInspector, "toggle_inspector", "Show or Hide Inspector", "GTDCraft", ["alt+cmd+i"];
    ToggleBoard, "toggle_board", "Switch List / Board", "GTDCraft", ["alt+cmd+b"];
    Search, "search", "Search", "GTDCraft", ["cmd+f"];
    OpenCompleted, "open_completed", "Open Completed", "GTDCraft", ["cmd+9"];
    OpenChanged, "open_changed", "Open Changed", "GTDCraft", ["cmd+8"];
    OpenStats, "open_stats", "Open Stats", "GTDCraft", ["cmd+0"];
    ClockToggle, "clock_toggle", "Clock In / Clock Out", "Time", ["ctrl+cmd+i"];
    Pomodoro, "pomodoro", "Start / Stop Pomodoro", "Time", ["ctrl+cmd+t"];
    FocusWindow, "focus_window", "Show Focus Window", "Time", ["ctrl+cmd+m"];
    QueueAdd, "queue_add", "Add to Up Next", "Time", ["ctrl+cmd+u"];
}

/// Effective bindings: user overrides replace an action's defaults.
pub fn bindings(over: &BTreeMap<String, Vec<String>>) -> Vec<(Action, Binding)> {
    let mut out = vec![];
    for &a in Action::ALL {
        let src: Vec<String> = over.get(a.id()).cloned().unwrap_or_else(|| a.defaults().iter().map(|s| s.to_string()).collect());
        out.extend(src.iter().filter_map(|s| Binding::parse(s)).map(|b| (a, b)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_parse() {
        for &a in Action::ALL {
            for d in a.defaults() {
                assert!(Binding::parse(d).is_some(), "{} {d}", a.id());
            }
        }
        let b = Binding::parse("shift+alt+cmd+n").unwrap();
        assert_eq!(b.format(), "⌥⇧⌘N");
        assert_eq!(Binding::parse(&b.to_config()), Some(b));
        let mut over = BTreeMap::new();
        over.insert("new_action".to_string(), vec!["ctrl+n".to_string()]);
        let bs = bindings(&over);
        assert!(bs.iter().any(|(a, b)| *a == Action::NewAction && b.ctrl && !b.cmd));
        assert!(!bs.iter().any(|(a, b)| *a == Action::NewAction && b.cmd));
    }
}
