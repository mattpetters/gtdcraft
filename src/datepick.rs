//! iPhone/iPad date fields use iOS's own date picker:
//! a sheet titled "Planned Date" (etc.) with Done, the inline calendar with its Time row (UIDatePicker, inline style),
//! and the quick buttons under it (Today, +1d, +1w, +1m, clear). Every change applies at once, so swiping the sheet
//! away keeps it; Done just closes it. Results go through `take` on the next frame.
use chrono::{Local, NaiveDateTime, TimeZone};

/// What the picker set: the field's slot (0 defer, 1 planned, 2 due), the task, and the new value (None = cleared).
pub type Picked = (usize, u64, Option<NaiveDateTime>);

/// The quick buttons' rule, shared with the inspector's own row: Today keeps the time; +1d/+1w/+1m add to it.
pub fn quick(i: usize, cur: Option<NaiveDateTime>, now: NaiveDateTime, default_time: chrono::NaiveTime) -> Option<NaiveDateTime> {
    let base = cur.unwrap_or_else(|| now.date().and_time(default_time));
    match i {
        0 => Some(now.date().and_time(cur.map(|x| x.time()).unwrap_or(default_time))),
        1 => Some(base + chrono::Duration::days(1)),
        2 => Some(base + chrono::Duration::weeks(1)),
        3 => base.checked_add_months(chrono::Months::new(1)),
        _ => None,
    }
}

#[allow(dead_code)] // used on iOS
fn to_secs(d: NaiveDateTime) -> f64 {
    Local.from_local_datetime(&d).earliest().map(|x| x.timestamp() as f64).unwrap_or(0.0)
}

#[allow(dead_code)] // used on iOS
fn from_secs(s: f64) -> Option<NaiveDateTime> {
    chrono::DateTime::from_timestamp(s as i64, 0).map(|x| x.with_timezone(&Local).naive_local())
}

#[cfg(target_os = "ios")]
pub use ios::{present, remember, take, top_vc, wake};

#[cfg(not(target_os = "ios"))]
#[allow(dead_code)]
pub fn take() -> Option<Picked> {
    None
}

#[cfg(target_os = "ios")]
mod ios {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, Sel};
    use objc2::{class, define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSString};
    use std::cell::RefCell;
    use std::sync::Mutex;

    static RESULT: Mutex<Vec<Picked>> = Mutex::new(Vec::new());
    static CTX: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();

    struct Session {
        slot: usize,
        task: u64,
        default_time: chrono::NaiveTime,
        picker: Retained<AnyObject>,
        nav: Retained<AnyObject>,
        _handler: Retained<Handler>,
    }
    thread_local! {
        static OPEN: RefCell<Option<Session>> = const { RefCell::new(None) };
    }

    fn send(value: Option<NaiveDateTime>) {
        OPEN.with(|o| {
            if let Some(s) = o.borrow().as_ref() {
                if let Ok(mut r) = RESULT.lock() {
                    r.push((s.slot, s.task, value));
                }
            }
        });
        if let Some(c) = CTX.get() {
            c.request_repaint();
        }
    }

    fn picker_date(p: &AnyObject) -> Option<NaiveDateTime> {
        let d: *mut AnyObject = unsafe { msg_send![p, date] };
        if d.is_null() {
            return None;
        }
        from_secs(unsafe { msg_send![d, timeIntervalSince1970] })
    }

    fn set_picker(p: &AnyObject, v: NaiveDateTime) {
        unsafe {
            let d: *mut AnyObject = msg_send![class!(NSDate), dateWithTimeIntervalSince1970: to_secs(v)];
            let _: () = msg_send![p, setDate: d, animated: Bool::YES];
        }
    }

    fn close() {
        let nav = OPEN.with(|o| o.borrow_mut().take().map(|s| s.nav));
        if let Some(nav) = nav {
            let none: *mut AnyObject = std::ptr::null_mut();
            unsafe {
                let _: () = msg_send![&*nav, dismissViewControllerAnimated: Bool::YES, completion: none];
            }
        }
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; Handler has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "GTDCraftDatePick"]
        struct Handler;

        impl Handler {
            #[unsafe(method(changed:))]
            fn changed(&self, sender: &AnyObject) {
                send(picker_date(sender));
            }

            #[unsafe(method(done:))]
            fn done(&self, _sender: &AnyObject) {
                close();
            }

            /// The quick buttons: a momentary segmented control (Today, +1d, +1w, +1m, clear).
            #[unsafe(method(quick:))]
            fn quick_tapped(&self, sender: &AnyObject) {
                let i: isize = unsafe { msg_send![sender, selectedSegmentIndex] };
                let Some((picker, dt)) = OPEN.with(|o| o.borrow().as_ref().map(|s| (s.picker.clone(), s.default_time))) else { return };
                if i == 4 {
                    send(None);
                    close();
                    return;
                }
                let now = Local::now().naive_local();
                if let Some(v) = quick(i as usize, picker_date(&picker), now, dt) {
                    set_picker(&picker, v);
                    send(Some(v));
                }
            }
        }
    );

    /// The view controller on top (the window's root, or whatever it's presenting), to present a sheet over.
    pub fn top_vc() -> Option<&'static AnyObject> {
        unsafe {
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let scenes: *mut AnyObject = msg_send![app, connectedScenes];
            let scene: *mut AnyObject = msg_send![scenes, anyObject];
            let windows: *mut AnyObject = msg_send![scene.as_ref()?, windows];
            let window: *mut AnyObject = msg_send![windows, firstObject];
            let mut top: *mut AnyObject = msg_send![window.as_ref()?, rootViewController];
            top.as_ref()?;
            loop {
                let next: *mut AnyObject = msg_send![top, presentedViewController];
                if next.is_null() {
                    return top.as_ref();
                }
                top = next;
            }
        }
    }

    /// Asks the app for a frame (a sheet handed it something).
    pub fn wake() {
        if let Some(c) = CTX.get() {
            c.request_repaint();
        }
    }

    /// The app's context, so the picker can wake it when a date changes.
    pub fn remember(ctx: &eframe::egui::Context) {
        let _ = CTX.set(ctx.clone());
    }

    /// Opens the sheet for one date field of one task (from the inspector or the row's field strip).
    pub fn present(title: &str, slot: usize, task: u64, cur: Option<NaiveDateTime>, default_time: chrono::NaiveTime) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if OPEN.with(|o| o.borrow().is_some()) {
            return;
        }
        let start = cur.unwrap_or_else(|| Local::now().naive_local().date().and_time(default_time));
        unsafe {
            let handler: Retained<Handler> = msg_send![Handler::alloc(mtm), init];
            let vc: Retained<AnyObject> = msg_send![class!(UIViewController), new];
            let view: *mut AnyObject = msg_send![&*vc, view];
            let bg: *mut AnyObject = msg_send![class!(UIColor), systemBackgroundColor];
            let _: () = msg_send![view, setBackgroundColor: bg];
            let _: () = msg_send![&*vc, setTitle: &*NSString::from_str(title)];

            // the inline calendar with its Time row
            let picker: Retained<AnyObject> = msg_send![class!(UIDatePicker), new];
            let _: () = msg_send![&*picker, setDatePickerMode: 2isize]; // UIDatePickerModeDateAndTime
            let _: () = msg_send![&*picker, setPreferredDatePickerStyle: 3isize]; // UIDatePickerStyleInline (iOS 14)
            set_picker(&picker, start);
            let changed: Sel = sel!(changed:);
            let _: () = msg_send![&*picker, addTarget: &*handler, action: changed, forControlEvents: 1usize << 12]; // ValueChanged

            // the quick buttons under it
            let items = NSArray::from_retained_slice(&["Today", "+1d", "+1w", "+1m", "Clear"].map(NSString::from_str));
            let seg: Retained<AnyObject> = msg_send![msg_send![class!(UISegmentedControl), alloc], initWithItems: &*items];
            let _: () = msg_send![&*seg, setMomentary: Bool::YES];
            let quick_sel: Sel = sel!(quick:);
            let _: () = msg_send![&*seg, addTarget: &*handler, action: quick_sel, forControlEvents: 1usize << 12];

            for v in [&picker, &seg] {
                let _: () = msg_send![&**v, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
                let _: () = msg_send![view, addSubview: &**v];
            }
            let guide: *mut AnyObject = msg_send![view, safeAreaLayoutGuide];
            let anchor = |o: *mut AnyObject, name: &str| -> *mut AnyObject {
                match name {
                    "top" => msg_send![o, topAnchor],
                    "bottom" => msg_send![o, bottomAnchor],
                    "leading" => msg_send![o, leadingAnchor],
                    _ => msg_send![o, trailingAnchor],
                }
            };
            let pin = |a: *mut AnyObject, b: *mut AnyObject, k: f64| -> Retained<AnyObject> { msg_send![a, constraintEqualToAnchor: b, constant: k] };
            let (p, s) = (Retained::as_ptr(&picker) as *mut AnyObject, Retained::as_ptr(&seg) as *mut AnyObject);
            let cons = [
                pin(anchor(p, "top"), anchor(guide, "top"), 8.0),
                pin(anchor(p, "leading"), anchor(guide, "leading"), 16.0),
                pin(anchor(p, "trailing"), anchor(guide, "trailing"), -16.0),
                pin(anchor(s, "top"), anchor(p, "bottom"), 12.0),
                pin(anchor(s, "leading"), anchor(guide, "leading"), 16.0),
                pin(anchor(s, "trailing"), anchor(guide, "trailing"), -16.0),
            ];
            let arr = NSArray::from_retained_slice(&cons);
            let _: () = msg_send![class!(NSLayoutConstraint), activateConstraints: &*arr];

            // Done at the right of the sheet's title
            let done_sel: Sel = sel!(done:);
            let done: Retained<AnyObject> = msg_send![msg_send![class!(UIBarButtonItem), alloc], initWithBarButtonSystemItem: 0isize, target: &*handler, action: done_sel];
            let item: *mut AnyObject = msg_send![&*vc, navigationItem];
            let _: () = msg_send![item, setRightBarButtonItem: &*done];
            let nav: Retained<AnyObject> = msg_send![msg_send![class!(UINavigationController), alloc], initWithRootViewController: &*vc];

            // present over whatever's on top
            let Some(top) = top_vc() else { return };
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![top, presentViewController: &*nav, animated: Bool::YES, completion: none];
            OPEN.with(|o| *o.borrow_mut() = Some(Session { slot, task, default_time, picker, nav, _handler: handler }));
        }
    }

    /// The next change the picker made, if any.
    pub fn take() -> Option<Picked> {
        // a sheet swiped away: forget the session (its changes already arrived)
        OPEN.with(|o| {
            let gone = o.borrow().as_ref().is_some_and(|s| {
                let pres: *mut AnyObject = unsafe { msg_send![&*s.nav, presentingViewController] };
                pres.is_null()
            });
            if gone {
                *o.borrow_mut() = None;
            }
        });
        RESULT.lock().ok().and_then(|mut r| (!r.is_empty()).then(|| r.remove(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn quick_buttons() {
        let t9 = chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().and_hms_opt(14, 30, 0).unwrap();
        let cur = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap().and_hms_opt(17, 0, 0).unwrap();
        assert_eq!(quick(0, Some(cur), now, t9), Some(now.date().and_hms_opt(17, 0, 0).unwrap())); // Today keeps the time
        assert_eq!(quick(0, None, now, t9), Some(now.date().and_time(t9)));
        assert_eq!(quick(1, None, now, t9), Some(now.date().and_time(t9) + chrono::Duration::days(1)));
        assert_eq!(quick(2, Some(cur), now, t9), Some(cur + chrono::Duration::weeks(1)));
        assert_eq!(quick(3, Some(cur), now, t9), NaiveDate::from_ymd_opt(2026, 12, 10).unwrap().and_hms_opt(17, 0, 0));
        assert_eq!(quick(4, Some(cur), now, t9), None);
        assert_eq!(from_secs(to_secs(cur)), Some(cur));
    }
}
