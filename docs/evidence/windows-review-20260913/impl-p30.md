# P30 implementation staging - RGB expansion regression

Task `st_01a09a0e`, root `01a0983f-c995-753d-afa9-593f6d118788`, 2026-09-13.

## Delivered and verification state

Implemented `kitty_rgb_expansion_limit_does_not_block_text_frames` in `src-tauri/tests/native_terminal_images.rs` and the exclusive-slot runner `scripts/fixtures/run-p30-cargo.sh`. Product source is deliberately unchanged: this task permits repair only after an actual production-seam RED, and no exclusive Cargo slot was issued during this child session. **Staged, not runtime verified; P30 remains open.** No Windows or overall objective completion is claimed.

Official `executeAgentToolkit` was imported from `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`, bound to this repo and the root session. `revise_criterion` on `G001-review-and-resolve-every-currently-o/C002` returned `ok: true`, `accepted: true` before editing. Full existing scenario and expected evidence were retained and the exact test and native binary condition appended. Receipt: `p30/registration.json`; official ledger contains `criteria_revised`.

Read root/backend/native-terminal directory instructions, Orca CLI skill discovery instructions, P30 addendum, repair-packet context, remaining register and `closure-images.md`. No Orca managed-state command was needed or executed. Scoped source/test were clean before edit, with many unrelated foreign dirty files preserved.

## Real seam and assertions

The test uses the existing actual NativeTerminal parser and WGPU renderer, not a forged snapshot or mock:

1. Feed/render supported red RGB image 71 at row 2, column 2, covering 2x2 cells; assert initial red pixel `(12,12)`.
2. Clear buffered protocol responses; move to row 1, column 1. Transmit blue RGB image 76 with `a=T,f=24,s=4097,v=4097,i=76,c=1,r=1,C=1,m=1`. Feed deterministic 3072-raw-byte chunks (4096 encoded bytes), continuation `m=1`, last `m=0`. The last chunk contains 3 raw bytes. No sleep, polling, external image fixture, compression, dependency mock or ambient process.
3. Require the exact response bytes `ESC_Gi=76;OK ESC\\` (without the explanatory space). This proves protocol acceptance rather than accidental rejection; total stored bytes are 50,356,227.
4. Feed row 5, column 1 explicit RGB green foreground/background and first a space, then `X` on the next frame. Require the actual text cell each time, one retained image with id 71, red `(12,12)=[255,0,0,255]`, green `(4,36)=[0,255,0,255]`, and omitted oversized placement `(4,4)=[0,0,0,255]` through real GPU readback.

Two captures exercise both initial and unchanged-image cache behavior. Dropping all images or rejecting the protocol cannot pass.

Current inspected source chain: pinned `graphics_image.zig::LoadingImage.complete` accepts exact nonzero width*height*3 bytes within its dimension limit; `ImageCache::capture` in `images.rs` treats `pixels > IMAGE_LIMIT/4` as InvalidValue; `NativeTerminal::render_snapshot` propagates that error after text capture. 4097 squared expands to 67,141,636 RGBA bytes, greater than the unchanged 67,108,864 cap. This is source evidence, **not executed RED**.

## Local evidence

- `p30/staging-checks.log`: `bash -n` exit 0; absent-slot invocation exits 2 before Cargo; scoped `git diff --check` exit 0.
- `p30/diagnostics-cleanup.log`: language-server diagnostics on the changed Rust test returned `No diagnostics found`; resource/ownership receipt.
- No Cargo build/test or affected runnable execution occurred. No RED/GREEN logs exist yet; compiler failures and zero tests must never be relabeled RED.
- No product patch, vendor changes, renderer changes, or foreign dirty edits were made. No daemon/window/PTY/background process was started and no runtime cleanup is outstanding. Owned staged artifacts remain uncommitted in the shared tree and are subject to concurrent changes.

## Exact lead relay needed

The child has no task-send/monitor tool. Grant and relay one exclusive Darwin shared-target slot, then invoke:

```sh
P30_CARGO_SLOT=lead-authorized bash scripts/fixtures/run-p30-cargo.sh red
```

Runner executes only:

```sh
cargo test --offline --manifest-path src-tauri/Cargo.toml --features native-terminal --test native_terminal_images kitty_rgb_expansion_limit_does_not_block_text_frames -- --exact --nocapture --test-threads=1
```

It records timestamp, HEAD and scoped SHA256 values; refuses to overwrite phase logs; requires exactly one passing test on success. Intended RED is `RGB expansion must not abort text snapshot` after exact OK response acceptance, not GPU initialization, protocol mismatch, unrelated compilation, or zero tests. Compiler/feature/GPU failures are a prerequisite requiring owner resolution.

After that actual RED, resume this packet with allocation of `src-tauri/src/native_terminal/images.rs`: preserve malformed foreign pointer/length errors; separate the supported-input expansion budget from ABI validation, remove any cached entry for the oversized image and continue without RGBA allocation. Keep the 64 MiB cap, supported entries, all existing parser/renderer behavior and vendor code. Re-read source and diff before the minimal edit. Then run the identical assertions via `P30_CARGO_SLOT=lead-authorized bash scripts/fixtures/run-p30-cargo.sh green`, changed-file diagnostics, related exact image tests and lead-scheduled build. There is no authorization here to invent a RED or apply the repair prematurely.

## Native handoff: runtime owner st_01a099f8 only

No remote mutation by this child. On the current Windows debug artifact, reproduce the exact stream above in a newly owned native terminal with 8x8 cell metrics for literal pixel coordinates, or map those same cell-relative points using recorded actual metrics. Subscribe to the owner's existing presentation-completion/readback signal before sending the stream; bounded failure deadline, not sleeps or polling. Native text row 5 and red image row 2 must remain visibly presented after both successive text updates; the one-cell blue oversized placement must not paint. Capture debug executable/library/source/target hashes and native image evidence. Offscreen test success alone does not establish HWND presentation.

Run the same exact Cargo test on the native toolchain (one case, native-terminal enabled), keeping source assertions unchanged. Close/reap only the newly owned pane/session and record completion; do not replace or restart shared daemons or install/release binaries. Missing native artifact, toolchain, adapter or presentation signal is a narrowly owned prerequisite, not Windows acceptance. Existing `closure-images.md` remains the source receipt; no whole-domain repeat audit is needed.
