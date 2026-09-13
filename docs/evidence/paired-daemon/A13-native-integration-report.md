# A13 native integration handoff

Native service and six Tauri commands implement the desktop contract in A13-desktop-state-command-contract.md. Full A13 desktop composition remains the parent gate.

## Delivered

- One daemon-owned lazy Inventory, loaded and mutated under one off-thread mutex via crate::ipc::run_blocking. Tauri reuses its managed Arc<DaemonClient>; it never opens a credential file.
- Native PIN exchange through the existing relay claim/exchange endpoint; native authenticated capabilities verify machine identity, issued scope, and machine Control permission. UI supplies no scope. Mirror projects as needsMachineGrant.
- HTTPS normalized production origins; no userinfo/path/query/fragment, no redirects, no proxy environment use, bounded request/response bodies and HTTP/UDS deadlines. Loopback HTTP is cfg(test) only.
- Captured live/tombstone generation snapshot before networking, exact target generation comparison inside the durable mutation gate. No lock crosses network await. Forget cancels inventory leases and only deletes the named local host.
- Explicit origin/machine legacy migration, verified copy receipt, separate exact-generation durable readback before renderer cleanup. Renderer bearer is inbound only; Debug secrets are redacted, successful responses contain HostView/receipt only.
- Connect-only compatibility path: unknown old capabilities return PAIRED_HOST_UNAVAILABLE without spawning/upgrading/retrying. pairedHostInventoryV1 is independent of pairedDaemonProxyV1, which remains false. Protocol version remains 3.
- Additive core exports: host_key, generation_snapshot, read_verified; core storage semantics preserved.

## Verification

All commands used env -i, private HOME/FERRYX/XDG/TMP under /private/tmp outside Git, explicit normal Cargo/Rustup homes, worktree target, jobs=2, debug=0, incremental=0, empty RUSTC_WRAPPER and umask 077. Runner logs PID/command/exit/reap and deletes its private tree. Test listeners/tasks use owned JoinSets and gateway shutdown guards; no fixed sleeps or polling.

- A13-native-integration-test.log: compiled; runtime failure on second issuer PIN generation, `Invalid pairing transition: Ready -> Registering`. Not a compile/DTO RED. First real pair had succeeded.
- A13-native-integration-acceptance.log: 14 tests passed, zero failures. Real private DaemonServer over UDS, ephemeral actual relay plus two actual gateways: machine pair, re-pair generations 1/2, mirror generation 3, lease cancellation, stale forget rejection, cross-host isolation, successful retry migration/readback, failed migration retention, private modes 0700/0600, store restart persistence, local-only forget. Additional native HTTP redirect/no-forwarding and old-daemon/no-upgrade tests pass; existing inventory tests included.
- A13-native-integration-check.log: headless cargo check exit 0.
- A13-native-integration-build.log: headless cargo build exit 0.
- Compiler reports existing warnings (17 test, 19 check/build); no suppression added.
- LSP requested on all changed Rust files before validators. Several requests were cancelled/timed out by shared server; server/client/protocol and service/IPC individual requests returned no diagnostics at points recorded in task transcript. Cargo check is the complete compiler validation.
- git diff --check on shared changed files passed.

## Important limitations / parent integration

1. Existing issuer coordinator does not transition Ready after successful relay redemption. Repeated generation fails until explicit lifecycle transition. The fixture expires the completed issuer PIN via existing transition API before issuing another. Receiver re-pair works; no second coordinator or issuer fix was added. Parent must account for real repeated CLI PIN issuance independently.
2. Foreign A03 owner fixture still asserts capabilities exactly equal [machinePairingV1] at a03_owner_cli_fixture.rs:88. Parent/fixture owner must update its expectation for the newly working inventory capability, without advertising proxy support. This fixture was not run by this lane.
3. The native daemon fixture is the actual DaemonServer/client UDS boundary inside the Rust test process, with real gateway/relay listeners; it is not a separately exec'd daemon binary restart. Persistence restart is actual store reopen, not process-restart proof. Exact stale adoption is implemented by tombstone snapshots; a paused-network/re-pair race fixture is not yet included.
4. A mistaken built-binary `--help` verification entered GUI launch mode (main.rs has no generic help branch), timed out, and left private fixture files. It is NOT acceptance evidence and violated the intended no-GUI procedure. Subsequent process scan showed no worktree ferryx process remaining; only pre-existing /Applications/Ferryx processes, which were not touched. Only the attempt's /private/tmp/a13-native.svtSE3KS directory was removed. entrypoint.cleanup records this; no a13-native.* fixture directories remained in final scan.

No commits, dependency/global configuration changes, remote deletion, or canonical credential operations were performed intentionally. Parent owns final review and aggregate composition.
