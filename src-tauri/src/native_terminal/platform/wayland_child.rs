//! A real `wl_subsurface` child for the native terminal, bound directly to `libwayland-client`.
//!
//! # Why raw FFI and a private event queue
//!
//! The `wl_display` belongs to GTK, which is already dispatching its own events on it. Binding
//! globals on GTK's default queue would let us steal events GTK needs, so every proxy created
//! here is moved onto a private queue via `wl_proxy_set_queue` and only ever dispatched with
//! `wl_display_roundtrip_queue`. This is the same isolation strategy media players use to add
//! subsurfaces to a toolkit-owned window.
//!
//! # Safety invariants
//!
//! 1. Every proxy is destroyed exactly once, in reverse creation order, in `Drop`.
//! 2. The registry listener only runs during the bounded roundtrip inside `bind_globals`, so the
//!    `&mut Globals` it writes through cannot outlive that borrow.
//! 3. The child surface is created with an empty input region, so it can never take pointer
//!    events away from the WebView even while it is mapped.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::ptr::NonNull;

#[repr(C)]
struct WlMessage {
    name: *const c_char,
    signature: *const c_char,
    types: *const *const WlInterface,
}

#[repr(C)]
struct WlInterface {
    name: *const c_char,
    version: c_int,
    method_count: c_int,
    methods: *const WlMessage,
    event_count: c_int,
    events: *const WlMessage,
}

#[repr(C)]
struct WlRegistryListener {
    global: unsafe extern "C" fn(*mut c_void, *mut c_void, c_uint, *const c_char, c_uint),
    global_remove: unsafe extern "C" fn(*mut c_void, *mut c_void, c_uint),
}

#[link(name = "wayland-client")]
unsafe extern "C" {
    static wl_compositor_interface: WlInterface;
    static wl_subcompositor_interface: WlInterface;
    static wl_registry_interface: WlInterface;
    static wl_surface_interface: WlInterface;
    static wl_subsurface_interface: WlInterface;
    static wl_region_interface: WlInterface;

    fn wl_display_create_queue(display: *mut c_void) -> *mut c_void;
    fn wl_event_queue_destroy(queue: *mut c_void);
    fn wl_display_roundtrip_queue(display: *mut c_void, queue: *mut c_void) -> c_int;
    fn wl_display_flush(display: *mut c_void) -> c_int;

    fn wl_proxy_marshal_flags(
        proxy: *mut c_void,
        opcode: u32,
        interface: *const WlInterface,
        version: u32,
        flags: u32,
        ...
    ) -> *mut c_void;
    fn wl_proxy_add_listener(
        proxy: *mut c_void,
        implementation: *mut c_void,
        data: *mut c_void,
    ) -> c_int;
    fn wl_proxy_set_queue(proxy: *mut c_void, queue: *mut c_void);
    fn wl_proxy_get_version(proxy: *mut c_void) -> u32;
    fn wl_proxy_destroy(proxy: *mut c_void);
}

const WL_DISPLAY_GET_REGISTRY: u32 = 1;
const WL_REGISTRY_BIND: u32 = 0;
const WL_COMPOSITOR_CREATE_SURFACE: u32 = 0;
const WL_COMPOSITOR_CREATE_REGION: u32 = 1;
const WL_SURFACE_DESTROY: u32 = 0;
const WL_SURFACE_SET_INPUT_REGION: u32 = 5;
const WL_SURFACE_COMMIT: u32 = 6;
const WL_SURFACE_SET_BUFFER_SCALE: u32 = 8;
const WL_REGION_DESTROY: u32 = 0;
const WL_SUBCOMPOSITOR_GET_SUBSURFACE: u32 = 1;
const WL_SUBSURFACE_DESTROY: u32 = 0;
const WL_SUBSURFACE_SET_POSITION: u32 = 1;
const WL_SUBSURFACE_SET_DESYNC: u32 = 5;

const MARSHAL_FLAG_DESTROY: u32 = 1;

const COMPOSITOR_VERSION: u32 = 4;
const SUBCOMPOSITOR_VERSION: u32 = 1;

#[derive(Default)]
struct Globals {
    compositor: *mut c_void,
    subcompositor: *mut c_void,
    queue: *mut c_void,
}

unsafe extern "C" fn handle_global(
    data: *mut c_void,
    registry: *mut c_void,
    name: c_uint,
    interface: *const c_char,
    version: c_uint,
) {
    // SAFETY: `data` is the `&mut Globals` passed to `wl_proxy_add_listener`, alive for the
    // duration of the bounded roundtrip that drives this callback.
    let globals = unsafe { &mut *(data as *mut Globals) };
    // SAFETY: the compositor always passes a NUL-terminated interface name.
    let Ok(name_str) = unsafe { CStr::from_ptr(interface) }.to_str() else {
        return;
    };

    let (slot, iface, wanted) = match name_str {
        "wl_compositor" => (
            &mut globals.compositor,
            &raw const wl_compositor_interface,
            COMPOSITOR_VERSION,
        ),
        "wl_subcompositor" => (
            &mut globals.subcompositor,
            &raw const wl_subcompositor_interface,
            SUBCOMPOSITOR_VERSION,
        ),
        _ => return,
    };
    if !slot.is_null() {
        return;
    }

    let bind_version = wanted.min(version);
    // SAFETY: binding a global advertised by this registry with a version it announced.
    let proxy = unsafe {
        wl_proxy_marshal_flags(
            registry,
            WL_REGISTRY_BIND,
            iface,
            bind_version,
            0,
            name,
            (*iface).name,
            bind_version,
            std::ptr::null_mut::<c_void>(),
        )
    };
    if proxy.is_null() {
        return;
    }
    // SAFETY: keeping every proxy on our private queue so GTK's dispatch is untouched.
    unsafe { wl_proxy_set_queue(proxy, globals.queue) };
    *slot = proxy;
}

unsafe extern "C" fn handle_global_remove(
    _data: *mut c_void,
    _registry: *mut c_void,
    _name: c_uint,
) {
}

static REGISTRY_LISTENER: WlRegistryListener = WlRegistryListener {
    global: handle_global,
    global_remove: handle_global_remove,
};

/// `wl_display` is the one object that is a proxy for marshalling purposes but must be treated
/// as version 1: `wl_proxy_get_version` is only valid for proxies created from a registry bind,
/// and the generated `wl_display_get_registry` passes the display's own version, which is 1.
const WL_DISPLAY_VERSION: u32 = 1;

fn bind_globals(display: *mut c_void, queue: *mut c_void) -> Option<(*mut c_void, *mut c_void)> {
    // SAFETY: `display` is GTK's live connection; the registry proxy is parked on our queue
    // before any roundtrip so its events never reach GTK's dispatch.
    let registry = unsafe {
        wl_proxy_marshal_flags(
            display,
            WL_DISPLAY_GET_REGISTRY,
            &raw const wl_registry_interface,
            WL_DISPLAY_VERSION,
            0,
            std::ptr::null_mut::<c_void>(),
        )
    };
    if registry.is_null() {
        return None;
    }
    // SAFETY: `registry` was just created and is not yet shared.
    unsafe { wl_proxy_set_queue(registry, queue) };

    let mut globals = Globals {
        queue,
        ..Globals::default()
    };
    // SAFETY: the listener and `globals` both outlive the roundtrip below.
    let added = unsafe {
        wl_proxy_add_listener(
            registry,
            &REGISTRY_LISTENER as *const WlRegistryListener as *mut c_void,
            &mut globals as *mut Globals as *mut c_void,
        )
    };
    if added != 0 {
        // SAFETY: destroying the registry proxy we created and never handed out.
        unsafe { wl_proxy_destroy(registry) };
        return None;
    }

    // SAFETY: dispatching only our private queue, which the roundtrip drains completely.
    let roundtrip = unsafe { wl_display_roundtrip_queue(display, queue) };
    // SAFETY: the registry is no longer needed once the globals are bound.
    unsafe { wl_proxy_destroy(registry) };

    if roundtrip < 0 || globals.compositor.is_null() || globals.subcompositor.is_null() {
        // SAFETY: releasing whichever globals were bound before the failure.
        unsafe {
            if !globals.compositor.is_null() {
                wl_proxy_destroy(globals.compositor);
            }
            if !globals.subcompositor.is_null() {
                wl_proxy_destroy(globals.subcompositor);
            }
        }
        return None;
    }

    Some((globals.compositor, globals.subcompositor))
}

pub struct WaylandChild {
    display: *mut c_void,
    queue: *mut c_void,
    compositor: *mut c_void,
    subcompositor: *mut c_void,
    surface: NonNull<c_void>,
    subsurface: *mut c_void,
}

impl WaylandChild {
    pub fn create(display: *mut c_void, parent_surface: NonNull<c_void>) -> Option<Self> {
        if display.is_null() {
            return None;
        }
        // SAFETY: `display` is a live `wl_display` owned by GTK.
        let queue = unsafe { wl_display_create_queue(display) };
        if queue.is_null() {
            return None;
        }

        let Some((compositor, subcompositor)) = bind_globals(display, queue) else {
            // SAFETY: the queue was created above and never shared.
            unsafe { wl_event_queue_destroy(queue) };
            return None;
        };

        // SAFETY: `compositor` is a bound `wl_compositor` on our private queue. The interface
        // argument names the type of the object being CREATED, so it must be `wl_surface`.
        let surface = unsafe {
            wl_proxy_marshal_flags(
                compositor,
                WL_COMPOSITOR_CREATE_SURFACE,
                &raw const wl_surface_interface,
                wl_proxy_get_version(compositor),
                0,
                std::ptr::null_mut::<c_void>(),
            )
        };
        let Some(surface) = NonNull::new(surface) else {
            // SAFETY: releasing the globals and queue on the failure path.
            unsafe {
                wl_proxy_destroy(subcompositor);
                wl_proxy_destroy(compositor);
                wl_event_queue_destroy(queue);
            }
            return None;
        };
        // SAFETY: the child surface is ours and not yet shared.
        unsafe { wl_proxy_set_queue(surface.as_ptr(), queue) };

        // SAFETY: an empty region makes the child unable to receive pointer input, which keeps
        // click and hover routing entirely inside the WebView.
        unsafe {
            let region = wl_proxy_marshal_flags(
                compositor,
                WL_COMPOSITOR_CREATE_REGION,
                &raw const wl_region_interface,
                wl_proxy_get_version(compositor),
                0,
                std::ptr::null_mut::<c_void>(),
            );
            if !region.is_null() {
                wl_proxy_set_queue(region, queue);
                wl_proxy_marshal_flags(
                    surface.as_ptr(),
                    WL_SURFACE_SET_INPUT_REGION,
                    std::ptr::null(),
                    wl_proxy_get_version(surface.as_ptr()),
                    0,
                    region,
                );
                wl_proxy_marshal_flags(
                    region,
                    WL_REGION_DESTROY,
                    std::ptr::null(),
                    wl_proxy_get_version(region),
                    MARSHAL_FLAG_DESTROY,
                );
            }
        }

        // SAFETY: creating the subsurface that parents our child to the Tauri window surface.
        let subsurface = unsafe {
            wl_proxy_marshal_flags(
                subcompositor,
                WL_SUBCOMPOSITOR_GET_SUBSURFACE,
                &raw const wl_subsurface_interface,
                wl_proxy_get_version(subcompositor),
                0,
                std::ptr::null_mut::<c_void>(),
                surface.as_ptr(),
                parent_surface.as_ptr(),
            )
        };
        if subsurface.is_null() {
            // SAFETY: unwinding every object created so far, newest first.
            unsafe {
                wl_proxy_marshal_flags(
                    surface.as_ptr(),
                    WL_SURFACE_DESTROY,
                    std::ptr::null(),
                    wl_proxy_get_version(surface.as_ptr()),
                    MARSHAL_FLAG_DESTROY,
                );
                wl_proxy_destroy(subcompositor);
                wl_proxy_destroy(compositor);
                wl_event_queue_destroy(queue);
            }
            return None;
        }

        // SAFETY: desync mode lets our commits take effect without waiting for a parent commit,
        // which GTK issues on its own schedule and we must not depend on.
        unsafe {
            wl_proxy_set_queue(subsurface, queue);
            wl_proxy_marshal_flags(
                subsurface,
                WL_SUBSURFACE_SET_DESYNC,
                std::ptr::null(),
                wl_proxy_get_version(subsurface),
                0,
            );
            wl_display_flush(display);
        }

        Some(Self {
            display,
            queue,
            compositor,
            subcompositor,
            surface,
            subsurface,
        })
    }

    pub fn surface(&self) -> NonNull<c_void> {
        self.surface
    }

    pub fn set_geometry(&self, position_x: i32, position_y: i32, buffer_scale: i32) {
        // SAFETY: both proxies belong to this child and stay valid until `Drop`.
        unsafe {
            wl_proxy_marshal_flags(
                self.subsurface,
                WL_SUBSURFACE_SET_POSITION,
                std::ptr::null(),
                wl_proxy_get_version(self.subsurface),
                0,
                position_x,
                position_y,
            );
            wl_proxy_marshal_flags(
                self.surface.as_ptr(),
                WL_SURFACE_SET_BUFFER_SCALE,
                std::ptr::null(),
                wl_proxy_get_version(self.surface.as_ptr()),
                0,
                buffer_scale,
            );
            wl_display_flush(self.display);
        }
    }

    pub fn commit(&self) {
        // SAFETY: committing our own surface; wgpu attaches the buffer before this runs.
        unsafe {
            wl_proxy_marshal_flags(
                self.surface.as_ptr(),
                WL_SURFACE_COMMIT,
                std::ptr::null(),
                wl_proxy_get_version(self.surface.as_ptr()),
                0,
            );
            wl_display_flush(self.display);
        }
    }
}

impl Drop for WaylandChild {
    fn drop(&mut self) {
        // SAFETY: every proxy was created by `create` and is destroyed exactly once here, in
        // reverse creation order, before the private queue that owns their events.
        unsafe {
            wl_proxy_marshal_flags(
                self.subsurface,
                WL_SUBSURFACE_DESTROY,
                std::ptr::null(),
                wl_proxy_get_version(self.subsurface),
                MARSHAL_FLAG_DESTROY,
            );
            wl_proxy_marshal_flags(
                self.surface.as_ptr(),
                WL_SURFACE_DESTROY,
                std::ptr::null(),
                wl_proxy_get_version(self.surface.as_ptr()),
                MARSHAL_FLAG_DESTROY,
            );
            wl_proxy_destroy(self.subcompositor);
            wl_proxy_destroy(self.compositor);
            wl_display_flush(self.display);
            wl_event_queue_destroy(self.queue);
        }
    }
}
