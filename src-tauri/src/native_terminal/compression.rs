//! Idle scrollback compression scheduling for libghostty-vt terminals.
//!
//! libghostty-vt compresses cold scrollback pages only when the embedder asks: it exposes an
//! opaque activity token and a bounded incremental compression step, and creates no timer or
//! thread of its own (`ghostty/vt/terminal.h`, "Scrollback Compression"). The scheduler decides
//! when the owner of a terminal runs the next step; the owner runs it while it already holds the
//! terminal exclusively, because a step is not thread-safe with any other access to the terminal.

use std::time::{Duration, Instant};

use super::engine::TerminalEngine;
use super::error::NativeTerminalError;

/// How long a terminal must stay quiet before its scrollback is compressed.
pub const COMPRESSION_IDLE_DELAY: Duration = Duration::from_secs(2);

/// Pause between incremental steps of one pass. A step compresses at most one page; the pause
/// lets output, rendering and input take the terminal between steps.
pub const COMPRESSION_STEP_INTERVAL: Duration = Duration::from_millis(1);

/// Scheduling result of one incremental compression step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionStep {
    /// Compression cannot free memory on this platform; it is never scheduled again.
    Unsupported,
    /// More work remains in this pass; run another step while the terminal stays idle.
    Pending,
    /// The pass finished; nothing to do until the terminal changes.
    Complete,
}

/// Memory held by a terminal, copied from `GhosttyTerminalMemoryUsage`.
///
/// Resident bytes are the physical memory the pages use now (a compressed page counts only its
/// compressed size) and are the figure to budget against. Image bytes are not part of the
/// resident figure. The alternate screen reports zero until a program first switches to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryUsage {
    pub compression_supported: bool,
    pub primary_pages: u64,
    pub primary_virtual_bytes: u64,
    pub primary_resident_bytes: u64,
    pub primary_compressed_pages: u64,
    pub primary_compressed_bytes: u64,
    pub primary_image_bytes: u64,
    pub alternate_pages: u64,
    pub alternate_virtual_bytes: u64,
    pub alternate_resident_bytes: u64,
    pub alternate_compressed_pages: u64,
    pub alternate_compressed_bytes: u64,
    pub alternate_image_bytes: u64,
}

/// Decides when the next idle compression step of one terminal is due.
///
/// Activity arms the scheduler [`COMPRESSION_IDLE_DELAY`] ahead and every later activity pushes
/// that deadline back. Once due, the owner runs one step per wakeup through
/// [`CompressionScheduler::run_due_step`]: a pending pass stays armed
/// [`COMPRESSION_STEP_INTERVAL`] ahead, a complete pass disarms, and an unsupported platform
/// disarms for good.
#[derive(Debug, Clone)]
pub struct CompressionScheduler {
    supported: bool,
    due_at: Option<Instant>,
    /// A pass is in progress: the last step reported pending work.
    continuing: bool,
    /// Activity token observed at the last step.
    activity: Option<u64>,
}

impl Default for CompressionScheduler {
    fn default() -> Self {
        Self {
            supported: true,
            due_at: None,
            continuing: false,
            activity: None,
        }
    }
}

impl CompressionScheduler {
    /// Records terminal activity at `now`: the idle delay restarts from here.
    pub fn mark_activity(&mut self, now: Instant) {
        if self.supported {
            self.due_at = Some(now + COMPRESSION_IDLE_DELAY);
            self.continuing = false;
        }
    }

    /// Drops any scheduled step until the next activity.
    pub fn disarm(&mut self) {
        self.due_at = None;
        self.continuing = false;
    }

    /// Time from `now` until the next step is due (zero when overdue), or `None` when nothing
    /// is scheduled.
    pub fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        if !self.supported {
            return None;
        }
        self.due_at.map(|due| due.saturating_duration_since(now))
    }

    /// Whether a step is due at `now`.
    pub fn is_due(&self, now: Instant) -> bool {
        self.supported && self.due_at.is_some_and(|due| now >= due)
    }

    /// Runs one incremental step on `engine` when one is due at `now`.
    ///
    /// `enabled` is the user's `scrollback-compression` preference; when it is off the scheduler
    /// disarms without touching the terminal. The caller must hold `engine` exclusively. A failed
    /// step disarms the scheduler so a broken terminal cannot keep waking its owner.
    pub fn run_due_step<E: TerminalEngine + ?Sized>(
        &mut self,
        now: Instant,
        enabled: bool,
        engine: &mut E,
    ) -> Result<Option<CompressionStep>, NativeTerminalError> {
        if !self.is_due(now) {
            return Ok(None);
        }
        if !enabled {
            self.disarm();
            return Ok(None);
        }
        let outcome = self.step(now, engine);
        if outcome.is_err() {
            self.disarm();
        }
        outcome
    }

    fn step<E: TerminalEngine + ?Sized>(
        &mut self,
        now: Instant,
        engine: &mut E,
    ) -> Result<Option<CompressionStep>, NativeTerminalError> {
        let activity = engine.compression_activity()?;
        if !self.should_step(now, activity) {
            return Ok(None);
        }
        let step = engine.compress_step()?;
        self.record(now, step);
        Ok(Some(step))
    }

    /// A changed activity token in the middle of a pass means the terminal changed without
    /// output (resize, scrolling): restart the idle delay instead of stepping.
    fn should_step(&mut self, now: Instant, activity: u64) -> bool {
        let changed = self.continuing && self.activity != Some(activity);
        self.activity = Some(activity);
        if changed {
            self.due_at = Some(now + COMPRESSION_IDLE_DELAY);
            self.continuing = false;
        }
        !changed
    }

    fn record(&mut self, now: Instant, step: CompressionStep) {
        match step {
            CompressionStep::Pending => {
                self.continuing = true;
                self.due_at = Some(now + COMPRESSION_STEP_INTERVAL);
            }
            CompressionStep::Complete => self.disarm(),
            CompressionStep::Unsupported => {
                self.supported = false;
                self.disarm();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::NativeTerminal;

    #[test]
    fn compression_scheduler_never_wakes_before_any_activity() {
        let scheduler = CompressionScheduler::default();
        let now = Instant::now();
        assert_eq!(scheduler.next_wakeup(now), None);
        assert!(!scheduler.is_due(now + Duration::from_secs(3600)));
    }

    #[test]
    fn compression_scheduler_activity_restarts_the_idle_delay() {
        let mut scheduler = CompressionScheduler::default();
        let start = Instant::now();
        scheduler.mark_activity(start);
        assert_eq!(scheduler.next_wakeup(start), Some(COMPRESSION_IDLE_DELAY));
        assert_eq!(
            scheduler.next_wakeup(start + Duration::from_millis(500)),
            Some(Duration::from_millis(1500))
        );
        assert!(!scheduler.is_due(start + Duration::from_millis(1999)));
        assert!(scheduler.is_due(start + COMPRESSION_IDLE_DELAY));
        assert_eq!(scheduler.next_wakeup(start + Duration::from_secs(5)), Some(Duration::ZERO));

        let later = start + Duration::from_millis(1500);
        scheduler.mark_activity(later);
        assert!(!scheduler.is_due(start + COMPRESSION_IDLE_DELAY));
        assert!(scheduler.is_due(later + COMPRESSION_IDLE_DELAY));
    }

    #[test]
    fn compression_scheduler_continues_pending_passes_and_disarms_on_complete() {
        let mut scheduler = CompressionScheduler::default();
        let start = Instant::now();
        scheduler.mark_activity(start);
        let due = start + COMPRESSION_IDLE_DELAY;

        assert!(scheduler.should_step(due, 7));
        scheduler.record(due, CompressionStep::Pending);
        assert_eq!(scheduler.next_wakeup(due), Some(COMPRESSION_STEP_INTERVAL));
        let next = due + COMPRESSION_STEP_INTERVAL;
        assert!(scheduler.is_due(next));

        assert!(scheduler.should_step(next, 7));
        scheduler.record(next, CompressionStep::Complete);
        assert_eq!(scheduler.next_wakeup(next), None);
    }

    #[test]
    fn compression_scheduler_token_change_mid_pass_restarts_the_idle_delay() {
        let mut scheduler = CompressionScheduler::default();
        let start = Instant::now();
        scheduler.mark_activity(start);
        let due = start + COMPRESSION_IDLE_DELAY;
        assert!(scheduler.should_step(due, 1));
        scheduler.record(due, CompressionStep::Pending);

        let next = due + COMPRESSION_STEP_INTERVAL;
        assert!(!scheduler.should_step(next, 2), "a changed token must not step");
        assert_eq!(scheduler.next_wakeup(next), Some(COMPRESSION_IDLE_DELAY));
        // The restarted wait is a fresh idle period, not a continuation.
        assert!(scheduler.should_step(next + COMPRESSION_IDLE_DELAY, 3));
    }

    #[test]
    fn compression_scheduler_stops_for_good_when_unsupported() {
        let mut scheduler = CompressionScheduler::default();
        let start = Instant::now();
        scheduler.mark_activity(start);
        let due = start + COMPRESSION_IDLE_DELAY;
        assert!(scheduler.should_step(due, 0));
        scheduler.record(due, CompressionStep::Unsupported);

        scheduler.mark_activity(due);
        assert_eq!(scheduler.next_wakeup(due), None);
        assert!(!scheduler.is_due(due + Duration::from_secs(3600)));
    }

    #[test]
    fn compression_scheduler_skips_and_disarms_when_the_preference_is_off() {
        let mut terminal = NativeTerminal::new(80, 24).unwrap();
        let mut scheduler = CompressionScheduler::default();
        let start = Instant::now();
        scheduler.mark_activity(start);
        let due = start + COMPRESSION_IDLE_DELAY;

        assert_eq!(scheduler.run_due_step(due, false, &mut terminal).unwrap(), None);
        assert_eq!(scheduler.next_wakeup(due), None);
    }

    #[test]
    fn compression_scheduler_runs_one_step_on_a_real_terminal_only_when_due() {
        let mut terminal = NativeTerminal::new(80, 24).unwrap();
        let mut scheduler = CompressionScheduler::default();
        let start = Instant::now();
        scheduler.mark_activity(start);

        let early = start + Duration::from_millis(100);
        assert_eq!(scheduler.run_due_step(early, true, &mut terminal).unwrap(), None);

        let due = start + COMPRESSION_IDLE_DELAY;
        let step = scheduler
            .run_due_step(due, true, &mut terminal)
            .unwrap()
            .expect("a due step must run");
        let expected_wakeup = match step {
            CompressionStep::Pending => Some(COMPRESSION_STEP_INTERVAL),
            CompressionStep::Complete | CompressionStep::Unsupported => None,
        };
        assert_eq!(scheduler.next_wakeup(due), expected_wakeup);
    }
}
