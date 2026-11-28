//! Deterministic, copied snapshot data types for terminal grid and state.

use std::borrow::Borrow;
use std::ffi::c_int;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use super::color::ColorRgb;
use super::cursor::CursorSnapshot;
use super::error::NativeTerminalError;

/// Inline-first UTF-8 cell text representation with zero external dependencies.
///
/// Stores grapheme clusters up to 22 UTF-8 bytes inline (covering virtually all
/// ASCII, CJK, combining mark, and emoji cells with zero heap allocation) and
/// spills longer sequences to a `Box<str>`.
#[derive(Clone)]
pub enum CellText {
    Inline { len: u8, buf: [u8; 22] },
    Heap(Box<str>),
}

const _: () = assert!(std::mem::size_of::<CellText>() == 24);

impl CellText {
    /// Maximum UTF-8 byte length stored inline without heap allocation.
    pub const INLINE_CAPACITY: usize = 22;

    /// Returns the cell text as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inline { len, buf } => {
                let n = usize::from(*len).min(buf.len());
                std::str::from_utf8(&buf[..n]).unwrap_or("")
            }
            Self::Heap(s) => s,
        }
    }

    /// Returns the length of the cell text in UTF-8 bytes.
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Inline { len, buf } => usize::from(*len).min(buf.len()),
            Self::Heap(s) => s.len(),
        }
    }

    /// Returns `true` if the cell text is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for CellText {
    #[inline]
    fn default() -> Self {
        Self::Inline {
            len: 0,
            buf: [0u8; Self::INLINE_CAPACITY],
        }
    }
}

impl Deref for CellText {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for CellText {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for CellText {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for CellText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for CellText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq for CellText {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CellText {}

impl PartialEq<str> for CellText {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<CellText> for str {
    #[inline]
    fn eq(&self, other: &CellText) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<&str> for CellText {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<CellText> for &str {
    #[inline]
    fn eq(&self, other: &CellText) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<String> for CellText {
    #[inline]
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<CellText> for String {
    #[inline]
    fn eq(&self, other: &CellText) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Hash for CellText {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl From<&str> for CellText {
    #[inline]
    fn from(s: &str) -> Self {
        let bytes = s.as_bytes();
        if bytes.len() <= Self::INLINE_CAPACITY {
            let mut buf = [0u8; Self::INLINE_CAPACITY];
            buf[..bytes.len()].copy_from_slice(bytes);
            Self::Inline {
                len: bytes.len() as u8,
                buf,
            }
        } else {
            Self::Heap(Box::<str>::from(s))
        }
    }
}

impl From<String> for CellText {
    #[inline]
    fn from(s: String) -> Self {
        if s.len() <= Self::INLINE_CAPACITY {
            Self::from(s.as_str())
        } else {
            Self::Heap(s.into_boxed_str())
        }
    }
}

impl From<char> for CellText {
    #[inline]
    fn from(c: char) -> Self {
        let mut buf = [0u8; Self::INLINE_CAPACITY];
        let len = c.encode_utf8(&mut buf).len() as u8;
        Self::Inline { len, buf }
    }
}

impl serde::Serialize for CellText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for CellText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CellTextVisitor;

        impl<'de> serde::de::Visitor<'de> for CellTextVisitor {
            type Value = CellText;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a UTF-8 string")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(CellText::from(v))
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(CellText::from(v))
            }
        }

        deserializer.deserialize_str(CellTextVisitor)
    }
}

/// Cell character width semantics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CellWide {
    /// Single column standard character width.
    Narrow,
    /// Multi-column / double-width character (e.g. CJK, emoji).
    Wide,
    /// Spacer tail following a wide character.
    SpacerTail,
    /// Spacer head before a soft-wrapped wide character.
    SpacerHead,
}

impl TryFrom<c_int> for CellWide {
    type Error = NativeTerminalError;

    fn try_from(w: c_int) -> Result<Self, Self::Error> {
        match w {
            0 => Ok(Self::Narrow),
            1 => Ok(Self::Wide),
            2 => Ok(Self::SpacerTail),
            3 => Ok(Self::SpacerHead),
            other => Err(NativeTerminalError::ForeignErrorCode(other)),
        }
    }
}

/// Fully-owned snapshot of a single grid cell.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellSnapshot {
    /// Renderable text / grapheme cluster (e.g. "A", "東", "🦀", "e\u{0301}").
    /// Empty string indicates a blank / unset cell.
    pub text: CellText,
    /// Cell width classification.
    pub wide: CellWide,
    /// Resolved foreground color, if explicitly set.
    pub fg: Option<ColorRgb>,
    /// Resolved background color, if explicitly set.
    pub bg: Option<ColorRgb>,
    /// Protocol color identifier of the foreground color: the palette index for an
    /// indexed color, otherwise the 24-bit RGB value. Kitty unicode placeholders
    /// carry an image id here. `None` when the cell sets no foreground color.
    #[serde(skip)]
    pub fg_ident: Option<u32>,
    /// Protocol color identifier of the underline color, which carries the
    /// placement id for Kitty unicode placeholders. `None` when unset.
    #[serde(skip)]
    pub underline_ident: Option<u32>,
    /// Bold text decoration.
    pub bold: bool,
    /// Italic text decoration.
    pub italic: bool,
    /// Underline text decoration.
    pub underline: bool,
    /// Inverse / reverse video text decoration.
    pub inverse: bool,
    /// Faint / dim text decoration (SGR 2).
    pub faint: bool,
    /// Blinking text decoration (SGR 5).
    ///
    /// Decoded and carried through the row hash, but deliberately not animated:
    /// blinking would require a timer-driven repaint loop that the render-on-
    /// demand surface host does not have, and blinking text is discouraged for
    /// accessibility. Blinking cells therefore render as ordinary text. Do not
    /// mistake this for the bug where `underline` was hashed yet drawn nowhere
    /// (see docs/NATIVE_TERMINAL_RENDERING_FIXES_2026-08-29.md); the other
    /// attributes here are all consumed by the instance builder.
    pub blink: bool,
    /// Invisible / concealed text decoration (SGR 8).
    pub invisible: bool,
    /// Strikethrough / crossed-out text decoration (SGR 9).
    pub strikethrough: bool,
    /// Overline text decoration (SGR 53).
    pub overline: bool,
}

impl Default for CellSnapshot {
    fn default() -> Self {
        Self {
            text: CellText::default(),
            wide: CellWide::Narrow,
            fg: None,
            bg: None,
            fg_ident: None,
            underline_ident: None,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            faint: false,
            blink: false,
            invisible: false,
            strikethrough: false,
            overline: false,
        }
    }
}

/// Fully-owned, independent render snapshot of terminal screen and state.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenderSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor: CursorSnapshot,
    pub grid: Vec<Vec<CellSnapshot>>,
    /// Native-only image placements. The remote text snapshot wire stays unchanged.
    #[serde(skip)]
    pub images: Vec<super::images::ImagePlacementSnapshot>,
}

impl RenderSnapshot {
    /// Convenience helper to access a cell reference safely with bounds check.
    pub fn cell(&self, col: usize, row: usize) -> Option<&CellSnapshot> {
        self.grid.get(row).and_then(|r| r.get(col))
    }

    /// Convenience helper to reconstruct a text line representation of a row.
    pub fn row_text(&self, row: usize) -> String {
        let Some(cells) = self.grid.get(row) else {
            return String::new();
        };

        let mut line = String::with_capacity(cells.len());
        for cell in cells {
            match cell.wide {
                CellWide::SpacerTail | CellWide::SpacerHead => {}
                CellWide::Narrow | CellWide::Wide => {
                    if cell.text.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(&cell.text);
                    }
                }
            }
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::{CellSnapshot, CellText};

    fn hash_value<T: Hash + ?Sized>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn cell_text_inline_boundary_and_utf8_storage() {
        assert_eq!(std::mem::size_of::<CellText>(), 24);

        let empty = CellText::default();
        assert!(matches!(empty, CellText::Inline { len: 0, .. }));
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.as_str(), "");

        let exact_inline = "a".repeat(22);
        let inline = CellText::from(exact_inline.as_str());
        assert!(matches!(inline, CellText::Inline { len: 22, .. }));
        assert_eq!(inline.len(), 22);
        assert_eq!(inline, exact_inline);
        assert_eq!(inline, exact_inline.as_str());

        let cjk = CellText::from("東京");
        assert!(matches!(cjk, CellText::Inline { len: 6, .. }));
        assert_eq!(cjk, "東京");

        let emoji = CellText::from("👨‍👩‍👧‍👦");
        // Family ZWJ emoji is 25 UTF-8 bytes (> 22), so it spills to Heap.
        assert_eq!(emoji.len(), 25);
        assert!(matches!(emoji, CellText::Heap(_)));
        assert_eq!(emoji, "👨‍👩‍👧‍👦");

        let crab = CellText::from("🦀");
        assert!(matches!(crab, CellText::Inline { len: 4, .. }));
        assert_eq!(crab, "🦀");

        let from_char = CellText::from('한');
        assert!(matches!(from_char, CellText::Inline { len: 3, .. }));
        assert_eq!(from_char, "한");
    }

    #[test]
    fn cell_text_heap_spill_and_from_string() {
        let long = "x".repeat(23);
        let from_str = CellText::from(long.as_str());
        let from_string = CellText::from(long.clone());
        assert!(matches!(from_str, CellText::Heap(_)));
        assert!(matches!(from_string, CellText::Heap(_)));
        assert_eq!(from_str, from_string);
        assert_eq!(from_str.as_str(), long.as_str());
        assert_eq!(from_str.len(), 23);
        assert!(!from_str.is_empty());

        let short_string = CellText::from("hello".to_string());
        assert!(matches!(short_string, CellText::Inline { len: 5, .. }));
        assert_eq!(short_string, "hello");
    }

    #[test]
    fn cell_text_hash_and_partial_eq_match_str() {
        let exact = "z".repeat(22);
        let long = "z".repeat(40);
        let samples = ["", "A", "東", "🦀", "e\u{0301}", exact.as_str(), long.as_str()];
        for &sample in &samples {
            let ct = CellText::from(sample);
            assert_eq!(hash_value(&ct), hash_value(sample));
            assert_eq!(ct, sample);
            assert_eq!(sample, ct);
            assert!(ct == *sample);
            assert!(*sample == ct);
            assert_eq!(ct, sample.to_string());
            assert_eq!(sample.to_string(), ct);
            assert_eq!(format!("{ct}"), sample);
            assert_eq!(format!("{ct:?}"), format!("{sample:?}"));
        }

        // Manually constructed Inline vs Heap with identical text compare equal and hash identically.
        let inline = CellText::from("abc");
        let heap = CellText::Heap(Box::<str>::from("abc"));
        assert_eq!(inline, heap);
        assert_eq!(hash_value(&inline), hash_value(&heap));
    }

    #[test]
    fn cell_text_serde_json_round_trip() {
        let exact = "k".repeat(22);
        let long = "k".repeat(32);
        let samples = ["", "A", "東", "🦀", "e\u{0301}", exact.as_str(), long.as_str()];
        for sample in samples {
            let ct = CellText::from(sample);
            let json = serde_json::to_string(&ct).expect("serialize CellText");
            assert_eq!(json, serde_json::to_string(sample).unwrap());
            let decoded: CellText = serde_json::from_str(&json).expect("deserialize CellText");
            assert_eq!(decoded, ct);
            if sample.len() <= CellText::INLINE_CAPACITY {
                assert!(matches!(decoded, CellText::Inline { .. }));
            } else {
                assert!(matches!(decoded, CellText::Heap(_)));
            }
        }

        // Also test full CellSnapshot JSON round-trip with escaped unicode.
        let cell = CellSnapshot {
            text: CellText::from("e\u{0301}"),
            bold: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&cell).expect("serialize CellSnapshot");
        let decoded: CellSnapshot = serde_json::from_str(&json).expect("deserialize CellSnapshot");
        assert_eq!(decoded, cell);
    }
}
