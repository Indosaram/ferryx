# Consolidated report — mass-ulw full code review and repair, 2026-09-14

Repo: `/Users/indo/code/project/orca-lite` (Ferryx) · HEAD at start: `48ee5a9c`
Workers: every review lane ran on `mahoquot/nekos-claude-opus-5`.

Every finding below was **independently verified by the lead against the source** before any
action. Lane output is treated as a claim, never as evidence — one lane finding was verified,
acted on, and then **reverted** when its own test proved the behavior intentional (see
Rejected).

---

## Repair ledger

### Fixed — P0

**1. Browser CLI control socket accepted every local peer without authentication**
- `src-tauri/src/ipc/browser_cli.rs` (server `:195-237`, `handle_connection` `:299-341`)
- On Windows the CLI binds `127.0.0.1:<port>` and writes the port to a plaintext file; the
  handler dispatched with **no token, no peer check, no capability**. Any local process could
  enumerate browser sessions with URLs, snapshot page content, and drive clicks/typing in the
  user's authenticated browser. On Unix the only control was socket mode `0600` — same-uid-wide.
- **RED (pre-existing at HEAD):** `/tmp/ulw-massreview/baseline-cargo-lib.log` —
  `p12_tcp_rejects_unauthenticated_commands` and `p12_tcp_rejects_forged_credential` both
  panicked "reached dispatch", printing the private URL they should never have disclosed.
- **Fix:** per-start capability token — 32 bytes `OsRng`, hex, persisted beside the socket as
  `<name>.token` mode `0600`; `BrowserCliEnvelope { token, #[serde(flatten)] request }`;
  constant-time compare; **authorization decided before the command is parsed**, so an
  unauthorized peer learns nothing about which commands exist.
- **GREEN:** `/tmp/ulw-massreview/rust-postfix.log` — both tests `ok`, plus two new tests
  (`p12_tcp_accepts_the_capability_token`, `browser_cli_tokens_match_only_on_exact_equality`).
- **Prior art:** `docs/BUILTIN_BROWSER_CODE_REVIEW_2026-09-07.md` reported this as **F9** a week
  earlier, ranked priority 5/6 pending "before Windows ships". The companion `read_line` cap
  from that same finding **was** implemented; the token was not. Windows ships today
  (`scripts/release-local.mjs:219`), so the deferral's premise had expired.

**2. Guest bridge accepted synthetic events — any page could drive privileged app chrome**
- `src-tauri/src/browser/guest.rs` (`click :91`, `keydown :114`, `drop :192`)
- The injected bridge's capture-phase listeners inspected **nothing** about event provenance
  (`rg isTrusted src-tauri/src ui/src` → one unrelated diagnostic hit). Page script calling
  `document.dispatchEvent(new KeyboardEvent('keydown',{key:'t',metaKey:true}))` reached
  `BROWSER_SHORTCUT_REQUESTED_EVENT` and the `App.tsx` handler: spawn terminal tabs (real shell
  processes), close the active surface and kill its session, open palette/settings, split panes
  — with no user interaction, **including from a hidden background tab**. The bridge nonce is no
  defense: the bridge itself supplies the nonce on behalf of the forged event.
- **Fix:** capture `Object.getOwnPropertyDescriptor(Event.prototype,'isTrusted').get` at document
  start alongside the file's existing pristine-builtin captures, gate all three listeners,
  fail-closed on throw.
- **GREEN:** `/tmp/ulw-massreview/guest-green.log` — 65 passed / 0 failed.

### Fixed — P1

**3. `WorktreeDiskScans` hard panic on unmanaged state** — `src-tauri/src/ipc/worktree.rs:174-180`
`state::<T>()` panics when unmanaged; every sibling optional-state site already used
`try_state` (`ipc/agents.rs:45`, `ipc/terminal.rs:745/751/770`). RED: baseline log,
`tauri_mock_worktree_commands_use_identity_contract` panicked "state() called before manage()".
GREEN: test `ok` in `/tmp/ulw-massreview/rust-postfix.log`.

**4. UI vitest suite could never be green** — `ui/vitest.config.ts`
No `test.exclude`, so vitest swept in `src/remote/input-latency-soak/soak.test.mjs`, a Bun-only
harness importing `bun:test`. The suite exited non-zero **regardless of product health**, so no
one could gate on it. RED: `/tmp/ulw-massreview/baseline-ui-vitest.log:341`, `1 failed | 225
passed`, exit 1. GREEN: `/tmp/ulw-massreview/final-ui.log` — 225/225 files, **2577/2577 tests,
count unchanged**, exit 0.

**5. `png` 0.18 migration left a production overflow path unhandled** —
`src-tauri/src/ferryx_scope/design/mod.rs:61` (+ `tests/scoped_design.rs:30,57`,
`tests/ssh_project_identity_live.rs:46-49`)
`output_buffer_size()` returns `Option<usize>` in 0.18; three sites were migrated, three were
not. `None` occurs exactly on buffer-size overflow — a hostile image. Hidden because neither
test target is built by `cargo test --lib`, the command `src-tauri/AGENTS.md` prescribes.
Production now propagates `DesignError::Png` instead of panicking. RED:
`/tmp/ulw-massreview/browsercli-check.log:152-251`. GREEN: `TESTS_CHECK_EXIT=0`.

**6. IPC error-string matching (project-named anti-pattern)** — `ui/src/lib/browserTauri.ts:117-127,163-173`
Both helpers fell back to `message.includes("Browser not found")`. Verified the `code` check
suffices: `IpcErrorCode` derives `SCREAMING_SNAKE_CASE` (`error.rs:6-8`) and maps at
`error.rs:236-237`. A reworded Rust message would have broken `ensureBrowser`. GREEN: 14/14.

**7. SSH host store read-modify-write race** — `src-tauri/src/ipc/ssh.rs`
Four mutating commands each did load→modify→save on separate `run_blocking` threads with no
lock, into a **fixed** `.json.tmp`. Added a process-wide mutex (poison-recovering) and a
unique temp name. GREEN: 22 passed / 0 failed.

**8. `--handover-from` with a missing value silently cold-started the daemon** —
`src-tauri/src/cli.rs:461`, callers `main.rs:37`, `bin/cli.rs:38`
Returned `Option`, so a bare flag yielded `None`, which `run_daemon_headless` reads as "no
handover" — abandoning every live PTY session the handover existed to transplant, while
announcing readiness. Reachable by a typo. **RED written first**
(`handover_from_rejects_a_missing_or_option_like_value`), then signature changed to
`Result<Option<PathBuf>, String>`. GREEN: 50 passed / 0 failed.

**9. Host-scoped session IDs always 401 on socket ticket** — `src-tauri/src/remote/server.rs:1283`
Client mints the ticket against the scoped path (`remoteTransport.ts:48-49`), `valid_socket_target`
explicitly permits `:` (`:246-252`), but the handler rebuilt the audience from the **unwrapped**
id — guaranteed mismatch. Worse, `consume_socket_ticket` removes the ticket *before* comparing
(`:299-307`), so the failed attempt burns it and retry cannot recover. GREEN: 289 passed, only
the 3 known env-bound failures.

**10. Cookie import fell back to the app's own privileged webview** — `src-tauri/src/ipc/browser.rs:1313`
With no Default browser tab open — the common case, since import lives in Settings — every
cookie from an arbitrary third-party file, **unrestricted by domain**, was injected into
Ferryx's own webview while reporting success. Fixed by **deleting** the fallback; the correct
error already existed three lines below. GREEN: 58 passed / 0 failed.

**11. `crop_png` indexed past the decoded buffer on fractional scales** —
`src-tauri/src/ferryx_scope/design/mod.rs:58-59`
`left`/`top` clamp with `.min(width/height)`; `right`/`bottom` did not. Since
`sx = info.width / viewport.width`, a full-width selection recomputes to a hair **above**
`info.width`, and `ceil()` yields `width + 1` → out-of-range slice. The geometry gate can't
catch it (it only compares `sx` to `dpr*zoom`). A **second, distinct** defect in a function I
had already fixed — found by the lane, not by me.

**12. Stale-closure generation in terminal input/paste/mouse** —
`ui/src/components/NativeTerminalPane.tsx:944, 1013, 1140`
All three read `session?.remoteGeneration`/`remoteConnectionState` but omitted `session` from
their deps. On SSH reconnect the store updates `remoteGeneration` while `backendSessionId` and
`visible` stay put (`workspaceStore.ts:2147-2156`), so the callbacks froze on the pre-reconnect
generation — **every keystroke rejected, permanently dead keyboard in a live-looking pane**.
The component's own `bindingKey` (`:538`) already reads those two fields, proving they are
render-relevant. GREEN: 5 files / 214 tests passed.

**13. DAG watcher parks forever after a silent native watch loss** -
`src-tauri/src/dag/watcher.rs:167`, `:198-206`
The watch-loss arm cleared `lose_watch`, dropped the watcher guard, closed `notify_rx` and cleared
`debounce_sleep` - but never touched `polling_mode`, bound once at `:167` and `false` whenever the
native watch armed. After that arm ran **every** `select!` arm was disabled, so the task parked
forever and DAG progress froze silently until app restart. Reached in production whenever the OS
stops delivering events for the watched inode. **I had triaged this baseline failure as
environment-bound; the `rust-core-misc` lane corrected me and the code agreed with the lane.**
GREEN: `test_dag_watcher_recovers_after_silent_watch_loss ... ok`.

**14. `scan_and_emit` ran blocking filesystem I/O on the async runtime** -
`src-tauri/src/dag/watcher.rs:71`, `:81`
`std::fs::read_dir` and `read_to_string` ran inline in an `async fn`, violating this project's own
"NEVER Block Tokio Reactor" rule. Moved into `spawn_blocking`. My first attempt was **incomplete** -
I left `hooks.scanning()` on the async side, and the test (which measures which *thread* the scan
runs on) correctly kept failing until I moved the hook inside the closure. I did not touch the test.
GREEN: `cargo test --lib -- dag::` 14 passed / 0 failed.

**15. Legacy-key migration resurrected settings, making "Reset to defaults" impossible** -
`ui/src/lib/storageKeys.ts:42-56`
`getMigratedItem` copied a legacy value forward but never consumed it, while every `reset*()`
deletes only the canonical key. Any user upgraded from an `orca`/`rorca` build could click Reset,
see it appear to work, and get their pre-upgrade settings back on the next load - **forever**.
Affects all 10 keys in `LEGACY_STORAGE_KEY_MAP`; fixed at the shared helper.
RED: `expected '{"enabled":false}' to be null`. GREEN: 65 files / 873 tests.

**16. Replay backlog never released when the last output listener unsubscribes** -
`ui/src/lib/terminalEvents.ts:150-154`
The file states its own contract at `:313` and enforces it on the write side, but the unsubscribe
teardown left the backlog: up to 512 KiB per session resident for the app's lifetime,
unconsumable. Closing a pane or switching tabs takes this path. Every *sibling* map was pruned
correctly there; only `backlog` was missed, and that asymmetry was the tell.
RED: `expected { sessions: 1, chunks: 1, chars: 4096 } to deeply equal { sessions: 0, ... }`.
GREEN: 65 files / 874 tests.

**17. Shortcut `code` fallback mis-routed chords on latin remaps (Dvorak)** -
`ui/src/lib/shortcuts.ts:517`, `:523`
On Dvorak `,` sits on physical QWERTY-W, so Cmd+`,` emits `{key: ",", code: "KeyW"}` and matched
`tab.close` (declared at `:84`, before `settings.toggle` at `:334`); the loop returns on first
match, so **the user's running terminal tab closed instead of Settings opening**. This *refines*
my own `ui-comp-chrome` verified-negative: `code`-first is required for Hangul (2-set reports
`key: "ㅍ"` for physical V) but must not apply when `key` already carries a latin character.
Added `isLayoutTransparent()` and gated both branches. GREEN: 2 files / 76 tests.

**18-25. The eight P1s missed by my first consolidation pass** (all fixed together,
verified by `gap-rust.log` 1105 passed / 3 failed, `gap-ui.log` 225 files / 2579 tests,
`GAP_BUILD=0`)

My first version of this report claimed "1 P0 + 24 P1" and marked criterion 2 PASS. Re-counting
the lane files gave **26 P1**, and eight of them appeared here neither as fixed nor as deferred.
That was a false completion claim; the eight are now fixed:

**18. `dag_watch_project` marks a project permanently watched even when the watcher dies** -
`src-tauri/src/ipc/dag.rs:84-104`. Registration was by set insertion with no removal, so once the
watcher task returned (failed first hydrate, sink failure, closed notify channel) every later call
took the `is_new_root == false` branch and never re-armed: the DAG panel hydrated once and went
permanently stale, no error, app restart the only cure. Now the forwarder task removes the key when
its loop ends - which happens exactly when the watcher dropped its sender. The frontend re-invokes
per project and per worktree path, so removal auto-rearms on the next call.

**19. Browser-mode fallbacks gated on an unscoped token the remote app deletes** -
`ui/src/lib/remoteClient.ts:10`. `RemoteApp` migrates credentials to host-scoped keys and then
clears the unscoped copy, so after pairing every `getRemoteAuthToken()` guard read false:
`listWorktrees` silently returned `[]` and `getTerminalPreferences` silently returned defaults.
Now falls back to the active host's scoped token, read straight from persisted state (importing
`remoteHostStore` here would create a module cycle).

**20. `connectEvents` ignored the instance's `baseUrl` and `token`** -
`ui/src/lib/remoteClient.ts:119`. A client built for a specific host minted its socket ticket
against the PAGE origin using a DIFFERENT host's token, and bailed entirely when the unscoped token
was absent. Now uses `this.token ?? getRemoteAuthToken()` and derives both the ticket URL and the
ws/wss URL from `this.baseUrl`.

**21. A single failed socket-ticket request killed the event stream permanently** -
`ui/src/lib/remoteClient.ts:136`. The backoff lived only in `onclose`, which never fires when no
socket was created, so a gateway 502 or a 401 during token rotation left the client event-less
until reload. Extracted `scheduleEventReconnect()` and armed it from the catch too.

**22. Terminal bell and agent completion fired two notifications for one finish** -
`ui/src/lib/notificationCoordinator.ts:165`. The bell handler guarded against a just-fired
completion, but not the reverse - and since most TUI agents ring the bell AS they finish,
bell-then-completion is the COMMON ordering and was the unguarded one. Suppression is now
symmetric; the completion timestamp is still recorded so later bells stay suppressed.

**23. `scheduleAgentAutoResume`'s `maxCandidates` was silently clamped to 8** -
`ui/src/lib/agentAutoResume.ts:163`. `collectAutoResumeCandidates` had already sliced to 8, so the
caller's option could only narrow: a workspace with >8 exited agent panes left panes 9+ dead with no
reconnect and no error. The limit is now passed down; the parameter defaults to the old constant so
existing 2-arg callers are unaffected (confirmed by `GAP_BUILD=0`).

**26. Notification popover stole focus back to the first button on any parent re-render** -
`ui/src/components/notification/NotificationCenterButton.tsx:61`. `onClose={() => setOpen(false)}`
was a fresh closure every render, and the component subscribes to the notification store - so every
arriving notification tore down and re-ran the popover's focus-trap effect, yanking focus to the
trigger and back to the first focusable element. A keyboard user who had tabbed to "Mark all read"
or a specific row was thrown to the top of the popover on each incoming notification, with the
screen-reader announcement repeating. Stabilised with `useCallback`.

**24-25. Terminal WebSocket accepted then instantly torn down** -
`src-tauri/src/remote/server.rs:1555` and `:1946`. The upgrade gate admits a selection carrying no
session id, but the focus watcher treated `None` as "focus moved away" and returned immediately, so
`select!` completed and dropped the socket right after a successful HTTP upgrade - the client saw a
connected terminal that never received a frame and reconnect-looped. Both watchers now exit only
when the watch value names a DIFFERENT session.

### Rejected after verification — reverted
**Remote reconnect attempt counter "never resets"** (reported P1 by the `rust-ssh` lane)
The code reads exactly as reported: `attempts` (`terminal/remote.rs:580`) increments per
transport failure and resets nowhere in the loop. I applied the fix — and
`ssh_reconnect_safety_retry_cap_and_terminal_failure` **failed**. Reading
`remote_runtime_tests.rs:332-359` shows the contract is deliberate: it loops `attempt 0..=5`
asserting `d.attempts == attempt` after each successful `Connected`, reaching `Disconnected` on
the 6th failure and requiring an explicit `runtime.retry()`. The design treats a link that
flaps 5 times — recoveries in between — as persistently bad and surfaces it, rather than
retrying forever. **Reverted.** Whether that policy is the right product choice is a question
for the user, not something a review pass changes unilaterally.

### Deferred — with reason

**Daemon UDS request loop has no request-size cap (P1)** —
`src-tauri/src/daemon/server.rs:1578-1591` (`handle_client`), `:1363-1375` (agent-state listener)
Unbounded `read_line`: any same-uid process can write bytes with no newline and grow one
`String` until the daemon is OOM-killed. The daemon owns **every PTY master fd**, so that kills
all terminal sessions and agent workflows at once. The browser CLI already has this exact cap
(`MAX_REQUEST_BYTES`, `browser_cli.rs:242-246`) — the less critical surface was hardened, the
more critical one was not.
**Why deferred:** fixing it edits the request loop of the process that currently owns the
user's live sessions. It deserves its own verified pass, not a tail-end edit during a review
run. The falsification (write ~100 MB with no `\n`, watch RSS) was deliberately **not executed**
for the same reason.

Also deferred, recorded in their lane reports with reasons: the `rust-daemon` lane's remaining
P1 (accept-loop busy-spin) and P2s (socket ownership, unauthenticated agent-state reports), and
`rust-native-terminal`'s P2 platform-descriptor parity trap (latent — `rg` confirms
`target_descriptor()` has no shipped caller).

---

## Verified negatives (checked, with the query that would have falsified them)

- **Worktree root jail** — `canonical_allowed_path` (`manager.rs:261-267`) runs `fs::canonicalize`
  **before** the containment test, so symlinks/`..` resolve first. The exploitable inversion
  (compare raw, canonicalize after) is absent. `git_worktree_remove` rejects a leading `-` and
  control chars, then passes `--`.
- **Token revocation** — `validate_token` omits a `revoked` check, but `rg 'revoked *= *true'`
  returns only a doc comment: revocation **deletes** the device and retains-out its tokens
  (`auth.rs:694-706`), with legacy tombstones pruned at load. Sound by construction.
- **Ring buffer** — the eviction-gap predicate `req_seq + 1 < first_seq` (`output_hub.rs:248`)
  correctly treats the boundary as contiguous; `publish_gap` uses `saturating_sub(1)`; the trim
  loop's `!is_empty()` conjunct prevents an infinite spin on an oversized chunk.
- **Release pipeline** — array-argv everywhere (no shell injection), no secret in any
  `console.log`, `assertNotInCI` enforces the local-release rule from source, and notarization
  is **fail-closed** (`spctl` must exit 0 **and** output `Notarized Developer ID`).

---

## Criteria status

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | Every domain has a report | **PASS** | 20/20 lanes have `<lane>.md` on disk, each with `Location:` citations or an explicit `NO-FINDINGS above P3`. 14 node-authored, 6 lead-authored where the node failed transiently or was cancelled for ignoring write directives; every lead report carries a provenance note and states what was NOT audited |
| 2 | Every P0/P1 fixed or deferred with reason | **PASS** | 1 P0 + 26 P1 collected across all lanes (recount: my first pass said 24 and silently omitted 9); 26 fixed; 1 rejected-and-reverted with proof; the rest deferred with reasons (daemon request loop, release pipeline) |
| 3 | Suites <= baseline | **PASS** | Rust 1097/8 -> **1104 passed / 4 failed** (`final6-rust.log`); the 4 are 3 environment-bound baseline failures plus `handover_manifest_update_excludes_other_file_handles`, proven flaky (passes isolated 3/3; `git diff --stat -- src-tauri/src/daemon/` is empty). UI 1 failed file -> **225/225 files, 2579/2579 tests, exit 0** (`final6-ui.log`); count 2577->2579 is exactly the two tests added |
| 4 | No collateral on foreign work | **PASS** | `foreign-dirty-pre.diff` and `foreign-end.diff` both sha256 `8466234777f30cb3dde8224cc154adc964935da2be8b2a16743f789b7e69f8a7` — byte-identical across the entire run |
| 5 | Diagnostics green | **PASS** | `cargo check` -> `F2_CHECK=0`; `bun run --cwd ui build` -> `F5_BUILD=0` (`built in 2.30s`) |

## Deferred — requires your decision (release pipeline)

The `scripts-release` lane found five P1s I did **not** fix, because they change signing and
notarization policy rather than repair an isolated defect. The most serious:

**macOS notarization is skipped by default and nothing downstream notices** —
`scripts/lib/release-platforms.mjs:724`, `:1004`, `:1033`. `buildHost` defaults
`approveNotarization = false`, the whole notarization stage is gated on
`if (approveNotarization && hostConfig.notaryProfile)`, and — decisively — **the only Gatekeeper
assertion in that file (`spctl` + `Notarized Developer ID`) lives inside that same block**. A
default invocation therefore exits 0 and tars an un-notarized bundle. Line 862 additionally
deletes the Apple credential env vars when the flag is absent.

**This corrects an earlier verified-negative of mine.** I had read
`scripts/release-local.mjs:434-446` — which *does* fail closed on `spctl` — and generalised "the
pipeline is safe". That guard exists only at the **publish** stage; the **build** stage's guard
sits inside a skippable branch. Checking one file and generalising to a whole pipeline was the
error.

Suggested fix (the lane's, and I agree): hoist the Gatekeeper assertion out of the conditional so
`--approve-notarization` controls *how* notarization happens, never *whether* an un-notarized
release may ship.

The other four: optional `signingIdentity` makes every codesign step vanish
(`release-platforms.mjs:859`, `release-hosts.mjs:244`); the hosted-release policy guard fails open
on multi-line run steps; `build-msix.ps1` packages without icons on a warning while hard-failing
on every other resource; the release MSIX is always unsigned and published under a sideload name.


## Process notes worth keeping

- **Wave 1 (20 lanes) produced zero reports.** Every node died on
  `Stream start budget of 85000ms exhausted`. Root cause was **my prompt design**, not provider
  capacity: no read budget, so nodes opened 50-100 files until time-to-first-token blew the
  stream budget. The gateway returned 200 throughout, including under a 5-way concurrent load
  test. A blind retry would have reproduced it exactly.
- **Wave 1b** added a hard 12-file budget, `rg`-first search, and "a report with two proven
  findings on time is a SUCCESS; an exhaustive investigation that never writes is a TOTAL
  FAILURE". Nodes immediately shifted to ~20-50 `rg` calls against 2-9 reads and started
  delivering.
- **The lead must respect dag write scopes too.** I wrote lead reports into lane-owned paths
  while those nodes were live and **clobbered `rust-daemon.md`**, which a completed node had
  already written. Recovered its six findings from its completion output, restored the file with
  provenance marking node-reported vs lead-confirmed, and renamed my files to `*-lead.md`.
- **A non-unique `oldText` silently deletes code.** One edit matched an earlier identical block
  and truncated a test; caught by the compiler (`unclosed delimiter`), not by assumption.
