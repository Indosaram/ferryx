# Lead verification of actual helper bridge survival

This verifies the local standalone helper and actual bridge process lifetime.
It does not yet prove real OpenSSH transport recovery, desktop-daemon restart,
or automatic UI restoration.

## Baseline failure

`bun scripts/qa/ssh-helper-survival.mjs` failed against the packaged baseline:

```text
AssertionError: project.register: FORBIDDEN: project outside configured runtime root
```

The harness registers a project outside the private helper state directory.
Two owned children were reaped and the fixture directory was removed.
Raw evidence: `helper-channel.log`.

## Lead-run current core verification

Commands:

```sh
cargo test --manifest-path remote-helper/Cargo.toml
cargo build --manifest-path remote-helper/Cargo.toml
bun scripts/qa/ssh-helper-survival.mjs
```

Monitor `mon_ND3GTYHZCBMF5F86` exited 0. Cargo tests: 18 passed, 0 failed.
The harness started the helper, spawned one PTY, SIGKILLed the first bridge,
connected a second bridge, and used the original scoped target without respawn.

Observed values:

- Remote target session: `1c2afb9a-f358-4f3c-8760-de9c793bda2d`
- Helper epoch: `16697114359428484962`
- Helper owner: `560ef275-e985-46e3-994d-bf85608ace46`
- PID before and after: `59032`
- Nonce before and after: `aff5fc0d-0a3d-4044-973c-a57354a99db6`
- Mutable shell counter: `1` before, `2` after
- Total `pty.spawn` requests: `1`

Cleanup: explicitly stopped the QA PTY, reaped bridges `59031`, `59033`
and helper `59022`, removed temporary directory ending `ferryx-helper-qa-GA1BXF`.

## Open blocker found in source review

`helper.rs` checked `clientRequestId` under the spawn registry mutex, released
the mutex before creating the PTY, and reacquired it only for insertion.
Concurrent identical requests can both miss and create two remote processes.
Sequential dedupe tests do not prove this requirement. The same producer has
been instructed to capture deterministic concurrent RED/GREEN evidence.

The helper node is not accepted until that criterion-3 blocker is fixed.
The harness was also tightened to require the string `chunk.cursor` rather than
convert a numeric sequence; its next run verifies that delta.
