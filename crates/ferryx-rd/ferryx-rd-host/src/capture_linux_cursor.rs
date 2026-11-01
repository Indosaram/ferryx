//! Optional ext-image-copy-capture pointer cursor stream on the screencopy queue.
use super::*;
use wayland_client::protocol::{wl_pointer, wl_seat};
use wayland_protocols::ext::{
    image_capture_source::v1::client::{
        ext_image_capture_source_v1, ext_output_image_capture_source_manager_v1,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_cursor_session_v1 as cursor,
        ext_image_copy_capture_frame_v1 as frame, ext_image_copy_capture_manager_v1 as manager,
        ext_image_copy_capture_session_v1 as session,
    },
};

#[derive(Default)]
pub(super) struct CursorCapture {
    manager: Option<manager::ExtImageCopyCaptureManagerV1>,
    source_manager:
        Option<ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_session: Option<cursor::ExtImageCopyCaptureCursorSessionV1>,
    session: Option<session::ExtImageCopyCaptureSessionV1>,
    size: (u32, u32),
    format: Option<wl_shm::Format>,
    constraints_ready: bool,
    buffer: Option<CachedBuffer>,
    pending: bool,
    visible: bool,
    position: (i32, i32),
    hotspot: (i32, i32),
    changed: bool,
    shape: Option<ferryx_rd_proto::CursorShape>,
    stopped: bool,
}

pub(super) fn metadata_mode(negotiated: bool, manager: bool, source: bool, pointer: bool) -> bool {
    negotiated && manager && source && pointer
}

impl LinuxCapture {
    pub fn enable_cursor_metadata(&mut self, negotiated: bool) -> bool {
        if self.state.cursor.session.is_some() {
            return true;
        }
        let c = &self.state.cursor;
        if !metadata_mode(
            negotiated,
            c.manager.is_some(),
            c.source_manager.is_some(),
            c.pointer.is_some(),
        ) || self.config.region.is_some()
        {
            return false;
        }
        let qh = self.event_queue.handle();
        let (output, _) = self
            .state
            .output(self.config.output_name.as_deref())
            .expect("selected output");
        let c = &mut self.state.cursor;
        let source = c
            .source_manager
            .as_ref()
            .unwrap()
            .create_source(&output, &qh, ());
        let cursor = c.manager.as_ref().unwrap().create_pointer_cursor_session(
            &source,
            c.pointer.as_ref().unwrap(),
            &qh,
            (),
        );
        c.session = Some(cursor.get_capture_session(&qh, ()));
        c.cursor_session = Some(cursor);
        source.destroy();
        self.config.overlay_cursor = false;
        true
    }

    pub fn take_cursor(
        &mut self,
    ) -> Option<(ferryx_rd_proto::CursorUpdate, Option<ferryx_rd_proto::CursorShape>)> {
        let c = &mut self.state.cursor;
        if c.stopped {
            self.config.overlay_cursor = true;
        }
        if !std::mem::take(&mut c.changed) {
            return None;
        }
        let (width, height) = (
            self.selected_output.pixel_width,
            self.selected_output.pixel_height,
        );
        let position = if c.visible && !c.stopped && width > 0 && height > 0 {
            ferryx_rd_proto::CursorUpdate {
                x: c.position.0 as f32 / width as f32,
                y: c.position.1 as f32 / height as f32,
                cursor_type: 1,
            }
        } else {
            ferryx_rd_proto::CursorUpdate {
                x: -1.0,
                y: -1.0,
                cursor_type: 0,
            }
        };
        Some((position, if c.stopped { None } else { c.shape.take() }))
    }
}

impl CursorCapture {
    pub(super) fn global(
        &mut self,
        registry: &wl_registry::WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        qh: &QueueHandle<CaptureState>,
    ) {
        if interface == manager::ExtImageCopyCaptureManagerV1::interface().name {
            self.manager = Some(registry.bind(name, 1, qh, ()));
        } else if interface == ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1::interface().name {
            self.source_manager = Some(registry.bind(name, 1, qh, ()));
        } else if interface == wl_seat::WlSeat::interface().name && self.pointer.is_none() {
            let _: wl_seat::WlSeat = registry.bind(name, version.min(5), qh, ());
        }
    }

    pub(super) fn request(
        &mut self,
        shm: Option<&wl_shm::WlShm>,
        qh: &QueueHandle<CaptureState>,
    ) -> Result<(), CaptureError> {
        if self.pending || self.stopped || !self.constraints_ready {
            return Ok(());
        }
        let (Some(session), Some(format), Some(shm)) = (&self.session, self.format, shm) else {
            return Ok(());
        };
        let (width, height) = self.size;
        if width == 0 || height == 0 || width > 256 || height > 256 {
            self.stopped = true;
            self.visible = false;
            self.changed = true;
            return Ok(());
        }
        let reusable = match self.buffer.as_ref() {
            Some(b) => {
                b.description.width == width
                    && b.description.height == height
                    && b.description.format == format
            }
            None => false,
        };
        if !reusable {
            if let Some(old) = self.buffer.take() {
                old.buffer.destroy();
                old._pool.destroy();
            }
            let file = anonymous_mem_file()?;
            let size = width * height * 4;
            file.set_len(u64::from(size))?;
            let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
            let buffer = pool.create_buffer(
                0,
                width as i32,
                height as i32,
                (width * 4) as i32,
                format,
                qh,
                (),
            );
            self.buffer = Some(CachedBuffer {
                file,
                _pool: pool,
                buffer,
                description: BufferDescription {
                    format,
                    width,
                    height,
                    stride: width * 4,
                },
            });
        }
        let frame = session.create_frame(qh, ());
        frame.attach_buffer(&self.buffer.as_ref().unwrap().buffer);
        frame.damage_buffer(0, 0, width as i32, height as i32);
        frame.capture();
        self.pending = true;
        Ok(())
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for CaptureState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            if caps.contains(wl_seat::Capability::Pointer) && state.cursor.pointer.is_none() {
                state.cursor.pointer = Some(seat.get_pointer(qh, ()));
            }
        }
    }
}
impl Dispatch<cursor::ExtImageCopyCaptureCursorSessionV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _: &cursor::ExtImageCopyCaptureCursorSessionV1,
        event: cursor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let c = &mut state.cursor;
        match event {
            cursor::Event::Enter => {
                c.visible = true;
                c.changed = true;
            }
            cursor::Event::Leave => {
                c.visible = false;
                c.changed = true;
            }
            cursor::Event::Position { x, y } => {
                c.position = (x, y);
                c.changed = true;
            }
            cursor::Event::Hotspot { x, y } => c.hotspot = (x, y),
            _ => {}
        }
    }
}
impl Dispatch<session::ExtImageCopyCaptureSessionV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _: &session::ExtImageCopyCaptureSessionV1,
        event: session::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let c = &mut state.cursor;
        match event {
            session::Event::BufferSize { width, height } => {
                c.size = (width, height);
                c.constraints_ready = false;
            }
            session::Event::ShmFormat {
                format: WEnum::Value(format),
            } if matches!(
                format,
                wl_shm::Format::Argb8888
                    | wl_shm::Format::Xrgb8888
                    | wl_shm::Format::Abgr8888
                    | wl_shm::Format::Xbgr8888
            ) =>
            {
                c.format = Some(format)
            }
            session::Event::Done => {
                if c.format.is_none() {
                    c.stopped = true;
                    c.visible = false;
                    c.changed = true;
                }
                c.constraints_ready = true;
                if let Err(error) = c.request(state.shm.as_ref(), qh) {
                    state.result = Some(Err(error));
                }
            }
            session::Event::Stopped => {
                c.stopped = true;
                c.visible = false;
                c.changed = true;
            }
            _ => {}
        }
    }
}
impl Dispatch<frame::ExtImageCopyCaptureFrameV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        frame: &frame::ExtImageCopyCaptureFrameV1,
        event: frame::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let c = &mut state.cursor;
        match event {
            frame::Event::Transform { transform }
                if transform
                    != WEnum::Value(wayland_client::protocol::wl_output::Transform::Normal) =>
            {
                c.stopped = true;
                c.visible = false;
                c.changed = true;
            }
            frame::Event::Ready => {
                frame.destroy();
                c.pending = false;
                let result = (|| -> Result<(), CaptureError> {
                    if c.stopped {
                        return Ok(());
                    }
                    if let Some(b) = &mut c.buffer {
                        let d = b.description;
                        let mut pixels = vec![0; (d.stride * d.height) as usize];
                        b.file.seek(SeekFrom::Start(0))?;
                        b.file.read_exact(&mut pixels)?;
                        let shape = crate::cursor::linux_shape(
                            d.width,
                            d.height,
                            d.stride as i32,
                            c.hotspot,
                            matches!(
                                d.format,
                                wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888
                            ),
                            matches!(
                                d.format,
                                wl_shm::Format::Xrgb8888 | wl_shm::Format::Xbgr8888
                            ),
                            &pixels,
                        );
                        if shape.is_some() {
                            c.shape = shape;
                        }
                        c.changed = true;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    state.result = Some(Err(error));
                }
            }
            frame::Event::Failed { .. } => {
                frame.destroy();
                c.pending = false;
                c.stopped = true;
                c.visible = false;
                c.changed = true;
            }
            _ => {}
        }
    }
}
delegate_noop!(CaptureState: ignore wl_pointer::WlPointer);
delegate_noop!(CaptureState: ignore manager::ExtImageCopyCaptureManagerV1);
delegate_noop!(CaptureState: ignore ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(CaptureState: ignore ext_image_capture_source_v1::ExtImageCaptureSourceV1);

impl Drop for CursorCapture {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            session.destroy();
        }
        if let Some(session) = self.cursor_session.take() {
            session.destroy();
        }
        if let Some(buffer) = self.buffer.take() {
            buffer.buffer.destroy();
            buffer._pool.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_requires_negotiation_and_all_protocol_objects() {
        for negotiated in [false, true] {
            for manager in [false, true] {
                for source in [false, true] {
                    for pointer in [false, true] {
                        assert_eq!(
                            metadata_mode(negotiated, manager, source, pointer),
                            negotiated && manager && source && pointer
                        );
                    }
                }
            }
        }
    }
}
