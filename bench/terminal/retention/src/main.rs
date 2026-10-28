//! Standalone harness over the production retention modules; no GUI, PTY, or Ghostty required.
#![allow(dead_code)]
#[path = "../../../../src-tauri/src/terminal/vt_state.rs"]
mod vt_state;
#[path = "../../../../src-tauri/src/terminal/vt_stream.rs"]
mod vt_stream;
#[path = "../../../../src-tauri/src/terminal/retained_history.rs"]
mod retained_history;

use retained_history::{RetainedHistory, RetentionBudget};
use std::{hint::black_box, time::Instant};

fn main() {
    // Small Kitty placement commands are image units too. Once the default image budget
    // fills, every new command must evict one old group without scanning all survivors.
    let command = b"\x1b_Ga=p,i=1;\x1b\\";
    let budget = RetentionBudget::default();
    let retained_groups = budget.image_bytes / command.len();
    let iterations = 4096;
    println!("image_budget_bytes={} command_bytes={} retained_groups={} measured_pushes={iterations}",
        budget.image_bytes, command.len(), retained_groups);
    for sample in 1..=3 {
        let mut history = RetainedHistory::new(budget);
        for sequence in 1..=retained_groups as u64 {
            history.push(sequence, command);
        }
        let start = Instant::now();
        for offset in 1..=iterations {
            history.push(retained_groups as u64 + offset, black_box(command));
        }
        let elapsed = start.elapsed();
        assert_eq!(history.image_bytes(), retained_groups * command.len());
        assert_eq!(history.start_sequence(), Some(iterations + 1));
        println!("sample={sample} steady_state_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
        black_box(history);
    }
}
