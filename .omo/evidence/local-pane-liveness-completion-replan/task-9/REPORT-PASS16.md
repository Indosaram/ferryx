# REPORT — PASS 16

## Scope and outcome

The dispatch proposed a specific, documented fix — redirect the child's stdio to files so it gets valid handles
instead of the broken inherited ones — with a one-minute gate first. **I tested it. It is REFUTED: the redirect
does not fix anything.**

**And testing it exposed that my own pass-15 evidence was INVALID.** The pass-15 "hangs" were produced by a bug
in **my** test harness, not by the host. I retract the pass-15 root cause below.

**No measurement run was taken** — the gate failed, and the reason it failed is that the gate was testing a
phenomenon that was never real.

## Revision

**`HEAD = bc078f70`**, tree clean. **No measurement was taken this pass.** Pass-13 source readings were on
`562144b3`; the c5 capture on `757f8414`; the isolation A/B on `5c423880`.

---

## 1. THE REFUTATION, and the retraction it forces

### What the dispatch's recipe predicted, and what happened

The documented mechanism (2026-09-19, this host): a `.NET Process` child launched with `CreateNoWindow=true`
and no redirects inherits **broken stdio handles** and blocks. The proposed fix: redirect stdio so the child gets
**valid file-backed handles**.

**Corrected measurement** (PowerShell 7.6.6, `ArgumentList` so the command actually arrives — see §2):

| form | redirects | state | exit | ms |
|---|---|---|---|---|
| `in_and_out` | stdin + stdout + stderr | **COMPLETED** | 0 | 46 |
| `in_only` | stdin | **COMPLETED** | 0 | 23 |
| `out_only` | stdout + stderr | **COMPLETED** | 0 | 24 |
| **`neither`** | **NONE** | **COMPLETED** | **0** | **34** |

**The `neither` row is the refutation.** With **no redirects at all**, a `.NET Process` child with
`CreateNoWindow=true` **completes in 34 ms**. If the documented broken-handle mechanism were active, that row
is exactly the one that must hang. It does not.

**So the redirect is not the fix, because there was nothing for it to fix.** The mechanism does not reproduce on
this host in this session.

### RETRACTION of the pass-15 root cause

Pass 15 concluded: *"what hangs is launching WITHOUT an inherited console… proven by the four-form matrix where
the app binary hangs identically to `cmd.exe` under `CreateNoWindow`."*

**That conclusion is withdrawn.** The matrix it rested on was produced by a bug in my harness (§2): the
`Arguments` string never reached the child, so `cmd.exe` started an **interactive shell** and waited for input
— an artifact of my bug, not a host behaviour. **The pass-15 "hangs" were my measurement error.**

I also withdraw the pass-15 claim that this explained the runner's stall: with the mechanism refuted, **the
stall is still unexplained**, and I am not substituting a new guess for the old one.

## 2. How my harness was wrong — the trap that produced a false root cause

The proof is in the output itself. My pass-15 `cmd_stdin+out+err` run printed:

```
Microsoft Windows [Version 10.0.26200.9457]
(c) Microsoft Corporation. All rights reserved.

sook@DESKTOP-1LAPJMP C:\Users\sook>
```

**That is `cmd.exe`'s interactive banner and a live prompt** — it had **not** received `/c echo …` at all. It
was sitting at an interactive shell reading stdin, which is why it never exited.

The cause: I set `$psi.Arguments` from a **quoted string** and read it back inside a nested scriptblock. Under
**pwsh 7.6.6** the reliable form is `$psi.ArgumentList.Add(...)`. With `ArgumentList`, **every form completes**
(§1). **The variable I thought I was testing (redirect vs no-redirect) was never the variable that changed
behaviour — the argument delivery was.**

**Trap (new):** when a process-launch test hangs, **check whether the child actually received its arguments**
before concluding anything about handles or consoles. A child showing its interactive banner is a **caller bug**,
not a host fault. In PowerShell, prefer `ProcessStartInfo.ArgumentList` over a hand-quoted `Arguments` string.

## 3. The scheduled task: still hangs, and the redirect does NOT help

| form | result |
|---|---|
| `.bat` with `echo TASK_OK > file` (**self-redirect**) | `Status=Running`, `LastResult=267009`, `outputExists=False` |
| `.bat` with `echo TASK_OK > file < NUL` (**stdin redirected too**) | `Status=Running`, `LastResult=267009`, `outputExists=False` |

**Both hang.** So for the **task** path the redirect does not rescue it either — the task's `cmd.exe` is spawned
(it exists as a real process, §4) and then never finishes.

**Stated plainly: I do not have a fix for the task hang.** The redirect was the candidate and it is refuted; I am
not proposing another mechanism without evidence.

## 4. What I did establish about the task hang

### 4.1 The runner's OWN launch mechanism works

**Node's `spawn` with pipe stdio — exactly what the runner uses (`stdio: ['ignore','pipe','pipe']`) — works:**

```
node_cmd_echo    COMPLETED  code=0  15 ms   out="NODE_OK"
node_cmd_file    COMPLETED  code=0  14 ms   (wrote its file)
```

So the mechanism the runner uses to launch the app is **not** itself broken.

**One data point I dismissed, and then checked rather than assumed:** `ferryx.exe --version` hung under the same
Node spawn. That is **not a valid diagnostic** — `cli.rs` accepts `--daemon`, `--handover-from`, `--errors`,
`--clear`, `--all`, `--json`, and **no `--version`**, so an unknown flag falls through to booting the **full
GUI**, which then waits for a desktop. I verified the flag list before using it to dismiss the observation.

### 4.2 The scheduler does spawn the task's process

I confirmed a live
`cmd.exe /c ""…\gate16d\task.bat""` whose **parent is `svchost.exe` (the Schedule service)**. So the failure
is not "the task never starts"; it starts and never completes.

**One observation I could not turn into a finding:** this host has **71 `conhost.exe` processes, essentially all
in session 0**, including many whose parent is already gone, and only **one** in session 1
(`parent=maho-host.exe`). Console-host exhaustion in session 0 is a *plausible* contributor, and my session-1
probe (`& cmd.exe /c echo`) does work — but **I did not test it**, so I am reporting it as an untested lead, not
a cause.

## 5. What this means for the pass-13 plan

The plan stays blocked, and the blocker is now **less well understood than it was in pass 14** — because the pass-15
explanation that made it feel explained has been withdrawn. Concretely:

- the four-case receipt discrimination, native-vs-DOM, and the inventory delta all still need a run;
- **`settledBy` versus my independent probe is still unmeasured**;
- the runner still stalls after `windows-interactive-relaunch`, and I have **no verified cause** for that.

## 6. What I would do next (for the decision, not as a recommendation I can prove)

1. **Test whether the hang is session-0 console-related**: run the same trivial task while watching for a
   session-0 `conhost` being created for it, and compare with a task whose action is a **GUI/subsystem-2** binary
   (which needs no console). That discriminates "console allocation in session 0 fails" from "process creation
   fails", and it is cheap.
2. **Try the task action with an explicit `cmd /c ... < NUL > file 2>&1` wrapper written by the *runner*** rather
   than by me — the runner's own relaunch body is the thing that must work, and my hand-built `.bat` may not
   reproduce its shape faithfully.
3. Only after those: any heavier action.

**I am not asking for a reboot**, and I did not attempt elevation.

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| my 5 probe tasks (`ferryx-p16-gate`, `-a`, `-e`, …) | ended + deleted by name | `ownTasksLeft=0` |
| my 7 stuck probe processes (2 pwsh wrappers, 2 powershell, 1 node, **2 task `cmd.exe`**) | killed by **exact PID**, each **identity-checked** against `ferryx-pane-completion\gate16*` / `node-spawn-test` | `IDENTITY_OK` ×7 |
| my probe dirs and files (`gate16`…`gate16f`, `node-spawn-test.mjs`) | removed from my own scratch | 6 dirs removed |
| **FOREIGN processes** (other sessions' `minio`, `cargo test`, `bun test`, 71 conhosts) | **reported, not killed** | — |
| the Schedule service | not modified | `ProcessId=1264` |
| the host profile | untouched | `sha=D07A1698…` |
| the **candidate tree** | never edited by me; clean | `HEAD = bc078f70`, `git status --porcelain` empty |
| final | — | `REMAINING_MINE=0`, `PORT_5173=FREE`, `FREE_GB=16.19` |

**The identity check did its job again**: the kill loop required `gate16*` in the command line, which is why the
foreign `cmd.exe` processes were left alone.

## Carry-forward traps (all thirteen, plus the new one)

1. **A Windows scheduled task does not inherit the interactive PATH** — bare `bun`/`node` die silently.
2. **`StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`** — the handshake blocks
   forever otherwise. Use `$w.NewLine = [char]10`.
3. **`TcpClient.Connect` has no timeout** — bound every blocking call in a poll loop.
4. **`Start-Process -ArgumentList @()` throws** — omit the parameter for a no-argument child.
5. **`serve-dist.mjs` takes the DIST dir** — passing `ui/` serves Vite's dev shell and the app renders blank
   while the HTTP check still passes. Pass `ui/dist`.
6. **A scheduled task's default cwd is `%SystemRoot%\System32`**, not where you launched from.
7. **A stale `.done` marker fires monitors immediately** — clear it at script start.
8. **The runner's typed `FRONTEND_PORT_OCCUPIED` refusal is real and fail-closed.**
9. **The harness gives the app no stderr sink** — fixed in the candidate by `562144b3`.
10. **A wedged task subsystem blocks every native run** (the runner requires session-1 delegation) — check a
    trivial task's output before believing a stalled run is a code defect.
11. **`isAdmin=True` from `WindowsPrincipal.IsInRole` is a false positive over SSH** — the token lists
    Administrators while `OpenService(SERVICE_STOP)` is denied. **Prove access by the operation.**
12. **The service DACL, not the group claim, decides a restart**; and **`Start-Service` after a failed stop is a
    no-op, not a restart** — check the host process's PID and start time.
13. **Isolate a process-launch fault with a four-form matrix** — and **before calling a long-lived process a
    hang, check its children** (a `cmd.exe` whose child is `minio.exe` or `rustup.exe` is working).

**New trap 14 (this pass) — the one that produced a false root cause:** **when a launched process hangs, verify
it actually RECEIVED its arguments before blaming the host.** A child that prints its interactive banner
(`cmd.exe`'s "Microsoft Windows [Version …]" + prompt) never got the command — that is a **caller bug**. In
PowerShell use `ProcessStartInfo.ArgumentList`, not a hand-quoted `Arguments` string. **A false root cause is
worse than no root cause: mine survived a whole pass and a dispatch built on it.**
