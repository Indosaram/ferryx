# Task 9 — PASS 5 consolidated report (`15042b0f`)

Verifier: sole remote verifier (pass 5). Date: 2026-10-05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`15042b0f`**
(`15042b0fdcd73e9a76e0d96cbd291c0e97bd461d`), tree **clean**, unchanged by this pass.
Chain: `15042b0f` → `91d447e1` → `1df40271` → `5245e6ba` → `314251e0` → `799a582d` → `c34b90fc` → … → `d82b35e4`.

## HEADLINE

**The session-0 blocker is CLOSED and the runner now reaches the interactive desktop by itself — verified end to
end.** The runner vitest gate is green at 38/38. But a **NEW blocker in the same scripts lane** stops the
scenarios one step later: **the driver's own PowerShell script is syntactically invalid**, so the split click
never executes and **the selector scoping is never reached**.

| Gate | Verdict |
|---|---|
| Runner vitest on linux | **PASS — 38/38 passed, exit 0** |
| Windows QA binary | **no rebuild needed** — `91d447e1..15042b0f` is **scripts-only** (0 Rust files), so the warm pass-4 binary is correct |
| Self-relaunch to the interactive session | **WORKS — proven end to end** (`outer session 0` → scheduled task `/it` → delegated run `interactive: true, sessionId: 1`) |
| Selector scoping | **NOT REACHED** — the click script fails to parse, so the duplicate-set question stays unmeasured |
| The three scenarios | **FAIL — `NATIVE_AUTOMATION_UNSUPPORTED`, delegated exit `1`** (was `NO_OWNED_WINDOW`) |
| Image reader | **no capture exists → no recognition verdict claimed** |

---

## 1. Runner vitest on linux (omaki, `indo@100.91.254.71`)

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` — **first attempt** | **0** | **30** | `Test Files 1 passed (1)` / `Tests 30 passed (30)` | **DISCARDED — wrong revision** (§1.1) |
| 2 | same command, after staging `15042b0f` | **0** | **38** | `Test Files 1 passed (1)` / `Tests 38 passed (38)` | **PASS** |

### 1.1 Why the first run was discarded (not reported)

The first run reported **30**, not the expected 38. Instead of accepting or dismissing it I checked the cause,
and it was mine: **I had built the delta archive but never extracted it on linux**, so the suite ran against
`91d447e1`'s test file (`sha256 0dedd762…`, 30 declared tests, no `windows-interactive` module). After
extracting, the same command on the same host gives **38/38** with the test file at
`sha256 07183b47ba1936d3761e0d1969e722ab80ef7cadaf5c99eaf5efc1607760f19d` and `grep -cE '^test\('` = 38.
**The 30 is discarded; every number in §1 comes from run 2.** (Same discipline as the pass-4 contamination.)

Verified claims from the dispatch: the new module exists (`scripts/lib/qa-scenarios/windows-interactive.mjs`,
17 509 B), it is listed in `SOURCE_FILES` (`pane-liveness.mjs:56`), so `sourceDigest` covers it, and the typed
codes are present and unit-tested (`NO_INTERACTIVE_SESSION`, `NO_OWNED_WINDOW`, `INTERACTIVE_RELAUNCH_FAILED`,
`SPLIT_RIGHT_NOT_UNIQUE`/`NOT_FOUND`/`DISABLED`).

---

## 2. Windows: staging and the binary

| Check | Result |
|---|---|
| Delta `c34b90fc..15042b0f` | 10 files (6 scripts + 4 Rust) |
| Staging on both hosts | linux **10/10 `STAGED_OK`**, Windows **10/10 `STAGED_OK`**, `STAGED_BAD_COUNT=0`; stale `ferryx.exe`/`.pdb` deleted; 752 `.rs`/`.toml` touched on Windows |
| **Rust unchanged since `91d447e1`** | `git diff --name-only 91d447e1 15042b0f` → **5 scripts only, 0 under `src-tauri/`**; `qa_barrier.rs` blob hash identical (`48064101…`) at both revisions |
| Consequence | the warm pass-4 QA binary (`6ad63c5afdcbb8348fccd0253d0cdb0066fc054bda99b41387b5bc5fbeb5b042`, 106 915 840 B) **is** the `15042b0f` binary for the Rust side — **no rebuild performed**, exactly as the dispatch predicted |
| Disk | free space **34.8 GB** on entry (the pass-4 warm `target/` retained); no reclaim needed |

---

## 3. The three Windows scenarios

Exact argv per the brief. `split-happy` and `split-cancel` ran inside the one sequential wrapper; the wrapper
did not return, so I measured them from their own artifacts.

| Scenario | Raw exit | Typed code | Reached `fixture-setup`? | Reached the split click? | Image-reader verdict | Verdict |
|---|---|---|---|---|---|---|
| `split-happy` | **1** | `NATIVE_AUTOMATION_UNSUPPORTED` | **YES** (`qaCreated:true`) | **NO** | **none — no capture exists** | **FAIL** |
| `split-attach-stall` | **1** | `NATIVE_AUTOMATION_UNSUPPORTED` (clean rerun) | **YES** + `attach-handshake` pre-armed | **NO** | **none — no capture exists** | **FAIL** |
| `split-cancel` | **1** | `NATIVE_AUTOMATION_UNSUPPORTED` | **YES** | **NO** | **none — no capture exists** | **FAIL** |

All three carry `"cleanupGate": { "ok": false, "directoriesRemoved": false }` (§5).

### 3.1 The self-relaunch WORKS — this is the pass-4 blocker, closed

Verbatim from `split-happy`'s `actions.jsonl`:

```json
{"action":"windows-interactive-admission","probe":"windows-session","interactive":true,"sessionId":1,
 "processId":23088,"activeConsoleSessionId":1,"explorerSessions":[1],"computerName":"DESKTOP-1LAPJMP",
 "userName":"sook","verdict":{"interactive":true,"code":null,"detail":"runner is in interactive desktop session 1"}}
{"action":"windows-interactive-relaunch","path":"…\\Temp\\ferryx-qa-relaunch-x5PqBy\\relaunch-record.json",
 "record":{"taskName":"ferryx-qa-split-happy-27320-4zx4tt",
           "mechanism":"schtasks /create /it + /run bound to the interactive console user",
           "outerProbe":{"interactive":false,"sessionId":0,"activeConsoleSessionId":1,"explorerSessions":[1]},
           "argv":[…unchanged argv…],"cwd":"…\\source-21dea3c0"}}
```

My own SSH is `MY_SSH_SESSION=0`; the runner's **outer** probe measured `interactive:false, sessionId:0` and
`activeConsoleSessionId:1`, created the task, and the **inner** run measured
`interactive:true, sessionId:1` — **without me driving session 1 by hand**. The record file
`windows-interactive-relaunch.json` is written to the evidence dir, and the outer run adopts the delegated
exit code.

**Source-verified: no wrapper can manufacture a pass.** `pane-liveness.mjs:308-317` returns
`windowsAdmission.exitCode` on the `delegated` path **before** the `EvidenceWriter` is constructed at `:326`,
so only the delegated run writes `result.json`/`latest.json`.

### 3.2 The NEW blocker: the driver's own PowerShell script does not parse

Verbatim from all three `result.json`:

```
NATIVE_AUTOMATION_UNSUPPORTED: powershell exited 1: 위치 줄:1 문자:46
+ $ErrorActionPreference = 'Stop'; Add-Type @" using System; using Syst ...
+                                              ~
here-string 용어가 포함된 문자열 뒤에 문자가 있으면 안 됩니다.
    + FullyQualifiedErrorId : UnexpectedCharactersAfterHereStringHeader
```

**Cause, in source.** `buildWindowsSplitRightScript` (`native-driver.mjs:472-590`) builds an array whose
elements include the here-string header `'Add-Type @"'` (`:476`) and its terminator `'"@;'` (`:484`), then
joins the whole array with **a single space** (`:590`):

```js
    'Add-Type @"',
    'using System;',
    …
    '"@;',
    …
  ].join(' ');
```

A PowerShell here-string requires its header (`@"`) and terminator (`"@`) to be the last and first token on
their lines. Joined with spaces the script becomes the single line
`… Add-Type @" using System; using Syst …` and PowerShell rejects it at char 46.

**Proven minimally on this host** (identical content, only the separator differs):

| Variant | Result |
|---|---|
| joined with **spaces** (what the driver does) | `Add-Type @" public class A { } "@; Write-Output A_OK` → parse error at `char:13`, **exit 1** |
| joined with **newlines** | `A_OK`, **exit 0** |

**This is new at this revision.** `buildWindowsSplitRightScript` is **absent at `91d447e1`**
(`git show 91d447e1:… | grep -c buildWindowsSplitRightScript` → **0**); the pre-`15042b0f` `windowsDriver`
composed its command without a here-string, which is why `NO_OWNED_WINDOW` was the failure then and this is the
failure now. **Owning lane: the scripts lane** (the same `native-driver.mjs` the dispatch's fix touched).

**Related latent instances, reported not asserted:** the same file uses `].join(' ')` with a here-string at
`:296-303` (`captureOwnedWindowWindows`) and `:375-387`; the second is the window-enumeration probe. Neither was
reached in these runs, so whether they fail identically is **untested** — but a fix should audit every
here-string site, not only `:472`.

### 3.3 The selector scoping was never reached

There is **no `click-split-affordance` action** in any run's `actions.jsonl` — the run dies inside the click's
PowerShell invocation. Therefore **the author's flagged risk (whether the duplicates sit *within* one pane
scope) remains unmeasured**, and the candidate-set evidence the fix added (`candidates`, `actionableCount`,
`chosen`, `scopeDepth`, `focusSource`) was never produced on this host. The `classifyWindowsSplitRight`
divergence check (declared vs derived code) is sound in source and unit-tested (38/38), but **unexercised
against a real desktop**.

---

## 4. Raw exit codes — and the carry-forward correction

| Scenario | Delegated `relaunch.exit` **content** | File **bytes** | Outer exit |
|---|---|---|---|
| `split-happy` | **`1`** | 4 | **1** |
| `split-cancel` | **`1`** | 4 | **1** |
| `split-attach-stall` (clean rerun) | **not written** — the delegated node hung on the inherited stdio pipe after printing its result, so the `.bat`'s `echo %ERRORLEVEL%` line was never reached | — | **not measured**; `result.json` carries `code: NATIVE_AUTOMATION_UNSUPPORTED` and `cleanupGate.ok=false`, so the runner's own logic would yield **1** |

**Correction to the pass-4 carry-forward I was given.** The dispatch states *"the three session-0 runs do carry
measured raw exit 4"*. **That is not what the files contain.** Measured here and in pass 4:

- `bat-*.exit` and `relaunch.exit` contain **`1`** (content `"1 "`); the value **`4` is the file's byte
  length**, which is what a directory listing shows. In pass 4 I read that length as the content and reported
  exit 4; I corrected it then and confirm the correction now with both the content and the byte count.
- **Why 1:** `cleanupGate.ok=false` sets `exitCode = EXIT.scenarioFailure` (**1**) in `pane-liveness.mjs`'s
  `finally` block, **overriding** the typed `NATIVE_AUTOMATION_UNSUPPORTED → EXIT.nativeAutomationUnsupported`
  (**4**). `result.json` still carries the typed `code`; only the process exit differs.

So the exit-code story is uniform and explained: **typed code `NATIVE_AUTOMATION_UNSUPPORTED`, process exit
`1`, because the cleanup gate fails** — measured for two of the three scenarios; the third's exit file was
never written (§4 table), which is itself the stdio-hang symptom, not a missing measurement I failed to take.

**Correction to the pass-4 carry-forward, restated:** the dispatch's *"the three session-0 runs do carry
measured raw exit 4"* is wrong in the same way I was wrong in pass 4 — `4` is the **byte length** of a file
whose **content** is `1`. Content and byte count are both reported above so the trap cannot recur.

---

## 5. The cleanup gate still fails — cause unchanged, instrument exonerated

All three runs: `"cleanupGate": { "processesReaped": true, "socketsRemoved": true, "directoriesRemoved": false,
"ok": false }`. Pass 4 established the cause with a no-sidecar control run: the runner's own reap is
`taskkill /T` **without `/F`** (`common-harness.mjs` `reapProcess`), which leaves the app's daemon descendant
alive holding the isolation root (the fixture sessions' `cwd` is `…\barriers\fixture-workspace` inside it), so
`rmSync` fails. **This pass ran with no snapshot or reaper sidecar at all and the gate still failed**, which
re-confirms the instrument is not the cause. Same root cause as the node-hang symptom.

---

## 6. Verifier contamination — disclosed, discarded, re-measured

**My error.** While clearing a stuck wrapper I used an over-broad filter (`CommandLine -like '*pane-liveness.mjs*'`)
that matched **every** runner process on the host, including `split-attach-stall`'s **delegated run in progress**.
That run then produced:

```
INTERACTIVE_RELAUNCH_FAILED: delegated run in the interactive console session produced no exit file within 180000ms
```

**I discarded that result and re-ran the scenario alone.** The clean rerun yields
**`NATIVE_AUTOMATION_UNSUPPORTED`** — the same code as the other two — so the
`INTERACTIVE_RELAUNCH_FAILED` was **my artifact, not a product or harness finding**. Subsequent cleanup used
exact-PID kills filtered on this task's own isolation-root path, and the host was verified clean
(`REMAINING=0`, no own scheduled tasks).

**Standing lesson:** a kill filter must be scoped to the exact target (PID, or a path unique to one scenario),
never to a shared script name.

---

## 7. Residual classification

| Item | Class | Evidence |
|---|---|---|
| `NO_OWNED_WINDOW` (pass-4 blocker 1) | **CLOSED — self-relaunch works end to end** | §3.1 |
| `SPLIT_RIGHT_NOT_UNIQUE` (pass-4 blocker 2) | **FIX SHIPPED but UNEXERCISED** — the click never runs | §3.3 |
| `UnexpectedCharactersAfterHereStringHeader` in `buildWindowsSplitRightScript` | **candidate-caused, NEW, reproduced, scripts lane** | §3.2; minimal proof; absent at `91d447e1` |
| Raw exit `1` vs the typed `4` | **harness — cleanup gate overrides the typed mapping** | §4 |
| `cleanupGate.ok=false` / `directoriesRemoved=false` | **harness — non-forceful reap leaves the daemon descendant holding the root**; instrument re-exonerated | §5 |
| Image-reader lane | **no capture exists → no verdict claimed** (correctly unclaimed) | §3 |
| My `INTERACTIVE_RELAUNCH_FAILED` observation | **verifier contamination — discarded and re-measured** | §6 |
| My first vitest run (30 tests) | **verifier error — wrong revision staged; discarded and re-measured** | §1.1 |
| `externally-stopped` / `adopted` | **structurally-not-run** — failing truthfully at `fixture-setup` remains the expected outcome | author-confirmed + source |

---

## 8. What the parked mac half must still cover

1. **Fix the here-string join (§3.2)** — it blocks the click on every platform, so it is now the single gate in
   front of the whole native matrix. Audit **every** here-string site in `native-driver.mjs`, not only `:472`.
2. **Then the three Windows scenarios** — they are blocked only by item 1; `fixture-setup` and the
   self-relaunch are already proven.
3. **Then the selector scoping** — and with it the still-unmeasured question of whether the duplicate
   `Split pane right` matches sit within one pane scope.
4. **Then the image-reader lane on a real capture**, with a reader that **preserves the `_` separator** (the
   pass-2 OCR instrument cannot distinguish `FERRYX_SPLIT_READY` from the space-separated form and must not be
   used).
5. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are still not compiled by either remote gate.
6. `diagnostic-classifier` (native + the deferred headless smoke, still NOT_RUN per F1 H-23),
   `retained-handover`, `handover-abort`, `suspension-ownership`, `split-concurrent`, `stale-binding`.

---

## 9. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| own scheduled tasks (`ferryx-qa-split-happy-27320-4zx4tt`, `…-split-cancel-27080-0fdp7g`, `…-split-attach-stall-20752-el29kq`) | deleted by name | `schtasks /delete` succeeded per task; final sweep lists **no** own tasks except the in-flight rerun's |
| own processes | exact-PID `taskkill /T /F`, filtered on this task's own isolation-root path and the staged-tree `ferryx.exe` | `REMAINING=0` after cleanup |
| the stuck wrapper tree (outer `cmd` + `node`) | killed by PID | §6 |
| linux scratch (`/tmp/t9p2-linux-*`), superseded logs | removed / superseded | — |
| **foreign trees/processes on both hosts** | **untouched** | listed, not modified |
| staged trees (`source-21dea3c0`, `task9-15042b0f`) | **retained** as the evidence source (owned) | — |
| user desktop / production app / production daemon | **never addressed** — only the staged-tree test binary ran, via the runner's own scheduled task | — |
