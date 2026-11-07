//! Platform-local native browser adapters (plan C4, tasks T7a/b/c).
//!
//! macOS capture stays in `browser::snapshot_source`; Windows and Linux add their
//! memory-backed capture here behind the shared `BrowserSnapshotSource` trait.

#[cfg(all(feature = "desktop", target_os = "linux"))]
pub(crate) mod linux;
#[cfg(all(feature = "desktop", target_os = "windows"))]
pub(crate) mod windows;
