# A13 desktop-only sanitized state and migration adapter

Delivered in `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8` by task st_01a098c9. This is the UI boundary deliverable, NOT full A13 acceptance or verified native IPC integration.

## Changes

- `ui/src/state/remoteHostStore.ts`: desktop runtime never restores legacy credentials or writes ordinary state to the legacy storage key. Desktop rows are explicit allowlisted projections, with backend auth/scope/generation and migration/capability state. Browser normalization/persistence remains on its prior path. Native offline rows are retained independent of token truthiness; generation high-water marks and forgotten-host tombstones reject stale records.
- `ui/src/lib/pairedHostInventory.ts`: typed native commands, sanitized exceptions, PIN-only pairing, exact-generation forget, native capability negotiation and request/callback fencing. Local capability failure disables machine features and marks retained rows offline. Inventory capability does not imply proxy support.
- Legacy migration reads only known origin/machine pairs and canonical host token keys. Origin-wide keys and ambiguous aliases are ignored; conflicting host credential copies remain pending. Native durable write receipt and separate exact-generation readback must both succeed before removing only that row's credential field and its matching token key. Other rows, selection and metadata remain intact. Storage snapshots and scoped-key values are compared after awaits; concurrent changes cause pending status without overwriting those changes. No exception/log includes migration input.
- `ui/src/main.tsx`: minimal native-only nonblocking bootstrap invocation, before importing App. Browser boot unchanged.
- Added `ui/src/state/remoteHostStore.native.test.ts` and `ui/src/lib/pairedHostInventory.test.ts` with private in-memory/localStorage fixtures. Existing browser test file already had foreign localStorage isolation changes; those were read and left untouched.

## Native contract

The exact stable command/argument/response table and backend obligations are in `A13-desktop-state-command-contract.md`. Names: `paired_host_list`, `paired_host_capabilities`, `paired_host_pair`, `paired_host_migrate_legacy`, `paired_host_read`, `paired_host_forget`. Request-bearing invokes use `{ request }`. Capabilities return `{ pairedHostInventoryV1, pairedDaemonProxyV1 }`; proxy MUST remain false until A15/A16 support is ready. No command returns a bearer. Rust remains the authority for durable credentials, scope, native request/socket cancellation and migration concurrency/idempotence.

## Verification and actual results

Commands executed under a Python subprocess monitor with process PID reporting, bounded wait (120/180 seconds), captured output and exit files. No monitor tool exists in this child's exposed tools. Node Vitest was used with the existing `ui/vitest.config.ts`, setup and single-worker runner; no dependency/config changes. Read `ui/package.json`, tsconfig and runner config. No relevant runner-memory result was found in the inspected notepads. Inspected installed skills; Orca CLI skill applies only to managed runtime actions, which were not performed.

1. RED: from `ui`, `node node_modules/vitest/vitest.mjs run --maxWorkers=1 src/state/remoteHostStore.native.test.ts`, exit **1**. `A13-desktop-state-RED.log/.exit` records four actual behavioral assertion failures: bearer in restored serialized state; token presence producing `paired` instead of `unknown`; ordinary state updates changing original migration input; generation 8 replacing generation 9. Initial exploratory run had only two failures because the stale-label fixture changed `name` but not higher-priority `displayName`; fixture was corrected before the recorded four-failure RED. The legacy failure RED concerns preservation before native confirmation, not an existing native command (none existed). Explicit failed native write/readback cases are covered by GREEN adapter tests.
2. LSP: all five changed source/test files were checked. Initial diagnostics identified optional identity typing and obsolete RED fixture casts; both were fixed. All five then had clean diagnostics. After the final adapter guard/two extra tests, fresh LSP requests for the two adapter files timed out (including a retry). This limitation is not a clean final LSP claim; the full final TypeScript compiler passed in the build.
3. Focused GREEN from `ui`:
   `node node_modules/vitest/vitest.mjs run --maxWorkers=1 src/state/remoteHostStore.native.test.ts src/state/remoteHostStore.test.ts src/lib/pairedHostInventory.test.ts src/remote/RemoteRouting.test.tsx src/remote/MobileHostDrawer.test.tsx src/remote/zeroConfigSecurityProbe.test.tsx`
   Initial gate **58/58**, exit **0**, `A13-desktop-state-GREEN.log/.exit`. Final gate after conflict/capability tests **60/60 across six files**, exit **0**, `A13-desktop-state-final-GREEN.log/.exit`. No full-suite run. New async tests use explicitly subscribed deferred signals, not sleeps or polling; Vitest supplies bounded test timeouts.
4. Build from `ui`: `npm run build` (`tsc && vite build`), exit **0**, `A13-desktop-state-build.log/.exit`. Final runtime implementation and final tests were present. 1884 modules transformed. This is also the affected runnable build entry point; no desktop runtime launch was authorized.
5. Scoped `git -c diff.ignoreSubmodules=all diff --check`, exit **0**. Default git status hit a pre-existing symlinked-submodule restriction; inspection used ignoreSubmodules and no foreign changes were modified.

Tests cover failed native migration write/readback, mismatched receipts/readback, canonical u64 range, conflicting copies, storage write failure, concurrent inventory/token updates, origin-wide/alias exclusion, exact cleanup, offline capability failure, separate proxy readiness, out-of-order refresh, forget/re-pair/captured callbacks, re-pair during migration and sanitized invoke exceptions. Browser routing, host drawer and zero-config security behavior passed through their existing component tests. Native UI interactions remain fixture-driven rather than real IPC/desktop execution.

## Artifacts and source hashes

All artifacts are under `docs/evidence/paired-daemon/` with `A13-desktop-state` prefix: this report; command-contract.md; source.sha256; RED.log/.exit; GREEN.log/.exit; final-GREEN.log/.exit; build.log/.exit.

`A13-desktop-state-source.sha256` records exact SHA-256 for the five changed source/test files. No commits, deployments, releases, desktop automation/launch, daemon operations, credential-file reads or Rust edits occurred.

## Assumptions and remaining integration

- Migration token access is transient within the migration call. Original legacy storage remains pending until verified cleanup; it is not restored into renderer state. Native durability/readback is an explicit IPC contract, not established by mocked tests.
- Cleanup uses synchronous compare-before-write after async native work. localStorage provides no cross-process CAS/transaction; native desktop bootstrap is assumed to be the migration writer, while this adapter refuses changes it observes. An independently executing legacy WebView can race the synchronous comparison/write itself; eliminating that platform-level window requires a coordinated writer/transactional storage design outside this scoped adapter. No React state writes compete with migration in this implementation.
- Forget callers must obtain user confirmation before calling the adapter. Actual UI pairing/forget controls and machine action consumers are intentionally not redesigned here. `capture(hostId)` supplies a callback-current predicate; native cancellation remains mandatory.
- Parent owns command registration, durable native readback and idempotent migration, native origin/redirect policy, grant/cancellation semantics, capability mapping, and real-surface integration. A13 remains pending that integration and its broader acceptance gates.
