//! Kitty graphics and PNG callback ABI from the pinned libghostty-vt headers.
use super::types::{GhosttyAllocator, GhosttyTerminal};
use std::ffi::{c_int, c_void};

pub type Graphics = *mut c_void;
pub type Image = *mut c_void;
pub type PlacementIterator = *mut c_void;

#[repr(C)]
#[derive(Default)]
pub struct RenderInfo {
    pub size: usize,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub grid_cols: u32,
    pub grid_rows: u32,
    pub viewport_col: i32,
    pub viewport_row: i32,
    pub viewport_visible: u8,
    pub source_x: u32,
    pub source_y: u32,
    pub source_width: u32,
    pub source_height: u32,
}

#[repr(C)]
pub struct SysImage {
    pub width: u32,
    pub height: u32,
    pub data: *mut u8,
    pub data_len: usize,
}

pub const STORAGE_LIMIT: c_int = 15;
pub const GRAPHICS: c_int = 30;
pub const DECODE_PNG: c_int = 1;
pub const PLACEMENT_ITERATOR: c_int = 1;
pub const GENERATION: c_int = 2;
pub const PLACEMENT_IMAGE_ID: c_int = 1;
pub const PLACEMENT_ID: c_int = 2;
pub const PLACEMENT_X_OFFSET: c_int = 4;
pub const PLACEMENT_Y_OFFSET: c_int = 5;
pub const PLACEMENT_Z: c_int = 12;
pub const IMAGE_WIDTH: c_int = 3;
pub const IMAGE_HEIGHT: c_int = 4;
pub const IMAGE_FORMAT: c_int = 5;
pub const IMAGE_DATA: c_int = 7;
pub const IMAGE_LEN: c_int = 8;
pub const IMAGE_GENERATION: c_int = 9;

extern "C" {
    pub fn ghostty_sys_set(option: c_int, value: *const c_void) -> c_int;
    pub fn ghostty_alloc(allocator: *const GhosttyAllocator, len: usize) -> *mut c_void;
    pub fn ghostty_kitty_graphics_get(graphics: Graphics, key: c_int, out: *mut c_void) -> c_int;
    pub fn ghostty_kitty_graphics_image(graphics: Graphics, id: u32) -> Image;
    pub fn ghostty_kitty_graphics_image_get(image: Image, key: c_int, out: *mut c_void) -> c_int;
    pub fn ghostty_kitty_graphics_placement_iterator_new(
        allocator: *const GhosttyAllocator,
        out: *mut PlacementIterator,
    ) -> c_int;
    pub fn ghostty_kitty_graphics_placement_iterator_free(iter: PlacementIterator);
    pub fn ghostty_kitty_graphics_placement_next(iter: PlacementIterator) -> u8;
    pub fn ghostty_kitty_graphics_placement_get(
        iter: PlacementIterator,
        key: c_int,
        out: *mut c_void,
    ) -> c_int;
    pub fn ghostty_kitty_graphics_placement_render_info(
        iter: PlacementIterator,
        image: Image,
        terminal: GhosttyTerminal,
        out: *mut RenderInfo,
    ) -> c_int;
}
