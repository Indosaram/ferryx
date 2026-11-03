//! Win32 text clipboard synchronization with sequence-number echo suppression.
//!
//! Windows' `GetClipboardSequenceNumber` is the direct analogue of
//! `NSPasteboard.changeCount`: after applying remote text, the resulting number
//! is recorded and the matching poll is suppressed. A deterministic content
//! hash adds the same content-level dedup used by the Swift host.
//!
//! Sensitive clipboard entries have no universal Windows marker. Before text is
//! read this backend checks Microsoft's registered exclusion formats, including
//! `ExcludeClipboardContentFromMonitorProcessing` (the practical equivalent of
//! `CFSTR_EXCLUDED_FROM_PROCESS_HISTORY`), `CanIncludeInClipboardHistory`, and
//! `CanUploadToCloudClipboard`. Remote writes publish the exclusion format so
//! synchronized text is not persisted by Clipboard History or cloud roaming.
//!
//! CI validates the bindings and pure state-machine tests. Runtime QA still
//! requires a real Windows desktop because the clipboard is a session-global,
//! contended resource.

use clipboard_win::{
    formats::{RawData, Unicode, CF_UNICODETEXT},
    raw, Clipboard, Format, Getter, Setter,
};
use ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES;
use thiserror::Error;

pub use crate::windows_logic::{clipboard_content_hash as content_hash, ClipboardEchoSuppressor};

const FORMAT_EXCLUDE_MONITOR: &str = "ExcludeClipboardContentFromMonitorProcessing";
const FORMAT_INCLUDE_HISTORY: &str = "CanIncludeInClipboardHistory";
const FORMAT_UPLOAD_CLOUD: &str = "CanUploadToCloudClipboard";
const FALSE_DWORD: [u8; 4] = 0_u32.to_le_bytes();
const TRUE_DWORD: [u8; 4] = 1_u32.to_le_bytes();

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("Win32 clipboard operation failed: {0}")]
    Win32(String),
    #[error("clipboard text exceeds the 4 KiB protocol limit")]
    TooLarge,
}

fn win32<T>(result: clipboard_win::SysResult<T>) -> Result<T, ClipboardError> {
    result.map_err(|error| ClipboardError::Win32(error.to_string()))
}

#[derive(Debug, Clone, Copy)]
struct SensitiveFormats {
    exclude_monitor: Option<u32>,
    include_history: Option<u32>,
    upload_cloud: Option<u32>,
}

impl SensitiveFormats {
    fn register() -> Self {
        Self {
            exclude_monitor: raw::register_format(FORMAT_EXCLUDE_MONITOR).map(|id| id.get()),
            include_history: raw::register_format(FORMAT_INCLUDE_HISTORY).map(|id| id.get()),
            upload_cloud: raw::register_format(FORMAT_UPLOAD_CLOUD).map(|id| id.get()),
        }
    }

    fn selection_is_sensitive(self) -> bool {
        self.exclude_monitor.is_some_and(raw::is_format_avail)
            || self.include_history.is_some_and(format_is_false)
            || self.upload_cloud.is_some_and(format_is_false)
    }

    fn publish_remote_exclusions(self) -> Result<(), ClipboardError> {
        if let Some(format) = self.exclude_monitor {
            win32(RawData(format).write_clipboard(&TRUE_DWORD))?;
        }
        if let Some(format) = self.include_history {
            win32(RawData(format).write_clipboard(&FALSE_DWORD))?;
        }
        if let Some(format) = self.upload_cloud {
            win32(RawData(format).write_clipboard(&FALSE_DWORD))?;
        }
        Ok(())
    }
}

pub struct WindowsClipboard {
    image: Option<Vec<u8>>,
    suppressor: ClipboardEchoSuppressor,
    formats: SensitiveFormats,
}

impl WindowsClipboard {
    pub fn new() -> Self {
        let mut suppressor = ClipboardEchoSuppressor::default();
        if let Some(sequence) = sequence_number() {
            suppressor.start(sequence);
        }
        Self {
            image: None,
            suppressor,
            formats: SensitiveFormats::register(),
        }
    }

    /// Poll once. `Ok(None)` means unchanged, echo-suppressed, concealed, empty,
    /// unavailable, or over the protocol's 4 KiB UTF-8 cap.
    pub fn poll(&mut self) -> Result<Option<String>, ClipboardError> {
        let Some(sequence) = sequence_number() else {
            return Ok(None);
        };
        if !self.suppressor.observe_sequence(sequence) {
            return Ok(None);
        }

        let clipboard = win32(Clipboard::new_attempts(10))?;
        if self.formats.selection_is_sensitive() || !Unicode.is_format_avail() {
            drop(clipboard);
            return Ok(None);
        }

        // Avoid allocating an arbitrarily large UTF-16 selection. Four KiB of
        // UTF-8 cannot require more than 8 KiB plus the UTF-16 terminator.
        // `raw::size` reports `GlobalSize`, the allocation capacity, which is
        // routinely far larger than the text it holds, so measure the wide
        // string up to its NUL terminator instead of the allocation.
        let max_utf16_bytes = MAX_CLIPBOARD_TEXT_BYTES * 2;
        let mut probe = vec![0_u8; max_utf16_bytes + 2];
        let copied = win32(raw::get(CF_UNICODETEXT, &mut probe))?;
        let within_cap = matches!(
            wide_string_len(&probe[..copied]),
            Some(wide_bytes) if wide_bytes <= max_utf16_bytes
        );
        if !within_cap {
            drop(clipboard);
            return Ok(None);
        }

        let mut text = String::new();
        win32(Unicode.read_clipboard(&mut text))?;
        drop(clipboard);
        if text.is_empty() || text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return Ok(None);
        }
        Ok(self.suppressor.observe_text(&text).then_some(text))
    }

    pub fn apply_remote_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        let clipboard = win32(Clipboard::new_attempts(10))?;
        win32(raw::empty())?;
        // `Unicode::write_clipboard` normally clears first. We already cleared
        // once so the text and exclusion formats remain in one clipboard entry.
        win32(raw::set_string_with(text, clipboard_win::options::NoClear))?;
        self.formats.publish_remote_exclusions()?;
        let sequence = sequence_number().unwrap_or_default();
        drop(clipboard);
        self.suppressor.record_remote_write(sequence, text);
        Ok(())
    }

    pub fn apply_remote_image(&mut self, bytes: &[u8]) -> Result<(), ClipboardError> {
        crate::clipboard_image::validate_png(bytes)
            .map_err(|error| ClipboardError::Win32(error.to_string()))?;
        let format = raw::register_format("PNG")
            .ok_or_else(|| ClipboardError::Win32("PNG format unavailable".into()))?
            .get();
        let _clipboard = win32(Clipboard::new_attempts(10))?;
        win32(raw::empty())?;
        win32(raw::set_without_clear(format, bytes))?;
        self.formats.publish_remote_exclusions()?;
        self.image = Some(bytes.to_vec());
        Ok(())
    }

    pub fn poll_image(&mut self) -> Result<Option<Vec<u8>>, ClipboardError> {
        let format = raw::register_format("PNG")
            .ok_or_else(|| ClipboardError::Win32("PNG format unavailable".into()))?
            .get();
        let _clipboard = win32(Clipboard::new_attempts(1))?;
        if self.formats.selection_is_sensitive() || !raw::is_format_avail(format) {
            return Ok(None);
        }
        let size = raw::size(format).map_or(0, |size| size.get());
        if size == 0 || size > crate::clipboard_image::LIMIT {
            return Ok(None);
        }
        let mut bytes = vec![0; size];
        let copied = win32(raw::get(format, &mut bytes))?;
        bytes.truncate(copied);
        crate::clipboard_image::validate_png(&bytes)
            .map_err(|error| ClipboardError::Win32(error.to_string()))?;
        if self.image.as_ref() == Some(&bytes) {
            return Ok(None);
        }
        self.image = Some(bytes.clone());
        Ok(Some(bytes))
    }
}

impl Default for WindowsClipboard {
    fn default() -> Self {
        Self::new()
    }
}

fn sequence_number() -> Option<u32> {
    raw::seq_num().map(|number| number.get())
}

/// Byte length of the UTF-16 payload before its NUL terminator, or `None` when
/// no terminator appears in `bytes` (the selection is larger than the cap).
fn wide_string_len(bytes: &[u8]) -> Option<usize> {
    bytes
        .chunks_exact(2)
        .position(|unit| unit == [0, 0])
        .map(|units| units * 2)
}

fn format_is_false(format: u32) -> bool {
    if !raw::is_format_avail(format) {
        return false;
    }
    let mut bytes = Vec::new();
    if RawData(format).read_clipboard(&mut bytes).is_err() {
        return true;
    }
    if bytes.len() < 4 {
        return true;
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 0
}
