//! Safe libghostty-vt Native Terminal Engine Adapter (Phase 1).
//!
//! Provides a safe Rust boundary over the pinned static `libghostty-vt.a` C API.
//! All native resources are managed via Rust RAII (`Drop`), typed errors are returned,
//! inputs are validated at the Rust boundary, and foreign render state data is
//! deterministically copied into Rust owned data structures.

mod bell;
mod cell_extractor;
pub mod child_surface;
mod color;
pub mod composition;
mod cursor;
mod engine;
mod error;
mod guards;
mod input;
mod key;
mod key_encoder;
mod lifecycle;
mod mouse;
mod mouse_encoder;
mod paste;
pub mod platform;
mod queries;
mod render_pass;
pub mod renderer;
mod scroll;
mod search;
mod selection;
mod snapshot;
mod surface_error;
pub mod surface_host;
mod surface_snapshot;
mod sys;
mod terminal;

pub use color::ColorRgb;
pub use composition::{
    CellMetrics, CompositorTargetKind, LogicalBounds, PhysicalBounds, PlatformCompositorDescriptor,
    SurfaceCompositionLayout,
};
pub use cursor::{CursorSnapshot, CursorState, CursorVisualStyle};
pub use engine::TerminalEngine;
pub use error::NativeTerminalError;
pub use input::NativeTerminalInput;
pub use key::{KeyAction, KeyCode, KeyEvent, KeyModifiers};
pub use mouse::{MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize};
pub use renderer::{
    canonical_scenario, GlyphAtlasStats, NativeTerminalRenderer, OffscreenFrame, RendererConfig,
    SelectionSnapshot,
};
pub use scroll::{
    compute_attention_frame_rects, compute_scrollbar_overlay_rect, macos_wheel_scroll_rows,
    ScrollViewport, ScrollbarOverlayState, ScrollbarState, ATTENTION_FRAME_COLOR,
    ATTENTION_FRAME_THICKNESS_LOGICAL_PX, ATTENTION_HALO_COLOR,
};
pub use snapshot::{CellSnapshot, CellWide, RenderSnapshot};
pub use terminal::NativeTerminal;
