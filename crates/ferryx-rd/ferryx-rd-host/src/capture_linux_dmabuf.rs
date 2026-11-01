use std::{
    fs::File,
    os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};
use thiserror::Error;
use wayland_client::{
    delegate_noop,
    protocol::{wl_buffer, wl_callback, wl_output, wl_registry},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

pub const GBM_BO_USE_RENDERING: u32 = 0x4;
pub const GBM_BO_USE_LINEAR: u32 = 0x10;
pub const GBM_FORMAT_ARGB8888: u32 = 0x34325241;

pub const RING_CAPACITY: usize = 3;

#[derive(Debug, Error)]
pub enum DmabufCaptureError {
    #[error("capture cancelled")]
    Cancelled,
    #[error("capture initialization deadline expired")]
    Timeout,
    #[error("failed to connect to Wayland compositor: {0}")]
    Connect(#[from] wayland_client::ConnectError),
    #[error("Wayland dispatch failed: {0}")]
    Dispatch(#[from] wayland_client::DispatchError),
    #[error("compositor missing required interface: {0}")]
    MissingInterface(&'static str),
    #[error("Wayland output {0:?} was not found")]
    OutputNotFound(Option<String>),
    #[error("screencopy did not offer dmabuf format")]
    NoDmabufOffer,
    #[error("failed to open DRM render node {0}: {1}")]
    DrmOpen(String, std::io::Error),
    #[error("GBM library error: {0}")]
    Gbm(String),
    #[error("screencopy capture failed")]
    CaptureFailed,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

struct GbmApi {
    _lib: Library,
    create_device: unsafe extern "C" fn(fd: RawFd) -> *mut std::ffi::c_void,
    destroy_device: unsafe extern "C" fn(device: *mut std::ffi::c_void),
    bo_create: unsafe extern "C" fn(
        device: *mut std::ffi::c_void,
        width: u32,
        height: u32,
        format: u32,
        flags: u32,
    ) -> *mut std::ffi::c_void,
    bo_destroy: unsafe extern "C" fn(bo: *mut std::ffi::c_void),
    bo_get_fd: unsafe extern "C" fn(bo: *mut std::ffi::c_void) -> RawFd,
    bo_get_stride: unsafe extern "C" fn(bo: *mut std::ffi::c_void) -> u32,
    bo_get_offset: unsafe extern "C" fn(bo: *mut std::ffi::c_void, plane: usize) -> u32,
    bo_get_modifier: unsafe extern "C" fn(bo: *mut std::ffi::c_void) -> u64,
}

impl GbmApi {
    fn load() -> Result<Self, DmabufCaptureError> {
        // SAFETY: dlopening libgbm dynamically resolves standard C functions with matching signatures.
        unsafe {
            let lib = Library::new("libgbm.so.1")
                .or_else(|_| Library::new("libgbm.so"))
                .map_err(|e| DmabufCaptureError::Gbm(format!("failed to dlopen libgbm: {e}")))?;

            let create_device: Symbol<unsafe extern "C" fn(RawFd) -> *mut std::ffi::c_void> = lib
                .get(b"gbm_create_device\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_create_device: {e}")))?;
            let destroy_device: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)> = lib
                .get(b"gbm_device_destroy\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_device_destroy: {e}")))?;
            let bo_create: Symbol<
                unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    u32,
                    u32,
                    u32,
                    u32,
                ) -> *mut std::ffi::c_void,
            > = lib
                .get(b"gbm_bo_create\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_bo_create: {e}")))?;
            let bo_destroy: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)> = lib
                .get(b"gbm_bo_destroy\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_bo_destroy: {e}")))?;
            let bo_get_fd: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void) -> RawFd> = lib
                .get(b"gbm_bo_get_fd\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_bo_get_fd: {e}")))?;
            let bo_get_stride: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void) -> u32> = lib
                .get(b"gbm_bo_get_stride\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_bo_get_stride: {e}")))?;
            let bo_get_offset: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, usize) -> u32> =
                lib.get(b"gbm_bo_get_offset\0")
                    .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_bo_get_offset: {e}")))?;
            let bo_get_modifier: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void) -> u64> = lib
                .get(b"gbm_bo_get_modifier\0")
                .map_err(|e| DmabufCaptureError::Gbm(format!("gbm_bo_get_modifier: {e}")))?;

            Ok(Self {
                create_device: *create_device,
                destroy_device: *destroy_device,
                bo_create: *bo_create,
                bo_destroy: *bo_destroy,
                bo_get_fd: *bo_get_fd,
                bo_get_stride: *bo_get_stride,
                bo_get_offset: *bo_get_offset,
                bo_get_modifier: *bo_get_modifier,
                _lib: lib,
            })
        }
    }
}

struct GbmDevice {
    api: GbmApi,
    ptr: *mut std::ffi::c_void,
    _file: File,
}
// SAFETY: device is immutable after construction; BOs retain it until their
// destruction. GBM operations are serialized on capture, teardown on last drop.
unsafe impl Send for GbmDevice {}
unsafe impl Sync for GbmDevice {}
impl Drop for GbmDevice {
    fn drop(&mut self) {
        unsafe { (self.api.destroy_device)(self.ptr) };
    }
}

pub struct GbmBuffer {
    device: Arc<GbmDevice>,
    bo: *mut std::ffi::c_void,
    pub fd: OwnedFd,
    pub stride: u32,
    pub offset: u32,
    pub modifier: u64,
    pub wl_buffer: Option<wl_buffer::WlBuffer>,
}

// SAFETY: buffers are immutable after publication. Their lease prevents reuse
// while VPP reads; the last owner destroys the proxy and BO after completion.
unsafe impl Send for GbmBuffer {}
unsafe impl Sync for GbmBuffer {}
impl Drop for GbmBuffer {
    fn drop(&mut self) {
        if let Some(buffer) = &self.wl_buffer {
            buffer.destroy();
        }
        unsafe { (self.device.api.bo_destroy)(self.bo) };
    }
}

pub struct DmabufRingSlot {
    pub gbm: Arc<GbmBuffer>,
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmabufOffer {
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

pub struct DmabufCapturedFrame {
    pub ring_index: usize,
    pub captured_at: Instant,
    pub offer: DmabufOffer,
    pub y_inverted: bool,
    pub generation: u64,
    pub buffer: Arc<GbmBuffer>,
    _lease: SlotLease,
}
struct SlotLease {
    generation: u64,
    index: usize,
    release: mpsc::Sender<(u64, usize)>,
}
impl Drop for SlotLease {
    fn drop(&mut self) {
        let _ = self.release.send((self.generation, self.index));
    }
}

struct CaptureState {
    sync_done: bool,
    manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    outputs: Vec<(wl_output::WlOutput, Option<String>)>,
    dmabuf_offer: Option<DmabufOffer>,
    ready: bool,
    failed: bool,
    flags: u32,
}

impl CaptureState {
    fn new() -> Self {
        Self {
            sync_done: false,
            manager: None,
            dmabuf: None,
            outputs: Vec::new(),
            dmabuf_offer: None,
            ready: false,
            failed: false,
            flags: 0,
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for CaptureState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "wl_output" {
                let output =
                    registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                state.outputs.push((output, None));
            } else if interface == "zwlr_screencopy_manager_v1" {
                state.manager = Some(
                    registry.bind::<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, _, _>(
                        name,
                        version.min(3),
                        qh,
                        (),
                    ),
                );
            } else if interface == "zwp_linux_dmabuf_v1" {
                state.dmabuf = Some(
                    registry.bind::<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, _, _>(
                        name,
                        version.min(3),
                        qh,
                        (),
                    ),
                );
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for CaptureState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            for (out, out_name) in &mut state.outputs {
                if out == output {
                    *out_name = Some(name);
                    break;
                }
            }
        }
    }
}

impl Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, ()> for CaptureState {
    fn event(
        _: &mut Self,
        _: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _: zwlr_screencopy_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for CaptureState {
    fn event(
        _: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        _: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for CaptureState {
    fn event(
        _: &mut Self,
        _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        _: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::LinuxDmabuf {
                format,
                width,
                height,
            } => {
                if state.dmabuf_offer.is_none() {
                    state.dmabuf_offer = Some(DmabufOffer {
                        format,
                        width,
                        height,
                    });
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => {
                state.sync_done = true;
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                state.flags = flags.into();
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                state.ready = true;
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                state.failed = true;
            }
            _ => {}
        }
    }
}

delegate_noop!(CaptureState: ignore wl_buffer::WlBuffer);

pub struct LinuxDmabufCapture {
    _connection: Connection,
    queue: wayland_client::EventQueue<CaptureState>,
    state: CaptureState,
    selected_output: wl_output::WlOutput,
    device: Arc<GbmDevice>,
    release_tx: mpsc::Sender<(u64, usize)>,
    release_rx: mpsc::Receiver<(u64, usize)>,
    generation: u64,
    pub ring: Vec<DmabufRingSlot>,
    pub offer: DmabufOffer,
    current_slot: usize,
}

impl LinuxDmabufCapture {
    pub fn connect(
        output_name: Option<&str>,
        stop: &AtomicBool,
        deadline: Instant,
        wake: BorrowedFd<'_>,
    ) -> Result<Self, DmabufCaptureError> {
        let connection = Connection::connect_to_env()?;
        let (mut state, mut queue) = (CaptureState::new(), connection.new_event_queue());
        let qh = queue.handle();
        let _registry = connection.display().get_registry(&qh, ());

        for _ in 0..2 {
            state.sync_done = false;
            connection.display().sync(&qh, ());
            while !state.sync_done {
                dispatch_slice(&mut queue, &mut state, stop, deadline, wake)?;
            }
        }

        if stop.load(Ordering::Acquire) {
            return Err(DmabufCaptureError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(DmabufCaptureError::Timeout);
        }

        let manager = state
            .manager
            .clone()
            .ok_or(DmabufCaptureError::MissingInterface(
                "zwlr_screencopy_manager_v1",
            ))?;
        let dmabuf = state
            .dmabuf
            .clone()
            .ok_or(DmabufCaptureError::MissingInterface("zwp_linux_dmabuf_v1"))?;

        let selected_output = if let Some(target) = output_name {
            state
                .outputs
                .iter()
                .find(|(_, name)| name.as_deref() == Some(target))
                .map(|(out, _)| out.clone())
                .ok_or_else(|| DmabufCaptureError::OutputNotFound(Some(target.to_string())))?
        } else {
            state
                .outputs
                .first()
                .map(|(out, _)| out.clone())
                .ok_or(DmabufCaptureError::OutputNotFound(None))?
        };

        state.sync_done = false;
        state.failed = false;
        let probe_frame = manager.capture_output(1, &selected_output, &qh, ());
        while !state.sync_done && !state.failed {
            if stop.load(Ordering::Acquire) {
                probe_frame.destroy();
                return Err(DmabufCaptureError::Cancelled);
            }
            if Instant::now() >= deadline {
                probe_frame.destroy();
                return Err(DmabufCaptureError::Timeout);
            }
            dispatch_slice(&mut queue, &mut state, stop, deadline, wake)?;
        }
        probe_frame.destroy();

        if state.failed || state.dmabuf_offer.is_none() {
            return Err(DmabufCaptureError::NoDmabufOffer);
        }
        let offer = state.dmabuf_offer.unwrap();

        let drm_path =
            std::env::var("MAHO_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".into());
        let drm_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&drm_path)
            .map_err(|e| DmabufCaptureError::DrmOpen(drm_path.clone(), e))?;

        let gbm = GbmApi::load()?;
        // SAFETY: drm_file has a valid open file descriptor to the DRM render node.
        let gbm_device = unsafe { (gbm.create_device)(drm_file.as_raw_fd()) };
        if gbm_device.is_null() {
            return Err(DmabufCaptureError::Gbm(
                "gbm_create_device returned null".into(),
            ));
        }

        let device = Arc::new(GbmDevice {
            api: gbm,
            ptr: gbm_device,
            _file: drm_file,
        });
        let ring = Self::allocate_ring(&device, &dmabuf, &qh, offer)?;
        let (release_tx, release_rx) = mpsc::channel();

        Ok(Self {
            _connection: connection,
            queue,
            state,
            selected_output,
            device,
            release_tx,
            release_rx,
            generation: 0,
            ring,
            offer,
            current_slot: 0,
        })
    }

    fn allocate_ring(
        device: &Arc<GbmDevice>,
        dmabuf: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        qh: &QueueHandle<CaptureState>,
        offer: DmabufOffer,
    ) -> Result<Vec<DmabufRingSlot>, DmabufCaptureError> {
        let mut ring = Vec::with_capacity(RING_CAPACITY);
        for _ in 0..RING_CAPACITY {
            // SAFETY: gbm_device is a valid gbm device pointer; format and flags match spec.
            let bo = unsafe {
                (device.api.bo_create)(
                    device.ptr,
                    offer.width,
                    offer.height,
                    offer.format,
                    GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR,
                )
            };
            if bo.is_null() {
                return Err(DmabufCaptureError::Gbm(
                    "gbm_bo_create returned null".into(),
                ));
            }

            // SAFETY: bo is a valid live gbm buffer object.
            let raw_fd = unsafe { (device.api.bo_get_fd)(bo) };
            if raw_fd < 0 {
                unsafe { (device.api.bo_destroy)(bo) };
                return Err(std::io::Error::last_os_error().into());
            }
            let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
            // SAFETY: bo is a valid live gbm buffer object.
            let stride = unsafe { (device.api.bo_get_stride)(bo) };
            // SAFETY: bo is a valid live gbm buffer object.
            let offset = unsafe { (device.api.bo_get_offset)(bo, 0) };
            // SAFETY: bo is a valid live gbm buffer object.
            let modifier = unsafe { (device.api.bo_get_modifier)(bo) };

            let mut owned = GbmBuffer {
                device: Arc::clone(device),
                bo,
                fd,
                stride,
                offset,
                modifier,
                wl_buffer: None,
            };
            let params = dmabuf.create_params(qh, ());
            let mod_hi = (modifier >> 32) as u32;
            let mod_lo = (modifier & 0xFFFF_FFFF) as u32;
            // SAFETY: fd is a valid raw file descriptor returned by gbm_bo_get_fd.
            let borrowed_fd = unsafe { BorrowedFd::borrow_raw(owned.fd.as_raw_fd()) };
            params.add(borrowed_fd, 0, offset, stride, mod_hi, mod_lo);
            let wl_buf = params.create_immed(
                offer.width as i32,
                offer.height as i32,
                offer.format,
                zwp_linux_buffer_params_v1::Flags::empty(),
                qh,
                (),
            );
            params.destroy();

            owned.wl_buffer = Some(wl_buf);
            ring.push(DmabufRingSlot {
                gbm: Arc::new(owned),
                busy: false,
            });
        }

        Ok(ring)
    }

    pub fn next_free_slot(&mut self) -> Option<usize> {
        while let Ok((generation, index)) = self.release_rx.try_recv() {
            if generation == self.generation {
                self.release_slot(index);
            }
        }
        for i in 0..RING_CAPACITY {
            let idx = (self.current_slot + i) % RING_CAPACITY;
            if !self.ring[idx].busy {
                self.current_slot = (idx + 1) % RING_CAPACITY;
                return Some(idx);
            }
        }
        None
    }

    pub fn capture_frame(
        &mut self,
        slot_index: usize,
        stop: &AtomicBool,
        timeout: Duration,
        wake: BorrowedFd<'_>,
    ) -> Result<DmabufCapturedFrame, DmabufCaptureError> {
        let deadline = Instant::now() + timeout;
        let qh = self.queue.handle();
        let manager = self
            .state
            .manager
            .clone()
            .ok_or(DmabufCaptureError::MissingInterface(
                "zwlr_screencopy_manager_v1",
            ))?;

        self.state.dmabuf_offer = None;
        self.state.flags = 0;
        self.state.sync_done = false;
        self.state.ready = false;
        self.state.failed = false;

        let frame = manager.capture_output(1, &self.selected_output, &qh, ());
        while !self.state.sync_done && !self.state.failed {
            if stop.load(Ordering::Acquire) {
                frame.destroy();
                return Err(DmabufCaptureError::Cancelled);
            }
            if Instant::now() >= deadline {
                frame.destroy();
                return Err(DmabufCaptureError::Timeout);
            }
            dispatch_slice(&mut self.queue, &mut self.state, stop, deadline, wake)?;
        }

        if self.state.failed {
            frame.destroy();
            return Err(DmabufCaptureError::CaptureFailed);
        }

        let offer = self
            .state
            .dmabuf_offer
            .ok_or(DmabufCaptureError::NoDmabufOffer)?;
        if offer != self.offer {
            self.ring = Self::allocate_ring(
                &self.device,
                self.state.dmabuf.as_ref().unwrap(),
                &qh,
                offer,
            )?;
            self.offer = offer;
            self.generation += 1;
        }
        let wl_buf = &self.ring[slot_index].gbm.wl_buffer;
        frame.copy(wl_buf.as_ref().expect("allocated capture buffer"));

        while !self.state.ready && !self.state.failed {
            if stop.load(Ordering::Acquire) {
                frame.destroy();
                return Err(DmabufCaptureError::Cancelled);
            }
            if Instant::now() >= deadline {
                frame.destroy();
                return Err(DmabufCaptureError::Timeout);
            }
            dispatch_slice(&mut self.queue, &mut self.state, stop, deadline, wake)?;
        }

        frame.destroy();

        if self.state.failed {
            return Err(DmabufCaptureError::CaptureFailed);
        }

        self.ring[slot_index].busy = true;

        Ok(DmabufCapturedFrame {
            ring_index: slot_index,
            captured_at: Instant::now(),
            offer: self.offer,
            y_inverted: self.state.flags & 1 != 0,
            generation: self.generation,
            buffer: Arc::clone(&self.ring[slot_index].gbm),
            _lease: SlotLease {
                generation: self.generation,
                index: slot_index,
                release: self.release_tx.clone(),
            },
        })
    }

    pub fn release_slot(&mut self, slot_index: usize) {
        if slot_index < self.ring.len() {
            self.ring[slot_index].busy = false;
        }
    }
}

fn dispatch_slice(
    queue: &mut wayland_client::EventQueue<CaptureState>,
    state: &mut CaptureState,
    stop: &AtomicBool,
    deadline: Instant,
    wake: BorrowedFd<'_>,
) -> Result<(), DmabufCaptureError> {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};
    use wayland_client::backend::WaylandError;
    if stop.load(Ordering::Acquire) {
        return Err(DmabufCaptureError::Cancelled);
    }
    let timeout = deadline
        .checked_duration_since(Instant::now())
        .ok_or(DmabufCaptureError::Timeout)?;
    if queue.dispatch_pending(state)? > 0 {
        return Ok(());
    }
    let mut events = PollFlags::IN;
    match queue.flush() {
        Ok(()) => {}
        Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
            events |= PollFlags::OUT
        }
        Err(error) => return Err(wayland_client::DispatchError::from(error).into()),
    }
    if let Some(guard) = queue.prepare_read() {
        let duration =
            Timespec::try_from(timeout.min(Duration::from_millis(50))).expect("bounded duration");
        let fd = guard.connection_fd();
        let mut fds = [PollFd::new(&fd, events), PollFd::new(&wake, PollFlags::IN)];
        match poll(&mut fds, Some(&duration)) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => return Ok(()),
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
        let ready = fds[0].revents();
        if !stop.load(Ordering::Acquire)
            && ready.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
        {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(wayland_client::DispatchError::from(error).into()),
            }
        }
    }
    if !stop.load(Ordering::Acquire) {
        queue.dispatch_pending(state)?;
    }
    Ok(())
}

impl Dispatch<wl_callback::WlCallback, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.sync_done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::native_pipeline::Handoff;
    use super::*;
    use std::os::fd::AsFd;

    #[test]
    fn stalled_encoder_releases_replaced_reselected_and_stopped_slots() {
        let handoff = Handoff::new();
        let (tx, rx) = mpsc::channel();
        let lease = |index| SlotLease {
            generation: 7,
            index,
            release: tx.clone(),
        };
        assert!(handoff.publish(lease(0)));
        let mut encoding = handoff.next().unwrap();
        assert!(handoff.publish(lease(1)));
        assert!(handoff.publish(lease(2)));
        assert_eq!(rx.try_recv().unwrap(), (7, 1));
        assert!(handoff.reselect(&mut encoding));
        assert_eq!(rx.try_recv().unwrap(), (7, 0));
        assert!(handoff.publish(lease(0)));
        handoff.stop();
        assert_eq!(rx.try_recv().unwrap(), (7, 0));
        drop(encoding);
        assert_eq!(rx.try_recv().unwrap(), (7, 2));
        assert!(!handoff.publish(lease(1)));
        assert_eq!(rx.try_recv().unwrap(), (7, 1));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn stop_wakes_readiness_poll_on_stalled_compositor() {
        use std::io::Read;
        let (client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let handoff = Arc::new(Handoff::<()>::new());
        let worker_handoff = Arc::clone(&handoff);
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let connection = Connection::from_socket(client).unwrap();
            let mut queue = connection.new_event_queue();
            let mut state = CaptureState::new();
            connection.display().sync(&queue.handle(), ());
            while !worker_handoff.is_stopped() {
                let result = dispatch_slice(
                    &mut queue,
                    &mut state,
                    worker_handoff.cancellation(),
                    Instant::now() + Duration::from_secs(10),
                    worker_handoff.readiness_fd(),
                );
                if matches!(result, Err(DmabufCaptureError::Cancelled)) {
                    break;
                }
                result.unwrap();
            }
            tx.send(()).unwrap();
        });
        // Flushed request proves the worker reached dispatch before stop.
        server.read_exact(&mut [0; 8]).unwrap();
        handoff.stop();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn stalled_compositor_dispatch_obeys_deadline_and_cancel() {
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let mut queue = connection.new_event_queue();
        let mut state = CaptureState::new();
        let stop = AtomicBool::new(false);
        let (wake, _writer) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(matches!(
            dispatch_slice(&mut queue, &mut state, &stop, Instant::now(), wake.as_fd()),
            Err(DmabufCaptureError::Timeout)
        ));
        stop.store(true, Ordering::Release);
        assert!(matches!(
            dispatch_slice(
                &mut queue,
                &mut state,
                &stop,
                Instant::now() + Duration::from_secs(5),
                wake.as_fd()
            ),
            Err(DmabufCaptureError::Cancelled)
        ));
    }
}
