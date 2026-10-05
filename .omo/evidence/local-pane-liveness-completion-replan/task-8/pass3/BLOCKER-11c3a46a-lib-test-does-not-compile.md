# BLOCKER — `11c3a46a` does not compile the lib test target (E0308)

Discovered while verifying the frozen repair commits by execution (not by reading them).

## What is broken

```
error[E0308]: mismatched types
   --> src/ipc/tests.rs:309:22
    |
309 |           incarnation: spawned
    |  ______________________^
310 | |             .session
311 | |             .incarnation
312 | |             .clone()
313 | |             .expect("spawn response reports the session's PTY incarnation"),
    | |___________________________________________________________________________^ expected `Option<String>`, found `String`
    |
    = note: expected enum `std::option::Option<std::string::String>`
             found struct `std::string::String`
help: try wrapping the expression in `Some`
    |
309 ~         incarnation: Some(spawned
310 |             .session
311 |             .incarnation
312 |             .clone()
313 ~             .expect("spawn response reports the session's PTY incarnation")),
    |
error: could not compile `ferryx` (lib test) due to 1 previous error; 47 warnings emitted
```

`PaneAttachTuple.incarnation` is `Option<String>`
(`src-tauri/src/daemon/protocol.rs:192-193`), but the fixture passes the unwrapped `String`.

## Which commit introduced it

`11c3a46a test(ipc): supply the persisted seven-field binding to the two attach fixtures` —
`git show 11c3a46a -- src-tauri/src/ipc/tests.rs` contains the `incarnation: spawned…expect(…)`
hunk verbatim. **Not** `e57685ec` (the surface_host repair), which touches only
`src-tauri/src/native_terminal/surface_host.rs`.

## Blast radius — total

The error is in the **lib test target** (`#[cfg(test)] mod tests` inside `src/ipc/tests.rs`), so every
command that compiles lib tests fails, on **all three hosts**:

| Command | Result |
| --- | --- |
| `cargo test --lib <any filter>` | **101**, `could not compile ferryx (lib test)` |
| `cargo check --all-targets` | **101** (it compiles the lib test target) |

Observed on **linux** at `e57685ec` (all four surface_host test runs exited 101 with
`selected=0` because nothing could be built): `logs/sh-*.log`. The mac run was still on its first
gate when this was found.

**Consequence for the record:** the "four presentation tests now pass" claim is **UNVERIFIED**, and it
cannot be verified until this compiles. The earlier per-target sweep that reported 81/81 was taken at
`ad0ffb5a`, i.e. **before** `11c3a46a`, so it does not cover this.

## Required fix (one line, routed to the author)

`src-tauri/src/ipc/tests.rs:309` must wrap the value:

```rust
incarnation: Some(
    spawned
        .session
        .incarnation
        .clone()
        .expect("spawn response reports the session's PTY incarnation"),
),
```

or pass `spawned.session.incarnation.clone()` directly (it is already an `Option<String>`), which
matches the fence's own semantics: the fence rejects `None`, so an unwrapped value would be wrong even
if it compiled.

The verifier did **not** edit this — the file belongs to the attach-fixture worker.

## Status

**NOT VERIFIED / BLOCKED.** No gate at `e57685ec` can run until this is fixed.
