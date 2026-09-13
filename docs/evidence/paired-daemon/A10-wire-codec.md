# A10 isolated terminal wire codec - BLOCKED before GREEN

## Scope and seed

Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-wave2`.
Read the full approved 833-line plan, root/src-tauri AGENTS, and wave1
`WAVE2-handoff-boundaries.md`. Baseline is the inherited parent composition,
not HEAD: initial survey showed 18 tracked modified files (1599 insertions,
127 deletions) plus inherited untracked sources/dependency links. None was
edited. This task adds only:

- `src-tauri/src/remote/terminal_wire.rs`
- `src-tauri/tests/a10_terminal_wire_codec.rs`
- this evidence file

No module registration, router, protocol, Cargo manifest/lock, dependencies,
historical wave1 evidence, capabilities, or canonical daemon changes.
No commits, deployment, desktop launch, network listeners, child PTYs, or
temporary runtime roots. Original PTY PID/CWD/owner epoch: not applicable;
this is an in-process codec harness, not a running terminal fixture. No
canonical processes were inspected or signaled. Harness processes exited;
private compiler artifacts remain in the allowed worktree target directory.

## Contract and integration API

Uses existing serde/serde_json dependencies, no new dependencies. Serializer
matches existing server field order, omitted options, OSC prefix
`ESC ]777;ferryx;`, JSON string u64s, BEL terminator, optional `ESC c`, and raw
PTY bytes. `Metadata::{Output,Replay}` uses native u64 and optional `ReplayGap`.
`encode_frame(metadata, payload, force_reset)` returns bounded framed bytes;
gaps always insert reset. Map OutputChunk.sequence/replay_gap and
AttachmentSnapshot.history_start_sequence/history_end_sequence/gap directly.
`decode_frame(&[u8])` returns borrowed metadata and `terminal_bytes`.
`decode_fragments(iterator)` assembles fragments of exactly one complete
binary WebSocket message under a 1 MiB bound and returns owned bytes.
Metadata bound is 16 KiB. Explicit `ParseError` variants distinguish prefix,
truncation, JSON, sequence, metadata consistency, reset, and size failures.

Only OSC metadata is removed. Reset remains in terminal_bytes deliberately:
the wire cannot distinguish an inserted reset from a PTY-emitted reset.
Apply replay/cursor decisions to metadata before publishing terminal_bytes;
do not forward the original message into the native output hub. Cursor
deduplication, attached/control JSON, owner epoch, controller fencing, and
socket transport remain integrator responsibilities. Text messages never
enter the codec. Empty replay metadata is supported but this does not itself
send the required attached JSON boundary.

There is no payload length. The caller must retain WebSocket message-end
information: concatenated messages cannot be split by prefix scanning, and
payload truncation cannot be distinguished from a shorter valid payload.
Embedded prefixes, BEL, UTF-8 splits and non-text bytes are ordinary PTY data.

## Executed commands and outcomes

All commands below ran from the worktree above on macOS arm64,
`rustc 1.92.0 (ded5c06cf 2025-12-08)` (version command exit 0).

Initial `git --no-pager status --short` failed exit 128 because the provisioned
ghostty submodule is a symbolic link. Corrected survey commands exited 0:

```sh
git --no-pager -c diff.ignoreSubmodules=all status --short --ignore-submodules=all
git --no-pager diff --ignore-submodules=all --stat
git --no-pager diff --ignore-submodules=all -- src-tauri/src/remote/mod.rs
```

Actual RED, executed before production file creation:

```sh
CARGO_BUILD_JOBS=4 RUSTC_WRAPPER= rustc --edition=2021 --test --cfg a10_wire_red src-tauri/tests/a10_terminal_wire_codec.rs -o src-tauri/target/a10-wire-red
src-tauri/target/a10-wire-red --nocapture
```

Compiler exit 0; test exit 101, 0 passed/1 failed:
`metadata_is_not_native_terminal_output` asserted byte equality. Expected
`hello NUL FF`; actual began `[27,93,55,55,55,59,102,101,114,114,121,120,59,...]`
(OSC metadata), then the same payload. The RED configuration faithfully
reproduces the inspected browser adapter's pass-through byte behavior. It
is a controlled baseline/mutation proof of metadata leakage if reused at
the native sink, not a claim that a native paired proxy already exists or
that the old browser renderer is broken.

GREEN attempt (not passing evidence):

```sh
CARGO_BUILD_JOBS=4 RUSTC_WRAPPER= rustc --edition=2021 --test src-tauri/tests/a10_terminal_wire_codec.rs -L dependency=src-tauri/target/debug/deps --extern serde=src-tauri/target/debug/deps/libserde-20103001e2270c27.rlib --extern serde_json=src-tauri/target/debug/deps/libserde_json-bbb1dc538a584af2.rlib -o src-tauri/target/a10-wire-green
```

Exit 1 at linker: `ld: write() failed, errno=28 (No space left on device)`;
`clang: error: linker command failed with exit code 1`.
The chained `src-tauri/target/a10-wire-green --nocapture` did NOT execute.
No test failure or build failure is mislabeled GREEN or behavioral RED.
Validation stopped; no blind linker retry and no foreign artifact deletion.

Earlier large test patch also failed before application with shell
`cannot create temp file for here document: No space left on device` (exit 1).
Read-back confirmed the test file unchanged; a smaller patch then applied
successfully. `df -h .` showed 118 MiB available/100% capacity at that check.

LSP diagnostics: module none; harness only inactive-code hint for intentional
RED cfg. `git --no-pager diff --ignore-submodules=all --check` exited 0 at the
post-module checkpoint (does not validate new untracked file contents).

## Written tests awaiting execution

Five deterministic tests (no sleeps, polling, async timing, or process setup):
metadata leakage regression; seven exact server-compatible output/replay/gap
vectors with empty/nonempty payloads and forced resets; A02 shared JSON u64
values including 9007199254740993 and u64 max, every two-fragment split and
bytewise fragmentation with all 256 byte values/embedded prefixes/UTF-8 split
across independently sequenced frames; malformed JSON/duplicate fields/u64s,
all incomplete header prefixes, inconsistent replay and missing gap reset;
exact 1 MiB allocation and 16 KiB metadata limits.

These assertions are written, NOT executed successfully. No Cargo full build,
socket runtime acceptance, PTY ownership survival, Linux/Windows runtime,
native renderer, forced-relay, or full-plan acceptance is claimed. The module
must not be treated as verified/operational or used to advertise capabilities
until disk capacity is resolved and the exact GREEN command plus test binary
execution succeeds. Changes remain uncommitted in the shared worktree.
