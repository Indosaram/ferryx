# Windows native-input audit - 2026-09-13

Status: COMPLETE (source review; Windows runtime verification remains outstanding).
Lane: native-input. Base: main b7ad4516. PR #2: OPEN, head 79b02ab6ea4753bf87080dd567368a870c018057.
Only this report was authored. Foreign dirty source was read, not changed; renderer output/font work is excluded.
"Confirmed" below means a reachable source defect, not a claim of execution on Windows today.

## Coverage and end-to-end routes

- Win32: reviewed windows.rs constructor/style, WNDPROC hit testing/default dispatch, viewport positioning, reveal/z-order, owner-thread posted destruction, all three existing lifetime tests; windows_focus.rs hook install/callback, screen-to-client/DPI conversion, descendant search, SetFocus, event emission and uninstall.
- Frontend: NativeTerminalPane pointer down/move/up/cancel, rAF coalescing, scrollbar dragging, Ctrl-click links, keyboard sink and document fallback, AltGr/IME, paste/copy, file-drop and native-focus subscribers. Reviewed lib/tauri.ts focus bridge and contextMenuGuard reachability.
- Backend: native_terminal IPC attached-session guards, authoritative mouse geometry, selection/tracking arbitration, input/paste encoding, scroll/scrollbar/focus commands; terminal.rs, input.rs, key_encoder.rs, mouse.rs, mouse_encoder.rs, wheel.rs, selection.rs and scroll.rs. Followed Ghostty legacy key and selection-release callees.
- Wheel route: Windows WM_MOUSEWHEEL is normally delivered to the focus HWND; DefWindowProc propagates upward, not to an underlying sibling. windows.rs:106-124 handles only hit-test and destruction, and windows_focus.rs:152-179 handles only left-button-up. There is no Windows native wheel bridge in these modules. WebView2 must receive the native event and produce DOM wheel -> NativeTerminalPane.tsx:2219-2234 -> cmd_native_terminal_scroll (ipc/native_terminal.rs:910-962) -> compute_wheel_outcome (wheel.rs:15-97).
- In tracking mode that function produces buttons 64/65 through terminal.rs:497-498 -> mouse_encoder.rs:22-137; alternate screen without tracking produces up/down keys (maximum five repetitions); primary screen produces ScrollViewport::Delta -> ipc/native_terminal.rs:750-757 -> terminal.rs:501-503 -> scroll.rs:220-250 -> ghostty_terminal_scroll_viewport. IPC schedules a main-thread render and frontend refreshes scrollbar state. Thus "there is no alternate-screen wheel support on Windows" is false.
- Focus route: lib.rs:1031-1032 installs the hook -> windows_focus.rs:152-179 -> surface_host.rs:877-889 -> emits native_terminal_focus -> ui/src/lib/tauri.ts:581-586 -> NativeTerminalPane.tsx:1594-1617 focuses the sink immediately, next frame, and after 40 ms -> sink onFocus/onBlur -> ipc/native_terminal.rs:612-645 -> surface_host.rs:2213-2244 changes local focus state. This is not a Win32 focus success acknowledgment.

## Confirmed findings (severity ordered)

### native-input-01 - HIGH: presented compositor is not cross-thread input-transparent (PR #2 fixes this scope)

- Source: src-tauri/src/native_terminal/platform/windows.rs:243-250, 311-333; WNDPROC :112-124 returns HTTRANSPARENT, but constructor omits WS_DISABLED.
- Reachable chain: surface_host.rs:2523 -> platform/mod.rs:73 -> WindowsCompositorTarget::new; surface_host.rs:2646-2648 -> reveal -> child raised above WebView2. HTTRANSPARENT only searches underlying windows in the same thread; WS_EX_TRANSPARENT is not a cross-thread input-routing guarantee.
- Consequence: real pointer selection can fail despite synthetic DOM tests; descriptor.pointer_transparent=true is stronger than the implementation. Native wheel targeting can also be affected, but wheel failure is NOT proved solely by this hit-test defect because focus/inactive-window routing matters.
- Failing-first proposal: run PR's production-constructor cross-thread hit-test regression against main flags, then WS_DISABLED flags; also subscribe to real DOM pointer down/move/up and backend release receipt before the owned Windows input action. Binary observable: chosen input HWND is WebView2 and dragging selects a known sentinel while presentation remains visible.
- Smallest write scope: PR #2's windows.rs flags/constructor seam plus windows_pointer_tests.rs. Recommendation: accept that narrowly scoped repair; do not label it a complete wheel fix.

### native-input-02 - HIGH: DOM wheel loses modifiers and position at the IPC boundary

- Source: ui/src/components/NativeTerminalPane.tsx:2219-2229 sends only rows; src-tauri/src/ipc/native_terminal.rs:922-939 substitutes pane center and KeyModifiers::default().
- Chain: DOM wheel -> delta scroll IPC -> wheel.rs:28-31, 48-64, 80-97 -> PTY mouse/arrow bytes or VT viewport. Shift-wheel cannot select the intended viewport override; Ctrl/Alt mouse modifiers disappear; SGR wheel events always address the center, so split TUI panels can scroll the wrong panel.
- Failing-first proposal: enable 1000+1006 on a real terminal, dispatch wheel at cell (2,3) with Ctrl, assert exact SGR button/modifier and coordinate bytes; dispatch Shift-wheel on primary screen with history and assert viewport offset changes with zero PTY writes. Exercise the actual command adapter, not only compute_wheel_outcome (whose direct Shift tests already pass by construction).
- Binary observable: native wheel over left/right TUI subpanels addresses the hovered one; Shift-wheel moves scrollback rather than the application's mouse handler.
- Smallest write scope: dedicated wheel DTO/IPC context in NativeTerminalPane.tsx and ipc/native_terminal.rs, reusing authoritative geometry and compute_wheel_outcome. Do not invent another global hook or double-deliver native+DOM wheel.

### native-input-03 - HIGH: global mouse-up hook routes unrelated clicks into terminal focus

- Source: src-tauri/src/native_terminal/platform/windows_focus.rs:152-175, 232; global WH_MOUSE_LL receives desktop mouse-up but checks only ScreenToClient and terminal rectangles. surface_host.rs:877-889 checks attached bounds, not foreground window, occlusion, originating target or DOM modal ownership.
- Chain: any desktop left-button-up at a screen point corresponding to an attached pane -> best_effort_focus_webview -> native_terminal_focus -> NativeTerminalPane.tsx:1594-1617 repeatedly focuses its textarea. A floating DOM input/dialog over the terminal is also indistinguishable from terminal content at this boundary.
- Failing-first proposal: isolated Win32 desktop with owned root, attached pane and covering unrelated HWND; call/drive the real hook callback at that point with a focus-event receiver subscribed first, assert no emit and no SetFocus attempt. Separate actual DOM dialog-input click must preserve activeElement through the callback/frame/timer signals.
- Binary observable: clicking a dialog input over terminal bounds keeps typing in that input; clicking an overlapping other application never emits terminal-focus or changes its focus.
- Smallest write scope: windows_focus.rs ownership/foreground hit filtering and frontend focus-event acceptance; preferably retire the workaround after proving PR #2 delivers normal pointer focus. SetFocus targets first Chrome_WidgetWin_1 and ignores its result (:112-139); actual child queue association/success remains unknown, not an independently proven cross-thread failure.

### native-input-04 - HIGH: normal TUI pointer reporting is unreachable from the pane

- Source: NativeTerminalPane.tsx:2190-2216 only sends Left presses and diverts Ctrl-left into links; :1197-1216 sends motion only during that left drag. ipc/native_terminal.rs:1276-1282 always classifies Left, null-button Motion and Release as selection even when tracking is enabled.
- Chain: physical click -> DOM -> sendMouse (:1077-1128) -> cmd_native_terminal_mouse -> selection instead of encode_attached_native_mouse. Right/middle presses and hover motion have no frontend encoder path; therefore 1000/1002/1003-capable TUIs cannot receive their ordinary click/drag/hover controls here.
- Failing-first proposal: real terminal in 1000+1006, plain Left press/release must write expected SGR bytes and Shift-left must select without PTY writes; add right/middle and 1003 hover cases through the pane adapter. Record current unconditional selection policy explicitly when changing it (this is existing behavior, not PR #2's regression).
- Binary observable: a TUI click receiver reports button/position; tmux/vim interaction works without stealing Shift-selection or explicit link gestures.
- Smallest write scope: NativeTerminalPane.tsx button/drag state and IPC selection-vs-tracking predicate; preserve actual held button for motion/release. No renderer change.

### native-input-05 - HIGH: Ctrl-punctuation can encode no input

- Source: src-tauri/src/native_terminal/key_encoder.rs:29-39 maps punctuation to Unidentified, :148-160 sets unshifted codepoint but suppresses UTF-8 under Ctrl. Caller NativeTerminalPane.tsx:2369-2383 supplies utf8:null through input.rs:133-143.
- Callee verification: vendor/ghostty/src/input/key_encode.zig:385-413 calls ctrlSeq then returns on empty UTF-8; :705-731 accepts UTF-8 or logical-key codepoint, not unshifted codepoint alone. Thus ordinary legacy Ctrl-backslash/Ctrl-] produce no bytes, not their expected 0x1c/0x1d. Kitty mode is a separate path; no claim that every negotiated mode fails.
- Failing-first proposal: NativeTerminalInput KeyEvent Character('\\') and Character(']'), Ctrl=true, utf8=None on a fresh terminal must produce [28] and [29]; also cover Ctrl-[ against the project's desired legacy/fixterms contract rather than assume every mode emits ESC.
- Binary observable: byte-capture child receives Ctrl-backslash/Ctrl-] under the normal Windows DOM path.
- Smallest write scope: key_encoder.rs punctuation logical-key/UTF-8 construction and engine/input tests; no platform-specific PTY rewrite.

### native-input-06 - MEDIUM: last coalesced selection motion is discarded on release

- Source: NativeTerminalPane.tsx:1197-1217 queues Motion on rAF; :1238-1245 cancels it and sends only Release. Backend selection.rs:605-646 supplies no selection result for Release; vendor/ghostty/src/terminal/SelectionGesture.zig:558-581 only updates click/drag bookkeeping.
- Chain: Press -> Move -> Up before next animation frame -> no Drag reaches apply_mouse_gesture (terminal.rs:250-251); the release location does not extend selection. Quick short drags can yield empty selection; longer drags stop at the previous frame's position. PR #2 exposes this existing downstream defect rather than fixing it.
- Failing-first proposal: controlled rAF scheduler, dispatch down/move/up without executing the frame, then assert final selected sentinel through real gesture application, not merely that Release IPC was called. Await exact IPC completion and order.
- Binary observable: rapid drag/release selects through the release cell, including under loaded frames.
- Smallest write scope: frontend flush/final Motion before Release, with ordered dispatch; related component + gesture integration test.

### native-input-07 - MEDIUM: horizontal/zero wheel becomes upward movement; deltaMode ignored

- Source: NativeTerminalPane.tsx:2223 unconditionally computes Math.trunc(deltaY/20) || sign; caller is the onWheel handler at :2219. No zero guard or line/page normalization.
- Independent Bun arithmetic probe: {deltaX:120,deltaY:0,deltaMode:0} -> -1 row; {deltaY:3,deltaMode:1} -> 1 rather than 3; page delta 1 -> 1 row. Existing test NativeTerminalPane.test.tsx:3458-3482 covers only +/-60 pixels.
- Failing-first proposal: zero vertical/horizontal-only event must perform no vertical IPC; line=3 must produce 3 rows; page=1 must use current visible rows; fractional pixels must accumulate rather than turn every tiny event into a row. Verify exact payloads with controlled state, no sleeps.
- Binary observable: horizontal touchpad motion does not scroll vertically; wheel/touchpad speed respects units. Actual WebView2 hardware deltaMode remains to be sampled (Chromium often supplies pixels); the prior audit's claim that Windows necessarily emits lines is unsupported.
- Smallest write scope: frontend wheel normalization and tests; bound row conversion before IPC's narrowing `*rows as i16` (:938), which otherwise wraps very large deltas.

### native-input-08 - MEDIUM: WebKit-only composition tail suppression applies on Windows

- Source: NativeTerminalPane.tsx:2395-2406 stores every composition's last character; :2330-2345 swallows the next matching non-composing key, with no platform or originating-event identity guard.
- Chain: Windows/WebView2 compositionEnd -> committed text sent -> later genuine keydown matching the tail -> preventDefault and return before sendInput (:2350-2361). For a composition committing a space, the next deliberate space can be lost even after arbitrary idle time.
- Failing-first proposal: Windows platform fixture, compositionEnd with a trailing ASCII space, then an independent non-composing Space keydown must send committed text plus space. Retain the separate WebKit replay case. Existing tests :6313-6359 explicitly assume the replay and therefore cannot prove Windows correctness.
- Binary observable: Windows IME commit then a deliberate space appears exactly once; native event trace must distinguish browser replay from a new key press. Actual Windows IME event ordering was not sampled today.
- Smallest write scope: frontend suppression provenance/platform gating, component tests; no Ghostty changes.

### native-input-09 - MEDIUM: bare Ctrl+V is always paste, not terminal SYN

- Source: NativeTerminalPane.tsx:244-254 matches Ctrl+V regardless of Shift; sink :2310-2313 and capture :1382-1391 prevent default and call performNativePasteFallback (:1045-1075).
- Consequence: with a text clipboard, vim visual-block/readline quoted-insert receives pasted text, not 0x16. The image-specific raw 0x16 helper (:1005-1012) does not restore the ordinary key route. Existing Windows Ctrl+V-paste UX may be intentional; this is a terminal-key capability conflict, not a claim that Windows users must reject Ctrl+V paste.
- Failing-first proposal: explicit terminal-key policy fixture, text clipboard populated, bare Ctrl+V -> [22] without clipboard access; Ctrl+Shift+V/Shift+Insert -> paste. Binary observable: vim enters visual-block mode without inserting clipboard content.
- Smallest write scope: shared shortcut policy used by sink/capture and tests; expose/document a preference if Windows Ctrl+V paste is retained.

### native-input-10 - MEDIUM: dropped paths use POSIX quoting in cmd.exe

- Source: NativeTerminalPane.tsx:376-381 quotes all backslash paths with single quotes; actual Windows onDragDropEvent (:1660-1680) -> insertPathsIfInsidePane (:1640-1653) -> sendPaste (:976-1000).
- Consequence: `C:\\A B\\file.txt` is sent with literal single-quote delimiters that cmd.exe does not recognize for arguments. PowerShell handles simple single-quoted arguments, but apostrophes use doubled quotes, not POSIX '\\'' escaping. Do not apply a blanket Windows double-quote fix to WSL or remote POSIX sessions.
- Failing-first proposal: profile-aware drop formatter cases for cmd path with spaces, PowerShell path with apostrophe, WSL and remote POSIX; execute resulting argument with each owned shell in later QA and assert exact received path.
- Binary observable: drag a known file after `type ` in Command Prompt and read its sentinel content without editing pasted delimiters.
- Smallest write scope: shell-profile-aware path quoting at frontend drop/paste boundary plus focused tests.

## PR #2 review and test limits

- Read `gh pr diff 2` and PR metadata twice; two files only, no wheel/focus-hook/frontend changes. WS_DISABLED=0x08000000 is the correct normal style, compatible with a visible draw-only child. It prevents the child acquiring input/focus; it does not hide the HWND, remove its swapchain, or invalidate posted private destruction messages. No introduced Win32 correctness defect identified in this patch.
- New windows_pointer_tests.rs at PR head: :38-79 isolates/restores a uniquely named desktop; :90-136 creates visible STATIC input child; :140-157 queries WindowFromPoint from a second thread; :170-198 constructs the production target, reveals it and asserts both visibility and underlying input target. FFI POINT is two i32s by value; style bits and handle-sized fields are appropriate. Cleanup destroys parent/children before closing desktop. No sleeps/polling or live-desktop switch.
- Important bound: both fixture HWNDs belong to the owner thread; the *query* is cross-thread. This validates the documented WindowFromPoint disabled-window behavior and production style, not an actual WebView2 process, real message delivery, focus queues, capture, OLE DnD or WM_MOUSEWHEEL. Query thread join is unbounded (test-runner timeout should bound a stuck OS call). Existing three lifetime tests use a posted FIFO barrier and message-queue event wait, not timing-luck polling.
- PR body reports four native tests and real selection RED/GREEN on Windows at scale 1; these are author claims, not rerun evidence in this session. Its reported eight unrelated frontend failures remain unverified here; do not silently call the full frontend suite green.
- Required wheel acceptance later: subscribe to owned HWND WM_MOUSEWHEEL, DOM wheel, command receipt, PTY bytes and scrollbar events before one real wheel action; compare primary history, alt screen, SGR, Shift, focused/unfocused panes, horizontal motion, 100%/150% DPI. Run the app only via `bun tauri dev`, never substitute the stale installed executable or restart the user daemon.

## Prior-audit dispositions and remaining unknowns

- L3-NATIVE-SURFACE-7: partially refuted. No Windows native monitor, but DOM IPC already handles alternate screen and tracking; missing context is native-input-02, native hit transparency is -01. A new low-level wheel hook is not inherently required.
- L3-NATIVE-SURFACE-12: unscoped macOS natural-text remaps still exist (input.rs:108-125); delivered Super+Left/Right/Backspace become 0x01/05/15. However Windows reserves many Win chords before DOM delivery, so historical "breaks window snapping" is unproven. Recommend platform gate if a delivered chord is reproduced; not promoted above verified user-reachable defects.
- L3-NATIVE-SURFACE-14: confirmed as -05 after reading the Ghostty callee; unshifted_codepoint does not rescue legacy punctuation. L3-NATIVE-SURFACE-17 physical KeyV/KeyC priority remains implemented (:237-241); IME/AltGr gates are shared (:210-223, :1337-1355, :2301-2308), not missing Windows support.
- L3-NATIVE-SURFACE-4: "no Windows drop implementation" refuted. Frontend subscribes to Tauri drops; locked wry 0.55.1 webview2/drag_drop.rs:43-78 enumerates HWNDs and registers IDropTarget. PR #2 does not prove OLE drop reaches that target over the renderer. Native drop targeting remains runtime unknown; quoting is -10.
- L3-NATIVE-SURFACE-5 and L5-UI-FRONTEND-5: native copy command remains macOS-only for OS writes (ipc/native_terminal.rs:1123-1168), but Windows caller writes via navigator.clipboard (:955-970). Async IPC alone does NOT prove Chromium transient activation is lost; Windows copy failure needs real clipboard permission/focus evidence. Windows paste read is implemented (:1492 onward), so not a missing Windows clipboard adapter.
- L3-NATIVE-SURFACE-8: per-frame restore is macOS-only (platform/mod.rs:115-118), but Windows child creation/reveal use NOACTIVATE and focus sink/hook paths exist. Missing method symmetry is not proof of failed focus; current confirmed hook routing defect is -03. L3-NATIVE-SURFACE-9/-18 are Linux/macOS-only, not Windows defects.
- L3-NATIVE-SURFACE-6: update_viewport(None) indeed returns without hiding, but examined production surface_host.rs calls at :420 and :2564 pass Some; no reachable None caller was found. Lifecycle/overlay visibility belongs to the surface lane, not an extra input finding. L3-NATIVE-SURFACE-10 visual overlay occlusion and -13 DPI capability API symmetry are surface-lane topics; mouse authoritative scaling exists (:1212-1249) and hook uses GetDpiForWindow (:161-165).
- L5-UI-FRONTEND-4/-6/-8/-9: disposed as -09/-04/-07/-10 respectively. Ctrl-click link activation is explicit policy; its terminal-reporting cost is included in -04, not a duplicate visual-cursor nit.
- L5-UI-FRONTEND-15: contextMenuGuard still prevents DOM contextmenu (lib/contextMenuGuard.ts:1-17), but repository search found only its tests as callers, not app installation. Historical universal-guard claim is not currently reachable. Terminal right-button reporting omission is independently -04; Windows native titlebar menu is outside this lane.
- Capture: no explicit setPointerCapture/SetCapture, release capture, or lostpointercapture handler in assigned path; global DOM move/up/cancel works inside WebView2. Actual Chromium implicit/native capture across window exit, embedded browser HWNDs, Alt-Tab, pointer cancel and pen/touch remains unknown. Probe release outside the owned root and re-entry, assert gesture cleared and no stale drag. Do not infer failure from missing explicit API alone.
- Focus-hook latency/lifetime: callback holds MONITOR_STATE and synchronously acquires session mutex, enumerates HWNDs, calls SetFocus and emits; low-level timeout/removal under load is a risk requiring a controlled lock-contention probe, not an observed freeze. Uninstall exists but has no located app caller; OS removes process hooks on exit, so no cross-process lifetime leak asserted.
- Keyboard focus reporting (?1004), negotiated Kitty release/repeat, selection autoscroll, unusual multi-monitor DPI/zoom, stale remote-generation closures and IPC reordering need targeted protocol/runtime probes; current audit does not claim these work merely because encoding primitives exist. No Windows hardware/browser run was authorized.
- FINAL-AUDIT.md (20260912) correctly separates stale installed startup defect from repaired source/debug binary. Its successful shell echo/resize run does not validate wheel, selection or this PR. This review does not overwrite that evidence or claim to update the installed application.

## Verification receipt

- AGENTS root/backend/native-terminal/UI/components/lib/IPC/vendor instructions and programming/debugging/ast-grep/frontend skills read. LSP document symbols resolved windows.rs; ast-grep enumerated frontend invoke calls. Every finding span was reopened and its caller/callee inspected before writing.
- Independent Microsoft Win32 documentation retrieved with parallel Bun fetch: WM_NCHITTEST (same-thread HTTRANSPARENT), WindowFromPoint (skips disabled/hidden), WM_MOUSEWHEEL (focus/parent propagation), SetFocus (calling-thread queue requirement). URLs: learn.microsoft.com/en-us/windows/win32/inputdev/{wm-nchittest,wm-mousewheel}; learn.microsoft.com/en-us/windows/win32/api/winuser/{nf-winuser-windowfrompoint,nf-winuser-setfocus}.
- No builds, suites, desktop launches or process manipulation performed. Only the four-case wheel arithmetic probe ran; it is not a WebView2/VT integration test. All failing-first tests above are proposals for the repair phase, not fabricated RED results.
- `git diff --stat` retained 23 foreign modified files; insertions increased from 120 to 178 during concurrent renderer work. Assigned windows.rs/windows_focus.rs/NativeTerminalPane.tsx/IPC file diff was empty. No source edits were made by this lane.
