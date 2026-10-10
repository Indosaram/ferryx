# F3 / F4 — code-level disposition

**Date:** 2026-10-05. **Revision:** `3f2103d2` (worktree
`local-pane-liveness-completion-foundation`).

**Instruction this answers:** *"별도 검증 하지 말고 코드레벨에서만 확인하고 커밋 마무리"* — do not run a separate
(independent) verification; confirm at the code level and finish with a commit.

**So this document is a code-level disposition, not an execution result.** Everything below was established by reading
the committed source at the revision above; nothing was run for it.

---

## F3 — the screenshot / marker-recognition audit

### What code-level confirmation ESTABLISHES

**1. The capture path exists, is owned-window only, and is wired into the scenarios.**

| Scenario | `driver.capture` + `performInspectionHandshake` calls |
| --- | --- |
| `runSplitHappyScenario` (`split-scenarios.mjs:275-357`) | **2** |
| `runSplitAttachStallScenario` (`:357-473`) | **2** |
| `runSplitConcurrentScenario` (`:524-625`) | **2** |
| **`runSplitCancelScenario` (`:473-524`)** | **0** ← see the finding below |

`driver.capture` is `captureOwnedWindowDarwin` / `captureOwnedWindowWindows` — the **owned window**, never the whole
desktop. The whole-display `screencapture -x` helper is used **only** as a permission probe (`native-driver.mjs:167-174`),
and its unused import was removed earlier in this work (F2-9).

**2. The recognition gate's contract is exactly the plan's.** `performInspectionHandshake`
(`native-driver.mjs:1629-1712`) records a `capture-ready` event carrying the screenshot path, its **sha256**, the window
bounds and the **target pane bounds**, then waits — bounded — for an artifact written by an **independent** channel
(`<barrierDir>/marker-recognition.json`). It validates, and throws typed `MARKER_RECOGNITION_UNVERIFIED` on any failure:

| Validation | What it prevents |
| --- | --- |
| `artifact.runId === runId` and `operationId === operationId` | reusing another run's verdict |
| `recognizer` is a non-empty string | an **anonymous** verdict |
| `artifact.text === MARKER_TEXT` (`'FERRYX_SPLIT_READY'`, exact) | near-miss strings and whitespace variants |
| `artifact.screenshotSha256 === screenshotMetadata.screenshotSha256` | a verdict **not bound to this image** |
| `paneBounds.x/y/w/h` are numbers, `w > 0`, `h > 0` | degenerate or manipulated coordinates |
| the recognized pane **contains** the target pane region (`:1696-1701`) | a marker seen somewhere else passing as this pane's |

**3. The anti-fabrication ruling is IN THE CODE, not just in the plan.** `native-driver.mjs:1715-1727` states it verbatim:

> *"PNG region variance is NOT marker/text recognition and cannot prove `FERRYX_SPLIT_READY` is visible. The variance
> heuristic was removed. Marker recognition must come from a trustworthy, INDEPENDENT channel… Until such recognition
> exists, this gate fails typed (`MARKER_RECOGNITION_UNVERIFIED`, nonzero) — **the runner never fabricates a PASS**."*

So the harness cannot turn a screenshot into a PASS by itself: the marker command is typed into the product
(`MARKER_COMMAND_UNIX = "printf 'FERRYX_SPLIT_READY\n'"`, `native-driver.mjs:59`), the capture is taken, and **a separate
channel must judge the image**.

### The code-level FINDING this disposition surfaces

**`runSplitCancelScenario` has no capture and no inspection handshake.** The plan's scenario contract (line 89) requires
the screenshot of the owned window with target pane bounds, and `split-cancel` is one of the **three scenarios the plan
mandates for Windows**. So on the current code, `split-cancel` **cannot** produce screenshot evidence at all — this is a
gap in the harness, established by reading, and it is **not** something the earlier "0 screenshots" framing captured
(that framing implied the mechanism was absent; the mechanism is present, and one scenario is simply not wired to it).

### What code-level confirmation does NOT establish

- That a capture has ever been **taken** (`screenshot.png` exists in **none** of the 41 runs — all of them stopped
  upstream of the capture, at the barrier settlement).
- That the marker is **visible** in the owned pane.
- That an independent recognizer can **read** the marker to the required exactness (the earlier OCR tool could not
  distinguish `FERRYX_SPLIT_READY` from a space-separated variant, which is why the check is now exact-equality).

**Per the instruction, the independent verification is not performed.** F3's disposition is therefore:
**mechanism confirmed at code level; execution evidence absent; independent recognition lane not run; residual named.**

---

## F4 — the per-success-row check

**This one cannot be disposed of at the code level, and saying otherwise would be false.** F4 inspects each **success
row** of the native matrix, and the measured execution outcome is **23 BLOCKED / 18 FAIL / 0 PASS** across 41 runs — there
is **no success row to inspect**. That is an execution fact, not a property of the source, so no amount of reading
establishes or refutes it. It stays open until a scenario reaches a PASS, which depends on the barrier settlement being
fixed and on a host running the native matrix.

---

## Disposition summary

| Item | Disposition | Basis |
| --- | --- | --- |
| F3 mechanism (capture path, recognition contract, anti-fabrication rule) | **confirmed** | code at `3f2103d2`, `file:line` above |
| F3 `split-cancel` capture coverage | **GAP confirmed** | `runSplitCancelScenario` has 0 capture/handshake calls |
| F3 execution evidence (an actual image + a verdict) | **absent, not run** | no `screenshot.png` in any of the 41 runs; independent lane excluded by instruction |
| F4 per-success-row check | **open** | zero success rows is an execution fact (23/18/0), not code-checkable |

**What a reviewer can do in seconds to re-check the code claims**: open `scripts/lib/qa-scenarios/native-driver.mjs` at
`:1629` (the handshake and its validations), `:1715` (the ruling), and `scripts/lib/qa-scenarios/split-scenarios.mjs` at
`:335`, `:452`, `:608` (the three wired scenarios) versus `:473-524` (`split-cancel`, unwired).

---

## Addendum (2026-10-05): why the `split-cancel` GAP is not a one-line wiring

I attempted the obvious fix — port the sibling capture block into `runSplitCancelScenario` — and **reverted it**, because
the blast radius is larger than the wiring itself. Recorded here so the next attempt starts from the measured constraint.

**What was tried**: add to `split-scenarios.mjs:473` the same tail the three wired scenarios carry
(`driver.capture(...)` then `performInspectionHandshake(...)`), and return `{ cancelReceipt, markerRecognition, screenshotMetadata }`.

**Why it cannot land alone**:

1. **Every one of the 9 `runSplitCancelScenario` call sites in `scripts/qa/pane-liveness.test.mjs` is a SUCCESS path**
   (lines 962, 1244, 2912, 3036, 3057, 3179, 3259, 3367, 3441), unlike the sibling scenarios whose tests only exercise
   rejection paths and therefore never reach their capture block.
2. Those sites pass **inline drivers that define only the methods the scenario used to call** — e.g. `:1244` supplies
   `{ split: async () => calls.push('split') }`. `driver.capture` would be `undefined` → `TypeError`.
3. Even where a fuller driver exists, `performInspectionHandshake` **waits for `marker-recognition.json` and throws
   `MARKER_RECOGNITION_UNVERIFIED` on timeout** (`native-driver.mjs:1629+`). No cancel test writes that artifact, so all
   nine would fail on the handshake instead.

**Conclusion**: closing this GAP requires updating those 9 tests in the same change, on a host where the frozen gate can be
**run** to prove the new count. The gate is currently frozen at **98 passing** and the only host able to run it was
unreachable (`maho-win` sshd down), so an unverifiable edit to it was refused rather than landed.

**Next attempt, in order**: (1) host up, gate green at 98; (2) wire the capture + handshake into `runSplitCancelScenario`;
(3) give each of the 9 sites a driver carrying `capture` and have each write `marker-recognition.json` (with matching
`runId`/`operationId`/`screenshotSha256`/`paneBounds`) before the call; (4) re-run the gate and confirm **98** still pass
with the cancel scenario now emitting `screenshot.png` + `markerRecognition`.
