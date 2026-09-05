# Design Mode producer - implementation receipt

Status: WORKING, owned implementation compiled; integration and cross-platform native capture incomplete.
No exports, BrowserPane, manifest, registry, gateway, daemon, stores or app roots changed.
Remote inventory enumeration 3791ce1 and all foreign dirty files are untouched.

## Shipped owned surfaces
- `ui/src/features/ferryx/design/model.ts`: instance/generation/operation/viewport identity, selection metadata, clipped reverse rectangles.
- `overlay.ts`: serializable injected page overlay, hover, element/rectangle selection, selector/ancestry/classes/computed CSS, explicit iframe metadata limitation, Escape/resize/scroll/pagehide/blur disposal. Never installed on terminal/app DOM; no body styling changes.
- `session.ts`: event subscription before arm, automatic native capture request, generation validation before/after asynchronous work, preserved failed/cancelled drafts, immutable shared ConfirmedDraft, explicit TargetRef, single-flight send and same request ID retries. Cancel invalidates sending without deleting the draft.
- `DesignFeedback.tsx`: existing Button/Input/Label/Select/theme, preview image, note, explicit image-capable target, confirm then send, error/empty/status states, Escape/focus restore, injected existing-mask acquisition. Selectors: design-mode-toggle, design-element, design-area, design-preview, design-note, design-target, design-send.
- `httpUploader.ts`: actual byte-body HTTP upload with target/hash/request metadata and verified receipt consistency; never a local path handed to a remote target.
- `src-tauri/src/ferryx_scope/design/mod.rs`: once-only native completion fence, bounded PNG decode/crop/encode, finite geometry, clip edges, measured X/Y pixel scale with DPR/zoom consistency.
- `native.rs`: real macOS WKWebView takeSnapshotWithConfiguration completion adapter; native NSImage -> NSBitmapImageRep PNG, main-thread ownership, owned bytes, 5s bounded completion, explicit errors. Uses existing objc2/block2 dependencies, not html2canvas or desktop capture.
- `src-tauri/tests/scoped_design.rs`: imports real library contracts and path-includes the production module, not a duplicate.

## RED/GREEN commands (repository root)

1. `bun run --cwd ui test src/features/ferryx/design/overlay.test.ts`
   - overlay-red.log: exit 1, 6 collected failures (empty selection events and wrong reverse bounds).
   - overlay-green.log: exit 0, 6 passed. Initial implementation exposed absent bare visualViewport in jsdom; fixed to window.visualViewport, no suppression.
2. `bun run --cwd ui test src/features/ferryx/design/session.test.ts`
   - session-red.log: exit 1, 2 collected behavioral failures (no staged bytes/draft and subscription absent).
   - session-green.log: exit 0, 2 passed.
   - cancel-red.log: exit 1, cancelled draft incorrectly delivered; cancel-green.log: exit 0, 3 passed.
3. `CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target CARGO_BUILD_JOBS=8 cargo test --manifest-path src-tauri/Cargo.toml --test scoped_design`
   - native-red.log/native-red.exit: exit 101, 3 collected failures. Invalidated callback accepted; crop 8x6 rather than 4x3/4x4.
   - native-green.log/native-green.exit: exit 0, 3 passed, macOS native adapter compiled. Shared library warnings remain outside owned code.
   - Earlier tool invocations timed out waiting for shared Cargo lock (not RED). No other Cargo processes killed, no alternate cache created.
4. `bun run --cwd ui test src/features/ferryx/design`: ui-green.log, exit 0, 3 files / 10 tests passed in one run.
5. `ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json`: typecheck.log, exit 0. Earlier foreign control replaceAll ES2020 diagnostic was not edited by this worker and later cleared by its owner.
6. `bun build ui/src/features/ferryx/design/DesignFeedback.tsx --target browser --packages external --outfile docs/evidence/ferryx-scope/implementation/design/component-build.js`: build.log, exit 0. Owned component bundle, NOT whole-app Vite build/integration.
7. `node /Users/indo/.agents/skills/impeccable/scripts/detect.mjs --json ui/src/features/ferryx/design/DesignFeedback.tsx`: ui-detector.log, exit 0, empty findings. No visual-review-agent tool available; no screenshot-based UI approval claimed.

## Real boundaries and cleanup
- `bun ui/src/features/ferryx/design/webview-proof.mjs`: webview.log, exit 0. Bun 1.4.0 WebKit WebView 320x240, persistent:false, data URL only. Actual page rectangle 120x64, computed blue CSS, click suppressed then restored, reverse area, Escape, resize invalidation, no overlay left. Actual WebView PNG signature verified (3517 bytes); this is not the Tauri native adapter being exercised.
- Rust real_webkit_viewport_crop test launches that script using a tempfile directory, decodes its actual PNG, crops production code, asserts native dimensions and interior RGB 20/100/200. Directory RAII cleanup and WebView close in finally; bounded 15s script deadline. Native-boundary.log/exit records final execution separately.
- `bun ui/src/features/ferryx/design/upload-proof.mjs`: upload.log, exit 0. Actual 127.0.0.1:0 HTTP bytes delivered to receiver-owned mode-0600 temporary file and compared. This is a transport proof with an 8-byte signature fixture, NOT signature-valid production image staging, SSH helper, auth policy, provider read or remote host certification. Server stopped and private directory removed in finally.
- No default daemon, PTY, credentials, provider stores, app/browser user profile, desktop launch, or live user session used. viewport.png is generated fixture evidence, not shipped UI asset.

## Exact integrator hooks and limits
1. Export `ferryx_scope::design` (worker deliberately cannot change module roots). Keep `png` enabled: currently provided by default native-terminal; make PNG dependency independent for any no-default-feature capture build.
2. Implement `DesignBridge` using real IPC/events. Native caller, not guest data, owns browser/webview identity and full operation nonce. Subscribe before begin; native child-ready acknowledgment required. Validate guest metadata/caps at Rust boundary; never grant capture/upload/delivery directly from guest messages.
3. Before capture, remove overlay and prove compositor presentation is flushed while keeping child visible; call native::capture_viewport, crop_png on blocking worker, then CaptureFence::complete against current native identity/viewport. Compute SHA256 from returned PNG. The producer removes overlay synchronously; compositor flush is NOT proven by JS removal alone.
4. Reuse BrowserManager generation and webview label. Explicitly invalidate the fence on document-start (including same-URL reload), navigation, resize, zoom, scroll, hide, close, recreate, selection replacement and unmount. Existing manager bounds/zoom paths do not increment generation. Never restore visibility directly: `maskPreview` must compose with BrowserPane's existing native mask.
5. Wire HttpAttachmentUploader to an authenticated same-origin staging route. Header contract: X-Ferryx-Target = encodeURIComponent(JSON.stringify(full TargetRef)); X-Ferryx-Sha256; X-Ferryx-Request-Id; Content-Type image/png; body actual bytes. Response shared {ok:true,data:AttachmentReceipt} or typed error. Enforce Control, target epoch/membership, image signature/hash/size, private jail, helper chunk upload + target-host hash verification under .ferryx/design-feedback, TTL and idempotency in integration-owned staging. Constructor URL is trusted app config, not guest-controlled.
6. Implement DesignDelivery.validateTarget/deliver against exact full target; reject unsupported media and stale epochs. Backend must dedupe requestId for uncertain retries. No focused-pane fallback. Return accepted/providerRead only with corresponding real evidence.
7. Windows currently returns CAPTURE_UNSUPPORTED, not pixels. Required direct `webview2-com = 0.38.2`, matching `windows = 0.61` with Win32_System_Com and Win32_UI_Shell; call controller.CoreWebView2().CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, retained IStream, CapturePreviewCompletedHandler), extract owned stream bytes on UI apartment after callback, bound timeout. Adapter implementation and Windows runner proof remain outstanding.
8. Linux currently returns CAPTURE_UNSUPPORTED. Required direct `webkit2gtk = 2.0.2` with v2_6 (existing Wry enables v2_40), `cairo-rs = 0.18` with png; WebViewExt::snapshot(SnapshotRegion::Visible, SnapshotOptions::NONE, cancellable, completion), Cairo PNG encoding on owning GLib context, owned bytes and timeout. Adapter implementation and Linux runner proof remain outstanding.
9. Native WKWebView runtime capture, canvas/WebGL/image/cross-origin frame compositor fidelity, DPR/zoom matrix, real remote agent marker recognition, desktop/browser/mobile final UI wiring and screenshots remain unavailable. No user-visible completion claim for unexported modules.

LSP globally unavailable by assignment and not retried. Applicable root/UI/Rust instructions and available frontend/impeccable skills read. Programming skill lookup did not locate a skill file; no claim it was read. No staging/commit/revert performed; owned changes remain uncommitted in the shared tree.
