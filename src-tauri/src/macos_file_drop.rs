//! Window-level file drag destination for macOS.
//!
//! # Why this exists
//!
//! Neither of the two drag paths that ship with the stack can deliver a Finder file drop to
//! Ferryx:
//!
//! 1. tao registers the `NSWindow` itself for `NSFilenamesPboardType` and its window delegate
//!    answers every dragging message with `YES`. It reports the drop as
//!    `WindowEvent::DroppedFile`, which `tauri-runtime-wry` never translates into a Tauri
//!    event, so the drop is swallowed and discarded.
//! 2. wry's `WryWebView` implements `NSDraggingDestination` and would emit
//!    `WindowEvent::DragDrop`, but it was measured to never receive a single dragging message
//!    in this app, so `onDragDropEvent` never fires in the frontend.
//!
//! This module installs a drag destination Ferryx owns outright, so neither upstream path is
//! load-bearing. The view is ordered above the WKWebView to win the destination search and
//! returns `nil` from `hitTest:` so it stays completely transparent to mouse input.

use std::ffi::c_void;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSPasteboard, NSPasteboardTypeFileURL,
    NSView, NSWindow, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSString, NSURL};

/// Reported position is in flipped, logical view coordinates, so it lines up with
/// `getBoundingClientRect()` in the webview without any device-pixel conversion.
pub struct FileDropIvars {
    on_drop: Box<dyn Fn(Vec<String>, f64, f64) + Send + Sync>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "FerryxFileDropView"]
    #[ivars = FileDropIvars]
    pub struct FerryxFileDropView;

    impl FerryxFileDropView {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<&NSView> {
            // Returning nil keeps every click, scroll and hover flowing to the WKWebView
            // underneath. AppKit resolves dragging destinations separately from mouse hit
            // testing, so this does not stop the drag methods below from running.
            None
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for FerryxFileDropView {}

    unsafe impl NSDraggingDestination for FerryxFileDropView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.operation_for(info)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.operation_for(info)
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> Bool {
            let paths = collect_paths(info);
            tracing::info!("ferryx file drop: {} path(s)", paths.len());
            if paths.is_empty() {
                return Bool::NO;
            }

            let location = unsafe { info.draggingLocation() };
            let point = unsafe { self.convertPoint_fromView(location, None) };
            (self.ivars().on_drop)(paths, point.x, point.y);
            Bool::YES
        }
    }
);

impl FerryxFileDropView {
    fn operation_for(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        if collect_paths(info).is_empty() {
            NSDragOperation::None
        } else {
            NSDragOperation::Copy
        }
    }
}

fn legacy_filenames_type() -> Retained<NSString> {
    NSString::from_str("NSFilenamesPboardType")
}

fn collect_paths(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };

    let legacy = legacy_filenames_type();
    if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
        if let Ok(array) = list.downcast::<NSArray>() {
            let mut paths = Vec::new();
            for entry in array.iter() {
                if let Ok(text) = entry.downcast::<NSString>() {
                    paths.push(text.to_string());
                }
            }
            if !paths.is_empty() {
                return paths;
            }
        }
    }

    // Finder stops advertising the legacy type in some macOS releases, so fall back to the
    // modern file URL representation before giving up.
    let file_url_type = unsafe { NSPasteboardTypeFileURL };
    if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
        if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
            if let Some(path) = unsafe { url.path() } {
                return vec![path.to_string()];
            }
        }
    }

    Vec::new()
}

/// Installs the drag destination into the given `NSWindow`.
///
/// `on_drop` receives the dropped paths plus the drop point in flipped logical view
/// coordinates.
pub fn install<F>(raw_ns_window: *mut c_void, on_drop: F) -> Result<(), String>
where
    F: Fn(Vec<String>, f64, f64) + Send + Sync + 'static,
{
    if raw_ns_window.is_null() {
        return Err("NSWindow pointer is null".into());
    }
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "file drop view must be installed on the main thread".to_string())?;

    let ns_window: &NSWindow = unsafe { &*(raw_ns_window as *const NSWindow) };
    let content_view = ns_window
        .contentView()
        .ok_or_else(|| "NSWindow has no contentView".to_string())?;

    let bounds: NSRect = content_view.bounds();
    let alloc: Allocated<FerryxFileDropView> = mtm.alloc();
    let partial = alloc.set_ivars(FileDropIvars {
        on_drop: Box::new(on_drop),
    });
    let view: Retained<FerryxFileDropView> =
        unsafe { msg_send![super(partial), initWithFrame: bounds] };

    // NSViewWidthSizable | NSViewHeightSizable keeps the destination covering the whole
    // content area across window resizes.
    unsafe {
        let _: () = msg_send![&*view, setAutoresizingMask: 18usize];
    }

    let file_url_type = unsafe { NSPasteboardTypeFileURL };
    let legacy = legacy_filenames_type();
    let types = NSArray::from_slice(&[file_url_type, &*legacy]);
    unsafe { view.registerForDraggedTypes(&types) };

    content_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);

    // The view must outlive this call; the window owns it as a subview from here on.
    let _leaked: *mut AnyObject = Retained::into_raw(view).cast();

    Ok(())
}
