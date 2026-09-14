# A12 owner-validated session metadata publication

## Outcome

Owner-local publication, successor live forwarding, canonical agent socket proof, revision separation, actual owner IPC reconnect/exhaustion/drain, and legacy wire compatibility are GREEN. The Git-transition socket reset is now traced and fixed, with deterministic snapshot/commit overlap coverage. The final reliability addendum supersedes earlier unmet/reset notes below; historical failures remain retained in their logs.

Implemented:
- `daemon/session_metadata_events.rs`: session-lifetime output subscription independent of focus/desktop attach, using existing NativeTerminal/TerminalEngine; bounded scrollback and existing broadcast/history; owner PID CWD query triggered by output, no polling; exact committed target; owner/catalog validation before machine-only publication. Lag emits invalidation and resubscribes retained history.
- `daemon/session_metadata_provider.rs`: canonical agent report hints routed to exact predecessor owner via additive `MachineSessionMetadata` IPC. Only existing authoritative OMO adapter is admitted: actual descendant discovery, typed provider key/resume plan, exact discovered ID, owner transcript/CWD resolution, duplicate claim validation, durable metadata commit. Unsupported/unverified providers reject rather than manufacture IDs.
- Metadata-specific server agent handler/request dispatch and protocol/client request-kind additions. No paired-host sections rewritten; no shared-file whole formatting.
- `Session.title`/`agentType` defaulted optional fields preserve old decoding.
- Journal metadata transactions fence exact target and exit state; latest title/CWD/agent/provider survive exit writes. Session exit events read the merged durable row.
- No net change to ipc/agents.rs: a speculative ps-width correction was disproven and reverted.

## RED -> GREEN evidence

Original RED retained, not rerun: `A12-session-metadata-RED.log`, exit 101. Compilation succeeded; inactive owner shell completed `cd` and emitted full title control, machine WS timed out awaiting sessionMetadataChanged. Parent independently reproduced it.

Final composed command:

```sh
bash docs/evidence/paired-daemon/A12-session-metadata-run.sh FINAL \
 cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features \
 --test machine_session_metadata --test machine_sessions --test machine_events -- --nocapture
```

`A12-session-metadata-FINAL.log`: all three integration binaries passed in one batch:
- machine_events: 1 passed, 9.67s (real watcher/worktree/snapshot/lag/mirror coverage).
- machine_session_metadata: 1 passed, 5.45s.
- machine_sessions: 1 passed, 26.32s (lifecycle/idempotency/root/close regression).

Metadata regression exercises real authenticated HTTP/WS/PTY:
1. Inactive shell CWD/title output reaches machine WS with exact target; CWD-only and title updates may be distinct events, so test awaits exact title state.
2. Ordered mirror barrier receives no machine path payload.
3. Stale epoch, forged machine, unowned raw session, and invented provider report all reject.
4. Actual descendant `/omo.js` fixture, real environment discovery, real owner transcript/CWD validates provider; typed provider event reaches WS. macOS fixture uses runner-supplied `A12_PROVIDER_FIXTURE_SHELL=/opt/homebrew/bin/bash` because protected system executables conceal their environment from ps. Production has no Homebrew-shell dependency.
5. Actual prepared predecessor listener and successor SessionRouter route exercise owner IPC/HTTP; retained title/target survive at distinct gateway epoch, successor owns no PTY. Fixture uses prepare/add_legacy_peer/abort rather than committing daemon retirement inside the test process.
6. Explicit HTTP close preserves title/CWD in exited event and HTTP detail.

`A12-session-metadata-build.log`: `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli` passed (19.63s).

Intermediate failed GREEN attempts are retained, not relabeled RED: initial nested imports, earlier CWD-only event, protected-executable provider fixture, and invalid attempt to abort a committed/draining fixture handover. One handover attempt exited the process via production retirement without a test summary; it is NOT counted as a test pass.

## Revisions: explicit current limitation

Session journal revision advances on accepted metadata. Test compares HTTP session revisions only to previous HTTP session revisions, not event/project revision. Sequence remains the stream ordering cursor.

Current machine_events.rs still mixes project and session domain revisions in top-level envelope revision. No changes made to that file. Concrete minimal proposal delivered before changes in `A12-session-metadata-revision-proposal.md`: make envelope revision equal the sequence cursor; preserve payload projects/sessions revisions and expose supplied domain revision separately. Parent approval/composition pending.

## Verification and cleanup

- LSP diagnostics: no errors on all changed Rust files, including tests/support modules.
- New production/support modules rustfmt formatted; shared server/client/protocol/session/journal files not whole-formatted.
- Runner `bash -n` passed.
- Runner isolates HOME/data/runtime/session/XDG/tmp before process initialization, env -i, private target, jobs2/debug0/incremental0, explicit /Users/indo/.cargo and /Users/indo/.rustup.
- Final test logs confirm owned PTYs closed, grants revoked, listeners joined, machine subscriptions zero, fixture roots removed. Runner `.cleanup` records private-root removal status; `.monitor` records child wait/exit. Async cleanup awaits bounded at 10 seconds.
- `A12-session-metadata-GREEN-source.sha256` records changed sources after new-file formatting. Build was executed on formatted production source. Shared files may change later in sibling lanes.
- Existing compiler warnings retained and not suppressed.

## Architectural review

New modules own output metadata, provider admission, predecessor route proof, or provider process fixture respectively. No new unsafe, casts, unwrap/expect in production, suppression, polling, provider guesses, generic-value domain APIs, or global config changes. Helpers have at most three parameters except the pre-existing publish_revision seam. Trusted metadata is derived from owner PID/parser/discovery before transaction; provider boundary rejects unverified hints. New module LOCs: output143, provider155, handover support89, provider support85, main regression199. Shared inherited oversized files received focused hooks only; no unrelated restructuring because concurrent owners retain those files.

## Explicit unmet items / do not overclaim

- Live metadata events from a predecessor are not forwarded into the successor's machine broadcaster; successor HTTP detail does route correctly. A successor machine WS subscription currently needs an owner event adapter, not local PID/path probing.
- Revision envelope composition awaiting parent decision/application.
- Canonical agent socket production hook exists, but accepted-provider regression invokes the same public owner admission method, not the Unix report socket itself.
- Unsupported provider types remain unsupported; tested discovery acceptance is macOS OMO only.
- The 10 inventory + 11 journal/watch parent gates were not rerun here as a named combined unit gate; real machine_events + machine_sessions passed. Parent owns its independent watch/build batch.
- Silent CWD changes with no PTY output do not emit until next output/report; no periodic PID polling was added.

No commits, release operations, canonical-daemon or desktop manipulation. This is production owner metadata GREEN with the remaining A12 integration obligations listed, not a claim of full paired-daemon completion.

## Successor completion addendum (current result)

- Approved revision contract applied: envelope revision equals sequence, snapshots capture both before construction. publish_revision preserves sessionRevision/projectRevision separately; payload domain revisions remain unchanged. Behavioral revision RED was real 3 != 4 (revision-RED.log), followed by GREEN.
- New session_metadata_forward.rs streams typed retained owner records over authenticated/validated legacy IPC; full target is checked, no successor PID/path probe. Per-WS JoinSet aborts forwarding on disconnect; line sizes capped at 64KiB, handshake/write deadlines 10s; owner failure emits invalidation and at most three reconnect attempts (1s/2s backoff), then WS redial restarts.
- Real successor WS test changes the predecessor title AFTER subscribing, observes original epoch, disconnects/redials, gets retained snapshot and another live change, verifies successor mirror ordered barrier and zero subscriptions.
- Provider acceptance now writes a typed newline report to the canonical Unix agent socket; subscribes before writing and awaits the exact admitted provider event. Listener/client task cleanup is joined.
- Process-bound existing adapters now admitted for OMO, Claude, Codex, Copilot, Cursor, Kimi, GJC. Pi/OpenCode/Antigravity adapters with guessed transcript fallback remain rejected; non-OMO acceptance is source-integrated but not platform/runtime-proven by this fixture.
- successor-verified.log: metadata test passed (5.52s); lifecycle result and runner exit are recorded alongside. successor-build.log: headless CLI build passed (19.86s). event-isolation.log: machine_events passed (9.73s). Earlier successor-FINAL.log failed machine_events with ResetWithoutClosingHandshake during external Git transition; cause not proven, no watcher code changed. This is an outstanding reliability finding, not a suppressed failure.
- successor-source.sha256 captures current sources. LSP diagnostics on changed files found no errors. New forwarder143 LOC, handover support165 LOC; no unsafe or generic-value domain state introduced. Shared refresh/select/permit logic was not rewritten; snapshot inventory setup adds scoped forwarders.
- Remaining verification limits: forced owner-IPC disconnect/backoff is not directly exercised (WS redial is); non-OMO provider runtime fixtures absent; bounded JoinSet cancellation is observed through WS subscription count/listener cleanup, not an independent count of every owner forwarder. Silent CWD still updates on next owner output/report.

## Final reliability and wire compatibility (supersedes earlier limitations)

Reset root cause proven in reset-trace.log: project_inventory observes catalog revision, concurrent Git/worktree commit changes revision, inventory returns STALE_REVISION, old serve drops watcher and returns without Close. Fix retains watcher, emits explicit stale partial boundary and rebuilds authoritative snapshot. Terminal snapshot failures send Close. deterministic-ready.log forces a real commit between observation/commit via cfg(test) eventInventoryObserved barrier and proves stale then complete on SAME WS (1 passed0.13s). No watcher or relay tunnel changes.

Actual owner IPC test now aborts and joins predecessor client tasks while successor WS remains open. It proves partial disconnect boundary, >=1s retry floor, explicit ownerRecovered complete boundary. It then disables owner accepts, exhausts finite retry budget, observes JoinSet completion and ownerUnavailable partial session inventory, and waits for forwarders=0, owner_streams=0, subscriptions=0. Completion is consumed; exhausted target retains explicit disconnected state until event redial rather than silently claiming complete inventory. Recovery state and snapshots use session-domain completeness, not HTTP detail success as proof of a working event stream. ipc-recovery-runtime-RED.log failed waiting for explicit recovery; ipc-recovery-GREEN and final-reliability pass.

Wire compatibility: Session title/agentType Option::None omitted again; original fixtures and equality assertions unchanged. New non-null shared contracts fixture is consumed by Rust and UI. UI RED proved decoder dropped metadata; decoder now retains typed optional strings. protocol-rust.log:9 passed. ui-parity-GREEN.log:2221 passed across Contracts/Parity. Explicit null optional metadata normalizes to omission in both runtimes. No blanket fixture rewrites.

Final combined command (--test machine_session_metadata --test machine_events --test machine_sessions) passed in ONE invocation: final-reliability.log event10.22s metadata9.61s lifecycle26.45s, each1 passed. reliability-build.log headless CLI build passed21.13s. Original/intermediate failing logs remain evidence, not ignored assertions. A14 missing module compile attempts explicitly excluded from behavioral results.

reliability-source.sha256 records current sources. LSP diagnostics found no errors on changed files. Forwarder166 LOC, MachineEvents197, deterministic test138, UI decoder196; handover fixture201 warning band (split before further additions). Shared oversized files retain focused hooks only. New code owns lifecycle accounting/metadata admission; no unsafe/type escapes; exact typed owner targets at boundary; no polling in tests; timer use only validates retry behavior. Resource counters use RAII, retry state uses watch channels; no arbitrary provider IDs or local remote-path probes. Canonical daemon/user desktop/config/commits untouched.

Remaining broader-scope validation: non-OMO runtime matrix and cross-platform owner streams remain A21/platform work. No unverified owner reconnect/reset item remains in this macOS A12 acceptance scope.
