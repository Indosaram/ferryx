# A13 native inventory core - NOT READY (excluded-source test-build conflict)

Date: 2026-09-13. Task st_01a098c3. Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. Platform: macOS Darwin arm64.

## Original scope and binding contract

Implement only native paired-host credential inventory, real filesystem tests, one lib.rs module declaration, and A13-inventory-core evidence. The full binding plan `/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md` was read (all 833 lines), including section 6.2 and A13. No daemon, protocol, IPC, HTTP client/server, UI, Cargo, terminal, deployment, commit, release, or canonical-daemon operation belongs to this child.

Required core: private daemon-owned paired-hosts.v1.json, normalized secure relay origin, machine identity, display label, grant scope, token and monotonic generation; token-free sanitized views; offline retention; re-pair fencing; host-local forget/cancellation; explicit host-scoped copy/read-back migration; fail-closed corrupt/newer files; existing private atomic writer behavior. Native IPC, native pairing exchange, frontend deletion and browser/mobile migration isolation are later integration work, not full A13 acceptance here.

Read project root/src-tauri AGENTS.md and existing auth private writer, workspace catalog, scoped Epoch, Cargo manifest and frontend host-key conventions. No programming/Rust skill was discoverable in the installed skill locations inspected; no skill compliance beyond project instructions is claimed. Initial git status needed `git -c diff.ignoreSubmodules=all status --short` because the foreign Ghostty submodule path is a symlink. Foreign work was not reverted. Current lib.rs diff at core registration time was exactly `pub mod paired_host;`.

## Delivered API and ownership assumptions

- `paired_host::inventory::{Inventory, Pairing, LegacyCredential, CredentialLease, HostView, MigrationReceipt, GrantScope, AuthStatus, InventoryError}` are reachable public types for subsequent native response DTOs. HostView and receipt derive Serialize/Deserialize; persisted private Record/Disk are not public. Inputs and records have no Debug; store/lease Debug is constant redacted; errors never embed serde values or credentials.
- HostView JSON remains `{hostId,relayOrigin,machineId,displayLabel,grantScope,generation,authStatus,online}`. Generation uses existing canonical decimal-string Epoch. Receipt contains only hostId/generation.
- One daemon-owned instance under the daemon mutation gate, synchronous IO on a blocking worker. No second daemon or universal network client. Caller must pass an issuer-verified grant, not trust a renderer-selected machine scope. Owner coordination across independent Inventory instances is NOT supplied by this core; integration must construct/share exactly one owner. External disk changes are detected before mutation and fence the owner, but this is not an interprocess read-modify-write transaction lock.
- Existing `remote::auth::write_private_json` supplies same-directory temporary write, Unix directory 0700/file 0600, file sync, rename, and best-effort parent-directory sync. Windows inherits the existing per-user-directory ACL convention. Core read-back verifies every committed candidate. It intentionally preserves the writer's existing best-effort parent-sync behavior, not stronger durability claims. Failed/ambiguous writes fence memory and cancel leases; old visible records become unknown, never empty.
- Forget removes only named credentials/label/record. A host-key/generation tombstone remains to prevent stale generation ABA after restart/re-pair. No remote operation exists. Re-pair/revoke/forget cancel the exact host's watch channel; callers subscribe before IO and must select cancellation and validate generation before adopting responses. Dropping the owner invalidates lease access.
- Migration requires exact explicitly host-scoped identity, rejects origin-wide proof, preserves borrowed original input and does not delete source files. An existing identical native record is idempotently verified; different/revoked/forgotten native authority is never overwritten by migration. A receipt is issued only after read-back.
- Production origins are HTTPS only, no userinfo/query/fragment/non-root path. URL canonicalization and encodeURIComponent-compatible machine encoding match frontend host identity. Under cfg(test), `Inventory::open_test_loopback(&private_dir)` allows only loopback HTTP and carries the policy through pair, load, commit and migration. Normal production open rejects fixture HTTP records without overwriting them. This method is for native unit fixtures, not external integration-test builds of a production library.
- Parent was informed that service.rs did not yet exist; no service module declaration was added. Parent/native integration owns it when created.

## RED and attempted GREEN evidence

All commands executed via the checked-in owned runner below, with clean env before initialization and unique private roots. No sleeps/polling in tests. Watch receivers are subscribed before mutation, then awaited with bounded timeout. Filesystem write failure uses a directory at the exact temporary-file path, so it fails deterministically even as root.

1. `A13-inventory-core-RED.log`, exit 101: minimal pass-through normalization stub, two meaningful security/identity failures: public HTTP accepted and uppercase/default-port origin not normalized. 0 passed, 2 failed.
2. `A13-inventory-core-RED-storage.log`, exit 101: deliberately omitted private write with read-back still required. Pair/migration failed as unavailable/pending; 3 passed, 6 failed. One additional fixture mistake was observed here: a Unicode machine ID was interpolated into an HTTP bearer. Corrected test data to ASCII hex-encoded unique fixture tokens; did not weaken production header-token validation.
3. `A13-inventory-core-RED-generation.log`, exit 101: deliberately reset re-pair generation to 1 while real persistence remained enabled. 8 passed, 1 failed at expected Epoch(2), observed Epoch(1). This run executed real filesystem permission, restart, secret exclusion, migration, actual failed-write, corruption and scope tests. Mutation removed afterward: production uses checked per-host increment.
4. `A13-inventory-core-GREEN.log`, exit 101: final full core suite DID NOT RUN. Concurrent excluded `src/remote/journal_spawn_contention_tests.rs` failed compilation: private `sessions.spawn_lock` at lines 25, 79, 89, 100; private `sessions.terminal_service` at 101; associated E0282 at 100. Six compiler errors. This is NOT a GREEN result and was reported to parent; excluded files were not edited.
5. `A13-inventory-core-check.log`, exit 0: requested headless CLI/relay check passed, existing warnings retained (19 warnings).
6. Parent subsequently requested actual inventory loopback fixture policy and public Deserialize DTOs. Added cfg(test) open_test_loopback and a tenth filesystem test proving fixture persistence/reopen and production rejection. `A13-inventory-core-check-loopback.log`, exit 0 verifies final production source. The added test and final generation-restored suite remain unexecuted due to the concrete excluded-source blocker. No full core GREEN claim.

LSP was called before builds. Final paired_host directory: 3 files, zero diagnostics. lib.rs: only inactive-code hints; an earlier fresh-diagnostics call timed out and a subsequent call succeeded. `git diff --check -- src-tauri/src/lib.rs` passed. Rustfmt applied only owned paired_host files.

## Exact commands

Working directory is the worktree above. Runner source is `docs/evidence/paired-daemon/A13-inventory-core-run.sh`; this is the complete reproducible environment definition (env -i, explicit PATH/CARGO_HOME/RUSTUP_HOME, jobs2, debug0, incremental0, empty RUSTC_WRAPPER, existing worktree target directory). Each invocation creates private HOME, FERRYX_RUNTIME_DIR/FERRYX_DATA_DIR/FERRYX_SESSION_DIR/FERRYX_AGENT_STATE_SOCKET, XDG config/cache/data/runtime and TMPDIR/TMP/TEMP. All inherited FERRYX variables are discarded.

```sh
bash docs/evidence/paired-daemon/A13-inventory-core-run.sh RED cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --nocapture
bash docs/evidence/paired-daemon/A13-inventory-core-run.sh RED-storage cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --nocapture
bash docs/evidence/paired-daemon/A13-inventory-core-run.sh RED-generation cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --nocapture
bash docs/evidence/paired-daemon/A13-inventory-core-run.sh GREEN cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --nocapture
bash docs/evidence/paired-daemon/A13-inventory-core-run.sh check cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay
bash docs/evidence/paired-daemon/A13-inventory-core-run.sh check-loopback cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay
/Users/indo/.cargo/bin/rustfmt --edition 2021 src-tauri/src/paired_host/inventory.rs src-tauri/src/paired_host/inventory_tests.rs
git diff --check -- src-tauri/src/lib.rs
```

RED commands describe intentional earlier source mutations; rerunning them on final source does not reproduce RED without those recorded mutations.

## Hashes and cleanup receipts

`A13-inventory-core-hashes.sha256` records final owned Rust source, lib.rs, binding plan, command runner and log SHA-256 values. These are snapshot hashes in a live shared worktree, not a commit. `A13-inventory-core-cleanup.log` records all six exact owned temporary roots removed with `exists=no`, including failed runs. Rust test roots are under each invocation's private TMPDIR; TempDir/drop plus runner EXIT trap cleans both passing and panicking cases. No subprocess daemon, socket listener, GUI, remote host, or network fixture was started. Existing worktree Cargo target intentionally remains per instruction. No global config changes or commits.

## Remaining integration / acceptance gaps

NOT READY until excluded journal contention test compile conflict is corrected by its owner and the final 10-test core suite passes. No native IPC command, daemon lifecycle wiring, pairing exchange, capability negotiation, authenticated request redirect policy, UI storage migration/deletion, actual proxy/socket cancellation consumer, desktop manual pairing, Linux runtime, Windows ACL verification, or full A13 acceptance is claimed. Parent/native integration may consume the stable API but must not mistake the passing production check or 8-pass negative-control run for final core acceptance.
