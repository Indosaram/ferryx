//! Process-wide inhibition exists only while streaming sessions hold a guard.
use std::sync::Mutex;

#[derive(Default)]
struct State {
    sessions: usize,
    native: Option<Native>,
}

static STATE: Mutex<State> = Mutex::new(State {
    sessions: 0,
    native: None,
});
static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) struct StreamingGuard;

impl StreamingGuard {
    pub(crate) fn acquire() -> Self {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.sessions == 0 {
            match Native::acquire() {
                Ok(native) => state.native = Some(native),
                Err(error) => {
                    if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        tracing::warn!(%error, "session sleep inhibition unavailable");
                    }
                }
            }
        }
        state.sessions += 1;
        Self
    }
}

impl Drop for StreamingGuard {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.sessions -= 1;
        if state.sessions == 0 {
            state.native = None;
        }
    }
}

#[cfg(target_os = "macos")]
struct Native(u32);

#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        kind: core_foundation::string::CFStringRef,
        level: u32,
        name: core_foundation::string::CFStringRef,
        id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

#[cfg(target_os = "macos")]
impl Native {
    fn acquire() -> std::io::Result<Self> {
        use core_foundation::{base::TCFType, string::CFString};
        let kind = CFString::new("PreventUserIdleDisplaySleep");
        let name = CFString::new("MahoRD active remote session");
        let mut id = 0;
        // SAFETY: CF strings and initialized output remain live for the call;
        // a successful assertion ID is released exactly once by Native::drop.
        let status = unsafe {
            IOPMAssertionCreateWithName(
                kind.as_concrete_TypeRef(),
                255,
                name.as_concrete_TypeRef(),
                &mut id,
            )
        };
        if status != 0 {
            return Err(std::io::Error::other(format!("IOPM assertion: {status}")));
        }
        Ok(Self(id))
    }
}

#[cfg(target_os = "macos")]
impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: self owns the successfully acquired assertion ID.
        let status = unsafe { IOPMAssertionRelease(self.0) };
        if status != 0 {
            tracing::warn!(status, "IOPM assertion release failed");
        }
    }
}

#[cfg(target_os = "linux")]
struct Native(std::sync::mpsc::Sender<()>);

#[cfg(target_os = "linux")]
impl Native {
    fn acquire() -> std::io::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("maho-idle-inhibit".into())
            .spawn(move || {
                let result = linux::hold(rx);
                if let Err(error) = result {
                    if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        tracing::warn!(%error, "session sleep inhibition unavailable");
                    }
                }
            })?;
        Ok(Self(tx))
    }
}

#[cfg(target_os = "linux")]
impl Drop for Native {
    fn drop(&mut self) {
        // Sending (or disconnecting) wakes the owner; neither acquisition nor
        // protocol cleanup runs on a streaming session thread.
        if self.0.send(()).is_err() {
            tracing::debug!("inhibitor owner already exited");
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        io,
        os::fd::AsFd,
        time::{Duration, Instant},
    };
    use wayland_client::{
        delegate_noop,
        protocol::{
            wl_buffer, wl_callback, wl_compositor, wl_region, wl_seat, wl_shm, wl_shm_pool,
            wl_surface,
        },
        Connection, Dispatch, QueueHandle,
    };
    use wayland_protocols::wp::idle_inhibit::zv1::client::{
        zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1,
    };
    use wayland_protocols_wlr::layer_shell::v1::client::{
        zwlr_layer_shell_v1, zwlr_layer_surface_v1,
    };

    pub(super) fn hold(rx: std::sync::mpsc::Receiver<()>) -> io::Result<()> {
        // Keep the bus connection alive: one-shot gdbus/busctl calls lose their
        // cookies when the caller disconnects. ScreenSaver is optional on wlroots.
        if let Ok(bus) = Bus::acquire() {
            tracing::info!("session idle inhibition acquired via ScreenSaver");
            let _ = rx.recv();
            drop(bus);
            return Ok(());
        }
        // Hyprland's portal GTK backend cannot inhibit compositor idle. Use a
        // mapped 1x1 transparent overlay instead: an unconfigured desktop surface
        // is not visible and therefore does not inhibit. No input, focus, reserved
        // space, window rules, or logind/polkit authorization are involved.
        let mut wayland = Wayland::acquire()?;
        tracing::info!("session idle inhibition acquired via Wayland");
        let _ = rx.recv();
        wayland.release()?;
        Ok(())
    }

    struct Bus {
        _library: libloading::Library,
        bus: *mut std::ffi::c_void,
        cookie: Option<u32>,
        call: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const std::ffi::c_char,
            *const std::ffi::c_char,
            *const std::ffi::c_char,
            *const std::ffi::c_char,
            *mut std::ffi::c_void,
            *mut *mut std::ffi::c_void,
            *const std::ffi::c_char,
            ...
        ) -> i32,
        close: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
        unref: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
    }

    impl Bus {
        fn acquire() -> io::Result<Self> {
            // SAFETY: libsystemd's documented sd-bus ABI uses opaque pointers;
            // all symbols have their C signatures, strings are static C strings,
            // and this owner and all replies are used only on this worker thread.
            unsafe {
                let library =
                    libloading::Library::new("libsystemd.so.0").map_err(io::Error::other)?;
                let open = library
                    .get::<unsafe extern "C" fn(*mut *mut std::ffi::c_void) -> i32>(
                        b"sd_bus_open_user\0",
                    )
                    .map_err(io::Error::other)?;
                let call = *library
                    .get(b"sd_bus_call_method\0")
                    .map_err(io::Error::other)?;
                let close = *library
                    .get(b"sd_bus_flush_close_unref\0")
                    .map_err(io::Error::other)?;
                let unref = *library
                    .get(b"sd_bus_message_unref\0")
                    .map_err(io::Error::other)?;
                let read =
                    *library
                        .get::<unsafe extern "C" fn(
                            *mut std::ffi::c_void,
                            *const std::ffi::c_char,
                            ...
                        ) -> i32>(b"sd_bus_message_read\0")
                        .map_err(io::Error::other)?;
                let timeout = *library
                    .get::<unsafe extern "C" fn(*mut std::ffi::c_void, u64) -> i32>(
                        b"sd_bus_set_method_call_timeout\0",
                    )
                    .map_err(io::Error::other)?;
                let mut bus = std::ptr::null_mut();
                let status = open(&mut bus);
                if status < 0 {
                    return Err(io::Error::from_raw_os_error(-status));
                }
                let mut owner = Self {
                    bus,
                    cookie: None,
                    call,
                    close,
                    unref,
                    _library: library,
                };
                let status = timeout(owner.bus, 2_000_000);
                if status < 0 {
                    return Err(io::Error::from_raw_os_error(-status));
                }
                let mut reply = std::ptr::null_mut();
                let status = (owner.call)(
                    owner.bus,
                    c"org.freedesktop.ScreenSaver".as_ptr(),
                    c"/org/freedesktop/ScreenSaver".as_ptr(),
                    c"org.freedesktop.ScreenSaver".as_ptr(),
                    c"Inhibit".as_ptr(),
                    std::ptr::null_mut(),
                    &mut reply,
                    c"ss".as_ptr(),
                    c"MahoRD".as_ptr(),
                    c"Active remote desktop session".as_ptr(),
                );
                if status < 0 {
                    return Err(io::Error::from_raw_os_error(-status));
                }
                let mut cookie = 0u32;
                let status = read(reply, c"u".as_ptr(), &mut cookie);
                (owner.unref)(reply);
                if status < 0 {
                    return Err(io::Error::from_raw_os_error(-status));
                }
                owner.cookie = Some(cookie);
                Ok(owner)
            }
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            // SAFETY: bus is exclusively owned and the library remains loaded.
            // Disconnect also releases inhibition if UnInhibit fails.
            unsafe {
                if let Some(cookie) = self.cookie {
                    let status = (self.call)(
                        self.bus,
                        c"org.freedesktop.ScreenSaver".as_ptr(),
                        c"/org/freedesktop/ScreenSaver".as_ptr(),
                        c"org.freedesktop.ScreenSaver".as_ptr(),
                        c"UnInhibit".as_ptr(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        c"u".as_ptr(),
                        cookie,
                    );
                    if status < 0 {
                        tracing::debug!(status, "ScreenSaver release failed; disconnecting");
                    }
                }
                (self.close)(self.bus);
            }
        }
    }

    #[derive(Default)]
    struct Events {
        configured: bool,
        synced: bool,
        closed: bool,
        globals: Vec<(u32, String)>,
        #[cfg(test)]
        idle: bool,
    }
    struct Wayland {
        queue: wayland_client::EventQueue<Events>,
        state: Events,
        surface: wl_surface::WlSurface,
        layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        inhibitor: zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
        _buffer: wl_buffer::WlBuffer,
        _file: std::fs::File,
        conn: Connection,
        #[cfg(test)]
        registry: wayland_client::protocol::wl_registry::WlRegistry,
    }

    impl Wayland {
        fn acquire() -> io::Result<Self> {
            use std::io::Write;
            let conn = Connection::connect_to_env().map_err(io::Error::other)?;
            let mut queue = conn.new_event_queue();
            let qh = queue.handle();
            let registry = conn.display().get_registry(&qh, ());
            let mut state = Events::default();
            sync(&conn, &mut queue, &mut state)?;
            let compositor: wl_compositor::WlCompositor =
                registry.bind(global(&state, "wl_compositor")?, 1, &qh, ());
            let shm: wl_shm::WlShm = registry.bind(global(&state, "wl_shm")?, 1, &qh, ());
            let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 =
                registry.bind(global(&state, "zwlr_layer_shell_v1")?, 1, &qh, ());
            let manager: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
                registry.bind(global(&state, "zwp_idle_inhibit_manager_v1")?, 1, &qh, ());
            let surface = compositor.create_surface(&qh, ());
            let region = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
            let layer = shell.get_layer_surface(
                &surface,
                None,
                zwlr_layer_shell_v1::Layer::Overlay,
                "maho-session-inhibit".into(),
                &qh,
                (),
            );
            layer.set_size(1, 1);
            layer.set_anchor(
                zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left,
            );
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
            surface.commit();
            sync(&conn, &mut queue, &mut state)?;
            if !state.configured || state.closed {
                return Err(io::Error::other("inhibitor surface not configured"));
            }
            let mut file = tempfile::tempfile()?;
            file.write_all(&[0; 4])?;
            let pool = shm.create_pool(file.as_fd(), 4, &qh, ());
            let buffer = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ());
            pool.destroy();
            surface.attach(Some(&buffer), 0, 0);
            surface.commit();
            sync(&conn, &mut queue, &mut state)?;
            let inhibitor = manager.create_inhibitor(&surface, &qh, ());
            sync(&conn, &mut queue, &mut state)?;
            Ok(Self {
                queue,
                state,
                surface,
                layer,
                inhibitor,
                _buffer: buffer,
                _file: file,
                conn,
                #[cfg(test)]
                registry,
            })
        }

        fn release(&mut self) -> io::Result<()> {
            self.inhibitor.destroy();
            self.layer.destroy();
            self.surface.destroy();
            sync(&self.conn, &mut self.queue, &mut self.state)
        }
    }

    fn global(state: &Events, interface: &str) -> io::Result<u32> {
        state
            .globals
            .iter()
            .find(|(_, name)| name == interface)
            .map(|(id, _)| *id)
            .ok_or_else(|| io::Error::other(format!("missing Wayland global {interface}")))
    }

    fn sync(
        conn: &Connection,
        queue: &mut wayland_client::EventQueue<Events>,
        state: &mut Events,
    ) -> io::Result<()> {
        state.synced = false;
        conn.display().sync(&queue.handle(), ());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !state.synced {
            queue.dispatch_pending(state).map_err(io::Error::other)?;
            if state.synced {
                break;
            }
            conn.flush().map_err(io::Error::other)?;
            if let Some(reader) = queue.prepare_read() {
                let fd = reader.connection_fd();
                let mut fds = [rustix::event::PollFd::new(
                    &fd,
                    rustix::event::PollFlags::IN,
                )];
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Wayland inhibitor sync",
                    ));
                }
                let timeout =
                    rustix::event::Timespec::try_from(remaining).map_err(io::Error::other)?;
                rustix::event::poll(&mut fds, Some(&timeout)).map_err(io::Error::other)?;
                if fds[0].revents().is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Wayland inhibitor sync",
                    ));
                }
                reader.read().map_err(io::Error::other)?;
            }
        }
        Ok(())
    }

    impl Dispatch<wl_callback::WlCallback, ()> for Events {
        fn event(
            state: &mut Self,
            _: &wl_callback::WlCallback,
            _: wl_callback::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            state.synced = true;
        }
    }
    impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for Events {
        fn event(
            state: &mut Self,
            layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
            event: zwlr_layer_surface_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                    layer.ack_configure(serial);
                    state.configured = true;
                }
                zwlr_layer_surface_v1::Event::Closed => state.closed = true,
                _ => {}
            }
        }
    }
    delegate_noop!(Events: ignore wl_compositor::WlCompositor);
    delegate_noop!(Events: ignore wl_surface::WlSurface);
    delegate_noop!(Events: ignore wl_region::WlRegion);
    delegate_noop!(Events: ignore wl_shm::WlShm);
    delegate_noop!(Events: ignore wl_shm_pool::WlShmPool);
    delegate_noop!(Events: ignore wl_buffer::WlBuffer);
    delegate_noop!(Events: ignore wl_seat::WlSeat);
    delegate_noop!(Events: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
    delegate_noop!(Events: ignore zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1);
    delegate_noop!(Events: ignore zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1);
    impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()> for Events {
        fn event(
            state: &mut Self,
            _: &wayland_client::protocol::wl_registry::WlRegistry,
            event: wayland_client::protocol::wl_registry::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wayland_client::protocol::wl_registry::Event::Global {
                name, interface, ..
            } = event
            {
                state.globals.push((name, interface));
            }
        }
    }

    #[cfg(test)]
    use wayland_protocols::ext::idle_notify::v1::client::{
        ext_idle_notification_v1, ext_idle_notifier_v1,
    };
    #[cfg(test)]
    delegate_noop!(Events: ignore ext_idle_notifier_v1::ExtIdleNotifierV1);
    #[cfg(test)]
    impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for Events {
        fn event(
            state: &mut Self,
            _: &ext_idle_notification_v1::ExtIdleNotificationV1,
            event: ext_idle_notification_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                ext_idle_notification_v1::Event::Idled => state.idle = true,
                ext_idle_notification_v1::Event::Resumed => state.idle = false,
                _ => {}
            }
        }
    }

    #[test]
    #[ignore = "requires a live Wayland user session"]
    fn live_inhibitor_acquires_and_releases() {
        match Bus::acquire() {
            Ok(bus) => {
                println!("ScreenSaver user-bus inhibitor acquired and released");
                drop(bus);
            }
            Err(error) => println!("ScreenSaver unavailable: {error}; probing Wayland fallback"),
        }
        let mut wayland = Wayland::acquire().expect("polkit-free Wayland acquisition");
        let qh = wayland.queue.handle();
        let seat: wl_seat::WlSeat =
            wayland
                .registry
                .bind(global(&wayland.state, "wl_seat").unwrap(), 1, &qh, ());
        let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = wayland.registry.bind(
            global(&wayland.state, "ext_idle_notifier_v1").unwrap(),
            1,
            &qh,
            (),
        );
        // Zero timeout asks the compositor for its current inhibition state;
        // synchronization observes events, not a timing-based absence of events.
        let notification = notifier.get_idle_notification(0, &seat, &qh, ());
        sync(&wayland.conn, &mut wayland.queue, &mut wayland.state).unwrap();
        assert!(!wayland.state.idle, "compositor must honor the inhibitor");
        println!("ACQUIRED Wayland idle-inhibit; zero-timeout notification idle=false; no polkit");
        wayland.release().unwrap();
        assert!(
            wayland.state.idle,
            "release must restore compositor idle notifications"
        );
        println!("RELEASED Wayland idle-inhibit; zero-timeout notification idle=true");
        notification.destroy();
        sync(&wayland.conn, &mut wayland.queue, &mut wayland.state).unwrap();
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
struct Native;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Native {
    fn acquire() -> std::io::Result<Self> {
        Ok(Self)
    }
}
