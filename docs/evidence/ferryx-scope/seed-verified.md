# Seed verification before producer work

2026-09-05; hephaestus task `st_01a07106`.

## Verdict

**Contracts: VERIFIED independently and frozen for producer imports. All six
domain producers can start against the listed exports. QA harness: BLOCKED
separately on its ambient runner. All product-surface criteria remain OPEN.** Full product
driver integrations remain future work. Earlier focused suite passes below used
`--no-preload` for QA and do not certify the ambient command. Rust tests exercise production DTO
serialization, not duplicate test DTOs. There are no placeholder service success
implementations in the seed. The QA driver does not fabricate product PASS or
push delivery. It is a fixture/self-test and read-only API probe, not a completed
scenario driver. The defects below prevent certifying unconditional isolation and
cleanup. No production changes or feature completion are claimed.

The lead independently confirms Rust contract tests 5/5 PASS, TS contract tests
2/2 PASS, and all 39 remote regression tests GREEN. Contract producer status is
not blocked by the QA producer's ongoing Bun.spawn investigation. This report
does not approve the ambient harness or any product surface. Missing future UI
integrations are also separate from the present runner blocker.
Lead reports contracts committed as `299af38` and remote-list HTTP/PTY RED/GREEN
plus 39 passing regressions committed as `3791ce1`; these are lead-provided
receipts, not additional executions or commits by this verifier.

Known future harness actions, not approval or completed work:
- Bind an isolated production router and authenticated fixture resources.
- Exercise mutation lifecycle, retained events, leases and exact-target routing.
- Drive SSH sever/reconnect and history/provider-authored resume callbacks.
- Drive isolated browser/native capture actions and real pixel assertions.
- Exercise mobile two-client ownership, approvals/questions, drafts/files/IME.
- Verify real background push and notification return on physical devices.
- Cover curl isolation, independent failure-path cleanup, bounded readiness and
  identity-header parsing defects documented below before trusting those paths.

## Latest ambient-run correction: blocked, supersedes earlier harness GREEN

Lead reports `bun test scripts/qa/ferryx-scope-qa.test.mjs` failed 3 of 7 tests at
Bun.spawn. That exact failure output was not supplied here. Current producer
correction is present: driver curl, nested boundary runner and test invoke helper
now explicitly pass `stdin: 'ignore'`. API curl retains `stdin: 'pipe'` for auth.
No producer source or test was edited by this verifier.

This verifier executed the requested command without `--no-preload`, without
altering environment/configuration, and without substituting a different runner:

```sh
bun test scripts/qa/ferryx-scope-qa.test.mjs
```

Exact observed command output:

```text
bun test v1.4.0 (34cbb9a40)
```

The enclosing tool returned `Command timed out after 60 seconds`. No tests or
assertions were reported; no exit-code marker was reached. This is **NOT GREEN**,
not a reproduced 3/7 assertion failure, and not behavioral RED evidence. Ambient
preload causality is unverified: no bunfig was found at the checked repository,
UI, ancestor or standard user paths, and no relevant BUN_OPTIONS/config variable
was reported. No preload was disabled or modified to manufacture a pass.

Immediate cleanup inspection found no matching QA test/driver/boundary process
and no `/private/tmp/ferryx-qa.*` roots. The pre-run root listing was also empty.
No fixture listener or root receipt was emitted because collection never became
observable. No foreign PID was signalled. No Cargo job was started; queued Rust
verification and the lead's remote regression work remain untouched.

Current blocker: the owner must establish a completing ambient QA test command
with literal results. Earlier no-preload GREEN and declaration-only contract
checks cannot substitute for it. Full product scenarios remain unimplemented
independently of this runner failure. No blind retry or production fix was made.

## Producer import contracts

- Internal Rust: `use crate::scoped_contracts::{TargetRef, ...};`
- External Rust: `use ferryx_lib::scoped_contracts::{TargetRef, ...};`
  Cargo names the library `ferryx_lib`; `src-tauri/src/lib.rs:11` registers
  `pub mod scoped_contracts;`.
- UI files directly under `ui/src/features/ferryx/<domain>/`:
  `import type { TargetRef, ... } from "../../../lib/scopedContracts";`
  Constants use value imports. Deeper files must adjust relative depth.
- Isolated Rust harnesses can path-include the self-contained module; its tests
  are a sibling file included only under `cfg(test)`. No isolated harness was
  created in this verification; the real library target was tested instead.

| Producer | Shared exports to consume, not redeclare |
| --- | --- |
| All six | `Epoch`, `TargetRef`, `MutationEnvelope<P>`, `ScopeErrorCode`, `ScopeError<D>`, `ScopeResult<T,D>`, `ScopeCapability` |
| control | `InventoryCompleteness`, `InventorySnapshot<T>`, `TransitionKind`, `TransitionSource`, `InventoryTransition`, `ControlLease`, `ScopeEvent<T>`, `EventReplay<T,S>` |
| ssh | `RunTarget`, `TargetRef`, `ScopeCapability`, `AttachmentReceipt`, `EventReplay<T,S>` |
| history | `CanonicalProvider`, `ConversationClaimKey`, `ConversationOwner` |
| design | `ConfirmedDraft`, `AttachmentReceipt`, `AttachmentMediaType`, `DeliveryStage`, `DeliveryReceipt` |
| chat | `CanonicalProvider`, `ConversationClaimKey`, `ConversationOwner`, `ChatDraft`, `AttachmentReceipt`, `ScopeEvent<T>`, `DeliveryReceipt` |
| push | `InventoryTransition`, `InventorySnapshot<T>`, `TargetRef`, `TransitionKind`, `TransitionSource` |
| Attachment consumers | `ATTACHMENT_MAX_FILE_BYTES`, `ATTACHMENT_MAX_FILES_PER_TURN`, `ATTACHMENT_MAX_TURN_BYTES`, `ATTACHMENT_UNREFERENCED_TTL_MS` |

Rust result constructors additionally import `WireBool`; TS exports `JsonValue`.
`Epoch(u64)` emits canonical decimal strings, including u64::MAX. TS `Epoch` is
only a string alias, not a parser. Other u64 fields emit JSON numbers: producers
must enforce JS-safe integer limits at adapters. Attachment limits are 10 MiB per
file, four files, 20 MiB per turn, 24-hour unreferenced TTL. These constants do not
implement staging or expiry. `RunTarget::default()` is local; persisted-value
migration is not implemented by that default. Specifically,
`producer_boundaries_roundtrip` line 31 asserts serialization of
`RunTarget::default()` equals `{"kind":"local"}`; that assertion ran GREEN.
It does **not** deserialize a legacy project missing runTarget, and the TS test
constructs only an explicit SSH target. Legacy persisted-project local fallback
is therefore NOT verified by this seed. Producers must import the exact exported
names in the table; this is not evidence that future producer consumers compile.
Mutation target absence serializes
as omission; provider request identity serializes nullable. Identity requires all
four target fields, never only backendSessionId or the active selection.

The declaration modules contain no operations, service traits, registry, lease
enforcement, request dedupe, attachment validation, transport negotiation or push
sender. The seed document directs native resume consumers to existing daemon/UI
resume DTOs and remote consumers to existing auth types; those existing subsystems
were not independently audited here.

## What the tests actually prove

`scoped_contracts_tests.rs` imports `super::*` from the registered production
module. `roundtrip<T>` invokes `serde_json::from_value::<T>` then
`serde_json::to_value`; the full-u64 test constructs the actual `TargetRef`.
The five tests cover canonical epoch rejection, full routing identity, literal
result discriminants and selected envelope/producer variants. They do not cover
every export/variant (for example ConfirmedDraft, ControlLease, DeliveryReceipt
and event replay's events branch lack direct roundtrip cases).

The two Vitest tests import real constants and production TS types, then roundtrip
locally constructed JSON values. They are declaration/constant checks, not runtime
DTO parsing or a cross-language fixture-exchange test. Strict tsc also ran.

The QA suite runs real Bun children and ephemeral loopback HTTP requests. Boundary
tests prove five requests, no auth on health, auth afterward, real 404 rejection,
token redaction and identity mismatch stopping after one unauthenticated request.
All four scenario CLI invocations return blocked with listener cleanup. Authored
success and synthetic physical-device receipts cannot override acceptance.

## Concrete defects and blockers for the responsible producers/integrator

1. **Product scenario adapter absent (acceptance blocker, executed).**
   `probeApi` rejects the default null integration as `QA_API_BINDING_REQUIRED`;
   `validateEvidence` always throws even with authored pass evidence. There is no
   mutation/event lifecycle, actual provider callback, SSH bridge reconnect,
   history resume, native capture, browser interaction, mobile device exercise or
   physical push delivery. Read-only HTTP 200 actions are not scenario acceptance.
   No production binding was provided or contacted in this verification.
2. **Fixture-page curl does not disable curlrc (source-confirmed isolation gap).**
   `ferryx-scope-qa.mjs` launches its first curl without `--disable` or
   `--noproxy '*'`, unlike `ferryx-scope-api.mjs`. Private HOME alone is not a
   complete curl configuration boundary: platform curl can also locate a config
   through the account home fallback. A personal curlrc can alter proxy,
   redirects or output behavior. The actual run returned the isolated fixture,
   but unconditional no-foreign-network/no-foreign-output isolation is not proven.
   Owner remedy: apply the API curl's explicit configuration isolation to this
   invocation and add behavioral coverage before changing it. No personal curlrc
   was read or changed to reproduce this risk.
3. **Cleanup is short-circuited on close failure (source-confirmed).**
   Driver `finally` puts `page.close()` and owned-root cleanup in a single try.
   If close rejects, root cleanup is skipped. `startPage` also creates/listens to
   the frame before its try block, and its catch closes only the frame, not the
   main server if that server was acquired before an exception. Thus normal-path
   receipts do not prove all error-path resources are released. Resource-specific
   cleanup must run independently; failure injection remains unverified.
4. **Preparation test can leak after an assertion failure (source-confirmed).**
   The prepare/cleanup test in `ferryx-scope-qa.test.mjs` does not enclose the
   retained prepared root in a finally. A failed assertion before the cleanup
   invocation leaves it behind. Existing passing runs leave no new roots, but
   the suite does not guarantee cleanup when it goes RED.
5. **Readiness waits are not bounded and subscribe after triggering (source-confirmed).**
   `startPage` calls listen before `once(..., 'listening')`, and neither await has
   an explicit abort timeout. Normal Node scheduling worked in the observed run;
   this does not meet the requested subscribe-before-trigger/bounded-signal
   discipline. Driver child waits likewise have no driver-owned deadline beyond
   curl's five-second deadline; the external tests impose their own timeout.
6. **Valid last-position identity header is rejected (source-confirmed).**
   `probeApi` strips the `\r\n\r\n` delimiter, then requires a substring ending
   in `fixture.id + '\r\n'`. If the QA identity header is the final header, that
   trailing CRLF is outside the sliced header string. A valid fixture can receive
   `QA_API_IDENTITY_MISMATCH`. Existing Bun fixture ordering does not cover it.
   Parse header lines rather than requiring a trailing delimiter; not fixed here.

None of the isolation findings demonstrates live daemon adoption in this run.
The scripts inspected contain no product/daemon launch, PTY adoption, daemon
Shutdown or foreign-PID signalling. The manifest identity is a same-user trusted
fixture guard, not cryptographic process identity. Seed imports can proceed;
integrators must not promote fixture self-test success to product acceptance.

## Exact commands and results from this verification

GREEN, each focused test command ran once:

```sh
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target CARGO_BUILD_JOBS=8 cargo test --manifest-path src-tauri/Cargo.toml --lib scoped_contracts::tests -- --nocapture
```

```text
Blocking waiting for file lock on build directory
Finished `test` profile [unoptimized + debuginfo] target(s) in 0.73s
running 5 tests
test scoped_contracts::tests::target_roundtrip_preserves_full_u64_epoch_as_string ... ok
test scoped_contracts::tests::result_rejects_mismatched_discriminants ... ok
test scoped_contracts::tests::epoch_rejects_noncanonical_or_out_of_range_wire_values ... ok
test scoped_contracts::tests::shared_envelopes_roundtrip ... ok
test scoped_contracts::tests::producer_boundaries_roundtrip ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 556 filtered out
EXIT: 0 ELAPSED: 12.85
```

Eleven existing warnings remained: unused variable in native input, unused mut in
font_manager, four unnecessary unsafe blocks in notification permission, unused
app field in IPC notifications, unused wait_and_reap in terminal session, and
three unused writer-lease items in worktree manager. No warnings were suppressed.
Cargo serialized through its existing build lock; no alternate cache or killed
foreign build. Available disk was 14 GiB at the preflight, not the assigned 18 GiB.

```sh
bun run --cwd ui test src/lib/scopedContracts.test.ts
```

```text
$ vitest run --maxWorkers=1 src/lib/scopedContracts.test.ts
RUN v3.2.7
Test Files 1 passed (1)
Tests 2 passed (2)
Duration 1.03s
```

```sh
bun test --no-preload ./scripts/qa/ferryx-scope-qa.test.mjs ./scripts/qa/ferryx-scope-boundary.test.mjs
```

```text
bun test v1.4.0 (34cbb9a40)
10 pass
0 fail
43 expect() calls
Ran 10 tests across 2 files. [388.00ms]
EXIT: 0
```

The following checks each completed without diagnostics in this verifier's pass
(exit 0; not a claim about other concurrent sessions' typechecks):

```sh
ui/node_modules/.bin/tsc --noEmit --strict --skipLibCheck --target ES2020 --module ESNext --moduleResolution bundler ui/src/lib/scopedContracts.ts ui/src/lib/scopedContracts.test.ts
ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json
for file in scripts/qa/ferryx-scope-*.mjs; do node --check "$file" || exit; done
ui/node_modules/.bin/tsc --allowJs --noEmit --target esnext --module esnext --moduleResolution bundler --skipLibCheck scripts/qa/ferryx-scope-*.mjs
```

Lead follow-up reported foreign TypeScript errors. This verifier reran the two
tsc commands below against the current shared tree; both produced no diagnostics:

```sh
ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json
ui/node_modules/.bin/tsc --noEmit --strict --skipLibCheck --target ES2020 --module ESNext --moduleResolution bundler ui/src/lib/scopedContracts.ts ui/src/lib/scopedContracts.test.ts
```

Exact exit markers from that follow-up:

```text
PROJECT_TSC_EXIT=0
SCOPED_TSC_EXIT=0
```

The lead's foreign diagnostics were not supplied as literal output here and were
not reproduced; they remain lead-owned evidence, not suppressed or fixed. A green
observation in this session does not invalidate a failure in a different shared
tree snapshot. No test suite, Cargo job or live UI was rerun for this clarification.
The lead's remote server/tests and active regression run `bash_57` were untouched.

The JS tsc command is not strict checkJs coverage. LSP is globally unavailable per
assignment; no retry or tooling change. Rust's real library test compilation and
tsc replace LSP for this pass. No full app build or desktop launch was performed.

RED: no new RED run or production edit in this report-only assignment. Existing
historical RED receipts in `contracts.md` describe two production-codec failures
(numeric epoch vs string and noncanonical acceptance); `qa-driver.md` describes
unsafe-root/false-pass acceptance, missing child-test execution and missing bearer
auth failures. Those receipts were read, not independently replayed; no baseline
was reverted and no historical RED is claimed as newly observed. Current source
defects above remain unpatched, with no new regression test added to foreign files.

## Runnable entry and immediate cleanup receipt

```sh
bun scripts/qa/ferryx-scope-qa.mjs --self-test
```

Exit 0; `status: self-test-passed`, `productVerified: false`. It executed curl
against `http://127.0.0.1:54829`, receiving HTTP/1.1 200 and 1,215 HTML bytes.
Secondary iframe origin was port 54828. Body SHA-256:
`a93f26a4b9d94e401789b5720bebc8f7f6e598d8b8e4a18bfc04a65dc1413342`.
It actually spawned the boundary test runner: 3 pass, 0 fail, 11 assertions,
61 ms, exit 0. Browser executed false, screenshots empty.

```json
[
  {"event":"cleanup","loopbackServersClosed":2,"processesSignalled":[]},
  {"event":"cleanup","id":"e5988099-6177-44ec-834f-ebc87d1ae800","root":"/private/tmp/ferryx-qa.mGlz29","removed":true,"processesSignalled":[],"leadSshTouched":false}
]
```

The test/self-test wrapper compared `/private/tmp/ferryx-qa.*` before and after:
`NEW_RETAINED_QA_ROOTS: []`. Subsequent lsof showed no listeners on 54828/54829;
the named root was absent. Every launched command returned. No retained evidence
directory was requested; no temporary source, credentials, process logs or test
harness files were authored by this verifier. Existing Cargo cache artifacts were
retained intentionally. Error-path cleanup remains subject to the defects above.

## Scope and worktree receipt

Only authored path: `docs/evidence/ferryx-scope/seed-verified.md`, created with
apply_patch after fresh absence/status/diff checks. Root, Rust, UI and UI-lib
AGENTS were read, as were programming, frontend and impeccable skills. This was
pure contract/script verification, not a visual change; no visual detector or
browser was launched. Two early broad AGENTS discovery commands timed out after
20 seconds; narrower indexed discovery completed. Those were not test failures.

Foreign dirty production/tests/docs were preserved. No staging, commit, global
configuration, daemon Shutdown, default desktop, personal PTY, SSH bridge or
excluded feature work. This report is uncommitted in a shared working tree and
can be affected by concurrent sessions.
