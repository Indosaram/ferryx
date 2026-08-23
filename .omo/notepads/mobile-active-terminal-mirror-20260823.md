# Mobile Active-Terminal Mirror — Delegated Web DAG

Date: 2026-08-23

## Confirmed current behavior

- `GET /api/v1/workspace/state` in `src-tauri/src/remote/server.rs` enumerates every running/starting PTY from `TerminalService`; this is why inactive desktop terminals appear on mobile.
- The desktop authority for the focused terminal is frontend layout state: `LayoutState.activeTabId`, the active tab's `TabPaneLayout.activeLeafId`, and `sessionIdsByLeafId`; the local session then maps to `TerminalSession.backendSessionId`.
- Current `RemoteSessionList` only groups PTY metadata. It cannot alter desktop workspace/worktree state and has no desktop-active terminal contract.

## Chosen product contract

1. The mobile app mirrors exactly one desktop terminal: the focused leaf in the desktop's active tab group. It must never enumerate background PTYs or permit an inactive-session WebSocket attach.
2. Mobile receives a safe desktop context containing the active workspace, active worktree label, and active backend terminal session. Local absolute paths never cross the Remote DTO boundary.
3. Mobile workspace/worktree selection is a desktop control surface, not a disconnected local filter: selecting a registered workspace/worktree asks the desktop shell to activate or open that worktree, then mobile refreshes and mirrors its resulting focused terminal.
4. The mobile UI uses a compact desktop-style context switcher: current workspace/worktree breadcrumb, chooser affordance, active-row state, and a pending transition state. The terminal itself stays singular.

## Required DAG topology

### Node: `web-fanout` (fresh Web batch; code producer)

**Read scope:** current Remote server/protocol/state, Tauri IPC, desktop `App`/layout/types, and `ui/src/remote` tests/components.

**Direct write scope:** none for the DAG coordinator. It must invoke exactly one `delegate_to_chatgpt_web --batch-stdin --json --progress-json` helper batch. The three Web tasks have non-overlapping ownership:

1. **Native active-context contract** — may write only `src-tauri/src/remote/**`, `src-tauri/src/ipc/remote.rs`, `src-tauri/src/lib.rs`, and Remote-native tests. Add a typed active-desktop-context state and safe Remote APIs that return/attach only the declared current session; add the authenticated mobile workspace/worktree selection request path and desktop-event bridge. Capture RED then GREEN native tests.
2. **Desktop context publisher** — may write only `ui/src/App.tsx`, `ui/src/lib/tauri.ts`, and their direct tests. Derive the focused leaf's backend session from live normalized layout; publish it whenever desktop project/worktree/tab/pane focus changes; receive a Remote selection event and use existing project/worktree actions to activate the requested context. Capture RED then GREEN UI tests.
3. **Mobile mirror selector** — may write only `ui/src/remote/**`. Replace the multi-session accordion with the one-terminal mirror plus a desktop-grade workspace/worktree switcher that requests desktop selection, shows current/pending context, and never presents a background session. Capture RED then GREEN Remote UI tests.

The helper result must include every retained `scope_id`, every `event:"dispatched"`, and every terminal worker record. No node may edit repository source outside its assigned scope or replace Web work with a local coding implementation.

### Node: `verify` (final fan-in verifier)

**Read-only scope:** all changed code, test output, UI build output, and final browser-equivalent render evidence. No source or Web-scope writes.

**Depends on:** `web-fanout`.

**Required checks:** targeted Rust active-context/attach authorization tests; targeted desktop/Remote React tests; `npm run build`; `cargo fmt --check`; `cargo check`; LSP diagnostics; and permitted browser QA of the built Remote UI with mocked/current API state. The node must report exact commands and exit codes.

### Scope lifecycle

Keep each returned Web scope retained through verification. If verification fails, a follow-up DAG run must resume the exact affected scope with `--resume-scope`; no generic OMO coding fallback. Close scopes only after the final verifier accepts every requirement.

## Completion evidence

- Native test proves background live PTYs are excluded from list/state and denied direct WebSocket attachment.
- Desktop test proves focused pane transitions publish the matching backend session and incoming mobile workspace/worktree selection reaches existing desktop selection behavior.
- Remote UI test proves one active terminal, a selected workspace/worktree context, selector request/pending/result behavior, and no absolute paths.
- Build, compiler/LSP, browser-equivalent QA, and an in-repo audit are green. Desktop manual QA remains user-run under the no-desktop-control constraint.

## Delegation outcome — blocked before source changes

- Native DAG: `dag_81dcd13a-11fa-4271-bbe6-f5f4b0bed86b`.
- Its required single fresh helper batch was invoked once at `/Users/indo/code/project/omo-bridge/target/release/delegate_to_chatgpt_web --batch-stdin --json --progress-json` with the three disjoint Native/Desktop/Mobile tasks described above.
- Authoritative helper preflight failed before scope creation: `gpt2omo is not reachable at http://127.0.0.1:18800` (`Connection refused (os error 61)`). The helper exited 1, emitted no `dispatched` or `terminal` records, created no retained scope IDs, ran no worker tests, and changed no source files.
- The DAG verifier's claimed PASS was rejected by direct source inspection: `list_sessions` and `get_workspace_state` still enumerate `TerminalService::list_sessions()`; `ws_terminal_handler` still permits any extant PTY; `RemoteApp`/`RemoteSessionList` still render the flat source result grouped as project/worktree accordions; and no desktop focused-layout publisher or mobile selection API exists.
- Binding `delegate-web-dag` stop condition therefore applies: do not bypass the unavailable helper with local coding or retry blindly. Resume only after the gpt2omo bridge on port 18800 is available, then invoke one fresh helper batch (no existing scopes exist).
