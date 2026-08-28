//! Safe RAII guard wrappers for foreign handles.

use std::ptr::NonNull;

use super::sys::ffi::{
    ghostty_key_encoder_free, ghostty_key_event_free, ghostty_mouse_encoder_free,
    ghostty_mouse_event_free, ghostty_render_state_free, ghostty_render_state_row_cells_free,
    ghostty_render_state_row_iterator_free, ghostty_selection_gesture_event_free,
    ghostty_selection_gesture_free,
};
use super::sys::types::{
    GhosttyKeyEncoderImpl, GhosttyKeyEventImpl, GhosttyMouseEncoderImpl, GhosttyMouseEventImpl,
    GhosttyRenderStateImpl, GhosttyRenderStateRowCellsImpl, GhosttyRenderStateRowIteratorImpl,
    GhosttySelectionGestureEventImpl, GhosttySelectionGestureImpl, GhosttyTerminalImpl,
};

pub struct RenderStateGuard(pub NonNull<GhosttyRenderStateImpl>);
impl Drop for RenderStateGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_render_state_free(self.0.as_ptr()) };
    }
}

pub struct RowIteratorGuard(pub NonNull<GhosttyRenderStateRowIteratorImpl>);
impl Drop for RowIteratorGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_render_state_row_iterator_free(self.0.as_ptr()) };
    }
}

pub struct RowCellsGuard(pub NonNull<GhosttyRenderStateRowCellsImpl>);
impl Drop for RowCellsGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_render_state_row_cells_free(self.0.as_ptr()) };
    }
}

pub struct KeyEncoderGuard(pub NonNull<GhosttyKeyEncoderImpl>);
impl Drop for KeyEncoderGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_key_encoder_free(self.0.as_ptr()) };
    }
}

pub struct KeyEventGuard(pub NonNull<GhosttyKeyEventImpl>);
impl Drop for KeyEventGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_key_event_free(self.0.as_ptr()) };
    }
}

pub struct MouseEncoderGuard(pub NonNull<GhosttyMouseEncoderImpl>);
impl Drop for MouseEncoderGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_mouse_encoder_free(self.0.as_ptr()) };
    }
}

pub struct MouseEventGuard(pub NonNull<GhosttyMouseEventImpl>);
impl Drop for MouseEventGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_mouse_event_free(self.0.as_ptr()) };
    }
}

pub struct SelectionGestureGuard {
    ptr: Option<NonNull<GhosttySelectionGestureImpl>>,
}

impl SelectionGestureGuard {
    pub fn new(ptr: NonNull<GhosttySelectionGestureImpl>) -> Self {
        Self { ptr: Some(ptr) }
    }

    pub fn as_ptr(&self) -> *mut GhosttySelectionGestureImpl {
        self.ptr.map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }

    pub fn free_with_terminal(&mut self, terminal: *mut GhosttyTerminalImpl) {
        if let Some(ptr) = self.ptr.take() {
            // SAFETY: Foreign resource deallocation with associated terminal.
            unsafe { ghostty_selection_gesture_free(ptr.as_ptr(), terminal) };
        }
    }
}

impl Drop for SelectionGestureGuard {
    fn drop(&mut self) {
        if let Some(ptr) = self.ptr.take() {
            // SAFETY: Foreign resource deallocation. Terminal may be already freed.
            unsafe { ghostty_selection_gesture_free(ptr.as_ptr(), std::ptr::null_mut()) };
        }
    }
}

pub struct SelectionGestureEventGuard(pub NonNull<GhosttySelectionGestureEventImpl>);
impl Drop for SelectionGestureEventGuard {
    fn drop(&mut self) {
        // SAFETY: Foreign resource deallocation. self.0 is guaranteed non-null and valid.
        unsafe { ghostty_selection_gesture_event_free(self.0.as_ptr()) };
    }
}
