//! Keyboard event encoding helper using libghostty-vt key encoder.

use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::guards::{KeyEncoderGuard, KeyEventGuard};
use super::key::{KeyAction, KeyCode, KeyEvent};
use super::sys::ffi::{
    ghostty_key_encoder_encode, ghostty_key_encoder_new, ghostty_key_encoder_setopt,
    ghostty_key_encoder_setopt_from_terminal, ghostty_key_event_new, ghostty_key_event_set_action,
    ghostty_key_event_set_key, ghostty_key_event_set_mods, ghostty_key_event_set_utf8,
};
use super::sys::types::{
    GhosttyKeyEncoder, GhosttyKeyEvent, GhosttyTerminal, GHOSTTY_KEY_A, GHOSTTY_KEY_ARROW_DOWN,
    GHOSTTY_KEY_ARROW_LEFT, GHOSTTY_KEY_ARROW_RIGHT, GHOSTTY_KEY_ARROW_UP, GHOSTTY_KEY_BACKSPACE,
    GHOSTTY_KEY_DELETE, GHOSTTY_KEY_DIGIT_0, GHOSTTY_KEY_END, GHOSTTY_KEY_ENTER,
    GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT, GHOSTTY_KEY_ESCAPE, GHOSTTY_KEY_F1,
    GHOSTTY_KEY_F10, GHOSTTY_KEY_F11, GHOSTTY_KEY_F12, GHOSTTY_KEY_F2, GHOSTTY_KEY_F3,
    GHOSTTY_KEY_F4, GHOSTTY_KEY_F5, GHOSTTY_KEY_F6, GHOSTTY_KEY_F7, GHOSTTY_KEY_F8,
    GHOSTTY_KEY_F9, GHOSTTY_KEY_HOME, GHOSTTY_KEY_INSERT, GHOSTTY_KEY_PAGE_DOWN,
    GHOSTTY_KEY_PAGE_UP, GHOSTTY_KEY_SPACE, GHOSTTY_KEY_TAB, GHOSTTY_KEY_UNIDENTIFIED,
    GHOSTTY_OPTION_AS_ALT_FALSE, GHOSTTY_OPTION_AS_ALT_TRUE, GHOSTTY_OUT_OF_SPACE,
    GHOSTTY_SUCCESS,
};

/// Maximum bounded bytes for an encoded keyboard escape sequence to prevent unbounded allocation.
const MAX_KEY_ENCODED_BYTES: usize = 1024;

fn map_key_code_to_c(key: KeyCode) -> i32 {
    match key {
        KeyCode::Unidentified => GHOSTTY_KEY_UNIDENTIFIED,
        KeyCode::Character(c) => match c.to_ascii_uppercase() {
            'A'..='Z' => GHOSTTY_KEY_A + (c.to_ascii_uppercase() as i32 - 'A' as i32),
            '0'..='9' => GHOSTTY_KEY_DIGIT_0 + (c as i32 - '0' as i32),
            ' ' => GHOSTTY_KEY_SPACE,
            '\n' | '\r' => GHOSTTY_KEY_ENTER,
            '\t' => GHOSTTY_KEY_TAB,
            _ => GHOSTTY_KEY_UNIDENTIFIED,
        },
        KeyCode::Enter => GHOSTTY_KEY_ENTER,
        KeyCode::Tab => GHOSTTY_KEY_TAB,
        KeyCode::Backspace => GHOSTTY_KEY_BACKSPACE,
        KeyCode::Escape => GHOSTTY_KEY_ESCAPE,
        KeyCode::Space => GHOSTTY_KEY_SPACE,
        KeyCode::ArrowDown => GHOSTTY_KEY_ARROW_DOWN,
        KeyCode::ArrowLeft => GHOSTTY_KEY_ARROW_LEFT,
        KeyCode::ArrowRight => GHOSTTY_KEY_ARROW_RIGHT,
        KeyCode::ArrowUp => GHOSTTY_KEY_ARROW_UP,
        KeyCode::Home => GHOSTTY_KEY_HOME,
        KeyCode::End => GHOSTTY_KEY_END,
        KeyCode::PageUp => GHOSTTY_KEY_PAGE_UP,
        KeyCode::PageDown => GHOSTTY_KEY_PAGE_DOWN,
        KeyCode::Insert => GHOSTTY_KEY_INSERT,
        KeyCode::Delete => GHOSTTY_KEY_DELETE,
        KeyCode::F1 => GHOSTTY_KEY_F1,
        KeyCode::F2 => GHOSTTY_KEY_F2,
        KeyCode::F3 => GHOSTTY_KEY_F3,
        KeyCode::F4 => GHOSTTY_KEY_F4,
        KeyCode::F5 => GHOSTTY_KEY_F5,
        KeyCode::F6 => GHOSTTY_KEY_F6,
        KeyCode::F7 => GHOSTTY_KEY_F7,
        KeyCode::F8 => GHOSTTY_KEY_F8,
        KeyCode::F9 => GHOSTTY_KEY_F9,
        KeyCode::F10 => GHOSTTY_KEY_F10,
        KeyCode::F11 => GHOSTTY_KEY_F11,
        KeyCode::F12 => GHOSTTY_KEY_F12,
    }
}

fn validate_utf8_for_key_event(text: &str) -> Result<(), NativeTerminalError> {
    if text
        .chars()
        .any(|c| (c <= '\x1f') || c == '\x7f' || ('\u{F700}'..='\u{F8FF}').contains(&c))
    {
        return Err(NativeTerminalError::InvalidValue(
            "KeyEvent utf8 text must not contain C0 controls (0x00-0x1F, 0x7F) or PUA codepoints (0xF700-0xF8FF)".to_string(),
        ));
    }
    Ok(())
}

pub fn encode_key_event(
    term_ptr: GhosttyTerminal,
    event: &KeyEvent,
) -> Result<Vec<u8>, NativeTerminalError> {
    encode_key_event_with_option_as_alt(term_ptr, event, false)
}

/// Encodes a key event, telling libghostty whether macOS Option acts as Alt/Meta.
///
/// `ghostty_key_encoder_setopt_from_terminal` cannot derive `macos_option_as_alt` from terminal
/// state and resets it to false, so it must be applied afterwards on every encode.
pub fn encode_key_event_with_option_as_alt(
    term_ptr: GhosttyTerminal,
    event: &KeyEvent,
    option_as_alt: bool,
) -> Result<Vec<u8>, NativeTerminalError> {
    let mut encoder_raw: GhosttyKeyEncoder = std::ptr::null_mut();
    // SAFETY: Category: Foreign Resource Allocation. Null allocator selects default.
    let res = unsafe { ghostty_key_encoder_new(std::ptr::null(), &mut encoder_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_key_encoder_new")?;
    let non_null_enc = NonNull::new(encoder_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_key_encoder_new returned null pointer".to_string(),
        )
    })?;
    let encoder_guard = KeyEncoderGuard(non_null_enc);

    // SAFETY: Category: Foreign Configuration from Terminal.
    // Invariant: encoder_guard.0 and term_ptr are validated non-null handles.
    unsafe {
        ghostty_key_encoder_setopt_from_terminal(encoder_guard.0.as_ptr(), term_ptr);
        let option_as_alt_value = if option_as_alt {
            GHOSTTY_OPTION_AS_ALT_TRUE
        } else {
            GHOSTTY_OPTION_AS_ALT_FALSE
        };
        ghostty_key_encoder_setopt(
            encoder_guard.0.as_ptr(),
            GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT,
            &option_as_alt_value as *const _ as *const std::ffi::c_void,
        );
    }

    let mut event_raw: GhosttyKeyEvent = std::ptr::null_mut();
    // SAFETY: Category: Foreign Resource Allocation. Creates new key event.
    let res = unsafe { ghostty_key_event_new(std::ptr::null(), &mut event_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_key_event_new")?;
    let non_null_evt = NonNull::new(event_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue("ghostty_key_event_new returned null pointer".to_string())
    })?;
    let event_guard = KeyEventGuard(non_null_evt);

    let action_c = match event.action {
        KeyAction::Release => 0,
        KeyAction::Press => 1,
        KeyAction::Repeat => 2,
    };
    let key_c = map_key_code_to_c(event.key);
    let raw_mods = event.modifiers.to_raw_mods();

    // SAFETY: Category: Foreign State Mutation. Handles are non-null and valid.
    unsafe {
        ghostty_key_event_set_action(event_guard.0.as_ptr(), action_c);
        ghostty_key_event_set_key(event_guard.0.as_ptr(), key_c);
        ghostty_key_event_set_mods(event_guard.0.as_ptr(), raw_mods);

        if let Some(ref text) = event.utf8 {
            validate_utf8_for_key_event(text)?;
            ghostty_key_event_set_utf8(event_guard.0.as_ptr(), text.as_ptr(), text.len());
        } else if let KeyCode::Character(c) = event.key {
            let mut char_buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut char_buf);
            validate_utf8_for_key_event(encoded)?;
            ghostty_key_event_set_utf8(event_guard.0.as_ptr(), encoded.as_ptr(), encoded.len());
        }
    }

    let mut stack_buf = [0u8; 128];
    let mut written: usize = 0;

    // SAFETY: Category: Foreign Buffer Encoding. Points to 128 valid stack bytes.
    let res = unsafe {
        ghostty_key_encoder_encode(
            encoder_guard.0.as_ptr(),
            event_guard.0.as_ptr(),
            stack_buf.as_mut_ptr(),
            stack_buf.len(),
            &mut written,
        )
    };

    if res == GHOSTTY_SUCCESS {
        if written > stack_buf.len() {
            return Err(NativeTerminalError::InvalidValue(
                "foreign key encoder reported written size exceeding stack buffer capacity"
                    .to_string(),
            ));
        }
        Ok(stack_buf[..written].to_vec())
    } else if res == GHOSTTY_OUT_OF_SPACE {
        if written == 0 || written > MAX_KEY_ENCODED_BYTES {
            return Err(NativeTerminalError::LimitExceeded);
        }
        let mut dynamic_buf = vec![0u8; written];
        let mut dyn_written: usize = 0;
        // SAFETY: Category: Dynamic Buffer Encoding Retry. Pre-allocated to required capacity.
        let retry_res = unsafe {
            ghostty_key_encoder_encode(
                encoder_guard.0.as_ptr(),
                event_guard.0.as_ptr(),
                dynamic_buf.as_mut_ptr(),
                dynamic_buf.len(),
                &mut dyn_written,
            )
        };
        NativeTerminalError::from_c_result(retry_res, "ghostty_key_encoder_encode heap")?;
        if dyn_written > dynamic_buf.len() {
            return Err(NativeTerminalError::InvalidValue(
                "foreign key encoder reported written size exceeding heap buffer capacity"
                    .to_string(),
            ));
        }
        Ok(dynamic_buf[..dyn_written].to_vec())
    } else {
        NativeTerminalError::from_c_result(res, "ghostty_key_encoder_encode")?;
        Ok(Vec::new())
    }
}
