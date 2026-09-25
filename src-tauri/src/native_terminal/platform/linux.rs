//! Linux Platform Compositor Target for Native Terminal.
//!
//! Provides an X11/Wayland composition surface target implementing `HasWindowHandle`
//! and `HasDisplayHandle` for wgpu surface creation.
//!
//! # Safety Invariants
//!
//! 1. **Handle Lifetime**: The `NativeChildViewHandle` retains the X11 Window/Wayland surface
//!    handle obtained from the parent window, valid for the lifetime of the compositor target.
//! 2. **Thread Safety**: Linux window and display handles are safe to borrow and query across threads.
//! 3. **Capability Reporting**: A session with a real isolated child -- an `InputOutput` window
//!    under X11/XWayland, a `wl_subsurface` under native Wayland -- reports layer-backed and
//!    pointer-transparent for that child. A session whose child could not be created (no
//!    `wl_subcompositor`, `FERRYX_DISABLE_WAYLAND_SUBSURFACE=1`, or a failed X11 child) has only
//!    the whole parent GTK window left as a surface, which nothing positions at the pane and
//!    nothing clips to it, so it reports that whole-window surface truthfully and fails
//!    composition instead of claiming child-surface capabilities it does not have;
//!    `LinuxChildSurfaceAbsence` names the cause.

use std::ffi::{c_int, c_uint, c_ulong, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle, XlibDisplayHandle,
    XlibWindowHandle,
};
use tauri::{Runtime, Window};

use crate::native_terminal::child_surface::{
    ChildSurfaceGeometry, ChildSurfaceVisibility, GeometryLatch, WaylandSubsurfaceGeometry,
};
use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};
use crate::native_terminal::error::NativeTerminalError;
use crate::native_terminal::platform::wayland_child::WaylandChild;

const INPUT_OUTPUT: u32 = 1;
const COPY_FROM_PARENT: c_ulong = 0;
const SHAPE_INPUT: c_int = 2;
const SHAPE_SET: c_int = 0;

#[link(name = "X11")]
unsafe extern "C" {
    fn XCreateSimpleWindow(
        display: *mut c_void,
        parent: c_ulong,
        x: c_int,
        y: c_int,
        width: u32,
        height: u32,
        border_width: u32,
        border: c_ulong,
        background: c_ulong,
    ) -> c_ulong;
    fn XMoveResizeWindow(
        display: *mut c_void,
        window: c_ulong,
        x: c_int,
        y: c_int,
        width: u32,
        height: u32,
    ) -> c_int;
    fn XMapWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XRaiseWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XUnmapWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XDestroyWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XQueryTree(
        display: *mut c_void,
        window: c_ulong,
        root_return: *mut c_ulong,
        parent_return: *mut c_ulong,
        children_return: *mut *mut c_ulong,
        nchildren_return: *mut c_uint,
    ) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
    fn XFlush(display: *mut c_void) -> c_int;
}

#[link(name = "Xext")]
unsafe extern "C" {
    fn XShapeCombineRectangles(
        display: *mut c_void,
        window: c_ulong,
        kind: c_int,
        x_off: c_int,
        y_off: c_int,
        rectangles: *const c_void,
        n_rects: c_int,
        op: c_int,
        ordering: c_int,
    );
}

struct X11Child {
    display: *mut c_void,
    window: c_ulong,
}

// SAFETY: the child window and its display connection are only touched from the Tauri main
// thread, and the pointers stay valid for the lifetime of the compositor target.
unsafe impl Send for X11Child {}
unsafe impl Sync for X11Child {}

impl X11Child {
    fn create(display: *mut c_void, parent: c_ulong) -> Option<Self> {
        if display.is_null() || parent == 0 {
            return None;
        }
        // SAFETY: `display` is the live Xlib connection owned by GTK and `parent` is the
        // Tauri window returned by tao, both valid for the duration of this call.
        let window = unsafe {
            XCreateSimpleWindow(
                display,
                parent,
                0,
                0,
                1,
                1,
                0,
                COPY_FROM_PARENT,
                COPY_FROM_PARENT,
            )
        };
        if window == 0 {
            return None;
        }
        // SAFETY: an empty input rectangle list makes the child ignore pointer events so
        // WebKit keeps receiving them; the child window was just created above.
        unsafe {
            XShapeCombineRectangles(
                display,
                window,
                SHAPE_INPUT,
                0,
                0,
                std::ptr::null(),
                0,
                SHAPE_SET,
                0,
            );
            XFlush(display);
        }
        Some(Self { display, window })
    }
}

impl Drop for X11Child {
    fn drop(&mut self) {
        // SAFETY: the window was created by `X11Child::create` and is destroyed exactly once.
        unsafe {
            XUnmapWindow(self.display, self.window);
            XDestroyWindow(self.display, self.window);
            XFlush(self.display);
        }
    }
}

#[derive(Debug, Clone)]
enum LinuxWindowHandleInner {
    Xlib {
        window: c_ulong,
        /// An Xlib visual ID, or 0 if unknown (`raw-window-handle` uses a plain `c_ulong`
        /// here).
        visual_id: c_ulong,
    },
    // No `Xcb` arm: tao's Linux backend only ever hands back an Xlib or Wayland handle, so an
    // XCB pair would be unreachable. Leaving one would also re-open the silent degradation this
    // module now reports explicitly, because an unconverted pair used to fall through to the
    // no-child path. An XCB handle now fails loudly at construction instead.
    Wayland {
        surface: NonNull<c_void>,
    },
}

#[derive(Debug, Clone)]
enum LinuxDisplayHandleInner {
    Xlib {
        display: Option<NonNull<c_void>>,
        screen: c_int,
    },
    // See `LinuxWindowHandleInner`: tao never reports an XCB connection on Linux.
    Wayland {
        display: NonNull<c_void>,
    },
}

/// Safe handle implementing `HasWindowHandle` and `HasDisplayHandle` for wgpu surface creation on Linux.
pub struct NativeChildViewHandle {
    window_inner: LinuxWindowHandleInner,
    display_inner: LinuxDisplayHandleInner,
}

// SAFETY: Linux display and window handles/pointers are safe to send and reference across threads.
unsafe impl Send for NativeChildViewHandle {}
unsafe impl Sync for NativeChildViewHandle {}

impl HasWindowHandle for NativeChildViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = match &self.window_inner {
            LinuxWindowHandleInner::Xlib { window, visual_id } => {
                let mut handle = XlibWindowHandle::new(*window);
                handle.visual_id = *visual_id;
                RawWindowHandle::Xlib(handle)
            }
            LinuxWindowHandleInner::Wayland { surface } => {
                let handle = WaylandWindowHandle::new(*surface);
                RawWindowHandle::Wayland(handle)
            }
        };
        // SAFETY: The raw window handle is borrowed from a valid initialized Linux window target.
        unsafe { Ok(WindowHandle::borrow_raw(raw)) }
    }
}

impl HasDisplayHandle for NativeChildViewHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = match &self.display_inner {
            LinuxDisplayHandleInner::Xlib { display, screen } => {
                let handle = XlibDisplayHandle::new(*display, *screen);
                RawDisplayHandle::Xlib(handle)
            }
            LinuxDisplayHandleInner::Wayland { display } => {
                let handle = WaylandDisplayHandle::new(*display);
                RawDisplayHandle::Wayland(handle)
            }
        };
        // SAFETY: The raw display handle wraps a valid Linux X11/Wayland display connection.
        unsafe { Ok(DisplayHandle::borrow_raw(raw)) }
    }
}

enum LinuxChild {
    X11(X11Child),
    Wayland(WaylandChild),
}

/// Descriptor for a Linux target that owns an isolated child surface: an X11 child window or a
/// `wl_subsurface`, both created with an empty input region, so the child clips rendering to the
/// pane and passes pointer events through to the webview.
pub const fn child_surface_descriptor() -> PlatformCompositorDescriptor {
    PlatformCompositorDescriptor {
        target_kind: CompositorTargetKind::LinuxChildWindow,
        pointer_transparent: true,
        layer_backed: true,
    }
}

/// Descriptor for a Linux target with no isolated child surface.
///
/// Without a child, the only surface wgpu can render into is the whole parent GTK window: nothing
/// clips it to the pane, nothing positions it there (see `LinuxCompositorTarget::update_viewport`),
/// and the webview draws over the same window. Reporting it as the root webview window, with no
/// layer backing and no pointer transparency, is what it is, so composition validation rejects the
/// target loudly. Reporting `LinuxChildWindow` capabilities here would be a fabrication that
/// bypasses that check and renders the terminal over the whole window instead.
pub const fn parent_window_surface_descriptor() -> PlatformCompositorDescriptor {
    PlatformCompositorDescriptor {
        target_kind: CompositorTargetKind::RootWebviewWindow,
        pointer_transparent: false,
        layer_backed: false,
    }
}

/// Why no isolated child surface exists, so a target left with only the whole parent window names
/// the real cause behind its composition failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxChildSurfaceAbsence {
    /// The `FERRYX_DISABLE_WAYLAND_SUBSURFACE=1` escape hatch disabled the subsurface, leaving only
    /// the whole parent window.
    WaylandSubsurfaceDisabledByEnv,
    /// The compositor does not advertise `wl_subcompositor` (or refused the subsurface), so no
    /// clipping-isolated child surface can exist.
    WaylandSubcompositorUnavailable,
    /// `XCreateSimpleWindow` on the parent window did not produce an X11 child window.
    X11ChildWindowUnavailable,
    /// The Tauri window/display handle pair cannot host an isolated child surface.
    UnsupportedHandlePair,
}

impl LinuxChildSurfaceAbsence {
    /// The cause to report, and what it takes to get a surface that can sit at the pane.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::WaylandSubsurfaceDisabledByEnv => {
                "FERRYX_DISABLE_WAYLAND_SUBSURFACE=1 disabled the Wayland subsurface, so the native terminal has no surface to place at its pane; unset it to attach a wl_subsurface"
            }
            Self::WaylandSubcompositorUnavailable => {
                "the Wayland compositor does not advertise wl_subcompositor, so the native terminal has no surface to place at its pane"
            }
            Self::X11ChildWindowUnavailable => {
                "no X11 child window could be created on the parent window, so the native terminal has no surface to place at its pane"
            }
            Self::UnsupportedHandlePair => {
                "the Tauri window/display handle pair cannot host an isolated child surface, so the native terminal has no surface to place at its pane"
            }
        }
    }
}

/// The parent window's client-area origin, in logical pixels.
///
/// A child window or `wl_subsurface` is parented to the GTK toplevel, whose origin can sit above
/// the WebView that DOM viewport coordinates are measured from. Adding this offset to a viewport
/// position keeps the surface aligned with the pane; it is the Linux counterpart of the
/// `ScreenToClient` correction the Windows path applies to pointer coordinates
/// (`platform/windows_focus.rs`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientAreaOrigin {
    y: i32,
}

impl ClientAreaOrigin {
    /// Derives the offset from the parent window's frame origin and its client-area origin.
    ///
    /// Both positions are reported in screen coordinates by the same window, so their difference
    /// is the decoration band above the client area. A platform that reports no frame origin
    /// (Wayland exposes no global window coordinates), an absent band, or a nonsensical scale
    /// factor yields no offset rather than shifting the surface upward.
    pub fn from_window_positions(
        frame_origin_y: Option<i32>,
        client_origin_y: Option<i32>,
        scale_factor: f64,
    ) -> Self {
        let (Some(frame_y), Some(client_y)) = (frame_origin_y, client_origin_y) else {
            return Self::default();
        };
        if !scale_factor.is_finite() || scale_factor <= 0.0 {
            return Self::default();
        }
        let band_px = client_y.saturating_sub(frame_y);
        if band_px <= 0 {
            return Self::default();
        }
        Self {
            y: (band_px as f64 / scale_factor).round() as i32,
        }
    }

    /// Applies the offset to a viewport position expressed in DOM coordinates.
    #[inline]
    pub const fn apply(self, dom_y: i32) -> i32 {
        dom_y.saturating_add(self.y)
    }
}

/// Linux native child compositor target.
pub struct LinuxCompositorTarget {
    handle: Arc<NativeChildViewHandle>,
    child: Option<LinuxChild>,
    child_absence: Option<LinuxChildSurfaceAbsence>,
    client_area_origin: ClientAreaOrigin,
    visibility: Mutex<ChildSurfaceVisibility>,
    geometry_latch: GeometryLatch,
    wayland_geometry_latch: GeometryLatch<WaylandSubsurfaceGeometry>,
}

// SAFETY: `LinuxCompositorTarget` contains thread-safe handles.
unsafe impl Send for LinuxCompositorTarget {}
unsafe impl Sync for LinuxCompositorTarget {}

impl LinuxCompositorTarget {
    /// Creates a compositor target for the given Tauri window.
    pub fn new<R: Runtime>(window: &Window<R>) -> Result<Self, NativeTerminalError> {
        let window_handle = window.window_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get window handle: {e}"))
        })?;
        let display_handle = window.display_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get display handle: {e}"))
        })?;

        let window_inner = match window_handle.as_raw() {
            RawWindowHandle::Xlib(h) => LinuxWindowHandleInner::Xlib {
                window: h.window,
                visual_id: h.visual_id,
            },
            RawWindowHandle::Wayland(h) => LinuxWindowHandleInner::Wayland { surface: h.surface },
            // tao's Linux backend produces Xlib or Wayland handles only, so an XCB window cannot
            // reach this arm; a future tao that switches must be converted deliberately instead
            // of degrading into the parent-window path.
            other => {
                return Err(NativeTerminalError::GpuPipelineError(format!(
                    "Unsupported window handle type on Linux: {other:?}"
                )));
            }
        };

        let display_inner = match display_handle.as_raw() {
            RawDisplayHandle::Xlib(d) => LinuxDisplayHandleInner::Xlib {
                display: d.display,
                screen: d.screen,
            },
            RawDisplayHandle::Wayland(d) => LinuxDisplayHandleInner::Wayland { display: d.display },
            other => {
                return Err(NativeTerminalError::GpuPipelineError(format!(
                    "Unsupported display handle type on Linux: {other:?}"
                )));
            }
        };

        let mut child_absence = None;
        let mut client_area_origin = ClientAreaOrigin::default();
        let child = match (&window_inner, &display_inner) {
            (
                LinuxWindowHandleInner::Xlib { window: parent, .. },
                LinuxDisplayHandleInner::Xlib { display, .. },
            ) => {
                let child = display
                    .and_then(|display| X11Child::create(display.as_ptr(), *parent))
                    .map(LinuxChild::X11);
                if child.is_none() {
                    child_absence = Some(LinuxChildSurfaceAbsence::X11ChildWindowUnavailable);
                } else if let Some(display) = display {
                    client_area_origin = x11_client_area_origin(window, display.as_ptr(), *parent);
                }
                child
            }
            (
                LinuxWindowHandleInner::Wayland { surface },
                LinuxDisplayHandleInner::Wayland { display },
            ) => {
                if std::env::var_os("FERRYX_DISABLE_WAYLAND_SUBSURFACE").is_some() {
                    child_absence = Some(LinuxChildSurfaceAbsence::WaylandSubsurfaceDisabledByEnv);
                    None
                } else {
                    match WaylandChild::create(display.as_ptr(), *surface) {
                        Some(child) => Some(LinuxChild::Wayland(child)),
                        None => {
                            child_absence =
                                Some(LinuxChildSurfaceAbsence::WaylandSubcompositorUnavailable);
                            None
                        }
                    }
                }
            }
            _ => {
                child_absence = Some(LinuxChildSurfaceAbsence::UnsupportedHandlePair);
                None
            }
        };

        let window_inner = match (&child, window_inner) {
            (Some(LinuxChild::X11(child)), LinuxWindowHandleInner::Xlib { visual_id, .. }) => {
                LinuxWindowHandleInner::Xlib {
                    window: child.window,
                    visual_id,
                }
            }
            (Some(LinuxChild::Wayland(child)), LinuxWindowHandleInner::Wayland { .. }) => {
                LinuxWindowHandleInner::Wayland {
                    surface: child.surface(),
                }
            }
            (_, window_inner) => window_inner,
        };

        let handle = Arc::new(NativeChildViewHandle {
            window_inner,
            display_inner,
        });

        if let Some(absence) = child_absence {
            // With no isolated child there is no surface that can be placed at the pane, so
            // composition rejects this target. Report the real cause loudly here, because the
            // descriptor must not claim child capabilities to get past that check.
            tracing::error!(
                reason = absence.reason(),
                "Linux native terminal has no isolated child surface to place at its pane"
            );
        }

        Ok(Self {
            handle,
            child,
            child_absence,
            client_area_origin,
            visibility: Mutex::new(ChildSurfaceVisibility::default()),
            geometry_latch: GeometryLatch::default(),
            wayland_geometry_latch: GeometryLatch::default(),
        })
    }

    /// Returns the raw-window-handle target for wgpu surface creation.
    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    /// Reports the capabilities of the surface wgpu actually renders into.
    ///
    /// With an isolated child -- an X11 child window or a Wayland subsurface, both created with an
    /// empty input region -- that child is the surface, so child-surface capabilities are reported.
    /// With no child, the whole parent GTK window is the surface: it is neither clipped to the pane
    /// nor pointer-transparent, so the parent-window descriptor is reported and composition
    /// validation fails. The cause is named through [`Self::child_surface_absence`] and the
    /// construction-time error.
    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        if self.child.is_some() {
            child_surface_descriptor()
        } else {
            parent_window_surface_descriptor()
        }
    }

    /// Names why no isolated child surface exists, for callers reporting the degradation.
    pub fn child_surface_absence(&self) -> Option<LinuxChildSurfaceAbsence> {
        self.child_absence
    }

    pub fn uses_wayland_subsurface(&self) -> bool {
        matches!(self.child, Some(LinuxChild::Wayland(_)))
    }

    pub fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        let Some(child) = self.child.as_ref() else {
            return;
        };
        let Some(bounds) = bounds.as_ref() else {
            return;
        };
        match child {
            LinuxChild::X11(child) => {
                let Some(geometry) = ChildSurfaceGeometry::from_logical_bounds(bounds) else {
                    return;
                };
                // Identical bounds must not reach X11: every frame re-sends the pane rectangle,
                // so without this each frame is an XMoveResizeWindow round trip to no effect.
                if !self.geometry_latch.needs_apply(geometry) {
                    return;
                }
                // The child is parented to the window we were handed, which is not necessarily
                // the client area the DOM viewport coordinates are measured from.
                let y = self.client_area_origin.apply(geometry.y);
                // SAFETY: the child window and display belong to this target and stay valid
                // until drop.
                unsafe {
                    XMoveResizeWindow(
                        child.display,
                        child.window,
                        geometry.x,
                        y,
                        geometry.width,
                        geometry.height,
                    );
                    XRaiseWindow(child.display, child.window);
                    XFlush(child.display);
                }
            }
            LinuxChild::Wayland(child) => {
                let Some(geometry) = WaylandSubsurfaceGeometry::from_logical_bounds(bounds) else {
                    return;
                };
                // Identical geometry must not reach the compositor: a repeated
                // wl_subsurface.set_position plus commit is a per-frame relayout for a
                // subsurface that is already exactly where it belongs.
                if !self.wayland_geometry_latch.needs_apply(geometry) {
                    return;
                }
                child.set_geometry(
                    geometry.position_x,
                    self.client_area_origin.apply(geometry.position_y),
                    geometry.buffer_scale,
                );
            }
        }
    }

    pub fn reveal(&self) {
        let Some(child) = self.child.as_ref() else {
            return;
        };
        let Ok(mut visibility) = self.visibility.lock() else {
            return;
        };
        if !visibility.should_map_on_present() {
            return;
        }
        match child {
            LinuxChild::X11(child) => {
                // SAFETY: the child window and display belong to this target and stay valid
                // until drop.
                unsafe {
                    XMapWindow(child.display, child.window);
                    XRaiseWindow(child.display, child.window);
                    XFlush(child.display);
                }
            }
            // A subsurface has no map call: it becomes visible once a buffer is committed, and
            // wgpu has already attached one by the time `reveal` runs.
            LinuxChild::Wayland(child) => child.commit(),
        }
        visibility.mark_presented();
    }
}

impl Drop for LinuxCompositorTarget {
    fn drop(&mut self) {
        if let Ok(mut visibility) = self.visibility.lock() {
            visibility.mark_detached();
        }
    }
}

/// Resolves the X11 client-area origin for a child parented to `parent`.
///
/// The child window is created inside the window handle tao handed us. A window-manager frame is
/// a direct child of the root window and its origin sits above the client area the WebView draws
/// in, so the band between the frame origin and the client origin belongs to every DOM viewport
/// position. A client window the window manager has reparented into such a frame is itself the
/// client area, where no correction applies.
fn x11_client_area_origin<R: Runtime>(
    window: &Window<R>,
    display: *mut c_void,
    parent: c_ulong,
) -> ClientAreaOrigin {
    if !x11_window_is_root_child(display, parent) {
        return ClientAreaOrigin::default();
    }
    ClientAreaOrigin::from_window_positions(
        window.outer_position().ok().map(|position| position.y),
        window.inner_position().ok().map(|position| position.y),
        window.scale_factor().unwrap_or(1.0),
    )
}

/// Returns true when `window` is a direct child of the root window, which is what a
/// window-manager frame is; a reparented client window sits under its frame instead.
fn x11_window_is_root_child(display: *mut c_void, window: c_ulong) -> bool {
    if display.is_null() || window == 0 {
        return false;
    }
    let mut root: c_ulong = 0;
    let mut parent: c_ulong = 0;
    let mut children: *mut c_ulong = std::ptr::null_mut();
    let mut child_count: c_uint = 0;
    // SAFETY: `display` is the live Xlib connection owned by GTK and `window` came from that
    // connection, so both stay valid for the duration of this call.
    let queried = unsafe {
        XQueryTree(
            display,
            window,
            &mut root,
            &mut parent,
            &mut children,
            &mut child_count,
        )
    };
    if !children.is_null() {
        // SAFETY: XQueryTree returns an Xlib-allocated child array the caller must release.
        unsafe { XFree(children as *mut c_void) };
    }
    queried != 0 && root != 0 && parent == root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_band_becomes_the_client_area_origin_in_logical_pixels() {
        // A 28 physical-pixel decoration band on a 2x output is 14 logical pixels.
        let origin = ClientAreaOrigin::from_window_positions(Some(100), Some(128), 2.0);
        assert_eq!(origin.apply(0), 14);
    }

    #[test]
    fn client_origin_at_or_above_the_frame_origin_adds_no_offset() {
        let same_origin = ClientAreaOrigin::from_window_positions(Some(100), Some(100), 1.0);
        assert_eq!(same_origin.apply(40), 40);
        let above_frame = ClientAreaOrigin::from_window_positions(Some(100), Some(96), 1.0);
        assert_eq!(above_frame.apply(40), 40);
    }

    #[test]
    fn platforms_without_window_positions_keep_dom_coordinates() {
        // Wayland exposes no global window origin, so the toplevel surface origin is the client
        // origin and DOM coordinates pass through unchanged.
        let wayland = ClientAreaOrigin::from_window_positions(None, None, 1.0);
        assert_eq!(wayland.apply(37), 37);
        let nonsensical_scale = ClientAreaOrigin::from_window_positions(Some(0), Some(28), 0.0);
        assert_eq!(nonsensical_scale.apply(37), 37);
    }

    #[test]
    fn offset_application_saturates_instead_of_wrapping() {
        let origin = ClientAreaOrigin::from_window_positions(Some(0), Some(1), 1.0);
        assert_eq!(origin.apply(i32::MAX), i32::MAX);
    }

    #[test]
    fn parent_window_surface_descriptor_claims_no_child_surface_capabilities() {
        let descriptor = parent_window_surface_descriptor();
        assert_eq!(
            descriptor.target_kind,
            CompositorTargetKind::RootWebviewWindow,
            "with no isolated child the only surface is the whole parent window"
        );
        assert!(
            !descriptor.layer_backed,
            "the whole parent window is not layer-backed, so the no-child descriptor must not claim it"
        );
        assert!(
            !descriptor.pointer_transparent,
            "the whole parent window is not pointer-transparent, so the no-child descriptor must not claim it"
        );
        let err = descriptor
            .validate_desktop_composition()
            .expect_err("a target with no surface to place at the pane must fail composition loudly");
        assert!(
            matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("Root WebviewWindow")),
            "Expected the whole-window rejection, got: {err:?}"
        );
    }

    #[test]
    fn child_surface_descriptor_reports_the_isolated_child() {
        let descriptor = child_surface_descriptor();
        assert_eq!(descriptor.target_kind, CompositorTargetKind::LinuxChildWindow);
        assert!(descriptor.layer_backed);
        assert!(descriptor.pointer_transparent);
        assert!(descriptor.validate_desktop_composition().is_ok());
    }
}
