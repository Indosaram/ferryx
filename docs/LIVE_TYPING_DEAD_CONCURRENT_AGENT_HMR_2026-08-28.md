# Live Typing-Dead Diagnosis: Concurrent Agent HMR Wedge (2026-08-28 20:25 KST)

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
