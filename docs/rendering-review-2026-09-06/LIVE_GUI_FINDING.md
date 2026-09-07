# Live GUI finding — blank panes reproduce WITHOUT the rendering increments (2026-09-07)

## What happened

During the loop's native real-surface QA (`bun tauri dev`, debug, integration
worktree), the native terminal panes rendered **nothing** — no prompt, no echo,
no output — while the daemon reported the sessions alive and fed.

## Defect characterization (measured)

- Daemon UDS (`/tmp/rorca-501-dev/daemon.sock`, handshake v3): sessions
  `running:true`, `endSequence` advancing (129–176 measured) after dense
  Korean/ASCII commands were written — the shells executed the content.
- Frontend trace (`/tmp/ferryx-switch-debug.jsonl`): panes attach complete and
  present exactly once at restore (`cursorCol 0, cursorRow 0`), then **never
  re-present**, despite output arriving. Input direction works: the trace
  records `terminal.surface.input.sent` carrying the full 139-codepoint
  Korean/ASCII command.
- Mapping is correct: trace pairs `localSessionId ↔ backendSessionId` for the
  visible panes; frontend JS is clean in the reproducing run (no unhandled
  rejections; the two rejections logged earlier belong to a prior run).

Conclusion: **PTY output reaches session state but the GUI never re-renders** —
an output→render/presentation pipeline break, not a session-mapping or input
defect.

## Attribution — the rendering increments are exonerated

Bisect: the same scenario (restore → write dense content → observe) executed on
a scratch worktree at **`20567f5`** — main's tip *without* any of the five
rendering increments (it contains only main's own newer commits plus the
in-flight alternate-screen chain). The identical failure reproduces: sessions
live, output accumulated, panes blank, no re-present.

Therefore the regression belongs to main's own newer state — in suspicion
order: the **wgpu v30 upgrade** (`29ea50b`, rewrote surface acquisition and
cfg-split the frame targets), the wheel integration (`8c8c1a4`), the dependency
migration (`9f92cc1`/`1b36167`), or the NSView ordering fix (`1e97568`). It is
**not** caused by D1–D7 (`17fb106`, `829fbe8`, `bce59b4`, `c349e3a`, `6abaa5a`)
or the v30 adaptations (`b7da8c3`, `fa7ef1f`).

## Evidence

- `.omo/evidence/ulw/rendering-review-20260906/gui/S0-dense-content-4sessions.png`
  — the blank-pane window capture (visual).
- `.omo/evidence/ulw/rendering-review-20260906/gui/switch-debug-trace-bisect.jsonl`
  — full frontend trace of the reproducing run.
- `.omo/evidence/ulw/rendering-review-20260906/gui/bisect-20567f5-dev.log`,
  `boot-trace-bisect.jsonl` — the bisect run's logs.
- Positive pipeline proofs on the increments themselves remain valid: renderer
  contract 26/26 on Metal (pixel-readback integrity), D5 rearm suite 7/7 on the
  merged tree, native Linux 26/26.

## What is needed to close the live-GUI criteria (G006 C003, G001 C002/C003)

1. Fix main's output→presentation regression (owned by the wgpu-v30 migration
   work — not the rendering-review increments). Reproduction is deterministic:
   launch dev, feed a PTY write, observe no `terminal.surface.presented` after
   the initial restore present while `endSequence` advances.
2. Screen capture permission for the driving host (Screen Recording toggle) —
   the Orca computer-use route was removed mid-session (Orca.app moved to
   Trash), and raw `screencapture` is TCC-blocked for this host.

Until then, the loop's live-screenshot criteria remain honestly **blocked**;
they were never claimed as passing.
