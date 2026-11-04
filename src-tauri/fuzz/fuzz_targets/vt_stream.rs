//! Pushes arbitrary bytes through the retained-history unit splitter, chunked at fuzzer-chosen
//! points, and checks the retention budgets hold after every chunk.
#![no_main]

use ferryx_lib::terminal::output_hub::retained_history::{RetainedHistory, RetentionBudget};
use ferryx_lib::terminal::output_hub::vt_stream::VtUnitSplitter;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&header, rest)) = data.split_first() else {
        return;
    };
    let max_unit = 64 + usize::from(header) * 16;
    let budget = RetentionBudget::from_text_capacity(max_unit);
    let mut splitter = VtUnitSplitter::new(max_unit, max_unit / 2);
    let mut history = RetainedHistory::new(budget);
    let mut units = Vec::new();
    for (sequence, chunk) in (1u64..).zip(rest.split(|byte| *byte == 0xff)) {
        splitter.push(sequence, chunk, &mut units);
        history.push(sequence, chunk);
        assert!(history.text_bytes() <= budget.text_bytes);
        assert!(history.image_bytes() <= budget.image_bytes);
    }
    let _ = splitter.pending();
    assert!(units.iter().all(|unit| !unit.bytes.is_empty()));
    let _ = history.replay();
});
