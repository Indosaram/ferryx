VERDICT: BLOCK

## FINDING-BY-FINDING

- **B1 — ARTIFACT.** `cmd_browser_snapshot_capability` is registered in `src-tauri/src/lib.rs`, wrapped by `getBrowserSnapshotCapability()` in `ui/src/lib/browserTauri.ts`, and consumed by `BrowserToolbar.tsx`. The original dead-plumbing finding does not stand.
- **B2 — RESOLVED.** The no-child Linux descriptor now returns `RootWebviewWindow` with `pointer_transparent: false` and `layer_backed: false`; composition rejects it, and the test would fail if the fabricated child descriptor were restored.
- **B3 — NOT RESOLVED.** The normal-path ordering is corrected, but listener setup still returns `None` on bind/configuration/publication failure and the caller ignores that result before sending readiness. A daemon can therefore announce ready with no rendezvous record or usable listener.
- **B4 — RESOLVED.** When `/bin/bash` and `/bin/sh` are absent but PATH contains `sh`, the resolver returns the executable name `"sh"`, not nonexistent `"/bin/sh"`. The test pins this return value.
- **B5 — NOT RESOLVED.** Endpoint deletion is gated by `StaleDaemonTermination`, but `is_process_alive_windows()` maps both “PID absent” and “`tasklist` failed/unreadable” to `false`. The initial polling loop consequently reports `Terminated` and permits deletion when process status is merely unknown.
- **P1-1 — RESOLVED.** The text read is gated on advertised text types, and `classify_linux_clipboard()` makes image win when only image types are advertised, even if lossy garbage text is supplied.
- **P1-2 — RESOLVED.** The agent-state endpoint is now one port/token record, staged and renamed, and current plus legacy stale rendezvous files are cleared before binding.
- **P1-3 — RESOLVED.** Externally managed updates now use separate Windows, Linux, and neutral messages rather than always naming the Microsoft Store.
- **P1-4 — NOT RESOLVED.** The new tests exercise publication and parsing, but none asserts that readiness occurs only after successful listener publication. Moving `spawn_agent_state_listener()` back below the ready signal would leave all shown tests passing. The Windows CI command also filters the lib test binary to `worktree`, so these rendezvous tests are not executed there.
- **P1-5 — RESOLVED.** The frontend takes the complete leading quoted segment as the executable path; the added tests cover spaced and unterminated quoted Windows paths.

## NEW BLOCKERS

none

The block verdict is caused by unresolved B3 and B5.

## NEW P1s / NOTES

1. **P1 — `src-tauri/src/daemon/client.rs:1418-1456`: process-query failure is treated as confirmed termination.**  
   `tasklist_image_name_windows()` returns `None` for all of:
   - the PID no longer existing,
   - failure to spawn `tasklist`,
   - non-zero `tasklist` exit,
   - malformed/non-UTF-specific output with no matching row.

   `is_process_alive_windows()` reduces that to `false`, and the first polling loop immediately returns `StaleDaemonTermination::Terminated`. This defeats the stated rule that unreadable `tasklist` evidence must preserve the endpoint. The query needs a three-state result such as `Alive(image) | Absent | Unknown`; only `Absent` may establish termination.

2. **P1 — `src-tauri/src/daemon/server.rs:1818-1870, 2197-2209`: readiness is not conditional on agent-state listener success.**  
   The non-Unix listener can fail during bind, `set_nonblocking`, address lookup, Tokio conversion, or rendezvous publication. Every failure returns `None`, but `run()` discards it and still sends `ready_tx`. Additionally, `agent_state_endpoint` is assigned before publication, so publication failure leaves the in-memory capability claiming an endpoint that was never made discoverable. Listener establishment should return `Result`, publication should precede setting the endpoint, and startup readiness should be withheld or failed when the required ingress cannot be published.

3. **P1 — `src-tauri/src/daemon/server.rs:2167-2177` and `src-tauri/src/daemon/client.rs:136-157`: daemon port and transport token are published as an uncoordinated pair.**  
   The server writes `daemon.port` first and then `daemon.token`; the client reads them independently. During startup, a client can read the new port with a stale token from the previous boot and be rejected. No stale-token removal is shown. Treat the port file as the publication marker: clear stale token state, write the new token first, and publish the port last, or publish both values in one atomic rendezvous record.

4. **P1 — `src-tauri/src/terminal/foreground.rs:49-106`: Windows executable normalization remains case-sensitive.**  
   `executable_name()` only strips lowercase `.exe`, and `is_agent()` compares names without lowercasing. Commands such as `Claude.EXE`, `NODE.EXE`, or `C:\...\Copilot.exe` can therefore remain unrecognized on Windows. The frontend implementation correctly uses case-insensitive suffix removal, but the Rust observer does not. The new Rust tests only use lowercase names and do not catch this mismatch.

5. **P1 — `src-tauri/src/ipc/native_terminal.rs:518-549`: clipboard type discovery and text retrieval can switch display backends.**  
   Type discovery uses `wl-paste ... .or_else(xclip ...)`, then text retrieval independently repeats `wl-paste ... .or_else(xclip ...)`. Types can therefore come from X11 while bytes come from Wayland, or vice versa. The advertised-type gate then validates one clipboard while decoding another. Select one successful backend during type discovery and use that same backend for the subsequent typed text read.

6. **NOTE — `src-tauri/src/browser/linux.rs:20-78`: per-window overlay entries have no removal path.**  
   The map fixes cross-window parenting, but entries are never removed when a window is destroyed. Reusing a label can return a stale GTK overlay, and closed secondary windows remain retained for the process lifetime. This is Linux-only and not covered by the keying test, which tests a plain `HashMap` rather than window lifecycle.

7. **NOTE — `src-tauri/src/daemon/server.rs` rendezvous replacement test is not Windows-semantic coverage.**  
   `rendezvous_publish_writes_the_port_and_the_token_as_one_file` republishes over an existing destination and runs on Unix because the helper is enabled by `cfg(test)`. That does not establish the behavior of `fs::rename` over an existing destination on Windows. Production startup currently clears the destination first, so this does not by itself break the normal boot path, but the test’s “replaces in place” claim is not portable evidence.

8. **NOTE — macOS shell behavior is unchanged by the shown resolver change.**  
   The modified fallback is in the Linux planning arm; the return-value correction from `"/bin/sh"` to `"sh"` does not alter the macOS shell path.

9. **NOTE — macOS agent-state startup ordering changed, but no direct regression is demonstrated in the supplied diff.**  
   The Unix listener, foreground observer, and extension installation now run before readiness. That removes the deterministic pane-start race, but it also makes their synchronous work part of startup latency. The shown tests do not pin the Unix listener’s bind-before-ready property or failure behavior. The new non-Unix rendezvous parsing/environment code is cfg-gated away on macOS.

10. **NOTE — `ui/src/lib/shortcuts.ts:607-620` introduces a possible macOS detection edge.**  
    macOS detection now requires `navigator.maxTouchPoints === 0`. WKWebView normally reports zero, but an environment where the property is absent would no longer be identified as macOS unless `process.platform === "darwin"` is exposed. Using `(navigator.maxTouchPoints ?? 0) === 0` would avoid that regression without misclassifying touch-capable iPadOS.

## EVIDENCE

I read the supplied diff sections for:

- Browser snapshot capability definition, registration, frontend wrapper, toolbar consumption, and toolbar tests:
  - `src-tauri/src/browser/screenshot.rs`
  - `src-tauri/src/lib.rs`
  - `ui/src/lib/browserTauri.ts`
  - `ui/src/components/BrowserToolbar.tsx`
  - `ui/src/components/BrowserToolbar.devtools.test.tsx`
- Linux compositor descriptors and composition validation:
  - `src-tauri/src/native_terminal/platform/linux.rs`
  - `src-tauri/src/native_terminal/composition.rs`
- Agent-state listener, rendezvous publication, startup ordering, and PTY environment reader:
  - `src-tauri/src/daemon/server.rs`
  - `src-tauri/src/terminal/pty.rs`
- Linux shell fallback:
  - `src-tauri/src/terminal/shell.rs`
- Windows stale-daemon termination and endpoint deletion:
  - `src-tauri/src/daemon/client.rs`
- Linux clipboard classification and helper execution:
  - `src-tauri/src/ipc/native_terminal.rs`
  - `src-tauri/src/clipboard_image.rs`
- Update ownership/backend and UI messaging:
  - `src-tauri/src/ipc/updater.rs`
  - `ui/src/components/settings/GeneralSection.tsx`
- Frontend and Rust command-line parsing:
  - `ui/src/lib/agentSessionDiscovery.ts`
  - `ui/src/lib/agentSessionDiscovery.test.ts`
  - `src-tauri/src/terminal/foreground.rs`
- CI test selection:
  - `.github/workflows/build-test.yml`
- macOS-adjacent startup, shell, drag-region, shortcut, notification, and filesystem-lock changes in the supplied diff.

I did not execute the repository or inspect files outside the supplied wider diff; test/build conclusions above distinguish the code I read from the execution evidence supplied in the prompt.