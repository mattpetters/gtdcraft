//! iPhone/iPad attachments (the person: "attachments on iOS should open a menu for files/photos"): Add from Files
//! (UIDocumentPickerViewController, copying the file in) or Add Photo (UIImagePickerController, the photo library),
//! saved under the app's Documents/Attachments so the path stays valid; tapping an attachment previews it with Quick
//! Look (iOS won't hand an app's own files to another app by URL).

/// The task or project the picked file goes on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Task(u64),
    Project(u64),
}

/// A file name that doesn't clash with what's already in `dir`: "Scan.pdf", then "Scan 2.pdf", "Scan 3.pdf"…
#[allow(dead_code)] // used on iOS
pub fn unique_name(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("Attachment");
    let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    let mut out = dir.join(format!("{stem}{ext}"));
    let mut n = 2;
    while out.exists() {
        out = dir.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    out
}

#[cfg(target_os = "ios")]
pub use ios::{preview, present_files, present_photos, take};

#[cfg(not(target_os = "ios"))]
#[allow(dead_code)]
pub fn take() -> Option<(Target, String)> {
    None
}

#[cfg(target_os = "ios")]
mod ios {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{class, define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSString};
    use std::cell::RefCell;
    use std::sync::Mutex;

    #[link(name = "UniformTypeIdentifiers", kind = "framework")]
    extern "C" {}
    #[link(name = "QuickLook", kind = "framework")]
    extern "C" {}

    static RESULT: Mutex<Vec<(Target, String)>> = Mutex::new(Vec::new());
    thread_local! {
        /// The open picker's target and its delegate (UIKit holds delegates weakly).
        static OPEN: RefCell<Option<(Target, Retained<Picker>)>> = const { RefCell::new(None) };
        static PREVIEW: RefCell<Option<(String, Retained<Preview>)>> = const { RefCell::new(None) };
    }

    fn dir() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_default();
        let d = std::path::PathBuf::from(home).join("Documents/Attachments");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Copies a picked file in and records it for its target.
    fn keep(url: &AnyObject) {
        let path: Option<Retained<NSString>> = unsafe { msg_send![url, path] };
        let Some(src) = path.map(|p| p.to_string()) else { return };
        let name = std::path::Path::new(&src).file_name().and_then(|s| s.to_str()).unwrap_or("Attachment").to_string();
        let dest = unique_name(&dir(), &name);
        // a security-scoped URL needs access around the copy (asCopy pickers hand over a temporary copy anyway)
        let scoped: Bool = unsafe { msg_send![url, startAccessingSecurityScopedResource] };
        let ok = std::fs::copy(&src, &dest).is_ok();
        if scoped.as_bool() {
            let _: () = unsafe { msg_send![url, stopAccessingSecurityScopedResource] };
        }
        let target = OPEN.with(|o| o.borrow().as_ref().map(|x| x.0));
        if let (true, Some(t), Ok(mut r)) = (ok, target, RESULT.lock()) {
            r.push((t, dest.to_string_lossy().into_owned()));
        }
        crate::datepick::wake();
    }

    fn dismiss(vc: &AnyObject) {
        let none: *mut AnyObject = std::ptr::null_mut();
        let _: () = unsafe { msg_send![vc, dismissViewControllerAnimated: Bool::YES, completion: none] };
        OPEN.with(|o| *o.borrow_mut() = None);
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "GTDCraftAttachPicker"]
        struct Picker;

        impl Picker {
            #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
            fn picked_files(&self, _picker: &AnyObject, urls: &NSArray<AnyObject>) {
                for u in urls.iter() {
                    keep(&u);
                }
                OPEN.with(|o| *o.borrow_mut() = None);
            }

            #[unsafe(method(documentPickerWasCancelled:))]
            fn files_cancelled(&self, _picker: &AnyObject) {
                OPEN.with(|o| *o.borrow_mut() = None);
            }

            #[unsafe(method(imagePickerController:didFinishPickingMediaWithInfo:))]
            fn picked_photo(&self, picker: &AnyObject, info: &AnyObject) {
                for key in ["UIImagePickerControllerImageURL", "UIImagePickerControllerMediaURL"] {
                    let u: *mut AnyObject = unsafe { msg_send![info, objectForKey: &*NSString::from_str(key)] };
                    if let Some(u) = unsafe { u.as_ref() } {
                        keep(u);
                        break;
                    }
                }
                dismiss(picker);
            }

            #[unsafe(method(imagePickerControllerDidCancel:))]
            fn photo_cancelled(&self, picker: &AnyObject) {
                dismiss(picker);
            }
        }
    );

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "GTDCraftAttachPreview"]
        struct Preview;

        impl Preview {
            #[unsafe(method(numberOfPreviewItemsInPreviewController:))]
            fn count(&self, _c: &AnyObject) -> isize {
                1
            }

            #[unsafe(method(previewController:previewItemAtIndex:))]
            fn item(&self, _c: &AnyObject, _i: isize) -> *mut AnyObject {
                let path = PREVIEW.with(|p| p.borrow().as_ref().map(|x| x.0.clone())).unwrap_or_default();
                // NSURL is a QLPreviewItem; autoreleased, as a getter's result
                unsafe { msg_send![class!(NSURL), fileURLWithPath: &*NSString::from_str(&path)] }
            }
        }
    );

    fn present(vc: &AnyObject) {
        if let Some(top) = crate::datepick::top_vc() {
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = unsafe { msg_send![top, presentViewController: vc, animated: Bool::YES, completion: none] };
        }
    }

    /// Add from Files: any file, copied in.
    pub fn present_files(target: Target) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        unsafe {
            let item: *mut AnyObject = msg_send![class!(UTType), typeWithIdentifier: &*NSString::from_str("public.item")];
            if item.is_null() {
                return;
            }
            let types: Retained<NSArray<AnyObject>> = msg_send![class!(NSArray), arrayWithObject: item];
            let picker: Retained<AnyObject> = msg_send![msg_send![class!(UIDocumentPickerViewController), alloc], initForOpeningContentTypes: &*types, asCopy: Bool::YES];
            let _: () = msg_send![&*picker, setAllowsMultipleSelection: Bool::YES];
            let delegate: Retained<Picker> = msg_send![Picker::alloc(mtm), init];
            let _: () = msg_send![&*picker, setDelegate: &*delegate];
            OPEN.with(|o| *o.borrow_mut() = Some((target, delegate)));
            present(&picker);
        }
    }

    /// Add Photo: the photo library (no permission prompt: the picker runs outside the app).
    pub fn present_photos(target: Target) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        unsafe {
            let picker: Retained<AnyObject> = msg_send![class!(UIImagePickerController), new];
            let _: () = msg_send![&*picker, setSourceType: 0isize]; // UIImagePickerControllerSourceTypePhotoLibrary
            let delegate: Retained<Picker> = msg_send![Picker::alloc(mtm), init];
            let _: () = msg_send![&*picker, setDelegate: &*delegate];
            OPEN.with(|o| *o.borrow_mut() = Some((target, delegate)));
            present(&picker);
        }
    }

    /// Quick Look for an attachment.
    pub fn preview(path: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        unsafe {
            let ds: Retained<Preview> = msg_send![Preview::alloc(mtm), init];
            PREVIEW.with(|p| *p.borrow_mut() = Some((path.to_string(), ds.clone())));
            let ql: Retained<AnyObject> = msg_send![class!(QLPreviewController), new];
            let _: () = msg_send![&*ql, setDataSource: &*ds];
            present(&ql);
        }
    }

    /// The next file picked, with where it goes.
    pub fn take() -> Option<(Target, String)> {
        RESULT.lock().ok().and_then(|mut r| (!r.is_empty()).then(|| r.remove(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_clash() {
        let d = std::env::temp_dir().join(format!("gtd-attach-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(unique_name(&d, "Scan.pdf"), d.join("Scan.pdf"));
        std::fs::write(d.join("Scan.pdf"), b"x").unwrap();
        assert_eq!(unique_name(&d, "Scan.pdf"), d.join("Scan 2.pdf"));
        std::fs::write(d.join("Scan 2.pdf"), b"x").unwrap();
        assert_eq!(unique_name(&d, "Scan.pdf"), d.join("Scan 3.pdf"));
        assert_eq!(unique_name(&d, ""), d.join("Attachment"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
