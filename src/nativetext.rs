//! Text fields. On the Mac these are egui's `TextEdit`. On iPhone/iPad, egui's window only hears single keystrokes
//! (winit's iOS view adopts UIKeyInput, not UITextInput), so autocorrect, the Select All / Copy / Paste menu and
//! hold-to-delete-by-word never reach it (the person's bug list). There, while a field is being edited, a real
//! UITextField (one line) or UITextView (several) sits exactly over it and does the editing; GTDCraft copies its text
//! back every frame. Not being edited, the field is drawn by egui as usual.
#[cfg_attr(not(target_os = "ios"), allow(unused_imports))]
use eframe::egui::{self, vec2, Align2, Color32, FontId, Id, Margin, Rect, Response, Sense, Ui, Widget};

/// What the keyboard is for (iOS: URL fields don't autocorrect or capitalise).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Text,
    Url,
    Secret,
}

pub struct Edit<'t> {
    text: &'t mut String,
    multiline: bool,
    hint: String,
    size: f32,
    font: Option<FontId>,
    color: Option<Color32>,
    width: f32,
    frame: bool,
    rows: usize,
    margin: Margin,
    kind: Kind,
}

impl<'t> Edit<'t> {
    pub fn singleline(text: &'t mut String) -> Self {
        Edit { text, multiline: false, hint: String::new(), size: 14.0, font: None, color: None, width: f32::INFINITY, frame: true, rows: 1, margin: Margin::symmetric(4, 2), kind: Kind::Text }
    }
    pub fn multiline(text: &'t mut String) -> Self {
        Edit { multiline: true, rows: 4, ..Self::singleline(text) }
    }
    pub fn hint_text(mut self, h: impl Into<String>) -> Self {
        self.hint = h.into();
        self
    }
    /// egui's font (the Mac); iOS takes its size.
    pub fn font(mut self, f: FontId) -> Self {
        self.size = f.size;
        self.font = Some(f);
        self
    }
    pub fn password(mut self, on: bool) -> Self {
        if on {
            self.kind = Kind::Secret;
        }
        self
    }
    pub fn text_color(mut self, c: Color32) -> Self {
        self.color = Some(c);
        self
    }
    pub fn desired_width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    pub fn frame(mut self, f: bool) -> Self {
        self.frame = f;
        self
    }
    pub fn desired_rows(mut self, r: usize) -> Self {
        self.rows = r;
        self
    }
    pub fn margin(mut self, m: Margin) -> Self {
        self.margin = m;
        self
    }
    pub fn kind(mut self, k: Kind) -> Self {
        self.kind = k;
        self
    }
}

impl Widget for Edit<'_> {
    #[cfg(not(target_os = "ios"))]
    fn ui(self, ui: &mut Ui) -> Response {
        let te = if self.multiline { egui::TextEdit::multiline(self.text).desired_rows(self.rows) } else { egui::TextEdit::singleline(self.text) };
        let mut te = te.hint_text(self.hint).font(self.font.unwrap_or(FontId::proportional(self.size))).desired_width(self.width).frame(self.frame).margin(self.margin).password(self.kind == Kind::Secret);
        if let Some(c) = self.color {
            te = te.text_color(c);
        }
        ui.add(te)
    }

    #[cfg(target_os = "ios")]
    fn ui(self, ui: &mut Ui) -> Response {
        let id = ui.next_auto_id().with("native-text");
        let width = if self.width.is_finite() { self.width.min(ui.available_width()) } else { ui.available_width() };
        let inner_w = (width - self.margin.sum().x).max(20.0);
        let font = FontId::proportional(self.size);
        let color = self.color.unwrap_or_else(|| ui.visuals().text_color());
        // one line, or as many as the text needs (at least `rows`): the field grows rather than spilling out
        let line_h = ui.fonts_mut(|f| f.row_height(&font));
        let text_h = if self.multiline {
            let g = ui.fonts_mut(|f| f.layout(format!("{} ", self.text), font.clone(), color, inner_w));
            g.size().y.max(line_h * self.rows as f32)
        } else {
            line_h
        };
        let (rect, _) = ui.allocate_exact_size(vec2(width, text_h + self.margin.sum().y), Sense::hover());
        let mut resp = ui.interact(rect, id, Sense::click());
        if self.frame {
            ui.painter().rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
        }
        let inner = Rect::from_min_size(rect.min + self.margin.left_top(), vec2(inner_w, text_h));
        if resp.clicked() {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let focused = ui.memory(|m| m.has_focus(id));
        if focused {
            let pressed_elsewhere = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
            let (changed, ended) = ios::sync(id, inner, self.text, self.multiline, self.size, color, &self.hint, self.kind);
            if changed {
                resp.mark_changed();
            }
            if ended || pressed_elsewhere {
                ios::release(id);
                ui.memory_mut(|m| m.surrender_focus(id));
            }
            ui.ctx().request_repaint(); // keep copying the native field's text
        } else {
            ios::release(id);
            let (s, c) = if self.text.is_empty() { (self.hint.as_str(), color.gamma_multiply(0.45)) } else { (self.text.as_str(), color) };
            if self.multiline {
                let g = ui.fonts_mut(|f| f.layout(s.to_string(), font.clone(), c, inner_w));
                ui.painter().galley(inner.min, g, c);
            } else {
                let mut job = egui::text::LayoutJob::simple_singleline(if self.kind == Kind::Secret && !self.text.is_empty() { "•".repeat(self.text.chars().count()) } else { s.to_string() }, font, c);
                job.wrap = egui::text::TextWrapping::truncate_at_width(inner_w);
                let g = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(inner.left_center() - vec2(0.0, g.size().y / 2.0), g, c);
            }
            let _ = Align2::LEFT_CENTER;
        }
        resp
    }
}

/// iPhone/iPad: a Return in a one-line native field, to hand egui as an Enter key press on the next frame (callers
/// that submit on Enter keep working).
#[cfg(target_os = "ios")]
pub fn take_return() -> bool {
    ios::RETURNED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// iPhone/iPad: call once a frame, after drawing — a native field whose egui field wasn't drawn goes away.
#[cfg(target_os = "ios")]
pub fn end_frame() {
    ios::end_frame();
}

#[cfg(target_os = "ios")]
mod ios {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{class, define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::NSString;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub static RETURNED: AtomicBool = AtomicBool::new(false);
    static ENDED: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    unsafe impl objc2::Encode for CGRect {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGRect", &[objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]), objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING])]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Insets {
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    }
    unsafe impl objc2::Encode for Insets {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct("UIEdgeInsets", &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING]);
    }

    struct Active {
        id: Id,
        view: Retained<AnyObject>,
        _delegate: Retained<Delegate>,
        started: bool,
        text: String,
    }
    thread_local! {
        static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
        /// The active field was drawn this frame (if not — its page closed — the native view goes).
        static SEEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub fn end_frame() {
        if !SEEN.with(|s| s.replace(false)) {
            remove();
        }
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "GTDCraftTextDelegate"]
        struct Delegate;

        impl Delegate {
            /// Return in a one-line field: done editing (and an Enter for egui).
            #[unsafe(method(textFieldShouldReturn:))]
            fn should_return(&self, field: &AnyObject) -> Bool {
                RETURNED.store(true, Ordering::Relaxed);
                ENDED.store(true, Ordering::Relaxed);
                let _: Bool = unsafe { msg_send![field, resignFirstResponder] };
                Bool::NO
            }
        }
    );

    fn uicolor(c: Color32) -> *mut AnyObject {
        let [r, g, b, a] = c.to_srgba_unmultiplied().map(|x| x as f64 / 255.0);
        unsafe { msg_send![class!(UIColor), colorWithRed: r, green: g, blue: b, alpha: a] }
    }

    fn frame(r: Rect) -> CGRect {
        CGRect { x: r.left() as f64, y: r.top() as f64, w: r.width() as f64, h: r.height() as f64 }
    }

    fn get_text(v: &AnyObject) -> String {
        let s: Option<Retained<NSString>> = unsafe { msg_send![v, text] };
        s.map(|s| s.to_string()).unwrap_or_default()
    }

    /// Keeps the native field over `r` with `text`; returns (the user changed it, editing ended).
    #[allow(clippy::too_many_arguments)]
    pub fn sync(id: Id, r: Rect, text: &mut String, multi: bool, size: f32, color: Color32, hint: &str, kind: Kind) -> (bool, bool) {
        let Some(mtm) = MainThreadMarker::new() else { return (false, true) };
        let other = ACTIVE.with(|a| a.borrow().as_ref().is_some_and(|x| x.id != id));
        if other {
            remove();
        }
        let missing = ACTIVE.with(|a| a.borrow().is_none());
        if missing {
            let Some(top) = crate::datepick::top_vc() else { return (false, true) };
            unsafe {
                let root: *mut AnyObject = msg_send![top, view];
                let window: *mut AnyObject = msg_send![root, window];
                let Some(window) = window.as_ref() else { return (false, true) };
                let cls = if multi { class!(UITextView) } else { class!(UITextField) };
                let view: Retained<AnyObject> = msg_send![msg_send![cls, alloc], initWithFrame: frame(r)];
                let font: *mut AnyObject = msg_send![class!(UIFont), systemFontOfSize: size as f64];
                let _: () = msg_send![&*view, setFont: font];
                let _: () = msg_send![&*view, setTextColor: uicolor(color)];
                let _: () = msg_send![&*view, setTintColor: uicolor(crate::th().accent)];
                let clear: *mut AnyObject = msg_send![class!(UIColor), clearColor];
                let _: () = msg_send![&*view, setBackgroundColor: clear];
                let dark = crate::th().bg.r() < 128;
                let _: () = msg_send![&*view, setKeyboardAppearance: if dark { 1isize } else { 0isize }];
                let _: () = msg_send![&*view, setText: &*NSString::from_str(text)];
                if kind != Kind::Text {
                    let _: () = msg_send![&*view, setAutocorrectionType: 1isize]; // UITextAutocorrectionTypeNo
                    let _: () = msg_send![&*view, setAutocapitalizationType: 0isize]; // None
                    let _: () = msg_send![&*view, setSpellCheckingType: 1isize]; // No
                }
                if kind == Kind::Url {
                    let _: () = msg_send![&*view, setKeyboardType: 3isize]; // UIKeyboardTypeURL
                }
                let delegate: Retained<Delegate> = msg_send![Delegate::alloc(mtm), init];
                if multi {
                    // no inner padding or scrolling: it lines up with egui's text and grows with it
                    let _: () = msg_send![&*view, setScrollEnabled: Bool::NO];
                    let container: *mut AnyObject = msg_send![&*view, textContainer];
                    let _: () = msg_send![container, setLineFragmentPadding: 0.0f64];
                    let _: () = msg_send![&*view, setTextContainerInset: Insets { top: 0.0, left: 0.0, bottom: 0.0, right: 0.0 }];
                } else {
                    let _: () = msg_send![&*view, setDelegate: &*delegate];
                    let _: () = msg_send![&*view, setReturnKeyType: 9isize]; // UIReturnKeyDone
                    let ph: Retained<NSString> = NSString::from_str(hint);
                    let _: () = msg_send![&*view, setPlaceholder: &*ph];
                    if kind == Kind::Secret {
                        let _: () = msg_send![&*view, setSecureTextEntry: Bool::YES];
                    }
                }
                let _: () = msg_send![window, addSubview: &*view];
                let _: Bool = msg_send![&*view, becomeFirstResponder];
                ENDED.store(false, Ordering::Relaxed);
                ACTIVE.with(|a| *a.borrow_mut() = Some(Active { id, view, _delegate: delegate, started: false, text: text.clone() }));
            }
        }
        SEEN.with(|s| s.set(true));
        ACTIVE.with(|a| {
            let mut a = a.borrow_mut();
            let Some(act) = a.as_mut() else { return (false, true) };
            unsafe {
                let _: () = msg_send![&*act.view, setFrame: frame(r)];
            }
            let first: Bool = unsafe { msg_send![&*act.view, isFirstResponder] };
            act.started |= first.as_bool();
            // the app changed the text (a quick button, undo): show it
            if *text != act.text {
                unsafe {
                    let _: () = msg_send![&*act.view, setText: &*NSString::from_str(text)];
                }
                act.text = text.clone();
            }
            let now = get_text(&act.view);
            let changed = now != act.text;
            if changed {
                *text = now.clone();
                act.text = now;
            }
            // done: Return, or the keyboard went away (another native control took over)
            let ended = ENDED.swap(false, Ordering::Relaxed) || (act.started && !first.as_bool());
            (changed, ended)
        })
    }

    fn remove() {
        if let Some(act) = ACTIVE.with(|a| a.borrow_mut().take()) {
            unsafe {
                let _: Bool = msg_send![&*act.view, resignFirstResponder];
                let _: () = msg_send![&*act.view, removeFromSuperview];
            }
        }
    }

    /// The field with `id` isn't being edited: take its native view away.
    pub fn release(id: Id) {
        if ACTIVE.with(|a| a.borrow().as_ref().is_some_and(|x| x.id == id)) {
            remove();
        }
    }
}
