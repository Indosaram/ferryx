# P23 implementation evidence - execution prerequisite outstanding

Owner: st_01a09a09. Scope: GB-01/03/04, relay_server.rs embedded tests; no remote/state.rs changes. This packet is NOT complete or verified GREEN.

## Completed staging

- Read repository, backend and remote AGENTS.md, orca-cli skill discovery stub, repair-packets/addendum/register and GB source evidence; checked current source and scoped git diff. relay_server.rs and auth.rs were clean on intake; foreign remote/state.rs and other dirty work remain untouched.
- Official executeAgentToolkit was imported from the requested installed module with resolveCwd bound to this repository and resolveSessionId bound to the parent. revise_criterion C002 returned ok=true, accepted=true. Full existing scenario was preserved and appended. Receipt: p23-registration.json.
- Added test-only probes at the actual bind_machine_key read and publication boundaries. An owned separate test process tries the tx.lock sidecar with File::try_lock. The parent requires WouldBlock at both boundaries, then verifies independent states preserve distinct keys on restart and reject conflicting ownership. No sleep, polling-success or two-reader barrier. Current production algorithm remains unchanged pending intended RED.
- Exact new test: remote::relay_server::tests::test_relay_key_store_transaction_is_cross_process. Child helper: remote::relay_server::tests::test_relay_transaction_lock_probe_child. Child command inherits no globally mutated environment; only its explicit owned key-file path is set. Child output/status is joined synchronously and temporary directory is RAII-owned.
- Staged p23-run.sh: exact --lib test, --exact --nocapture --test-threads=1, rustc/Cargo/source hash receipts, pipefail and explicit lead slot guard. No broad Cargo filters, no alternative targets/caches, no daemons or remote mutations.

## Actual validation

- Language server on relay_server.rs returned `No diagnostics found` at error severity.
- `bash -n docs/evidence/windows-review-20260913/p23-run.sh`: exit 0.
- `git diff --check -- src-tauri/src/remote/relay_server.rs`: exit 0.
- Ungranted runner refuses execution with exit 64 and `BLOCKED: lead-issued exclusive Cargo slot required (P23_CARGO_SLOT_GRANTED=st_01a09a09)`. This is NOT behavioral RED. Staging receipts: p23-staging.log.
- Cargo has not run; no test count/binary/behavioral RED/GREEN/build claims. No owned runtime processes were started, so none require cleanup.

## Exact lead action needed

Grant exclusive shared Darwin Cargo slot and resume this child, or execute:

```sh
P23_CARGO_SLOT_GRANTED=st_01a09a09 bash docs/evidence/windows-review-20260913/p23-run.sh red
```

Require one selected parent test and intended child failure `enrollment must hold a cross-process lock throughout reload and publication`, surfaced at the production read boundary. Compilation or zero tests is not RED. Only after that receipt apply smallest portable tx.lock guard before reload through publication, preserving separate low-level writer lock, propagating filesystem/lock errors. auth.rs need not change for this server-local guard.

Further P23 work remains staged conceptually but NOT implemented: GB-04 deterministic admission notice/rendezvous instead of 300ms success timeout; single injected lease clock with cap+1 checks; all-frame zero-payload denial through closure and a malicious benign-then-payload fixture; computed shell output absent from echoed command; revocation assertions and PTY/task cleanup on all paths. Existing exact test names and binary conditions are registered in C002. No claim that these assertions currently pass.

Native prerequisite belongs ONLY to st_01a099f8: execute exact coordinator_pairs_through_relay_to_real_gateway on owned Windows profile to capture the original /bin/sh spawn failure, then after native-shell fixture repair run identical real gateway/ConPTY case with computed output, focus denial, single-use ticket and revocation. No native commands were executed by this child. Binary enrollment acceptance still requires two isolated relay processes/ports sharing only owned key store and restart retaining both distinct keys and refusing a conflicting signer; lock-probe test is necessary regression evidence, not a replacement for that binary condition.

All changes remain uncommitted. No git branch/worktree/commit/push/install/release operations were performed.
