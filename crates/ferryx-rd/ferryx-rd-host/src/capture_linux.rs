//! Hyprland/wlroots screen capture for the Linux host.
//!
//! The fast path talks to `zwlr_screencopy_manager_v1` directly and uses a
//! reusable `wl_shm` buffer. Hyprland exposes this protocol without opening a
//! desktop portal chooser, and version 2+ reports compositor damage regions.
//!
//! Runtime QA must be performed in a real Hyprland session. The module binds
//! wlr-screencopy v3, whose `linux_dmabuf` offer is the zero-copy extension; the
//! current host seam deliberately selects its portable wl_shm BGRA offer so the
//! CPU encoder fallback has identical input. A future DRM-frame host seam can
//! import that DMA-BUF directly into VAAPI without changing this API.
//!
//! Compositors without wlr-screencopy use the shared session selector's
//! XDG Desktop Portal ScreenCast/PipeWire fallback. Output probing remains
//! picker-free; consent is requested only when a streaming session starts.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::AsFd,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[path = "capture_linux_cursor.rs"]
mod cursor_capture;

use thiserror::Error;
use wayland_client::{
    delegate_noop,
    protocol::{wl_buffer, wl_callback, wl_output, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

/// Pixel-space rectangle reported by the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Optional logical-output region to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Selects a Wayland output and whether the cursor is composited into frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    /// `wl_output.name`, for example `DP-1`. `None` selects the first output.
    pub output_name: Option<String>,
    pub region: Option<CaptureRegion>,
    pub overlay_cursor: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            output_name: None,
            region: None,
            overlay_cursor: true,
        }
    }
}

/// Output metadata obtained from `wl_output`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    pub global_name: u32,
    pub name: Option<String>,
    /// Native dimensions are populated from the current `wl_output.mode` when
    /// advertised. Capture buffer dimensions remain authoritative.
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale: i32,
}

impl OutputInfo {
    pub fn logical_width(&self) -> u32 {
        self.pixel_width / self.scale.max(1) as u32
    }

    pub fn logical_height(&self) -> u32 {
        self.pixel_height / self.scale.max(1) as u32
    }
}

/// A tightly packed, top-down BGRA frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
    pub damage: Vec<DamageRect>,
    /// Compositor presentation timestamp. Its epoch is compositor-defined.
    pub presentation_time_ns: u128,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("capture cancelled")]
    Cancelled,
    #[error("capture initialization deadline expired")]
    Timeout,
    #[error("failed to connect to the Wayland compositor: {0}")]
    Connect(#[from] wayland_client::ConnectError),
    #[error("Wayland dispatch failed: {0}")]
    Dispatch(#[from] wayland_client::DispatchError),
    #[error("the compositor does not expose {0}; use the XDG portal PipeWire fallback")]
    PortalRequired(&'static str),
    #[error("Wayland output {0:?} was not found")]
    OutputNotFound(Option<String>),
    #[error("the compositor offered unsupported wl_shm format {0}")]
    UnsupportedFormat(String),
    #[error("invalid capture buffer: {0}")]
    InvalidBuffer(&'static str),
    #[error("the compositor rejected the screencopy request")]
    CaptureFailed,
    #[error("screen capture I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy)]
struct OutputData {
    global_name: u32,
}

#[derive(Debug, Clone, Copy)]
struct BufferDescription {
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
}

struct CachedBuffer {
    file: File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    description: BufferDescription,
}

struct ActiveCapture {
    description: BufferDescription,
    damage: Vec<DamageRect>,
    y_inverted: bool,
}

struct CaptureState {
    cursor: cursor_capture::CursorCapture,
    sync_done: bool,
    manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    shm: Option<wl_shm::WlShm>,
    outputs: Vec<(wl_output::WlOutput, OutputInfo)>,
    offered_buffer: Option<BufferDescription>,
    /// Format offers that this host cannot consume, reported only when no
    /// supported offer arrives before the frame is finalized.
    rejected_formats: Vec<String>,
    /// `Flags` arrives before `BufferDone`, so it cannot be stored on `active`.
    y_inverted: bool,
    active: Option<ActiveCapture>,
    result: Option<Result<CapturedFrame, CaptureError>>,
    raw_shm_buf: Vec<u8>,
    cached_buffer: Option<CachedBuffer>,
}

impl CaptureState {
    fn output(&self, requested_name: Option<&str>) -> Option<(wl_output::WlOutput, OutputInfo)> {
        self.outputs
            .iter()
            .find(|(_, info)| match requested_name {
                Some(requested) => info.name.as_deref() == Some(requested),
                None => true,
            })
            .cloned()
    }

    fn create_buffer(
        &mut self,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        qh: &QueueHandle<Self>,
    ) -> Result<(), CaptureError> {
        let description =
            self.offered_buffer
                .take()
                .ok_or_else(|| match self.rejected_formats.first() {
                    Some(format) => CaptureError::UnsupportedFormat(format.clone()),
                    None => CaptureError::InvalidBuffer("missing wl_shm buffer offer"),
                })?;
        let size = u64::from(description.stride)
            .checked_mul(u64::from(description.height))
            .ok_or(CaptureError::InvalidBuffer("buffer size overflow"))?;
        if size == 0 || size > i32::MAX as u64 {
            return Err(CaptureError::InvalidBuffer(
                "buffer size is outside Wayland limits",
            ));
        }
        if description.stride < description.width.saturating_mul(4) {
            return Err(CaptureError::InvalidBuffer(
                "stride is smaller than BGRA row width",
            ));
        }

        let is_cached_valid = match &self.cached_buffer {
            Some(cached) => {
                cached.description.width == description.width
                    && cached.description.height == description.height
                    && cached.description.stride == description.stride
                    && cached.description.format == description.format
            }
            None => false,
        };

        if !is_cached_valid {
            let mut file = anonymous_mem_file()?;
            file.set_len(size)?;
            file.seek(SeekFrom::Start(0))?;

            let shm = self
                .shm
                .as_ref()
                .ok_or(CaptureError::PortalRequired("wl_shm"))?;
            let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
            let buffer = pool.create_buffer(
                0,
                description.width as i32,
                description.height as i32,
                description.stride as i32,
                description.format,
                qh,
                (),
            );
            // Dropping the Rust proxies sends nothing; the compositor keeps the
            // old buffer, pool, and shm mapping alive without explicit destroys.
            if let Some(previous) = self.cached_buffer.take() {
                previous.buffer.destroy();
                previous._pool.destroy();
            }
            self.cached_buffer = Some(CachedBuffer {
                file,
                _pool: pool,
                buffer,
                description,
            });
        }

        let cached = self.cached_buffer.as_ref().unwrap();
        // Use `copy` to guarantee immediate frame presentation at target cadence
        // without blocking indefinitely on compositor damage events.
        frame.copy(&cached.buffer);
        self.active = Some(ActiveCapture {
            description,
            damage: Vec::new(),
            y_inverted: self.y_inverted,
        });
        Ok(())
    }

    fn finish_capture(
        &mut self,
        tv_sec_hi: u32,
        tv_sec_lo: u32,
        tv_nsec: u32,
    ) -> Result<CapturedFrame, CaptureError> {
        let mut active = self
            .active
            .take()
            .ok_or(CaptureError::InvalidBuffer("ready arrived before copy"))?;
        let cached = self
            .cached_buffer
            .as_mut()
            .ok_or(CaptureError::InvalidBuffer("missing cached buffer"))?;
        let description = active.description;
        let row_bytes = description.width as usize * 4;
        let total_bytes = row_bytes * description.height as usize;
        let mut bgra = vec![0_u8; total_bytes];
        cached.file.seek(SeekFrom::Start(0))?;

        if !active.y_inverted && description.stride as usize == row_bytes {
            cached.file.read_exact(&mut bgra)?;
        } else {
            let source_len = description.stride as usize * description.height as usize;
            if self.raw_shm_buf.len() < source_len {
                self.raw_shm_buf.resize(source_len, 0);
            }
            cached
                .file
                .read_exact(&mut self.raw_shm_buf[..source_len])?;

            for destination_y in 0..description.height as usize {
                let source_y = if active.y_inverted {
                    description.height as usize - 1 - destination_y
                } else {
                    destination_y
                };
                let source_start = source_y * description.stride as usize;
                let destination_start = destination_y * row_bytes;
                bgra[destination_start..destination_start + row_bytes]
                    .copy_from_slice(&self.raw_shm_buf[source_start..source_start + row_bytes]);
            }
        }

        // XRGB8888 has an unused high byte. Making it opaque also gives callers
        // one stable BGRA contract for both ARGB8888 and XRGB8888.
        let (_, u32_slice, _) = unsafe { bgra.align_to_mut::<u32>() };
        for pixel in u32_slice {
            *pixel |= 0xFF00_0000;
        }

        if active.y_inverted {
            for rect in &mut active.damage {
                rect.y = description
                    .height
                    .saturating_sub(rect.y.saturating_add(rect.height));
            }
        }

        let seconds = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
        Ok(CapturedFrame {
            width: description.width,
            height: description.height,
            stride: description.width * 4,
            bgra,
            damage: active.damage,
            presentation_time_ns: u128::from(seconds) * 1_000_000_000 + u128::from(tv_nsec),
        })
    }
}

/// Blocking wlr-screencopy source. Run it on the host capture thread.
pub struct LinuxCapture {
    _connection: Connection,
    event_queue: wayland_client::EventQueue<CaptureState>,
    state: CaptureState,
    config: CaptureConfig,
    selected_output: OutputInfo,
    request_pending: bool,
}

impl LinuxCapture {
    pub fn connect(config: CaptureConfig) -> Result<Self, CaptureError> {
        let connection = Connection::connect_to_env()?;
        Self::from_connection(
            config,
            connection,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(5),
            true,
        )
    }

    pub fn connect_cancellable(
        config: CaptureConfig,
        stop: &AtomicBool,
        deadline: Instant,
    ) -> Result<Self, CaptureError> {
        check_startup(stop, deadline)?;
        Self::from_connection(config, Connection::connect_to_env()?, stop, deadline, true)
    }

    /// Reads output geometry without requiring a capture protocol or opening a portal picker.
    pub fn probe_output(config: CaptureConfig) -> Result<OutputInfo, CaptureError> {
        let capture = Self::from_connection(
            config,
            Connection::connect_to_env()?,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(5),
            false,
        )?;
        Ok(capture.selected_output)
    }

    fn from_connection(
        config: CaptureConfig,
        connection: Connection,
        stop: &AtomicBool,
        deadline: Instant,
        require_screencopy: bool,
    ) -> Result<Self, CaptureError> {
        check_startup(stop, deadline)?;
        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();
        connection.display().get_registry(&qh, ());

        let mut state = CaptureState {
            cursor: cursor_capture::CursorCapture::default(),
            sync_done: false,
            manager: None,
            shm: None,
            outputs: Vec::new(),
            offered_buffer: None,
            rejected_formats: Vec::new(),
            y_inverted: false,
            active: None,
            result: None,
            raw_shm_buf: Vec::new(),
            cached_buffer: None,
        };
        // First roundtrip discovers globals; the second receives wl_output
        // metadata, including the connector name on wl_output v4.
        for _ in 0..2 {
            state.sync_done = false;
            connection.display().sync(&qh, ());
            while !state.sync_done {
                check_startup(stop, deadline)?;
                dispatch_slice(
                    &mut event_queue,
                    &mut state,
                    stop,
                    deadline.saturating_duration_since(Instant::now()),
                )?;
            }
        }

        if require_screencopy && state.manager.is_none() {
            return Err(CaptureError::PortalRequired("zwlr_screencopy_manager_v1"));
        }
        if require_screencopy && state.shm.is_none() {
            return Err(CaptureError::PortalRequired("wl_shm"));
        }
        let (_, selected_output) = state
            .output(config.output_name.as_deref())
            .ok_or_else(|| CaptureError::OutputNotFound(config.output_name.clone()))?;

        Ok(Self {
            _connection: connection,
            event_queue,
            state,
            config,
            selected_output,
            request_pending: false,
        })
    }

    pub fn output_info(&self) -> &OutputInfo {
        &self.selected_output
    }

    pub fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let stop = AtomicBool::new(false);
        loop {
            if let Some(frame) = self.capture_frame_cancellable(&stop, Duration::from_millis(50))? {
                return Ok(frame);
            }
        }
    }

    pub fn capture_frame_cancellable(
        &mut self,
        stop: &AtomicBool,
        poll_timeout: Duration,
    ) -> Result<Option<CapturedFrame>, CaptureError> {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        self.state
            .cursor
            .request(self.state.shm.as_ref(), &self.event_queue.handle())?;
        if !self.request_pending {
            self.state.offered_buffer = None;
            self.state.rejected_formats.clear();
            self.state.y_inverted = false;
            self.state.active = None;
            self.state.result = None;

            let manager = self
                .state
                .manager
                .as_ref()
                .ok_or(CaptureError::PortalRequired("zwlr_screencopy_manager_v1"))?
                .clone();
            let (output, _) = self
                .state
                .output(self.config.output_name.as_deref())
                .ok_or_else(|| CaptureError::OutputNotFound(self.config.output_name.clone()))?;
            let qh = self.event_queue.handle();
            let overlay_cursor = i32::from(self.config.overlay_cursor);
            if let Some(region) = self.config.region {
                if region.width <= 0 || region.height <= 0 {
                    return Err(CaptureError::InvalidBuffer("capture region is empty"));
                }
                manager.capture_output_region(
                    overlay_cursor,
                    &output,
                    region.x,
                    region.y,
                    region.width,
                    region.height,
                    &qh,
                    (),
                );
            } else {
                manager.capture_output(overlay_cursor, &output, &qh, ());
            }

            self.request_pending = true;
        }
        dispatch_slice(&mut self.event_queue, &mut self.state, stop, poll_timeout)?;
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        if let Some(result) = self.state.result.take() {
            self.request_pending = false;
            return result.map(Some);
        }
        Ok(None)
    }
}

fn check_startup(stop: &AtomicBool, deadline: Instant) -> Result<(), CaptureError> {
    if stop.load(Ordering::Acquire) {
        return Err(CaptureError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(CaptureError::Timeout);
    }
    Ok(())
}

/// Anonymous in-memory file backing the `wl_shm` pool. `memfd_create` keeps
/// frame pixels out of any filesystem entirely, so the per-frame `read_exact`
/// is a pure kernel memcpy and never touches a disk. Kernels without memfd
/// (older than 3.17) fall back to an unlinked tempfile.
fn anonymous_mem_file() -> std::io::Result<File> {
    use std::os::fd::FromRawFd;

    let name = b"maho-wl-shm\0";
    // SAFETY: name is a NUL-terminated literal; the returned descriptor, when
    // positive, is freshly created and exclusively owned by the caller.
    let fd = unsafe { libc::memfd_create(name.as_ptr().cast(), libc::MFD_CLOEXEC) };
    if fd >= 0 {
        // SAFETY: we own the descriptor created above.
        return Ok(unsafe { File::from_raw_fd(fd) });
    }
    tempfile::tempfile()
}

fn dispatch_slice(
    queue: &mut wayland_client::EventQueue<CaptureState>,
    state: &mut CaptureState,
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<(), CaptureError> {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};
    use wayland_client::backend::WaylandError;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
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
        let mut fds = [PollFd::new(&fd, events)];
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

impl Dispatch<wl_registry::WlRegistry, ()> for CaptureState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface
                == zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1::interface().name =>
            {
                state.manager = Some(registry.bind(name, version.min(3), qh, ()));
            }
            wl_registry::Event::Global {
                name,
                interface,
                version: _,
            } if interface == wl_shm::WlShm::interface().name => {
                state.shm = Some(registry.bind(name, 1, qh, ()));
            }
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == wl_output::WlOutput::interface().name => {
                let output =
                    registry.bind(name, version.min(4), qh, OutputData { global_name: name });
                state.outputs.push((
                    output,
                    OutputInfo {
                        global_name: name,
                        name: None,
                        pixel_width: 0,
                        pixel_height: 0,
                        scale: 1,
                    },
                ));
            }
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                state.cursor.global(registry, name, &interface, version, qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                state
                    .outputs
                    .retain(|(_, output)| output.global_name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, OutputData> for CaptureState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        data: &OutputData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, info)) = state
            .outputs
            .iter_mut()
            .find(|(_, info)| info.global_name == data.global_name)
        else {
            return;
        };
        match event {
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) => {
                info.pixel_width = width.max(0) as u32;
                info.pixel_height = height.max(0) as u32;
            }
            wl_output::Event::Scale { factor } => info.scale = factor.max(1),
            wl_output::Event::Name { name } => info.name = Some(name),
            _ => {}
        }
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let format = match format {
                    WEnum::Value(wl_shm::Format::Argb8888) => wl_shm::Format::Argb8888,
                    WEnum::Value(wl_shm::Format::Xrgb8888) => wl_shm::Format::Xrgb8888,
                    other => {
                        // Versions below 3 advertise alternatives across several
                        // buffer events; only BufferDone can declare failure.
                        state.rejected_formats.push(format!("{other:?}"));
                        return;
                    }
                };
                if state.offered_buffer.is_some() || state.active.is_some() {
                    // `copy` may be sent at most once per frame, so keep the
                    // first supported offer instead of re-arming the capture.
                    return;
                }
                state.offered_buffer = Some(BufferDescription {
                    format,
                    width,
                    height,
                    stride,
                });
                // Protocol versions 1 and 2 have no buffer_done event.
                if frame.version() < 3 {
                    if let Err(error) = state.create_buffer(frame, qh) {
                        state.result = Some(Err(error));
                    }
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => {
                if let Err(error) = state.create_buffer(frame, qh) {
                    state.result = Some(Err(error));
                }
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                // `Flags` precedes `BufferDone`, so record it on the state and
                // let `create_buffer` copy it into the active capture.
                let y_inverted = match flags {
                    WEnum::Value(flags) => flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert),
                    WEnum::Unknown(_) => false,
                };
                state.y_inverted = y_inverted;
                if let Some(active) = &mut state.active {
                    active.y_inverted = y_inverted;
                }
            }
            zwlr_screencopy_frame_v1::Event::Damage {
                x,
                y,
                width,
                height,
            } => {
                if let Some(active) = &mut state.active {
                    active.damage.push(DamageRect {
                        x,
                        y,
                        width,
                        height,
                    });
                }
            }
            zwlr_screencopy_frame_v1::Event::Ready {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                state.result = Some(state.finish_capture(tv_sec_hi, tv_sec_lo, tv_nsec));
                frame.destroy();
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                state.result = Some(Err(CaptureError::CaptureFailed));
                frame.destroy();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixStream, sync::mpsc, thread};

    #[test]
    fn anonymous_mem_file_supports_len_and_readback() {
        let mut file = anonymous_mem_file().expect("allocate anonymous frame buffer");
        file.set_len(16).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        std::io::Write::write_all(&mut file, &[7_u8; 16]).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut read_back = [0_u8; 16];
        file.read_exact(&mut read_back).unwrap();
        assert_eq!(read_back, [7_u8; 16]);
    }

    #[test]
    fn cancelled_initialization_does_not_wait_for_compositor() {
        let (client, server) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = LinuxCapture::from_connection(
                CaptureConfig::default(),
                connection,
                &AtomicBool::new(true),
                Instant::now() + Duration::from_secs(5),
                true,
            );
            tx.send(result.is_err()).unwrap();
        });
        let result = rx.recv_timeout(Duration::from_secs(1));
        drop(server);
        worker.join().unwrap();
        assert_eq!(
            result,
            Ok(true),
            "cancelled startup must not await a Wayland roundtrip"
        );
    }

    #[test]
    fn pending_initialization_cancels_after_request_flush() {
        use std::sync::Arc;
        let (client, mut server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = LinuxCapture::from_connection(
                CaptureConfig::default(),
                connection,
                &worker_stop,
                Instant::now() + Duration::from_secs(10),
                true,
            );
            tx.send(matches!(result, Err(CaptureError::Cancelled)))
                .unwrap();
        });
        // Actual flushed Wayland request is the readiness signal, not a sleep.
        server.read_exact(&mut [0; 8]).unwrap();
        stop.store(true, Ordering::Release);
        let result = rx.recv_timeout(Duration::from_secs(2));
        drop(server);
        worker.join().unwrap();
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn pending_frame_slice_keeps_request_and_observes_stop() {
        let (client, _server) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let queue = connection.new_event_queue();
        let mut capture = LinuxCapture {
            _connection: connection,
            event_queue: queue,
            state: CaptureState {
                cursor: cursor_capture::CursorCapture::default(),
                sync_done: false,
                manager: None,
                shm: None,
                outputs: Vec::new(),
                offered_buffer: None,
                rejected_formats: Vec::new(),
                y_inverted: false,
                active: None,
                result: None,
                raw_shm_buf: Vec::new(),
                cached_buffer: None,
            },
            config: CaptureConfig::default(),
            selected_output: OutputInfo {
                global_name: 1,
                name: None,
                pixel_width: 1,
                pixel_height: 1,
                scale: 1,
            },
            request_pending: true,
        };
        let stop = AtomicBool::new(false);
        assert!(capture
            .capture_frame_cancellable(&stop, Duration::ZERO)
            .unwrap()
            .is_none());
        assert!(
            capture.request_pending,
            "timeout must not submit another request"
        );
        stop.store(true, Ordering::Release);
        assert!(capture
            .capture_frame_cancellable(&stop, Duration::from_secs(60))
            .unwrap()
            .is_none());
    }
}

delegate_noop!(CaptureState: ignore wl_shm::WlShm);
delegate_noop!(CaptureState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(CaptureState: ignore wl_buffer::WlBuffer);
delegate_noop!(CaptureState: ignore zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1);
