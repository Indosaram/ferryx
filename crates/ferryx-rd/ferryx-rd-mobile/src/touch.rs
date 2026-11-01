use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Began,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchPoint {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub phase: TouchPhase,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportState {
    pub view_width: f32,
    pub view_height: f32,
    pub zoom: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            view_width: 1280.0,
            view_height: 800.0,
            zoom: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum TouchError {
    #[error("non-finite touch coordinates: x={0}, y={1}")]
    NonFiniteCoordinates(f32, f32),
    #[error("invalid viewport dimension: width={0}, height={1}")]
    InvalidViewportDimensions(f32, f32),
    #[error("invalid zoom factor: {0}")]
    InvalidZoom(f32),
}

impl ViewportState {
    pub fn new(view_width: f32, view_height: f32) -> Result<Self, TouchError> {
        if !view_width.is_finite()
            || !view_height.is_finite()
            || view_width <= 0.0
            || view_height <= 0.0
        {
            return Err(TouchError::InvalidViewportDimensions(
                view_width,
                view_height,
            ));
        }
        Ok(Self {
            view_width,
            view_height,
            zoom: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        })
    }

    pub fn set_zoom(&mut self, zoom: f32, center_x: f32, center_y: f32) -> Result<(), TouchError> {
        if !zoom.is_finite() || !(0.1..=10.0).contains(&zoom) {
            return Err(TouchError::InvalidZoom(zoom));
        }
        if !center_x.is_finite() || !center_y.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(center_x, center_y));
        }
        let old_zoom = self.zoom;
        if !old_zoom.is_finite() || old_zoom <= 0.0 {
            return Err(TouchError::InvalidZoom(old_zoom));
        }

        let factor = zoom / old_zoom;
        let new_offset_x = center_x - factor * (center_x - self.offset_x);
        let new_offset_y = center_y - factor * (center_y - self.offset_y);
        if !new_offset_x.is_finite() || !new_offset_y.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(new_offset_x, new_offset_y));
        }

        self.zoom = zoom;
        self.offset_x = new_offset_x;
        self.offset_y = new_offset_y;
        Ok(())
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            let next_x = self.offset_x + dx;
            let next_y = self.offset_y + dy;
            if next_x.is_finite() && next_y.is_finite() {
                self.offset_x = next_x;
                self.offset_y = next_y;
            }
        }
    }

    pub fn transform_to_host(&self, x: f32, y: f32) -> Result<(f32, f32), TouchError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(x, y));
        }
        if !self.view_width.is_finite()
            || !self.view_height.is_finite()
            || self.view_width <= 0.0
            || self.view_height <= 0.0
        {
            return Err(TouchError::InvalidViewportDimensions(
                self.view_width,
                self.view_height,
            ));
        }
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            return Err(TouchError::InvalidZoom(self.zoom));
        }
        if !self.offset_x.is_finite() || !self.offset_y.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(
                self.offset_x,
                self.offset_y,
            ));
        }

        let diff_x = x - self.offset_x;
        let diff_y = y - self.offset_y;
        if !diff_x.is_finite() || !diff_y.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(diff_x, diff_y));
        }

        let unzoomed_x = diff_x / self.zoom;
        let unzoomed_y = diff_y / self.zoom;
        if !unzoomed_x.is_finite() || !unzoomed_y.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(unzoomed_x, unzoomed_y));
        }

        let norm_x_raw = unzoomed_x / self.view_width;
        let norm_y_raw = unzoomed_y / self.view_height;
        if !norm_x_raw.is_finite() || !norm_y_raw.is_finite() {
            return Err(TouchError::NonFiniteCoordinates(norm_x_raw, norm_y_raw));
        }

        let norm_x = norm_x_raw.clamp(0.0, 1.0);
        let norm_y = (1.0 - norm_y_raw).clamp(0.0, 1.0);

        Ok((norm_x, norm_y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchMode {
    DirectTouch,
    TrackpadRelative,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveDirectTouch {
    id: u64,
    last_emitted_norm_x: f32,
    last_emitted_norm_y: f32,
}

fn make_mouse_event(event_type: InputEventType, x: f32, y: f32) -> InputEvent {
    InputEvent {
        event_type,
        x,
        y,
        key_code: 0,
        modifiers: Modifiers::empty(),
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    }
}

fn make_relative_event(scroll_dx: f32, scroll_dy: f32) -> InputEvent {
    InputEvent {
        event_type: InputEventType::RelativeMove,
        x: 0.0,
        y: 0.0,
        key_code: 0,
        modifiers: Modifiers::empty(),
        scroll_dx,
        scroll_dy,
    }
}

pub struct TouchGestureHandler {
    viewport: ViewportState,
    mode: TouchMode,
    primary_touch: Option<ActiveDirectTouch>,
    relative_baseline: Option<(u64, f32, f32)>,
}

impl TouchGestureHandler {
    pub fn new(viewport: ViewportState, mode: TouchMode) -> Self {
        Self {
            viewport,
            mode,
            primary_touch: None,
            relative_baseline: None,
        }
    }

    pub fn viewport_mut(&mut self) -> &mut ViewportState {
        &mut self.viewport
    }

    pub fn set_mode(&mut self, mode: TouchMode) -> Option<InputEvent> {
        if self.mode == mode {
            return None;
        }

        let release_evt = if self.mode == TouchMode::DirectTouch {
            self.primary_touch.take().map(|primary| {
                make_mouse_event(
                    InputEventType::LeftMouseUp,
                    primary.last_emitted_norm_x,
                    primary.last_emitted_norm_y,
                )
            })
        } else {
            None
        };

        self.mode = mode;
        self.primary_touch = None;
        self.relative_baseline = None;
        release_evt
    }

    pub fn process_touch(&mut self, touch: TouchPoint) -> Result<Option<InputEvent>, TouchError> {
        if touch.phase == TouchPhase::Cancelled {
            if self.mode == TouchMode::DirectTouch {
                if let Some(primary) = self.primary_touch {
                    if primary.id == touch.id {
                        self.primary_touch = None;
                        return Ok(Some(make_mouse_event(
                            InputEventType::LeftMouseUp,
                            primary.last_emitted_norm_x,
                            primary.last_emitted_norm_y,
                        )));
                    }
                }
            } else if self.mode == TouchMode::TrackpadRelative {
                if let Some((prev_id, _, _)) = self.relative_baseline {
                    if prev_id == touch.id {
                        self.relative_baseline = None;
                    }
                }
            }
            return Ok(None);
        }

        // Began and Moved coordinates are validated because they are emitted
        // verbatim to the host. Ended (like Cancelled above) must always
        // release the touch even with non-finite coordinates: a viewport that
        // temporarily collapses (e.g. during rotation) can produce NaN, and
        // swallowing the release would leave the host mouse button stuck down
        // and block every future touch. Ended falls back to the last emitted
        // host coordinates instead.
        if matches!(touch.phase, TouchPhase::Began | TouchPhase::Moved)
            && (!touch.x.is_finite() || !touch.y.is_finite())
        {
            return Err(TouchError::NonFiniteCoordinates(touch.x, touch.y));
        }

        match touch.phase {
            TouchPhase::Began => {
                if self.mode == TouchMode::DirectTouch {
                    if self.primary_touch.is_none() {
                        let (norm_x, norm_y) = self.viewport.transform_to_host(touch.x, touch.y)?;
                        self.primary_touch = Some(ActiveDirectTouch {
                            id: touch.id,
                            last_emitted_norm_x: norm_x,
                            last_emitted_norm_y: norm_y,
                        });
                        return Ok(Some(make_mouse_event(
                            InputEventType::LeftMouseDown,
                            norm_x,
                            norm_y,
                        )));
                    }
                } else if self.mode == TouchMode::TrackpadRelative
                    && self.relative_baseline.is_none()
                {
                    self.relative_baseline = Some((touch.id, touch.x, touch.y));
                }
            }
            TouchPhase::Moved => {
                if self.mode == TouchMode::DirectTouch {
                    if let Some(ref mut primary) = self.primary_touch {
                        if primary.id == touch.id {
                            let (norm_x, norm_y) =
                                self.viewport.transform_to_host(touch.x, touch.y)?;
                            primary.last_emitted_norm_x = norm_x;
                            primary.last_emitted_norm_y = norm_y;
                            return Ok(Some(make_mouse_event(
                                InputEventType::LeftMouseDragged,
                                norm_x,
                                norm_y,
                            )));
                        }
                    }
                } else if self.mode == TouchMode::TrackpadRelative {
                    if let Some((prev_id, prev_x, prev_y)) = self.relative_baseline {
                        if prev_id == touch.id {
                            let dx = touch.x - prev_x;
                            let dy = touch.y - prev_y;
                            if !dx.is_finite() || !dy.is_finite() {
                                return Err(TouchError::NonFiniteCoordinates(dx, dy));
                            }
                            self.relative_baseline = Some((touch.id, touch.x, touch.y));
                            return Ok(Some(make_relative_event(dx, dy)));
                        }
                    }
                }
            }
            TouchPhase::Ended => {
                if self.mode == TouchMode::DirectTouch {
                    if let Some(primary) = self.primary_touch {
                        if primary.id == touch.id {
                            self.primary_touch = None;
                            let (release_x, release_y) = self
                                .viewport
                                .transform_to_host(touch.x, touch.y)
                                .unwrap_or((
                                    primary.last_emitted_norm_x,
                                    primary.last_emitted_norm_y,
                                ));
                            return Ok(Some(make_mouse_event(
                                InputEventType::LeftMouseUp,
                                release_x,
                                release_y,
                            )));
                        }
                    }
                } else if self.mode == TouchMode::TrackpadRelative {
                    if let Some((prev_id, _, _)) = self.relative_baseline {
                        if prev_id == touch.id {
                            self.relative_baseline = None;
                        }
                    }
                }
            }
            TouchPhase::Cancelled => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_ended_release_clears_primary_touch_and_emits_mouse_up() {
        let vp = ViewportState::new(800.0, 600.0).unwrap();
        let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);
        let down = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 400.0,
                y: 300.0,
                phase: TouchPhase::Began,
            })
            .unwrap()
            .unwrap();
        assert_eq!(down.event_type, InputEventType::LeftMouseDown);

        // The client dispatches the release with NaN coordinates after the
        // viewport collapsed: the release must still emit MouseUp at the last
        // emitted host coordinates and clear the primary touch.
        let up = handler
            .process_touch(TouchPoint {
                id: 1,
                x: f32::NAN,
                y: f32::NAN,
                phase: TouchPhase::Ended,
            })
            .unwrap()
            .unwrap();
        assert_eq!(up.event_type, InputEventType::LeftMouseUp);
        assert_eq!((up.x, up.y), (0.5, 0.5));

        // A later touch must be accepted again instead of being dropped.
        let next = handler
            .process_touch(TouchPoint {
                id: 2,
                x: 400.0,
                y: 300.0,
                phase: TouchPhase::Began,
            })
            .unwrap()
            .unwrap();
        assert_eq!(next.event_type, InputEventType::LeftMouseDown);
    }

    #[test]
    fn viewport_transform_at_unit_zoom() {
        let vp = ViewportState::new(1000.0, 500.0).unwrap();
        let (x, y) = vp.transform_to_host(0.0, 0.0).unwrap();
        assert_eq!((x, y), (0.0, 1.0));

        let (x, y) = vp.transform_to_host(500.0, 250.0).unwrap();
        assert_eq!((x, y), (0.5, 0.5));

        let (x, y) = vp.transform_to_host(1000.0, 500.0).unwrap();
        assert_eq!((x, y), (1.0, 0.0));
    }

    #[test]
    fn viewport_transform_with_zoom_and_pan() {
        let mut vp = ViewportState::new(1000.0, 500.0).unwrap();
        vp.set_zoom(2.0, 500.0, 250.0).unwrap();
        assert_eq!(vp.zoom, 2.0);

        let (x, y) = vp.transform_to_host(500.0, 250.0).unwrap();
        assert!((x - 0.5).abs() < 1e-5);
        assert!((y - 0.5).abs() < 1e-5);
    }

    #[test]
    fn gesture_handler_direct_touch_emits_mouse_down_drag_up() {
        let vp = ViewportState::new(800.0, 600.0).unwrap();
        let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);

        let down = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 400.0,
                y: 300.0,
                phase: TouchPhase::Began,
            })
            .unwrap()
            .unwrap();
        assert_eq!(down.event_type, InputEventType::LeftMouseDown);
        assert_eq!((down.x, down.y), (0.5, 0.5));

        let drag = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 600.0,
                y: 300.0,
                phase: TouchPhase::Moved,
            })
            .unwrap()
            .unwrap();
        assert_eq!(drag.event_type, InputEventType::LeftMouseDragged);
        assert_eq!((drag.x, drag.y), (0.75, 0.5));

        let up = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 600.0,
                y: 300.0,
                phase: TouchPhase::Ended,
            })
            .unwrap()
            .unwrap();
        assert_eq!(up.event_type, InputEventType::LeftMouseUp);
    }

    #[test]
    fn gesture_handler_trackpad_relative_emits_relative_move() {
        let vp = ViewportState::new(800.0, 600.0).unwrap();
        let mut handler = TouchGestureHandler::new(vp, TouchMode::TrackpadRelative);

        assert!(handler
            .process_touch(TouchPoint {
                id: 1,
                x: 100.0,
                y: 100.0,
                phase: TouchPhase::Began,
            })
            .unwrap()
            .is_none());

        let rel = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 125.0,
                y: 110.0,
                phase: TouchPhase::Moved,
            })
            .unwrap()
            .unwrap();
        assert_eq!(rel.event_type, InputEventType::RelativeMove);
        assert_eq!(rel.scroll_dx, 25.0);
        assert_eq!(rel.scroll_dy, 10.0);
    }

    #[test]
    fn gesture_handler_mode_change_releases_held_drag() {
        let vp = ViewportState::new(800.0, 600.0).unwrap();
        let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);

        let down = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 400.0,
                y: 300.0,
                phase: TouchPhase::Began,
            })
            .unwrap()
            .unwrap();
        assert_eq!(down.event_type, InputEventType::LeftMouseDown);

        let release = handler.set_mode(TouchMode::TrackpadRelative);
        assert!(release.is_some());
        let rel_evt = release.unwrap();
        assert_eq!(rel_evt.event_type, InputEventType::LeftMouseUp);
        assert_eq!((rel_evt.x, rel_evt.y), (0.5, 0.5));
    }

    #[test]
    fn gesture_handler_same_mode_preserves_drag() {
        let vp = ViewportState::new(800.0, 600.0).unwrap();
        let mut handler = TouchGestureHandler::new(vp, TouchMode::DirectTouch);

        let down = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 400.0,
                y: 300.0,
                phase: TouchPhase::Began,
            })
            .unwrap()
            .unwrap();
        assert_eq!(down.event_type, InputEventType::LeftMouseDown);

        let no_release = handler.set_mode(TouchMode::DirectTouch);
        assert!(no_release.is_none());

        let drag = handler
            .process_touch(TouchPoint {
                id: 1,
                x: 600.0,
                y: 300.0,
                phase: TouchPhase::Moved,
            })
            .unwrap()
            .unwrap();
        assert_eq!(drag.event_type, InputEventType::LeftMouseDragged);
        assert_eq!((drag.x, drag.y), (0.75, 0.5));
    }
}
