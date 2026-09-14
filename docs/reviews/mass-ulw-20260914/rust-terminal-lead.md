# Review: the PTY lifecycle and ring buffer (Rust)

Scope: `src-tauri/src/terminal/output_hub.rs` (1054 lines), read for sequence arithmetic,
`ReplayGap` correctness, and trim-loop safety.
Reviewed-at: 2026-09-14
Reviewer: lead session (this lane's dag node did not deliver; findings below are the lead's own,
each verified by direct read)

## Findings

`NO-FINDINGS above P3`

The ring buffer was examined specifically for the defect classes that would corrupt a terminal
reattach — off-by-one gap detection, sequence wraparound, underflow, and an unbounded or
non-terminating trim. None are present. Each check below cites the line I read.

### Verified negative — eviction-gap boundary is correct

- Location: `src-tauri/src/terminal/output_hub.rs:248-253`
- Observed: the gap condition is `req_seq + 1 < first_seq`, producing
  `ReplayGap { requested_after_sequence: req_seq, available_from_sequence: first_seq }`.
- Why this is the right predicate: `req_seq + 1 == first_seq` is deliberately **not** a gap —
  the next chunk the client wants is exactly the oldest chunk still retained, so history is
  contiguous and no resync is needed. Reporting a gap there would force a spurious full
  terminal re-sync on every reattach that lands on the boundary. This is the off-by-one most
  implementations invert, and this one has it right.

### Verified negative — no sequence wraparound risk, no modular comparison

- Location: `output_hub.rs:106` (`next_sequence: 1`), `:111-115` (`allocate_sequence`),
  `:248`, `:276` (comparisons)
- Observed: sequences are `u64`, monotonically incremented from 1, and compared with plain
  `>` / `<`. At one chunk per nanosecond a `u64` takes roughly 584 years to wrap, so there is
  no wraparound case to handle and therefore no modular-arithmetic comparison that could be
  written incorrectly.

### Verified negative — no underflow in gap publication

- Location: `output_hub.rs:451-458` (`publish_gap`)
- Observed: `requested_after_sequence: sequence.saturating_sub(1)`. Allocating sequence 1
  yields 0 rather than underflowing `u64::MAX`.

### Verified negative — trim loop is bounded and cannot spin

- Location: `output_hub.rs:147-149`, capacity constant at `:7`
  (`DEFAULT_BUFFER_CAPACITY: usize = 512 * 1024`)
- Observed: `while self.current_size > self.capacity && !self.chunks.is_empty()` then
  `pop_front()`. The `!is_empty()` conjunct is load-bearing: without it, a single chunk larger
  than the whole capacity would loop forever with nothing left to pop.
- Existing coverage: `:675` (pops until under capacity), `:922` (explicit eviction boundary at
  capacity 24 — 10+10+10 > 24 evicts seq 1), `:1015` (64-byte capacity).

## Summary

- P0: 0
- P1: 0
- P2: 0
- P3: 0

`NO-FINDINGS above P3`
