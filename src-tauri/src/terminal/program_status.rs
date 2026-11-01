//! Per-terminal store for OSC 7501 program status records.
//!
//! libghostty parses and validates reports but keeps nothing; the caller owns the records. This
//! module applies the record rules from the `GhosttyTerminalProgramStatusFn` contract and the
//! Program Status Protocol spec: a report replaces its record by id, `clear` removes the record and
//! its subtree (an empty id removes everything), `app` is inherited from the nearest ancestor,
//! `working`/`blocked`/`idle` records drop at a new shell prompt or program exit, `done`/`error`
//! records stay until acknowledged, and at most [`MAX_RECORDS`] records are kept with the least
//! recently updated evicted first.
//!
//! Report text is untrusted. [`sanitize`] and [`sanitize_title`] strip invisible formatting and
//! control characters and enforce the spec's decoded size limits before anything is stored.

use std::collections::HashMap;

/// Records kept per terminal. The spec caps this at 256 and requires at least 64.
pub const MAX_RECORDS: usize = 256;
/// Largest decoded `msg` the spec allows, in UTF-8 bytes.
pub const MAX_MESSAGE_BYTES: usize = 2048;
/// Largest decoded `title` the spec allows, in UTF-8 bytes.
pub const MAX_TITLE_BYTES: usize = 192;

/// The `state` key of a report. `Clear` is a command, never a stored state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    Clear,
}

/// The `kind` key of a `blocked` report: what the program needs from the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

/// One owned report, copied out of the libghostty callback. An empty `id` addresses the root
/// record. `progress` is the raw value (`-1` or absent means indeterminate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusReport {
    pub id: String,
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub progress: Option<i8>,
}

/// What a record says once inheritance and sanitizing are applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusSummary {
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub progress: Option<u8>,
}

#[derive(Debug, Clone)]
struct Record {
    state: ProgramState,
    kind: Option<BlockedKind>,
    app: Option<String>,
    title: Option<String>,
    message: Option<String>,
    progress: Option<u8>,
    updated: u64,
    /// The summary revision the mirror took, under its lock, for the change that applied this
    /// record (see [`ProgramStatusRecords::stamp`]); `None` until then.
    revision: Option<u64>,
}

/// The record set of one terminal.
#[derive(Debug, Default)]
pub struct ProgramStatusRecords {
    records: HashMap<String, Record>,
    clock: u64,
}

impl ProgramStatusRecords {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one report: `clear` removes the addressed subtree, anything else replaces the
    /// record with that id completely.
    pub fn apply(&mut self, report: ProgramStatusReport) {
        if report.state == ProgramState::Clear {
            self.clear(&report.id);
            return;
        }
        if !self.records.contains_key(&report.id) && self.records.len() >= MAX_RECORDS {
            self.evict_oldest();
        }
        self.clock += 1;
        let progress_allowed = matches!(report.state, ProgramState::Working | ProgramState::Blocked);
        let record = Record {
            state: report.state,
            kind: report.kind.filter(|_| report.state == ProgramState::Blocked),
            app: report.app.filter(|app| !app.is_empty()),
            title: report.title.map(|title| sanitize_title(&title)).filter(|title| !title.is_empty()),
            message: report.message.map(|message| sanitize(&message)).filter(|message| !message.is_empty()),
            progress: report
                .progress
                .filter(|value| progress_allowed && (0..=100).contains(value))
                .and_then(|value| u8::try_from(value).ok()),
            updated: self.clock,
            revision: None,
        };
        self.records.insert(report.id, record);
    }

    /// Stamps every record applied since the last stamp with `revision`, the summary revision
    /// the caller took for them while it holds the lock that orders this store's changes.
    /// Returns whether one of them is a `working`, `blocked` or `idle` record: its program must
    /// be seen gone before it is released, even when it did not change the summary.
    pub fn stamp(&mut self, revision: u64) -> bool {
        let mut transient = false;
        for record in self.records.values_mut() {
            if record.revision.is_none() {
                record.revision = Some(revision);
                transient |= !matches!(record.state, ProgramState::Done | ProgramState::Error);
            }
        }
        transient
    }

    /// A new shell prompt started (OSC 133 `A`): drop `working`, `blocked` and `idle` records.
    pub fn prompt_started(&mut self) {
        self.drop_transient();
    }

    /// The program attached to the terminal exited, as proven by evidence gathered while
    /// `through` was the newest published revision: drops the `working`, `blocked` and `idle`
    /// records stamped at or before it. A record stamped later (or not stamped yet) was applied
    /// after that evidence was gathered and may belong to a program that is still running.
    pub fn program_exited(&mut self, through: u64) {
        self.records.retain(|_, record| {
            matches!(record.state, ProgramState::Done | ProgramState::Error)
                || record.revision.is_none_or(|revision| revision > through)
        });
    }

    /// The user has seen the terminal (for example it was focused): drop `done` and `error`.
    pub fn acknowledge(&mut self) {
        self.records
            .retain(|_, record| !matches!(record.state, ProgramState::Done | ProgramState::Error));
    }

    /// The terminal-level summary: the root record when present, else the most recently updated.
    pub fn summary(&self) -> Option<ProgramStatusSummary> {
        if self.records.contains_key("") {
            return self.record("");
        }
        let (id, _) = self.records.iter().max_by_key(|(_, record)| record.updated)?;
        self.record(id)
    }

    /// The record with `id`, with `app` filled in from its nearest ancestor that has one.
    pub fn record(&self, id: &str) -> Option<ProgramStatusSummary> {
        let record = self.records.get(id)?;
        Some(ProgramStatusSummary {
            state: record.state,
            kind: record.kind,
            app: record.app.clone().or_else(|| self.inherited_app(id)),
            title: record.title.clone(),
            message: record.message.clone(),
            progress: record.progress,
        })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    fn clear(&mut self, id: &str) {
        if id.is_empty() {
            self.records.clear();
            return;
        }
        self.records.retain(|key, _| !is_in_subtree(key, id));
    }

    fn drop_transient(&mut self) {
        self.records
            .retain(|_, record| matches!(record.state, ProgramState::Done | ProgramState::Error));
    }

    fn evict_oldest(&mut self) {
        let oldest = self
            .records
            .iter()
            .min_by_key(|(_, record)| record.updated)
            .map(|(id, _)| id.clone());
        if let Some(id) = oldest {
            self.records.remove(&id);
        }
    }

    fn inherited_app(&self, id: &str) -> Option<String> {
        if id.is_empty() {
            return None;
        }
        let mut current = id;
        loop {
            let parent = current.rfind('/').map_or("", |slash| &current[..slash]);
            if let Some(app) = self.records.get(parent).and_then(|record| record.app.as_ref()) {
                return Some(app.clone());
            }
            if parent.is_empty() {
                return None;
            }
            current = parent;
        }
    }
}

fn is_in_subtree(key: &str, id: &str) -> bool {
    key.strip_prefix(id).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Strips control and invisible formatting characters and caps the result at
/// [`MAX_MESSAGE_BYTES`] on a character boundary. Use for `msg`.
pub fn sanitize(text: &str) -> String {
    sanitize_to(text, MAX_MESSAGE_BYTES)
}

/// [`sanitize`] with the `title` limit of [`MAX_TITLE_BYTES`].
pub fn sanitize_title(text: &str) -> String {
    sanitize_to(text, MAX_TITLE_BYTES)
}

fn sanitize_to(text: &str, limit: usize) -> String {
    let mut out: String = text
        .chars()
        .filter(|ch| !ch.is_control() && !is_format_char(*ch))
        .collect();
    if out.len() > limit {
        let mut end = limit;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    out
}

/// Unicode general category Cf (format characters): bidi controls, zero-width characters, BOM,
/// interlinear annotation marks, tag characters and similar invisible formatting.
fn is_format_char(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: &str, state: ProgramState) -> ProgramStatusReport {
        ProgramStatusReport {
            id: id.to_owned(),
            state,
            kind: None,
            app: None,
            title: None,
            message: None,
            progress: None,
        }
    }

    #[test]
    fn report_replaces_record_completely() {
        let mut records = ProgramStatusRecords::new();
        records.apply(ProgramStatusReport {
            app: Some("brew".into()),
            title: Some("Updates".into()),
            message: Some("Installing".into()),
            progress: Some(40),
            ..report("", ProgramState::Working)
        });
        records.apply(report("", ProgramState::Done));

        let summary = records.summary().expect("root record");
        assert_eq!(summary.state, ProgramState::Done);
        assert_eq!(summary.app, None);
        assert_eq!(summary.title, None);
        assert_eq!(summary.message, None);
        assert_eq!(summary.progress, None);
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn clear_removes_record_and_subtree_only() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("build", ProgramState::Working));
        records.apply(report("build/test", ProgramState::Working));
        records.apply(report("builder", ProgramState::Working));
        records.apply(report("", ProgramState::Idle));

        records.apply(report("build", ProgramState::Clear));

        assert!(records.record("build").is_none());
        assert!(records.record("build/test").is_none());
        assert!(records.record("builder").is_some());
        assert!(records.record("").is_some());
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn clear_of_parent_removes_child_without_parent_record() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("build/test", ProgramState::Working));
        records.apply(report("build", ProgramState::Clear));
        assert!(records.is_empty());
    }

    #[test]
    fn clear_with_empty_id_removes_every_record() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("", ProgramState::Working));
        records.apply(report("a", ProgramState::Done));
        records.apply(report("a/b", ProgramState::Error));

        records.apply(report("", ProgramState::Clear));

        assert!(records.is_empty());
        assert!(records.summary().is_none());
    }

    #[test]
    fn app_is_inherited_from_nearest_ancestor() {
        let mut records = ProgramStatusRecords::new();
        records.apply(ProgramStatusReport { app: Some("deploy".into()), ..report("", ProgramState::Working) });
        records.apply(ProgramStatusReport { app: Some("cargo".into()), ..report("us", ProgramState::Working) });
        records.apply(report("us/east/a", ProgramState::Working));
        records.apply(report("eu", ProgramState::Working));
        records.apply(ProgramStatusReport { app: Some("own".into()), ..report("us/west", ProgramState::Working) });

        assert_eq!(records.record("us/east/a").and_then(|r| r.app).as_deref(), Some("cargo"));
        assert_eq!(records.record("eu").and_then(|r| r.app).as_deref(), Some("deploy"));
        assert_eq!(records.record("us/west").and_then(|r| r.app).as_deref(), Some("own"));
    }

    #[test]
    fn app_inheritance_yields_none_without_ancestor_app() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("", ProgramState::Working));
        records.apply(report("job", ProgramState::Working));
        assert_eq!(records.record("job").and_then(|r| r.app), None);
        assert_eq!(records.record("").and_then(|r| r.app), None);
    }

    #[test]
    fn prompt_start_drops_working_blocked_and_idle_but_keeps_done_and_error() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("w", ProgramState::Working));
        records.apply(report("b", ProgramState::Blocked));
        records.apply(report("i", ProgramState::Idle));
        records.apply(report("d", ProgramState::Done));
        records.apply(report("e", ProgramState::Error));

        records.prompt_started();

        assert!(records.record("w").is_none());
        assert!(records.record("b").is_none());
        assert!(records.record("i").is_none());
        assert!(records.record("d").is_some());
        assert!(records.record("e").is_some());
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn program_exit_drops_working_blocked_and_idle_but_keeps_done_and_error() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("w", ProgramState::Working));
        records.apply(report("b", ProgramState::Blocked));
        records.apply(report("i", ProgramState::Idle));
        records.apply(report("d", ProgramState::Done));
        records.apply(report("e", ProgramState::Error));
        records.stamp(1);

        records.program_exited(1);

        assert!(records.record("w").is_none());
        assert!(records.record("b").is_none());
        assert!(records.record("i").is_none());
        assert!(records.record("d").is_some());
        assert!(records.record("e").is_some());
        assert_eq!(records.len(), 2);
    }

    /// The exit evidence was gathered while revision 1 was the newest: a record applied after
    /// it (stamped 2, or not stamped yet) may come from a program that evidence never saw.
    #[test]
    fn program_exit_keeps_records_applied_after_its_evidence() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("old", ProgramState::Working));
        records.stamp(1);
        records.apply(report("late", ProgramState::Blocked));
        records.stamp(2);
        records.apply(report("unstamped", ProgramState::Working));

        records.program_exited(1);

        assert!(records.record("old").is_none());
        assert!(records.record("late").is_some());
        assert!(records.record("unstamped").is_some());
        // A replaced record is a new application with a new stamp.
        records.stamp(3);
        records.apply(report("late", ProgramState::Working));
        records.stamp(4);
        records.program_exited(3);
        assert!(records.record("late").is_some());
        assert!(records.record("unstamped").is_none());
    }

    /// Only a transient record applied since the previous stamp is reported, once.
    #[test]
    fn stamp_reports_newly_applied_transient_records_once() {
        let mut records = ProgramStatusRecords::new();
        assert!(!records.stamp(1), "nothing applied");
        records.apply(report("d", ProgramState::Done));
        assert!(!records.stamp(2), "an outcome needs no release");
        records.apply(report("w", ProgramState::Working));
        assert!(records.stamp(3));
        assert!(!records.stamp(4), "already stamped");
        records.apply(report("i", ProgramState::Idle));
        records.apply(report("e", ProgramState::Error));
        assert!(records.stamp(5));
    }

    #[test]
    fn acknowledge_drops_done_and_error_only() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("w", ProgramState::Working));
        records.apply(report("d", ProgramState::Done));
        records.apply(report("e", ProgramState::Error));

        records.acknowledge();

        assert!(records.record("w").is_some());
        assert!(records.record("d").is_none());
        assert!(records.record("e").is_none());
    }

    #[test]
    fn records_past_cap_evict_least_recently_updated() {
        let mut records = ProgramStatusRecords::new();
        for index in 0..MAX_RECORDS {
            records.apply(report(&format!("r{index}"), ProgramState::Working));
        }
        // Touching r0 makes r1 the least recently updated.
        records.apply(report("r0", ProgramState::Working));
        assert_eq!(records.len(), MAX_RECORDS);

        records.apply(report("r256", ProgramState::Working));

        assert_eq!(records.len(), MAX_RECORDS);
        assert!(records.record("r1").is_none());
        assert!(records.record("r0").is_some());
        assert!(records.record("r256").is_some());
    }

    #[test]
    fn replacing_existing_record_at_cap_evicts_nothing() {
        let mut records = ProgramStatusRecords::new();
        for index in 0..MAX_RECORDS {
            records.apply(report(&format!("r{index}"), ProgramState::Working));
        }
        records.apply(report("r0", ProgramState::Done));
        assert_eq!(records.len(), MAX_RECORDS);
        assert!(records.record("r1").is_some());
    }

    #[test]
    fn summary_prefers_root_record() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("", ProgramState::Working));
        records.apply(report("child", ProgramState::Blocked));
        assert_eq!(records.summary().map(|s| s.state), Some(ProgramState::Working));
    }

    #[test]
    fn summary_without_root_uses_most_recently_updated() {
        let mut records = ProgramStatusRecords::new();
        records.apply(report("a", ProgramState::Working));
        records.apply(report("b", ProgramState::Blocked));
        assert_eq!(records.summary().map(|s| s.state), Some(ProgramState::Blocked));
        records.apply(report("a", ProgramState::Done));
        assert_eq!(records.summary().map(|s| s.state), Some(ProgramState::Done));
        assert!(ProgramStatusRecords::new().summary().is_none());
    }

    #[test]
    fn kind_kept_only_for_blocked() {
        let mut records = ProgramStatusRecords::new();
        records.apply(ProgramStatusReport { kind: Some(BlockedKind::Auth), ..report("b", ProgramState::Blocked) });
        records.apply(ProgramStatusReport { kind: Some(BlockedKind::Question), ..report("w", ProgramState::Working) });
        assert_eq!(records.record("b").and_then(|r| r.kind), Some(BlockedKind::Auth));
        assert_eq!(records.record("w").and_then(|r| r.kind), None);
    }

    #[test]
    fn progress_kept_only_for_working_or_blocked_in_range() {
        let mut records = ProgramStatusRecords::new();
        records.apply(ProgramStatusReport { progress: Some(40), ..report("w", ProgramState::Working) });
        records.apply(ProgramStatusReport { progress: Some(100), ..report("b", ProgramState::Blocked) });
        records.apply(ProgramStatusReport { progress: Some(50), ..report("d", ProgramState::Done) });
        records.apply(ProgramStatusReport { progress: Some(-1), ..report("n", ProgramState::Working) });
        records.apply(ProgramStatusReport { progress: Some(101), ..report("o", ProgramState::Working) });
        assert_eq!(records.record("w").and_then(|r| r.progress), Some(40));
        assert_eq!(records.record("b").and_then(|r| r.progress), Some(100));
        assert_eq!(records.record("d").and_then(|r| r.progress), None);
        assert_eq!(records.record("n").and_then(|r| r.progress), None);
        assert_eq!(records.record("o").and_then(|r| r.progress), None);
    }

    #[test]
    fn stored_text_is_sanitized() {
        let mut records = ProgramStatusRecords::new();
        records.apply(ProgramStatusReport {
            title: Some("a\u{202E}b".into()),
            message: Some("\u{200B}".into()),
            ..report("", ProgramState::Working)
        });
        let summary = records.summary().expect("root record");
        assert_eq!(summary.title.as_deref(), Some("ab"));
        assert_eq!(summary.message, None);
    }

    #[test]
    fn sanitize_removes_bidi_overrides_and_isolates() {
        assert_eq!(sanitize("a\u{202A}b\u{202B}c\u{202C}d\u{202D}e\u{202E}f"), "abcdef");
        assert_eq!(sanitize("a\u{2066}b\u{2067}c\u{2068}d\u{2069}e"), "abcde");
    }

    #[test]
    fn sanitize_removes_zero_width_and_bom() {
        assert_eq!(sanitize("a\u{200B}b\u{200C}c\u{200D}d\u{200E}e\u{200F}f\u{FEFF}g\u{00AD}h"), "abcdefgh");
        assert_eq!(sanitize("tag\u{E0041}\u{E007F}"), "tag");
    }

    #[test]
    fn sanitize_removes_c0_and_c1_controls() {
        assert_eq!(sanitize("a\u{0000}b\u{001B}c\n\td\u{007F}e\u{0085}f\u{009F}g"), "abcdefg");
    }

    #[test]
    fn sanitize_keeps_cjk_emoji_and_accents() {
        let text = "\u{6784}\u{5EFA}\u{5B8C}\u{6210} \u{1F680} caf\u{00E9} \u{D55C}\u{AE00}";
        assert_eq!(sanitize(text), text);
    }

    #[test]
    fn sanitize_truncates_message_on_char_boundary() {
        assert_eq!(sanitize(&"a".repeat(MAX_MESSAGE_BYTES + 10)).len(), MAX_MESSAGE_BYTES);
        // 3-byte chars: 2048 is not a multiple of 3, so the cut lands on 2046.
        let cjk = "\u{4E2D}".repeat(MAX_MESSAGE_BYTES);
        let out = sanitize(&cjk);
        assert_eq!(out.len(), 2046);
        assert!(out.chars().all(|ch| ch == '\u{4E2D}'));
    }

    #[test]
    fn sanitize_title_truncates_on_char_boundary() {
        assert_eq!(sanitize_title(&"t".repeat(300)).len(), MAX_TITLE_BYTES);
        // 4-byte emoji: 192 is a multiple of 4.
        assert_eq!(sanitize_title(&"\u{1F680}".repeat(100)).len(), 192);
        // Mixed: one leading byte then 4-byte chars, so the cut lands on 189.
        let mixed = format!("x{}", "\u{1F680}".repeat(100));
        assert_eq!(sanitize_title(&mixed).len(), 189);
    }

    #[test]
    fn sanitize_strips_before_truncating() {
        let text = format!("{}{}", "\u{200B}".repeat(500), "ok");
        assert_eq!(sanitize_title(&text), "ok");
    }
}
