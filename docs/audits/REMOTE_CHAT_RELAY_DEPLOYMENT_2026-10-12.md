# Remote chat relay deployment mismatch

Status: relay executable and UI replaced on 2026-10-12 with user approval.
The public relay no longer refuses reference-chat routes, and an isolated
relay-to-daemon-to-shell send succeeded. See "Deployment and post-deployment
verification" at the end; earlier sections are the historical record.

## Measured behavior

On 2026-10-12, identical authenticated reference-chat requests were compared
against the running local gateway and its public relay host route. Credentials
are intentionally omitted. No message was submitted to a live agent session.

| Request | Local gateway | Public relay host route |
| --- | --- | --- |
| GET capabilities | 200, reference owner and epoch present | 200, same reference owner and epoch |
| GET reference-chat prompt with the same target query | 200, prompt response | 403, empty response body |
| POST reference-chat submit with body `{}` | 400, INVALID_REQUEST for missing requestId | 403, empty response body |

The invalid submit cannot name a target or type input. The differential response
establishes a relay-path compatibility failure; it does not establish which
failure the user's existing browser encountered.

The public site served RemoteApp-Cy36Guel.js, whose chat callback writes text
with a trailing LF to a terminal WebSocket. Its close handler clears the socket
without scheduling a reconnect. The local gateway served
RemoteApp-zG0Wmg5p.js, which uses the current reference-chat mutation path.
These are inspected deployment artifacts, not evidence of a particular socket
closure in the user's browser.

The relay service runs /home/indo/bin/ferryx-relay, whose file timestamp was
2026-10-05 00:34:01 +0900. Its configured static UI directory is
/home/indo/ferryx-relay-web/dist. A timestamp does not identify a source revision.

## Current source and executed verification

Current source admits reference-chat reads and mutations in
src-tauri/src/remote/relay_server.rs:1947-1949. Its read-query contract is
declared at :2013 and applied at :2039-2040. Existing regression coverage is
present at :5652 and :5672; no new test is needed for an already-covered source
contract merely to repair its deployment.

- The focused RemoteApp reference-chat, MobileChatComposer, and chat-frame
  Vitest selection passed: 51 tests, exit 0.
- `bun run --cwd ui build` passed, exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests:: -- --test-threads=1`
  passed: 90 tests, exit 0, including reference-chat query admission and
  refusal contracts.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::reference_chat::route_tests:: -- --test-threads=1`
  passed: 9 tests, exit 0. The fixture subscribes before sending and observes
  real PTY output markers; it does not type into the user's daemon.
- The freshly built UI was rendered with isolated synthetic API responses at
  390x844 and 1280x800. Clicking Send in the mobile browser produced a POST
  submit with the fixture owner, epoch, original session ID, registryId omo,
  and the exact entered message; an accepted message bubble appeared.
  This validates browser-to-API behavior, not live provider delivery.

## Remaining work

Obtain approval to update the relay executable and static UI together.
Updating only the UI leaves the
measured reference-chat 403 unresolved. After deployment, repeat the read-only
comparison and verify sending through an isolated session and relay fixture.

Do not restart the user's Mac gateway or daemon. Do not send a test message to
an existing live agent. Do not claim production sending is repaired before the
deployment and relay-path verification complete.

## Completion audit

| Requested result | Evidence | Verdict |
| --- | --- | --- |
| Establish a reproducing failure | Identical authenticated local/relay reads and invalid submits return 200/403 and 400/403 respectively | Relay compatibility failure reproduced; original browser symptom not reproduced |
| Make the smallest repair | Current source already admits the rejected routes; serving artifacts differ | Deployment repair required, not yet applied |
| Regression coverage | 51 frontend, 9 real-PTY route, and 90 relay tests passed | Current source covered |
| Observable isolated behavior | Fresh build mobile Send produced the correct target and text; accepted bubble rendered | Browser-to-API proven; production provider delivery unverified |
| Protect the live Mac | No desktop actions, daemon restarts, or valid live submits were performed | Satisfied |
| Report fixed and unverified precisely | This report distinguishes artifact inspection, measured refusals, fixture results, and pending repair | Investigation recorded; not a completion claim |

The objective is not achieved: production deployment and post-deployment
relay sending verification remain open. The local debug relay executable
build passed with exit 0. An isolated executable was launched with synthetic
credentials and a separate account data directory. Both reference-chat prompt
and submit requests returned 404 for the deliberately unregistered fixture
machine, rather than the deployed relay's 403 route refusal. This demonstrates
route admission only, not forwarding or provider delivery. The owned process
was stopped through its recorded session handle bash_27. This macOS artifact
cannot be installed on Linux as a substitute for a same-source Linux build.

The local Linux candidate build was attempted with cargo-zigbuild targeting
x86_64-unknown-linux-gnu. The first run was stopped through its owned handle
bash_30 after compiler-cache workers were observed waiting. The replacement
run used `RUSTC_WRAPPER=` and four build jobs under monitor
mon_RKBMSFYYF1Q85EW5 (bash_32). It exited 101: glib-sys 0.18.1,
gio-sys 0.18.1, and gobject-sys 0.18.0 each reported
`pkg-config has not been configured to support cross-compilation.`
No Linux executable was produced. A real Linux sysroot with the required
development libraries, or an authorized native Linux build, is required;
pkg-config stubs used for cargo check cannot provide a deployable binary.
Production replacement remains approval-gated independently of this build
prerequisite. No production service restart or asset replacement occurred.

Native Linux preparation confirmed that omarchy has GLib, GIO, GObject,
GTK 3, WebKitGTK 4.1, ALSA, and Rust installed. The frozen candidate revision
is da05a01ed3e242268524f96dcfdc6ea6ecb5b57b. GitHub could not supply that
revision (`upload-pack: not our ref`), so it was transferred directly to
the host's canonical clone at /home/indo/code/ferryx. Native compilation
is pending under monitor mon_8PCZZ2F9F6176DGA, using the detached worktree
/home/indo/code/ferryx-wt/chat-relay-20261012 and shared cargo target
/home/indo/code/ferryx-wt/.target. This is candidate preparation only;
the production system service still runs /home/indo/bin/ferryx-relay.
The owned worktree must be removed after candidate verification or failure.

## Linux candidate result

The initial native build failed because `../ui/dist` was absent. UI assets
copied from the shared Mac workspace had changed since earlier verification,
so the isolated frozen-revision checkout rebuilt its own UI instead. That
build passed in 5.19 seconds. The no-default-features build then failed with
eight unavailable native-terminal symbol errors. Building with repository
default features under mon_SXXQ7EQGPY10VSR5 passed, exit 0, in 2m23s.

The resulting x86-64 Linux ELF has SHA-256
`6fe1f08dc110e068611ac9111105f9f8f0e19f375dadfa99c8a63171673d90ee`.
An isolated instance on port 47879 forwarded a POST to
`/api/v1/reference-chat/omo/submit` with the exact supplied JSON body once
and returned HTTP 200 from a synthetic upstream. An initial probe omitted
the `omo` path segment and correctly received 403; it was a probe error.
This result proves candidate tunnel forwarding, not real provider delivery.
The dynamic path segment is a session ID, not a provider ID:
`referenceChatRoute(sessionId, suffix)` in
`ui/src/remote/chat/referenceTypes.ts:475` constructs that route. The fixture
used the arbitrary session ID `omo`; no real OMO session was addressed.

Candidate binary and UI assets are preserved under
`/home/indo/code/ferryx-wt/.evidence/chat-relay-20261012/`.
The clean owned worktree was removed; remote worktree listing now contains
only `/home/indo/code/ferryx` at 0f578e6e on main.
Stopping the SSH monitor did not terminate the remote fixture process:
PID 700881 remained at `/home/indo/code/ferryx-wt/.target/debug/ferryx-relay`
with port 47879. Its PID was not recorded at spawn, so no pattern-based
termination was performed. This leftover is reported for user-directed
cleanup. Production relay PID 3409888 at `/home/indo/bin/ferryx-relay`
was not modified. Production deployment remains unapproved and unapplied.

A later read-only cleanup census on 2026-10-12 ran
`ps -p 700881 -o pid=,ppid=,comm=,args=` and
`git -C /home/indo/code/ferryx worktree list` through SSH. The command exited
0 and returned only `/home/indo/code/ferryx 0f578e6e [main]`; the previously
reported PID had no process row. No termination was performed. This settles
the recorded leftover PID observation, not production deployment or sending.

## Raw receipt index

The following receipts were recovered from the original session record without
rerunning builds, tests, or live requests. The local JSONL record is
`/Users/indo/.omo/agent/sessions/--Volumes-T9-Mac-project-ferryx--/2026-10-12T01-23-35-674Z_01a12db5-407a-7644-84e2-467b2d469998.jsonl`.
Line numbers identify historical evidence, not validation of a later revision.

| Record line | Observed receipt | Scope |
| --- | --- | --- |
| 55 | 51 frontend tests passed; monitor exit 0 | Three selected frontend test files |
| 138 | UI built in 7.30s; monitor exit 0 | Original local UI build |
| 219 | Prompt returned local 200 and public relay 403 | Read-only compatibility reproduction |
| 232 | Empty submit returned local 400 INVALID_REQUEST and public relay 403 | Invalid envelope; no live input |
| 241 | 9 route tests passed, 0 failed; monitor exit 0 | Isolated real-PTY route fixtures |
| 251 | 90 relay tests passed, 0 failed; monitor exit 0 | Relay regression selection |
| 321 | macOS relay dev build finished; monitor exit 0 | Local executable; not Linux deployment |
| 719 | Linux relay dev build finished in 2m23s; monitor exit 0 | Frozen Linux candidate |
| 721 | ELF SHA-256 matches the candidate hash above | Candidate artifact identity |
| 738 | status 200, forwarded 1, exactBody true | Synthetic upstream forwarding only |

The deployment approval question timed out without an answer. No operational
replacement was performed. Approval and post-deployment isolated sending
verification remain required; neither these historical receipts nor the
question timeout establish production recovery.

## Additional integrated regression (pending execution result)

The test-only source addition
`a_reference_chat_submit_through_the_relay_reaches_the_real_pane` in
`src-tauri/src/remote/reference_chat/route_tests.rs` connects the production
relay router and RelayClient to the existing isolated gateway/PTY fixture.
It uses an ephemeral loopback listener, synthetic signing keys and enrollment
token, and a temporary relay key store. Pairing registration acknowledgment
is the readiness barrier. The output subscription precedes submission; a
split output marker distinguishes executed shell output from input echo.
The relay and client tasks are owned by a JoinSet and aborted and joined
before the gateway fixture is stopped, including on an assertion panic.

This addition closes a coverage gap between the earlier separate relay and
PTY checks; it is not a production deployment repair or provider test.
Execution was started with
`cargo test --manifest-path src-tauri/Cargo.toml --lib remote::reference_chat::route_tests:: -- --test-threads=1`
under monitor `mon_A42GX96WB6GZ9W6S` (session `bash_56`). This attempt timed
out at the monitor's default 300-second deadline with exit code 1 before a
test result was emitted. Its recorded Cargo/rustc processes were absent
after termination. This is an incomplete gate, not a measured test failure.
The same command was restarted under `mon_Y1A5B241NVV1QEM0` (session
`bash_61`) with `timeout_ms: 1800000`. It reported build-directory lock
contention; no other session's compiler or lock owner was stopped. The
second attempt was explicitly stopped after source inspection found an
unsafe auth-store construction path; its monitor receipt is `watcher killed
(exit code 1)`. Neither attempt produced a passing integrated result.

SOURCE finding: `RelayClient::with_identity` calls `with_gateway`, which
constructs `PairingCoordinator::new`; that constructor resolves
`AuthManager::with_persistence(canonical_auth_path())` before a later
`with_auth_manager` replaces it (`relay_client.rs:64-75`, `:342-355`,
`:366-374`). Actual user-file access or mutation was not measured.
The fixture now uses a `#[cfg(test)]` construction seam,
`RelayClient::with_identity_and_auth`, which uses the existing
`PairingCoordinator::new_with_auth` without constructing the canonical
persistent manager. This is a test-only isolation repair within the requested
regression scope; the earlier additional approval requirement was an incorrect
scope assessment. Production constructors are unchanged. LSP reported no
errors for the seam and `git diff --check` passed. The route test selection
was restarted under `mon_GJMZNYHAY89CN0T0` (`bash_66`) with a 1800000ms budget;
it completed with exit 0. Compilation finished in 3m40s. The selection ran
10 tests: 10 passed, 0 failed, 0 ignored, 3286 filtered out, in 3.97s.
`a_reference_chat_submit_through_the_relay_reaches_the_real_pane` passed,
establishing authenticated submit through the production relay/client and
isolated gateway to executed real-shell output, not merely an accepted HTTP
response or echoed input. This does not establish deployed service recovery.
An initial LSP check reported no errors; a subsequent check timed out after
3000ms. `git diff --check` passed. Whole-file rustfmt checking reported
existing formatting differences as well as an added import ordering issue;
only the new import ordering was repaired. The later executable test result
above covers the added integration case despite the LSP freshness timeout.

## Objective-to-evidence completion audit

| Required outcome | Concrete artifact or evidence | Remaining condition |
| --- | --- | --- |
| Establish reproducing evidence | Local/public differential prompt and invalid-submit responses in the measured behavior table and record lines 219/232 | The user's original browser failure is not reproduced; do not equate the route failure with that exact symptom |
| Apply the minimal repair | Frozen Linux relay executable and matching current UI candidate described above | Operational replacement is not authorized or performed |
| Preserve regression coverage | Existing relay contracts plus the added real-relay-to-real-PTY submit test using construction-time auth injection; current selection 10/10, exit 0 | Covered for the local tested source; not production deployment |
| Run relevant checks | Historical selected frontend 51/51 and relay 90/90 receipts; new route selection 10/10 under mon_GJMZNYHAY89CN0T0, exit 0 | Historical frontend/relay counts retain their original revision scope |
| Observe isolated sending | Browser-to-synthetic-API capture, Linux candidate synthetic forwarding, and executed local real-relay-to-real-PTY marker test | Post-replacement isolated path verification remains open |
| Report fixed and unverified precisely | This audit separates source, historical execution, pending execution, and deployment status | Production recovery must not be claimed before replacement and verification |
| Preserve the user's live Mac desktop and daemon | New test uses temporary state and loopback listeners only | No live desktop selection, daemon replacement, or live-agent input is part of this regression |

The objective is not achieved while operational repair and the required
verification remain open. Unit or integration green status alone does not
establish the deployed browser's recovery.

## Preserved deployment fingerprints

A read-only SSH check confirmed the preserved candidate executable still has
SHA-256 `6fe1f08dc110e068611ac9111105f9f8f0e19f375dadfa99c8a63171673d90ee`.
Its same-checkout UI has `assets/RemoteApp-BJSYdCio.js` with SHA-256
`ac0555103f91fee66e7b19a7bcd6004035da89b0804c071582557049f1102d24`, and
`index.html` has SHA-256
`0baf65e7774a25edce7babda9ef27c80a5270b2909ca1e7ac12298d967cd7914`.
These fingerprints identify the candidate for post-install verification;
they do not assert that it has been installed. Production remains at the
previous executable and UI pending deployment approval.

## Deployment and post-deployment verification (2026-10-12)

The user authorized replacement. Before replacement the service was active as
PID 3409888 with `ExecStart=/home/indo/bin/ferryx-relay --port 8787`, and the
candidate hashes matched. Rollback copies of the previous executable and UI are
at `/home/indo/code/ferryx-wt/.evidence/chat-relay-20261012/rollback-20261012T151102`.
`/home/indo/bin/ferryx-relay` and `/home/indo/ferryx-relay-web/dist` were
replaced and `ferryx-relay.service` restarted; it is active as MainPID 1135903
and the installed executable hash is `6fe1f08d...0ee` (the candidate).

Measured after replacement:

| Request | Before | After |
| --- | --- | --- |
| Public relay POST reference-chat submit with body `{}` | 403, empty body | 400 INVALID_REQUEST, identical to the local gateway |
| Public relay `/` index.html SHA-256 | previous bundle | `0baf65e7...` (candidate) |

A public prompt read with a cached owner returned 410 TARGET_EXPIRED because
the owning gateway's owner changed; a fresh capabilities read returned 200 but
no active terminal, so no live session was read or written.

### Process-level isolated send (real relay, real daemon, real shell)

Script: `.omo/scratch/chat-relay-e2e.mjs` (copied to
`/home/indo/code/ferryx-wt/.evidence/chat-relay-20261012/e2e-run-1/`, with
`result.log` and `steps.log`). Monitor `mon_2EB4R1K66WFYFRV2`, exit 0.
On omarchy, under a throwaway root on loopback:

- The deployed relay executable (SHA-256 `6fe1f08d...0ee`) started with no
  machine tokens, so the daemon authenticated with its own Ed25519 identity and
  was addressed by machine id, as on the public relay.
- An isolated `ferryx --daemon` (release build 2026.1011.1 at a86df627; the
  daemon, remote, and reference-chat sources are unchanged between a86df627 and
  da05a01e) ran in relay mode; its control channel connected.
- A PIN from the daemon was exchanged through the relay (200); capabilities
  through the relay returned 200 with owner, epoch, and host id `local`.
- A real bash pane was spawned and its output subscribed before sending.
- POST `/host/<machineId>/api/v1/reference-chat/<sessionId>/submit` with the
  exact body `sendChatMessage` posts returned 200 `stage: accepted` in 621 ms,
  and the shell printed `CHATE2E_8662eb38_1209399` 626 ms after the send; the
  typed line never contains that string, so this is executed output.
- The same send naming a different owner returned 410 TARGET_EXPIRED.
- The relay served index.html with the candidate SHA-256.

Teardown: the daemon exited 0 on in-band shutdown; the relay received SIGTERM
after its `/proc/<pid>/exe` matched the launched path. The shell PID no longer
exists. Production MainPID 1135903 and its executable hash were unchanged
afterwards. The omarchy worktree `chat-e2e-20261012` was removed; the worktree
list contains only `/home/indo/code/ferryx 0f578e6e [main]`.

### Still unverified

- The user's own phone flow against the live Mac gateway; no message was sent
  to the user's live sessions.
- Delivery into a real agent TUI (only bash was driven).
- A browser tab opened before the owner change: its first send fails with
  "send failed" and the text returns to the composer; the 3-second history poll
  re-reads identity on TARGET_EXPIRED, after which sending works. Reloading the
  page also recovers.
