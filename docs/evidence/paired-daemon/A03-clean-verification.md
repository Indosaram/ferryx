# A03 clean-candidate independent verification

Verifier: st_01a093a1, generation 9 revision, 2026-09-12. Source-isolation findings retained from st_01a09398 and rechecked against the current candidate.
Candidate: `/Users/indo/code/project/orca-lite-wt/herdr-wave0-clean`.
Base and observed HEAD: `cd16c90e76e2ea1ce56f0213932675b613d0a47b`.

## Verdict: scoped A03 acceptance CONFIRMED; required full remote command remains FAILED

The clean candidate contains the A03 scope/grant/admission changes without the foreign feature overlay and follows `herdr-wave0/docs/evidence/paired-daemon/A03-clean-commit-dependencies.md`. Parent receipt `A03-clean-runtime.log` establishes successful clean headless CLI/relay check, fresh CLI build, and all 18 A03 tests including actual owner CLI and HTTP scenarios. These close the prior missing-build/live-evidence blockers. This verifier inspected the receipts; it did not independently execute Cargo.

The required full remote command is NOT green: 196 passed, 5 failed, exit 101. The subsequently supplied `A03-clean-baseline-remote.log` proves the identical command on untouched cd16c90 also fails the same five tests (179 passed, 5 failed). Failure attribution is therefore resolved as pre-existing for this configuration; the overall required green gate remains unmet. No full Wave0, overall goal, staging or commit approval follows from scoped A03 acceptance. The producer JSON's `candidate_validated: false` is historical preparation status, superseded only to the bounded extent of these actual receipts.

The original overlay's 218 remote tests and 18 focused tests are provenance, NOT clean-candidate verification. The clean remote-suite count must not be required to equal 218: foreign tests are intentionally absent. No runtime commands or mutation reruns were performed by this verifier.

## Sources inspected and provenance

Read the A03 packet in `herdr-wave0/docs/evidence/paired-daemon/plan-baseline.md:495-507`, the complete original accepted `A03.md` and `A03-verification.md`, the clean dependency audit and complete `A03-clean-command-request.json`. Read the original foreign manifest and original patch sections establishing three/four-field response differences, foreign daemon constructors/configuration, terminal-service state plumbing and auto-spawn tests. Read the clean overlay README, which correctly labels its logs as historical, not candidate results.

Inspected every tracked candidate hunk against `git diff cd16c90`, both complete new Rust test files, and the surrounding actual auth exchange/approval and coordinator cancellation/acknowledgement paths. Independently compared all 11 source hashes with the clean producer manifest: all matched. The four unmixed files (auth, relay client, machine authorization tests, owner CLI fixture) are byte-identical to the accepted overlay source. That establishes provenance, not execution in the new build context.

## Complete source delta and ownership

Nine tracked Rust files differ from base (324 insertions, 42 deletions); two new Rust test files are untracked. No other tracked source changes were observed.

| File under src-tauri/src | Inspected A03 delta and exclusion proof |
| --- | --- |
| cli.rs | Adds explicit machine selection, strict access argument parsing, separate machine request and authority display. Both success branches return the base three-tuple. Existing environment/default URL resolution remains; foreign daemon URL precedence was not copied. |
| daemon/client.rs | Exactly two additive request-name arms. Detailed pairing return remains three elements. |
| daemon/protocol.rs | Adds GetCapabilities, distinct RemoteCreateMachinePairingCode, CapabilitiesOk. Pairing response still has only code/pairing_token/machine_id; no relay_url addition or foreign test adjustment. |
| daemon/server.rs | Adds test module, publishes epoch, dispatches additive capabilities and scoped issuance inside base coordinator/fallback control flow. Base constructors, auto-configuration and local identity fallback remain. No foreign service-sharing/config-error/relay-URL rewrite. |
| remote/auth.rs | Default-Mirror persisted access scope, validated scoped issuers, issuer scope propagation through exchange and CLI approval. Machine View rejects, not promotes. Existing compatibility wrappers remain test-only where appropriate. |
| remote/relay_client.rs | Existing single coordinator delegates legacy issuance to Mirror, validates Machine Control before publication, supplies scope to both PIN and capability records. Existing generation/ack cancellation remains. |
| remote/server.rs | Off-thread shared identity helper, authenticated capabilities with pre/post-await auth, typed no-store admission errors, empty service advertisement, private exchange success, identity startup use and four A03 tests. Base native feature guards and mirror/grid implementation remain. The conditional test Duration import is a clean-context adaptation. |
| remote/state.rs | Only epoch/identity directory/test probe and required AtomicU64 import/initialization in base new_with_paths_backend. No terminal_service field or foreign service constructors. |
| remote/tests.rs | Only machine_auth module declaration. No foreign headless/handover tests copied. |
| remote/machine_auth_tests.rs | Complete 13-test A03 module, byte-identical to accepted source. |
| daemon/a03_owner_cli_fixture.rs | Complete real owner-CLI isolated fixture, byte-identical to accepted source. |

Independently checked foreign-only tracked files against `git show cd16c90:path`: release scripts, IPC remote, remote mirror, relay server, terminal service, Tauri configuration, App/App tests, RemoteHostSwitcher, RemoteApp and remoteHostStore tests are all base-identical. Foreign new UI files (App.remoteHostShortcuts.test.tsx, PairMachineModal.tsx, RemoteHostSwitcher.test.tsx, pairClient.ts) are absent. Thus neither source-level relay allowlist changes nor frontend/release/terminal overlays were smuggled through untouched paths.

All base test bodies remain in the reviewed diff: no deletion, skip or weakening. An additional textual comparison found all 131 base functions named test_*, a01_* or a02_* in changed files verbatim; this auxiliary check is not a compiler/parser or a complete inventory of differently named tests. The diff itself shows the additive test insertion and no base-test modifications. The three foreign terminal-service-dependent tests identified in the dependency audit are excluded by retaining base test content, not by suppressing failures.

## Plan-to-mechanism and test mapping (source inspection plus parent clean execution)

- Owner CLI default issuance is Mirror; explicit `--access machine` uses a separate daemon request that an old daemon cannot silently reinterpret as an unknown field. Protocol remains 3 and GetCapabilities is additive.
- Machine issuance is Control-only at both AuthManager and coordinator boundaries. Persisted device and pairing scope use serde defaults to Mirror. Actual exchange takes scope from the owner-issued record, not client-supplied JSON fields. Approval retains the same authority.
- Coordinator scope reaches both redeemable records; rejected/stale acknowledgement transitions cancel credentials. The focused test waits for exact registration then sends rejected or stale ack and tests actual redemption denial for both credentials.
- Capability requests authenticate before identity work and again after its await. Identity IO uses the blocking dispatcher. The deterministic race test subscribes before dispatch, revokes after identity-entry notification, releases the worker and checks 401. No fixed sleep is used in A03 tests.
- Directory GET and session POST authenticate first, return typed/no-store 403 for Mirror or 503 for Machine Control while services are absent. Capabilities advertise no unimplemented machine services.
- The fixture launches a real freshly built CLI, uses a private in-crate DaemonServer and private real relay, exchanges each CLI-issued PIN, checks actual scopes and direct-gateway capabilities/admission, revokes and verifies zero identity probes. It checks exact owner PID, protocol 3, additive capabilities and legacy Ping. It captures authority for human review without pinning full prose or printing secrets.
- Fixture isolation is source-backed: private HOME/data/runtime roots, ephemeral listeners, explicit gateway configuration, bounded requests, signaled listener shutdown, joined UDS client tasks, owned child process group and checked temporary-root removal. It deliberately does not skip when the CLI binary is missing. The parent clean execution now records successful cleanup for this fixture; this is not a claim that every existing remote-suite harness explicitly joins its handlers.

Retained limitations: legacy pairing-record default is source-backed, not a separate executed migration scenario. Existing local PIN fallback identity lookup and configuration behavior remain base behavior. The manual fixture configures its private gateway before issuing PINs; it does not validate the foreign fresh-Off auto-configuration enhancement. This packet does not implement A04 shared services, directory/session execution or relay capability routing.

## Historical parent execution request and resolved prerequisite blocker

A bridge-request tool is not exposed in this child's available tool namespace. The evidence request was emitted in child commentary and is recorded here for the parent; no successful bridge delivery or response is claimed. The existing producer request is `herdr-wave0/docs/evidence/paired-daemon/A03-clean-command-request.json`.

Parent must execute from the clean worktree with an absolute private prepared CARGO_TARGET_DIR and private suite-wide HOME/FERRYX_DATA_DIR/FERRYX_RUNTIME_DIR. Preserve the toolchain's actual CARGO_HOME/RUSTUP_HOME when changing HOME. No shared target or canonical daemon is authorized by this report. The exact requested sequential command is:

```sh
cd /Users/indo/code/project/orca-lite-wt/herdr-wave0-clean
: "${CARGO_TARGET_DIR:?Parent must export private prepared target directory}"
export CARGO_BUILD_JOBS=4
unset A03_PRIVATE_ROOT A03_CLI_BINARY FERRYX_MACHINE_TOKEN FERRYX_RELAY_URL
cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay &&
cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli &&
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib a03_ -- --nocapture &&
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture &&
git diff --check
```

The original evidence requirements, now addressed by the clean and baseline receipts below, were:

1. Actual command, working directory, private target location, enclosing exit status and headless CLI/relay check plus fresh CLI build output.
2. Focused results including `daemon::server::a03_owner_cli_fixture::a03_private_owner_cli_surface`, mirror/machine authority and returned scopes, exact PID/protocol receipts and successful cleanup; no secret output. The source contains 18 A03 tests including this fixture.
3. Clean remote regression result with its actual count and failures/warnings, without importing the overlay's 218 expectation.
4. Before/after source identity matching the inspected manifest. If source changes, re-review the changed delta rather than assigning these conclusions to another candidate.

Late-arriving parent receipts were read before finalization. `A03-clean-build-prerequisite.log` records the exact requested clean command with worktree-local target, monitor `mon_D0289B6X8VBDM72J`, exit 101. Cargo failed in the Tauri custom build command: `resource path ../ui/dist doesn't exist`. The chained fresh CLI build and both test commands therefore did not run in that invocation. This is a generated-resource prerequisite failure, not behavioral RED or proof of a Rust source error. `A03-clean-ui-build.log`, monitor `mon_SK9CCY29YMSJ947M`, records `bun run --cwd ui build` (`tsc && vite build`) exit 0, with Vite completing in 5.68 seconds. The parent has supplied the missing generated resource; the subsequent successful headless check/build and focused live/test receipt is now recorded below. Overlay compile failures are historical and are not relabeled as clean failures.

## Source snapshot

All entries matched the producer clean manifest during this verification.

| Path under src-tauri/src | SHA-256 |
| --- | --- |
| cli.rs | e64d4413869f12fb5c564685e1d77aecfbfb4712b60d05105bc2561156b77531 |
| daemon/a03_owner_cli_fixture.rs | 4e2239db9711390b7a3d32615d1d249f958e76b524c655ddc41225c583c4e330 |
| daemon/client.rs | 69151358926a13e7be8193367bad0ae19cabf50ab81ec5674a835eafe7f80fd6 |
| daemon/protocol.rs | 255834281e33a943baea9c876e575e1cf8bb8962fdc99d94a9e908c80ed03dbd |
| daemon/server.rs | fa92980b92102b8799346be88c4ff03affd26a0503f09b6fbd07fccefe32031c |
| remote/auth.rs | 8d9468845f28f6113419a9e05867227705f8f786c159fe4ecf057b1748125bf5 |
| remote/machine_auth_tests.rs | 6956043b428f055cbd6fec894d8b3ee786a37c225a5f527a26aba3d272a77767 |
| remote/relay_client.rs | 386abd5a2c71c0889912d6790b74bbf77352e2b75fab7d9bf0ac3f48a8786c0e |
| remote/server.rs | 0b568a165570701e3d78435209b76a23154ddcf9b2c007910460eb4212f344d1 |
| remote/state.rs | a07b344a9443d0d2bbc3375ceec8dcb04ab6cdf3c60661517656878b97327760 |
| remote/tests.rs | c80096f03889c567720b45747e6649bd6ad08411aeefaf9a0dc6799bb4528824 |

## Preservation and independently executed validation

`git diff --check` returned exit 0 independently in this revision. All 11 source hashes match the clean producer manifest; the four unmixed A03 files match the accepted overlay byte-for-byte. All foreign-only paths enumerated by the original foreign patch were compared with cd16c90: existing files are base-identical and foreign new files are absent. The complete candidate diff and both new test files were reread. Baseline HEAD independently resolves to cd16c90e76e2ea1ce56f0213932675b613d0a47b and its status was empty after the supplied run. No independent LSP/build/test success is claimed; producer source diagnostics are provenance only. This verifier changed only this report, did not control a daemon, launch a desktop, rerun mutations or Cargo, stage or commit. Historical overlay evidence was left untouched.

## Generation 9 actual clean execution and baseline comparison

All execution below is parent-supplied evidence, not independent verifier execution or overlay results.

`A03-clean-runtime.log`, monitor `mon_K86ZM0XPT7K7CRJJ`, records the historical requested chain above with the explicit worktree-local target `/Users/indo/code/project/orca-lite-wt/herdr-wave0-clean/src-tauri/target`, four build jobs, and cleared A03/relay credential overrides:

| Gate | Actual result |
| --- | --- |
| Headless check, ferryx-cli and ferryx-relay | Successful dev-profile completion, 10.67s |
| Fresh headless ferryx-cli build | Successful dev-profile completion, 36.81s |
| a03_ tests | 18 passed, 0 failed, 0 ignored, 835 filtered out, 1.30s |
| Nested actual owner CLI fixture | 1 passed, 0 failed; included in the outer 18, not a nineteenth test |
| Full remote:: tests | 196 passed, 5 failed, 0 ignored, 652 filtered out, 5.10s; exit 101 |
| Chained diff check | Not reached after the failed suite; parent separately reports exit 0 and verifier independently obtained exit 0 |

The live fixture reports exact owner PID 63655, UDS protocol 3, machinePairingV1 and legacy Ping. Actual CLI PIDs 63676 and 63891 exited 0 and were reaped. Default issuance displayed `Access: mirror; controls only exposed desktop sessions.` Explicit issuance displayed `Access: machine; permits browsing and executing programs as the daemon OS user.` Actual relay exchange returned Mirror/Control and Machine/Control respectively despite forged client authority fields. Direct gateway capabilities returned 200 with corresponding scope, stable machine identity, and no advertised services. Source assertions exercised by the passing fixture additionally require no-store, typed capabilities, anonymous/malformed 401, Mirror directory/session denial 403, Machine unavailable 503, revoked 401 and zero post-revocation identity probes. No bearer, PIN or signing key appears in this sanitized receipt.

Cleanup records stopped runtimes, removed sockets, reaped owner 63655 and removed outer private root. Parent follow-up `ps -p 63655,63676,63891 -o pid=,command=` returned no rows. These are exact-PID receipts, not inference from test success. Source hashes checked in this revision still match the pre-execution manifest. The log's command does not itself export suite-wide private HOME/data/runtime roots, so that earlier recommendation is not claimed as fulfilled by the command header. The owner fixture independently enforces its own private child roots; existing full-suite detached-handler/runtime-drop limitations remain.

### Baseline evidence resolves attribution, not the green gate

`A03-clean-baseline-remote.log`, monitor `mon_ZWZRVD3J4G2QE7DF`, records this actual parent command (not a rerun request):

```sh
cd /Users/indo/code/project/orca-lite-wt/herdr-a03-baseline
unset A03_PRIVATE_ROOT A03_CLI_BINARY FERRYX_MACHINE_TOKEN FERRYX_RELAY_URL
CARGO_TARGET_DIR='/Users/indo/code/project/orca-lite-wt/herdr-a03-baseline/src-tauri/target' CARGO_BUILD_JOBS=4 cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture
```

It compiled from the separate untouched cd16c90 tree and exited 101: 179 passed, 5 failed, 0 ignored, 651 filtered out, 3.27s. Both receipts name exactly these failures under `remote::tests::`:

- `test_daemon_remote_worktree_selection_then_grid_terminal_control`
- `test_grid_render_attach_sends_full_frame_with_session_dimensions`
- `test_grid_render_initial_frame_uses_requested_viewport_dimensions`
- `test_grid_render_live_pty_output_emits_grid_frame_after_attach`
- `test_grid_render_resize_returns_full_frame_with_requested_geometry`

Attach fails with `expected first server frame to be Text` (base tests.rs:1651, candidate:1654). The other four fail with `grid socket must emit a Text frame` (base:1851, candidate:1854). The three-line shift is solely the additive A03 test module declaration. Base server.rs:1228-1247 returns immediately for render=grid without native-terminal; the candidate preserves that branch, while the existing tests still expect grid frames. Actual baseline execution now corroborates this source explanation; no timing hypothesis or overlay pass substitutes for it.

The clean remote count is 201 = 184 base tests + 17 A03 remote tests. The eighteenth A03 test is the daemon owner CLI fixture outside the remote:: filter. There is no requirement to reach the overlay's 218. No base test was removed, ignored, weakened, or guarded to manufacture green. Compiler warnings remain: the baseline and candidate test builds each report 17 warnings, including unused mirror import/token/no_auth_query, unnecessary unsafe blocks and existing dead code. The clean non-test check/build report 16 warnings, not warning-free execution.

No baseline-result evidence request remains outstanding. Parent explicitly dispositioned the demonstrated pre-existing grid feature/test mismatch for separate repair rather than inclusion in the A03 auth commit. This closes the scope decision, not the required full-regression green gate: the full remote command still fails with the five failures above. No failures are suppressed or waived as passing. Fixing the grid feature/test mismatch is outside the authorized A03 verification scope. Native-feature builds, Linux/Windows, directory/session execution, relay capability routing, fresh-Off foreign auto-configuration and the full machine workflow remain unverified or unimplemented as previously stated. Scoped A03 is accepted on the clean candidate; full Wave0 and overall goal are not approved.
