//! Portal ScreenCast capture. SHM is negotiated because the current VAAPI
//! importer only accepts linear single-plane DRM buffers, not portal modifiers.
use crate::capture_linux::CapturedFrame;
use anyhow::{anyhow, bail, Context, Result};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode,
};
use pipewire::{self as pw, properties::properties, spa};
use std::{
    cell::RefCell,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub struct PortalCapture;
impl PortalCapture {
    pub fn run(
        fps: u32,
        metadata_cursor: bool,
        stop: &AtomicBool,
        mut on_frame: impl FnMut(CapturedFrame),
        mut on_cursor: impl FnMut(ferryx_rd_proto::CursorUpdate, Option<ferryx_rd_proto::CursorShape>),
    ) -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let setup_deadline = Instant::now() + Duration::from_secs(120);
            let portal = tokio::time::timeout(Duration::from_secs(10), Screencast::new()).await??;
            let session = tokio::time::timeout(Duration::from_secs(10), portal.create_session()).await??;
            let capture = async {
                let modes = portal.available_cursor_modes().await?;
                let mode = cursor_mode(metadata_cursor, modes.contains(CursorMode::Metadata));
                let path = token_path()?;
                let token = load_token(&path)?;
                portal
                    .select_sources(
                        &session,
                        mode,
                        SourceType::Monitor.into(),
                        false,
                        token.as_deref(),
                        PersistMode::ExplicitlyRevoked,
                    )
                    .await?
                    .response()?;
                let streams = portal.start(&session, None).await?.response()?;
                store_token(&path, streams.restore_token())?;
                let node = streams
                    .streams()
                    .first()
                    .context("portal returned no monitor")?
                    .pipe_wire_node_id();
                let remote = portal.open_pipe_wire_remote(&session).await?;
                run_pipewire(remote, node, fps, stop, &mut on_frame, &mut on_cursor)
            };
            tokio::pin!(capture);
            let result = loop {
                tokio::select! {
                    result = &mut capture => break result,
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {
                        if stop.load(Ordering::Acquire) { break Ok(()); }
                        if Instant::now() >= setup_deadline { break Err(anyhow!("portal consent deadline expired")); }
                    }
                }
            };
            // Closing the portal session revokes this PipeWire node and releases consent.
            let closed = tokio::time::timeout(Duration::from_secs(5), session.close()).await;
            result?;
            closed??;
            Ok(())
        })
    }
}
fn cursor_mode(requested: bool, supported: bool) -> CursorMode {
    if requested && supported {
        CursorMode::Metadata
    } else {
        CursorMode::Embedded
    }
}
fn token_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no configuration directory")?
        .join("maho-rd/portal-restore-token"))
}
fn load_token(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn store_token(path: &Path, token: Option<&str>) -> Result<()> {
    let Some(token) = token else {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    };
    let parent = path.parent().context("token path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(token.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn packed_frame(
    data: &[u8],
    width: u32,
    height: u32,
    stride: i32,
    rgb: bool,
) -> Result<CapturedFrame> {
    let row = (width as usize).checked_mul(4).context("row overflow")?;
    let pitch = stride.unsigned_abs() as usize;
    let len = pitch
        .checked_mul(height as usize)
        .context("frame overflow")?;
    if width == 0 || height == 0 || pitch < row || data.len() < len {
        bail!("invalid PipeWire frame layout");
    }
    let mut bgra = vec![0; row * height as usize];
    for y in 0..height as usize {
        let sy = if stride < 0 {
            height as usize - y - 1
        } else {
            y
        };
        let src = &data[sy * pitch..sy * pitch + row];
        let dst = &mut bgra[y * row..(y + 1) * row];
        dst.copy_from_slice(src);
        for pixel in dst.chunks_exact_mut(4) {
            if rgb {
                pixel.swap(0, 2);
            }
            pixel[3] = 255;
        }
    }
    Ok(CapturedFrame {
        width,
        height,
        stride: row as u32,
        bgra,
        damage: Vec::new(),
        presentation_time_ns: 0,
    })
}
fn rgb_format(format: spa::param::video::VideoFormat) -> Result<bool> {
    use spa::param::video::VideoFormat;
    if format == VideoFormat::BGRx {
        Ok(false)
    } else if format == VideoFormat::RGBx {
        Ok(true)
    } else {
        bail!("unsupported portal video format {format:?}")
    }
}

fn run_pipewire(
    remote: std::os::fd::OwnedFd,
    node: u32,
    fps: u32,
    stop: &AtomicBool,
    on_frame: &mut impl FnMut(CapturedFrame),
    on_cursor: &mut impl FnMut(ferryx_rd_proto::CursorUpdate, Option<ferryx_rd_proto::CursorShape>),
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd_rc(remote, None)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "maho-portal",
        properties! { *pw::keys::MEDIA_TYPE => "Video", *pw::keys::MEDIA_CATEGORY => "Capture", *pw::keys::MEDIA_ROLE => "Screen" },
    )?;
    let frame = Rc::new(RefCell::new(None));
    let cursor = Rc::new(RefCell::new(
        None::<(ferryx_rd_proto::CursorUpdate, Option<ferryx_rd_proto::CursorShape>)>,
    ));
    let cursor_output = cursor.clone();
    let error = Rc::new(RefCell::new(None));
    let output = frame.clone();
    let errors = error.clone();
    let state_errors = error.clone();
    let _listener = stream
        .add_local_listener_with_user_data(spa::param::video::VideoInfoRaw::default())
        .state_changed(move |_, _, _, state| {
            if let pw::stream::StreamState::Error(message) = state {
                *state_errors.borrow_mut() = Some(message.to_string());
            }
        })
        .param_changed(|stream, format, id, param| {
            if id == spa::param::ParamType::Format.as_raw() {
                if let Some(param) = param {
                    if format.parse(param).is_err() {
                        return;
                    }
                    let meta = spa::pod::Object {
                        type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
                        id: spa::param::ParamType::Meta.as_raw(),
                        properties: vec![
                            spa::pod::Property {
                                key: spa::sys::SPA_PARAM_META_type,
                                flags: spa::pod::PropertyFlags::empty(),
                                value: spa::pod::Value::Id(spa::utils::Id(
                                    spa::sys::SPA_META_Cursor,
                                )),
                            },
                            spa::pod::Property {
                                key: spa::sys::SPA_PARAM_META_size,
                                flags: spa::pod::PropertyFlags::empty(),
                                value: spa::pod::Value::Int(CURSOR_META_SIZE as i32),
                            },
                        ],
                    };
                    if let Ok((bytes, _)) = spa::pod::serialize::PodSerializer::serialize(
                        std::io::Cursor::new(Vec::new()),
                        &spa::pod::Value::Object(meta),
                    ) {
                        let bytes = bytes.into_inner();
                        if let Some(pod) = spa::pod::Pod::from_bytes(&bytes) {
                            if let Err(error) = stream.update_params(&mut [pod]) {
                                tracing::warn!(%error, "cursor metadata allocation failed");
                            }
                        }
                    }
                }
            }
        })
        .process(move |stream, format| {
            // The raw buffer remains owned by this callback until it is queued.
            // Read metadata before wrapping the SPA data planes.
            let raw = unsafe { stream.dequeue_raw_buffer() };
            if raw.is_null() {
                return;
            }
            let result =
                unsafe { read_cursor((*raw).buffer, format.size().width, format.size().height) };
            if let Some((position, shape)) = result {
                let mut pending = cursor_output.borrow_mut();
                let shape = shape.or_else(|| pending.as_mut().and_then(|(_, shape)| shape.take()));
                *pending = Some((position, shape));
            }
            let result = (|| {
                let buffer = unsafe { (*raw).buffer.as_mut() }.context("missing SPA buffer")?;
                if buffer.n_datas == 0 || buffer.datas.is_null() {
                    return Ok(None);
                }
                let data = unsafe { &mut *buffer.datas };
                let chunk = unsafe { data.chunk.as_ref() }.context("missing SPA chunk")?;
                if chunk.size == 0 {
                    return Ok(None);
                }
                let offset = chunk.offset as usize;
                let end = offset
                    .checked_add(chunk.size as usize)
                    .context("chunk overflow")?;
                if data.data.is_null() || end > data.maxsize as usize {
                    bail!("invalid mapped SHM chunk");
                }
                let bytes = unsafe {
                    std::slice::from_raw_parts(data.data.cast::<u8>(), data.maxsize as usize)
                };
                let rgb = rgb_format(format.format())?;
                packed_frame(
                    &bytes[offset..end],
                    format.size().width,
                    format.size().height,
                    chunk.stride,
                    rgb,
                )
                .map(Some)
            })();
            unsafe {
                stream.queue_raw_buffer(raw);
            }
            match result {
                Ok(Some(value)) => *output.borrow_mut() = Some(value),
                Ok(None) => {}
                Err(error) => *errors.borrow_mut() = Some(error.to_string()),
            }
        })
        .register()?;
    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBx
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 16384,
                height: 16384
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: fps, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: fps, denom: 1 }
        )
    );
    let bytes = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )?
    .0
    .into_inner();
    stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut [spa::pod::Pod::from_bytes(&bytes).context("invalid format pod")?],
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut received = false;
    while !stop.load(Ordering::Acquire) {
        if mainloop.loop_().iterate(Duration::from_millis(50)) < 0 {
            bail!("PipeWire loop failed");
        }
        if let Some(error) = error.borrow_mut().take() {
            return Err(anyhow!(error));
        }
        if let Some(value) = cursor.borrow_mut().take() {
            on_cursor(value.0, value.1);
        }
        if let Some(value) = frame.borrow_mut().take() {
            received = true;
            on_frame(value);
        }
        if !received && Instant::now() >= deadline {
            bail!("portal first frame deadline expired");
        }
    }
    stream.disconnect()?;
    Ok(())
}

fn cursor_position(bytes: &[u8], width: u32, height: u32) -> Option<ferryx_rd_proto::CursorUpdate> {
    if bytes.len() < 28 || width == 0 || height == 0 {
        return None;
    }
    let id = u32::from_ne_bytes(bytes[0..4].try_into().ok()?);
    if id == 0 {
        return None;
    }
    let x = i32::from_ne_bytes(bytes[8..12].try_into().ok()?);
    let y = i32::from_ne_bytes(bytes[12..16].try_into().ok()?);
    let hidden = x < 0 || y < 0 || x as u32 >= width || y as u32 >= height;
    Some(ferryx_rd_proto::CursorUpdate {
        x: if hidden {
            -1.0
        } else {
            x as f32 / width as f32
        },
        y: if hidden {
            -1.0
        } else {
            y as f32 / height as f32
        },
        cursor_type: u8::from(!hidden),
    })
}

// SPA cursor (28), bitmap (20), then a maximum 256x256 four-byte image.
const CURSOR_META_SIZE: usize = 28 + 20 + 256 * 256 * 4;

fn cursor_bitmap(bytes: &[u8]) -> Option<ferryx_rd_proto::CursorShape> {
    let word = |offset: usize| -> Option<u32> {
        Some(u32::from_ne_bytes(
            bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    let bitmap = word(24)? as usize;
    if bitmap < 28 {
        return None;
    }
    let format = word(bitmap)?;
    let width = word(bitmap.checked_add(4)?)?;
    let height = word(bitmap.checked_add(8)?)?;
    let stride = word(bitmap.checked_add(12)?)? as i32;
    let offset = word(bitmap.checked_add(16)?)? as usize;
    if offset < 20 || width == 0 || height == 0 || width > 256 || height > 256 {
        return None;
    }
    use spa::param::video::VideoFormat;
    let (bgra, opaque) = if format == VideoFormat::BGRA.as_raw() {
        (true, false)
    } else if format == VideoFormat::RGBA.as_raw() {
        (false, false)
    } else if format == VideoFormat::BGRx.as_raw() {
        (true, true)
    } else if format == VideoFormat::RGBx.as_raw() {
        (false, true)
    } else {
        return None;
    };
    let start = bitmap.checked_add(offset)?;
    let end = start.checked_add((width * height * 4) as usize)?;
    crate::cursor::linux_shape(
        width,
        height,
        stride,
        (word(16)? as i32, word(20)? as i32),
        bgra,
        opaque,
        bytes.get(start..end)?,
    )
}

unsafe fn read_cursor(
    buffer: *const spa::sys::spa_buffer,
    width: u32,
    height: u32,
) -> Option<(ferryx_rd_proto::CursorUpdate, Option<ferryx_rd_proto::CursorShape>)> {
    let buffer = unsafe { buffer.as_ref()? };
    if buffer.metas.is_null() {
        return None;
    }
    let metas = unsafe { std::slice::from_raw_parts(buffer.metas, buffer.n_metas as usize) };
    let meta = metas
        .iter()
        .find(|meta| meta.type_ == spa::sys::SPA_META_Cursor)?;
    if meta.data.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(meta.data.cast(), meta.size as usize) };
    Some((cursor_position(bytes, width, height)?, cursor_bitmap(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_metadata_parsing_bounds_and_position() {
        let mut bytes = [0u8; 28];
        assert!(cursor_position(&bytes, 100, 100).is_none());
        bytes[0..4].copy_from_slice(&1u32.to_ne_bytes());
        bytes[8..12].copy_from_slice(&25i32.to_ne_bytes());
        bytes[12..16].copy_from_slice(&75i32.to_ne_bytes());
        let cursor = cursor_position(&bytes, 100, 100).unwrap();
        assert_eq!((cursor.x, cursor.y, cursor.cursor_type), (0.25, 0.75, 1));
        bytes[8..12].copy_from_slice(&(-1i32).to_ne_bytes());
        let hidden = cursor_position(&bytes, 100, 100).unwrap();
        assert_eq!((hidden.x, hidden.y, hidden.cursor_type), (-1.0, -1.0, 0));
        assert!(cursor_position(&bytes[..27], 100, 100).is_none());
        assert!(cursor_position(&bytes, 0, 100).is_none());
    }
    fn bitmap(format: u32) -> Vec<u8> {
        let mut bytes = vec![0; 56];
        for (offset, value) in [
            (0, 1),
            (16, 1),
            (24, 28),
            (28, format),
            (32, 2),
            (36, 1),
            (40, 8),
            (44, 20),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
        }
        bytes[48..].copy_from_slice(&[10, 20, 30, 255, 40, 50, 60, 255]);
        bytes
    }
    #[test]
    fn bitmap_formats_hotspot_and_dedup() {
        use spa::param::video::VideoFormat;
        for (format, expected) in [
            (VideoFormat::RGBA, vec![10, 20, 30, 255, 40, 50, 60, 255]),
            (VideoFormat::BGRA, vec![30, 20, 10, 255, 60, 50, 40, 255]),
            (VideoFormat::RGBx, vec![10, 20, 30, 255, 40, 50, 60, 255]),
            (VideoFormat::BGRx, vec![30, 20, 10, 255, 60, 50, 40, 255]),
        ] {
            let shape = cursor_bitmap(&bitmap(format.as_raw())).unwrap();
            assert_eq!(shape.rgba, expected);
            assert_eq!((shape.hotspot_x, shape.hotspot_y), (1, 0));
            let mut cache = crate::cursor::ShapeCache::default();
            let (id, changed) = cache.update(shape.clone());
            assert!(changed.is_some());
            assert_eq!(cache.update(shape), (id, None));
        }
    }
    #[test]
    fn bitmap_rejects_invalid_and_unchanged_metadata() {
        let valid = bitmap(spa::param::video::VideoFormat::RGBA.as_raw());
        for (offset, value) in [
            (24, 0u32),
            (32, 0),
            (36, 0),
            (32, 257),
            (36, 257),
            (40, 7),
            (40, u32::MAX),
            (44, 19),
            (44, u32::MAX),
            (28, 0),
            (16, 2),
            (20, u32::MAX),
        ] {
            let mut bytes = valid.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
            assert!(
                cursor_bitmap(&bytes).is_none(),
                "offset {offset} value {value}"
            );
        }
        for len in 0..valid.len() {
            assert!(cursor_bitmap(&valid[..len]).is_none());
        }
    }
    #[test]
    fn old_peer_uses_embedded_cursor() {
        assert_eq!(cursor_mode(false, true), CursorMode::Embedded);
        assert_eq!(cursor_mode(true, false), CursorMode::Embedded);
        assert_eq!(cursor_mode(true, true), CursorMode::Metadata);
    }
    #[test]
    fn restore_token_persists_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        assert_eq!(load_token(&path).unwrap(), None);
        store_token(&path, Some("first")).unwrap();
        assert_eq!(load_token(&path).unwrap().as_deref(), Some("first"));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        store_token(&path, Some("second")).unwrap();
        assert_eq!(load_token(&path).unwrap().as_deref(), Some("second"));
        store_token(&path, None).unwrap();
        assert_eq!(load_token(&path).unwrap(), None);
    }
    #[test]
    fn formats_map_only_supported_shm_layouts() {
        use spa::param::video::VideoFormat as F;
        assert!(!rgb_format(F::BGRx).unwrap());
        assert!(rgb_format(F::RGBx).unwrap());
        assert!(rgb_format(F::NV12).is_err());
    }
    #[test]
    fn padded_rgb_and_negative_stride_convert() {
        let pixels = [1, 2, 3, 0, 9, 9, 9, 9, 4, 5, 6, 0, 9, 9, 9, 9];
        assert_eq!(
            packed_frame(&pixels, 1, 2, 8, true).unwrap().bgra,
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
        assert_eq!(
            packed_frame(&pixels, 1, 2, -8, false).unwrap().bgra,
            [4, 5, 6, 255, 1, 2, 3, 255]
        );
        assert!(packed_frame(&pixels, 2, 2, 4, false).is_err());
    }
}
