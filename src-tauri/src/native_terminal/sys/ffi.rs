//! Raw foreign function declarations for libghostty-vt.

use std::ffi::{c_int, c_void};

use super::types::{
    GhosttyAllocator, GhosttyCell, GhosttyGridRef, GhosttyKeyEncoder, GhosttyKeyEvent,
    GhosttyMouseEncoder, GhosttyMouseEvent, GhosttyMousePosition, GhosttyPoint,
    GhosttyPointCoordinate, GhosttyRenderState, GhosttyRenderStateRowCells,
    GhosttyRenderStateRowIterator, GhosttySelection, GhosttyTerminal,
    GhosttyTerminalScrollViewport, GhosttyTerminalSelectLineOptions,
    GhosttyTerminalSelectWordOptions, GhosttyTerminalSelectionFormatOptions,
};

extern "C" {
    pub fn ghostty_terminal_new(
        allocator: *const GhosttyAllocator,
        terminal: *mut GhosttyTerminal,
        cols: u16,
        rows: u16,
    ) -> c_int;

    pub fn ghostty_terminal_free(terminal: GhosttyTerminal);
    pub fn ghostty_terminal_reset(terminal: GhosttyTerminal);
    pub fn ghostty_terminal_resize(
        terminal: GhosttyTerminal,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> c_int;
    pub fn ghostty_terminal_set(
        terminal: GhosttyTerminal,
        option: c_int,
        value: *const c_void,
    ) -> c_int;
    pub fn ghostty_terminal_vt_write(terminal: GhosttyTerminal, data: *const u8, len: usize);
    pub fn ghostty_terminal_get(terminal: GhosttyTerminal, data: c_int, out: *mut c_void) -> c_int;
    pub fn ghostty_terminal_scroll_viewport(
        terminal: GhosttyTerminal,
        behavior: GhosttyTerminalScrollViewport,
    );
    pub fn ghostty_terminal_grid_ref(
        terminal: GhosttyTerminal,
        point: GhosttyPoint,
        out_ref: *mut GhosttyGridRef,
    ) -> c_int;
    pub fn ghostty_terminal_point_from_grid_ref(
        terminal: GhosttyTerminal,
        grid_ref: *const GhosttyGridRef,
        tag: c_int,
        out: *mut GhosttyPointCoordinate,
    ) -> c_int;
    pub fn ghostty_grid_ref_graphemes(
        grid_ref: *const GhosttyGridRef,
        buf: *mut u32,
        buf_len: usize,
        out_len: *mut usize,
    ) -> c_int;

    pub fn ghostty_terminal_select_all(
        terminal: GhosttyTerminal,
        out_selection: *mut GhosttySelection,
    ) -> c_int;
    pub fn ghostty_terminal_select_word(
        terminal: GhosttyTerminal,
        options: *const GhosttyTerminalSelectWordOptions,
        out_selection: *mut GhosttySelection,
    ) -> c_int;
    pub fn ghostty_terminal_select_line(
        terminal: GhosttyTerminal,
        options: *const GhosttyTerminalSelectLineOptions,
        out_selection: *mut GhosttySelection,
    ) -> c_int;
    pub fn ghostty_terminal_selection_format_alloc(
        terminal: GhosttyTerminal,
        allocator: *const GhosttyAllocator,
        options: GhosttyTerminalSelectionFormatOptions,
        out_ptr: *mut *mut u8,
        out_len: *mut usize,
    ) -> c_int;
    pub fn ghostty_terminal_selection_ordered(
        terminal: GhosttyTerminal,
        selection: *const GhosttySelection,
        desired: c_int,
        out_selection: *mut GhosttySelection,
    ) -> c_int;

    pub fn ghostty_paste_is_safe(data: *const u8, len: usize) -> bool;
    pub fn ghostty_paste_encode(
        data: *mut u8,
        data_len: usize,
        bracketed: bool,
        buf: *mut u8,
        buf_len: usize,
        out_written: *mut usize,
    ) -> c_int;
    pub fn ghostty_free(allocator: *const GhosttyAllocator, ptr: *mut u8, len: usize);

    pub fn ghostty_cell_get(cell: GhosttyCell, data: c_int, out: *mut c_void) -> c_int;

    pub fn ghostty_render_state_new(
        allocator: *const GhosttyAllocator,
        state: *mut GhosttyRenderState,
    ) -> c_int;
    pub fn ghostty_render_state_free(state: GhosttyRenderState);
    pub fn ghostty_render_state_update(
        state: GhosttyRenderState,
        terminal: GhosttyTerminal,
    ) -> c_int;
    pub fn ghostty_render_state_get(
        state: GhosttyRenderState,
        data: c_int,
        out: *mut c_void,
    ) -> c_int;

    pub fn ghostty_render_state_row_iterator_new(
        allocator: *const GhosttyAllocator,
        out_iterator: *mut GhosttyRenderStateRowIterator,
    ) -> c_int;
    pub fn ghostty_render_state_row_iterator_free(iterator: GhosttyRenderStateRowIterator);
    pub fn ghostty_render_state_row_iterator_next(iterator: GhosttyRenderStateRowIterator) -> u8;
    pub fn ghostty_render_state_row_get(
        iterator: GhosttyRenderStateRowIterator,
        data: c_int,
        out: *mut c_void,
    ) -> c_int;

    pub fn ghostty_render_state_row_cells_new(
        allocator: *const GhosttyAllocator,
        out_cells: *mut GhosttyRenderStateRowCells,
    ) -> c_int;
    pub fn ghostty_render_state_row_cells_free(cells: GhosttyRenderStateRowCells);
    pub fn ghostty_render_state_row_cells_next(cells: GhosttyRenderStateRowCells) -> u8;
    pub fn ghostty_render_state_row_cells_get(
        cells: GhosttyRenderStateRowCells,
        data: c_int,
        out: *mut c_void,
    ) -> c_int;

    // Key event and encoder
    pub fn ghostty_key_event_new(
        allocator: *const GhosttyAllocator,
        event: *mut GhosttyKeyEvent,
    ) -> c_int;
    pub fn ghostty_key_event_free(event: GhosttyKeyEvent);
    pub fn ghostty_key_event_set_action(event: GhosttyKeyEvent, action: c_int);
    pub fn ghostty_key_event_set_key(event: GhosttyKeyEvent, key: c_int);
    pub fn ghostty_key_event_set_mods(event: GhosttyKeyEvent, mods: u16);
    pub fn ghostty_key_event_set_utf8(event: GhosttyKeyEvent, utf8: *const u8, len: usize);

    pub fn ghostty_key_encoder_new(
        allocator: *const GhosttyAllocator,
        encoder: *mut GhosttyKeyEncoder,
    ) -> c_int;
    pub fn ghostty_key_encoder_free(encoder: GhosttyKeyEncoder);
    pub fn ghostty_key_encoder_setopt_from_terminal(
        encoder: GhosttyKeyEncoder,
        terminal: GhosttyTerminal,
    );
    pub fn ghostty_key_encoder_setopt(
        encoder: GhosttyKeyEncoder,
        option: c_int,
        value: *const c_void,
    );
    pub fn ghostty_key_encoder_encode(
        encoder: GhosttyKeyEncoder,
        event: GhosttyKeyEvent,
        out_buf: *mut u8,
        out_buf_size: usize,
        out_len: *mut usize,
    ) -> c_int;

    // Mouse event and encoder
    pub fn ghostty_mouse_event_new(
        allocator: *const GhosttyAllocator,
        event: *mut GhosttyMouseEvent,
    ) -> c_int;
    pub fn ghostty_mouse_event_free(event: GhosttyMouseEvent);
    pub fn ghostty_mouse_event_set_action(event: GhosttyMouseEvent, action: c_int);
    pub fn ghostty_mouse_event_set_button(event: GhosttyMouseEvent, button: c_int);
    pub fn ghostty_mouse_event_clear_button(event: GhosttyMouseEvent);
    pub fn ghostty_mouse_event_set_mods(event: GhosttyMouseEvent, mods: u16);
    pub fn ghostty_mouse_event_set_position(
        event: GhosttyMouseEvent,
        position: GhosttyMousePosition,
    );

    pub fn ghostty_mouse_encoder_new(
        allocator: *const GhosttyAllocator,
        encoder: *mut GhosttyMouseEncoder,
    ) -> c_int;
    pub fn ghostty_mouse_encoder_free(encoder: GhosttyMouseEncoder);
    pub fn ghostty_mouse_encoder_setopt(
        encoder: GhosttyMouseEncoder,
        option: c_int,
        value: *const c_void,
    );
    pub fn ghostty_mouse_encoder_setopt_from_terminal(
        encoder: GhosttyMouseEncoder,
        terminal: GhosttyTerminal,
    );
    pub fn ghostty_mouse_encoder_encode(
        encoder: GhosttyMouseEncoder,
        event: GhosttyMouseEvent,
        out_buf: *mut u8,
        out_buf_size: usize,
        out_len: *mut usize,
    ) -> c_int;
}
