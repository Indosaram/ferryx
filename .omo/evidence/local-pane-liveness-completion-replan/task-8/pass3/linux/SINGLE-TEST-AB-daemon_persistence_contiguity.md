# Scoped A/B — `test_daemon_output_sequence_contiguity_and_replay_gap`

Requested follow-up: the linux `daemon_persistence_contract` failure (`assertion left:0 / right:1`,
exit 101 at `abd9e890`) had **not** been A/B'd, and the candidate changed the stream-pump generation
fencing in `src-tauri/src/ipc/terminal.rs`, so it might have been candidate-caused.

## Setup

Host: **linux** `indo@100.91.254.71`. One command, both sides, same shell script:

```
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract \
  -- --nocapture --test-threads=1 test_daemon_output_sequence_contiguity_and_replay_gap
```

The base tree had been torn down after classification, so it was rebuilt: `cp -al` hardlink clone of
`source-21dea3c0` → `source-base`, `target/` removed, the 76 candidate-**modified** files deleted, the
20 candidate-**added** files deleted, then the base overlay extracted. Verified before running:

| File | base `source-base` | candidate `source-21dea3c0` |
| --- | --- | --- |
| `src-tauri/src/daemon/protocol.rs` | `831778372961a50cb62fc50eeeaeab99b6b77c67b6386235c7a9a2822ded9bdb` | `360688bd2e5359e91f086db86ad1ab80972c6dfd094640e63d1fef8ecd117de5` |
| `src-tauri/src/ipc/terminal.rs` (the file the requester flagged) | `5d5fd952ecf0ce4938708492403e7335c16130ecaef619cf3d0f989cdc6de766` | `7f6d68fbf58d091c81c21e599b74dde4c5dd6c3694cefe38461eec5f330dff58` |
| `src-tauri/src/daemon/split_journal.rs` | **absent** (candidate-added) | present |
| `src-tauri/src/ipc/qa_barrier.rs` | **absent** (candidate-added) | present |

So the two sides genuinely differ in `ipc/terminal.rs`, and the test exists at base
(`d82b35e4:src-tauri/tests/daemon_persistence_contract.rs`).

## Result — both sides FAIL identically

| Side | Raw native exit | Asserted line | Assertion |
| --- | --- | --- | --- |
| **base `d82b35e4`** | **101** | `tests/daemon_persistence_contract.rs:1093:9` | `assertion \`left == right\` failed` — `left: 0` / `right: 1` |
| **candidate `abd9e890`** | **101** | `tests/daemon_persistence_contract.rs:1098:9` | `assertion \`left == right\` failed` — `left: 0` / `right: 1` |

Verbatim, base:

```
thread 'test_daemon_output_sequence_contiguity_and_replay_gap' (1956270) panicked at tests/daemon_persistence_contract.rs:1093:9:
assertion \`left == right\` failed
  left: 0
 right: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 14 filtered out; finished in 1.85s
```

Verbatim, candidate:

```
thread 'test_daemon_output_sequence_contiguity_and_replay_gap' (1956488) panicked at tests/daemon_persistence_contract.rs:1098:9:
assertion \`left == right\` failed
  left: 0
 right: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 14 filtered out; finished in 1.41s
```

The 5-line offset (1093 → 1098) is exactly the fixture lines the candidate inserted into that file
(`local_split: None` plus its comment, and `capabilities`/`admission_time_unix_ms`) — the *same*
assertion, moved.

## Verdict: **PRE-EXISTING — NOT candidate-caused**

Both sides fail the identical assertion with identical values, so the candidate did not introduce it.
**Do not route a repair worker for this on the strength of a candidate regression** — it is a
pre-existing failure of the `daemon_persistence_contract` suite on linux, and the base evidence above
is what a `pre-existing` claim requires.

Evidence: `linux/ab-single-test.log`, `linux/logs/ab-single-base.log`,
`linux/logs/ab-single-cand.log`, `linux/ulw.single-ab.sh`.
