//! DEC mode 2031 color scheme reporting through libghostty-vt.

use crate::native_terminal::engine::TerminalEngine;
use crate::native_terminal::NativeTerminal;
use crate::terminal::{
    preference_cache_test_lock, set_terminal_appearance, terminal_appearance, TerminalAppearance,
};

const DARK_REPORT: &[u8] = b"\x1b[?997;1n";
const LIGHT_REPORT: &[u8] = b"\x1b[?997;2n";

fn written(terminal: &NativeTerminal) -> Vec<Vec<u8>> {
    terminal
        .buffered_pty_writes()
        .into_iter()
        .map(|record| record.data)
        .collect()
}

fn terminal_with_2031() -> NativeTerminal {
    let mut terminal = NativeTerminal::new(80, 24).unwrap();
    terminal.feed_str("\x1b[?2031h").unwrap();
    terminal.discard_buffered_pty_writes();
    terminal
}

#[test]
fn color_scheme_query_answers_the_live_appearance() {
    let _lock = preference_cache_test_lock();
    let previous = terminal_appearance();
    set_terminal_appearance(TerminalAppearance::Light);

    let mut terminal = NativeTerminal::new(80, 24).unwrap();
    terminal.feed_str("\x1b[?996n").unwrap();
    let light = written(&terminal);
    terminal.discard_buffered_pty_writes();

    set_terminal_appearance(TerminalAppearance::Dark);
    terminal.feed_str("\x1b[?996n").unwrap();
    let dark = written(&terminal);

    set_terminal_appearance(previous);
    assert_eq!(light, vec![LIGHT_REPORT.to_vec()]);
    assert_eq!(dark, vec![DARK_REPORT.to_vec()]);
}

#[test]
fn mode_2031_session_gets_unsolicited_report_on_flip() {
    let terminal = terminal_with_2031();

    assert!(terminal
        .report_color_scheme_change(TerminalAppearance::Light)
        .unwrap());
    assert_eq!(written(&terminal), vec![LIGHT_REPORT.to_vec()]);
    terminal.discard_buffered_pty_writes();

    assert!(terminal
        .report_color_scheme_change(TerminalAppearance::Dark)
        .unwrap());
    assert_eq!(written(&terminal), vec![DARK_REPORT.to_vec()]);
}

#[test]
fn session_without_2031_gets_nothing() {
    let mut terminal = NativeTerminal::new(80, 24).unwrap();
    assert!(!terminal
        .report_color_scheme_change(TerminalAppearance::Light)
        .unwrap());
    assert!(written(&terminal).is_empty());

    // A program that enabled and then disabled 2031 is opted out again.
    terminal.feed_str("\x1b[?2031h\x1b[?2031l").unwrap();
    terminal.discard_buffered_pty_writes();
    assert!(!terminal
        .report_color_scheme_change(TerminalAppearance::Dark)
        .unwrap());
    assert!(written(&terminal).is_empty());
}

#[test]
fn no_report_while_writes_suppressed_or_replaying() {
    let mut terminal = terminal_with_2031();

    // History replay runs with PTY writes suppressed: neither an unsolicited report nor a
    // replayed CSI ? 996 n query may reach the live program.
    terminal.set_pty_writes_suppressed(true);
    assert!(!terminal
        .report_color_scheme_change(TerminalAppearance::Light)
        .unwrap());
    terminal.feed_str("\x1b[?996n").unwrap();
    assert!(written(&terminal).is_empty());

    terminal.set_pty_writes_suppressed(false);
    assert!(terminal
        .report_color_scheme_change(TerminalAppearance::Light)
        .unwrap());
    assert_eq!(written(&terminal), vec![LIGHT_REPORT.to_vec()]);
}
