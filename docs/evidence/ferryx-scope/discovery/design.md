# Design Mode: bounded implementation contract

Status: WORKING discovery complete; implementation and runtime verification NOT performed.
Scope: desktop child webview inspect -> rectangle -> native PNG -> preview -> explicit agent handoff.
No html2canvas, DOM re-render screenshots, OS desktop screenshots, full-page stitching, or remote transport expansion.
Paths below are relative to orca-lite unless prefixed `O` or `R`.
`O` = /Users/indo/code/project/orca; `R` = /Users/indo/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f.
Read root, IPC, UI components/lib AGENTS and O/AGENTS.md. No deeper applicable AGENTS found.
LSP document-symbol request failed: daemon unreachable at /Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock.
References were checked with source reads/rg, not LSP. Foreign dirty product files remain read-only.

## Verified current seams
- `src-tauri/src/ipc/browser.rs:840-1043`: register session before queued main-thread `add_child`; successful command return is NOT child-ready acknowledgement; creation errors currently are not propagated.
- `src-tauri/src/ipc/browser.rs:911-943`: initialization script plus navigation interception delivers guest open/download/shortcut events; normal navigation returns true.
- `src-tauri/src/browser/guest.rs:18-32,50-53`: guest routing parses reserved hosts/query values and uses location.assign; guest-supplied data is not an authorization boundary.
- `src-tauri/src/ipc/browser.rs:774-790`: `eval_webview` uses callback + oneshot + five-second timeout. Reuse for short synchronous extraction, NOT a long-lived user-selection Promise.
- `src-tauri/src/ipc/browser.rs:1387-1429`: `browser_automation_snapshot` extracts DOM metadata and records generation-bound selectors; it does not capture pixels.
- `src-tauri/src/ipc/browser_cli.rs:13-34`: CLI exposes List/Snapshot/Act only; do not reinterpret its Snapshot as PNG or widen CLI in this slice.
- `src-tauri/src/browser/model.rs:3-15,207-245`: LogicalRect and automation payloads exist; capture needs stricter finite, positive dimensions than LogicalRect::is_valid.
- `src-tauri/src/browser/manager.rs:119,180-240,278-327`: generation starts at 1; explicit navigation/reload/history invalidate targets; `assert_automation_generation` is reusable.
- `src-tauri/src/browser/manager.rs:350-379`: page-state generation advances only when URL changes; same-URL guest reload and DOM mutation cannot be detected by generation alone.
- `src-tauri/src/browser/manager.rs:266-276,330-348`: bounds/visibility/zoom changes do not advance generation. Recreated browser IDs can restart at 1; bind operations to native webview_label too.
- `src-tauri/src/ipc/browser.rs:1475-1485`: `cmd_browser_close` removes manager session and closes native view.
- `ui/src/lib/browserTauri.ts:42-56,138-155`: visibility and close share `enqueueBrowserLifecycle`; capture currently has no queue. Bounds use retries at :119-136, not readiness events.
- `ui/src/components/BrowserPane.tsx:83-88,180-224`: child surface sits above DOM; maskAwareVisible controls hiding; cleanup hides, does NOT destroy. Preserve this current ownership despite stale AGENTS cleanup wording.
- `ui/src/components/BrowserPane.tsx:111-132`: listener receives generation but does not retain it in liveTab. Design state must explicitly retain operation identity/generation.
- `ui/src/components/BrowserToolbar.tsx:33-39,267-355`: navigation/zoom/focus controls are the bounded entry point for a Design Mode toggle.
- `ui/src/lib/browserTauri.ts:207-217`: typed invoke wrappers for existing DOM snapshot/action; add separate Design wrappers rather than changing their semantics.
- `ui/src/lib/tauri.ts:376-379`: `writeTerminal({sessionId,data})` is text-only and no-ops outside Tauri; it is NOT an image attachment or agent-acceptance API.
- `ui/src/components/NativeTerminalPane.tsx:532-535`: actual native target uses backendSessionId, not frontend sessionId. Never route handoff to whichever pane later becomes active.
- `src-tauri/src/lib.rs:838-840`: existing browser commands registered in invoke handler; integrator must register new commands here.

## Real child-pixel APIs (source existence verified, runtime quality unverified)
Locked versions: `src-tauri/Cargo.lock:3637,5590,6901,6963,7713` = objc2-web-kit 0.3.2, Tauri 2.11.5, WebKitGTK 2.0.2, webview2-com 0.38.2, Wry 0.55.1.
Common entry: `R/tauri-2.11.5/src/webview/mod.rs:1610-1613,1668-1675` provides main-thread `Webview::with_webview`; native handles are semver-sensitive.
- macOS: `PlatformWebview::inner()` is WKWebView pointer (:200); current cast precedent is `src-tauri/src/ipc/browser.rs:822-825`.
  Call `WKWebView::takeSnapshotWithConfiguration_completionHandler`; NSImage/NSError completion signature verified at `R/objc2-web-kit-0.3.2/src/generated/WKWebView.rs:696-704`.
  Enable WKSnapshotConfiguration + block2 features (method gated); current `src-tauri/Cargo.toml:120` only explicitly enables WKWebView/WKNavigation. Add PNG encoding and callback lifetime ownership deliberately.
- Windows: `PlatformWebview::controller()` at `R/tauri-2.11.5/src/webview/mod.rs:180-184`; `CoreWebView2()` at `R/webview2-com-sys-0.38.2/src/bindings.rs:9175`.
  Use `ICoreWebView2::CapturePreview(PNG, IStream, completion)` (:1376-1394); PNG enum at :176; `CapturePreviewCompletedHandler` at `R/webview2-com-0.38.2/src/callback.rs:304`.
  Retain COM stream/callback on owning UI apartment through completion; return owned bytes, not COM objects, across Tokio boundary.
- Linux: `PlatformWebview::inner()` returns WebKitGTK WebView at `R/tauri-2.11.5/src/webview/mod.rs:173-175`.
  `WebViewExt::snapshot` / native `webkit_web_view_get_snapshot` + finish returns Cairo surface: `R/webkit2gtk-2.0.2/src/auto/web_view.rs:1123-1172`.
  Choose SnapshotRegion::Visible (`src/auto/enums.rs:3520-3522` in same crate); GLib main-context ownership asserted at :1132-1140. Encode/copy surface safely before crossing threads.
All adapters: visible viewport only, native completion + bounded timeout, explicit errors on blank/empty/oversized output; no fake fallback. Direct target-specific dependencies/features and PNG encoders remain to compile-prove.

## Reusable upstream Orca (adapt, not wholesale transplant)
- `O/src/shared/browser-grab-types.ts:9-108,181-202`: page/target/CSS rect/PNG shapes and bounded payload budgets; adopt subset, not annotations/React internals.
- `O/src/main/browser/grab-guest-overlay-script.ts:1-11,26-65`: in-guest hit-catcher, closed shadow visuals, getBoundingClientRect highlight. Port to Ferryx naming; add rectangle pointer capture/Escape cleanup.
- `O/src/main/browser/grab-guest-selection-scripts.ts:28-47,77-84,120-134`: suppress selection clicks and settle cancellation; adapt event-based completion, not Electron Promise-await assumptions.
- `O/src/main/browser/browser-grab-session-controller.ts:54-75,97-132,175-190`: one operation per tab, settle-once and cancellation on navigation/destruction; port lifecycle principles, not Electron event calls.
- `O/src/main/browser/browser-grab-payload.ts:18-80`: validate/clamp untrusted guest content and sanitize URL; reimplement boundary validation in Rust, render snippets as text.
- `O/src/main/browser/browser-grab-screenshot.ts:26-110`: hide overlay in try/finally, real capturePage, derive scale from bitmap/viewport width, bounded PNG.
  Electron capturePage is NOT available on WKWebView/WebView2/WebKitGTK. Reuse coordinate/budget logic only; improve to independent X/Y measured ratios and pre/post geometry validation.

## Assignment A: native bridge owner (implement first)
Write scope: new `src-tauri/src/browser/design*.rs` + `browser/capture/{mod,macos,windows,linux}.rs`; targeted browser model/manager/guest/mod and IPC browser changes/tests.
Integrator exclusively owns Cargo.toml/Cargo.lock, src-tauri/src/lib.rs and shared error registration. Do not collide with dirty ipc/mod.rs or unrelated files.
Proposed new types/commands (not existing symbols; camelCase wire, snake_case Rust):
```ts
type DesignIdentity = { browserId: string; webviewLabel: string; generation: number; opId: string };
type CssRect = { x: number; y: number; width: number; height: number };
type DesignSelection = DesignIdentity & {
  mode: 'inspect' | 'rectangle'; rectViewport: CssRect;
  viewport: { width: number; height: number; scrollX: number; scrollY: number; revision: number };
  url: string; title: string; selector?: string; text?: string; html?: string;
};
type DesignCapture = { captureId: string; identity: DesignIdentity; selection: DesignSelection;
  pngBase64: string; pixelWidth: number; pixelHeight: number; sha256: string };
// cmd_browser_design_begin({browserId,generation,mode}) -> DesignIdentity
// cmd_browser_design_capture({identity}) -> DesignCapture (native-owned selected rect)
// cmd_browser_design_cancel({identity}) -> void
// event browser_design_state: identity + armed|selected|cancelled|error (+ selection/error)
```
Install selection listener before arming. New reserved guest action reports only bounded operation/selection metadata; native handler derives browser identity from child closure, validates active op and rejects forged/mismatched tokens. Never permit guest-triggered capture or handoff.
Guest fields remain untrusted even with tokens; fetch selection using short eval after signal, not large HTML in URL. No tokens grant arbitrary filesystem/terminal access.
Add child-ready acknowledgement/error before begin can succeed; invalidate design ops on document-start including same-URL reload, hide, close, recreation, resize, zoom, scroll, selection replacement, and unmount.
Capture checks identity and geometry revision before AND after native completion; late callbacks settle/discard once. Use finite positive clipped rects, floor left/top and ceil right/bottom in measured bitmap coordinates.
Remove guest overlay visuals, await render acknowledgement, capture while child is still visible, then restore only the still-current op. A JS callback alone is not proven compositor flush; native QA must prove overlay exclusion.
Proposed caps: 2 MiB PNG, 4096 HTML chars, 200 text chars, 700 selector chars; invalid/out-of-bounds/oversized results are explicit errors, not screenshot-null success.
Do not promise atomic DOM+animation pixels; reject measured geometry changes and document snapshot timing. Cross-origin iframe internals/closed shadow roots get rectangle pixels + unavailable-context reason.

## Assignment B: desktop UI and exact handoff owner
Write scope: BrowserPane.tsx, BrowserToolbar.tsx, new DesignPreview component, new ui/src/lib/browserDesign*.ts and focused tests; integrator owns dirty types.ts/tauri.ts/state files if needed.
Use begin/cancel/capture wrappers in browserTauri.ts, keep design state transient per identity. State machine: off -> armed -> selected -> capturing -> preview -> sending -> sent/error; cancel from all pre-send states.
Preview opens only after real PNG succeeds, hides native child using existing mask ownership, shows image/context/target/prompt and explicit Send/Cancel. Restore only if pane remains visible; never show over another modal/drag.
Freeze captureId, PNG hash, serialized context, editable prompt and selected backend target into one draft revision. Editing creates a new revision; preview and delivery consume the SAME immutable revision, never re-extract/re-capture at Send.
Recommended minimal handoff: persist PNG to app-owned capture storage, show the exact final text plus attached file/hash; send text containing the file reference to explicitly chosen local agent terminal via verified backendSessionId.
Add narrow native `cmd_browser_design_handoff({captureId,draftRevision,targetBackendSessionId,text})` with ownership/target validation and single-flight idempotency; request paths are never arbitrary. Disk I/O uses run_blocking.
Return structured written/failed/unknown-delivery status; text write completion is not proof the agent consumed the image. Never auto-retry an ambiguous terminal write or append hidden submit keystrokes.
Keep artifact through handoff; cancel deletes unhanded artifacts; explicit cleanup policy for handed artifacts is a remaining decision. No image-in-terminal escape hacks or invented multimodal API.

## RED seam and verification assignment (not run here)
Native injectable capture completion + real BrowserManager: begin op A, defer capture, navigate/recreate same browser ID, complete A -> stale error, no preview/artifact/handoff. Deliver callback twice -> one settlement.
Add same-URL reload, resize/zoom/scroll, close/hide and missing-child cases; injected native bytes prove plumbing only, not real pixels. New tests carry `browser::design` namespace.
Pure crop tests: reversed drag normalization, fractional X/Y scaling, mixed DPI, finite checks, clip edges, byte cap. Decode PNG dimensions in tests; do not pin prose.
UI RED tests: subscribe before triggering exact selected/capture completion; switch target/unmount while pending; exact shipped-copy equality and PNG hash equality; double Send writes once; Cancel writes zero.
Use deferred completions/events with bounded timeouts, no sleeps/polling. Existing browser automation stale-target tests are at `src-tauri/src/browser/tests.rs:199,241`; no tests were executed or claimed passing.
Commands from repository root (validated against help or declared scripts, execute only during implementation in isolated QA):
```sh
bun run --cwd ui test src/lib/browserTauri.test.ts src/components/BrowserPane src/components/BrowserToolbar
bun run --cwd ui test src/lib/browserDesign src/components/DesignPreview
cargo test --manifest-path src-tauri/Cargo.toml --lib browser::design
bun run --cwd ui build
cargo build --manifest-path src-tauri/Cargo.toml
```
`ui/package.json:7-10` declares dev/build/test (test is Vitest, not Bun test); `bun run --help`, `cargo test --help`, `cargo build --help` checked. New test paths/names above are assignments, not existing tests.
Run LSP diagnostics on every changed source before build; currently LSP unavailable. Do not run daemon persistence tests, broad backend suites, or shared Shutdown.

## Real-surface acceptance actions and release blockers
On each OS, use a disposable user/profile + workspace and separately launched built desktop app; do NOT attach to or mutate current shared live session. Launch isolation mechanism is not verified here.
Open browser using URL address bar to a controlled fixture with canvas/WebGL checkerboard, image, iframe, long scroll and a button click counter. Implementer supplies fixture before QA; no existing fixture/server claimed.
Click Design Mode -> Inspect -> hover/click button: highlight tracks element, counter unchanged; Capture -> Preview: checkerboard/iframe pixels visible, toolbar/highlight absent. Cancel restores page interactivity.
Design Mode -> Rectangle -> reverse-direction drag across canvas/iframe; preview matches visible crop at 100% and non-default zoom, after scroll, and after moving window between mixed-DPI displays.
Hold test capture completion, reload same URL/resize/close/switch tab, release: no stale preview or send. Open global modal and drag pane: native surface stays masked; cancel does not resurrect hidden tabs.
Choose disposable local agent target, edit prompt, inspect final text/file/hash, Send once: exactly that draft reaches that target once; compare saved PNG hash to preview bytes. No writes to another active terminal.
Record OS/webview versions, screenshot dimensions/hash, event ordering and terminal receipt; API signatures do not establish canvas/WebGL/video/iframe compositor fidelity. Windows/Linux runtime proof remains a release blocker.
Decisions: approve file-reference local-agent handoff versus genuine provider image attachments (latter has no verified bridge); define explicit submit behavior and handed-artifact retention; approve caps and unsupported-context UX.
Recommendation: ship viewport-only + file-reference handoff first only after three native adapter proofs. Fail closed per platform on capture failure; do not silently call a pixel-less feature complete.
