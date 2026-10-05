# Round-3 backend compile repair: static-only report

## Result and evidence limits

The permitted backend source sites in the pass2 routing index have been repaired by reading and editing source. This is not a successful-compilation claim. One indexed backend source site remains blocked by the explicit prohibition against editing daemon/protocol.rs.

Nothing was executed by the agent: no shell command, eval, cargo command, check, test, build, or explicit LSP request. The apply_patch tool nevertheless invoked automatic LSP hooks, which returned timeouts or connection-refused errors. Those hooks were not requested or retried and provide no validation evidence. Static reads and AST searches were used to inspect construction sites.

All source coordinates below are the original pass2 diagnostic coordinates, not post-edit line numbers. Identical sites reported on multiple hosts and under different type spellings share the same repair. The authoritative routing index remains unchanged.

## Diagnostic mapping

### Imports, formatting, inference, and platform availability

| Original source | Hosts | Repair |
| --- | --- | --- |
| src/daemon/split_journal.rs:1:5 | Linux, macOS, Windows | Removed the absent fs2 import. |
| src/daemon/split_journal.rs:172:19 | Linux, macOS, Windows | Replaced lock_exclusive with std File::lock; Drop uses File::unlock. Preserved blocking exclusive locking rather than changing contention semantics. Cargo.toml now declares rust-version 1.89, the std locking minimum; evidence toolchains are 1.92, 1.97, and 1.98. No dependency added. |
| src/daemon/server.rs:102:24, :116:24 | Linux, macOS, Windows | Imported the real super::handover::HandoverManager. |
| src/daemon/server.rs:2704:33, :2726:62 | Linux, macOS | Same HandoverManager import repairs production rollback decision reads. |
| src/daemon/server.rs:100:13, :115:9 | macOS | The formerly unresolved recorded_decision calls now supply the concrete Result<Option<HandoverState>, String> required by rollback_transferred_readers; these inference diagnostics were downstream of the missing import. |
| src/daemon/server.rs:3135:59 | Linux, macOS, Windows | Implemented Display for IpcErrorCode using its existing Serde string representation, preserving SCREAMING_SNAKE_CASE and Custom string values. |
| src/terminal/service.rs:570:55 | Linux, macOS, Windows | Passed session_id directly to mark_running's impl Into<String> parameter; removed the ambiguous intermediate .into(). |
| src/terminal/pty.rs:192:9, :196:9 | Linux, macOS, Windows | Compared strings on both sides of the existing fixture assertions; normalize_process_cwd returns PathBuf, so its value is converted with to_str().unwrap(), matching the command-side fixture conversion. |
| src/terminal/shell.rs:647:13, :653:13 (Fn and FnOnce at each site) | Linux, macOS, Windows | Annotated the reusable environment closure parameter as &str, allowing the required higher-ranked argument lifetimes. No unsafe added. |
| src/daemon/handover.rs:517:20, :533:20, :516:80 | Windows | Made the platform-independent handover transaction state/ledger module available on Windows. Removed both its module/re-export Unix gates and file-level cfg(unix); actual Unix socket/descriptor transport remains gated. |
| src/daemon/server.rs:49:58, :55:25, :111:45 | Windows | Same transaction-module availability repair supplies HandoverState and HandoverTransaction to decision classification and fixtures. |
| src/daemon/server.rs:6735:13 | Linux, macOS | Added .. to the handshake test pattern, which asserts existing binary identity fields rather than split admission. |

### Handshake fixture literals

Each site below now explicitly carries capabilities: Vec::new() and admission_time_unix_ms: None, with a short legacy-fixture comment. Existing fixture identity, epoch, version, and binary fields are preserved. AST inspection found 11 client, 17 IPC, one reset-event, and two remote handshake construction sites.

| Original source | Hosts |
| --- | --- |
| src/daemon/client.rs:4500:37, :4584:37, :4672:37, :4820:25, :4906:25, :4956:29, :5098:29, :5289:27, :5366:27, :5701:29, :5813:29 | Linux, macOS |
| src/ipc/agents_reset_event_tests.rs:103:25 | Linux, macOS |
| src/ipc/tests.rs:1076:64, :1434:60, :1594:60, :1770:60, :1922:60, :2089:60, :2264:60, :2436:60, :2576:60, :2714:60, :2847:60, :2958:60, :3105:60, :3264:60, :3425:60, :3558:60, :3656:60 | Linux, macOS |
| src/remote/tests.rs:3883:50, :4205:50 | Linux, macOS |

### Spawn fixture literals

Each SpawnTerminalRequest fixture below now explicitly carries create_only: None, prepared_local_split: None, and remaining_ms: None, with an ordinary-spawn fixture comment. AST inspection found all 12 IPC construction sites. Existing startup commands and worktree authority values are preserved.

| Original source | Hosts |
| --- | --- |
| src/ipc/tests.rs:119:9, :199:9, :261:9, :462:9, :540:9, :600:9, :806:9, :850:26, :890:25, :961:31, :1188:19, :1259:19 | Linux, macOS |
| src/ipc/file_link_tests.rs:554:9 | Linux, macOS |

The backend DaemonRequest::Spawn fixtures at src/remote/workspace_api/worktree_authority_tests.rs:350:13 (all three hosts) and :589:35 (Linux and macOS) now explicitly carry local_split: None. These are ordinary authority/busy-worktree spawns, not prepared split attempts.

### Session-details construction sites

| Original source | Hosts | Repair |
| --- | --- | --- |
| src/ipc/tests.rs:1087:42 | Linux, macOS | Explicit incarnation: None with a legacy-session fixture comment. |
| src/ipc/file_link_tests.rs:11:5 | Linux, macOS, Windows | Explicit incarnation: None; file-link fixture does not model a split incarnation. |
| src/remote/tests.rs:3939:42, :4258:42 | Linux, macOS | Explicit incarnation: None with legacy fixture comments. |
| src/ipc/terminal.rs:2410:22 (both qualified and unqualified diagnostic spellings) | Linux, macOS, Windows | Explicit, documented absence of a local split incarnation for the paired proxy. Read the authoritative machine Session and RemoteTerminalTarget definitions: they carry machine ID, daemon epoch, and session ID, but no local split incarnation. No random or epoch-derived incarnation was invented. Existing epoch-qualified proxy identity is preserved. |

### Aggregate compiler failures

The routing index also labels aggregate compiler failure lines as backend entries. They are consequences, not additional source fixes, and cannot be marked passed without a compiler run:

- Linux, source null: lib test failures reporting 70 and 74 errors; lib failure reporting 10 errors.
- Windows, source null: lib test failures reporting 25 and 29 errors; lib failure reporting 13 errors.
- macOS, source null: lib test failures reporting 72 and 76 errors; lib failure reporting 10 errors.
- macOS, src/daemon/handover_socket.rs:316:9: aggregate lib failure reporting 10 errors, not a diagnostic against the socket code itself.

## Unresolved and cross-owner blockers

- src/daemon/protocol.rs:1814:13, Linux/macOS/Windows: the test's exhaustive DaemonRequest::Spawn pattern omits local_split (E0027). Although the index calls this backend-owned, the task explicitly excludes this file. It was read but not edited. The protocol owner must bind local_split (and assert the legacy value is None) or use .. if the test intentionally ignores it.
- Native-owner indexed diagnostics remain outside this repair: active_presentation_generation and active_presentation_epoch in native_terminal/surface_host.rs:971-972, and schedule_cancellation_receipt, try_claim, release_claim at :651, :706, :728, :778. The index also contains alternate qualified type spellings and a Windows native_terminal/input.rs:372:13 aggregate failure. No native source was edited and no native diagnostics are claimed resolved.

## Files touched by this session

All paths below are relative to the candidate root:

- src-tauri/Cargo.toml
- src-tauri/src/daemon/client.rs
- src-tauri/src/daemon/mod.rs
- src-tauri/src/daemon/handover_transaction.rs
- src-tauri/src/daemon/server.rs
- src-tauri/src/daemon/split_journal.rs
- src-tauri/src/ipc/agents_reset_event_tests.rs
- src-tauri/src/ipc/error.rs
- src-tauri/src/ipc/file_link_tests.rs
- src-tauri/src/ipc/terminal.rs
- src-tauri/src/ipc/tests.rs
- src-tauri/src/remote/tests.rs
- src-tauri/src/remote/workspace_api/worktree_authority_tests.rs
- src-tauri/src/terminal/pty.rs
- src-tauri/src/terminal/service.rs
- src-tauri/src/terminal/shell.rs
- .omo/evidence/local-pane-liveness-completion-replan/task-8/round3-backend-repair-report.md

No commit was created. The edits remain in the shared working tree. Nothing was executed by the agent; compilation and runtime behavior remain unverified.
