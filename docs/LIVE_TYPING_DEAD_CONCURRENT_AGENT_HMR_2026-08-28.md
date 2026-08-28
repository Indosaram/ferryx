# Live Typing-Dead Diagnosis: Concurrent Agent HMR Wedge (2026-08-28 20:25 KST)

> **READ `docs/NATIVE_TERMINAL_FIRST_RESPONDER_POSTMORTEM_2026-08-29.md` FIRST** — it is the
> curated postmortem. This file is the raw investigation log; the title's HMR attribution
> turned out to explain only the trace freezes, not the input death (see FINAL VERDICT below).

**Scope:** diagnosis only — no code was written or changed for this incident.

## Symptom
_USER report, ~20:22:_ "지금 터미널에서 입력 안돼" in the running Ferryx dev app
(`cargo tauri dev`, app PID launched 20:10, daemon 20:03).

## Evidence

| Layer | Observation | Verdict |
|---|---|---|
| Daemon | `scripts/verify-terminal-typing.mjs`: all 14 sessions described instantly, rings advancing; 14 zsh PTY children alive/idle | **Healthy** |
| Webview input | Latest trace runs (`efff464c`, `6844f815`) show **0** `terminal.surface.input.capture` / `sent` / `dropped` — keydowns never reached the document despite a capture-phase listener | **Dead link** |
| Trace continuity | `/tmp/ferryx-switch-debug.jsonl` run 58f0e655 froze at 20:20:49 for **150 s** while the app process stayed alive at 0% CPU | Webview wedged |
| Concurrent churn | `git reflog`: `reset: moving to HEAD` at 20:10:38 / 20:17:49 / 20:19:50 / 20:23:18 / 20:24:18 and commit `3fa553c` ("refactor(ui): remove Tailscale detection") at **20:24:44 — during this diagnosis**. Core ui/src files (`App.tsx`, `NativeTerminalPane.tsx`, `tauri.ts`, `workspaceStore.ts`, ...) rewritten every ~40 s | Second agent session live in the same repo |
| Alignment | Every trace gap/new runId aligns 1:1 with those git operations (HMR full-reloads); run 58f0e655 alone shows 18 attach/18 detach lifecycle churn in 6.5 min | HMR reload storm |

## Root cause
A **second coding-agent session is actively editing and committing in this same working
tree** while the user interacts with the dev app. Because the app runs on the Vite dev
server, every one of its writes hot-reloads into the user's running app within ~1 s (a
previously documented hazard). Each reload wedges or remounts the webview; during those
windows the terminal panes remount in a detach/attach storm and keystrokes never reach the
document — which reads to the user as "터미널에서 입력이 안 된다".

The daemon, PTY sessions, and the (already fixed) input pipeline were all innocent.

## Not the cause (ruled out)
- Daemon connection stall / 15 s timeout path (2026-08-25 issue) — daemon answers instantly.
- Detach/attach lifecycle race or capture-fallback gap (morning issues, fixed) — no
  `input.dropped` / `input.error` in the affected runs; the events simply never fire.
- Display/system sleep — caffeinate + Amphetamine assertions active.

## Remediation (options; none applied — user decision)
1. Pause or stop the concurrent agent session while manually exercising the dev app.
2. Test against a stable build (`cargo tauri build`) instead of `cargo tauri dev`.
3. Serve Vite from an rsync sandbox (`rsync ui/ /tmp/ulw-ui` + symlinked `node_modules`),
   the pattern already proven for test mutations.

## Diagnostic shortcut for next time
Before walking the input stack, check: (1) `git reflog` freshness and advancing ui/src
mtimes for a live concurrent session; (2) whether trace gaps align with that session's
git ops. If yes, the cause is environmental churn, not a code defect.

---

# FINAL VERDICT (2026-08-28 23:5x, instrumented live reproduction) — supersedes the HMR attribution above

The user challenged the HMR conclusion ("진짜 이거 맞음?") and asked for instrumented
verification. The HMR story above explained only the dev-app TRACE-FREEZE timeline. Reproducing
typing-dead in the RELEASE build — no vite, no HMR, no agent churn — exposed the real,
HMR-independent input killer.

## Reproduction (release bundle, one GUI + one daemon)
1. User opens a terminal tab (23:48:21 `fce6267e`, 23:51:35 `facf4a41`) and types:
   **zero** `input.capture`, **zero** `input.sent`, ring endSequences frozen (7, 8).
2. User clicks into the terminal and types again at 23:56:02: exactly ONE key
   (`ㅁ`, activeElement `BODY/`) → capture + sent → daemon ring 8→11 → the char echoes
   (screenshot). Every subsequent keypress: nothing. **"한글자만 쳐짐" — one character per click.**
3. Trace-history corroboration: no multi-char typing burst exists anywhere in
   `/tmp/ferryx-switch-debug.jsonl`; every successful capture in the whole day (19:22,
   19:59, 21:08) is 1-3 isolated keys — the one-per-click signature was present all along.

## Root cause (code-confirmed mechanism)
`SurfaceHost::render_snapshot` (`surface_host.rs:1206`) calls
`restore_first_responder(window)` after **every** `frame.present()`.
`platform/macos.rs::restore_webview_first_responder` then unconditionally calls
`ns_window.makeFirstResponder(WKWebView NSView)` (the outer container view).
The fatal loop: click → inner key-handling view becomes first responder → key 1 reaches
the DOM and is sent → PTY echo triggers a repaint → the present calls `makeFirstResponder`
on the container → **first responder is stolen from the inner view** → every following
keypress never reaches the DOM → dead until the next click. Once the container holds first
responder, later restore calls are measured no-ops, so the dead state persists indefinitely
(this morning's offscreen AppKit probe missed all of it because its window was never key).

## Fix (implemented 2026-08-29 00:0x)
`macos.rs::restore_webview_first_responder` now reads `ns_window.firstResponder()` and
returns early when it is an `NSView` equal to or a descendant of the webview NSView — the
guard this morning's invalid probe got us to retire. `cargo check`, 353 `cargo test --lib`,
and UI `tsc` all green. Temporary DOM-side instrumentation (`ui/src/lib/liveInputDiag.ts`
+ its import in `ui/src/main.tsx`) removed in the same pass. Remaining: live verification
in a rebuilt app — click once into the terminal, type a sustained burst, expect every key
to land in the daemon ring (`scripts/verify-terminal-typing.mjs`). Requires rebuilding and
relaunching the app; the running release build still carries the bug.

---

# RETRACTION (same session, 20:3x KST) — THE ROOT CAUSE ABOVE IS NOT SUPPORTED

I re-tested my own verdict against the full trace instead of the window I had sampled.
It does not survive. Recording the refutation rather than quietly amending the file.

## The fatal confound I never checked
The trace holds **611 runs**. Split by whether typing reached the DOM:

| bucket | count |
|---|---|
| runs with `input.capture > 0` | **4** |
| runs with `capture = 0` AND duration < 5 s | **482** |
| runs with `capture = 0` AND duration >= 5 s | 126 |

482 of the "dead" runs lived **under five seconds**. No human types in a 0.6 s page load.
`capture = 0` in those runs is not evidence of broken input — it is the absence of a
typing opportunity. I read "no capture events" as "keys are being swallowed" when it
overwhelmingly means "nobody pressed a key".

## The claim that actually breaks the theory
The longest `capture = 0` runs are **9281 s, 8114 s, 3899 s, 2422 s** (08:18-14:38 and
20:37-21:13). Those are hours-long, stable runs in git-QUIET windows with no reset storm.
If HMR churn were the cause, typing would have worked there. It shows `capture = 0` too.

### CORRECTION TO THIS RETRACTION (I overclaimed once; fixing it)
I first wrote that run **`4534b9e4` (18:52-19:26)** proved "typing worked straight THROUGH the
git/HMR activity" because it spans the commits at 18:53/18:54/18:55 with capture=3, sent=3.
**That inference was wrong.** An HMR full-reload starts a NEW runId; `4534b9e4` survived those
commits intact, which means those commits **never reloaded the page at all**. The captures are
timestamped 18:57:07-18:57:11 — *after* the commits, inside a run that was never interrupted.
So it does not show typing surviving a reload.

What it DOES show, and this still cuts against the theory: three commits in a row produced
**no page reload whatsoever**. The premise "every commit triggers a reload storm" is therefore
false as stated. But the load-bearing refutation is the duration confound above, not this run.

## Two supporting claims were also wrong
1. **"The webview froze for 150 s."** Run `58f0e655`'s final events are
   `worktree.refresh.complete` / `workspace.render` at 20:20:49 — a normal page teardown
   ending the run. A frozen webview cannot emit teardown events. That was a run ENDING,
   not a webview wedging. I mislabelled a run boundary as a freeze.
2. **"Another agent is rewriting core ui/src files every ~40 s."** Every reflog entry I
   cited is `reset: moving to HEAD` onto the SAME sha (`b7d855c` / `3fa553c`) — a **no-op
   reset**. It rewrites files on disk with identical bytes and fresh mtimes, which is
   exactly the 20:10:40 identical-mtime cluster I flagged. The reloads are real; the
   "core files being rewritten with new code" is not. My own files kept passing 61/61
   throughout, which should have warned me.

## What stands and what does not
- STANDS: daemon/PTY healthy (14 sessions, rings advancing); a concurrent session IS
  committing in this tree; no-op resets DO trigger Vite reloads.
- DOES NOT STAND: "HMR storm causes typing to die." Unproven and contradicted by `4534b9e4`.
- HONEST STATUS: for the user's ~20:22 report I have **no captured keystroke**, so there is
  no evidence of what the input path did at that moment. The correct answer is
  "unreproduced", not a mechanism. Diagnosing without a captured RED is what produced the
  error above.

## Method note worth keeping
I sampled the reload-heavy window, found reloads, and declared causation — without ever
checking the quiet windows where the same effect appears. The refutation cost one query
over data I already had. Test the null window BEFORE naming a cause.

---

# RETRACTION OF THE RETRACTION (21:11 KST) — THE ORIGINAL VERDICT WAS RIGHT

I refuted the HMR-storm theory above. **That refutation was wrong.** Direct observation
falsifies it. Recording the full reversal rather than quietly deleting my error.

## The observation that settles it
The user typed on request at ~21:09-21:11. Result: **`capture = 0`** — keys never reached the
document. In the same window the page reloaded **7 times in ~105 seconds**:

```
21:09:25  0.7s     21:10:08  0.9s     21:11:14  0.9s
21:09:34  0.8s     21:10:24 14.3s
21:09:58  9.7s     21:10:45 26.7s
```

## The decisive contrast — same app, same session, ~1 minute apart
| time | page state | key input |
|---|---|---|
| 21:08:15 | run `a6dd67d4`, **839 s old and stable** | Cmd+`,` **captured** |
| 21:09-21:11 | **7 reloads in 105 s** | typed, **capture = 0** |

A key was captured while the page was stable, and keys vanished while it was thrashing.

## Cause, matched 1:1
`git reflog`: **five commits landed in the same second at 21:10:35**, another (`f986b2b`) at
**21:11:14** — and a page reload is stamped at **21:11:14**, the same second. Eight `ui/src`
and `src-tauri/src` files were rewritten within three minutes by the concurrent session.
Vite full-reloads on each; a reloading document has no capture listener, so keystrokes are
discarded.

## Why my refutation failed
I argued from aggregate trace statistics (482 runs shorter than 5 s all showing `capture=0`)
that `capture=0` means "nobody typed". The statistic was true; the inference was not.
**"Typing attempts were not recorded" does not entail "typing was not broken."** Absence of
capture events is exactly what BOTH hypotheses predict, so the aggregate could never
discriminate between them — I treated non-discriminating data as evidence for one side.
The only thing that could decide it was a keystroke made *during* a storm window, which is
what the user then supplied. One directed observation beat 611 runs of passive data.

## Standing conclusion
- The concurrent-agent HMR reload storm **does** kill typing in the dev app. Original verdict
  restored.
- Independently true: the branch-(b) capture-fallback fix is loaded in the running app
  (verified via the Vite-served module: `toForwardableKeyEvent`, `isClipboardShortcut`,
  `shouldForwardKey`, `physicalKeyForAltChord` all present) and is unrelated to this failure.
  During a reload there is no listener at all for any fix to run in.
- Remedy: stop the concurrent session, or test on a release build that does not use Vite HMR.

## Method lesson (corrected)
My earlier lesson said "test the null window before naming a cause". The deeper error was
different: I let a non-discriminating aggregate overturn a mechanism, and I demanded a
captured RED while simultaneously arguing from data that structurally could not contain one.
When a statistic is equally consistent with both hypotheses, it is not evidence — go get the
one directed observation that separates them.
