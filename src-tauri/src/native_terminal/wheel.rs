use super::composition::{CellMetrics, LogicalBounds};
use super::engine::TerminalEngine;
use super::error::NativeTerminalError;
use super::key::{KeyAction, KeyCode, KeyEvent, KeyModifiers};
use super::mouse::{MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize};
use super::scroll::ScrollViewport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalWheelOutcome {
    WritePty(Vec<u8>),
    ScrollViewport(ScrollViewport),
    None,
}

pub fn compute_wheel_outcome<T: TerminalEngine>(
    term: &T,
    bounds: Option<&LogicalBounds>,
    cell_metrics: Option<&CellMetrics>,
    logical_x: f64,
    logical_y: f64,
    rows: i16,
    modifiers: KeyModifiers,
) -> Result<TerminalWheelOutcome, NativeTerminalError> {
    if rows == 0 {
        return Ok(TerminalWheelOutcome::None);
    }

    let has_shift = modifiers.shift;
    let tracking = term.mouse_tracking_enabled().unwrap_or(false);
    let is_alt = term.is_alternate_screen().unwrap_or(false);

    if tracking && !has_shift {
        let button = if rows < 0 {
            MouseButton::Four
        } else {
            MouseButton::Five
        };

        let mut mouse_event = MouseEvent {
            action: MouseAction::Press,
            button: Some(button),
            position: MousePosition::default(),
            modifiers,
            timestamp_ns: None,
            size: None,
        };

        if let (Some(b), Some(cm)) = (bounds, cell_metrics) {
            let scale = b.scale_factor as f32;
            let rel_x = ((logical_x - b.x).max(0.0) as f32) * scale;
            let rel_y = ((logical_y - b.y).max(0.0) as f32) * scale;
            mouse_event.position = MousePosition { x: rel_x, y: rel_y };
            let phys_w = (b.width * b.scale_factor).round() as u32;
            let phys_h = (b.height * b.scale_factor).round() as u32;
            mouse_event.size = Some(MouseRendererSize {
                screen_width: phys_w,
                screen_height: phys_h,
                cell_width: cm.width_px,
                cell_height: cm.height_px,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            });
        }

        let one_tick = term.encode_mouse(&mouse_event)?;
        if one_tick.is_empty() {
            return Ok(TerminalWheelOutcome::None);
        }
        let ticks = (rows.unsigned_abs() as usize).clamp(1, 5);
        let mut bytes = Vec::with_capacity(one_tick.len() * ticks);
        for _ in 0..ticks {
            bytes.extend_from_slice(&one_tick);
        }
        return Ok(TerminalWheelOutcome::WritePty(bytes));
    }

    if is_alt && !has_shift {
        let key = if rows < 0 {
            KeyCode::ArrowUp
        } else {
            KeyCode::ArrowDown
        };
        let key_event = KeyEvent::new(key, KeyAction::Press);
        let one_tick = term.encode_key(&key_event)?;
        if one_tick.is_empty() {
            return Ok(TerminalWheelOutcome::None);
        }
        let ticks = (rows.unsigned_abs() as usize).clamp(1, 5);
        let mut bytes = Vec::with_capacity(one_tick.len() * ticks);
        for _ in 0..ticks {
            bytes.extend_from_slice(&one_tick);
        }
        return Ok(TerminalWheelOutcome::WritePty(bytes));
    }

    Ok(TerminalWheelOutcome::ScrollViewport(ScrollViewport::Delta(rows as isize)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::terminal::NativeTerminal;

    #[test]
    fn test_wheel_zero_rows_returns_none() {
        let terminal = NativeTerminal::new(80, 24).expect("terminal");
        let outcome = compute_wheel_outcome(
            &terminal,
            None,
            None,
            0.0,
            0.0,
            0,
            KeyModifiers::default(),
        )
        .expect("outcome");
        assert_eq!(outcome, TerminalWheelOutcome::None);
    }

    #[test]
    fn test_wheel_with_mouse_tracking_encodes_sgr_wheel() {
        let mut terminal = NativeTerminal::new(80, 24).expect("terminal");
        terminal
            .feed(b"\x1b[?1000h\x1b[?1006h")
            .expect("enable mouse");
        assert!(terminal.mouse_tracking_enabled().expect("tracking"));

        let outcome_up = compute_wheel_outcome(
            &terminal,
            None,
            None,
            10.0,
            20.0,
            -1,
            KeyModifiers::default(),
        )
        .expect("outcome up");
        match outcome_up {
            TerminalWheelOutcome::WritePty(bytes) => {
                let s = String::from_utf8_lossy(&bytes);
                assert!(
                    s.starts_with("\x1b[<64;"),
                    "Wheel Up must encode SGR button 64, got {s:?}"
                );
            }
            other => panic!("expected WritePty for mouse tracking wheel up, got {other:?}"),
        }

        let outcome_down = compute_wheel_outcome(
            &terminal,
            None,
            None,
            10.0,
            20.0,
            1,
            KeyModifiers::default(),
        )
        .expect("outcome down");
        match outcome_down {
            TerminalWheelOutcome::WritePty(bytes) => {
                let s = String::from_utf8_lossy(&bytes);
                assert!(
                    s.starts_with("\x1b[<65;"),
                    "Wheel Down must encode SGR button 65, got {s:?}"
                );
            }
            other => panic!("expected WritePty for mouse tracking wheel down, got {other:?}"),
        }
    }

    #[test]
    fn test_wheel_with_mouse_tracking_shift_overrides_to_viewport() {
        let mut terminal = NativeTerminal::new(80, 24).expect("terminal");
        terminal
            .feed(b"\x1b[?1000h\x1b[?1006h")
            .expect("enable mouse");

        let shift_mods = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        let outcome = compute_wheel_outcome(
            &terminal,
            None,
            None,
            10.0,
            20.0,
            -3,
            shift_mods,
        )
        .expect("outcome");
        assert_eq!(
            outcome,
            TerminalWheelOutcome::ScrollViewport(ScrollViewport::Delta(-3))
        );
    }

    #[test]
    fn test_wheel_alternate_screen_encodes_arrow_keys() {
        let mut terminal = NativeTerminal::new(80, 24).expect("terminal");
        terminal.feed(b"\x1b[?1049h").expect("enter alt screen");
        assert!(terminal.is_alternate_screen().expect("is alt"));
        assert!(!terminal.mouse_tracking_enabled().expect("tracking"));

        let outcome_up = compute_wheel_outcome(
            &terminal,
            None,
            None,
            0.0,
            0.0,
            -2,
            KeyModifiers::default(),
        )
        .expect("outcome up");
        match outcome_up {
            TerminalWheelOutcome::WritePty(bytes) => {
                let s = String::from_utf8_lossy(&bytes);
                assert!(
                    s.contains("\x1b[A") || s.contains("\x1bOA"),
                    "expected ArrowUp escape sequence, got {s:?}"
                );
            }
            other => panic!("expected WritePty for alternate screen wheel up, got {other:?}"),
        }

        let outcome_down = compute_wheel_outcome(
            &terminal,
            None,
            None,
            0.0,
            0.0,
            2,
            KeyModifiers::default(),
        )
        .expect("outcome down");
        match outcome_down {
            TerminalWheelOutcome::WritePty(bytes) => {
                let s = String::from_utf8_lossy(&bytes);
                assert!(
                    s.contains("\x1b[B") || s.contains("\x1bOB"),
                    "expected ArrowDown escape sequence, got {s:?}"
                );
            }
            other => panic!("expected WritePty for alternate screen wheel down, got {other:?}"),
        }
    }

    #[test]
    fn test_wheel_alternate_screen_shift_overrides_to_viewport() {
        let mut terminal = NativeTerminal::new(80, 24).expect("terminal");
        terminal.feed(b"\x1b[?1049h").expect("enter alt screen");

        let shift_mods = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        let outcome = compute_wheel_outcome(
            &terminal,
            None,
            None,
            0.0,
            0.0,
            2,
            shift_mods,
        )
        .expect("outcome");
        assert_eq!(
            outcome,
            TerminalWheelOutcome::ScrollViewport(ScrollViewport::Delta(2))
        );
    }

    #[test]
    fn test_wheel_primary_screen_returns_viewport() {
        let terminal = NativeTerminal::new(80, 24).expect("terminal");
        assert!(!terminal.is_alternate_screen().expect("is alt"));

        let outcome = compute_wheel_outcome(
            &terminal,
            None,
            None,
            0.0,
            0.0,
            -3,
            KeyModifiers::default(),
        )
        .expect("outcome");
        assert_eq!(
            outcome,
            TerminalWheelOutcome::ScrollViewport(ScrollViewport::Delta(-3))
        );
    }
}
