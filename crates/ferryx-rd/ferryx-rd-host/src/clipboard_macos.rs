//! macOS text and PNG clipboard synchronization over `NSPasteboard`.
//!
//! `NSPasteboard.changeCount` is the analogue of Win32's
//! `GetClipboardSequenceNumber`, so this backend reuses the host's
//! sequence-based [`ClipboardEchoSuppressor`]: a remote write records the
//! resulting change count and text hash so the next poll does not bounce the
//! client's own text back to it.
//!
//! The general pasteboard belongs to the logged-in Aqua session. The shipped
//! host runs as a per-user LaunchAgent (`/Applications/MahoRDHost.app`), so it
//! sees the user's pasteboard. A LaunchDaemon or pre-login context has no
//! pasteboard server; AppKit then reports writes as rejected and reads as
//! empty, which this backend surfaces as [`ClipboardError::Unavailable`] or
//! `Ok(None)` instead of failing the session.
//!
//! Selections marked `org.nspasteboard.ConcealedType` / `TransientType`
//! (password managers) are never sent to the client.

use ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES;
use std::time::Duration;
use thiserror::Error;

use crate::windows_logic::ClipboardEchoSuppressor;

pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

const CONCEALED_TYPES: [&str; 2] = [
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("macOS pasteboard unavailable in this session")]
    Unavailable,
    #[error("clipboard text exceeds the 4 KiB protocol limit")]
    TooLarge,
    #[error("invalid clipboard image: {0}")]
    Image(#[from] std::io::Error),
}

/// Minimal pasteboard surface the synchronizer needs; faked in tests.
pub trait Pasteboard {
    fn change_count(&self) -> i64;
    fn types(&self) -> Vec<String>;
    fn text(&self) -> Option<String>;
    fn png(&self) -> Option<Vec<u8>>;
    /// Replaces the pasteboard contents; returns false when AppKit rejects it.
    fn set_text(&self, text: &str) -> bool;
    fn set_png(&self, bytes: &[u8]) -> bool;
}

/// `NSPasteboard.generalPasteboard`. Each call runs inside an autorelease pool
/// because the session loop is a plain Rust thread without one.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeneralPasteboard;

impl Pasteboard for GeneralPasteboard {
    fn change_count(&self) -> i64 {
        objc2::rc::autoreleasepool(|_| {
            objc2_app_kit::NSPasteboard::generalPasteboard().changeCount() as i64
        })
    }

    fn types(&self) -> Vec<String> {
        objc2::rc::autoreleasepool(|_| {
            objc2_app_kit::NSPasteboard::generalPasteboard()
                .types()
                .map(|types| types.iter().map(|ty| ty.to_string()).collect())
                .unwrap_or_default()
        })
    }

    fn text(&self) -> Option<String> {
        objc2::rc::autoreleasepool(|_| {
            let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
            // SAFETY: NSPasteboardTypeString is an immutable AppKit constant.
            unsafe { pasteboard.stringForType(objc2_app_kit::NSPasteboardTypeString) }
                .map(|text| text.to_string())
        })
    }

    fn png(&self) -> Option<Vec<u8>> {
        objc2::rc::autoreleasepool(|_| {
            let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
            // SAFETY: NSPasteboardTypePNG is an immutable AppKit constant.
            let data = unsafe { pasteboard.dataForType(objc2_app_kit::NSPasteboardTypePNG) }?;
            (data.len() <= crate::clipboard_image::LIMIT).then(|| data.to_vec())
        })
    }

    fn set_text(&self, text: &str) -> bool {
        objc2::rc::autoreleasepool(|_| {
            let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
            let text = objc2_foundation::NSString::from_str(text);
            pasteboard.clearContents();
            // SAFETY: NSPasteboardTypeString is an immutable AppKit constant.
            unsafe { pasteboard.setString_forType(&text, objc2_app_kit::NSPasteboardTypeString) }
        })
    }

    fn set_png(&self, bytes: &[u8]) -> bool {
        objc2::rc::autoreleasepool(|_| {
            let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
            let data = objc2_foundation::NSData::with_bytes(bytes);
            pasteboard.clearContents();
            // SAFETY: NSPasteboardTypePNG is an immutable AppKit constant.
            unsafe { pasteboard.setData_forType(Some(&data), objc2_app_kit::NSPasteboardTypePNG) }
        })
    }
}

/// `changeCount` is an NSInteger; the shared suppressor compares u32
/// sequences, and only equality matters, so truncation is harmless.
fn sequence(change_count: i64) -> u32 {
    change_count as u32
}

pub struct MacClipboard<P: Pasteboard = GeneralPasteboard> {
    pasteboard: P,
    suppressor: ClipboardEchoSuppressor,
    image: Option<Vec<u8>>,
    image_change_count: Option<i64>,
}

impl MacClipboard<GeneralPasteboard> {
    pub fn new() -> Self {
        Self::with_pasteboard(GeneralPasteboard)
    }
}

impl Default for MacClipboard<GeneralPasteboard> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Pasteboard> MacClipboard<P> {
    /// Starts from the current change count so whatever the user already had
    /// copied is not pushed to a freshly connected client.
    pub fn with_pasteboard(pasteboard: P) -> Self {
        let mut suppressor = ClipboardEchoSuppressor::default();
        let current = pasteboard.change_count();
        suppressor.start(sequence(current));
        Self {
            pasteboard,
            suppressor,
            image: None,
            image_change_count: Some(current),
        }
    }

    fn concealed(&self) -> bool {
        self.pasteboard
            .types()
            .iter()
            .any(|ty| CONCEALED_TYPES.contains(&ty.as_str()))
    }

    /// Poll once. `Ok(None)` means unchanged, echo-suppressed, concealed,
    /// empty, non-text, or over the protocol's 4 KiB UTF-8 cap.
    pub fn poll(&mut self) -> Result<Option<String>, ClipboardError> {
        if !self
            .suppressor
            .observe_sequence(sequence(self.pasteboard.change_count()))
        {
            return Ok(None);
        }
        if self.concealed() {
            return Ok(None);
        }
        let Some(text) = self.pasteboard.text() else {
            return Ok(None);
        };
        if text.is_empty() || text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return Ok(None);
        }
        Ok(self.suppressor.observe_text(&text).then_some(text))
    }

    pub fn apply_remote_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        if !self.pasteboard.set_text(text) {
            return Err(ClipboardError::Unavailable);
        }
        let change_count = self.pasteboard.change_count();
        self.suppressor
            .record_remote_write(sequence(change_count), text);
        self.image = None;
        self.image_change_count = Some(change_count);
        Ok(())
    }

    pub fn apply_remote_image(&mut self, bytes: &[u8]) -> Result<(), ClipboardError> {
        crate::clipboard_image::validate_png(bytes)?;
        if !self.pasteboard.set_png(bytes) {
            return Err(ClipboardError::Unavailable);
        }
        let change_count = self.pasteboard.change_count();
        // Mark the text side as seen too, so the image write does not look
        // like a fresh local change to the text poll.
        self.suppressor
            .record_remote_write(sequence(change_count), "");
        self.image = Some(bytes.to_vec());
        self.image_change_count = Some(change_count);
        Ok(())
    }

    /// Reads `public.png` only when the change count moved, so a large image
    /// is not copied out of the pasteboard server every 500 ms.
    pub fn poll_image(&mut self) -> Result<Option<Vec<u8>>, ClipboardError> {
        let change_count = self.pasteboard.change_count();
        if self.image_change_count == Some(change_count) {
            return Ok(None);
        }
        self.image_change_count = Some(change_count);
        if self.concealed() {
            return Ok(None);
        }
        let Some(bytes) = self.pasteboard.png() else {
            self.image = None;
            return Ok(None);
        };
        crate::clipboard_image::validate_png(&bytes)?;
        if self.image.as_ref() == Some(&bytes) {
            return Ok(None);
        }
        self.image = Some(bytes.clone());
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        count: RefCell<i64>,
        types: RefCell<Vec<String>>,
        text: RefCell<Option<String>>,
        png: RefCell<Option<Vec<u8>>>,
        reject_writes: bool,
    }

    impl Fake {
        fn local_copy(&self, text: &str) {
            *self.count.borrow_mut() += 1;
            *self.text.borrow_mut() = Some(text.to_owned());
            *self.png.borrow_mut() = None;
            *self.types.borrow_mut() = vec!["public.utf8-plain-text".to_owned()];
        }
    }

    impl Pasteboard for &Fake {
        fn change_count(&self) -> i64 {
            *self.count.borrow()
        }
        fn types(&self) -> Vec<String> {
            self.types.borrow().clone()
        }
        fn text(&self) -> Option<String> {
            self.text.borrow().clone()
        }
        fn png(&self) -> Option<Vec<u8>> {
            self.png.borrow().clone()
        }
        fn set_text(&self, text: &str) -> bool {
            if self.reject_writes {
                return false;
            }
            self.local_copy(text);
            true
        }
        fn set_png(&self, bytes: &[u8]) -> bool {
            if self.reject_writes {
                return false;
            }
            *self.count.borrow_mut() += 1;
            *self.text.borrow_mut() = None;
            *self.png.borrow_mut() = Some(bytes.to_vec());
            *self.types.borrow_mut() = vec!["public.png".to_owned()];
            true
        }
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3]).unwrap();
        }
        bytes
    }

    #[test]
    fn existing_selection_is_not_sent_on_connect() {
        let fake = Fake::default();
        fake.local_copy("before session");
        let mut clipboard = MacClipboard::with_pasteboard(&fake);
        assert_eq!(clipboard.poll().unwrap(), None);
    }

    #[test]
    fn change_count_poll_sends_each_local_change_once() {
        let fake = Fake::default();
        let mut clipboard = MacClipboard::with_pasteboard(&fake);
        fake.local_copy("hello");
        assert_eq!(clipboard.poll().unwrap().as_deref(), Some("hello"));
        assert_eq!(clipboard.poll().unwrap(), None, "unchanged count");
        fake.local_copy("hello");
        assert_eq!(clipboard.poll().unwrap(), None, "same text recopied");
        fake.local_copy("world");
        assert_eq!(clipboard.poll().unwrap().as_deref(), Some("world"));
    }

    #[test]
    fn remote_text_is_not_echoed_back() {
        let fake = Fake::default();
        let mut clipboard = MacClipboard::with_pasteboard(&fake);
        clipboard.apply_remote_text("from client").unwrap();
        assert_eq!(fake.text.borrow().as_deref(), Some("from client"));
        assert_eq!(clipboard.poll().unwrap(), None);
        assert_eq!(clipboard.poll_image().unwrap(), None);
        fake.local_copy("from host user");
        assert_eq!(clipboard.poll().unwrap().as_deref(), Some("from host user"));
    }

    #[test]
    fn concealed_empty_and_oversized_selections_are_skipped() {
        let fake = Fake::default();
        let mut clipboard = MacClipboard::with_pasteboard(&fake);
        fake.local_copy("secret");
        fake.types
            .borrow_mut()
            .push("org.nspasteboard.ConcealedType".to_owned());
        assert_eq!(clipboard.poll().unwrap(), None);
        fake.local_copy("");
        assert_eq!(clipboard.poll().unwrap(), None);
        fake.local_copy(&"x".repeat(MAX_CLIPBOARD_TEXT_BYTES + 1));
        assert_eq!(clipboard.poll().unwrap(), None);
        assert!(matches!(
            clipboard.apply_remote_text(&"x".repeat(MAX_CLIPBOARD_TEXT_BYTES + 1)),
            Err(ClipboardError::TooLarge)
        ));
    }

    #[test]
    fn rejected_write_reports_unavailable() {
        let fake = Fake {
            reject_writes: true,
            ..Fake::default()
        };
        let mut clipboard = MacClipboard::with_pasteboard(&fake);
        assert!(matches!(
            clipboard.apply_remote_text("x"),
            Err(ClipboardError::Unavailable)
        ));
        assert!(matches!(
            clipboard.apply_remote_image(&png()),
            Err(ClipboardError::Unavailable)
        ));
    }

    #[test]
    fn images_round_trip_without_echo() {
        let fake = Fake::default();
        let mut clipboard = MacClipboard::with_pasteboard(&fake);
        let image = png();
        clipboard.apply_remote_image(&image).unwrap();
        assert_eq!(*fake.png.borrow(), Some(image.clone()));
        assert_eq!(clipboard.poll_image().unwrap(), None, "remote image echo");
        assert_eq!(clipboard.poll().unwrap(), None, "image write is not text");

        fake.local_copy("text");
        assert_eq!(clipboard.poll_image().unwrap(), None);
        *fake.count.borrow_mut() += 1;
        *fake.png.borrow_mut() = Some(image.clone());
        assert_eq!(clipboard.poll_image().unwrap(), Some(image));
        assert!(clipboard.apply_remote_image(b"not a png").is_err());
    }

    /// Round-trips through the real general pasteboard, restoring the
    /// developer's text afterwards. Run with `--ignored` in a GUI session.
    #[test]
    #[ignore = "touches the real macOS pasteboard"]
    fn real_pasteboard_round_trip_restores_selection() {
        let saved = GeneralPasteboard.text();
        let mut clipboard = MacClipboard::new();
        let marker = format!("ferryx-rd-host-pasteboard-test-{}", std::process::id());
        clipboard.apply_remote_text(&marker).unwrap();
        let read_back = GeneralPasteboard.text();
        let echoed = clipboard.poll().unwrap();
        if let Some(saved) = saved {
            assert!(GeneralPasteboard.set_text(&saved));
        }
        assert_eq!(read_back.as_deref(), Some(marker.as_str()));
        assert_eq!(echoed, None);
    }
}
