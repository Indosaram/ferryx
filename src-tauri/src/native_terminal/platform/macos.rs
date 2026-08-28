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
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, MainThreadMarker};
use objc2_app_kit::{NSView, NSWindow, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize};
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
    }
);

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
        let layer: Option<&AnyObject> = unsafe { msg_send![&view, layer] };
        if let Some(layer) = layer {
            unsafe {
                let _: () = msg_send![layer, setOpaque: true];
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
