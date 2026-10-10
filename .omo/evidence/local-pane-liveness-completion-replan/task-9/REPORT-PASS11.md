# REPORT — PASS 11

## Scope

Two measurements, dispatched together:

1. **Resolve the pass-9 / pass-10 contradiction** on the pane frontier (same scenario, same revision,
   opposite outcomes) with an element dump at the moment the pane probe runs, and classify it (A) flake /
   (B) probe scope-or-name defect / (C) app-state difference.
2. **Measure whether the isolated profile can spawn at all** — what workspace the app believes it is in,
   whether the isolated daemon knows it, and whether a click can produce a session.

No product repairs by me. Everything below is measurement.

## Revision — stated plainly

**Every number in this report is measured on `757f8414`**, the revision staged on the Windows host at
`C:\Users\sook\ferryx-pane-completion\source-21dea3c0` (the directory name is stale; its contents are
`757f8414`). I verified the staged revision rather than trusting the directory name:

| marker in the staged `scripts/lib/qa-scenarios/native-driver.mjs` | hits | meaning |
|---|---|---|
| `uiaWarmLines` | 3 | the warm-attach pass exists (introduced by `757f8414`) |
| `warmElements` | 7 |  |
| `warmBaselineElements` | **0** | the attach **loop** (introduced by `04cf8c7a`) is **absent** |
| `warmAttachCount` | **0** | same |

So the staged tree is `757f8414` — **the same revision both pass 9 and pass 10 measured**, which is what makes
the contradiction a real contradiction. For the record, **`HEAD` has since moved to `04cf8c7a`**
(`fix(qa): re-issue the UIA attach in a bounded loop, and report what the pane step read`; 4 files, 0 Rust,
so the warm binary stays valid). **None of these numbers are on `04cf8c7a`.**

## Measurement 1 — the element dump at the pane probe

### How it was taken

The runner's own Windows pane probe (`buildWindowsNewPaneScript`) was instrumented **in the staged copy only**
(never in the candidate tree) to record, at the moment the probe decides:

- per owned window: total element count, named-element count, whether a `ControlType.Document` is present,
  and **the count of elements whose `Name` is exactly `"New Terminal"`** (`-ceq`, case-sensitive);
- the full list of accessible names (capped at 200 per window) with control types;
- a bounded sample of every element whose name or automation id matches `terminal|new|pane|tab` **or whose
  control type is `Button`** — with enabled/offscreen/rect;
- the per-attempt candidate count of the probe's own name search (33 attempts).

**Placement matters and is deliberate:** the dump runs **immediately AFTER the probe's name-search loop**, so it
cannot drive the tree and cannot change the verdict. The probe's own sequence (single warm attach → 33 name
searches) is untouched. The instrumentation is recorded verbatim in `win-pass11/native-driver.patch`.

### Result — the first run

```
warmElements (pre-search, single attach): 16      <- Chromium-internal baseline, DOM not built
warmAttempts: 33   warmElapsedMs: 4016
attemptCandidateCounts: [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
candidateCount: 0  actionableCount: 0
dumpElapsedMs: 91

WINDOW hwnd=30739108 title='Ferryx' className='Tauri Window'
       total=110  named=65  hasDocument=True  exactNewTerminal=0  sampleCount=29
WINDOW hwnd=27855178 title='' className='Tao Thread Event Target'
       total=0    named=0   hasDocument=False exactNewTerminal=0  sampleCount=0
```

**Answer to question 2: `New Terminal` does NOT appear in the dump — `exactNewTerminal: 0`.**

And note the shape of this run: the warm attach read **16** (baseline, no DOM) while the dump read **110 with
`hasDocument: true`**. **By the time the dump walked it, the tree was built — and `New Terminal` was still
absent.**

**A precision that matters, and that independently corroborates the `04cf8c7a` diagnosis:** the dump is itself a
`FindAll(Descendants, TrueCondition)` attach, so **the dump's own attach is the most likely thing that drove the
build** from 16 to 110. That is exactly the mechanism `04cf8c7a` implements ("re-issuing the attach is what drives
the lazy build"), and it means the dump incidentally *demonstrated* it: one re-attach flipped 16 → 110 in 91 ms.
But it also means I cannot claim from this run alone that the tree was already built *before* the dump — only
that **once built, the element is genuinely not there**, which is the load-bearing claim. The same conclusion
holds for the isolated runs, where the element **was** found in the very same 93/56 tree.

### What the UI actually was

The dump is not the empty state. It is the **pane state**, with a project sidebar, an open tab and a modal:

| element | control type | name |
|---|---|---|
| `Add project` | Button | enabled |
| `Strawberry` / `Collapse Strawberry` / `Remove project Strawberry` / `Strawberry primary` | Button | enabled |
| `System32` / `Expand System32` / `Remove project System32` | Button | enabled |
| `main Close main` | **TabItem** | an **existing terminal tab** |
| `New tab` | Button | enabled |
| `Split pane right` / `Split pane down` | Button | enabled |
| `Native terminal input` | Edit | the pane's input |
| `Failed to attach native terminal` | Text | **an error surface** |
| `Click to retry connecting terminal` | Text | |
| `Welcome to Ferryx` | **Window** | the **setup wizard modal** ("Sign in", "Skip setup", "Continue", "Remind me later") |
| `Ferryx for Windows is moving to the Microsoft Store…` | ListItem/Button | a notification toast |

**There is no "New Terminal" button because the empty state is not showing.** `EmptyWorkspaceView`
(`ui/src/components/EmptyWorkspaceView.tsx`) — the only surface that exposes `New Terminal` — renders when a
workspace has **no tabs**. Here the workspace already has a tab (`main`), so the app renders the **pane chrome**
(`Split pane right`, `Split pane down`, `New tab`) instead.

### Classification — **(C) an app-state difference**, with the mechanism identified

Not (A) a flake, and not (B) a probe scope/name defect:

- **Not (B):** the probe's condition is `PropertyCondition(NameProperty, "New Terminal")`, and its window
  scope is *all visible owned windows, descendants*. The dump walks exactly that same scope and finds the
  element is **absent from the tree** — so the name condition and the scope are both correct; there is nothing
  to match. (The window scope question is settled too: the second owned window,
  `Tao Thread Event Target`, has **0 elements**, so the walk was not searching the wrong window.)
- **Not (A):** the two runs differ by a **persisted input**, not by chance — see below.
- **(C), and the state is identified:** the app **restored a layout**.

### The mechanism — the app's own session state is not isolated

The app boots from `session_state.json`, resolved by `get_session_file_path`
(`src-tauri/src/ipc/session.rs`), which consults `session_dir_override()` **first**. On this host, during the
run, the file the app actually read and rewrote is:

```
C:\Users\sook\AppData\Roaming\com.ferryx.app\dev\session_state.json   (written 2026-10-05 04:17, i.e. DURING the run)
```

Its content is the app state that produced the UI above:

```json
{ "version": 3, "timestamp": 1791141474751,
  "activeWorkspaceId": "Strawberry",
  "workspaces": { "Strawberry": { "repoRoot": "\\\\?\\C:\\Strawberry",
      "layout": { "tabs": [ { "kind": "terminal", "label": "main",
        "terminal": { "primarySessionId": "session:90b1d804-c07f-40b3-adbb-75f4a4b6afdf" } } ] } } } }
```

**That file is in the host's real user profile, outside the runner's isolation root.** So "a fresh profile" is
not fresh from the app's point of view: the app restores a workspace with one terminal tab, the empty state never
renders, and the `New Terminal` affordance **cannot** exist.

**And the harness cannot currently redirect it, because of a one-variable gap:**

- The product already provides the override for exactly this purpose:
  ```rust
  pub(crate) fn session_dir_override() -> Option<PathBuf> {
      std::env::var_os("FERRYX_SESSION_DIR").filter(|dir| !dir.is_empty()).map(PathBuf::from)
  }
  ```
  (`src-tauri/src/daemon/server.rs`, documented as "the QA and multi-instance session directory override").
- The runner's child environment is an **allowlist** (`buildIsolatedEnv` in
  `scripts/lib/qa-scenarios/diagnostic-classifier.mjs`), and it sets `FERRYX_DATA_DIR` and
  `FERRYX_RUNTIME_DIR` — **but not `FERRYX_SESSION_DIR`**. It even *throws* on any `FERRYX_*` key outside its
  allowlist, so the variable cannot be supplied from outside either.

So the app falls back to `app_data_dir/dev/session_state.json` — the host's real profile — and the pane step's
verdict depends on a file that persists across runs and is shared with the user's real app usage.

**This is the whole contradiction.** Pass 9 found `New Terminal` because at that moment the host's
`dev/session_state.json` carried no restorable layout (the empty state). Pass 10 and pass 11 did not, because by
then that same host-global file carried the restored `main` tab. Same revision, same scenario, same probe — a
different persisted input. **The fix is in what the harness does before the pane step** (one line in the
allowlist), **not in the probe and not in the product.**

## Measurement 2 — the two runs compared explicitly (question 3)

Pass 9's raw actions are on disk (\`win-pass9/runs/split-happy/task-3-harness/split-happy/run-8e075644-…/actions.jsonl\`),
so this is a field-by-field comparison of two real runs, not a reconstruction.

| dimension | **pass 9** (found + clicked) | **pass 10** (found nothing) | **pass 11 run 1** (found nothing) |
|---|---|---|---|
| revision | \`757f8414\` | \`757f8414\` | \`757f8414\` |
| action sequence | \`barriers.prearmed → windows-interactive-admission → windows-interactive-relaunch → frontend.served → launch.binary → fixture-setup → powershell → owned-window → powershell → owned-windows-enumerated → powershell → click-pane-affordance\` | **identical, same order** | **identical, same order** |
| owned windows | 2 — \`Ferryx\` (15796732) + \`Tao Thread Event Target\` (44696800) | 2 — \`Ferryx\` (6229050) + \`Tao…\` (33031808) | 2 — \`Ferryx\` (30739108) + \`Tao…\` (27855178) |
| windows searched | both | both | both |
| the \`Tao…\` window's element count | not recorded | not recorded | **0** |
| **\`warmElements\` (the warm attach)** | **16** | **16** | **16** |
| \`warmAttempts\` / \`warmElapsedMs\` | **1 / 29** | 33 / 4036 | 33 / 4016 |
| \`candidateCount\` / \`actionableCount\` | **1 / 1** | 0 / 0 | 0 / 0 |
| element in tree (fresh dump, after the search) | — (matched on attempt 1) | — | **\`total: 110, named: 65, hasDocument: true, exactNewTerminal: 0\`** |
| probe was the run's first UIA attach | **yes** (same sequence) | yes | yes |
| outcome | clicked \`New Terminal\` @ \`910,715,156,36\` | no click | no click |

### The hypothesis in my pass-9 note is **killed**

Pass 9's note said: *"The most likely remaining explanation — an earlier \`TrueCondition\` attach in that run's own
path, followed by >= ~320 ms before the pane probe — is a hypothesis I did not verify."*

**It is false, and the evidence is the \`warmElements: 16\` column.** All three runs read the **same 16** at the
warm attach, and all three reached the pane probe through the **same action sequence** (verified action-by-action
above). So there was no extra attach in pass 9's path: the warm attach was the probe's first UIA call in every
run, and it read the same baseline every time.

What actually differs is **\`warmAttempts: 1\` vs \`33\`** — and that is an *effect*, not a cause. Pass 9 matched on
the very first name search (29 ms after the attach) because the element **was in the tree**; the other runs
searched 33 times over 4 s and never matched because it **was not**. And the fresh dump settles which: after the
search, the tree was fully built (**110 elements, \`hasDocument: true\`) and \`New Terminal\` still had
\`exactNewTerminal: 0\`.** A built tree with no such element cannot be a laziness problem.

So the answer to question 2 is: **\`New Terminal\` does NOT appear in the dump**, and therefore the app's UI state
differed between the runs — **(C)** — with the state identified in §"The mechanism" above.

### The project's own documentation already prescribes the missing variable

This is not a new requirement I am inventing — the repository's Windows QA environment doc states it, and the
pinned QA launch scripts already do it:

```
docs/evidence/windows-terminal-20260912/windows-environment.md:13
  "Set BOTH \`FERRYX_SESSION_DIR\` and \`FERRYX_RUNTIME_DIR\` to unique QA paths. Source inspection confirms
   that the session override alone does not isolate daemon endpoints. Also isolate QA application settings."

docs/evidence/windows-terminal-20260912/runtime/qa/launch-isolated.cmd:9
  set FERRYX_SESSION_DIR=%QA_ROOT%\session
docs/evidence/windows-terminal-20260912/runtime/qa/launch-fresh.cmd:5
  set FERRYX_SESSION_DIR=%QA_ROOT%\session
docs/evidence/windows-terminal-20260912/runtime/pinned-commands.md:64
  "Isolation: FERRYX_RUNTIME_DIR/FERRYX_SESSION_DIR redirects daemon + session state to the QA ..."
```

So the documented QA isolation recipe is **both** variables, and the current `buildIsolatedEnv` allowlist sets
`FERRYX_DATA_DIR` + `FERRYX_RUNTIME_DIR` but **drops `FERRYX_SESSION_DIR`** — a documented isolation
requirement that the runner does not meet. The same doc adds "**Also isolate QA application settings**", which is
the webview-profile point below.

**One prediction this makes, which the positive control below tests:** if the only problem were
`session_state.json`, then setting `FERRYX_SESSION_DIR` to an empty directory should flip the app back to the
empty state (`recoverProjectBootstrap` returns null for a session with no workspaces, `loadProjects()` falls
back to the stored project list, and with `layout.tabs.length === 0` the render branch
`activeWorktree && state.layout.tabs.length === 0` selects `EmptyWorkspaceView`). If instead the webview
profile also matters, the control will still show the restored `main` tab.

**A second isolation gap, named but not measured by me:** `loadProjects()` reads the webview's **localStorage**
(`ferryx.projects`), and the app's WebView2 profile lives at `%LOCALAPPDATA%\com.ferryx.app\EBWebView` —
outside the isolation root and not redirectable by any `FERRYX_*` variable the app reads (`WebviewWindowBuilder`
is called without `data_directory`). I did **not** measure whether that profile alone is enough to restore the
tab, so I am reporting it as a named second gap rather than a proven one.

## Measurement 3 — the controlled A/B (2 control vs 2 session-isolated)

The mechanism above is a code reading. This is the experiment that proves it, with **one variable changed**:
`buildIsolatedEnv`'s child-env allowlist, adding the product's own `FERRYX_SESSION_DIR` (pointing into the
isolation root) and nothing else. Same revision, same scenario, same binary, same probe, same instrumentation.

| run | variant | `candidateCount` | `actionableCount` | window `total` | window `named` | `exactNewTerminal` | verdict |
|---|---|---|---|---|---|---|---|
| control-run1 | as-is | **0** | 0 | 110 | 65 | **0** | `PANE_AFFORDANCE_NOT_FOUND` |
| control-run2 | as-is | **0** | 0 | 110 | 65 | **0** | `PANE_AFFORDANCE_NOT_FOUND` |
| sessioniso-run1 | + `FERRYX_SESSION_DIR` | **1** | **1** | **93** | **56** | **1** | `PANE_BINDING_UNBOUND` |
| sessioniso-run2 | + `FERRYX_SESSION_DIR` | **1** | **1** | **93** | **56** | **1** | `PANE_BINDING_UNBOUND` |

**2 of 2 control runs cannot find the affordance; 2 of 2 session-isolated runs find it, and click it:**

```
sessioniso-run1 chosen: {"name":"New Terminal","controlType":"ControlType.Button","enabled":true,
                        "offscreen":false,"rectEmpty":false,"rect":"1101,907,156,36"}
sessioniso-run2 chosen: {"name":"New Terminal","controlType":"ControlType.Button","enabled":true,
                        "offscreen":false,"rectEmpty":false,"rect":"877,683,156,36"}
```

**And the session-isolated runs reproduce pass 9 exactly.** Their verdict is
`PANE_BINDING_UNBOUND: no presentation receipt named a session other than the 1 fixture session(s) within
8000ms, so the UI pane step created no observable pane (observed sessions: [])` — **character-for-character the
same message pass 9 produced.** So the `PANE_BINDING_UNBOUND` failure mode is fully reproduced on demand, and
the difference between the runs is entirely the persisted app state.

**The tree sizes identify the two app states**, and one of them is the one already documented in the code:
- **93 elements / 56 named** — the **empty state**. This is the number the driver's own comment records
  (`native-driver.mjs`: *"the next enumeration, seconds later, returned 93 elements / 56 named including
  `New Terminal`"*). The session-isolated runs land on it exactly.
- **110 elements / 65 named** — the **restored pane** state (sidebar, `main` tab, pane chrome, setup wizard).

### The mechanism, confirmed from the outside

| observation | control runs | session-isolated runs |
|---|---|---|
| host `%APPDATA%\com.ferryx.app\dev\session_state.json` | **rewritten during every run** (mtime advanced 04:23:12 → 04:25:38 → 04:25:48) | **never touched** (mtime stayed at 04:25:48.9747346, the value control-run2 left) |

That is the whole mechanism in one line: without the override the app reads **and rewrites** the host's real
profile; with it, the host file is untouched and the app boots to the empty state.

### So the verdict for question 4 is **(C) — an app-state difference**, and the fix is in the harness

- **Not (A) a flake.** The outcome is a deterministic function of a persisted input, reproduced 2/2 and 2/2.
  There is no repetition count to size, because there is no randomness: the difference is which
  `session_state.json` the app read.
- **Not (B) a probe scope or name defect.** The probe's name condition and window scope are correct — the same
  scope, walked by the dump, contains the element in one state and not in the other. The `Tao Thread Event
  Target` window holds **0 elements**, so the walk was never looking in the wrong window.
- **(C), and the fix is one variable in `buildIsolatedEnv`'s allowlist.** Not the probe, not the product.

## Measurement 4 — is the isolated profile spawnable at all? (question 2 of the dispatch)

### 4.1 What workspace does the app believe it is in?

Two different answers, depending on whether the app's own state is isolated:

| | control runs (host profile) | isolated runs (session or profile) |
|---|---|---|
| `session_state.json` the app read | host `%APPDATA%\com.ferryx.app\dev\session_state.json` | the isolation root's `session\session_state.json` (empty) |
| `activeWorkspaceId` | **`Strawberry`**, `repoRoot: \\?\C:\Strawberry` | — (no restorable layout) |
| sidebar project shown | `Strawberry`, `System32` | **`source-21dea3c0`**, `0 sessions watched` |
| tree | 110 elements / 65 named — pane chrome | **85 elements / 52 named — the empty state** (`No open tabs`, `New Terminal`, `New Browser Tab`) |
| an open modal | `Welcome to Ferryx` setup wizard (first-run) | `Welcome to Ferryx` setup wizard (first-run) |

So the app's belief is **the staged source directory itself**: with isolation the sidebar names `source-21dea3c0`,
which is `C:\Users\sook\ferryx-pane-completion\source-21dea3c0` — the directory the runner launches the binary
from, adopted through the product's `initial_project` (`std::env::current_dir()` → `initial_project_from_path`).
In the control runs it is instead whatever the host profile last restored (`Strawberry`, whose
`repoRoot \\?\C:\Strawberry` exists on this host but is **not a git repository**).

**The setup wizard is not a project** — it is the first-run onboarding modal, present in every run including the
ones that found `New Terminal`. It does not block the empty state, so it is not part of the blocker.

### 4.2 Does the isolated daemon know a workspace, and can the harness register one?

**Yes — the runner already registers a workspace, and it works.** The `fixture-setup` receipt from
`sessioniso-run1` names the fixture session and its workspace:

```json
{"backendSessionId":"39899989-fbd3-4021-b4f6-c08f265b1898","kind":"source",
 "ownershipReceipt":{"cwd":"…\\pd12-iso\\split-happy\\sessioniso-run1\\barriers\\fixture-workspace\\",
                     "daemonEpoch":"1791141957514","fixtureKindBasis":"daemon-reports-running", …}}
```

So a workspace under the isolation root **is** registered with the isolated daemon and **is** spawned into. The
`machine-workspaces.v1.json` I reported ABSENT in pass 10 is therefore not the blocker for the fixture path — the
fixture path registers and spawns fine. The blocker is specific to **the app's own** spawn.

**There is no workspace-inventory operation on the daemon wire** (the protocol has only `RegisterWorkspace` /
`UnregisterWorkspace`; there is no `listWorkspaces`/`workspaceCatalog` request, and none in the client's
wire-name map). So the daemon cannot be asked "which workspaces do you know" — registration is only observable
indirectly, through a successful spawn or a refusal.

### 4.3 Does the click produce a session? **No — in 6 of 6 successful clicks.**

| batch | runs | affordance | daemon sessions after the click |
|---|---|---|---|
| control (incl. the validation run) | 3 | **not found** | 2 (1 → 2 at boot; flat) |
| `+FERRYX_SESSION_DIR` | 2 | **found + clicked** | **2 — unchanged** |
| `+FERRYX_SESSION_DIR` + profile env | 2 | **found + clicked** | **2 — unchanged** |
| profile env + isolation root **outside** the home git repo | 2 | **found + clicked** | **2 — unchanged** |

**Six successful clicks, zero new sessions.** So a successful click does **not** produce a pane session in this
setup, and the pass-9 `PANE_BINDING_UNBOUND` (`observed sessions: []`) is the honest report of exactly that.

### 4.4 Why — and whether the harness can fix it without a product change

**The staged tree is not its own git root, so the app's workspace root cannot satisfy the product's own
registration contract.** The chain, with each link verified:

1. The app adopts its workspace from its **working directory** (`initial_project` → `std::env::current_dir()`),
   and the runner launches it with cwd = the staged source root.
2. **That directory is inside a git repository rooted at the user's home directory.** Verified on the host:
   `source-21dea3c0\.git` **does not exist**, and `git -C …\source-21dea3c0 rev-parse --show-toplevel`
   returns **`C:/Users/sook`**, because `C:\Users\sook\.git` exists. The staged source was copied **without
   its `.git`**, so it is not a repository of its own.
3. The product's registration requires the root to be the canonical git root
   (`WorkspaceRegistry::register` → `WorktreeManager::try_new` → `if manager.repo_root() != canonical` →
   *"repo_root '…' must be the canonical repository root '…'"*, `src-tauri/src/daemon/workspace_service.rs`),
   and both `resolve_terminal_target` and `resolve_spawn_cwd` go through that manager.
4. The daemon **"refuses a spawn in a workspace it does not know"** (the fixture code's own words,
   `src-tauri/src/ipc/qa_barrier.rs`). So a failed registration is a failed spawn.

**Conclusion, stated plainly: the harness can make the profile show the affordance without a product change
(one variable), but it cannot make this staged profile spawn without either a product change or a staging
change** — because the staged source tree has no `.git` of its own and therefore sits inside the home
directory's repository. **The fix on the harness side is a staging fix, not a product change:** stage the source
**with its `.git`** so the exe's directory is its own git root, or point the app's workspace at a real git repo
root. The product behaviour that is missing (if you want the app to work from a non-git root at all) is that
`initial_project`/registration rejects a workspace whose directory is not its own git root — that is a
**product** decision, and I am reporting it, not proposing it.

**What I did not capture, and the measurement that would close it:** the app's own error for the failed spawn.
The click's outcome is only observable through the daemon session count, and I did not instrument the app's
`cmd_terminal_spawn` result. **The single measurement that closes the last link** is to run one isolated
scenario with the app's IPC result surfaced (or to read the app's own error surface for the pane after the
click) and confirm the typed failure is the registration/root refusal rather than something else. I am
reporting the chain as **four verified links plus one inferred**, not as five verified ones.

### Evidence provenance for the four profile runs (a defect in my own instrument)

I ran **two** profile batches (2 runs each): one with the isolation root inside `C:\Users\sook\…` and one
**outside** it (`C:\ferryx-qa-p11`, which `git rev-parse` confirms is *not* in any repository). My `IsoBase`
patch redirected the **iso** root but **not** the **evidence** dir, so the second batch **overwrote the first
batch's `pd12-ev` artifacts**. The consequence, stated so no number is misread:

| batch | runs | outcome | artifacts preserved |
|---|---|---|---|
| profile, iso root inside the home git repo | 2 | `PANE_BINDING_UNBOUND` (found + clicked) | **overwritten** by the next batch — outcome recorded in the progress log only |
| profile, iso root **outside** any git repo | 2 | `PANE_BINDING_UNBOUND` (found + clicked) | **yes** — `win-pass11/outside-repo/` |

So **4 profile runs succeeded at the click; 2 of them have full artifacts on disk** (the pair in
`win-pass11/profileiso-run{1,2}/` are the *outside-repo* pair, because they overwrote their namesakes). Both
batches produced the identical verdict and the identical tree shape (`85 elements / 52 named`,
`exactNewTerminal=1`), and both are on the ``PANE_BINDING_UNBOUND`` line in the progress log.

**The count that matters is unchanged and is conservative:** the A/B compares **0 / 3 control** against
**6 / 6 isolated**, and the 6 are 2 session-isolated + 4 profile-isolated runs. **The outside-repo batch proves
that moving the isolation root out of the home-directory repository does *not* by itself make the click produce
a session** — the click still succeeded and the daemon session count still stayed at 2. That was the point of
running it, and it is settled by the 2 runs whose artifacts survive.

## Flake sizing (question 4, (A))

**(A) is ruled out, and there is no rate to size.** Across every observation in this pass (the instrumentation-validation run, the 2 control runs, and the 6 isolated runs):

| outcome | control (no isolation) | isolated (`FERRYX_SESSION_DIR` or profile env) |
|---|---|---|
| affordance found | **0 / 3** | **6 / 6** |
| affordance missed | **3 / 3** | 0 / 6 |

The outcome is a **deterministic function of the persisted app state**: all 3 runs without the override missed it,
all 6 runs with it found it. No run was ambiguous, and no repeat produced a different result from its own batch.

**One more number worth recording, because it retires the laziness reading for good:** the run that *succeeded*
reported `warmElements: 16` / `warmAttempts: 2` / `warmElapsedMs: 160` — **the same `warmElements: 16` the
failing runs report.** `warmElements` is the pre-attach Chromium baseline and is therefore **not diagnostic of
anything**; it appears identically in a run that found the button and in runs that did not. Pass 9's
`warmAttempts: 1` vs `33` is likewise an effect of the element being present or absent, not a cause.

---

## Carry-forward traps (as requested)

Both of these cost a full run each and are worth recording:

1. **A Windows scheduled task does not inherit the interactive PATH.** `bun` and `node` are not resolvable
   there, so a probe launched that way dies silently with no output. Both must be addressed by absolute path
   (`C:\Program Files\nodejs\node.exe`, `C:\Users\sook\.bun\bin\bun.exe`).
2. **`.NET StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`.** Writing the
   handshake that way makes the daemon never answer and the probe block forever with no error. The fix is
   `$w.NewLine = [char]10`, which also avoids backtick escaping entirely.

A third, self-inflicted one from this pass: **the pass-10 aggregation bug repeated its class here.** Pass 10's
`maxCount: 3` came from `Measure-Object -Property count -Maximum` over an `ArrayList` of ordered hashtables;
this pass computes the maximum with an explicit loop over the samples and reports
`maxCountFromSamples`, so the number cannot drift from the sample list again.

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| staged `native-driver.mjs` (instrumented) | **restored from a pristine pre-patch copy** | `VERIFIER DUMP hits = 0`, `uiaWarmLines = 3`, `warmBaselineElements = 0` — back to `757f8414` |
| staged `diagnostic-classifier.mjs` (both variants) | **restored** | `classifierRestored=True` (SHA-256 equals the original) |
| my stuck `pane-delta1{1,2}.ps1` instances + their monitors | killed by **exact PID recorded from my own listing**, each **identity-checked** on command line + start time (21008, 13264, 17008, 9248, then 10608, 21100, 24324, 2624) | `IDENTITY_OK` printed for each; two refusals were correct |
| the leftover `serve-dist.mjs` listener on 5173 | killed by exact PID (26792) taken from the `Get-NetTCPConnection` listener listing | `PORT_5173=FREE` |
| own scheduled tasks (`ferryx-pd`, `ferryx-pf`, `ferryx-qa-*`) | none left | `ownTaskCount=0` |
| **anything matching my own artifact names** | **REPORTED, NOT KILLED** — the report-only sweep found **zero** leftovers of mine this pass | — |
| **candidate tree** `local-pane-liveness-completion-foundation` | **never edited by me** — all instrumentation went to the host's staged copy | see below |
| host free space | 15.72 GB | — |

**The identity check earned its keep twice this pass**, exactly as designed: it **refused** PID 29008 (its
executable path was the PowerShell binary, not my script) and refused the second `pwsh` wrapper, instead of
killing on a name match.

### One honest disclosure: my control runs rewrote the host's real app profile

This is a **consequence of the finding, not a side effect I chose**, and it must be stated plainly.

The app's session state for this debug build lives at
`%APPDATA%\com.ferryx.app\dev\session_state.json` on the host — **outside the isolation root**, which is
precisely the defect being reported. So each of my **4 control runs** read and rewrote it. Measured:

| | value |
|---|---|
| content I found before the batch | `activeWorkspaceId: "Strawberry"`, `repoRoot \\?\C:\Strawberry`, one terminal tab `main` |
| content after my control runs | **the same** — `Strawberry`, same `repoRoot`, same single `main` tab, same `primarySessionId` |
| what changed | only the `timestamp` field (1791141474751 → 1791141948973) |
| the 6 isolated runs | host file **untouched** (mtime frozen at 04:25:48.9747346 across all six) |

So the semantic content is unchanged and no session was destroyed; the file's timestamp moved. The referenced
`session:90b1d804-…` is the host's own stale session id from a previous launch and was already stale before I
started (the host's `C:\Strawberry` is not a git repository, so nothing could restore it live). **I did not
restore the timestamp**, because rewriting the file would be a second unrequested write to the user's profile —
I am reporting it instead. If you want it reset, say so and I will write the exact bytes I pulled.

## Trap 3 — the pass-11 first attempt (recorded because it cost the attempt)

The first control/iso batch produced **no run records at all** and hung for four minutes. Two causes, both mine:

1. **`System.Net.Sockets.TcpClient.Connect` has no timeout.** Once the app's daemon stopped answering (the
   runner had already exited), the poll loop's connect blocked indefinitely and the loop never reached its own
   `$runner.HasExited` break, so the run entry was never written. Fixed with a bounded
   `BeginConnect`/`WaitOne(1200)` plus `ReceiveTimeout`/`SendTimeout` on the stream — a probe that polls a
   process it does not own must bound every blocking call.
2. **The launcher must be killed by recorded PID, and mine was.** Both stuck instances were mine (started 04:17
   and 04:20, command lines naming my scripts); I killed them by exact PID with an identity check. Worth noting
   what that listing also showed: the host carries **many stale `powershell.exe` processes from other sessions**
   (old `test-lifecycle-*` and `cargo test` runs from 09:45–15:24, plus other sessions' `pane-delta` leftovers).
   None of those were mine to touch, and none were touched.

**Correction to a red herring:** my scripts' own `(Get-Process -Id $PID).SessionId` reads **0** because the
*polling script* runs in the SSH session 0 — that says nothing about the app. The app is launched by the runner,
which relaunches it into an interactive session; every run's own probe evidence reports
`sessionId: 1, interactive: true`. The value is recorded in the run records under `scriptSession` so it is not
mistaken for the app's again.

