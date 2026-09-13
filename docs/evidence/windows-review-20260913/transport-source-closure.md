# Terminal transport: five-path source closure

2026-09-13. Source review only; no tests, runtime checks or repairs executed.
Observed HEAD: `ab08b94fdeda5039982fc8a37e8bc36885426667`.
These five paths were `not-selected` in the existing census and had no
terminalTransport references in the existing Markdown audit reports.
This closes this bounded source-reading obligation, not exhaustive coverage.

## Full-file receipts

All paths below are under `ui/src/lib/terminalTransport/`. Inclusive read
ranges cover the entire files. SHA256 values bind current working bytes.

- `index.ts:1-3`: `9006e1c9c38e80d120de19e34341f1ec2302842c18c689e2cb1d5df3fb79418a`.
  Re-exports interfaces and both implementations; no transport selection.
- `types.ts:1-26`: `40d6472a2df19dffe4ceca007fe5ec0ffefacb2bd8d55f503a6231ad8e788841`.
  Shared session, replay, input, lifecycle and unsubscribe signatures.
- `tauriTransport.ts:1-89`: `588de44a042be0faa6678499d19d0d598cb38bf75870df1ddd65226c1812c28e`.
  Read session mapping, history decode, string/byte conversion, IPC forwarding,
  output subscription startup and deferred lifecycle cleanup.
- `remoteTransport.ts:1-114`: `9b0338fa8deadde714bff2e9570ef0aba7e794acfc4b66cdc7a58407068bf059`.
  Read bearer-header session listing/ticket exchange, socket creation,
  output listeners, ready-state guarded input, resize/signal, close and
  no-op lifecycle subscription.
- `terminalTransport.test.ts:1-176`: `5439096a62ccc8bed0708134a8b5743fafd2f68c079f32ec4aba4497e4f7298b`.
  Read all eight cases and their mocks. No fixed sleeps. Method-existence
  checks do not prove WebSocket behavior; the last case never attaches.

## Actual callers and limits

LSP references omitted test imports and the restore consumer. They were
therefore insufficient as a negative caller oracle. Corrected explicit
AST rules scanning identifiers and type identifiers across `ui/src`
returned 21 TypeScript occurrences in five files and one TSX occurrence
in `remote/zeroConfigSecurityProbe.test.tsx`; neither result was truncated.
Earlier invalid-argument and missing-kind searches failed and supply no
negative evidence.

The five TypeScript files with references were the two implementations,
the interface, the transport test and `state/workspaceRestore.ts`.
Directly read `workspaceRestore.ts:1-150`: its native Tauri branch at
74-82 calls `listTerminalSessions` directly. The non-Tauri fallback at
83-89 uses `defaultTauriTransport.listSessions`. No production use of the
other adapter methods was established by this scoped reference search.

Directly read `zeroConfigSecurityProbe.test.tsx:145-213`: the TSX reference
dynamically imports WebSocketTerminalTransport and exercises ticket-based
socket construction in a dedicated test, not in the mounted remote app.
No shipped WebSocket adapter consumer was established. This is not proof
against arbitrary namespace/dynamic loading outside the searched scope.

Consequently, byte-decoding loss, repeated-attach socket ownership, silent
pre-open writes and missing remote lifecycle forwarding are limitations
of these adapter implementations, not newly confirmed Windows user-facing
failures. Do not change the shipped remote client on that inference.

## TRANSPORT-TEST-01: session-list assertion omits running

Confirmed test/implementation mismatch; subsequent macOS RED is recorded
in `red-green.md`. Not a Windows terminal product defect. The fixture at test lines 19-24
omits `running`. The real mapping at tauriTransport lines 17-22 always adds
`running: s.running ?? true`. The exact-object assertion at test lines
30-32 omits that key. Vitest configuration was read in full: this test is
not platform-excluded, and mocks are cleared/restored between cases.
Global setup was also read; it does not replace this mapping or assertion.

Required future invocation:
`bun run --cwd ui test src/lib/terminalTransport/terminalTransport.test.ts`.
Registered in C002 at 2026-09-13T03:30:46.402Z through the official CLI;
the saved scenario exactly matched the original plus this extension.
Reproduce the current missing-key mismatch through the real mapper.
Preserve full object equality while testing both absent-running -> true
and explicit-running false -> false, including epoch/path identity.
A mutation that drops running or converts false to true must be rejected.
This is contract-oracle correction, not permission to delete or relax the
test or remove running from production output.

The native Windows test runner is this validator's affected surface.
Existing C3 combined frontend verification owns the obligation. The C002
extension owns only `terminalTransport.test.ts` expectation/default/false
cases; no adapter behavior change is authorized by this source observation.
Registration, existing-test macOS RED and corrected-oracle macOS GREEN are
complete; both loader-only mutations are rejected. Native Windows execution
remains pending. See `red-green.md` for exact runs and config cleanup.

## State and preservation

Five files total 408 lines. No production/test/config changes, branch,
worktree, desktop, daemon, network or process lifecycle actions occurred.
Foreign tracked diff remained 20 files, 371 insertions and 38 deletions.
Implementation remains blocked on explicit isolation approval. Existing
RED/GREEN, Windows GUI, cleanup, gate, PR disposition and push requirements
remain pending. This report must not be used as a passing runtime receipt.
The statements above describe the initial source audit. A later isolated
Vitest process reproduced the registered mismatch without source changes;
see `red-green.md` for the process and preservation receipt.
