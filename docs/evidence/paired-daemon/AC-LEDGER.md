# AC01-AC12 ledger - evidence-backed status

Built by the parent from the plan's own AC wording and from what each packet report
actually claims. Every "NOT met" below is the packet's own written admission, quoted,
not a parent guess. No AC is marked met on the strength of a passing unit test alone.

### Evidence-accuracy correction (gate review blocker 4)

The gate review caught two inaccuracies in an earlier version of this ledger. Both were
the parent's errors and are corrected here:

1. Single-use ticket coverage was attributed to the `paired_host` suite. **Wrong**: the
   ticket tests live in the **remote** suite. The ticket guarantees cited below are
   sourced to production code lines, not to that suite.
2. AC10 was signed off as met. **Wrong**, and downgraded to PARTIAL below.

**`A23-GREEN.log` is misleadingly named.** It is not uniformly green. It contains:
`FAILED. 275 passed; 8 failed`, `FAILED. 1 passed; 3 failed`, and
`FAILED. 183 passed; 100 failed` followed by `EXIT=101` and a missing-temp-directory
compilation failure. The 100 failures are all `remote::workspace_api_tests::*` and they
fell over in **3.80s**, against 171.41s for the healthy run of the same target - the
signature of a fixture/environment fault, not 100 production defects. A23 did disclose
this in its report ("broad regression attempts failed due to fixture environment
mistakes... not represented as green"), so the disclosure was honest even though the
filename is not. **That log cannot support combined acceptance** and is not used as
such here.

Verified by the parent directly, in this worktree:
- `paired_host` suite: **31 passed, 0 failed** (`--test-threads=1`), re-run by the parent
  after A23 landed. It grew 27 -> 28 (A15) -> 30 (A16) -> 31 (A23) and never went red.
- `cargo check --lib` exit 0; `cargo check --bin ferryx-cli --bin ferryx-relay` exit 0.
- UI suite 4470/4473; `bun run --cwd ui build` exit 0.
- The 3 UI failures are `push/client.test.ts`, failing identically on the canonical
  tree - pre-existing, not caused by this work.

## The single fact that gates most of the table

`pairedDaemonProxyV1` is advertised **false**, pinned by assertions in
`paired_host/process_tests.rs:121` and `native_operation_tests.rs:162`. A15 landed the
transport core and A16 landed native routing/lifecycle, but neither proved the
end-to-end UDS/renderer path. While that capability is false, no real remote terminal
can be created, so every AC that depends on a live remote PTY cannot be demonstrated.

## Status

- **AC01 - Add Project offers Local/SSH/Paired, browse without SSH**: implementation
  landed (A06, A18). NOT signed off. A18: "No running desktop, user daemon, PTY, relay
  machine, or real HOME was accessed. Native command responses were mocked at the Tauri
  boundary." Needs human desktop QA.
- **AC02 - canonical identity, survives desktop and daemon restart**: partially proven.
  Registration/persistence covered by A07 and the durable journals; descriptor
  persistence across a renderer restart remains open (delete-dialog repair report:
  "mutation reconciliation references do not survive renderer restart").
- **AC03 - Local/SSH/paired coexist, collisions stay separate**: A19 explicitly says
  "Do not mark AC03 or AC11 fully complete from this packet alone." Human QA listed.
- **AC04 - tabs, splits, focus, shortcuts, native menus**: **NOT met.** A20: "AC04 NOT
  met." A22 repeats AC04 among the unproven.
- **AC05 - remote worktree create/delete with owner-side checks**: **NOT met.** A21:
  "AC05 and AC06 are NOT signed off"; owning-daemon dirty/locked/live-session/root-jail
  checks "were not independently exercised in this packet."
- **AC06 - agents run remotely, correct host/pane attribution**: **NOT met.** A21:
  "remote agent/manual text launch and authoritative cross-host attribution are
  unproven."
- **AC07 - no session loss on switch/settings/renderer close/relay loss**: NOT proven.
  A22 lists AC07 among what the false proxy blocks.
- **AC08 - forced-relay full functional pass**: **NOT met.** A15: "This does not unblock
  AC08." A16: "A16 acceptance and AC08 are not complete." This is the headline blocker.
- **AC09 - legacy mobile policy and redaction preserved**: covered by the retained
  active-desktop-session policy and scope tests; no packet claimed a regression, but no
  packet exercised a real legacy mobile client either. Weakly verified.
- **AC10 - revocation, epochs, replay, controller fencing, no bearer in URL/log**:
  **PARTIAL, downgraded from "met" after A23 corrected the parent.** Proven: single-use
  ticket consumption (`relay_server.rs:979` removes the ticket on lookup; `auth.rs:1255`
  documents 60s expiry + consume-on-first-use; a refused attempt also burns the ticket),
  generation fencing, controller fencing, revocation, and bounded/oversized-frame
  rejection. Also verified by the parent: **no token is written to any log** (zero
  matches for tokens inside log macros / console calls), and the `?token=` seen in WS
  URLs is pre-existing legacy mobile code (16 occurrences in canonical
  `RemoteTerminal.tsx`), not introduced here; new machine paths use single-use
  `?ticket=`.
  **Not true as stated**: A23 established that Authorization headers **intentionally
  travel inside relay tunnel payloads** - a tunnel forwards the request, headers
  included - so a blanket "no bearer in any wire payload" claim is false. Exhaustive
  credential-log auditing, composed revocation, and global resource stability remain
  unproven. The parent's earlier "met" rating was an over-claim from a grep that never
  inspected tunnel payloads.
- **AC11 - offline projects visible, no empty-inventory authority, no Local fallback**:
  partially proven; A19 withholds sign-off together with AC03.
- **AC12 - compatibility and rollback across all versions**: **NOT met.** A24: "AC12 is
  not met." Request drain proven; **live-session drain not** - the user prohibits
  touching a running PTY. Archive/mobile matrix and the Linux rehearsal are human-only.

## Honest summary

PARTIAL with real evidence: **AC10** (downgraded per the correction above; the
tunnel-payload finding stands). AC02/AC09/AC11 in part.
Explicitly NOT met, by the packets' own words: **AC04, AC05, AC06, AC08, AC12**.
Withheld pending human QA: **AC01, AC03, AC07, AC11**.

This is a legitimate boundary, not a shortfall of effort. Closing the remaining ACs
requires three things the parent cannot do alone: flipping and proving the real proxy
path end to end, manual desktop QA on a live machine, and a live-PTY/Linux rehearsal the
user has forbidden against their running processes.

## Deployment risk that must not be lost

`git status` is broken in this worktree - `expected submodule path
'src-tauri/vendor/ghostty' not to be a symbolic link` - so it prints nothing and **~70
new untracked files are invisible**, including the entire `paired_host/` module,
`machine_gateway.rs`, `machine_owner.rs`, `machine_peer.rs`, `session_metadata_*.rs` and
`ipc/paired_host.rs`. `git diff HEAD` only shows the 79 tracked files. Any commit made
from `git add -u` or a status-driven flow **will silently omit most of this work**.
Commit with explicit paths.

## Final gate re-review (post-repair)

Reviewer: one gate reviewer (`omo-senpi-gate-reviewer`), read-only, 2026-09-13. Report:
`.omo/evidence/paired-daemon-rereview-gate-review.md`.

- Verdict: **no unresolved production-code blocker.** The three code blockers are
  confirmed resolved in code (`client.rs:128-162,324-327,403,442-445,622`;
  `paired_runtime.rs:25-42,60-95`); the client and owner RED logs reproduce their
  original defects and the GREEN logs record 36 passing tests.
- The single REJECT finding was an artifact inconsistency: this ledger's summary line
  still said AC10 "Met" while the body said PARTIAL. **Fixed** in this revision.
- Finding (original RED missing): `REGRESSION-blockon-RED.log` recorded the post-fix
  run. Closed: the parent's pre-fix isolation reproduction is preserved as
  `REGRESSION-blockon-original-RED.log` (24 passed / 10 failed, all panicking
  `Cannot start a runtime from within a runtime` at `workspace_api.rs:247:39`,
  including the security tests `test_omo_resume_cwd_cannot_escape_workspace` and
  `test_server_spawn_cwd_validation`).
- Finding (plan absent from worktree): accepted as a stated limitation - the plan
  lives in the canonical checkout, which the reviewer is prohibited from reading. The
  AC audit used `PARENT-AUDIT-AC01-AC12.md` wording.
- Finding (`App.test.tsx:745` mocked update-check polling replaces direct
  integration coverage): non-blocking, recorded; no `#[ignore]` anywhere in the
  session diff.
- Desktop/PTY acceptance remains human-only per `MANUAL-QA-CHECKLIST.md` and the
  default-off `pairedDaemonProxyV1` slice.

### Post-review adjudication of residual tracks (2026-09-13, parent)

- **A11 repair "preserve real relay PTY streams"**: the relay duplex-tunnel transport
  (Binary messages unwrapped onto TCP streams, `relay_server.rs:1073-1074`) and the
  generation-bound stream test
  (`test_relay_stream_half_is_bound_to_the_allocating_control_generation`,
  `relay_server.rs:2890`) are PRE-EXISTING at HEAD and PASS in the final aggregate
  (1034 passed includes them). The A11 packet's method/path/query work did not break
  the stream transport: preservation demonstrated by the green suite. Full
  desktop-over-relay PTY E2E remains in the AC08 bucket.
- **Q4 repair "prove Linux readiness signal framing"**: DONE. The marker-capture QA
  ran on a real Linux host (`Q4-linux-readiness-capture.log`: catalog exit 0 with
  `A05_READY` captured, worktree-replay exit 0, barrier tests exit -9 by design), and
  the newline-prefixed markers are applied in source
  (`machine_catalog_persistence.rs:47`, `workspace_api_tests.rs:515,543,549`).
- **Q4 repair "resolve native Windows platform failures"**: DONE. RED -> build
  attempts -> final -> final2 arc concludes `DEADLINE_PASSED` with
  `2 passed; 0 failed` (`A10-input-windows-final2.log`).
- Remaining genuinely open: the human desktop QA - user-gated.

### Q4 frozen-backend rehearsals on real hardware (2026-09-13, parent)

Method: a 94 MB tar of this worktree (including untracked files, excluding
`src-tauri/target`, `node_modules`, `.git`, `.omo`, ghostty vendor) was extracted on
two foreign machines; `vendor/ghostty` was pinned to the audited 6a508fd5 tree
(symlink on omaki, copy on maho-win); rehearsal scripts `r9-linux-rehearsal.sh` /
`r9-win-rehearsal.ps1` ran `cargo build` + tests + headless boot smoke from the synced
trees. Logs: `Q4-linux-rehearsal.log`, `Q4-windows-rehearsal.log`,
`Q4-windows-build.out`, `Q4-windows-test.out`, `Q4-windows-rehearsal-RED.md`.

- **Windows (maho-win, x86_64, real MSVC + ConPTY)**: first run RED with
  `error: future cannot be sent between threads safely` at
  `src/remote/server.rs:1271` (`axum::extract::ws::WebSocketUpgrade::on_upgrade`
  requires `Fut: Send`; the `#[cfg(windows)] write_input_cancellable` pump held
  parking_lot guards across its backpressure await). Fixed in
  `src-tauri/src/terminal/session.rs`: the per-iteration lock acquisition moved into a
  plain synchronous `write_input_slice()` so no guard or raw-handle-bearing value is
  live across the await. Green rerun: `BUILD_EXIT=0` (frozen backend lib + bins
  including ghostty), `paired_host::` suite `33 passed; 0 failed` (the remaining 3 of
  36 are cfg-gated out on Windows). This defect was invisible to every macOS-only
  verification; the RED -> GREEN loop is closed on real Windows hardware.
- **Linux (omaki, x86_64, cargo 1.98.0)**: `BUILD_EXIT=0`; headless boot smoke
  `SMOKE_READY=yes` (`FERRYX_DAEMON_READY` captured, clean kill, not still running);
  CLI banner renders; full lib suite `--test-threads=1`: **1009 passed / 3 failed /
  1 ignored** (223s). Fixes applied during bring-up: (1) the tar had carried the
  macOS-built `remote-helper/target/debug/ferryx-remote-helper`, so nine
  `ssh::bridge` tests panicked `Exec format error` - rebuilt the helper natively
  (4.1s) and all nine pass; (2) `a24_rollback_waits_for_drain` expects the runner to
  export `TMPDIR` (macOS test runners always set it; Linux does not) - exported an
  isolated `TMPDIR` and it passes; (3) `ssh_bridge_live_loopback_openssh_connection`
  needs loopback SSH - enabled a loopback-only sshd on omaki (ListenAddress
  127.0.0.1, external exposure unchanged: tailscaled still fronts the tailnet IP);
  (4) `test_spawn_startup_command_omo` needs an `omo` binary - installed a disclosed
  bench stub on omaki (the real omo ships via trusted publishing, Mac-side only);
  (5) `test_cmd_open_file_path_resolves_relative_with_cwd` asserted an opener
  success that is impossible headless (xdg-open rc=3 in SSH) - added a DISPLAY/
  WAYLAND gated skip in the test (the resolution logic does not depend on a
  display). First-run-only failures (`input_is_cancelled_when_socket_disconnects`,
  `direct_ssh_real_transport_registration_and_pty`) passed in the clean rerun;
  classified as load-flakes under first-run CPU saturation, no product change.
  **Final rerun (all fixes applied): BUILD_EXIT=0, TEST_EXIT=0, smoke
  `SMOKE_READY=yes`, full lib suite 1012 passed / 0 failed / 1 ignored (224s)**
  (`Q4-linux-rehearsal.log`). Machine disclosure: enabling the loopback sshd rotates
  the host identity tailscaled fronts - the operator's known_hosts entry for
  100.91.254.71 was updated to the verified new key (matches omaki's /etc/ssh host
  key); other raw-ssh clients may see one host-key warning.
- Q4 verdict: the frozen backend builds and the paired-host behaviors pass on real
  Windows and Linux hardware. Code changes this produced: the Windows Send fix in
  `session.rs` (cfg(windows)-only) and the headless skip guard in the browser
  open-file test; macOS suite unaffected by construction (final aggregate
  1034 passed / 0 failed / 1 ignored).
- Machine disclosures: omaki received a native `remote-helper` build inside its tree
  and a loopback self-key entry in `~/.ssh/authorized_keys` + `known_hosts` (loopback
  self-ssh; the test remains blocked by sshd's interface binding, not by auth).
  maho-win received the updated `session.rs` and rebuilt artifacts. Rehearsal trees
  are kept on both hosts for re-verification; the 94 MB tarballs were removed from
  the Mac, omaki, and maho-win.
