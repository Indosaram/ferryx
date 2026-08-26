use std::cell::RefCell;

use super::composition::{CellMetrics, SurfaceCompositionLayout};
use super::cursor::CursorVisualStyle;
use super::engine::TerminalEngine;
use super::error::NativeTerminalError;
use super::key::{KeyAction, KeyCode, KeyEvent, KeyModifiers};
use super::snapshot::RenderSnapshot;
use super::terminal::NativeTerminal;

const MAX_NATIVE_INPUT_BYTES: usize = 8192;

/// Browser key event DTO matching frontend payload shape.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserKeyEventDto {
    pub key: String,
    pub action: KeyAction,
    pub modifiers: KeyModifiers,
    pub utf8: Option<String>,
}

pub(crate) fn parse_browser_key(key: &str) -> Result<KeyCode, NativeTerminalError> {
    match key {
        "Enter" => Ok(KeyCode::Enter),
        "Tab" => Ok(KeyCode::Tab),
        "Backspace" => Ok(KeyCode::Backspace),
        "Escape" => Ok(KeyCode::Escape),
        "Space" | " " => Ok(KeyCode::Space),
        "ArrowUp" => Ok(KeyCode::ArrowUp),
        "ArrowDown" => Ok(KeyCode::ArrowDown),
        "ArrowLeft" => Ok(KeyCode::ArrowLeft),
        "ArrowRight" => Ok(KeyCode::ArrowRight),
        "Home" => Ok(KeyCode::Home),
        "End" => Ok(KeyCode::End),
        "PageUp" => Ok(KeyCode::PageUp),
        "PageDown" => Ok(KeyCode::PageDown),
        "Insert" => Ok(KeyCode::Insert),
        "Delete" => Ok(KeyCode::Delete),
        "F1" => Ok(KeyCode::F1),
        "F2" => Ok(KeyCode::F2),
        "F3" => Ok(KeyCode::F3),
        "F4" => Ok(KeyCode::F4),
        "F5" => Ok(KeyCode::F5),
        "F6" => Ok(KeyCode::F6),
        "F7" => Ok(KeyCode::F7),
        "F8" => Ok(KeyCode::F8),
        "F9" => Ok(KeyCode::F9),
        "F10" => Ok(KeyCode::F10),
        "F11" => Ok(KeyCode::F11),
        "F12" => Ok(KeyCode::F12),
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(KeyCode::Character(c)),
                _ => Err(NativeTerminalError::InvalidValue(format!(
                    "Unsupported browser key: {other}"
                ))),
            }
        }
    }
}

impl TryFrom<BrowserKeyEventDto> for KeyEvent {
    type Error = NativeTerminalError;

    fn try_from(dto: BrowserKeyEventDto) -> Result<Self, Self::Error> {
        let key = parse_browser_key(&dto.key)?;
        Ok(Self {
            key,
            action: dto.action,
            modifiers: dto.modifiers,
            utf8: dto.utf8,
        })
    }
}

impl BrowserKeyEventDto {
    pub fn to_key_event(&self) -> Result<KeyEvent, NativeTerminalError> {
        let key = parse_browser_key(&self.key)?;
        Ok(KeyEvent {
            key,
            action: self.action,
            modifiers: self.modifiers,
            utf8: self.utf8.clone(),
        })
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum NativeTerminalInput {
    KeyEvent {
        #[serde(rename = "keyEvent")]
        key_event: BrowserKeyEventDto,
    },
    Text {
        text: String,
    },
}

impl NativeTerminalInput {
    pub(crate) fn encoded(
        &self,
        terminal: &NativeTerminal,
    ) -> Result<Vec<u8>, NativeTerminalError> {
        match self {
            Self::KeyEvent { key_event } => {
                let option_as_alt =
                    crate::terminal::preferences::cached_terminal_preferences().macos_option_as_alt;
                terminal.encode_key_with_option_as_alt(&key_event.to_key_event()?, option_as_alt)
            }
            Self::Text { text } => {
                if text.is_empty() {
                    return Err(NativeTerminalError::InvalidValue(
                        "Native terminal input text must not be empty".into(),
                    ));
                }
                if text.len() > MAX_NATIVE_INPUT_BYTES {
                    return Err(NativeTerminalError::LimitExceeded);
                }
                Ok(text.as_bytes().to_vec())
            }
        }
    }
}

#[allow(dead_code)]
pub struct NativeTerminalInputState {
    terminal: Option<NativeTerminal>,
    focused: bool,
}

impl Default for NativeTerminalInputState {
    fn default() -> Self {
        Self {
            terminal: None,
            focused: false,
        }
    }
}

#[allow(dead_code)]
impl NativeTerminalInputState {
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn feed(
        &mut self,
        layout: SurfaceCompositionLayout,
        input: &NativeTerminalInput,
        cell_metrics: CellMetrics,
    ) -> Result<RenderSnapshot, NativeTerminalError> {
        let focused = self.focused;
        let terminal = self.ensure_terminal(layout, cell_metrics)?;
        let encoded = input.encoded(terminal)?;
        terminal.feed(&encoded)?;
        let mut snapshot = terminal.render_snapshot()?;
        snapshot.cursor.visual_style = cursor_style_for_focus(focused);
        Ok(snapshot)
    }

    pub(crate) fn snapshot(&self) -> Result<Option<RenderSnapshot>, NativeTerminalError> {
        let Some(terminal) = &self.terminal else {
            return Ok(None);
        };
        let mut snapshot = terminal.render_snapshot()?;
        snapshot.cursor.visual_style = cursor_style_for_focus(self.focused);
        Ok(Some(snapshot))
    }

    pub(crate) fn snapshot_for_layout(
        &mut self,
        layout: SurfaceCompositionLayout,
        cell_metrics: CellMetrics,
    ) -> Result<Option<RenderSnapshot>, NativeTerminalError> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(None);
        };
        if terminal.dimensions()? != (layout.cols, layout.rows) {
            terminal.resize(
                layout.cols,
                layout.rows,
                cell_metrics.width_px,
                cell_metrics.height_px,
            )?;
        }
        let mut snapshot = terminal.render_snapshot()?;
        snapshot.cursor.visual_style = cursor_style_for_focus(self.focused);
        Ok(Some(snapshot))
    }

    fn ensure_terminal(
        &mut self,
        layout: SurfaceCompositionLayout,
        cell_metrics: CellMetrics,
    ) -> Result<&mut NativeTerminal, NativeTerminalError> {
        match self.terminal.as_mut() {
            Some(terminal) => {
                if terminal.dimensions()? != (layout.cols, layout.rows) {
                    terminal.resize(
                        layout.cols,
                        layout.rows,
                        cell_metrics.width_px,
                        cell_metrics.height_px,
                    )?;
                }
            }
            None => {
                self.terminal = Some(NativeTerminal::new(layout.cols, layout.rows)?);
            }
        }

        self.terminal.as_mut().ok_or(NativeTerminalError::NoValue)
    }
}

#[allow(dead_code)]
struct NativeTerminalInputSlot {
    session_id: String,
    state: NativeTerminalInputState,
}

thread_local! {
    static LOCAL_INPUT_STATE: RefCell<Option<NativeTerminalInputSlot>> = const { RefCell::new(None) };
}

#[allow(dead_code)]
pub(crate) fn with_local_input_state<T>(
    session_id: &str,
    action: impl FnOnce(&mut NativeTerminalInputState) -> Result<T, NativeTerminalError>,
) -> Result<T, NativeTerminalError> {
    LOCAL_INPUT_STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|existing| existing.session_id != session_id)
        {
            *slot = Some(NativeTerminalInputSlot {
                session_id: session_id.into(),
                state: NativeTerminalInputState::default(),
            });
        }

        let state = slot.as_mut().ok_or(NativeTerminalError::NoValue)?;
        action(&mut state.state)
    })
}

pub(crate) fn cursor_style_for_focus(focused: bool) -> CursorVisualStyle {
    if focused {
        CursorVisualStyle::Block
    } else {
        CursorVisualStyle::BlockHollow
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeTerminalInput, NativeTerminalInputState};
    use crate::native_terminal::composition::{
        CellMetrics, LogicalBounds, SurfaceCompositionLayout,
    };
    use crate::native_terminal::key::{KeyAction, KeyCode, KeyModifiers};
    use crate::native_terminal::{CellWide, CursorVisualStyle};

    fn test_layout(metrics: &CellMetrics) -> SurfaceCompositionLayout {
        SurfaceCompositionLayout::compute(
            &LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
                scale_factor: 1.0,
            },
            metrics,
        )
        .expect("valid input probe bounds")
    }

    #[test]
    fn local_ime_input_updates_real_terminal_snapshot_with_wide_cells() {
        let cell_metrics = CellMetrics {
            width_px: 10,
            height_px: 20,
        };
        let layout = test_layout(&cell_metrics);
        let mut state = NativeTerminalInputState::default();
        state.set_focused(true);

        let snapshot = state
            .feed(
                layout,
                &NativeTerminalInput::Text {
                    text: "한국".into(),
                },
                cell_metrics,
            )
            .expect("local IME input should render through the native terminal");

        assert_eq!(snapshot.cursor.x, 4);
        assert_eq!(snapshot.cursor.y, 0);
        assert!(snapshot.row_text(0).contains("한국"));
        assert_eq!(snapshot.grid[0][0].text, "한");
        assert_eq!(snapshot.grid[0][0].wide, CellWide::Wide);
        assert_eq!(snapshot.grid[0][1].wide, CellWide::SpacerTail);
        assert_eq!(snapshot.grid[0][2].text, "국");
        assert_eq!(snapshot.grid[0][2].wide, CellWide::Wide);
        assert_eq!(snapshot.grid[0][3].wide, CellWide::SpacerTail);
        assert_eq!(snapshot.cursor.visual_style, CursorVisualStyle::Block);

        state.set_focused(false);
        let unfocused = state.snapshot().unwrap().unwrap();
        assert_eq!(
            unfocused.cursor.visual_style,
            CursorVisualStyle::BlockHollow
        );
    }

    #[test]
    fn local_input_snapshot_survives_layout_resize() {
        let cell_metrics = CellMetrics {
            width_px: 10,
            height_px: 20,
        };
        let initial_layout = test_layout(&cell_metrics);
        let resized_layout = SurfaceCompositionLayout::compute(
            &LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 480.0,
                scale_factor: 1.0,
            },
            &cell_metrics,
        )
        .expect("valid resized input probe bounds");
        let mut state = NativeTerminalInputState::default();

        state
            .feed(
                initial_layout,
                &NativeTerminalInput::Text {
                    text: "preserved".into(),
                },
                cell_metrics,
            )
            .expect("local input should render before resize");

        let snapshot = state
            .snapshot_for_layout(resized_layout, cell_metrics)
            .expect("existing local input should resize")
            .expect("existing terminal should provide a snapshot");

        assert_eq!(snapshot.cols, resized_layout.cols);
        assert_eq!(snapshot.rows, resized_layout.rows);
        assert!(snapshot.row_text(0).starts_with("preserved"));
    }

    #[test]
    fn ctrl_c_frontend_payload_deserializes_and_encodes_control_character() {
        let ctrl_c_json = r#"{
            "keyEvent": {
                "key": "c",
                "action": "Press",
                "modifiers": {
                    "shift": false,
                    "ctrl": true,
                    "alt": false,
                    "superKey": false,
                    "capsLock": false,
                    "numLock": false
                },
                "utf8": null
            }
        }"#;

        let input: NativeTerminalInput = serde_json::from_str(ctrl_c_json)
            .expect("deserialize Ctrl+C frontend keyEvent payload");

        let cell_metrics = CellMetrics {
            width_px: 10,
            height_px: 20,
        };
        let layout = test_layout(&cell_metrics);
        let mut state = NativeTerminalInputState::default();
        state.set_focused(true);

        let snapshot = state
            .feed(layout, &input, cell_metrics)
            .expect("Ctrl+C input should feed into terminal state");
        assert_eq!(snapshot.cursor.visual_style, CursorVisualStyle::Block);
    }

    #[test]
    fn native_terminal_input_deserializes_from_frontend_payloads() {
        let text_json = r#"{"text": "안녕"}"#;
        let text_input: NativeTerminalInput =
            serde_json::from_str(text_json).expect("deserialize text input");
        assert!(matches!(text_input, NativeTerminalInput::Text { text } if text == "안녕"));

        let key_json = r#"{"keyEvent":{"key":"Enter","action":"Press","modifiers":{"shift":false,"ctrl":true,"alt":false,"superKey":false,"capsLock":false,"numLock":false},"utf8":null}}"#;
        let key_input: NativeTerminalInput =
            serde_json::from_str(key_json).expect("deserialize keyEvent input");
        match key_input {
            NativeTerminalInput::KeyEvent { key_event } => {
                assert_eq!(key_event.key, "Enter");
                assert_eq!(key_event.action, KeyAction::Press);
                assert!(key_event.modifiers.ctrl);
                assert!(!key_event.modifiers.shift);
                let converted = key_event.to_key_event().expect("convert to KeyEvent");
                assert_eq!(converted.key, KeyCode::Enter);
            }
            NativeTerminalInput::Text { .. } => panic!("expected KeyEvent"),
        }
    }

    #[test]
    fn parse_browser_key_handles_named_keys_and_unicode_and_rejects_unsupported() {
        use super::parse_browser_key;

        // Named keys
        assert_eq!(parse_browser_key("Enter").unwrap(), KeyCode::Enter);
        assert_eq!(parse_browser_key("Tab").unwrap(), KeyCode::Tab);
        assert_eq!(parse_browser_key("Backspace").unwrap(), KeyCode::Backspace);
        assert_eq!(parse_browser_key("Escape").unwrap(), KeyCode::Escape);
        assert_eq!(parse_browser_key("Space").unwrap(), KeyCode::Space);
        assert_eq!(parse_browser_key(" ").unwrap(), KeyCode::Space);
        assert_eq!(parse_browser_key("ArrowUp").unwrap(), KeyCode::ArrowUp);
        assert_eq!(parse_browser_key("ArrowDown").unwrap(), KeyCode::ArrowDown);
        assert_eq!(parse_browser_key("ArrowLeft").unwrap(), KeyCode::ArrowLeft);
        assert_eq!(
            parse_browser_key("ArrowRight").unwrap(),
            KeyCode::ArrowRight
        );
        assert_eq!(parse_browser_key("Home").unwrap(), KeyCode::Home);
        assert_eq!(parse_browser_key("End").unwrap(), KeyCode::End);
        assert_eq!(parse_browser_key("PageUp").unwrap(), KeyCode::PageUp);
        assert_eq!(parse_browser_key("PageDown").unwrap(), KeyCode::PageDown);
        assert_eq!(parse_browser_key("Insert").unwrap(), KeyCode::Insert);
        assert_eq!(parse_browser_key("Delete").unwrap(), KeyCode::Delete);
        assert_eq!(parse_browser_key("F1").unwrap(), KeyCode::F1);
        assert_eq!(parse_browser_key("F2").unwrap(), KeyCode::F2);
        assert_eq!(parse_browser_key("F3").unwrap(), KeyCode::F3);
        assert_eq!(parse_browser_key("F4").unwrap(), KeyCode::F4);
        assert_eq!(parse_browser_key("F5").unwrap(), KeyCode::F5);
        assert_eq!(parse_browser_key("F6").unwrap(), KeyCode::F6);
        assert_eq!(parse_browser_key("F7").unwrap(), KeyCode::F7);
        assert_eq!(parse_browser_key("F8").unwrap(), KeyCode::F8);
        assert_eq!(parse_browser_key("F9").unwrap(), KeyCode::F9);
        assert_eq!(parse_browser_key("F10").unwrap(), KeyCode::F10);
        assert_eq!(parse_browser_key("F11").unwrap(), KeyCode::F11);
        assert_eq!(parse_browser_key("F12").unwrap(), KeyCode::F12);

        // Single Unicode scalar character
        assert_eq!(parse_browser_key("c").unwrap(), KeyCode::Character('c'));
        assert_eq!(parse_browser_key("C").unwrap(), KeyCode::Character('C'));
        assert_eq!(parse_browser_key("1").unwrap(), KeyCode::Character('1'));
        assert_eq!(parse_browser_key("한").unwrap(), KeyCode::Character('한'));

        // Unsupported multi-character keys or empty string
        assert!(parse_browser_key("").is_err());
        assert!(parse_browser_key("Control").is_err());
        assert!(parse_browser_key("Shift").is_err());
        assert!(parse_browser_key("UnsupportedKey").is_err());
    }

    #[test]
    fn key_event_input_advances_terminal_state() {
        use super::BrowserKeyEventDto;

        let cell_metrics = CellMetrics {
            width_px: 10,
            height_px: 20,
        };
        let layout = test_layout(&cell_metrics);
        let mut state = NativeTerminalInputState::default();
        state.set_focused(true);

        state
            .feed(
                layout,
                &NativeTerminalInput::Text { text: "abc".into() },
                cell_metrics,
            )
            .expect("text feed succeeds");

        let after_enter = state
            .feed(
                layout,
                &NativeTerminalInput::KeyEvent {
                    key_event: BrowserKeyEventDto {
                        key: "Enter".into(),
                        action: KeyAction::Press,
                        modifiers: KeyModifiers::default(),
                        utf8: None,
                    },
                },
                cell_metrics,
            )
            .expect("key event feed succeeds");

        assert_eq!(after_enter.cursor.x, 0);
    }
}
