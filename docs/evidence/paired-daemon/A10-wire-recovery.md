# A10 standalone wire codec recovery - GREEN

Recovery completed on macOS arm64. The historical ENOSPC report and original
pre-production controlled RED in `A10-wire-codec.md` remain unchanged. This
document supersedes its pending-GREEN status, not its historical observations.

## Scope and source provenance

Working directory for every command:
`/Users/indo/code/project/orca-lite-wt/herdr-wave2`.
Read the complete approved 833-line plan, applicable root and src-tauri AGENTS,
wave1 handoff boundaries, existing server encoders, browser pass-through, A02
fixtures, preserved codec/tests, and frozen parent seed receipt.

Initial `df -k .` exit 0 reported 44,120,908 KiB available. Immediately before
compilation the bounded command runner observed 44,592,922,624 bytes free,
above the required 10 GiB. No caches were cleaned and no dependency links were
written. Existing pinned serde/serde_json artifacts were reused; no dependency
or Cargo changes were needed.

All 34 files in `WAVE2-parent-seed.json` matched their frozen SHA-256 values at
validation (`seed_delta={}`). This is the inherited composed baseline, not
git HEAD. Recovery made **no production or test source edits**: the pending
GREEN passed on the preserved implementation without repairs. Source additions
relative to seed remain:

- `src-tauri/src/remote/terminal_wire.rs`, SHA-256
  `52a91ccdf54a3453bdbaee14b4cc93d8b1cb926b696bb19ebd93d44614a9f9b7`
- `src-tauri/tests/a10_terminal_wire_codec.rs`, SHA-256
  `849472b3276429ec1c828682b2cfa35b2d0308286e85fe838817bf45536ed894`

This recovery adds only this evidence document and the new GREEN/RED logs.
No mod.rs, server.rs, protocol, manifest, lockfile, capability, other worktree,
or historical evidence edits. Work remains uncommitted.

## Executed validation

`rustc --version`: exit 0, rustc 1.92.0 (ded5c06cf 2025-12-08).
LSP diagnostics on both codec and test harness: no diagnostics found.
Commands below used `CARGO_BUILD_JOBS=4`, `RUSTC_WRAPPER=` and the private
`src-tauri/target`; GREEN also explicitly set absolute `CARGO_TARGET_DIR` to
that same directory. Each compiler/test subprocess had a 120-second timeout.

Pending GREEN, executed once after capacity restoration:

```sh
CARGO_BUILD_JOBS=4 RUSTC_WRAPPER= rustc --edition=2021 --test src-tauri/tests/a10_terminal_wire_codec.rs -L dependency=src-tauri/target/debug/deps --extern serde=src-tauri/target/debug/deps/libserde-20103001e2270c27.rlib --extern serde_json=src-tauri/target/debug/deps/libserde_json-bbb1dc538a584af2.rlib -o src-tauri/target/a10-wire-recovery-green
src-tauri/target/a10-wire-recovery-green --nocapture
```

Compiler exit **0**; test exit **0**, **5 passed, 0 failed, 0 ignored**.
Exact output and source hashes: `A10-wire-recovery-GREEN.log`.
Executed coverage: seven exact canonical output/replay/gap/reset vectors,
empty history, A02 values above 2^53 and u64 max, all byte values, embedded
metadata-looking payload, every two-fragment boundary and bytewise assembly,
UTF-8 split across messages, malformed/duplicate JSON, noncanonical and
overflowing integers, truncated headers/reset, replay consistency, and exact
frame/metadata allocation boundaries. Tests use no sleeps or polling.

Controlled RED replay after GREEN (not a new pre-production RED claim):

```sh
CARGO_BUILD_JOBS=4 RUSTC_WRAPPER= rustc --edition=2021 --test --cfg a10_wire_red src-tauri/tests/a10_terminal_wire_codec.rs -o src-tauri/target/a10-wire-recovery-red
src-tauri/target/a10-wire-recovery-red --nocapture
```

Compiler exit **0**; test exit **101**, **0 passed, 1 failed** at
`metadata_is_not_native_terminal_output`: actual bytes started with OSC
metadata; expected bytes were `[104,101,108,108,111,0,255]`. Exact output:
`A10-wire-recovery-RED.log`. The cfg replaces decoding with the inspected
browser adapter's raw forwarding behavior; it proves the native-sink assertion
detects metadata leakage. It does not call the browser runtime or assert that
the legacy browser renderer is broken. Original pre-production RED remains
documented in the preserved historical report. No build failure counts as RED.

Survey/check commands exited 0:

```sh
git --no-pager -c diff.ignoreSubmodules=all status --short --ignore-submodules=all
git --no-pager -c diff.ignoreSubmodules=all diff --stat
git --no-pager -c diff.ignoreSubmodules=all diff --check
```

The tracked diff check does not validate untracked source; the explicit rustc
compilation and test run validate those two source additions.

## Minimal integrator API and boundaries

Register `terminal_wire` only when the A10 integration owner composes it.
Map OutputChunk sequence/gap to `Metadata::Output`; map AttachmentSnapshot
history start/end/gap to `Metadata::Replay`. All sequence values are u64.
`encode_frame(metadata, payload, force_reset)` returns a Result with bytes
compatible with the existing OSC prefix, ordered JSON string fields, BEL,
optional hard reset and arbitrary PTY bytes. It bounds a frame to 1 MiB.

`decode_frame(&[u8])` returns borrowed `DecodedFrame { metadata,
terminal_bytes }`; `decode_fragments` returns the owned equivalent from ordered
fragments of exactly one complete WebSocket binary message. Typed `ParseError`
distinguishes invalid prefix, truncated/oversized metadata, oversized frame,
invalid JSON, invalid sequence, inconsistent metadata and missing gap reset.
Metadata is bounded to 16 KiB. Existing serde dependencies are sufficient.

Publish only `terminal_bytes`, after applying cursor/replay policy. They retain
ESC c deliberately: the format cannot distinguish inserted resets from PTY
resets. Do not scan payload for more frames. There is no payload length, so
transport message-end is required and payload truncation is not detectable by
this codec. Text JSON belongs to lifecycle/control handling, never PTY output.
Empty replay encoding is supported but is not an attached control handshake.

## Runtime proof, teardown and limits

Actual runtime proof is the standalone in-process Rust test executable, not
source grep. Both GREEN and deliberately failing RED processes exited and
were reaped by the runner. No listeners, daemon, PTY, shell fixture, private
runtime root, or background task was created; there was nothing else to tear
down on the expected assertion failure. Original terminal PID/CWD/owner epoch
are not applicable to this pure codec fixture. No canonical daemon or PTY was
inspected, restarted, launched or signaled. Only private compiler artifacts
remain in the existing target directory.

No full Cargo build, A10 socket runtime acceptance, controller fencing,
owner-epoch survival, revocation, relay, native renderer, Linux/Windows runtime,
capability activation or full-plan/platform approval is claimed. Those remain
with the integration owner. The standalone codec is now compiled and usable.
