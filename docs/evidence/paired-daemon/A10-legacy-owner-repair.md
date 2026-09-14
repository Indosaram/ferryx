# A10 routed legacy owner repair - IN PROGRESS / NOT READY

Task st_01a09869. Worktree `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

## Evidence currently established

- Full approved plan and requested A09/A10/A12 evidence read. Prior implementation reused; no CRUD rewrite.
- Actual RED before production edits: `A10-legacy-owner-RED-seam.log`, command `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_owner_handover -- --nocapture`, exit101. Real prepare/commit/manifest adoption, two private owner objects and actual HTTP/legacy UDS. Original PID54337 survives, old HTTP detail running at owner epoch1789264045867, replacement HTTP detail expired while gateway epoch1789264045868.
- Initial `A10-legacy-owner-RED.log` and `...RED-runtime.log` are fixture setup failures (path-component prefix check, then private runtime mode755), NOT behavioral RED. Corrected supervisor uses directories mode700.
- Actual compiled socket increment: `A10-legacy-owner-socket-increment.log`, same command exit101. Original PID67518 and original CWD are emitted by the actual PTY through routed WS; owner generation2 fences first gateway socket; resize103x37 reaches original PTY. HTTP detail still expired because parent-owned adapter had not yet been integrated. This is scoped runtime progress, NOT GREEN acceptance.
- Intermediate headless check exit101 is retained in `A10-legacy-owner-increment-check.log`: socket module import error corrected. Parent subsequent compile succeeded. A later parent E0061 occurred between paired parameter-removal edits; final function and call both have six arguments. No clean final compile claim yet.

## Candidate implementation

- Additive MachineSessionDetail request transports explicit machine SessionDetail, preserving original raw target and owner epoch transitively. Missing old contract fails MACHINE_OWNER_UNSUPPORTED, no Local/mirror inference.
- Routed inventory offloads local journal reads, preserves unavailable predecessor rows and target epochs, marks partial/unavailableWorkspaceIds rather than issuing Create.
- MachineGateway negotiates a connection on the validated legacy daemon socket and runs the existing authenticated owner HTTP/WS router. The original owner retains the sole controller map, generations, leases and PTY operations. Socket binary input uses that owner's write_input_cancellable.
- Routed close HTTP transport preserves bearer header, raw path and unchanged body; owner authenticates and executes existing close epoch/controller/journal checks. Parent owns session_api composition; transport alone is not acceptance.
- Async mirror list/socket classification offloads journal ownership reads and fails closed on worker error. Synchronous selection classification remains inherited.
- A12 snapshot now awaits routed inventory after off-thread catalog read; receiver/sequence ordering preserved.

## Coordination/freeze

Parent owns session_api.rs and machine_operation_journal.rs; neither edited here. Exact integration is in A10-legacy-owner-adapter-coordination.md. Child production writes are frozen pending parent's adapter composition at explicit request. Source hashes at freeze are in A10-legacy-owner-frozen.sha256. Subsequent parent/server composition may legitimately change those hashes.

No output_hub.rs, terminal/session.rs, terminal/pty.rs, dependency/Cargo, relay, filesystem/worktree, or UI changes. Client.rs edits explicitly authorized: exhaustive request names plus retry-safe detail classification. Output budget remains OPEN/parent-owned. Parent journal contention RED/GREEN is separate evidence and not attributed to this child.

## Cleanup receipts

Each child test command ran with fresh `/tmp/a10-owner-*` HOME, FERRYX_RUNTIME_DIR/DATA_DIR/SESSION_DIR, XDG_CONFIG_HOME/DATA_HOME/CACHE_HOME/RUNTIME_DIR and TMPDIR before library state. Normal /Users/indo/.cargo and .rustup retained. Private existing target; CARGO_BUILD_JOBS=2, DEV/TEST_DEBUG=0, CARGO_INCREMENTAL=0, RUSTC_WRAPPER empty.

RED and increment logs record owned PTYs reaped, gateway/legacy tasks joined, fixture root removed, and supervisor removed on failure. No canonical daemon, desktop, foreign worktree, public relay, commit or release contacted. Temporary .debug-journal.md remains until final cleanup.

## Remaining acceptance (not silently waived)

Parent adapter composition and full GREEN; explicit old-peer runtime case; complete stale-input/ticket and unavailable-owner assertions; final snapshot/regression/plan cargo targets; all changed-file diagnostics and headless check; complete source hashes and cleanup. Current fixture uses two real private owner objects in one isolated test process with actual HTTP/WS/PTY/UDS and handover manager, not two separately spawned owner processes. Do not claim separate-process owner proof from this fixture. Expanded tests after the socket-increment run remain unverified.

LSP: machine_owner, machine_gateway, machine_peer, machine_events and owner regression returned no errors in individual calls; later remote server/socket requests cancelled, not clean diagnostics. Protocol/client returned no errors. git diff --check exit0 at freeze. New modules/test files are below200 pure LOC; inherited large modules remain large. No final architecture/test-completeness signoff until GREEN.
