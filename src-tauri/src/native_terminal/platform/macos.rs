//! macOS Platform Child View Compositor Target for Native Terminal.
//!
//! Provides a dedicated, layer-backed child NSView sibling placed directly above
//! the Tauri WKWebView inside the main NSWindow contentView.
//!
//! # Safety Invariants
//!
//! 1. **Thread Affinity**: AppKit view hierarchy mutations (`addSubview`, `removeFromSuperview`,
//!    `setFrame`, `setWantsLayer`) must happen on the macOS main thread (`MainThreadMarker`).
//! 2. **Pointer Validity & Lifetime**: The Objective-C `FerryxNativeTerminalView` instance is owned
//!    by `MacosCompositorTarget` via a retained raw pointer for the lifetime of the host. Its
//!    underlying pointer remains valid and pinned in memory for raw-window-handle surface creation and lifetime.
//! 3. **Pointer Transparency**: `FerryxNativeTerminalView` overrides `-[NSView hitTest:]` to return
//!    `nil` (`None`), ensuring mouse/scroll/click events transparently reach the WKWebView and React UI below.
//! 4. **Teardown & Cleanup**: On `Drop` of `MacosCompositorTarget`, the view is removed from its superview
//!    and released on the main thread (dispatched to the main queue if dropped from a worker thread).

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker};
#[allow(deprecated)] // NSFilenamesPboardType mirrors wry's drag payload collection.
use objc2_app_kit::NSFilenamesPboardType;
use objc2_app_kit::{
    NSDragOperation, NSDraggingInfo, NSPasteboardType, NSView, NSWindow, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};

static LOGGED_FIRST_RESPONDER_ERROR: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn log_first_responder_error_once(msg: &str) {
    if !LOGGED_FIRST_RESPONDER_ERROR.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!("Native terminal first responder error: {msg}");
    }
}

use crate::native_terminal::error::NativeTerminalError;

/// Appends one JSONL record to the shared switch-debug log so release builds
/// (which have no devtools console) can be traced after the fact.
///
/// Tracing is on in debug builds and in release builds launched with
/// `FERRYX_SWITCH_DEBUG=1`, matching the JS-side `switchDebug` sink and the
/// `cmd_switch_debug_log` command. When disabled this is a no-op, so the drag
/// instrumentation below is free in normal operation.
fn switch_debug_log(event: &str, details: serde_json::Value) {
    if !crate::ipc::debug::switch_debug_sink_enabled(
        cfg!(debug_assertions),
        std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref(),
    ) {
        return;
    }
    let wall_time_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let record = serde_json::json!({
        "runId": "rust-native-terminal",
        "sequence": 0,
        "event": event,
        "wallTimeMs": wall_time_ms,
        "details": details,
    });
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/ferryx-switch-debug.jsonl")
        .and_then(|mut file| {
            std::io::Write::write_all(&mut file, format!("{record}\n").as_bytes())
        });
}

/// Reads the file paths offered by a drag session, mirroring wry's
/// `collect_paths` so the instrumentation logs the same payload the webview
/// would receive (empty when the pasteboard carries no filenames).
#[allow(deprecated)] // NSFilenamesPboardType matches wry's collect_paths; logging must mirror what the webview sees.
fn drag_info_paths(drag_info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let pb = drag_info.draggingPasteboard();
    let filenames_type: Retained<NSPasteboardType> = unsafe {
        Retained::retain(NSFilenamesPboardType as *const NSPasteboardType as *mut NSPasteboardType)
    }
    .expect("NSFilenamesPboardType is a non-null static");
    let types = NSArray::from_retained_slice(&[filenames_type]);
    let mut paths = Vec::new();
    if pb.availableTypeFromArray(&types).is_some() {
        if let Some(plist) = pb.propertyListForType(unsafe { NSFilenamesPboardType }) {
            if let Some(array) = plist.downcast::<NSArray>().ok() {
                for item in array {
                    if let Ok(path) = item.downcast::<NSString>() {
                        paths.push(path.to_string());
                    }
                }
            }
        }
    }
    paths
}

define_class!(
    /// Custom NSView subclass that is completely pointer-transparent.
    /// Returning `None` from `hitTest:` allows all clicks and pointer interactions
    /// to fall through to the underlying WKWebView and React components.
    #[unsafe(super(NSView))]
    #[name = "FerryxNativeTerminalView"]
    pub struct FerryxNativeTerminalView;

    impl FerryxNativeTerminalView {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<&NSView> {
            // In AppKit, returning nil from hitTest: indicates this view does not consume
            // mouse events, passing them to views below it in the hierarchy.
            None
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        // Drag instrumentation: this view sits ABOVE the WKWebView in the
        // contentView hierarchy and is pointer-transparent only for mouse
        // events (`hitTest:` returns nil). AppKit's drag-session routing is a
        // separate path, so these overrides log whether the drag reaches this
        // view at all. Each forwards to the default NSView implementation so
        // the drop keeps falling through to the webview.
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            drag_info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            let location = drag_info.draggingLocation();
            let frame = self.frame();
            switch_debug_log(
                "terminal.surface.drag.native.entered",
                serde_json::json!({
                    "location": { "x": location.x, "y": location.y },
                    "frame": {
                        "x": frame.origin.x,
                        "y": frame.origin.y,
                        "width": frame.size.width,
                        "height": frame.size.height,
                    },
                    "paths": drag_info_paths(drag_info),
                }),
            );
            unsafe { msg_send![super(self), draggingEntered: drag_info] }
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(
            &self,
            drag_info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            let location = drag_info.draggingLocation();
            switch_debug_log(
                "terminal.surface.drag.native.updated",
                serde_json::json!({
                    "location": { "x": location.x, "y": location.y },
                }),
            );
            unsafe { msg_send![super(self), draggingUpdated: drag_info] }
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(
            &self,
            drag_info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> bool {
            let location = drag_info.draggingLocation();
            switch_debug_log(
                "terminal.surface.drag.native.performDrop",
                serde_json::json!({
                    "location": { "x": location.x, "y": location.y },
                    "paths": drag_info_paths(drag_info),
                }),
            );
            unsafe { msg_send![super(self), performDragOperation: drag_info] }
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, drag_info: &ProtocolObject<dyn NSDraggingInfo>) {
            switch_debug_log("terminal.surface.drag.native.exited", serde_json::json!({}));
            unsafe { msg_send![super(self), draggingExited: drag_info] }
        }
    }
);

impl FerryxNativeTerminalView {
    pub fn window_backing_scale_factor(&self) -> f64 {
        let Some(window) = self.window() else {
            return 1.0;
        };
        let scale = window.backingScaleFactor();
        let scale: f64 = scale.into();
        if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        }
    }
}

impl FerryxNativeTerminalView {
    /// Initializes a new instance with the given frame bounds.
    pub fn new(
        bounds: NSRect,
        mtm: MainThreadMarker,
    ) -> Result<Retained<Self>, NativeTerminalError> {
        let alloc: Allocated<Self> = mtm.alloc();
        let partial = alloc.set_ivars(());
        let view: Option<Retained<Self>> =
            unsafe { msg_send![super(partial), initWithFrame: bounds] };
        view.ok_or_else(|| {
            NativeTerminalError::GpuPipelineError(
                "Failed to initialize FerryxNativeTerminalView".into(),
            )
        })
    }
}

/// Safe handle implementing `HasWindowHandle` and `HasDisplayHandle` for wgpu surface creation.
pub struct NativeChildViewHandle {
    raw_view_ptr: NonNull<c_void>,
}

// SAFETY: Raw pointer to the retained NSView is immutable and safe to reference across threads.
unsafe impl Send for NativeChildViewHandle {}
unsafe impl Sync for NativeChildViewHandle {}

impl HasWindowHandle for NativeChildViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = AppKitWindowHandle::new(self.raw_view_ptr);
        let raw = RawWindowHandle::AppKit(handle);
        // SAFETY: The raw AppKit window handle wraps a valid NSView pointer retained by MacosCompositorTarget.
        unsafe { Ok(WindowHandle::borrow_raw(raw)) }
    }
}

impl HasDisplayHandle for NativeChildViewHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = AppKitDisplayHandle::new();
        let raw = RawDisplayHandle::AppKit(handle);
        // SAFETY: AppKit display handle is stateless and always valid on macOS.
        unsafe { Ok(DisplayHandle::borrow_raw(raw)) }
    }
}

/// macOS native child compositor target managing the layer-backed child NSView.
pub struct MacosCompositorTarget {
    view_ptr: NonNull<c_void>,
    handle: Arc<NativeChildViewHandle>,
}

// SAFETY: `MacosCompositorTarget` owns the retained raw pointer to the `FerryxNativeTerminalView`.
// All AppKit UI interactions and drops are guarded or dispatched to the main thread.
unsafe impl Send for MacosCompositorTarget {}
unsafe impl Sync for MacosCompositorTarget {}

impl MacosCompositorTarget {
    /// Creates a layer-backed child view above WKWebView in the window content view.
    pub fn new<R: Runtime>(window: &WebviewWindow<R>) -> Result<Self, NativeTerminalError> {
        let raw_ns_window = window.ns_window().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get NSWindow: {e}"))
        })?;

        if raw_ns_window.is_null() {
            return Err(NativeTerminalError::GpuPipelineError(
                "NSWindow pointer is null".into(),
            ));
        }

        // SAFETY: NSWindow pointer was retrieved from Tauri window.
        // We obtain the MainThreadMarker to safely interact with AppKit UI classes.
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            NativeTerminalError::GpuPipelineError(
                "MacosCompositorTarget must be initialized on the main thread".into(),
            )
        })?;

        let ns_window: &NSWindow = unsafe { &*(raw_ns_window as *const NSWindow) };
        let content_view = ns_window.contentView().ok_or_else(|| {
            NativeTerminalError::GpuPipelineError("NSWindow has no contentView".into())
        })?;

        let bounds: NSRect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));

        // Instantiate the pointer-transparent custom NSView
        let view = FerryxNativeTerminalView::new(bounds, mtm)?;

        // Configure layer-backing
        view.setWantsLayer(true);
        let backing_scale = view.window_backing_scale_factor();
        let layer: Option<&AnyObject> = unsafe { msg_send![&view, layer] };
        if let Some(layer) = layer {
            unsafe {
                let _: () = msg_send![layer, setOpaque: true];
                let scale: f64 = if backing_scale.is_finite() && backing_scale > 0.0 {
                    backing_scale
                } else {
                    1.0
                };
                let _: () = msg_send![layer, setContentsScale: scale];
                let gravity = NSString::from_str("topLeft");
                let _: () = msg_send![layer, setContentsGravity: &*gravity];
                let nearest = NSString::from_str("nearest");
                let _: () = msg_send![layer, setMagnificationFilter: &*nearest];
                let _: () = msg_send![layer, setMinificationFilter: &*nearest];
            }
        }

        // Initially hidden until an active terminal layout is positioned
        view.setHidden(true);

        // Add child view positioned above all other subviews (such as WKWebView)
        content_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);

        let raw_view_ptr =
            NonNull::new(Retained::into_raw(view) as *mut c_void).ok_or_else(|| {
                NativeTerminalError::GpuPipelineError("Child view pointer is null".into())
            })?;

        let handle = Arc::new(NativeChildViewHandle { raw_view_ptr });

        Ok(Self {
            view_ptr: raw_view_ptr,
            handle,
        })
    }

    /// Returns the raw-window-handle target for wgpu surface creation.
    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    pub fn window_backing_scale_factor(&self) -> f64 {
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = mtm;
            let view = unsafe { &*(self.view_ptr.as_ptr() as *const FerryxNativeTerminalView) };
            view.window_backing_scale_factor()
        } else {
            1.0
        }
    }

    /// Returns the target descriptor for this platform compositor target.
    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::NativeChildView,
            pointer_transparent: true,
            layer_backed: true,
        }
    }

    /// Sets the child view frame to match the active terminal viewport in AppKit coordinates.
    /// When `bounds` is None, the view is hidden and zeroed out so it cannot intercept events.
    pub fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        let view_ptr = self.view_ptr.as_ptr() as usize;
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = mtm;
            let view = unsafe { &*(view_ptr as *const FerryxNativeTerminalView) };
            unsafe {
                if let Some(bounds) = bounds {
                    if let Some(superview) = view.superview() {
                        let superview_bounds = superview.bounds();
                        let is_flipped = superview.isFlipped();
                        let appkit_frame =
                            bounds.to_appkit_frame(superview_bounds.size.height, is_flipped);
                        view.setFrame(NSRect::new(
                            NSPoint::new(appkit_frame.x, appkit_frame.y),
                            NSSize::new(appkit_frame.width, appkit_frame.height),
                        ));
                        let layer: Option<&AnyObject> = msg_send![view, layer];
                        if let Some(layer) = layer {
                            let scale: f64 = bounds.scale_factor;
                            let _: () = msg_send![layer, setContentsScale: scale];
                            let gravity = NSString::from_str("topLeft");
                            let _: () = msg_send![layer, setContentsGravity: &*gravity];
                            let nearest = NSString::from_str("nearest");
                            let _: () = msg_send![layer, setMagnificationFilter: &*nearest];
                            let _: () = msg_send![layer, setMinificationFilter: &*nearest];
                            let drawable_w = (appkit_frame.width * scale).round();
                            let drawable_h = (appkit_frame.height * scale).round();
                            let _: () = msg_send![layer, setDrawableSize: NSSize::new(drawable_w, drawable_h)];
                        }
                    }
                } else {
                    view.setHidden(true);
                    view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)));
                }
            }
        } else {
            dispatch2::DispatchQueue::main().exec_async(move || unsafe {
                let view = &*(view_ptr as *const FerryxNativeTerminalView);
                if let Some(bounds) = bounds {
                    if let Some(superview) = view.superview() {
                        let superview_bounds = superview.bounds();
                        let is_flipped = superview.isFlipped();
                        let appkit_frame =
                            bounds.to_appkit_frame(superview_bounds.size.height, is_flipped);
                        view.setFrame(NSRect::new(
                            NSPoint::new(appkit_frame.x, appkit_frame.y),
                            NSSize::new(appkit_frame.width, appkit_frame.height),
                        ));
                        let layer: Option<&AnyObject> = msg_send![view, layer];
                        if let Some(layer) = layer {
                            let scale: f64 = bounds.scale_factor;
                            let _: () = msg_send![layer, setContentsScale: scale];
                            let gravity = NSString::from_str("topLeft");
                            let _: () = msg_send![layer, setContentsGravity: &*gravity];
                            let nearest = NSString::from_str("nearest");
                            let _: () = msg_send![layer, setMagnificationFilter: &*nearest];
                            let _: () = msg_send![layer, setMinificationFilter: &*nearest];
                            let drawable_w = (appkit_frame.width * scale).round();
                            let drawable_h = (appkit_frame.height * scale).round();
                            let _: () = msg_send![layer, setDrawableSize: NSSize::new(drawable_w, drawable_h)];
                        }
                    }
                } else {
                    view.setHidden(true);
                    view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)));
                }
            });
        }
    }

    pub fn restore_webview_first_responder<R: Runtime>(&self, window: &WebviewWindow<R>) {
        let view_ptr = self.view_ptr.as_ptr() as usize;
        let window_clone = window.clone();
        let restore = move || unsafe {
            let view = &*(view_ptr as *const FerryxNativeTerminalView);
            let Some(ns_window) = view.window() else {
                log_first_responder_error_once("child view has no window");
                return;
            };

            let raw_webview_view = match window_clone.ns_view() {
                Ok(ptr) if !ptr.is_null() => ptr,
                Ok(_) => {
                    log_first_responder_error_once("webview NSView pointer is null");
                    return;
                }
                Err(err) => {
                    log_first_responder_error_once(&format!("failed to get webview NSView: {err}"));
                    return;
                }
            };

            let webview_view = match Retained::retain(raw_webview_view as *mut NSView) {
                Some(view) => view,
                None => {
                    log_first_responder_error_once("failed to retain webview NSView");
                    return;
                }
            };

            // Live evidence (2026-08-28): calling makeFirstResponder unconditionally after
            // every presented frame steals first responder from the webview's inner key-handling
            // view on the echo repaint, so only the first keystroke after each click ever reached
            // the DOM. Skip the restore when focus is already anywhere inside the webview hierarchy.
            let first_responder_inside_webview = match ns_window.firstResponder() {
                Some(responder) => match responder.downcast_ref::<NSView>() {
                    Some(fr_view) => {
                        std::ptr::eq(fr_view, &*webview_view)
                            || fr_view.isDescendantOf(&webview_view)
                    }
                    None => false,
                },
                None => false,
            };
            if first_responder_inside_webview {
                return;
            }

            let _ = ns_window.makeFirstResponder(Some(&webview_view));
        };

        if let Some(_mtm) = MainThreadMarker::new() {
            restore();
        } else {
            dispatch2::DispatchQueue::main().exec_async(restore);
        }
    }

    /// Reveals the child view by unhiding it. Must only be called after
    /// the first rendered frame has been presented.
    pub fn reveal(&self) {
        let view_ptr = self.view_ptr.as_ptr() as usize;
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = mtm;
            let view = unsafe { &*(view_ptr as *const FerryxNativeTerminalView) };
            view.setHidden(false);
        } else {
            dispatch2::DispatchQueue::main().exec_async(move || unsafe {
                let view = &*(view_ptr as *const FerryxNativeTerminalView);
                view.setHidden(false);
            });
        }
    }
}

impl Drop for MacosCompositorTarget {
    fn drop(&mut self) {
        let raw_ptr = self.view_ptr.as_ptr() as usize;
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = mtm;
            unsafe {
                if let Some(view) = Retained::from_raw(raw_ptr as *mut FerryxNativeTerminalView) {
                    view.removeFromSuperview();
                }
            }
        } else {
            // When dropping from a worker thread, dispatch removeFromSuperview and release to main queue.
            dispatch2::DispatchQueue::main().exec_async(move || unsafe {
                if let Some(view) = Retained::from_raw(raw_ptr as *mut FerryxNativeTerminalView) {
                    view.removeFromSuperview();
                }
            });
        }
    }
}
