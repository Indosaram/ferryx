//! Safe libghostty-vt Native Terminal Engine Adapter (Phase 1).
//!
//! Provides a safe Rust boundary over the pinned static `libghostty-vt.a` C API.
//! All native resources are managed via Rust RAII (`Drop`), typed errors are returned,
//! inputs are validated at the Rust boundary, and foreign render state data is
//! deterministically copied into Rust owned data structures.

mod bell;
mod cell_extractor;
#[cfg(feature = "native-terminal")]
pub mod child_surface;
mod color;
#[cfg(feature = "native-terminal")]
pub mod composition;
mod cursor;
mod engine;
mod error;
mod guards;
mod images;
#[cfg(feature = "native-terminal")]
mod input;
mod key;
mod key_encoder;
mod lifecycle;
mod mouse;
mod mouse_encoder;
mod paste;
mod png_decoder;
#[cfg(feature = "native-terminal")]
pub mod platform;
mod queries;
mod render_pass;
#[cfg(feature = "native-terminal")]
pub mod renderer;
#[cfg(feature = "native-terminal")]
mod scroll;
mod search;
mod selection;
mod snapshot;
#[cfg(feature = "native-terminal")]
mod surface_error;
#[cfg(feature = "native-terminal")]
pub mod surface_host;
pub mod thread_ownership;
#[cfg(feature = "native-terminal")]
mod surface_snapshot;
mod sys;
mod terminal;
mod url;
mod viewport;
#[cfg(feature = "native-terminal")]
pub mod wheel;

pub use color::ColorRgb;
#[cfg(feature = "native-terminal")]
pub use composition::{
    CellMetrics, CompositorTargetKind, LogicalBounds, PhysicalBounds, PlatformCompositorDescriptor,
    SurfaceCompositionLayout,
};
pub use cursor::{CursorSnapshot, CursorState, CursorVisualStyle};
pub use engine::TerminalEngine;
pub use error::NativeTerminalError;
#[cfg(feature = "native-terminal")]
pub use input::NativeTerminalInput;
pub use images::{ImagePlacementSnapshot, TerminalImage};
pub use key::{KeyAction, KeyCode, KeyEvent, KeyModifiers};
pub use mouse::{MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize};
#[cfg(feature = "native-terminal")]
pub use renderer::{
    canonical_scenario, GlyphAtlasStats, NativeTerminalRenderer, OffscreenFrame, RendererConfig,
    SelectionSnapshot,
};
#[cfg(feature = "native-terminal")]
pub use scroll::{
    compute_attention_frame_rects, compute_scrollbar_overlay_rect, macos_wheel_scroll_rows,
    ScrollbarOverlayState, ATTENTION_FRAME_COLOR,
    ATTENTION_FRAME_THICKNESS_LOGICAL_PX, ATTENTION_HALO_COLOR,
};
pub use snapshot::{CellSnapshot, CellWide, RenderSnapshot};
pub use terminal::NativeTerminal;
pub use viewport::{ScrollViewport, ScrollbarState};
#[cfg(feature = "native-terminal")]
pub use wheel::{compute_wheel_outcome, TerminalWheelOutcome};
