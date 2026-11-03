//! Shared CPU-frame producer: portal only when screencopy is unavailable at connect.
use super::*;
use crate::capture_linux::CaptureError;

#[derive(Debug)]
enum WlrError {
    Connect(CaptureError),
    Stream(CaptureError),
}

pub(super) fn run(
    handoff: &native_pipeline::Handoff<LinuxRawFrame>,
    sender: &SyncSender<MediaEvent>,
    output_name: Option<String>,
    fps: u32,
    scale_permille: u16,
    metadata_cursor: bool,
) -> Result<(), String> {
    fallback(
        || {
            run_wlr(
                handoff,
                sender,
                output_name,
                fps,
                scale_permille,
                metadata_cursor,
            )
        },
        || handoff.is_stopped(),
        |error| {
            warn!(%error, "screencopy unavailable; requesting portal capture");
            let mut cursor = CursorEmitter::default();
            crate::capture_linux_portal::PortalCapture::run(
                fps,
                metadata_cursor,
                handoff.cancellation(),
                |frame| {
                    publish(handoff, frame, scale_permille);
                },
                |position, shape| cursor.emit(sender, handoff, position, shape),
            )
            .map_err(|error| format!("portal capture: {error:#}"))
        },
    )
}

#[derive(Default)]
struct CursorEmitter {
    cache: crate::cursor::ShapeCache,
    shape_id: Option<u32>,
    pending: Option<CursorShape>,
}
impl CursorEmitter {
    fn emit(
        &mut self,
        sender: &SyncSender<MediaEvent>,
        handoff: &native_pipeline::Handoff<LinuxRawFrame>,
        position: CursorUpdate,
        shape: Option<CursorShape>,
    ) {
        if let Some(shape) = shape {
            let (id, changed) = self.cache.update(shape);
            self.shape_id = Some(id);
            if changed.is_some() {
                self.pending = changed;
            }
        }
        match sender.try_send(MediaEvent::Cursor(
            position,
            self.shape_id,
            self.pending.take(),
        )) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(MediaEvent::Cursor(_, _, shape))) => self.pending = shape,
            Err(mpsc::TrySendError::Disconnected(_)) => handoff.stop(),
            Err(mpsc::TrySendError::Full(_)) => unreachable!("cursor event"),
        }
    }
}

fn fallback(
    wlr: impl FnOnce() -> Result<(), WlrError>,
    stopped: impl FnOnce() -> bool,
    portal: impl FnOnce(CaptureError) -> Result<(), String>,
) -> Result<(), String> {
    match wlr() {
        Err(WlrError::Connect(error @ CaptureError::PortalRequired(_))) if !stopped() => {
            portal(error)
        }
        Err(WlrError::Connect(error) | WlrError::Stream(error)) => Err(error.to_string()),
        Ok(()) => Ok(()),
    }
}

fn run_wlr(
    handoff: &native_pipeline::Handoff<LinuxRawFrame>,
    sender: &SyncSender<MediaEvent>,
    output_name: Option<String>,
    fps: u32,
    scale_permille: u16,
    metadata_cursor: bool,
) -> Result<(), WlrError> {
    let mut capture = LinuxCapture::connect_cancellable(
        LinuxCaptureConfig {
            output_name,
            ..Default::default()
        },
        handoff.cancellation(),
        Instant::now() + Duration::from_secs(5),
    )
    .map_err(WlrError::Connect)?;
    capture.enable_cursor_metadata(metadata_cursor);
    let mut cursor = CursorEmitter::default();
    let interval = Duration::from_micros(1_000_000 / u64::from(fps));
    while !handoff.is_stopped() {
        let started = Instant::now();
        if let Some(frame) = capture
            .capture_frame_cancellable(handoff.cancellation(), Duration::from_millis(50))
            .map_err(WlrError::Stream)?
        {
            publish(handoff, frame, scale_permille);
            handoff.pace_until(started + interval);
        }
        if let Some((position, shape)) = capture.take_cursor() {
            cursor.emit(sender, handoff, position, shape);
        }
    }
    Ok(())
}

fn publish(
    handoff: &native_pipeline::Handoff<LinuxRawFrame>,
    frame: crate::capture_linux::CapturedFrame,
    scale_permille: u16,
) {
    let captured_at = Instant::now();
    let (bgra, stride, width, height) = if scale_permille < 1000 {
        let width =
            (((u64::from(frame.width) * u64::from(scale_permille)) / 1000) & !1).max(2) as u32;
        let height =
            (((u64::from(frame.height) * u64::from(scale_permille)) / 1000) & !1).max(2) as u32;
        let bgra = downscale_bgra_nearest(
            &frame.bgra,
            frame.stride as usize,
            frame.width,
            frame.height,
            width,
            height,
        );
        (bgra, width as usize * 4, width, height)
    } else {
        (frame.bgra, frame.stride as usize, frame.width, frame.height)
    };
    handoff.publish(LinuxRawFrame {
        bgra,
        stride,
        width,
        height,
        captured_at,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn cursor_emitter_retains_shape_under_backpressure_and_deduplicates() {
        let handoff = native_pipeline::Handoff::new();
        let (sender, receiver) = mpsc::sync_channel(1);
        let position = CursorUpdate {
            x: 0.5,
            y: 0.5,
            cursor_type: 1,
        };
        let shape = CursorShape {
            shape_id: 0,
            width: 1,
            height: 1,
            hotspot_x: 0,
            hotspot_y: 0,
            rgba: vec![1, 2, 3, 255],
        };
        sender
            .try_send(MediaEvent::Cursor(position, None, None))
            .unwrap();
        let mut emitter = CursorEmitter::default();
        emitter.emit(&sender, &handoff, position, Some(shape.clone()));
        assert!(emitter.pending.is_some());
        receiver.recv().unwrap();
        emitter.emit(&sender, &handoff, position, None);
        let MediaEvent::Cursor(_, id, changed) = receiver.recv().unwrap() else {
            panic!("cursor event");
        };
        assert_eq!(changed.unwrap().shape_id, id.unwrap());
        emitter.emit(&sender, &handoff, position, Some(shape));
        let MediaEvent::Cursor(_, next_id, changed) = receiver.recv().unwrap() else {
            panic!("cursor event");
        };
        assert_eq!(id, next_id);
        assert!(changed.is_none());
        emitter.emit(&sender, &handoff, position, None);
        let MediaEvent::Cursor(_, next_id, changed) = receiver.recv().unwrap() else {
            panic!("cursor event");
        };
        assert_eq!(id, next_id);
        assert!(changed.is_none());
    }

    #[test]
    fn screencopy_success_never_opens_portal() {
        fallback(|| Ok(()), || false, |_| panic!("portal must not open")).unwrap();
    }

    #[test]
    fn screencopy_failure_precedes_portal() {
        let order = RefCell::new(Vec::new());
        fallback(
            || {
                order.borrow_mut().push("wlr");
                Err(WlrError::Connect(CaptureError::PortalRequired(
                    "screencopy",
                )))
            },
            || false,
            |error| {
                assert!(matches!(error, CaptureError::PortalRequired("screencopy")));
                order.borrow_mut().push("portal");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), ["wlr", "portal"]);
    }

    #[test]
    fn cancelled_screencopy_does_not_open_picker() {
        assert!(fallback(
            || Err(WlrError::Connect(CaptureError::PortalRequired(
                "screencopy"
            ))),
            || true,
            |_| panic!("portal must not open")
        )
        .is_err());
    }

    #[test]
    fn mid_stream_screencopy_error_does_not_open_portal() {
        for error in [
            CaptureError::CaptureFailed,
            CaptureError::PortalRequired("output removed"),
        ] {
            assert!(fallback(
                || Err(WlrError::Stream(error)),
                || false,
                |_| panic!("connected screencopy must not open portal"),
            )
            .is_err());
        }
    }

    #[test]
    fn non_portal_connect_error_does_not_open_portal() {
        assert!(fallback(
            || Err(WlrError::Connect(CaptureError::OutputNotFound(Some(
                "HDMI-A-2".into()
            )))),
            || false,
            |_| panic!("non-protocol connect error must not open portal"),
        )
        .is_err());
    }

    #[test]
    fn portal_error_is_not_hidden() {
        assert_eq!(
            fallback(
                || Err(WlrError::Connect(CaptureError::PortalRequired(
                    "screencopy"
                ))),
                || false,
                |_| Err("denied".into()),
            ),
            Err("denied".into())
        );
    }
}
