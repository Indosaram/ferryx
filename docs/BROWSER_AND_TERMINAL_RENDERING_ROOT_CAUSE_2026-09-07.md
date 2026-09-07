# 내장 브라우저 렌더링 깨짐 / 브라우저 켜져 있을 때 터미널 미렌더링 — 원인 분석 및 수정 (2026-09-07)

두 증상은 **같은 macOS AppKit airspace 구조에서 나온 서로 다른 두 결함**이다. 원인 분석 후 수정까지 적용했다.

> 이 문서는 최초 조사본을 수정 결과에 맞춰 갱신한 것이다. 조사 단계에서 세운 가설 두 개(자식 웹뷰 투명 상속, 좌표계 이중 변환)는 이후 검증에서 **반증**되어 철회했다. 철회 근거는 §4에 남긴다.

---

## 1. 전제: 실제 뷰 계층

Ferryx 메인 윈도우의 `contentView` 아래에 **서로 형제(sibling)인 세 종류의 NSView**가 공존한다. 아래에서 위 순서로:

1. `FerryxNativeTerminalView` (WGPU 터미널 서피스) — `src-tauri/src/native_terminal/platform/macos.rs:275`에서 `addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None)`.
2. 메인 `WKWebView` (앱 UI). wry가 `ns_window.setContentView(parent_view)`로 심는다 — `wry-0.55.1/src/wkwebview/mod.rs:685`.
3. 브라우저용 자식 `WKWebView` — `window.add_child(...)` → `tauri-runtime-wry-2.11.4/src/lib.rs:5245`의 `WebviewKind::WindowChild` → `build_as_child` → wry `new_as_child`의 `ns_view.addSubview(&webview)` (`wry mod.rs:666`). `addSubview`는 맨 위에 붙이므로 **최상단**.

결정적 사실: `build_as_child`에 넘어가는 `RawWindowHandle`은 tao에서 **`ns_window.contentView()`** 다 (`tao-0.35.3/src/platform_impl/macos/window.rs:1567-1568`). 즉 브라우저 자식 웹뷰는 메인 WKWebView의 형제로 contentView 최상단에 놓인다.

2026-09-07의 토스트 가림 수정(`docs/NATIVE_TERMINAL_OVERLAY_BELOW_ORDERING_FIX_2026-09-07.md`)이 터미널을 `Above`에서 `Below`로 내린 이후, **터미널 픽셀은 `tauri.conf.json`의 `"transparent": true` + `ui/src/index.css:182-200`의 투명화 체인이 뚫어주는 구멍으로만 보인다.** 브라우저 자식 웹뷰는 그 체인 밖의 불투명 네이티브 뷰이면서 최상단이다. 따라서 **브라우저 웹뷰가 잘못된 사각형에 보이는 순간 그 아래 터미널은 통째로 사라지고, CSS z-index로는 절대 되돌릴 수 없다.**

---

## 2. 확정된 원인 두 가지

### 원인 A — 생성 직후 기본 사각형(0,0 800x600)이 보이는 채로 뜬다

`ui/src/state/workspaceStore.ts`의 `createBrowserTab`이 `visible: true`를 넘기면서 **bounds는 넘기지 않았다.** 백엔드 `cmd_browser_create`(`src-tauri/src/ipc/browser.rs`)는 `request.bounds`가 없으면 `LogicalPosition {0,0}` + `LogicalSize {800,600}`으로 자식 웹뷰를 만들고, 세션의 `visible`이 참이므로 즉시 `show()`한다.

결과: 브라우저를 새로 열 때마다 **윈도우 좌상단 800x600이 불투명하게 덮인다.** `BrowserPane`이 실제 pane 좌표를 보내기 전까지 그 사각형 안의 앱 크롬과 터미널 pane이 사라진다. `browserTauri.ts:119-136`의 `setBrowserBounds` 5회 재시도(`WEBVIEW_NOT_FOUND` 대기)가 이 창을 더 길게 만든다.

### 원인 B — show가 자기 bounds보다 먼저 도착할 수 있다

`BrowserPane.updateBounds()`는 `setBrowserBounds(...)`와 `setBrowserVisible(true)`를 **서로 await 하지 않는 별개의 IPC 두 개**로 쐈다. 둘 다 async Tauri 커맨드라 순서 보장이 없다. show가 먼저 적용되면 웹뷰는 **직전 레이아웃의 낡은 사각형에 그대로 노출된다** — 탭 전환, 분할 변경, 모달 해제 후 복귀 등 매 재노출마다 재현 가능한 경로다.

### 부수 원인 — 무음 실패가 진단을 막는다

`cmd_browser_set_visible` / `navigate` / `reload` / `set_zoom` / `focus`가 `if let Some(webview) = app.get_webview(...)` 패턴으로 **웹뷰가 없으면 조용히 `Ok(())`** 를 반환했고, `webview.set_bounds/show/hide/set_zoom/set_focus`의 결과는 `let _ =`로 버려졌다. 그래서 "로드되지 않는 브라우저"와 "숨겨지지 않은 채 터미널을 덮는 브라우저"가 둘 다 성공으로 보고됐다.

---

## 3. 적용한 수정

### 3.1 생성은 숨긴 상태로 (`ui/src/state/workspaceStore.ts`)
`createBrowserTab`이 `visible: false`로 생성한다. 백엔드에 pane 지오메트리가 없는 시점에는 절대 보이지 않고, `BrowserPane`이 실제 bounds를 확정한 뒤에만 노출한다. `browserSessionHydration.ts`는 이미 `visible: false`였으므로 이제 두 경로가 일치한다.

### 3.2 노출은 bounds 확인 이후에만 (`ui/src/components/BrowserPane.tsx`)
`updateBounds()`가 `setBrowserBounds`가 resolve된 뒤에만 `updateVisibility(true)`를 호출한다. `boundsSeq` 세대 카운터와 `disposed` 플래그로 보호한다:

- 최신 요청이 아닌 응답(`seq !== boundsSeq`)은 노출하지 않는다 — 리사이즈 연타 중 낡은 사각형으로 뜨는 것을 막는다.
- 숨김 경로(`!maskAwareVisible || loadError`)는 `boundsSeq`를 올려 **비행 중인 노출을 무효화**하고, 즉시 숨긴다. 숨김은 절대 지오메트리를 기다리지 않는다.
- cleanup은 `disposed = true` + `boundsSeq += 1`로 언마운트 후 노출을 차단한다.
- bounds 호출이 실패하면 노출하지 않는다. 잘못된 자리에 띄우는 것보다 안 띄우는 편이 안전하다.

### 3.3 무음 실패 제거 (`src-tauri/src/ipc/browser.rs`)
`cmd_browser_set_visible` / `navigate` / `reload` / `set_zoom` / `focus`가 웹뷰 부재 시 `BrowserError::WebviewNotFound`를 반환한다(`cmd_browser_set_bounds` / `find`가 이미 쓰던 계약과 일치). `set_bounds` / `show` / `hide` / `set_zoom` / `set_focus`의 실패도 더 이상 버리지 않는다. 특히 `set_bounds` 실패 전파는 3.2가 성립하기 위한 전제다 — 프런트엔드가 이 resolve를 노출 조건으로 쓰기 때문이다.

### 3.4 회귀 테스트
- `ui/src/components/BrowserPane.masking.test.tsx`: "bounds가 확인되기 전에는 노출하지 않는다"(지연 프로미스로 검증), "bounds 실패 시 절대 노출하지 않는다". 기존 마스킹 테스트의 "보임" 단정은 비동기가 된 계약에 맞춰 `waitFor`로 바꿨고, "숨김" 단정은 동기 그대로 두어 **숨김이 지오메트리를 기다리지 않음**을 계속 강제한다.
- `src-tauri/src/browser/tests.rs`: `cmd_browser_set_visible`(show/hide 양쪽)과 `cmd_browser_navigate`가 웹뷰 부재 시 `WebviewNotFound`를 반환하는지 검증.

---

## 4. 철회한 가설 두 개 (조사본 대비 정정)

**철회 1 — "자식 웹뷰가 투명 속성을 상속해 아래가 비쳐 보인다."** 반증: `tauri-runtime-2.11.x/src/webview.rs`의 `WebviewAttributes::new`가 `transparent: false`를 기본값으로 둔다. 윈도우의 `transparent: true`는 윈도우 자신의 웹뷰에만 `config.transparent` 경로로 적용된다. 자식 웹뷰는 불투명하게 생성되므로 `drawsBackground = NO`(`wry mod.rs:376`) 경로에 진입하지 않는다. 따라서 `.transparent(false)`를 명시하는 수정은 불필요해 넣지 않았다.

**철회 2 — "titlebar overlay 때문에 DOM 좌표와 contentView 좌표가 어긋난다."** 반증: wry가 `ns_window.setContentView(&parent_view)`로 `WryWebViewParent`를 contentView로 만들고 메인 웹뷰가 autoresizing으로 이를 가득 채운다. 자식 웹뷰의 superview는 그 동일한 contentView이므로 `window_position`의 `parent.height - y - height` 뒤집기(`wry mod.rs:1425-1443`)는 DOM 원점과 정합한다. 추측만으로 좌표 변환을 고치는 것은 위험해서 손대지 않았다.

**이미 고쳐져 있던 항목.** `docs/BUILTIN_BROWSER_CODE_REVIEW_2026-09-07.md`의 F2(pane으로 닫은 브라우저의 웹뷰 누수)와 F5(생성 실패 무음 통과)는 그 사이 다른 세션이 수정했다. `closeTab`/`closePane` 모두 leaf content kind로 분기해 `closeBrowser`를 호출하고(`workspaceStore.ts`), `cmd_browser_create`는 oneshot + 5초 타임아웃으로 생성 완료를 기다린 뒤 실패 시 세션을 롤백한다. 중복 수정하지 않았다.

**의도적으로 바꾸지 않은 것.** `TerminalSplitView.tsx:488`의 `browserPanesVisible: activeDrag === null`은 전역이다. 터미널과 달리 브라우저 자식 웹뷰는 **pointer-transparent가 아니라 포인터 입력을 삼키므로**, 드래그 중 전역 숨김은 dnd-kit이 동작하기 위한 필수 조건이다(`TerminalSplitView.browserDragVisibility.runtime.test.tsx`가 이 계약을 강제한다). 터미널의 pane 단위 yield와 대칭을 맞추려던 초안은 이 이유로 폐기했다.

---

## 5. 검증

- `cargo check --manifest-path src-tauri/Cargo.toml`: exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: **708 passed, 0 failed**.
- `bun run --cwd ui build` (tsc + vite): exit 0.
- 브라우저 관련 프런트엔드 스위트(BrowserPane 5종 + TerminalSplitView 드래그 가시성): 19/19 passed.
- `ui/src/state` 전체 + `browserTauri.test.ts`: 275/275 passed.
- 전체 `vitest run`: 1770 passed / 18 failed(7개 파일). **동일한 7개 파일·18개 테스트가 `git worktree`로 뜬 HEAD 기준선에서도 똑같이 실패**하므로 전부 선행 실패이며 이 변경과 무관하다(App.test, appearanceThemeContract, AgentsSection, SettingsDialog, push/client, lib/tauri, terminalTransport).
- **RED→GREEN 양방향 확인.** BrowserPane의 노출 순서를 수정 전으로 되돌리면 신규 프런트엔드 테스트 2개가 실패하고 복구 시 통과한다. `set_visible` 가드를 되돌리면 신규 Rust 테스트가 `missing webview must not report success`로 실패하고 복구 시 통과한다. 수정 없이도 통과하는 테스트는 없다.
- 리뷰 수정(`BrowserSection`) 이후 재검증: `bun run --cwd ui build` exit 0, `src/components/settings` + BrowserPane/BrowserToolbar 스위트에서 실패는 선행 실패 2개 파일(AgentsSection 2건, SettingsDialog 7건)뿐이며 BrowserPane·BrowserToolbar·BrowserSection 관련 파일은 전부 통과.

미검증 경계: 실제 macOS 데스크톱에서의 육안 확인은 하지 않았다. 컴포지터 Z 순서와 픽셀 결과는 코드 경로 추적과 자동화 테스트로만 뒷받침된다.

## 6. 코드리뷰 결과

위임 리뷰어 3종(opencode/Kimi, Anthropic, OpenAI Codex)이 모두 인증·크레딧 오류로 기동하지 못해 동일 체크리스트로 직접 리뷰했다. 발견/판정은 다음과 같다.

**High — 처리되지 않는 프로미스 거부 1건 (수정 완료).** `cmd_browser_focus`가 이제 `WebviewNotFound`로 거부될 수 있는데, `ui/src/components/settings/BrowserSection.tsx`의 Focus 버튼만 `void focusBrowser(...)`로 catch 없이 호출하고 있었다. `cmd_browser_list`는 닫히는 중인 세션도 열거할 수 있으므로 실제로 도달 가능한 unhandled rejection이다. `BrowserToolbar.handleFocus`의 기존 선례에 맞춰 catch를 추가했다. 나머지 호출부는 모두 이미 안전하다: `setBrowserVisible`은 `BrowserPane` 한 곳뿐이고 `.catch(() => undefined)`, `setBrowserZoom`/`focusBrowser`의 `BrowserToolbar` 경로(214/224/232)는 전부 `try/catch`, `BrowserSection`의 zoom 경로(60/147)도 `try/catch`와 `.catch`로 감싸져 있다. `navigateBrowser`/`reloadBrowser`는 `App.tsx`에서 `reportRuntimeError`로 토스트를 띄우는데, 이는 **의도한 동작**이다 — 웹뷰 없이 탐색이 무음 성공하던 것이 원래 결함이었다.

**세대 가드는 경합에 안전하다.** `disposed`와 `boundsSeq`는 effect 지역 변수라 effect가 재실행될 때마다 새 클로저가 새 카운터를 갖고, 이전 클로저는 cleanup에서 `disposed = true`가 박힌다. 따라서 A→B→A처럼 `browserId`가 왕복해도 옛 응답이 새 소유자를 건드릴 수 없다(문자열 비교만으로는 못 막는 사례 — `ferryx-native-rendering-repair-20260906`의 동일 교훈). StrictMode 이중 호출은 mount→cleanup→mount이므로 첫 클로저가 무효화되어 안전하다. 리사이즈 연타는 `seq !== boundsSeq`로 마지막 요청만 노출한다. 숨김 경로도 `boundsSeq`를 올려 비행 중인 노출을 무효화하며, 숨김 자체는 지오메트리를 기다리지 않는다.

**`visible: false` 생성이 브라우저를 영구히 숨길 수 있는가 — 조건부로 가능하나 현재 도달 불가.** 노출을 여는 유일한 열쇠가 `setBrowserBounds`의 resolve이므로, bounds 호출이 항상 거부되는 경로가 있다면 브라우저는 영원히 보이지 않는다. 실제 경로를 점검한 결과: `cmd_browser_create`가 oneshot으로 웹뷰 생성 완료를 기다린 뒤 반환하므로 `BrowserPane` 마운트 시점에는 웹뷰가 존재하고, macOS wry `set_bounds`는 자식 웹뷰에 대해 `Ok(())`를 반환한다. `createBrowserTab` / `duplicateBrowserTab`(전자를 재사용) / `browserSessionHydration`(원래 `visible:false`) / `tabPaneDrop`(기존 browserId 재사용) 모두 동일하게 `BrowserPane` 마운트를 통해 노출된다. **다만 이것은 감수한 위험이다**: bounds가 실패하면 잘못된 자리에 띄우는 대신 띄우지 않는 쪽을 택했다. 터미널을 통째로 가리는 것보다 브라우저가 안 보이는 편이 덜 나쁘다는 판단이며, 실패는 이제 `WebviewNotFound`/`Internal`로 관측 가능하다.

**테스트 정직성.** 지연 프로미스 테스트는 `setBrowserBounds`를 수동으로 resolve하기 전에 `setBrowserVisible(browserId, true)`가 **호출되지 않았음**을 단정하고, resolve 후에야 노출을 확인한다. 노출을 무조건 실행하는 옛 구현에서는 첫 단정이 깨지므로 잘못된 이유로 통과할 수 없다(실제로 되돌려 확인함). 기존 마스킹 테스트에서 "숨김" 단정을 동기로 남긴 것도 의도적이다 — 숨김이 bounds를 기다리기 시작하면 그 단정이 깨진다.

**Rust diff의 오류 의미 변화.** 정상 케이스를 오류로 바꾸는 지점은 없다. 모든 신규 오류는 "웹뷰가 있어야 하는데 없다" 또는 "AppKit 호출이 실패했다"이며, 둘 다 이전에는 성공으로 보고되던 진짜 실패다. `cmd_browser_set_bounds`/`find`가 이미 쓰던 계약과 일치시킨 것이라 프런트엔드의 `isWebviewNotFoundError` 처리(`browserTauri.ts:106-136`)가 그대로 적용된다.

## 7. 근거 위치

- `src-tauri/src/native_terminal/platform/macos.rs:147-166, 227-280` — 뷰 부착 순서, `apply_viewport` 좌표 변환, `setOpaque: true`.
- `src-tauri/src/ipc/browser.rs` — `cmd_browser_create`의 기본 800x600 bounds, 각 커맨드의 `WebviewNotFound` 계약.
- `ui/src/components/BrowserPane.tsx:190-250` — `maskAwareVisible`, `updateBounds`, 노출 순서 가드.
- `ui/src/lib/browserTauri.ts:44-57, 119-140` — lifecycle 큐, bounds 재시도.
- `ui/src/index.css:182-200`, `src-tauri/tauri.conf.json` — macOS 투명화 체인과 `transparent: true`.
- `wry-0.55.1/src/wkwebview/mod.rs:366-382, 660-690, 1010-1043, 1425-1443`; `tauri-2.11.5/src/window/mod.rs:1129-1146`; `tauri-runtime-wry-2.11.4/src/lib.rs:4816-4824, 5245`; `tao-0.35.3/src/platform_impl/macos/window.rs:1567-1569`.

## 주의

이 저장소 작업 트리는 여러 세션이 공유한다. 위 변경은 **커밋되지 않은 상태**이며 다른 세션에 의해 덮일 수 있다. 커밋이 필요하면 알려달라.
