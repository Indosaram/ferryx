# Frozen scoped contract seed - 2026-09-05

Status: declarations compiled and wire tests passed; no producer implementation or integration claimed.
Assignment paths override section 3's prospective contracts paths. Import these owned modules directly;
do not copy their DTOs or create a competing barrel.

## Import surfaces and ownership

- Rust producer: `use crate::scoped_contracts::{TargetRef, ...};`
- External Rust consumer: `use ferryx_lib::scoped_contracts::{TargetRef, ...};`
- UI producer at `ui/src/features/ferryx/<domain>/`: `import type { TargetRef, ... } from "../../../lib/scopedContracts";`
- Rust root registration: only `pub mod scoped_contracts;` in `src-tauri/src/lib.rs`.
- Rust isolated inclusion: the self-contained `scoped_contracts.rs` uses only serde/serde_json;
  `#[path = "scoped_contracts_tests.rs"] mod tests` is gated by `cfg(test)`.
  A producer's isolated harness may path-include this module as `scoped_contracts` without the app tree.
- UI isolated inclusion: direct module imports and standard `*.test.ts` Vitest discovery; no runner configuration changes.

| Consumer | Exact shared exports |
| --- | --- |
| All six | `Epoch`, `TargetRef`, `MutationEnvelope<P>`, `ScopeErrorCode`, `ScopeError<D>`, `ScopeResult<T,D>`, `ScopeCapability` |
| control | `InventoryCompleteness`, `InventorySnapshot<T>`, `TransitionKind`, `TransitionSource`, `InventoryTransition`, `ControlLease`, `ScopeEvent<T>`, `EventReplay<T,S>` |
| ssh | `RunTarget`, `TargetRef`, `ScopeCapability`, `AttachmentReceipt`, `EventReplay<T,S>` |
| history | `CanonicalProvider`, `ConversationClaimKey`, `ConversationOwner` |
| design | `ConfirmedDraft`, `AttachmentReceipt`, `AttachmentMediaType`, `DeliveryStage`, `DeliveryReceipt` |
| chat | `CanonicalProvider`, `ConversationClaimKey`, `ConversationOwner`, `ChatDraft`, `AttachmentReceipt`, `ScopeEvent<T>`, `DeliveryReceipt` |
| push | `InventoryTransition`, `InventorySnapshot<T>`, `TargetRef`, `TransitionKind`, `TransitionSource` |
| staging/design/chat | `ATTACHMENT_MAX_FILE_BYTES`, `ATTACHMENT_MAX_FILES_PER_TURN`, `ATTACHMENT_MAX_TURN_BYTES`, `ATTACHMENT_UNREFERENCED_TTL_MS` |
| Wire support | Rust `WireBool<true/false>` fixes result discriminants; TS `JsonValue` bounds default error details |

Rust success construction: `ScopeResult::Success { ok: WireBool, data, request_id }`.
Rust failure construction: `ScopeResult::Failure { ok: WireBool, error, request_id }`.
Rust `Epoch(u64)` serializes as a decimal string; deserialization rejects numbers, leading zeroes,
signs, whitespace, fractions and overflow. TS `Epoch` is a string DTO declaration, not a runtime parser.
Other numeric fields use JSON numbers; adapters must constrain counters to JS safe integers.
Optional mutation target is omitted, not null; nullable provider request identity remains explicit.
`RunTarget::default()` is Local; applying it to absent persisted project values remains integrator work.

## Existing DTO reuse / deliberately absent implementation

Existing daemon `AgentProviderSession`, `AgentProviderSessionKey`, and `TerminalStartup::AgentResume`
remain authoritative for native resume. UI counterparts remain in `lib/types.ts` / `lib/agentResume.ts`.
No overlapping resume DTO or transcript path exposure was added. Existing remote auth `DevicePermission`
and device tokens remain authoritative; no second permission/token type was introduced.

No service traits or fake success services, producer trees, transport messages, stores, global UI wiring,
dependencies, helper binaries, capture adapters or push senders were added. Capability declarations are
not negotiated by the current daemon. Registry atomicity/handoff, lease enforcement/revoke,
per-client unread, inventory subscription atomicity, retained ordering/gaps, request dedupe, HTTP status
mapping, provider-confirmed completion, attachment validation/jailing/staging/cleanup, delivery evidence,
draft persistence keys and revocation remain producer/integrator behavior, not guarantees made by DTOs.
`ConfirmedDraft` freezes the routing/generation/note/attachment receipt shape; it does not implement confirmation.

## RED evidence

Command (compiling baseline; same command later GREEN):

```sh
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target CARGO_BUILD_JOBS=8 cargo test --manifest-path src-tauri/Cargo.toml --lib scoped_contracts::tests -- --nocapture
```

Initial invocation timed out during compilation after 200 seconds, with no test result; not RED.
After confirming no matching Cargo/rustc process remained, the bounded retry compiled in 3m12s.
It collected two tests, exited 101:

```text
target_roundtrip_preserves_full_u64_epoch_as_string ... FAILED
left: ... "epoch": Number(18446744073709551615) ...
right: ... "epoch": String("18446744073709551615") ...
epoch_rejects_noncanonical_or_out_of_range_wire_values ... FAILED
panicked ...: 1
test result: FAILED. 0 passed; 2 failed; 0 ignored; 555 filtered out
```

These failures preceded the string codec implementation; neither was an import/setup failure.
Other DTOs are declaration seeds with codec compatibility tests, not claimed behavioral RED.
TS is declarations/constants only; no TS behavioral RED is claimed.

## GREEN evidence

The Rust command above completed in 25.71s, exit 0:

```text
running 5 tests
target_roundtrip_preserves_full_u64_epoch_as_string ... ok
epoch_rejects_noncanonical_or_out_of_range_wire_values ... ok
result_rejects_mismatched_discriminants ... ok
shared_envelopes_roundtrip ... ok
producer_boundaries_roundtrip ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 556 filtered out
```

Warnings remained outside owned modules: 11 total in native input/font manager, notification permission,
IPC notifications, terminal session and worktree manager. None was suppressed or modified.

```sh
bun run --cwd ui test src/lib/scopedContracts.test.ts
```

Vitest 3.2.7: `Test Files 1 passed (1)`, `Tests 2 passed (2)`, exit 0, duration 979ms.

```sh
ui/node_modules/.bin/tsc --noEmit --strict --skipLibCheck --target ES2020 --module ESNext --moduleResolution bundler ui/src/lib/scopedContracts.ts ui/src/lib/scopedContracts.test.ts
ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json
```

Both exited 0 with no diagnostics. LSP globally unavailable per assignment; not retried.
Rust tests compile the real lib target with default features; no all-targets or desktop launch.
No UI visual surface changed, so no visual/manual desktop validation is claimed.

## Cleanup and scope receipt

No QA daemons, listeners, PTYs, provider processes, temporary roots or credentials were created.
Only focused test/compiler processes ran and completed; the initial timed-out Cargo invocation had no
remaining matching Cargo/rustc process at inspection. The existing warm Cargo cache was reused;
no cache deletion, staging, commit, git config change or Shutdown was performed.
Foreign dirty files, including the lead's concurrent remote listing test, were not edited.
Work remains uncommitted in the shared tree and subject to concurrent sessions.
