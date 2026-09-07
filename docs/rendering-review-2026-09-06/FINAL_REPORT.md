# Ferryx rendering review — final report (2026-09-06)

Loop session: `.omo/ulw-loop/rendering-review-20260906` (ledger: `ledger.jsonl`).
Gate review: one deep reviewer (`st_01a0791f`) — overall **PASS**, no blocking
findings (`.omo/evidence/ulw/rendering-review-20260906/gate-review.md`).

## Outcome

Seven confirmed defects (D1–D7) from the source review each have a captured
right-reason RED, a minimal verified fix, and a same-case GREEN, committed as
five increments on isolated branches. All executable criteria (tests, builds,
contract targets, browser-harness scenarios, frozen-source native Linux
validation) pass. The only remaining evidence class is **native GUI runtime
proof on desktop surfaces** (macOS debug app 1x/2x, Wayland compositor,
Windows visible desktop), which this session cannot capture: the macOS GUI
runtime is unavailable (`orca` reports runtime_unavailable; osascript error
-1728) and Windows visible-desktop sessions are not reachable (Session 0 has
no visible desktop). These are recorded honestly as blocked/aggregate-pending,
never claimed as passing.

## Defects, fixes, verification

| Defect | Fix (commit, branch) | Verification (this session) |
| --- | --- | --- |
| D1/D4/D7 atlas capacity accounting, dense working sets, history-pressure final frames | `17fb10608ad31e0e60ae565e8dc686afdc4735f9` on `fix/rendering-atlas-20260906` | RED 992/1000 glyphs → GREEN 1000/1000 both repeated frames; 4000-glyph multi-size growth 1024→4096; typed capacity error, no partial success; history-pressure RED 822/4096 bytes differ → GREEN 0; renderer_contract **26/26** incl. `test_dense_scale_2_working_set_pixel_readback_integrity` exact full tiles F0=1000 F1=1000 on Apple M4 Max Metal; independent 14,000-tile raw audit (12,000 green, 2,000 differ only at indices 992–999 pre-fix); reports `atlas-repair.md`, `atlas-verification.md`, `MAIN_ATLAS_VALIDATION.md` |
| D2 obsolete pane input recovery after tab replacement | `bce59b45d6d2d43f46cc63e35a5fb9b1161ca145` on `fix/rendering-review-20260906` | RED/GREEN replacement/hidden/leave-during-recovery + queued-op cancellation and stale publication guards; 171 UI tests; browser harness screenshots (`D2/browser-*.png`, image-reviewed `BROWSER_IMAGE_REVIEW.md`) |
| D3 Wayland fractional buffer geometry + effective IME scale receipt | `6abaa5abbfa860b8e00b16be54150c6d734d1928` on `fix/rendering-review-20260906` | RED 801%2 and 600×450-vs-800×600 → GREEN via real host composition path; shared adjacent edges x=411/y=321 at DPR 1/1.5/2/2.5; wire RED `Null != 2.0` → GREEN `effectiveScaleFactor: 2.0` end-to-end; UI IME anchor RED 21.333px → GREEN 16,48 size 8×16 with legacy/null fallbacks; macOS/Windows/X11 default geometry preserved; **native Linux (omaki, Arch x86_64)** frozen identical sources: cargo check 0 + 26/26 (5 child-surface, 7 composition, 12 Wayland, wire 1/1, host 1/1) — `linux-validation.md`, lead-audited `linux-verification.md` |
| D5 dropped direct frames never re-present | `829fbe8e01e06418f329fa7022141b2fbab3e6e9` on `fix/rendering-retry-20260906` | One-shot render requeue RED (Acquire→Dropped) → GREEN (Acquire→Presented) at the real completion path; Lost/Timeout/detach/fatal/coalescing edge tests; surface-host 28/28 + cargo check on D3 tree |
| D6 Windows children destroyed off owner thread | `c349e3a6ffa1ff36cef30ac4d66dd78250e3f428` on `fix/rendering-review-20260906` | Native Win32 RED child-alive → GREEN one owner-thread `WM_NCDESTROY`; same-owner and parent-already-destroyed cases; zero leaked windows; 60 artifact hashes |

## Aggregate checks

- `bun run --cwd ui test` (171 tests incl. the three named files) and
  `bun run --cwd ui build`: exit 0.
- `cargo check --manifest-path src-tauri/Cargo.toml`: exit 0 (D3 tree and atlas
  tree).
- Changed-file LSP diagnostics: 0 errors on all nine D3 files.
- Frozen-source native Linux validation: inputs hash-pinned
  (`base.bundle` `ee0e602c…`, `d3-sources.tgz` `275c570e…`, 9 source SHA-256s),
  foreign daemon PID 41475 and Hyprland preserved, owned staging removed after
  54-file hash verification (`linux/cleanup.log`).

## Blocked on GUI access (the one open item)

1. macOS debug app (`bun tauri dev`): dense Korean/ASCII render + forced
   repaint at 1x/2x density, split/resize/move/tab-switch/overlay scenarios
   with per-action screenshots and teardown receipts (G001 C002/C003,
   G006 C003 — G006 checkpointed `blocked`).
2. Native Wayland compositor pixel/IME-popup proof on omaki (G007 C003 note).
3. Windows visible-desktop tab/overlay correlation (G008 C003 note).
Code-level evidence for every one of these is already green; only the
screenshot-grade desktop proof is missing.

## Integration state (needs your approval)

Branches are **not merged**. Main has advanced (`37272f5` drag-and-drop link/
path insertion, SSH projects) and touches `NativeTerminalPane.tsx` and
`src-tauri/src/ipc/native_terminal.rs`, so merging D3 (`6abaa5a`) requires a
rebase; the gate review flags the D5 retry spawn at `surface_host.rs:1837` as
the known overlap point. Suggested order: `fix/rendering-atlas-20260906` →
`fix/rendering-retry-20260906` (already stacked) → rebase and merge
`fix/rendering-review-20260906`.

## Artifacts index

- Loop: `.omo/ulw-loop/rendering-review-20260906/{goals.json,ledger.jsonl}`
- Evidence: `.omo/evidence/ulw/rendering-review-20260906/{D2,D3,D5,D6,atlas,linux,gate-review.md}`
- Reports: `docs/rendering-review-2026-09-06/` (PLAN, review.md, baseline.md,
  per-defect repair/verification reports, REPAIR_PHASE, WINDOW_PHASE,
  WAYLAND_RECEIPT_DELTA, RECEIPT_RED_REDO, MAIN_ATLAS_VALIDATION,
  DROPPED_FRAME_PHASE, LINUX_VALIDATION, linux-validation.md,
  linux-verification.md, BROWSER_IMAGE_REVIEW.md, QA checklist, this report)
- Checkpoint gates: `.omo/evidence/ulw/rendering-review-20260906/gates/*.json`
