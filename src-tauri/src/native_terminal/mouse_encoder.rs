//! Mouse event encoding helper using libghostty-vt mouse encoder.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::guards::{MouseEncoderGuard, MouseEventGuard};
use super::mouse::{MouseAction, MouseButton, MouseEvent};
use super::sys::ffi::{
    ghostty_mouse_encoder_encode, ghostty_mouse_encoder_new, ghostty_mouse_encoder_setopt,
    ghostty_mouse_encoder_setopt_from_terminal, ghostty_mouse_event_clear_button,
    ghostty_mouse_event_new, ghostty_mouse_event_set_action, ghostty_mouse_event_set_button,
    ghostty_mouse_event_set_mods, ghostty_mouse_event_set_position,
};
use super::sys::types::{
    GhosttyMouseEncoder, GhosttyMouseEncoderSize, GhosttyMouseEvent, GhosttyMousePosition,
    GhosttyTerminal, GHOSTTY_MOUSE_ENCODER_OPT_SIZE, GHOSTTY_OUT_OF_SPACE, GHOSTTY_SUCCESS,
};

/// Maximum bounded bytes for an encoded mouse escape sequence to prevent unbounded allocation.
const MAX_MOUSE_ENCODED_BYTES: usize = 1024;

pub fn encode_mouse_event(
    term_ptr: GhosttyTerminal,
    event: &MouseEvent,
) -> Result<Vec<u8>, NativeTerminalError> {
    if !event.position.x.is_finite() || !event.position.y.is_finite() {
        return Err(NativeTerminalError::InvalidValue(
            "MousePosition x and y coordinates must be finite".to_string(),
        ));
    }

    let mut encoder_raw: GhosttyMouseEncoder = std::ptr::null_mut();
    // SAFETY: Category: Foreign Resource Allocation. Null allocator selects default.
    let res = unsafe { ghostty_mouse_encoder_new(std::ptr::null(), &mut encoder_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_mouse_encoder_new")?;
    let non_null_enc = NonNull::new(encoder_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_mouse_encoder_new returned null pointer".to_string(),
        )
    })?;
    let encoder_guard = MouseEncoderGuard(non_null_enc);

    // SAFETY: Category: Foreign Configuration from Terminal.
    // Invariant: encoder_guard.0 and term_ptr are validated non-null handles.
    unsafe {
        ghostty_mouse_encoder_setopt_from_terminal(encoder_guard.0.as_ptr(), term_ptr);
    }

    if let Some(sz) = event.size {
        if sz.cell_width == 0 || sz.cell_height == 0 {
            return Err(NativeTerminalError::InvalidValue(
                "MouseRendererSize cell_width and cell_height must be > 0".to_string(),
            ));
        }

        let mut c_sz = GhosttyMouseEncoderSize {
            size: std::mem::size_of::<GhosttyMouseEncoderSize>(),
            screen_width: sz.screen_width,
            screen_height: sz.screen_height,
            cell_width: sz.cell_width,
            cell_height: sz.cell_height,
            padding_top: sz.padding_top,
            padding_bottom: sz.padding_bottom,
            padding_right: sz.padding_right,
            padding_left: sz.padding_left,
        };
        // SAFETY: Category: Sized Struct ABI Option Configuration.
        // Invariant: c_sz.size is initialized to sizeof(GhosttyMouseEncoderSize).
        unsafe {
            ghostty_mouse_encoder_setopt(
                encoder_guard.0.as_ptr(),
                GHOSTTY_MOUSE_ENCODER_OPT_SIZE,
                &mut c_sz as *mut _ as *const c_void,
            );
        }
    }

    let mut event_raw: GhosttyMouseEvent = std::ptr::null_mut();
    // SAFETY: Category: Foreign Resource Allocation. Creates new mouse event handle.
    let res = unsafe { ghostty_mouse_event_new(std::ptr::null(), &mut event_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_mouse_event_new")?;
    let non_null_evt = NonNull::new(event_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_mouse_event_new returned null pointer".to_string(),
        )
    })?;
    let event_guard = MouseEventGuard(non_null_evt);

    let action_c = match event.action {
        MouseAction::Press => 0,
        MouseAction::Release => 1,
        MouseAction::Motion => 2,
    };
    let pos_c = GhosttyMousePosition {
        x: event.position.x,
        y: event.position.y,
    };
    let raw_mods = event.modifiers.to_raw_mods();

    // SAFETY: Category: Foreign State Mutation. Handles are non-null and valid.
    unsafe {
        ghostty_mouse_event_set_action(event_guard.0.as_ptr(), action_c);
        ghostty_mouse_event_set_position(event_guard.0.as_ptr(), pos_c);
        ghostty_mouse_event_set_mods(event_guard.0.as_ptr(), raw_mods);

        if let Some(btn) = event.button {
            let btn_c = match btn {
                MouseButton::Left => 1,
                MouseButton::Right => 2,
                MouseButton::Middle => 3,
                MouseButton::Four => 4,
                MouseButton::Five => 5,
                MouseButton::Six => 6,
                MouseButton::Seven => 7,
                MouseButton::Eight => 8,
                MouseButton::Nine => 9,
                MouseButton::Ten => 10,
                MouseButton::Eleven => 11,
            };
            ghostty_mouse_event_set_button(event_guard.0.as_ptr(), btn_c);
        } else {
            ghostty_mouse_event_clear_button(event_guard.0.as_ptr());
        }
    }

    let mut stack_buf = [0u8; 128];
    let mut written: usize = 0;

    // SAFETY: Category: Foreign Buffer Encoding. Points to 128 valid stack bytes.
    let res = unsafe {
        ghostty_mouse_encoder_encode(
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
                "foreign mouse encoder reported written size exceeding stack buffer capacity"
                    .to_string(),
            ));
        }
        Ok(stack_buf[..written].to_vec())
    } else if res == GHOSTTY_OUT_OF_SPACE {
        if written == 0 || written > MAX_MOUSE_ENCODED_BYTES {
            return Err(NativeTerminalError::LimitExceeded);
        }
        let mut dynamic_buf = vec![0u8; written];
        let mut dyn_written: usize = 0;
        // SAFETY: Category: Dynamic Buffer Encoding Retry. Pre-allocated to required capacity.
        let retry_res = unsafe {
            ghostty_mouse_encoder_encode(
                encoder_guard.0.as_ptr(),
                event_guard.0.as_ptr(),
                dynamic_buf.as_mut_ptr(),
                dynamic_buf.len(),
                &mut dyn_written,
            )
        };
        NativeTerminalError::from_c_result(retry_res, "ghostty_mouse_encoder_encode heap")?;
        if dyn_written > dynamic_buf.len() {
            return Err(NativeTerminalError::InvalidValue(
                "foreign mouse encoder reported written size exceeding heap buffer capacity"
                    .to_string(),
            ));
        }
        Ok(dynamic_buf[..dyn_written].to_vec())
    } else {
        NativeTerminalError::from_c_result(res, "ghostty_mouse_encoder_encode")?;
        Ok(Vec::new())
    }
}
