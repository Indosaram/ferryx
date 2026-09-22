//! Terminal state carried across eviction so retained history renders coherently.
//!
//! Retention drops the oldest bytes, and those bytes may have been the ones that set the
//! current SGR attributes, DEC private modes, or charset. Replaying only the surviving tail
//! would render it with default state. The recorder folds evicted bytes into a compact
//! state and emits it as a prelude before the retained window.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const MAX_CSI_PARAMETER_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    Ground,
    Escape,
    Csi,
    String,
    StringEscape,
    Charset,
}

/// DEC private modes whose loss visibly corrupts a reconstructed pane.
const TRACKED_PRIVATE_MODES: [u16; 11] = [1, 7, 25, 1000, 1002, 1003, 1004, 1005, 1006, 1049, 2004];

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TerminalStatePrelude {
    #[serde(default)]
    sgr: Vec<Vec<u8>>,
    #[serde(default)]
    private_modes: Vec<(u16, bool)>,
    #[serde(default)]
    utf8_charset: bool,
}

impl TerminalStatePrelude {
    pub fn is_empty(&self) -> bool {
        self.sgr.is_empty() && self.private_modes.is_empty() && !self.utf8_charset
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.utf8_charset {
            out.extend_from_slice(b"\x1b%G");
        }
        for (mode, enabled) in &self.private_modes {
            out.extend_from_slice(b"\x1b[?");
            out.extend_from_slice(mode.to_string().as_bytes());
            out.push(if *enabled { b'h' } else { b'l' });
        }
        if !self.sgr.is_empty() {
            out.extend_from_slice(b"\x1b[0m");
            for params in &self.sgr {
                out.extend_from_slice(b"\x1b[");
                out.extend_from_slice(params);
                out.push(b'm');
            }
        }
        out
    }
}

pub struct TerminalStateRecorder {
    scan: Scan,
    params: Vec<u8>,
    params_overflowed: bool,
    private: bool,
    sgr: Vec<Vec<u8>>,
    private_modes: Vec<(u16, bool)>,
    utf8_charset: bool,
}

impl Default for TerminalStateRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalStateRecorder {
    pub fn new() -> Self {
        Self {
            scan: Scan::Ground,
            params: Vec::new(),
            params_overflowed: false,
            private: false,
            sgr: Vec::new(),
            private_modes: Vec::new(),
            utf8_charset: false,
        }
    }

    pub fn prelude(&self) -> TerminalStatePrelude {
        TerminalStatePrelude {
            sgr: self.sgr.clone(),
            private_modes: self.private_modes.clone(),
            utf8_charset: self.utf8_charset,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Restore a previously exported prelude as this recorder's base state so an imported
    /// session rebuilds exactly the state its predecessor had accumulated across evictions.
    /// Future observes layer on top of it, matching a session that never left memory.
    pub fn restore(&mut self, prelude: TerminalStatePrelude) {
        self.scan = Scan::Ground;
        self.params.clear();
        self.params_overflowed = false;
        self.private = false;
        self.sgr = prelude.sgr;
        self.private_modes = prelude.private_modes;
        self.utf8_charset = prelude.utf8_charset;
    }

    pub fn observe(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match self.scan {
                Scan::Ground => match byte {
                    ESC => self.scan = Scan::Escape,
                    _ => {}
                },
                Scan::Escape => match byte {
                    b'[' => {
                        self.params.clear();
                        self.params_overflowed = false;
                        self.private = false;
                        self.scan = Scan::Csi;
                    }
                    b'_' | b'P' | b']' | b'^' | b'X' => self.scan = Scan::String,
                    b'%' => self.scan = Scan::Charset,
                    b'c' => {
                        let restart = Self::new();
                        self.sgr = restart.sgr;
                        self.private_modes = restart.private_modes;
                        self.utf8_charset = restart.utf8_charset;
                        self.scan = Scan::Ground;
                    }
                    ESC => {}
                    _ => self.scan = Scan::Ground,
                },
                Scan::Csi => {
                    if byte == b'?' && self.params.is_empty() {
                        self.private = true;
                    } else if byte.is_ascii_digit() || byte == b';' || byte == b':' {
                        if self.params.len() < MAX_CSI_PARAMETER_BYTES {
                            self.params.push(byte);
                        } else {
                            self.params_overflowed = true;
                        }
                    } else if (0x40..=0x7e).contains(&byte) {
                        if !self.params_overflowed {
                            self.dispatch_csi(byte);
                        }
                        self.scan = Scan::Ground;
                    } else if byte == ESC {
                        self.scan = Scan::Escape;
                    }
                }
                Scan::String => match byte {
                    ESC => self.scan = Scan::StringEscape,
                    BEL => self.scan = Scan::Ground,
                    _ => {}
                },
                Scan::StringEscape => {
                    self.scan = if byte == b'\\' {
                        Scan::Ground
                    } else {
                        Scan::String
                    };
                }
                Scan::Charset => {
                    if byte == b'G' {
                        self.utf8_charset = true;
                    }
                    self.scan = Scan::Ground;
                }
            }
        }
    }

    fn dispatch_csi(&mut self, final_byte: u8) {
        match (final_byte, self.private) {
            (b'm', false) => self.apply_sgr(),
            (b'h', true) | (b'l', true) => self.apply_private_mode(final_byte == b'h'),
            _ => {}
        }
    }

    fn apply_sgr(&mut self) {
        let params = std::mem::take(&mut self.params);
        // `\x1b[m` and `\x1b[0m` both reset, which makes every earlier attribute irrelevant.
        if params.is_empty() || params.split(|b| *b == b';').all(|p| p == b"0" || p.is_empty()) {
            self.sgr.clear();
            return;
        }
        let fields: Vec<&[u8]> = params.split(|byte| *byte == b';').collect();
        let mut index = 0;
        while index < fields.len() {
            let first = fields[index].split(|byte| *byte == b':').next().unwrap_or_default();
            let code = std::str::from_utf8(first).ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(0);
            if code == 0 {
                self.sgr.clear();
                index += 1;
                continue;
            }
            let group = sgr_group(code);
            let count = if matches!(code, 38 | 48 | 58) && !fields[index].contains(&b':') {
                match fields.get(index + 1).copied() {
                    Some(b"5") => 3,
                    Some(b"2") => 5,
                    _ => 1,
                }
            } else { 1 };
            let end = (index + count).min(fields.len());
            if let Some(group) = group {
                self.sgr.retain(|prior| {
                    let first = prior.split(|byte| *byte == b';' || *byte == b':').next().unwrap_or_default();
                    let prior_code = std::str::from_utf8(first).ok().and_then(|value| value.parse::<u16>().ok());
                    prior_code.and_then(sgr_group) != Some(group)
                });
                if code == 22 {
                    self.sgr.retain(|prior| prior != b"2");
                }
                self.sgr.push(fields[index..end].join(&b';'));
            }
            index = end;
        }
    }

    fn apply_private_mode(&mut self, enabled: bool) {
        let params = std::mem::take(&mut self.params);
        for part in params.split(|b| *b == b';') {
            let Ok(text) = std::str::from_utf8(part) else {
                continue;
            };
            let Ok(mode) = text.parse::<u16>() else {
                continue;
            };
            if !TRACKED_PRIVATE_MODES.contains(&mode) {
                continue;
            }
            match self.private_modes.iter_mut().find(|(m, _)| *m == mode) {
                Some(entry) => entry.1 = enabled,
                None => self.private_modes.push((mode, enabled)),
            }
        }
    }
}

fn sgr_group(code: u16) -> Option<u8> {
    Some(match code {
        1 | 22 => 1,
        2 => 2,
        3 | 20 | 23 => 3,
        4 | 21 | 24 => 4,
        5 | 6 | 25 => 5,
        7 | 27 => 7,
        8 | 28 => 8,
        9 | 29 => 9,
        10..=19 => 10,
        30..=39 | 90..=97 => 30,
        40..=49 | 100..=107 => 40,
        51 | 52 | 54 => 51,
        53 | 55 => 53,
        58 | 59 => 58,
        60..=65 => 60,
        73..=75 => 73,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prelude_of(bytes: &[u8]) -> Vec<u8> {
        let mut recorder = TerminalStateRecorder::new();
        recorder.observe(bytes);
        recorder.prelude().encode()
    }

    #[test]
    fn evicted_sgr_attributes_are_restored_before_retained_history() {
        // Given: colour set in bytes that retention later evicts.
        let prelude = prelude_of(b"\x1b[1;38;5;196mred text");

        // Then: the prelude re-establishes that colour for the surviving tail.
        assert_eq!(prelude, b"\x1b[0m\x1b[1m\x1b[38;5;196m");
    }

    #[test]
    fn sgr_reset_discards_earlier_attributes() {
        assert!(prelude_of(b"\x1b[31m\x1b[0m").is_empty());
        assert!(prelude_of(b"\x1b[31m\x1b[m").is_empty());
    }

    #[test]
    fn repeated_color_changes_have_bounded_reconstruction_state() {
        let mut recorder = TerminalStateRecorder::new();
        recorder.observe(b"\x1b[1m");
        for value in 0..10000 {
            recorder.observe(format!("\x1b[38;5;{}m", value % 256).as_bytes());
        }
        let prelude = recorder.prelude().encode();
        assert!(prelude.len() < 128, "SGR history grew to {} bytes", prelude.len());
        assert!(prelude.windows(4).any(|bytes| bytes == b"\x1b[1m"));
        assert!(prelude.windows(10).any(|bytes| bytes == b"\x1b[38;5;15m"));
    }

    #[test]
    fn oversized_csi_and_colon_sgr_cannot_expand_saved_state() {
        let mut recorder = TerminalStateRecorder::new();
        recorder.observe(b"\x1b[32m\x1b[38:");
        for _ in 0..1000 {
            recorder.observe(&[b'1'; 128]);
        }
        assert!(recorder.params.len() <= 1024);
        recorder.observe(b"m");
        assert_eq!(recorder.prelude().encode(), b"\x1b[0m\x1b[32m");
        recorder.observe(b"\x1b[34m");
        assert_eq!(recorder.prelude().encode(), b"\x1b[0m\x1b[34m");
    }

    #[test]
    fn tracked_private_modes_survive_eviction_with_latest_value() {
        // Given: bracketed paste and alt-screen toggled in evicted output.
        let prelude = prelude_of(b"\x1b[?2004h\x1b[?25l\x1b[?2004l\x1b[?2004h");

        // Then: each mode appears once, carrying its final state.
        assert_eq!(prelude, b"\x1b[?2004h\x1b[?25l");
    }

    #[test]
    fn untracked_modes_and_non_private_modes_are_ignored() {
        assert!(prelude_of(b"\x1b[?9999h\x1b[4h").is_empty());
    }

    #[test]
    fn utf8_charset_designation_is_preserved() {
        assert_eq!(prelude_of(b"\x1b%Gtext"), b"\x1b%G");
    }

    #[test]
    fn full_reset_clears_recorded_state() {
        assert!(prelude_of(b"\x1b[31m\x1b[?2004h\x1bc").is_empty());
    }

    #[test]
    fn sgr_inside_a_graphics_payload_is_not_recorded() {
        // Given: an escape-looking byte pattern inside an APC payload.
        let prelude = prelude_of(b"\x1b_Ga=T;AA\x1b[31mAA\x1b\\");

        // Then: string payloads are opaque, so no phantom attribute is restored.
        assert!(prelude.is_empty());
    }

    #[test]
    fn prelude_orders_charset_then_modes_then_attributes() {
        let prelude = prelude_of(b"\x1b%G\x1b[?7l\x1b[33m");
        let charset = prelude.windows(3).position(|w| w == b"\x1b%G").unwrap();
        let mode = prelude.windows(3).position(|w| w == b"\x1b[?").unwrap();
        let sgr = prelude.windows(4).position(|w| w == b"\x1b[0m").unwrap();
        assert!(charset < mode && mode < sgr);
    }
}
