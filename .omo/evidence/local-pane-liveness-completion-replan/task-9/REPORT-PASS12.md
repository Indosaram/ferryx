# REPORT — PASS 12

## Scope

Three things, from the dispatch:

1. **Restore the host profile** I rewrote, and prove it by **content hash** (the mtime necessarily moves).
2. **Capture the missing link** in the spawnability chain — the app's own error for the failed pane spawn, the
   daemon's side of the exchange — and decide whether the fix is (a) staging with `.git`, (b) the harness
   registering the workspace the app's cwd resolves to, or (c) a product change.
3. **Carry my traps forward** in the report.

No product repairs by me.

## Revision

**`757f8414`**, the revision staged on the Windows host at
`C:\Users\sook\ferryx-pane-completion\source-21dea3c0` (verified in pass 11 by markers: `uiaWarmLines`
present, `warmBaselineEntries`/`warmAttachCount` absent). `HEAD` is `04cf8c7a`; **none of these numbers are on
it**. The staged tree is unmodified by this pass — pass 12 made **no edits to any staged file**.

## 1. The host profile restore

### What I pulled, and what I found

Before my control runs, I pulled the file to `/tmp/t9p10/dev-session-state.json`. Its bytes:

| | value |
|---|---|
| path | `%APPDATA%\com.ferryx.app\dev\session_state.json` |
| size | 1963 |
| **sha256 (pulled, before my runs)** | `d07a1698111ca672e2c546fd4e62c2442f505d2f94534c4c8dd49bfe52d8b69f` |

### What my control runs actually changed — **my pass-11 disclosure was incomplete**

I said "only the `timestamp` field moved". **That was wrong, and the raw diff proves it: four leaves changed,
not one.**

| leaf | pulled (before) | after my control runs |
|---|---|---|
| `.timestamp` | 1791141474751 | 1791141948973 |
| `…terminalSessions["session:90b1d804-…"].createdAt` | 1791141474751 | 1791141948973 |
| `…terminalSessions["session:90b1d804-…"].daemonEpoch` | `1791141472258` | `1791141946422` |
| `…terminalSessions["session:90b1d804-…"].backendSessionId` | `1a1a084b-71b9-49c5-9cda-a1625e7ee860` | `8c57bd79-7f79-4020-9e1d-2b93df54c356` |

The last one matters: my run **re-pointed the restored tab at a backend session id that only existed inside my
isolated profile**. That is a real (if inert — the session is dead) mutation of the user's persisted layout, and
it is exactly the kind of thing I should have caught before writing "only the timestamp moved". The file was
1963 bytes before and after, which is why a size check missed it; **only a content hash and a leaf diff exposed
it.**

### The restore

```
BEFORE  sha256=3A3E3FB956BF248670821B98F3A8C6A6DA15C01205347B39DBBBA618AA70232C  (what my runs left)
STAGED  sha256=D07A1698111CA672E2C546FD4E62C2442F505D2F94534C4C8DD49BFE52D8B69F  == expected, verified BEFORE writing
AFTER   sha256=D07A1698111CA672E2C546FD4E62C2442F505D2F94534C4C8DD49BFE52D8B69F
        RESTORE_MATCHES_PULLED_BYTES=True     CHANGED_FROM_PRERESTORE=True
        mtime 04:25:48.9747346 -> 04:36:27.6955984   (necessarily moved, as you said)
```

The script **refused to write unless the staged bytes hashed to the expected value**, kept the pre-restore file as
`restore-session-state.json.prerestore.bak`, and verified the result after writing. **The user's real profile
now carries no trace of our runs**: the content hash equals the bytes I pulled before the control runs.

## 2. The missing link — captured

### The decisive artifact: the daemon's own startup registration log

`<iso>/data/logs/daemon.log`, from an isolated launch in **session 1** (`cap12/c3`):

```
2026-10-04T19:39:48.527814Z  INFO ferryx_lib::daemon::server: rorca daemon listening on …\cap12\c3\runtime\daemon.port
2026-10-04T19:39:48.532287Z  INFO ferryx_lib::daemon::server: Agent state ingress listening on loopback port=50795
2026-10-04T19:39:48.615267Z  INFO ferryx_lib::cli: Registered startup workspace for headless daemon
       workspace_id=source-21dea3c0  repo_root=\\?\C:\Users\sook\ferryx-pane-completion\source-21dea3c0
```

**This is the link, and it corrects my pass-11 inference in an important way.** The daemon **does** know the
staged directory: it registers it itself at startup, under `workspace_id=source-21dea3c0`, with
`repo_root` exactly the staged root. The registering code is `src-tauri/src/cli.rs` (the
"Registered startup workspace for headless daemon" `tracing::info!`), which runs
`crate::ipc::project::initial_project(&registry)` in a `spawn_blocking` task as the daemon starts.

### What that log line tells us about the git question

`initial_project` → `initial_project_from_path` → `register_canonical_project` →
`WorktreeManager::try_new`, and `try_new` does:

```rust
let (repo_root, git_backed) = match run_git(&canonical, &["rev-parse", "--show-toplevel"]) {
    Ok(top_level) => match fs::canonicalize(PathBuf::from(top_level.trim())) {
        Ok(git_root) => (git_root, true),
        Err(_) => (canonical, false),
    },
    Err(_) => (canonical, false),   // <-- plain-folder mode
};
```

So a `repo_root` equal to the staged directory is only possible if **`git rev-parse --show-toplevel` failed**
(or its output was not a canonicalizable directory) in that process's environment — otherwise the root would
have walked up to `C:\Users\sook`. The daemon log's own `repo_root` is therefore **evidence that git was
not available to that process**, and consequently:

- **The daemon registers `source-21dea3c0` as a plain-folder (non-git) workspace** — and it **spawns the
  fixtures into it successfully**, which is the spawn path the runner validates every run.
- **My pass-11 claim that the app registers `sook` and the daemon refuses it is WRONG**, because the app
  inherits that same environment and reaches the same `Err(_) => (canonical, false)` branch. The app should
  register `source-21dea3c0` too — a workspace the daemon demonstrably knows.

**So the failure is not "the workspace is unknown".** The remaining candidates are the other stages in the
app's own spawn path, which print typed codes to stderr:

```
[cmd_terminal_spawn] stage=resolve_target   failed code=…
[cmd_terminal_spawn] stage=validate_cwd     failed code=…
[cmd_terminal_spawn] stage=daemon_register  failed code=…
[cmd_terminal_spawn] stage=daemon_spawn     failed code=…
[cmd_terminal_spawn] stage=daemon_attach    failed code=…
```

### What I did NOT capture, and exactly why

**I did not obtain the app's stderr for the click.** Stated plainly so the gap is not mistaken for a result:

- I built a manual capture harness (own frontend + own isolated app in the interactive session) to read the
  app's stderr and the post-click tree directly. It launched the app correctly in **session 1** (verified: the
  app and its daemon are session-1 processes, and the daemon log was written), **but the app never rendered
  the UI for me** — after 30 s of retried attaches the tree stayed at `16 elements / 2 named` with
  `exactNewTerminal=0`, while the harness's own runs render `85–93 elements / 52–56 named` at the same stage.
  My manual launch differs from the harness's in ways I did not isolate (env surface, launch parent, timing).
- I then checked whether the **harness's own** logs already carry the app's stderr. They do **not**:
  `runner.err` is ~2 KB in every run and contains **zero** `cmd_terminal_spawn` hits; a recursive search for
  that string across every artifact directory of my runs (`pd12-logs`, `pd12-ev`, `cap12`,
  `C:\ferryx-qa-p11`) returns **0 hits**. **So the app's stage line is not being captured anywhere in the
  current harness**, which is itself a finding: the harness gives the app no stderr sink.

**The precise measurement that closes the link** is therefore one of:
- **(i)** give the app a stderr sink in the harness (the runner already owns the child process, so this is a
  one-line `stdio` change in the launch), then re-run one isolated scenario and read the
  `[cmd_terminal_spawn] stage=…` line; or
- **(ii)** my manual harness, with the UI-render difference isolated (the app rendered for the harness at the
  same stage, so the difference is in how I launched it, not in the app).

Both are mechanical. **(i) is strictly better**, because it captures the line in the *real* run rather than a
reconstruction.

### Therefore: which fix?

**I am not naming (a), (b), or (c) as settled, because the link that discriminates them is still missing** —
and the evidence I *do* have **eliminates the version of (a) I proposed in pass 11**:

- **(a) as I stated it in pass 11 is dead.** I argued the staged tree has no `.git` so it sits inside the home
  repo, so the canonical root is `C:\Users\sook` and registration refuses it. **The daemon log disproves the
  premise**: the daemon itself registered `repo_root = <staged dir>`, i.e. it did **not** resolve to the home
  repo, because git was unavailable to that process. If git were available (as it is in my SSH shell), the root
  *would* walk up — so **staging with `.git` would change the canonical root to the staged directory, which is
  the root the daemon already registers.** On this evidence (a) is at least *consistent*, but it is no longer
  *necessary* — the daemon already knows a root the app can register.
- **(b) has no work left to do** on the same evidence: if the app registers `source-21dea3c0` and the daemon
  already knows `source-21dea3c0`, there is nothing for the harness to pre-register.
- **(c) cannot be ruled out** — but nothing yet points at it either.

**So the honest answer to your question is: the discriminating evidence is the `stage=` code, and it is one
stderr sink away.** I am not going to recommend a staging change on an inferred link twice; the previous
inference was wrong, which is precisely why you asked for this.

## 2b. The positive control

### The positive control: the click DOES produce a session

Because my manual harness finally rendered the UI (the fix was passing `ui/dist` to `serve-dist.mjs` — it
`join`s the request path onto `argv[2]`, so passing `ui/` serves Vite's dev shell and the app renders blank),
I captured the case the harness has never shown: **a click that produces a session.**

```
invoke      : {"attempted":true,"attempts":1,"hwnd":22481948,"count":1,"enabled":true,
               "rect":"877,683,156,36","invoked":true,"elapsedMs":83}
sessionsAfter: count=1  session=8a84dfc6-84ed-4e2f-b729-494e69ef018f
               stable at 1 across all 14 samples (atMs 0 … 9100)
treeBefore  : total=16  named=2   exactNewTerminal=0
treeAfter   : total=100 named=59  exactNewTerminal=0
treeAfter.tabItems : ["main Close main"]
app.stderr  : (empty)
```

Three independent signals say the spawn **succeeded**:

1. **A session exists and stays**: `8a84dfc6-…` in the daemon inventory at every one of 14 samples.
2. **The UI is the pane state, not the empty state**: `TabItem|main Close main`, `Split pane right`,
   `Split pane down`, `New tab`, `Native terminal input`, and the sidebar counter reads
   **`1 session watched`** with `source-21dea3c0 primary` as the project.
3. **The app printed no failure line.** `cmd_terminal_spawn` prints
   `[cmd_terminal_spawn] stage=resolve_target|validate_cwd|daemon_register|daemon_spawn|daemon_attach failed code=…`
   to stderr on every failure path; `app.stderr` is **empty**.

**So the isolated profile IS spawnable, on this revision, with this binary, from this staged root.** That is the
missing link, and it points the answer away from both staging and the product.

### Why this does not perfectly match the harness — stated honestly

My manual run is **not a perfectly matched control**, and the differences are exactly where the remaining
question lives:

| dimension | my manual run | the harness's runs |
|---|---|---|
| isolation root | `cap12/c5` | `pd12-iso/…` |
| QA fixtures | **none** (I set no `FERRYX_QA_FIXTURE_KINDS`) | `fixture-setup` provisions them first |
| app launched by | **me**, `-WorkingDirectory <staged root>` | the runner, itself relaunched by a **scheduled task** |
| app's adopted workspace | `source-21dea3c0` (visible in the tree) | `source-21dea3c0` in the profile-isolated runs; **`System32` in the control runs** |
| click result | **1 session created** | 0 sessions, 6/6 |

**The leading candidate, with direct evidence, is the app's working directory.** The app adopts its workspace
from its **own cwd** (`initial_project` → `std::env::current_dir()`), and the runner is relaunched through a
scheduled task, whose working directory is `%SystemRoot%\System32` rather than the staged root. The evidence is
in the pass-11 **control** tree: it lists **`System32`** as a project — `Expand System32`, `System32 primary`,
`Remove project System32` — alongside the host profile's `Strawberry`. That is the app having adopted
`C:\Windows\System32` as a workspace. My manual run, launched with `-WorkingDirectory <staged root>`, adopted
`source-21dea3c0` instead — and the daemon demonstrably knows `source-21dea3c0`.

**I am flagging that as a candidate, not a conclusion**, because I could not test it: the harness deletes each
run's isolation root at the *start* of the next run, so **no harness run's `daemon.log` survived** to compare its
registration against mine. The one measurement that settles it is to log the app's cwd (and the `stage=` line)
in a harness run.

### Which fix — (a), (b), or (c)?

**On the captured evidence: none of the three as I framed them in pass 11, and specifically NOT (a).**

- **(a) staging the source with its own `.git` — withdrawn.** My pass-11 argument was that the staged tree has
  no `.git`, so it sits inside the home repo, the canonical root becomes `C:\Users\sook`, and the daemon
  refuses it. **The daemon log refutes the premise**: the daemon registered
  `repo_root = <staged dir>` — it did **not** resolve to the home repo, because `git` was unavailable to that
  process (`try_new` falls back to `(canonical, false)` when `run_git` errors). And the positive control
  shows the click spawning fine in that exact configuration. **Staging with `.git` would be a change made to fix
  a problem that the evidence now says does not exist.**
- **(b) the harness registering the app's resolved workspace — no work left to do.** The daemon already
  registers the staged root at startup, and the app registers the same root when its cwd is the staged root.
- **(c) a product change — not indicated.** The product's spawn path works; the app printed no failure line and
  produced a session.

**The answer is a harness-side fix**, and the candidate with evidence is: **the runner must give the app a
working directory the daemon knows (the staged root), and must capture the app's stderr so the
`[cmd_terminal_spawn] stage=` line is visible when it fails.** Both are in the harness's launch step, not in
the product and not in staging.

**The one measurement that closes it** (and I did not take it): re-run one isolated scenario with the app
launched with `cwd = <staged root>` and its stderr teed to a file, then read the `stage=` line and the daemon
inventory. If the session appears, the cwd is the cause and the fix is one line in `spawnOwned`.

## 3. Carry-forward traps

- **A Windows scheduled task does not inherit the interactive PATH.** `bun` and `node` are unresolvable
  there, so a probe launched that way dies silently with no output. Both must be absolute
  (`C:\Program Files\nodejs\node.exe`, `C:\Users\sook\.bun\bin\bun.exe`). **The runner hits this too**: it
  relaunches itself through `schtasks` (`DELEGATED-TO-INTERACTIVE-SESSION`) precisely because session 0 cannot
  show a window, and the task it creates inherits that PATH — which is also why the app under test may see no
  `git` on its PATH.
- **`.NET StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`.** A handshake written
  that way makes the daemon never answer and the probe block forever with no error. Fix:
  `$w.NewLine = [char]10` (which also avoids backtick escaping entirely).
- **`TcpClient.Connect` has no timeout.** A poll loop over a process you do not own must bound every blocking
  call (`BeginConnect` + `WaitOne(ms)`, plus `ReceiveTimeout`/`SendTimeout`), or the loop hangs forever once
  the daemon stops answering — which is exactly what happened on this pass's first attempt.
- **The runner's `FRONTEND_PORT_OCCUPIED` refusal is real and good.** When my own leftover `serve-dist.mjs`
  listener held 5173, the runner **refused with its typed code rather than reusing or killing the listener** —
  fail-closed behaviour, confirmed live. I then killed that listener by the **exact PID recorded from the
  `Get-NetTCPConnection` listener listing (26792)**, never by pattern.
- **A scheduled task's default working directory is `%SystemRoot%\System32`**, not the directory you launched
  from. Anything the child derives from `current_dir()` silently changes. (This is the leading candidate for the
  spawn difference above.)
- **`Start-Process -ArgumentList @()` throws** ("element of the argument collection contains a null value");
  omit the parameter for a no-argument child. This silently prevented my app from launching at all.
- **`serve-dist.mjs` takes the DIST directory.** It `join`s the request path onto `argv[2]`, so passing
  `ui/` serves Vite's dev shell and the app renders a blank page — while still returning HTTP 200 with a
  `id="root"` div, so the HTTP check passes. Pass `ui/dist`.
- **A stale `.done`/result marker makes monitors fire immediately.** Clear it at the start of the script, not
  only at the end.
- **The harness gives the app no stderr sink.** `runner.err` is ~2 KB in every run and contains **zero**
  `cmd_terminal_spawn` lines; a recursive search across every artifact directory returned 0 hits. Any future
  spawn investigation is blind without this.

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| the host profile | **restored to the pulled bytes** | `sha256 = D07A1698…69F` — **still holding after the c3/c4/c5 runs**, re-verified at teardown |
| my manual-capture app/daemon/serve processes | killed by **exact PID recorded from my own listings** | `IDENTITY_OK` on each; the app and its `--daemon` child, and the `serve-dist` listener, all down |
| my `capture12-c1` monitor shells (17948, 22668) | killed by exact PID, identity = the command line names my `capture12-c1.json` | `REMAINING_MINE=0` |
| my scheduled tasks (`ferryx-p11-capture`, `ferryx-p11-pathtest`) | deleted by name | `ownTasksLeft=0` |
| **anything matching my artifact names** | the report-only sweep found only the processes listed above — **all of which I could prove were mine**; nothing was killed on a pattern alone | — |
| the **staged tree** | **untouched by pass 12** — no staged file was edited this pass | `native-driver` instrumented-hits = 0; classifier SHA equals the original |
| **candidate tree** `local-pane-liveness-completion-foundation` | **never edited** | `HEAD = 04cf8c7a`, `git status --porcelain` empty |
| host free space | **4.17 GB** at teardown (down from 15.72 GB — my capture runs' isolation roots and the `ui/dist`-serving runs are the likely consumers; worth a sweep before the next build) | — |

**The identity check refused three times this pass** — PID 29008 (path was the PowerShell binary, not my
script), PID 25800 (empty command line), and PID 20584 on the first attempt — and each refusal was correct:
the pattern I had used was not proof. **The two `capture12-c1` shells were killed only after I re-listed them
with full command lines and confirmed they named my own monitor file.**

## Files

- `win-pass12/capture-c5.json` — **the positive control**: the click, the session, the pane tree, the empty
  stderr.
- `win-pass12/capture-c3.json` — the first manual capture (UI not yet rendered; retained because it is the run
  whose daemon log carries the startup registration).
- `win-pass12/daemon-c3.log` — **the decisive daemon line**: `Registered startup workspace for headless daemon
  workspace_id=source-21dea3c0 repo_root=\\?\C:\Users\sook\ferryx-pane-completion\source-21dea3c0`.
- `win-pass12/capture14.ps1` — the manual capture harness, including the `ui/dist` fix.


---

# PASS 12b — measured at `5c423880`

## Revision

**`5c423880`** (`fix(qa): isolate FERRYX_SESSION_DIR so the app stops restoring the host's real profile`),
staged on the Windows host at `C:\Users\sook\ferryx-pane-completion\source-21dea3c0` by extracting a tar of
the six changed files over the existing tree. **Staged-file verification:**

| file | staged sha256 | matches local `5c423880` |
|---|---|---|
| `scripts/lib/qa-scenarios/diagnostic-classifier.mjs` | `1E49EAF2…` | **yes** |
| `scripts/lib/qa-scenarios/native-driver.mjs` | `ADB71154…` | **yes** |
| `scripts/lib/qa-scenarios/pane-binding.mjs` | `A965E570…` | **yes** |
| `scripts/qa/pane-liveness.mjs` | `854EEAA7…` | **yes** |
| `scripts/qa/pane-liveness.test.mjs` | `D3AD0CF1…` | **yes** |
| `scripts/lib/qa-scenarios/common-harness.mjs` | `58664701…` | differs **only** by my verifier sink (below) |

`6 files, 0 Rust`, so the existing debug binary
(`sha256 6ad63c5a…`, `daemonVersion 2026.928.7`) is still correct. **Measurements in this section are on
`5c423880`; everything before this section is on `757f8414`.**

## The one instrumentation I added (staged copy only, never the candidate tree)

The harness pipes the app's stdio (`spawnOwned`: `stdio: ['ignore','pipe','pipe']`) but **nothing drains it**,
so the app's own `[cmd_terminal_spawn] stage=…` lines reach no artifact. I added a drain in the staged
`common-harness.mjs`, gated on an env var so it is inert otherwise:

```js
const sink = process.env.FERRYX_VERIFIER_APP_STDERR;
if (sink) { const append = chunk => { try { appendFileSync(sink, chunk); } catch {} };
            child.stdout?.on('data', append); child.stderr?.on('data', append); }
```

This is **diagnostic only** — it changes no control flow, no verdict, and no product code.

## 1. The isolation fix — CONFIRMED BY MEASUREMENT

### Both `launch.binary` evidence records carry `FERRYX_SESSION_DIR`

```
r1 -> {"FERRYX_DATA_DIR":"…\iso5c\r1\data","FERRYX_RUNTIME_DIR":"…\iso5c\r1\runtime",
       "FERRYX_SESSION_DIR":"…\iso5c\r1\session","FERRYX_QA_BARRIER_DIR":"…\iso5c\r1\barriers", …}
r2 -> {"FERRYX_DATA_DIR":"…\iso5c\r2\data","FERRYX_RUNTIME_DIR":"…\iso5c\r2\runtime",
       "FERRYX_SESSION_DIR":"…\iso5c\r2\session","FERRYX_QA_BARRIER_DIR":"…\iso5c\r2\barriers", …}
```

### The host profile stayed FROZEN

| | before the run | after the run |
|---|---|---|
| sha256 | `D07A1698111CA672E2C546FD4E62C2442F505D2F94534C4C8DD49BFE52D8B69F` | **identical** |
| mtime | `2026-10-05T04:36:27.6955984+09:00` | **identical** |

**That is the decisive confirmation of the fix in the same shape as the pass-11 control observation**: the
control runs advanced that mtime on every run; the runs at `5c423880` leave it untouched. **The host file is
frozen and no host-profile trace is written.**

**One honest gap in this measurement:** I recorded `isoSessionStateExists` **before** the run's own artifact had
settled, so my result file reports it empty for both runs. The **hash evidence above is the load-bearing part**
(an untouched host file cannot be explained by anything except the override taking effect), and the
`FERRYX_SESSION_DIR` records are the direct proof. **I am not claiming the isolated `session_state.json` file
existence from my own result file**, because that field did not settle.

## 2. The missing link — CAPTURED

### What the app tried to register and what the daemon answered

**The app's spawn did not fail, and no refusal was answered.** The app's own stderr, captured for the first time
in a harness run, contains **exactly one line per run and no failure line**:

```
r1 sink: [cmd_terminal_spawn] request received has_worktree=false has_cwd=true has_client_request_id=true
r2 sink: [cmd_terminal_spawn] request received has_worktree=false has_cwd=true has_client_request_id=true
```

`cmd_terminal_spawn` prints a typed line on **every** failure path:

| stage that would have printed | printed? |
|---|---|
| `stage=resolve_target failed code=…` | **no** |
| `stage=validate_cwd failed code=…` | **no** |
| `stage=daemon_register failed code=…` | **no** |
| `stage=daemon_spawn failed code=…` | **no** |
| `stage=daemon_attach failed code=…` | **no** |
| `stage=emit_lifecycle failed` | **no** |

**So the app's local spawn path completed end-to-end**: it resolved the target, validated the cwd, **registered
the workspace successfully**, **spawned the session successfully**, and **attached successfully**. There is no
registration refusal, and the daemon refused nothing.

### What actually fails: the receipt the harness binds on is never produced

```
PANE_BINDING_UNBOUND: no presentation receipt named a session other than the 1 fixture session(s) within 8000ms,
  so the UI pane step created no observable pane
  (receipt: "…\iso5c\r2\barriers\presentation.receipt.jsonl", receipt lines: 0,
   sessions named by those lines: [], fixture sessions excluded: ["f64c1598-…"], observed pane sessions: [])
```

**`receipt lines: 0`.** That receipt is written by the **native surface host's frame-presentation completion**
(`src-tauri/src/native_terminal/surface_host.rs:1656`, `emit_native_presentation_receipt_qa` guarded by
`pane_liveness_presentation_receipt(frame_attach_tuple, true)`). So the harness's binding is waiting on a
producer that requires the **native terminal surface to present a frame** — and that is what this run
configuration does not drive.

### Therefore, (a), (b), or (c)? **None of them.**

- **Not (b)**: nothing for the harness to pre-register — the app registered successfully itself
  (`daemon_register` printed no failure).
- **Not (c)**: no product failure was observed. The product's spawn path succeeded on every stage; the missing
  artifact is a *renderer presentation receipt*, and the harness is the party waiting for it.
- **Not (a)**: withdrawn for a second, independent reason. My pass-11 argument rested on the daemon refusing a
  registration — **and the capture shows no registration attempt ever failed**, so the premise is moot
  regardless of what the canonical root would be.

**And I must correct my pass-11 statement**, because the capture contradicts it: I wrote *"6/6 successful clicks,
zero new sessions … the daemon refuses a spawn in a workspace it does not know."* **The app's spawn does not
fail** — the IPC call passes all five stages. What was absent was the **presentation receipt**, and I inferred
"no session" from a message that is *defined* over receipt lines. **The honest statement is: the click and the
spawn both succeed; the harness cannot observe the pane because the receipt it binds on is empty.**

### What I did not measure, stated so it is not mistaken for a result

- **I did not capture the daemon inventory delta for these two runs.** My poller ran, but its serialized samples
  came out with blank fields (`atMs=`, `count=`), so I have **no measured session count** for `5c423880`. I
  therefore do **not** claim whether a session appeared; the stderr evidence says the spawn call succeeded, which
  implies one did, but **that is an inference and I am labelling it as one.**
- **I did not read the app's post-click UI tree** in these runs (my manual harness read it in pass 12's c5 —
  `TabItem|main`, `Split pane right`, `1 session watched` — but that was on `757f8414`).

**The single measurement that closes the remaining question** is to read the isolated daemon's inventory (and
the app's post-click tree) **during** a `5c423880` run, with the receipt path instrumented. That would settle
whether the pane exists but is unobserved, or whether the native surface never attaches — and both of those are
**harness-side** questions, not product ones, on everything captured here.

## Teardown (12b)

| Resource | Action | Receipt |
|---|---|---|
| my `run5c.ps1` launcher (PID 26516) | killed by **exact PID from my own listing**, identity = command line names `run5c` | `IDENTITY_OK` |
| my `run7.ps1` launcher (PID 12892) | killed by exact PID, same identity rule | `IDENTITY_OK` |
| the app processes bound to the staged binary | killed by exact PID, identity = executable path under `source-21dea3c0` | identity-checked before each |
| my scheduled tasks (`ferryx-p11-run5c`, `ferryx-p11-capture`, `ferryx-p11-pathtest`) | deleted by name | `OWN_TASKS=0` |
| port 5173 | free | `PORT_5173=FREE` |
| **the host profile** | **untouched by the `5c423880` runs** | `sha=D07A1698…`, mtime still `04:36:27.6955984` — the same values the pass-12 restore left |
| the staged tree | `5c423880` files staged by tar, plus my verifier sink in `common-harness.mjs` | recorded in the hash table above |
| the **candidate tree** | **never edited by me** | `HEAD = 5c423880`, `git status --porcelain` empty |

**The identity check again did its job**: it refused a `pwsh` wrapper whose command line was empty rather than
killing on the pattern alone.

## Files (12b)

- `win-pass12b/runner-actions-r{1,2}.jsonl` — **both `launch.binary` records carrying `FERRYX_SESSION_DIR`**.
- `win-pass12b/app-stderr-r{1,2}.log` — **the app's own stderr**: `request received` and **no failure line**.
- `win-pass12b/runner-result-r{1,2}.json` — `PANE_BINDING_UNBOUND` with `receipt lines: 0`.
- `win-pass12b/run7.ps1` — the run harness with the inventory poller and the daemon enumeration.
