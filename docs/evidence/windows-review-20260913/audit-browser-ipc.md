# Windows browser / IPC audit - 2026-09-13

Status: COMPLETE (source review, not runtime certification). Lane: browser-ipc; task st_01a09849.
Reviewed working-tree source at HEAD b7ad4516. Foreign dirty files were read-only; no production, test, configuration, or existing documentation was changed.
The installed Windows executable is stale. Nothing here establishes its behavior or identity. No desktop, daemon, build, test suite, or runtime QA was launched.

## Method and coverage

Read root/backend/IPC/notification/remote and applicable UI AGENTS; programming, debugging, ast-grep skill instructions. Used LSP document symbols and references, ast-grep call-shape searches, and Bun to independently resolve locked dependency versions.
Dependency evidence below refers to installed registry sources matching Cargo.lock: tauri 2.11.5, tauri-runtime-wry 2.11.4, wry 0.55.1, notify-rust 4.18.0, tauri-winrt-notification 0.7.3, notification plugin 2.3.3.
All confirmed findings have source and current callers checked; failing-first probes are proposals, NOT executed results. For async probes subscribe before triggering and await the exact completion/event with a bounded timeout, never fixed sleeps.

| Coverage paths | Reviewed disposition |
|---|---|
| `src-tauri/src/ipc/browser.rs`; `browser/{manager,model,security,guest,cookies,download,find}.rs` | Entire command lifecycle, profiles, bounds, visibility, focus, zoom, reload/history, find, automation, imports, download and OS open. Findings B02-B05/B08/B10; clipboard unknown below. Linux-only overlay and macOS FFI excluded from Windows execution. |
| `ipc/browser_cli.rs`; `cli.rs` browser dispatch; `lib.rs` setup | Windows TCP and Unix comparison, port replacement, bounded request parsing, JSON errors, list/snapshot/act. B01. |
| `ipc/{notifications,permissions}.rs`; `notification/{mod,model,service,permission,audio,activation,notify_rust_adapter,badge}.rs`; `permissions/mod.rs` | Dispatch/preflight, sound, click routing, callbacks, settings URIs, badge support. B04/B12; no dialog deadlock established. macOS modules cfg-excluded. |
| `ipc/remote.rs`; `remote/{auth,state,server}.rs`; `remote/discovery/tailscale.rs` | Current desktop delegates to daemon, pairing identity/data paths, bearer auth, single-use WS tickets, revocation transaction lock, listener interfaces, static asset jail, discovery. B09; ACL/firewall unknowns below. Relay/SSH/PTY implementation internals belong to their lanes. |
| `ipc/{agents,terminal}.rs` | Detection/discovery; all spawn/batch/attach/CWD/write/resize/signal/list handlers, binary output/replay pump and error forwarding. B06/B07. Current get-CWD delegates to daemon, not process_cwd. Native input internals excluded. |
| `ipc/{project,project_remote,ssh,session,preferences,dag}.rs` | Path/reveal and registration wrappers; SSH config home expansion, CRUD and image transfer; session app-data paths; main-thread rerender dispatch; DAG watch/list/get offloading. No additional Windows-specific defect established in wrappers. Worktree operations/internal SSH execution delegated out. |
| `ipc/{cli_install,debug,diagnostics,updater,native_menu,mod,error,native_terminal_disabled}.rs` | Explicit unsupported CLI installation, portable boot trace, WindowsApps updater ownership, main-thread menus, spawn_blocking and structured errors, disabled-feature commands. B11; badge/CLI feature gaps below. |
| `ipc/native_terminal.rs` clipboard seams; `clipboard_image.rs`; `lib.rs` drop/setup; UI callers | Windows text/PNG/DIB readers exist, image read offloaded; native selection writes only macOS but Windows JS fallback exists. Tauri drop listener exists on Windows; B08 concerns inserted quoting, not missing event registration. Wheel/input/compositor ownership stays with native-input lane. |
| Tests and historical reports | Reviewed relevant embedded tests/call sites and IPC test-module gate; no tests run or changed. Prior cross-platform audit domain L9 and related IPC items dispositioned below; FINAL-AUDIT read with source/installed boundary preserved. |

## Confirmed source findings (severity ordered)

### BROWSER-IPC-B01 - HIGH - Windows browser automation has no client authentication
- Evidence: `src-tauri/src/ipc/browser_cli.rs:194-240,299-365` binds loopback TCP, accepts any peer, parses a request and immediately executes list/snapshot/act. Neither request enum nor connection has a secret or Windows principal check.
- Reachable: `src-tauri/src/lib.rs:1033-1036` starts it unconditionally at desktop setup -> listener -> `execute_request` -> `browser_automation_snapshot` / `browser_automation_act`. Remote gateway being Off does not disable this listener.
- Impact: another local Windows account/process that learns or enumerates the TCP port can read browser URLs/DOM metadata and operate logged-in pages. Reading browser.port is unnecessary for a local port enumerator; inherited file ACLs do not authenticate a TCP client. This is not a claim of Internet exposure or arbitrary Rust-command access.
- Red proposal: `windows_browser_cli_rejects_unauthenticated_peer`: actual ephemeral listener, send `{"command":"list"}\n` without credentials and require a typed auth rejection; then authenticated list/snapshot/act must work. Current implementation returns List.
- Binary observable: a second local account cannot retrieve a fixture tab title or click its test button without the capability. Smallest fix: browser_cli protocol/server/client authentication with a per-instance secret stored using a user-only Windows ACL (or user-restricted named pipe); retain request size bound.

### BROWSER-IPC-B02 - HIGH - external URLs are passed through cmd.exe command syntax
- Evidence: `src-tauri/src/ipc/browser.rs:1673-1693` passes validated URL as `cmd /C start <url>` and discards spawn errors. `src-tauri/src/browser/security.rs:67-98` validates URL scheme/credentials, not cmd metacharacters; ampersands remain valid in HTTP URLs.
- Reachable: `ui/src/components/BrowserPane.tsx:332` Open in system browser -> `ui/src/lib/browserTauri.ts:224-225` -> command. A download URL may originate from the guest page.
- Impact: URL query separators such as `&` are parsed by cmd rather than reliably delivered intact; command metacharacters introduce a shell-injection boundary. Standard Rust argument quoting is not cmd-language escaping. No Windows exploit was executed; exact launch serialization is a required red probe.
- Red proposal: `windows_external_open_preserves_url`: replace only the OS launch seam with an argv recorder, require shell-free opening of `https://example.test/?a=1&b=2`; in a disposable Windows harness use a benign marker helper to prove no second command can execute. Binary observable: default browser receives the entire URL, marker helper not invoked.
- Smallest fix: use a shell-free Windows URI opener (ShellExecuteW or existing suitable platform opener) and propagate errors; apply the same boundary review to `cmd_open_file_path` at `browser.rs:1743-1746` rather than attempting ad hoc metacharacter stripping.

### BROWSER-IPC-B03 - MEDIUM - Windows browser history is shadow state, not engine history
- Evidence: `src-tauri/src/ipc/browser.rs:802-834` supplies real flags only on macOS; `1196-1209,1253-1267` can stop before JS navigation or optimistically accept it. `src-tauri/src/browser/manager.rs:237-282` gates/mutates history from a URL vector.
- Reachable: `ui/src/components/BrowserToolbar.tsx:98-113,278-291` disables Back/Forward from these flags -> `browserTauri.ts:149-154` -> go-back/forward commands.
- Failure: same-document pushState entries that do not emit page-load/title events are absent from the vector; Back remains disabled although WebView2 can go back. Failed/no-op navigation can leave optimistic URL/loading state unreconciled. Combine historical L9-3/L9-4 as one cause, not two counted bugs.
- Red proposal: `windows_browser_history_tracks_push_state`: load fixture, await load, pushState twice without title changes, await fixture signal, assert actual Back availability and traverse back/forward via toolbar; compare URL and loading state against engine completion.
- Binary observable: toolbar traverses the same entries as WebView2, no stuck loading. Smallest fix: Windows WebView2 HistoryChanged/native history flags plus source/history completion reconciliation in browser.rs; do not infer forward availability from history.length or check location immediately after asynchronous history.back().

### BROWSER-IPC-B04 - MEDIUM - System notification sound is suppressed on Windows
- Evidence: `src-tauri/src/notification/notify_rust_adapter.rs:29-59` ignores content.sound; targetless path `src-tauri/src/ipc/notifications.rs:50-67` also omits it. notify-rust `src/windows.rs:50-54,81-86` maps missing sound_name to `.sound(None)`; tauri-winrt-notification `src/lib.rs:470-483` emits `<audio silent="true" />` for None.
- Reachable: `ui/src/lib/notificationCoordinator.ts:138-147` sends `sound: 'system'` and relies on OS banner audio rather than custom playback -> notification IPC -> service -> adapter. Plugin 2.3.3 desktop.rs:179-194 also leaves sound_name unset without explicit sound.
- Red proposal: `windows_system_toast_has_nonsilent_audio`: exercise actual adapter builder for System versus Silent and parse the machine-consumed toast XML; System must not have silent=true, Silent must. Current both silent.
- Binary observable: with OS sounds enabled, System toast audible and Silent toast inaudible; custom sound plays once. Smallest fix: map NotificationSound in both Windows submission branches to the dependency's supported Default/silent representation. Historical L9-5 had the direction reversed.

### BROWSER-IPC-B05 - MEDIUM - Windows browser keypress automation reports success without native editing
- Evidence: `src-tauri/src/ipc/browser.rs:342-352,1632-1641` uses synthetic KeyboardEvents on Windows; only macOS dispatches native navigation/editing keys. dispatchEvent does not perform trusted default text editing, caret movement or Tab focus navigation.
- Reachable: `src-tauri/src/cli.rs:90-98,130-142` parses browser keypress -> browser_cli Act -> browser_automation_act; the command returns success after callback regardless of actual edit.
- Red proposal: `windows_browser_keypress_edits_focused_input`: fixture input value `abc`, caret at end; call real CLI Backspace with current generation, await command response, assert value `ab`; separately Tab advances focus. No mocked DOM dispatcher.
- Binary observable: CLI keypress changes native input/focus, not merely event-listener counters. Smallest fix: Windows trusted WebView2 input path for promised keypress semantics, or typed unsupported result instead of false success; browser.rs and focused integration tests.

### BROWSER-IPC-B06 - MEDIUM - agent detection does not resolve Windows executable extensions
- Evidence: `src-tauri/src/ipc/agents.rs:510-523,570-592` checks only dir.join(name), no PATHEXT, and accepts any regular file off Unix.
- Reachable: `ui/src/App.tsx:560-566` -> `ui/src/lib/tauri.ts:753-755` -> cmd_agents_detect -> resolve_binary. LSP also found the startup-resolution caller at `src-tauri/src/terminal/shell.rs:420-425`; this report does not claim that caller necessarily fails because later spawn can resolve an extension.
- Red proposal: `windows_detect_agents_resolves_pathext`: explicit temp search path containing only agent.exe, then agent.cmd, query bare `agent`, require correct available/path; also reject an extensionless non-executable data file. Inject lookup inputs, do not mutate shared PATH in parallel tests.
- Binary observable: Agents settings identifies a normal Windows CLI install. Smallest fix: Windows-specific extension/runnability resolution in agents.rs, with precedence matching Windows command resolution.

### BROWSER-IPC-B07 - MEDIUM - provider-session fallback discovery is Unix-only on Windows
- Evidence: `src-tauri/src/ipc/agents.rs:56-80,172-205,313-343` calls /bin/ps, /usr/sbin/lsof or process_cwd; `src-tauri/src/ipc/terminal.rs:993-997` returns None on Windows.
- Reachable: `ui/src/state/workspaceStore.ts:487-511` -> discoverAgentProviderSession -> IPC -> `src-tauri/src/daemon/server.rs:1685-1707` -> spawn_blocking(discover_agent_session_id).
- Impact bounded to fallback provider identity discovery (not every agent-state/resume route). A Windows process with no authoritative provider-session metadata cannot be discovered through this fallback.
- Red proposal: `windows_provider_discovery_returns_known_session`: controlled child provider with known metadata and PID, subscribe to ready signal, invoke discovery, require exact provider ID rather than None. Binary observable: fallback-discovered conversation can be resumed without creating a new one.
- Smallest fix: Windows process/provider metadata adapter, preferably authoritative daemon/extension metadata rather than emulating ps/lsof; agents.rs plus the relevant metadata boundary. Historical broad get-CWD allegation is refuted below.

### BROWSER-IPC-B08 - MEDIUM - dropped Windows file paths use POSIX shell quoting
- Evidence: `ui/src/components/NativeTerminalPane.tsx:376-380` wraps backslash-containing paths in single quotes and escapes apostrophes as POSIX `'\''`; `1640-1654` uses it unconditionally for dropped paths.
- Reachable: same file `1660-1680` subscribes to Tauri onDragDropEvent -> insertPathsIfInsidePane -> sendPaste. Therefore missing macOS-only custom drop registration is NOT the demonstrated bug.
- Failure: cmd.exe does not treat single quotes as argument delimiters; `C:\QA\two words.txt` becomes two/wrong arguments. PowerShell apostrophe escaping also differs. WSL additionally needs path translation, not merely quotation.
- Red proposal: `windows_drop_path_round_trips_in_cmd`: after listener-ready event, drop a fixture file with spaces into a cmd pane using an argv-print helper and assert one exact path argument; repeat PowerShell with apostrophe path. Binary observable: consuming command opens the dropped file.
- Smallest fix: quote/translate using the target session's shell/platform (including remote target), not host OS alone; NativeTerminalPane quoting seam and session shell metadata. Native HWND drop delivery remains a separate unknown.

### BROWSER-IPC-B09 - MEDIUM - local-network mode cannot discover an isolated Windows LAN
- Evidence: `src-tauri/src/remote/state.rs:149-190,217-227` Windows interface enumeration consists solely of route probes to 8.8.8.8 and 100.100.100.100. With a usable on-link IPv4 LAN but no route to either destination it returns Err, never enumerating the LAN adapter.
- Reachable: cmd_remote_enable (`src-tauri/src/ipc/remote.rs:194-207`) -> daemon configuration -> start_remote_server -> SystemInterfaceResolver; `src-tauri/src/remote/server.rs:2307-2342` aborts listener startup on resolver failure.
- Red proposal: `windows_lan_without_default_route_is_resolved`: inject adapter inventory containing 192.168.50.10 and route lookup errors; require that address and successful same-subnet listener. Then isolated Windows QA host with a peer and no default route; do not alter user networking.
- Binary observable: Local Network enables and same-subnet peer reaches authenticated gateway without Internet/default route. Smallest fix: GetAdaptersAddresses-backed Windows enumeration while retaining explicit-interface binding; state.rs/platform adapter and deterministic resolver test.

### BROWSER-IPC-B10 - MEDIUM - terminal ~/ file links fail under normal Windows environment
- Evidence: `src-tauri/src/ipc/browser.rs:1709-1725` expands only HOME; standard GUI Windows environments may have USERPROFILE but no HOME. Bare `~` is incorrectly joined as a literal `~` even when HOME exists.
- Reachable: `ui/src/lib/linkRouting.ts:266-285` openTerminalToken(file) -> cmd_open_file_path -> candidate.exists returns false for a real profile-relative file.
- Red proposal: `windows_file_link_uses_userprofile`: injectable home lookup with HOME absent, USERPROFILE containing a fixture file; `~/file` must resolve exactly, bare `~` to profile itself. Binary observable: clicking the terminal file link launches the selected real file.
- Smallest fix: reuse framework home resolution / existing SSH tilde semantics (`ipc/ssh.rs:88-120`), separate bare tilde from suffix. Do not expand scope to editor line/column support.

### BROWSER-IPC-B11 - LOW - Windows switch-debug sink uses a Unix root
- Evidence: `src-tauri/src/ipc/debug.rs:33-54` opens /tmp/ferryx-switch-debug.jsonl without creating its directory. On a standard Windows volume without root tmp this fails, rather than writing to the user's temp directory.
- Reachable: `ui/src/lib/switchDebug.ts:69-81` serializes debug IPC writes and logs failures; no claim that failure is entirely silent.
- Red proposal: `windows_switch_log_uses_temp_dir`: inject a temporary sink root, dispatch a parsed event, require one JSONL event there. Binary observable: debug tab switch produces readable trace without manually creating C:\tmp.
- Smallest fix: std::env::temp_dir or the existing runtime-directory convention in debug.rs. Current portable boot trace in diagnostics.rs is not affected.

### BROWSER-IPC-B12 - LOW - Windows Permissions displays an irrelevant Full Disk Access warning
- Evidence: `ui/src/components/settings/PermissionsSection.tsx:175-189` renders macOS warning whenever !allGranted; `src-tauri/src/permissions/mod.rs:229-239` and non-authoritative notification mapping make allGranted false on Windows.
- Reachable: PermissionsSection fetchStatus (`:76-85`) -> getSystemPermissionsStatus -> ipc/permissions.rs -> permission provider. The individual FDA/accessibility cards ARE macOS-gated, but the warning is not.
- Red probe: open Permissions on Windows with non-authoritative notification status and inspect rendered alert category/platform applicability. No new prose-pinning test; if behavior test is added, assert platform/capability rendering, not wording.
- Binary observable: no instruction to grant macOS Full Disk Access on Windows. Smallest fix: platform-gate that warning in PermissionsSection only.

## Historical findings: verified dispositions, not copied allegations

- **L9-BROWSER-OS-1 (cookie deadlock): refuted.** Caller BrowserSection:106-119 invokes async IPC; browser.rs:1289-1337 calls set_cookie. Tauri runtime `src/lib.rs:1803-1812` queues SetCookie without recv; Wry applies CookieManager.AddOrUpdateCookie (`webview2/mod.rs:1671-1678`). Tauri warning at webview/mod.rs:2165-2166 is for reading cookies. No evidence that merely calling a synchronous Rust method inside async causes a WebView2 deadlock.
- **Dialogs deadlock: not established.** notifications.rs:175-201 uses callback pick_file + oneshot, no blocking_pick_file. Browser cookie picker is awaited frontend plugin dialog with main capability; native menus dispatch UI construction via run_on_main_thread and asynchronously await at native_menu.rs:189-213.
- **L9-2 (all embedded clipboard disabled): refuted as stated / permission behavior unknown.** Builder omits enable_clipboard_access, but locked Wry webview2/mod.rs:497-514 only auto-allows CLIPBOARD_READ when enabled. Absence does not prove keyboard paste/copy or clipboard writes universally fail. Blindly enabling it would grant every external page clipboard read without prompting; do not ship that as a portability fix.
- **L9-3/L9-4:** confirmed common history cause B03. **L9-5:** wrong Windows direction; corrected B04. Linux sound behavior is outside this Windows conclusion.
- **L9-6 (badge): confirmed explicit feature gap, not false success.** notifications.rs:247-250 returns supported=false on Windows; App.tsx:1332-1334 invokes but ignores support. No Windows taskbar overlay exists here. Product parity decision: implement numeric overlay via Windows API, not Tauri set_badge_count (unsupported on Windows); require unread/zero actual taskbar probe.
- **L9-7:** B11. **L9-8 (accessibility): contract mismatch remains but no reachable Windows UI action.** permissions/mod.rs:151-154 returns true and :258 can_request true; PermissionsSection:230 gates the whole card to macOS. Not a current Windows permission-request failure; future API cleanup should report unsupported.
- **L9-9 (AppUserModelID): intentional development fallback, not proven click-routing failure.** adapter.rs:37-44 skips app_id under target/debug or target/release; notify-rust uses POWERSHELL_APP_ID, as does plugin policy. Installed path receives configured ID; registration/toast attribution needs installed-package QA.
- **L9-10 (fallback UA): not applicable on Windows**, security.rs:51-54 has Windows UA. **L9-11: Linux-only**, no Windows defect. **L9-12: macOS-only named-profile limitation**, not Windows; current Windows data_directory branch exists.
- **Prior missing Windows file-drop implementation:** refuted at bridge level by Tauri onDragDropEvent caller; native child delivery unknown, B08 independent. **Prior selection-copy activation expiry:** native write missing on Windows is true, but frontend writeText fallback exists (NativeTerminalPane:954-972); awaiting IPC alone does not prove Chromium transient activation expired. Need real focus/permission probe, not a blanket failure claim.
- **Prior process_cwd test Windows failure:** ipc/mod.rs:38 cfg(all(test, unix)) excludes that suite. **L7-6 get-CWD claim:** current terminal.rs:859-873 reads daemon details, not process_cwd; fallback agent impact B07 remains. **L7-5/process discovery and executable-extension findings:** B07/B06 remain.
- **Prior CLI launcher HOME crash:** current cli_install.rs:60-89,250-260 returns explicit unsupported before home lookup on Windows. Installation itself remains Unix-only; unsupported feature, not a status crash. Do not add a shim without coordinating existing installer/PATH lane.
- **L7-8 wildcard/firewall claim:** wildcard premise is stale; server.rs:2243-2249,2295-2342 binds loopback plus requested interface and fails closed. Firewall reachability remains environment-dependent, not proven by missing netsh code. Pairing auth canonical Windows directories are unified (auth.rs:43-72); transactions use portable File::lock at :736-765, so Unix-only low-level StoreLock is not proof that auth transactions are unlocked.
- **Prior reveal missing platform branch:** project.rs:320-375 includes explorer with a single /select argument. **Prior notification shell-payload test:** notifications.rs:332-344 tests rejection of .sh as unsupported audio; it never executes /bin/sh, so no Windows shell dependency there.
- **20260912 FINAL-AUDIT:** previous popup event issue was helper focus interference, not a backend bug. Current native_menu.rs:189-260 dispatches and forwards events. Installed compositor/source repair and shell/wheel internals are not reclassified by this lane.

## Unknowns / bounded follow-up probes (not confirmed defects)

- Real WebView2 clipboard permission defaults, copied-selection focus, mixed browser/native overlays, DPI drop coordinates and native child HWND drop delivery: fresh source Windows QA required. Subscribe to clipboard/drop/UI events, drive real copy/drop, inspect OS clipboard and daemon input; do not infer working/failing from macOS-only auxiliary hooks.
- Browser profile failure cleanup: browser.rs:878-895 registers before fallible directory creation; errors can leave manager-only state, and :861-864 restored lookup may return it. Source mechanism exists, but no current user retry scenario proven here; probe injected app-data denial + retry same browserId before promoting. Also verify default/private/named WebView2 data-store isolation with cookie readback.
- Cookie import acknowledges queued setting, not native success; runtime logs application failures. Probe invalid/domainless cookie readback and valid import before claiming imported_count guarantees persistence. Offloading the loop does not fix native acknowledgement semantics.
- External downloads use a separate reqwest client (browser/download.rs), not WebView2 cookies; authenticated download parity and Windows native file-save dialog behavior need fixture HTTP authentication + actual Save As probe. Not Windows-specific source proof.
- Remote credential files inherit Windows ACLs (auth.rs:868-909); default LOCALAPPDATA is user-private but overridden data directories may not be. Inspect effective DACL with a second user before alleging universal disclosure. Identity first-creation races and persistence errors deserve separate cross-process probes; no claim that std::fs::rename universally fails to replace on Windows.
- Remote Tailscale/relay reachability, firewall allow/deny, packaged assets, notification permission/Focus mode, installed AUMID and actual clicks/audio: not run. Static assets reject Windows separators/colon/ADS and canonical escapes (server.rs:2012-2070); deployment presence belongs to packaging lane.
- Browser named IDs permit Windows reserved names/case aliases (model.rs:53-62); verify Win32 directory semantics and WebView2 canonicalization before defining identity rules. Browser guest bridge/new-window OAuth and redirects use nonce routing but real provider compatibility is not established.
- Wheel non-working symptom remains native-input lane: this audit makes no diagnosis from browser DOM wheel or from stale installed terminal behavior.

## Verification receipt

- Reopened cited source mechanisms and current callers; LSP symbols for browser.rs and references for resolve_binary; ast-grep found the actual set_cookie and cmd/start shapes; Bun confirmed all five core locked dependency versions above.
- `git diff --stat` before and after report creation showed the same pre-existing 23 tracked dirty files (178 insertions, 3 deletions). Report was absent before authorized apply_patch and was reopened afterward. Source was never written by this lane.
- Report-only completion: no source fixes, no test claims, no Windows execution, no installation/daemon manipulation, no git ref/branch/commit changes. Unverified runtime observables are explicitly proposals/unknowns.
