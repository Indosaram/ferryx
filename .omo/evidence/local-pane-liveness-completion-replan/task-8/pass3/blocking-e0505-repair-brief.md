# Blocking compile defect — route to the native lane owner (Task 8 pass 3)

Candidate: `21dea3c01d1ec423498bc48eb2d29107be75eddf` (tree `02488435`)
File: `src-tauri/src/native_terminal/surface_host.rs`
Verdict: the candidate does not compile on any platform. Exactly ONE error.

## Raw diagnostic (mac, `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list`)

```
error[E0505]: cannot move out of `completion_window` because it is borrowed
    --> src/native_terminal/surface_host.rs:989:90
     |
 952 |                 let completion_window = surface_window.clone();
     |                     ----------------- binding `completion_window` declared here
...
 989 |                     if let Err(err) = dispatch_render_on_main_thread(&completion_window, move || {
     |                                       ------------------------------ ------------------ ^^^^^^^ move out of `completion_window` occurs here
     |                                       |                              |
     |                                       |                              borrow of `completion_window` occurs here
     |                                       borrow later used by call
...
1050 |                                     if let Err(error) = completion_window.emit(
     |                                                         ----------------- move occurs due to use in closure
     |
help: consider cloning the value before moving it into the closure
```

## Exact source (candidate, unchanged)

- :952 `let completion_window = surface_window.clone();`
- :989 `if let Err(err) = dispatch_render_on_main_thread(&completion_window, move || {`
- :1050 `if let Err(error) = completion_window.emit(` (inside the `move` closure)

## Suggested fix (the compiler's own suggestion, not applied by the verifier)

```rust
// before the closure, inside the enclosing block:
let completion_window_for_closure = completion_window.clone();
if let Err(err) = dispatch_render_on_main_thread(&completion_window, move || {
    ...
    if let Err(error) = completion_window_for_closure.emit(
        crate::ipc::native_terminal::NATIVE_TERMINAL_PRESENTATION_RECEIPT_EVENT,
        presentation_receipt,
    ) { ... }
```

## Attribution

Introduced by commit `5464da0d` ("feat(terminal): compose pane-liveness completion candidate for
consolidated verification") — `git log -S "completion_window.emit"` returns only that commit. Base
`d82b35e4` has neither `completion_window.emit` nor `active_presentation_generation`, so this is
candidate-branch code, not a baseline defect.

It was invisible in pass 1 and pass 2 because the E0599 unresolved-method errors
(`active_presentation_generation` / `active_presentation_epoch`) stopped name resolution before
borrow checking ran. Commit `21dea3c0` added those accessors, so borrowck now runs and reports E0505.

## Blast radius (measured, not inferred)

| host | configuration | native exit | errors | warnings |
| --- | --- | --- | --- | --- |
| mac | `--lib local_split_reliability_ --list` | 101 | 1 | 51 |
| mac | `--lib --features local-split-qa qa_barrier --list` | 101 | 1 | 53 |
| mac | `--test daemon_handover_transfer_contract --list` | 101 | 1 | 32 |
| linux | `--lib local_split_reliability_ --list` | 101 | 1 | 47 |
| windows | `--lib local_split_reliability_ --list` | 101 | 1 | 43 |
| windows | `--lib --features local-split-qa qa_barrier --list` | 101 | 1 | 45 |
| windows | `--test daemon_handover_transfer_contract --list` | 101 | 1 | 17 |

The mac/windows handover runs fail while compiling `ferryx` **(lib)** — the production crate, not just
the test harness — so `cargo build` and any packaging step (Task 10) also fail on this commit until the
fix lands.

## Verifier boundary

No verifier repair was made (Task 8 brief: compile errors are findings to route to the owning lane, never
a reason to relax a gate; the dispatch also forbids verifier product repairs). Dependent Rust gates are
recorded `NOT_RUN_BLOCKED` with this log as the causal evidence; the failure was not repeated 34 times as
in pass 2. Re-run the whole Rust half as soon as the fix lands.
