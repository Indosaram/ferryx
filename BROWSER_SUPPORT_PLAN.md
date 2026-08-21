# rorca Embedded Webview Browser 지원 계획

> 대상: `orca-lite` / rorca  
> 목표: 원본 Orca의 browser tab 경험을 Tauri v2 구조에 맞게 재설계하여, 외부/로컬 웹사이트를 워크스페이스 내부에서 안전하게 탐색할 수 있도록 한다.  
> 작성 기준일: 2026-08-21

---

## 0. 결론

rorca의 내장 브라우저는 **Tauri v2 Multi-webview의 Child Webview**를 사용해 구현하는 것이 적합하다.

- `WebviewWindow`는 별도 최상위 OS 창이므로 탭/분할 pane 내부에 자연스럽게 임베딩할 수 없다.
- `<iframe>`은 `X-Frame-Options`, `frame-ancestors`, mixed-content, 사이트별 CSP 때문에 일반 웹 브라우저 용도로 사용할 수 없고, 브라우저 히스토리/쿠키/프로필/포커스도 충분히 제어할 수 없다.
- 따라서 메인 rorca UI는 기존 Tauri app webview로 유지하고, 각 browser pane의 실제 페이지 렌더러는 메인 창의 **native child webview**로 둔다.
- React는 탭, toolbar, pane tree, focus routing, child webview의 위치/크기만 관리하고, 실제 페이지 탐색과 browsing data는 Rust/Tauri browser runtime이 소유한다.

다만 이 기능은 일반 UI 기능과 달리 **임의의 외부 코드를 앱 프로세스 안의 webview에서 실행**하게 하므로, 구현 순서상 보안 경계 정비가 브라우저 생성보다 먼저 와야 한다.

특히 현재 저장소 상태에서는 다음 두 항목이 browser child webview 생성의 선행 조건이다.

1. `src-tauri/capabilities/default.json`이 현재 `"windows": ["main"]`을 사용한다. Tauri v2 multi-webview에서는 window capability가 해당 창의 모든 child webview에 적용되므로, browser child가 추가되면 이 설정을 그대로 둘 수 없다. app shell capability를 `"webviews": ["main"]`으로 좁혀야 한다.
2. `src-tauri/build.rs`는 현재 `tauri_build::build()`만 호출한다. Tauri app의 `invoke_handler`에 등록한 application command는 별도 app manifest ACL을 만들지 않으면 기본적으로 모든 app window/webview에서 호출 가능하다. browser child를 만들기 전에 `AppManifest::commands(...)`로 현재 및 신규 app command를 permission 대상으로 등록하고, main app webview에만 allow permission을 부여해야 한다.

즉, **"child webview를 먼저 만들고 나중에 보안을 강화"하는 순서는 허용하지 않는다.**

---

## 1. 범위와 parity 기준

### 1.1 이번 기능의 목표

첫 번째 안정 버전에서 다음 경험을 제공한다.

- 워크스페이스 안에서 Browser tab 생성
- `http://` / `https://` URL 탐색
- 뒤로 / 앞으로 / reload
- 주소 입력 및 현재 URL 표시
- 현재 title 반영
- 로딩 상태 및 load error 반영
- 외부 기본 브라우저로 열기
- page zoom 조절
- terminal tab과 browser tab의 공존
- 현재 pane tree와의 결합 및 split layout 대응
- 로컬 개발 서버 포트(`localhost:<port>`)를 Browser tab에서 열기
- 탭 전환 시 browser history/session 유지
- 탭 닫기/워크스페이스 정리 시 native webview 리소스 해제
- 외부/localhost 페이지에서 Tauri IPC 및 앱 권한에 접근할 수 없도록 격리

### 1.2 첫 버전에서 복제하지 않는 원본 Orca 기능

원본 Orca의 browser 구현 전체를 그대로 이식하지 않는다. 다음은 후속 범위다.

- remote browser runtime / screencast
- browser automation / agent-browser 제어
- annotation / element picker / screenshot capture
- 브라우저 내부 다중 page workspace 모델
- download manager
- credential/password manager
- per-site permission UI (camera/mic/geolocation)
- favicon 완전 동기화
- DevTools UI 노출(개발 빌드 내부 도구 제외)
- background browser LRU/eviction의 원본 수준 최적화
- browser session restore의 완전한 원본 parity

MVP에서는 **1개의 `BrowserTab`이 1개의 기본 browser runtime/page를 소유**한다. pane split이 필요한 경우 각 leaf가 독립적인 browser runtime id를 갖도록 확장 가능하게 모델링한다.

---

## 2. 분석 기준점: 현재 rorca 상태

### 2.1 기존 분석 문서

`RORCA_EMBEDDED_BROWSER_SUPPORT_ANALYSIS.md`의 결론처럼 현재 rorca에는 embedded browser backend/IPC/UI가 존재하지 않는다. 이번 문서는 그 gap을 실제 구현 가능한 구조로 메우는 계획이다.

### 2.2 Tauri backend 현황

현재 주요 구조는 다음과 같다.

- `src-tauri/src/lib.rs`
  - PTY, remote gateway, worktree registry, notification service 등을 `manage(...)`
  - terminal/worktree/project/notification/remote command를 `invoke_handler(...)`에 직접 등록
- `src-tauri/src/ipc/mod.rs`
  - `terminal`, `worktree`, `project`, `preferences`, `remote`, `notifications`
  - browser 모듈 없음
- `src-tauri/src/ipc/terminal.rs`
  - async Tauri command + managed service + typed DTO + event emit 패턴
  - browser IPC도 이 패턴을 따르는 것이 일관적이다.
- `src-tauri/Cargo.toml`
  - `tauri = { version = "2", features = ["test"] }`
  - Child Webview에 필요한 `WebviewBuilder`는 현재 Tauri에서 `unstable` feature 대상이므로 feature/버전 정책 변경이 필요하다.
- `src-tauri/tauri.conf.json`
  - main window 1개
  - app UI용 CSP 존재
- `src-tauri/capabilities/default.json`
  - `windows: ["main"]`이므로 multi-webview 보안 경계 변경 필요
- `src-tauri/build.rs`
  - app command ACL manifest 미설정

현재 rorca에서 `port`라는 이름으로 존재하는 backend 기능은 remote gateway의 listen port가 주된 것이며, 원본 Orca의 workspace development port preview/forwarding 계층은 없다.

### 2.3 frontend 현황과 pane-tree 리팩터 주의점

현재 작업 트리에는 pane layout 리팩터가 진행된 상태가 보인다.

- `ui/src/lib/types.ts`
  - `LayoutState.tabs: TerminalTab[]`
  - `activeTabId`
  - `layoutsByTabId`
  - `TabPaneLayout.root`
  - `TabPaneLayout.sessionIdsByLeafId`
- `ui/src/state/paneTree.ts`
  - recursive split tree 도입
- `ui/src/state/layout.ts`
  - `SPLIT_PANE`, `CLOSE_PANE`, `FOCUS_PANE`, `SET_PANE_RATIO`, `SWAP_PANES` 등으로 확장
- `ui/src/state/workspaceStore.ts`
  - pane별 terminal session lifecycle 관리로 확장 중

반면 현재 `ui/src/App.tsx`, `ui/src/components/TerminalSplitView.tsx` 일부는 여전히 `primaryTabId`, `split`, `enableSplit` 등 이전 API를 참조하고 있어 과도기 상태다.

따라서 browser 작업은 **pane-tree 리팩터가 green 상태로 정리된 리비전 위에 rebase/진행**해야 한다. browser 구현이 이 과도기 상태를 우회하기 위해 또 다른 layout abstraction을 만들면 안 된다.

---

## 3. 원본 Orca 내장 브라우저 분석

원본 bundle의 symbol/callsite를 기준으로 browser 동작을 추적했다.

### 3.1 `createBrowserTab`과 browser state 모델

관련 파일:

- `ui/original-dist/assets/web-session-tabs-sync-CYKZbAxS.js`
- `ui/original-dist/assets/App-D7xQsIRS.js`
- `ui/original-dist/assets/unsaved-close-queue-Bzv89tJf.js`

원본은 browser를 단순 `url` 문자열 하나로 취급하지 않고 크게 두 층으로 나눈다.

#### Browser page state

확인되는 대표 필드:

- `id`
- `workspaceId`
- `worktreeId`
- `url`
- `title`
- `loading`
- `faviconUrl`
- `canGoBack`
- `canGoForward`
- `loadError`
- `createdAt`
- `browserRuntimeEnvironmentId`
- `viewportPresetId`

#### Browser workspace/tab state

대표 필드:

- `id`
- `worktreeId`
- `label`
- `sessionProfileId`
- `activePageId`
- `pageIds`
- 현재 active page의 `url/title/loading/favicon/canGoBack/canGoForward/loadError` mirror

그리고 unified tab layer에서는 browser를 `contentType: "browser"`로 일반 탭과 함께 취급한다.

### 3.2 rorca에서 가져올 것 / 단순화할 것

가져와야 하는 핵심은 다음이다.

- terminal/browser를 동일한 tab/layout 시스템에 넣는 것
- navigation state를 UI store에 mirror하는 것
- native browser surface를 tab lifecycle과 분리하지 않는 것
- profile/session id를 browser runtime 생성 시점에 고정하는 것
- worktree ownership을 browser tab에도 유지하는 것

반면 MVP에서 원본의 `BrowserWorkspace -> BrowserPage[]` 이중 구조까지 가져올 필요는 없다.

rorca MVP는 다음으로 단순화한다.

- `BrowserTab` 1개 = 기본 child webview 1개
- split 시 필요하면 별도 `browserId`를 가진 pane content를 생성
- 원본의 page sub-tab 기능은 후속 단계

### 3.3 persistent guest webview와 navigation state sync

`unsaved-close-queue-Bzv89tJf.js`에서 원본 browser pane은 guest webview를 만들고 다음 이벤트를 연결한다.

- attach / DOM ready
- render process gone / destroyed
- focus
- load start / stop
- top-level navigate / in-page navigate
- title update
- favicon update
- load failure

navigation이 바뀔 때마다 `canGoBack()`, `canGoForward()`, URL/title/loading/error를 store로 다시 동기화한다.

즉, rorca에서도 React가 history를 추측해서는 안 되고, **native webview가 source of truth**가 되어야 한다.

### 3.4 toolbar

원본 browser toolbar의 핵심 parity 항목:

- Back
- Forward
- Reload
- URL/address input
- 외부 기본 브라우저로 열기
- loading/retry 상태

원본에는 hard reload, stop, devtools, grab/annotation 등 더 많은 기능이 있으나 MVP 필수는 아니다.

### 3.5 session partition

원본은 대략 다음 우선순위로 browser partition/profile을 선택한다.

- 명시된 `sessionPartition`
- 선택된 session profile partition
- default browser profile partition
- fallback partition
- 최종 기본 `persist:orca-browser`

따라서 rorca도 tab마다 임의 cookie jar를 직접 관리하기보다 `sessionProfileId`라는 abstraction을 두고, platform이 지원하는 storage context를 profile에 대응시키는 것이 맞다.

### 3.6 focus와 shortcut

원본은 주소창 focus와 webview focus를 명시적으로 전환하며 browser가 active pane일 때만 history/reload/find/zoom shortcut을 적용한다.

이 부분은 rorca에서 더 중요하다. Tauri Child Webview는 메인 React webview와 별도 native focus surface이므로 `window.addEventListener("keydown")`만으로는 child가 focus된 동안 shortcut을 잡을 수 없다.

### 3.7 tab split / layout

관련 파일:

- `ui/original-dist/assets/Terminal-qm6WvB4Q.js`
- `ui/original-dist/assets/web-session-tabs-sync-CYKZbAxS.js`

원본은 tab group과 layout tree를 분리하고, terminal/browser의 persistent surface를 해당 group body rectangle에 anchor한다.

browser surface는 background에서도 보존될 수 있고 active group/page만 paint/input 가능 상태가 된다. background worktree browser guest는 resource pressure에 따라 eviction할 수 있다.

rorca에서는 CSS anchor overlay를 그대로 복제하지 않고, **React placeholder의 screen rect -> Tauri Child Webview bounds** 동기화로 같은 역할을 구현한다.

### 3.8 `openWorkspacePortInBrowser`

관련 파일:

- `ui/original-dist/assets/workspace-port-localhost-label-selector-BFjPG9iO.js`

확인되는 동작:

1. port에 대한 browser URL을 만든다.
2. local target이면 localhost worktree labeling/route를 적용할 수 있다.
3. 설정 또는 modifier에 따라 Orca browser가 아니라 system browser로 열 수 있다.
4. port owner의 worktree를 찾아 활성화한다.
5. remote runtime이면 remote browser tab RPC/screencast 쪽으로 라우팅한다.
6. local runtime이면 `createBrowserTab(worktreeId, url, { activate: true })`로 연다.

rorca의 terminal/worktree는 현재 local execution이므로 MVP에는 실제 SSH port forwarding이 필요하지 않다. `localhost:<port>` 자체가 접근 가능하다. 단, API는 이후 remote target이 들어와도 browser layer를 뜯어고치지 않도록 **"port target resolution"과 "browser open"을 분리**한다.

---

## 4. 채택 아키텍처: Tauri v2 Child Webview

### 4.1 선택

**채택: `tauri::webview::WebviewBuilder` + main `Window::add_child(...)`**

이 구조에서:

- main Tauri webview: React/TabBar/toolbar/pane chrome
- child webview: 실제 외부 사이트 렌더링
- 각 child는 고유 label, browser runtime id, session profile을 갖는다.
- child bounds는 해당 browser pane의 content rectangle과 동기화한다.

### 4.2 왜 `WebviewWindow`가 아닌가

`WebviewWindow`는 webview 1개를 별도의 최상위 OS window에 넣는 API다.

단점:

- 기존 tab body에 자연스럽게 clip할 수 없음
- split pane resize와 연동이 불안정
- 메인 titlebar/window move/minimize lifecycle을 별도 관리해야 함
- 원본 Orca의 pane-inside-workspace UX와 다름

따라서 fallback/debug용 별도 preview window가 필요할 때만 사용할 수 있고, 주 구현으로 사용하지 않는다.

### 4.3 왜 iframe이 아닌가

일반 웹사이트는 다음 이유로 iframe 기반 browser에 적합하지 않다.

- `X-Frame-Options`
- `Content-Security-Policy: frame-ancestors`
- mixed content
- cross-origin history 제어 제한
- site login/cookie context 제어 제약
- 특정 사이트가 embedding을 명시적으로 차단

그리고 iframe을 허용하려고 main app CSP를 넓히는 것은 보안상 역방향이다.

### 4.4 Cargo/Tauri 버전 정책

Child Webview용 `WebviewBuilder`는 현재 Tauri 2.x에서 `unstable` feature 대상이다.

따라서 구현 시:

- `tauri` feature에 `unstable` 추가
- 현재처럼 단순 `version = "2"`로 두지 말고, multi-webview 및 `with_webview` platform handle을 쓰는 동안 **검증된 minor 버전으로 pin**
- 플랫폼 native handle crate 버전도 해당 Tauri minor와 맞춘다.

예상 변경 방향:

```toml
# 실제 버전은 Phase 0 spike에서 선택/고정한다.
tauri = { version = "~2.x.y", features = ["test", "unstable"] }
```

Tauri 문서도 `with_webview`로 native handle을 사용할 경우 minor release에서 platform crate가 바뀔 수 있으므로 최소 minor pin을 권장한다.

---

## 5. Rust backend 설계

### 5.1 모듈 구조

권장 구조:

```text
src-tauri/src/
  browser/
    mod.rs
    manager.rs
    model.rs
    navigation.rs
    profile.rs
    security.rs
  ipc/
    browser.rs
```

`src-tauri/src/lib.rs`에서는 `BrowserManager`를 managed state로 등록하고 browser command/event를 handler에 추가한다.

### 5.2 BrowserManager 책임

`BrowserManager`는 native webview 객체 자체를 장기 lock 안에 보관하기보다 다음 metadata를 registry로 관리한다.

```text
BrowserId -> {
  webview_label,
  workspace_id,
  worktree_path,
  session_profile_id,
  generation,
  url,
  title,
  loading,
  can_go_back,
  can_go_forward,
  zoom_factor,
  load_error,
  visible
}
```

실제 `Webview` handle이 필요할 때는 app manager에서 label로 조회한다.

이점:

- managed state가 platform webview internals와 강하게 결합되지 않음
- close/recovery가 idempotent해짐
- registry unit test 가능
- stale event를 `generation`으로 거를 수 있음

### 5.3 Browser ID/label

- frontend tab id를 Tauri webview label로 그대로 쓰지 않는다.
- backend가 UUID 기반 opaque `browserId`를 생성한다.
- webview label은 안전한 형식으로 `browser-<uuid>`를 생성한다.
- browser child label pattern은 capability에 **절대 allow하지 않는다.**

### 5.4 생성 lifecycle

`cmd_browser_create`의 기본 흐름:

1. request 검증
2. URL policy 검증
3. profile/runtime capability 확인
4. backend UUID/browser label 생성
5. main window 조회
6. `WebviewBuilder::new(label, WebviewUrl::External(url))`
7. security callback 연결
8. profile data store 설정
9. child webview 생성
10. 초기 bounds 적용
11. 필요 시 숨김 상태로 시작
12. registry 등록
13. 초기 `BrowserState` 반환

Windows에서 webview 생성은 synchronous command/event handler 안에서 deadlock 위험이 있으므로 **async command 경로**만 사용한다.

### 5.5 Builder callback

MVP에서 최소 다음 callback을 연결한다.

- `on_navigation`
  - top-level URL allow/deny policy
  - blocked internal scheme 방어
- `on_page_load`
  - started -> `loading = true`
  - finished -> URL/history state refresh, `loading = false`
- `on_document_title_changed`
  - title state update
- `on_new_window`
  - unmanaged popup 생성 금지
  - 허용 가능한 HTTP(S) target이면 `browser_open_requested` event를 main UI로 emit하고 native popup은 deny
- `on_download`
  - MVP에서는 자동 다운로드를 허용하지 않음
  - 후속 download manager가 생길 때 명시적 user-confirm flow로 변경

favicon은 Tauri high-level callback이 원본 Electron만큼 직접적이지 않으므로 MVP에서 필수 상태로 만들지 않는다.

### 5.6 navigation/history adapter

Tauri high-level `Webview`는 `navigate`, `reload`, `set_zoom`, `set_bounds`, `show/hide`, `set_focus`, `close` 등을 제공하지만, 현재 high-level API에는 원본 Electron처럼 `goBack/canGoBack/goForward/canGoForward`가 직접 노출되지 않는다.

반면 Tauri가 사용하는 WRY/native engine에는 history API가 있다. 따라서 browser 기능은 다음 abstraction을 둔다.

```text
BrowserNavigationAdapter
  go_back(browser)
  go_forward(browser)
  read_history_capabilities(browser) -> { can_go_back, can_go_forward }
  install_focus_observer(browser)     -> optional/platform-specific
```

구현은 `Webview::with_webview(...)`를 통해 플랫폼 handle을 사용한다.

- macOS: WKWebView history API
- Windows: WebView2 history API
- Linux: WebKitGTK history API

**고정 JavaScript `history.back()`를 주 구현으로 삼지 않는다.**

이유:

- `canGoForward`를 신뢰성 있게 구하기 어려움
- redirects / same-document / site script history mutation을 정확히 반영하기 어려움
- browser chrome 상태의 source of truth가 native engine이어야 함

단, platform adapter 구현의 임시 fallback으로 app-owned 고정 script를 사용할 수는 있으나, 사용자 URL/문자열을 JavaScript source에 interpolation해서는 안 된다.

### 5.7 state event

Rust -> frontend event는 여러 개로 쪼개기보다 우선 하나의 typed snapshot event를 사용한다.

```ts
type BrowserStateChangedPayload = {
  browserId: string;
  generation: number;
  url: string;
  title: string | null;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomFactor: number;
  loadError: string | null;
};
```

이벤트 이름 예:

```text
browser_state_changed
```

추가 event:

```text
browser_open_requested
browser_runtime_closed   # 비정상/native close 대응이 필요할 경우
browser_focus_changed    # platform focus observer가 확보되면 사용
```

frontend는 이미 닫힌 `browserId` 또는 generation이 다른 stale event를 무시한다.

---

## 6. IPC 계약

`ui/src/lib/tauri.ts`를 계속 확장할 수 있으나 browser command 수가 많아지므로 `ui/src/lib/browser/tauri.ts`로 분리하고 barrel/re-export하는 편이 낫다.

### 6.1 command 목록

| Command | 입력 | 출력 | 비고 |
|---|---|---|---|
| `cmd_browser_create` | workspace/worktree/url/profile/initial bounds | `BrowserState` | child 생성 |
| `cmd_browser_navigate` | browserId, url | `void` | Rust에서 URL 재검증 |
| `cmd_browser_reload` | browserId | `void` | MVP normal reload |
| `cmd_browser_go_back` | browserId | `void` | native history adapter |
| `cmd_browser_go_forward` | browserId | `void` | native history adapter |
| `cmd_browser_get_state` | browserId | `BrowserState` | canGoBack/Forward 포함 |
| `cmd_browser_set_zoom` | browserId, factor | `BrowserState` 또는 `void` | 범위 clamp |
| `cmd_browser_set_bounds` | browserId, rect | `void` | logical pixel 검증 |
| `cmd_browser_set_visible` | browserId, visible | `void` | inactive tab 보존 |
| `cmd_browser_focus` | browserId | `void` | child focus |
| `cmd_browser_close` | browserId | `void` | idempotent destroy |
| `cmd_browser_list` | - | summary[] | diagnostics/leak test |
| `cmd_browser_open_external` | url | `void` | HTTP(S) 재검증 후 OS browser |
| `cmd_browser_clear_profile_data` | profileId | result | profile settings 단계 |

`canGoBack`, `canGoForward`를 별도 boolean command로 매번 호출하는 대신 `cmd_browser_get_state`와 state event에 포함한다. toolbar는 store snapshot을 렌더링하고, backend가 navigation 발생 시 값을 갱신한다.

### 6.2 IPC error code

기존 `StructuredIpcError` 규칙에 맞춰 최소 다음 code를 고정한다.

```text
BROWSER_NOT_FOUND
BROWSER_URL_INVALID
BROWSER_URL_SCHEME_DENIED
BROWSER_PROFILE_UNSUPPORTED
BROWSER_PLATFORM_UNSUPPORTED
BROWSER_HISTORY_FAILED
BROWSER_BOUNDS_INVALID
BROWSER_CREATE_FAILED
BROWSER_NAVIGATION_FAILED
BROWSER_CLOSE_FAILED
```

UI가 문자열 parsing으로 error type을 추측하지 않게 한다.

---

## 7. 보안 설계 — 구현의 최우선 조건

### 7.1 threat model

browser child에는 다음을 모두 **untrusted remote content**로 본다.

- 인터넷 HTTPS 사이트
- 일반 HTTP 사이트
- `localhost`
- `127.0.0.1`
- worktree가 띄운 dev server

개발자가 직접 실행한 localhost 앱이라고 해서 Tauri IPC를 허용하면 안 된다. dependency/XSS를 통해 즉시 terminal/worktree command까지 연결될 수 있기 때문이다.

### 7.2 capability를 window 단위에서 webview 단위로 변경

현재:

```json
{
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging"
  ]
}
```

multi-webview에서는 window label이 match하면 같은 창의 child webview도 permission을 받는다.

변경 방향:

```json
{
  "identifier": "app-shell",
  "webviews": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging",
    "... generated allow permissions for approved app commands ..."
  ]
}
```

그리고 `tauri.conf.json`에서 enabled capability를 명시적으로 `app-shell`로 고정해, capabilities directory에 나중에 파일이 추가되어도 의도치 않게 자동 활성화되는 위험을 줄인다.

browser label(`browser-*`)은 어떤 local/remote capability에도 포함하지 않는다.

### 7.3 custom app command ACL 등록

현재 `build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

변경 방향:

```text
tauri_build::try_build(
  Attributes::new().app_manifest(
    AppManifest::new().commands(&[
      현재 cmd_* 전체,
      신규 cmd_browser_* 전체,
    ])
  )
)
```

그러면 command별 `allow-*` / `deny-*` permission이 생성된다.

**중요:** browser command만 ACL에 넣는 것으로 충분하지 않다. 기존 `cmd_terminal_*`, `cmd_worktree_*`, `cmd_project_*`, `cmd_remote_*`, `cmd_notification_*` 등도 함께 등록해 untrusted child에서 기본 허용되는 application command가 남지 않게 해야 한다.

생성된 app permission 중 main app UI에 필요한 allow permission만 `app-shell` capability에 넣는다.

### 7.4 remote capability 금지

- `remote.urls` allow list를 browser 사이트 용도로 만들지 않는다.
- 외부 사이트에는 Tauri API가 필요하지 않다.
- remote site가 특정 기능을 써야 한다는 이유로 capability를 넓히지 않는다.

### 7.5 navigation scheme policy

주소창 및 redirect/navigation에서 top-level target은 backend가 재검증한다.

기본 허용:

- `https:`
- `http:`
- 제한적으로 `about:blank` (runtime bootstrap 목적일 때만)

기본 거부:

- `tauri:`
- `asset:`
- `ipc:`
- `file:`
- `javascript:`
- `data:`
- 임의 custom scheme

`blob:` top-level navigation은 MVP에서 거부하고 실제 사이트 호환성 요구가 확인될 때 별도 정책으로 추가한다.

주소 입력에서 `localhost:3000`처럼 scheme이 없는 로컬 주소는 UI가 `http://localhost:3000`으로 normalize할 수 있지만, 최종 allow/deny는 항상 Rust가 다시 판단한다.

### 7.6 app origin 진입 차단

browser가 redirect를 통해 rorca의 bundled app origin/dev origin으로 이동할 수 없도록 한다.

- production app custom origin 차단
- dev server (`http://localhost:5173`)는 browser pane에서 차단
- `ipc.localhost` 접근용 app origin을 navigation target으로 허용하지 않음

이 방어는 capability isolation과 별개로 적용하는 defense-in-depth다.

### 7.7 CSP 역할

`src-tauri/tauri.conf.json`의 CSP는 **rorca app shell을 보호하는 정책**이다.

- remote child에 일반 인터넷을 허용하려고 `frame-src *`, `connect-src *` 등으로 main CSP를 넓히지 않는다.
- remote 페이지 자신의 CSP는 remote 서버가 결정한다.
- main app CSP만으로 child의 IPC 격리를 해결하려고 하지 않는다.

현재 app CSP의 `'unsafe-inline'`, `'unsafe-eval'`은 별도 hardening 대상으로 볼 수 있지만, browser 지원을 위해 더 느슨하게 만들 이유는 없다.

### 7.8 JavaScript injection 정책

- arbitrary remote child에 Tauri bridge용 initialization script를 추가하지 않는다.
- user-provided 문자열을 `eval()` script에 삽입하지 않는다.
- browser navigation/history는 native API를 우선한다.
- fixed app-owned compatibility script가 꼭 필요한 경우 origin과 목적을 좁히고 별도 security review를 통과한다.

### 7.9 popup / download / permission

MVP 정책:

- `window.open` / target `_blank`: unmanaged OS webview 생성 금지, main UI에 open request만 전달
- download: 자동 허용 금지
- camera/microphone/geolocation: 제품 기능으로 지원한다고 선언하지 않음
- devtools: release build에서 노출 금지
- clipboard permission: 플랫폼 기본보다 넓히는 설정을 임의 활성화하지 않음

---

## 8. session / cookie / cache 격리

### 8.1 격리 단위

per-tab cookie jar보다 **Browser Session Profile 단위 격리**를 채택한다.

```ts
type BrowserSessionProfile = {
  id: string;
  mode: "persistent" | "private";
};
```

- 같은 profile의 tab/pane은 login/cookie/cache를 공유할 수 있다.
- 다른 profile은 가능한 플랫폼에서는 storage context를 분리한다.
- private profile은 비영속 data store를 사용한다.

이 방식이 일반 브라우저 UX와 원본 Orca `sessionProfileId/sessionPartition` 구조에 가깝다.

### 8.2 플랫폼별 storage 전략

Tauri v2 `WebviewBuilder`의 관련 기능을 사용한다.

#### Windows / Linux

- profile별 `data_directory(app_data/browser-profiles/<profile-id>)`
- persistent profile은 directory 유지
- private profile은 `incognito(true)`를 우선 사용

#### macOS 14+

- WKWebView의 custom data store를 위해 profile id에서 안정적인 16-byte identifier를 만들고 `data_store_identifier(...)` 사용
- private profile은 non-persistent/incognito data store 사용

#### macOS 14 미만

custom persistent WK data-store identifier가 제한되므로 **여러 persistent profile의 완전 격리를 지원한다고 거짓으로 표시하면 안 된다.**

MVP 옵션:

- shared `Default` persistent profile
- `Private` non-persistent profile
- backend capability에 `supportsPersistentProfileIsolation`을 노출
- 지원 불가 플랫폼에서는 추가 persistent profile UI를 숨기거나 disabled 처리

### 8.3 browsing data는 frontend에 serialize하지 않는다

쿠키/cache/localStorage를 React state/localStorage로 옮기지 않는다.

frontend가 저장하는 것은 필요할 경우 다음 metadata뿐이다.

- browser tab URL
- label/title fallback
- `sessionProfileId`
- workspace/worktree ownership
- zoom preference

실제 cookie/cache는 native browser data store가 소유한다.

### 8.4 close lifecycle

browser tab/pane close:

1. UI에서 해당 browser id가 더 이상 어떤 leaf에서도 참조되지 않는지 계산
2. `cmd_browser_close(browserId)`
3. backend `Webview::close()`
4. registry에서 metadata 제거
5. stale event generation 무효화
6. 연동된 port forwarding lease가 있다면 reference release

regular persistent profile의 on-disk cookie/cache는 tab close로 지우지 않는다.

private profile은 마지막 runtime이 닫히면 임시 storage가 남지 않도록 정리한다.

### 8.5 inactive tab

탭 전환만으로 webview를 destroy하지 않는다.

- inactive -> `hide()`
- 다시 active -> bounds 갱신 후 `show()`
- history/login state 유지

장시간 background browser가 많아지는 문제는 Phase 5에서 LRU를 추가한다.

---

## 9. frontend 상태 모델

### 9.1 `TerminalTab`을 discriminated union으로 확장

`ui/src/lib/types.ts`의 tab 모델을 다음 방향으로 바꾼다.

```ts
type BaseWorkspaceTab = {
  id: string;
  label: string;
};

type TerminalTab = BaseWorkspaceTab & {
  kind: "terminal";
  sessionId: string;
};

type BrowserTab = BaseWorkspaceTab & {
  kind: "browser";
  browserId: string;
  sessionProfileId: string;
  workspaceId: string;
  worktreePath: string;
};

type WorkspaceTab = TerminalTab | BrowserTab;
```

`sessionId`를 `TerminalTab`에 유지하면 기존 terminal 코드를 점진적으로 migration할 수 있다.

### 9.2 dynamic browser state 분리

URL/title/loading/history처럼 자주 변하는 값은 `BrowserTab` 자체보다 별도 state에 둔다.

```ts
type BrowserRuntimeState = {
  id: string;
  generation: number;
  url: string;
  title: string | null;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomFactor: number;
  loadError: string | null;
};

type WorkspaceState = {
  ...;
  sessions: Record<string, TerminalSession>;
  browsers: Record<string, BrowserRuntimeState>;
  layout: LayoutState;
};
```

`BrowserTab.label`은 title/host의 presentation snapshot이고, authoritative navigation state는 `browsers[browserId]`다.

### 9.3 pane tree binding 일반화

현재 `TabPaneLayout`은:

```ts
sessionIdsByLeafId: Record<string, string>
```

로 terminal에 고정돼 있다.

browser split까지 자연스럽게 지원하려면 다음으로 일반화한다.

```ts
type PaneContentRef =
  | { kind: "terminal"; sessionId: string }
  | { kind: "browser"; browserId: string };

type TabPaneLayout = {
  root: PaneNode;
  activeLeafId: string | null;
  expandedLeafId: string | null;
  contentsByLeafId: Record<string, PaneContentRef>;
};
```

그리고:

- `selectLiveSessionIds` -> terminal content만 추출
- `selectLiveBrowserIds` 추가
- `closePane` -> 사라진 content kind에 맞게 PTY 또는 browser runtime cleanup
- `syncWorktrees` -> 해당 worktree에 귀속된 terminal/browser content 모두 정리

MVP 정책상 한 top-level tab의 pane들은 같은 `tab.kind`를 유지해도 된다. 즉 terminal tab split은 terminal pane, browser tab split은 새로운 browser pane을 만든다. 이후 mixed terminal+browser leaf가 필요해도 `PaneContentRef`가 이미 지원한다.

### 9.4 `LayoutState.tabs`

```ts
type LayoutState = {
  tabs: WorkspaceTab[];
  activeTabId: string | null;
  layoutsByTabId: Record<string, TabPaneLayout>;
};
```

layout reducer는 tab content를 직접 해석하지 않고 `id`, `label`, pane content ref만 다룬다.

### 9.5 `selectAgents` 수정

현재 agent selector가 모든 tab을 terminal로 간주하는 코드를 갖고 있으므로:

- `tab.kind === "terminal"` 또는 `PaneContentRef.kind === "terminal"`만 대상으로 한다.
- browser tab은 agent list에 나타나지 않는다.
- 동일 terminal session이 여러 pane에 참조될 경우 현재처럼 de-dup한다.

### 9.6 last-tab close 규칙

현재 workspace store는 마지막 terminal tab을 닫으면 새 terminal을 spawn해 workspace가 비지 않게 하는 규칙이 있다.

범용 tab 이후에는 다음처럼 명시적으로 처리한다.

- browser tab close -> browser runtime만 destroy
- terminal tab close -> 참조가 사라진 PTY만 close
- 마지막 UI tab을 닫았을 때 제품 규칙이 "항상 terminal 하나 유지"라면 **generic close가 끝난 뒤** active worktree의 새 terminal tab을 생성
- browser close 경로에서 `sessionId`를 읽거나 terminal cleanup을 실행하지 않음

이 분리를 하지 않으면 browser tab close가 PTY replacement/close 로직을 잘못 탈 수 있다.

---

## 10. UI 구조

### 10.1 `TerminalSplitView` -> `WorkspaceView`

pane tree가 안정화된 뒤 `TerminalSplitView`를 범용 renderer로 교체한다.

권장 구조:

```text
WorkspaceView
  TabBar
  PaneTreeView
    WorkspacePaneFrame
      TerminalPane
      or
      BrowserPane
```

점진 migration이 필요하면 처음에는 `TerminalSplitView` 이름을 유지한 채 generic content renderer를 넣고, 이후 `WorkspaceView`로 rename할 수 있다. 최종 public component명은 `WorkspaceView`를 권장한다.

### 10.2 `TabBar`

`TabBar.tsx` 변경:

- `tabs: WorkspaceTab[]`
- terminal -> terminal icon
- browser -> globe/browser icon
- browser loading indicator 선택적 표시
- browser title이 없으면 hostname 또는 `Browser` fallback
- close/activate는 kind와 무관한 generic callback
- `+`는 terminal 고정이 아니라 creation menu 또는 split button으로 확장
  - New Terminal
  - New Browser

Cmd/Ctrl+T의 기존 의미는 terminal 생성으로 유지해도 된다. browser 생성에는 별도 command palette action/shortcut을 추가한다.

### 10.3 BrowserToolbar

신규 `ui/src/components/browser/BrowserToolbar.tsx`.

필수 요소:

- Back button
- Forward button
- Reload button
- URL input
- External browser button
- loading/error affordance
- 선택적으로 zoom display/menu

상태:

- Back disabled: `!canGoBack`
- Forward disabled: `!canGoForward`
- Reload: `loading` 중 stop 기능을 구현하지 않으면 계속 reload로 둠
- URL input은 typing draft와 committed `BrowserRuntimeState.url`을 분리
- navigation event가 오면 address bar를 현재 URL로 sync
- 사용자가 input을 editing 중이면 remote navigation update가 타이핑을 덮지 않도록 focus/editing 상태를 고려

### 10.4 BrowserPane

신규 `BrowserPane.tsx`는 실제 remote DOM을 렌더링하지 않는다.

```text
BrowserPane
  BrowserToolbar         # main React webview
  BrowserSurfaceHost     # geometry placeholder
```

`BrowserSurfaceHost`가 child webview의 rectangle을 Rust에 알려준다.

### 10.5 child webview bounds 동기화

`BrowserSurfaceHost`는:

1. `ResizeObserver`
2. `getBoundingClientRect()`
3. main window resize
4. pane ratio 변경
5. sidebar open/close
6. tab activation

을 감지해 `cmd_browser_set_bounds`를 호출한다.

호출은 `requestAnimationFrame`으로 coalesce해 split drag 중 IPC 폭주를 막는다.

주의점:

- DOM toolbar 영역을 제외한 **page content rectangle만** child bounds로 사용
- CSS px와 Tauri logical pixel의 대응을 Phase 0에서 HiDPI/scale factor로 검증
- negative/NaN/0-size rect는 backend에서도 거부/보정
- hidden tab은 bounds만 0으로 만드는 대신 `hide()` 사용
- active가 되면 `set_bounds -> show` 순서로 flash를 줄임

### 10.6 z-order와 split divider

native child webview는 main DOM보다 별도 surface이므로 일반 CSS `z-index`로 browser 위에 React 요소를 띄운다고 가정하면 안 된다.

따라서:

- webview bounds가 toolbar/tabbar/divider를 침범하지 않게 정확히 자름
- split drag 시작 시 필요하면 visible child를 잠시 hide하고 drag 끝에 bounds 갱신 후 show
- popup/menu는 browser content 위에 겹쳐야 하는지 별도 검증
- DOM border radius clipping에 의존하지 않음

---

## 11. focus 및 keyboard shortcut 설계

### 11.1 현재 문제

`ui/src/lib/shortcuts.ts`는 main React webview의:

```ts
window.addEventListener("keydown", ...)
```

으로 shortcut을 처리한다.

Child Webview가 focus된 순간 remote page의 key event는 main React webview로 올라오지 않는다.

따라서 browser 지원 후에도 기존 방식만 쓰면 다음이 고장난다.

- close tab
- next/previous tab
- address bar focus
- reload
- back/forward
- zoom
- command palette/global chrome action 일부

### 11.2 source-of-truth

focus owner를 명시한다.

```ts
type WorkspaceFocusOwner =
  | { kind: "app" }
  | { kind: "terminal"; tabId: string; leafId: string; sessionId: string }
  | { kind: "browser"; tabId: string; leafId: string; browserId: string };
```

### 11.3 shortcut routing

native child가 focus되어도 동작해야 하는 **app chrome shortcut**은 Tauri application menu accelerator/event 쪽으로 이동시키는 것을 권장한다.

시스템 전체를 가로채는 global-shortcut plugin은 사용하지 않는다.

app accelerator 대상:

- Close active tab
- Next/previous tab
- Browser address (`Cmd/Ctrl+L`)
- Reload (`Cmd/Ctrl+R`)
- Back/Forward (플랫폼 표준 조합)
- Zoom in/out/reset
- command palette처럼 browser focus 중에도 반드시 호출돼야 하는 global action

중복 실행을 피하려면 동일 action을 React keydown과 Tauri menu에서 동시에 처리하지 말고 `ShortcutRouter` 하나로 수렴한다.

terminal 내부 문자/키 입력은 기존 xterm 경로를 유지한다.

### 11.4 address bar focus

`Cmd/Ctrl+L`:

1. main app webview에 focus 반환
2. `browser_focus_address` app event 또는 React command dispatch
3. 해당 active BrowserPane의 input `focus()` + `select()`

### 11.5 browser focus

browser page focus 요청:

- active browser pane 변경
- bounds/visibility 확인
- `cmd_browser_focus(browserId)` -> native child `set_focus()`

직접 child content를 클릭했을 때 active pane을 추적하는 것은 Tauri high-level `WebviewEvent`가 focus event를 제공하지 않는 현재 API에서 별도 platform work가 필요하다.

Phase 0 spike에서 다음 중 하나를 확정한다.

1. `with_webview` platform handle에 focus observer 설치
2. accelerator 실행 시 native focused child 조회
3. 플랫폼별 안전한 focus bridge

이 문제가 해결되지 않은 상태에서 여러 browser child를 동시에 split에 노출하면 shortcut target이 stale해질 수 있으므로 **multi-browser split의 release gate**로 둔다.

### 11.6 browser/terminal 전환

- terminal tab 활성화 -> browser child hide -> xterm focus
- browser tab 활성화 -> xterm blur -> child bounds update/show -> browser focus 또는 address focus
- toolbar input focus 중 browser navigation shortcut이 문자를 가로채지 않도록 action별 editable target policy 유지

---

## 12. local port / `openWorkspacePortInBrowser` 설계

### 12.1 rorca local 실행의 기본

현재 rorca PTY는 local machine의 worktree에서 실행된다. 따라서 local dev server port는 대부분 실제 forwarding 없이:

```text
http://localhost:<port>
```

로 접근할 수 있다.

### 12.2 browser API와 port resolver 분리

다음 두 계층을 분리한다.

```text
WorkspacePortResolver
  resolve(workspace, worktree, port) -> ResolvedPortTarget

openWorkspacePortInBrowser(target)
  -> openBrowserTab(target.browserUrl, ownership metadata)
```

`ResolvedPortTarget` 예:

```ts
type ResolvedPortTarget = {
  browserUrl: string;
  workspaceId: string;
  worktreePath: string;
  port: number;
  forwardingLeaseId?: string;
};
```

MVP local resolver는 `http://localhost:<port>`를 반환한다.

future remote execution이 생기면 resolver가 SSH/tunnel/proxy를 만들고 local forwarded URL을 반환하며, browser 계층은 그대로 유지한다.

### 12.3 UI 동작

원본 parity에 맞춰:

- 일반 click -> embedded BrowserTab
- 설정에서 embedded port preview off -> system browser
- Shift+click 같은 explicit modifier -> system browser override
- port owner worktree가 있으면 그 worktree를 먼저 활성화하고 browser tab을 생성

### 12.4 forwarding lease lifecycle

remote port forwarding이 후속 도입될 경우:

- BrowserTab/PaneContentRef가 optional `forwardingLeaseId`를 보유하거나 별도 ownership table 사용
- 마지막 consumer browser가 닫히면 lease release
- tab switch/hide만으로는 release하지 않음

browser manager가 SSH tunnel 자체를 소유하지 않게 한다.

---

## 13. 외부 기본 브라우저 열기

BrowserToolbar의 external button은 frontend에서 임의 shell command를 실행하지 않는다.

권장:

1. `cmd_browser_open_external(url)`
2. Rust에서 `http/https` URL만 재검증
3. Tauri opener plugin 또는 platform open API 호출

`tauri-plugin-opener`를 추가한다면 permission은 `app-shell`에만 부여하고 browser child label에는 부여하지 않는다.

`file:`, `javascript:`, app internal scheme은 external-open command에서도 거부한다.

---

## 14. 파일별 변경 계획

### 14.1 Rust/Tauri

#### `src-tauri/Cargo.toml`

- 검증한 Tauri minor version으로 pin
- `unstable` feature 추가
- external open 방식에 따라 `tauri-plugin-opener` 추가
- native history/focus adapter에 필요한 target-specific crate를 Tauri가 사용하는 버전과 맞춤

#### `src-tauri/build.rs`

- `AppManifest::commands(...)` 도입
- 현재 `invoke_handler` command 전체 ACL 등록
- 신규 browser command 등록

#### `src-tauri/capabilities/default.json`

- `windows: ["main"]` 제거
- `webviews: ["main"]`로 app shell 한정
- generated app-command permissions allow list 추가
- browser label pattern 미포함

가능하면 파일명도 `app-shell.json`으로 rename해 목적을 명확히 한다.

#### `src-tauri/tauri.conf.json`

- enabled capability를 명시적으로 지정
- browser 때문에 main CSP를 넓히지 않음

#### `src-tauri/src/browser/*`

- manager/model/security/navigation/profile 구현

#### `src-tauri/src/ipc/browser.rs`

- command DTO / handlers / browser event 정의

#### `src-tauri/src/ipc/mod.rs`

- browser module export

#### `src-tauri/src/lib.rs`

- `BrowserManager` manage
- browser command register
- opener plugin이 필요하면 plugin 등록
- app exit lifecycle에서 browser cleanup hook

### 14.2 Frontend

#### `ui/src/lib/types.ts`

- `WorkspaceTab = TerminalTab | BrowserTab`
- `kind` discriminator
- `BrowserRuntimeState`
- `PaneContentRef`
- `LayoutState.tabs: WorkspaceTab[]`
- `TabPaneLayout.contentsByLeafId`

#### `ui/src/state/layout.ts`

- terminal-specific `sessionIdsByLeafId` 제거/일반화
- `PaneContentRef` 기반 add/split/close/select helper
- terminal/browser live resource selector 분리

#### `ui/src/state/workspaceStore.ts`

- `browsers` state 추가
- browser event reducer
- `openBrowserTab`
- `closeBrowserTab` / generic `closeTab`
- `splitBrowserPane`
- resource cleanup을 kind별로 분리
- `selectAgents`가 browser 무시

#### `ui/src/lib/browserEvents.ts`

- Tauri browser event listener 단일 등록
- stale generation filtering 지원

#### `ui/src/lib/browser/tauri.ts`

- typed invoke wrapper
- preview/test mock boundary

#### `ui/src/components/TabBar.tsx`

- generic tab
- kind별 icon/label/loading
- New Terminal / New Browser creation path

#### `ui/src/components/WorkspaceView.tsx`

- pane tree recursive rendering
- `TerminalPane` / `BrowserPane` dispatch
- active leaf/focus
- divider geometry

#### `ui/src/components/browser/BrowserToolbar.tsx`

- browser chrome

#### `ui/src/components/browser/BrowserPane.tsx`

- toolbar + native surface host

#### `ui/src/components/browser/BrowserSurfaceHost.tsx`

- bounds/visibility/focus lifecycle

#### `ui/src/lib/shortcuts.ts`

- terminal-specific title을 generic tab title로 정리
- browser action id 추가
- app chrome accelerator migration과 중복 제거

#### `ui/src/App.tsx`

- `TerminalSplitView` -> `WorkspaceView`
- generic tab activation
- New Browser action
- port open integration hook

---

## 15. 단계별 구현 계획

## Phase 0 — prerequisite / security / API spike

목표: browser remote content를 넣어도 되는 보안 경계와 Tauri API viability를 먼저 증명한다.

### 작업

- [ ] 현재 pane-tree 리팩터를 green 상태로 안정화하고 `App.tsx`/view/store API 불일치 제거
- [ ] Tauri minor version pin + `unstable` feature
- [ ] disposable child webview 1개 생성 spike
- [ ] child `set_bounds`, `show`, `hide`, `set_focus`, `close` 검증
- [ ] macOS/Windows/Linux history adapter에서 back/forward/canGoBack/canGoForward 검증
- [ ] multi-browser focus observer 방법 확정
- [ ] `build.rs`에 app command ACL manifest 도입
- [ ] capability를 `webviews: ["main"]`으로 축소
- [ ] browser child에서 기존 terminal/worktree command invoke가 거부되는 security test 확보

### Exit criteria

- remote fixture page에서 어떤 existing/new app command도 호출할 수 없음
- main UI는 기존 command를 정상 호출
- child webview 생성/이동/숨김/닫기가 대상 OS에서 동작
- native history capability 값을 읽을 수 있음
- focus routing의 구현 방법이 최소 macOS에서 검증되고, 다른 desktop target의 구현/제한이 명확함

**이 Phase가 실패하면 이후 browser UI 구현을 진행하지 않는다.**

---

## Phase 1 — backend browser runtime + IPC

### 작업

- [ ] `browser/model.rs`
- [ ] `browser/manager.rs`
- [ ] URL policy
- [ ] session profile abstraction
- [ ] navigation adapter
- [ ] `ipc/browser.rs`
- [ ] state event
- [ ] create/navigate/reload/back/forward/get-state
- [ ] set-zoom/bounds/visible/focus
- [ ] close/list
- [ ] external-open command
- [ ] idempotent cleanup

### Exit criteria

Rust command-level test 및 작은 debug harness에서:

- child 생성
- URL 이동
- 뒤/앞
- reload
- zoom
- show/hide
- close
- unknown id error

가 일관되게 동작한다.

---

## Phase 2 — generic tab/state + Browser toolbar UI

### 작업

- [ ] `WorkspaceTab` union
- [ ] `BrowserTab`
- [ ] `PaneContentRef`
- [ ] `BrowserRuntimeState`
- [ ] layout reducer generic화
- [ ] workspaceStore browser lifecycle
- [ ] browser event bus
- [ ] `TabBar` generic화
- [ ] `BrowserToolbar`
- [ ] New Browser action
- [ ] browser close / agent selector regression 수정

이 단계에서는 native surface가 아직 placeholder여도 된다. 상태/toolbar를 mock backend로 독립 검증한다.

### Exit criteria

- terminal + browser tab이 같은 TabBar에 존재
- tab 전환/close가 서로의 resource를 오염시키지 않음
- browser state event가 toolbar disabled/loading/title/url에 반영
- browser tab은 agent list에 포함되지 않음

---

## Phase 3 — native webview mount / WorkspaceView / focus

### 작업

- [ ] `WorkspaceView` recursive pane renderer
- [ ] `BrowserPane`
- [ ] `BrowserSurfaceHost`
- [ ] `ResizeObserver` + rAF bounds batching
- [ ] inactive hide / active show
- [ ] split resize 연동
- [ ] HiDPI logical-pixel 검증
- [ ] Tauri menu accelerator 기반 app chrome shortcut
- [ ] address bar <-> browser focus 전환
- [ ] native browser focus observer
- [ ] multi-browser split release gate

### Exit criteria

- terminal/browser 탭 전환 시 native surface 잔상이 없음
- browser가 tabbar/toolbar/sidebar/divider를 덮지 않음
- 창 resize, sidebar toggle, split drag 후 bounds가 정확함
- browser content focus 상태에서도 Close Tab / Cmd+L / Reload / tab cycle이 정상 동작
- terminal로 돌아오면 xterm focus/shortcut 정상

---

## Phase 4 — local port integration

### 작업

- [ ] `WorkspacePortResolver` interface
- [ ] local resolver: localhost URL
- [ ] `openWorkspacePortInBrowser`
- [ ] worktree ownership/activation
- [ ] embedded vs system browser 설정
- [ ] Shift+click system browser override
- [ ] future forwarding lease metadata 자리 확보

### Exit criteria

worktree에서 `localhost:<port>` server를 띄우고:

- port action -> 해당 worktree BrowserTab
- refresh/history 정상
- system browser override 정상
- browser tab close가 terminal/session을 종료하지 않음

---

## Phase 5 — session isolation / hardening / stabilization

### 작업

- [ ] persistent Default profile
- [ ] Private profile
- [ ] 플랫폼별 profile isolation capability reporting
- [ ] macOS 14+ data store id 검증
- [ ] Windows/Linux data directory 검증
- [ ] clear browsing data
- [ ] popup/new-window policy
- [ ] download deny/UX
- [ ] background browser resource budget/LRU
- [ ] crash/runtime disappearance recovery
- [ ] security fixture suite
- [ ] full desktop platform matrix

### Exit criteria

- profile cookie isolation이 지원 플랫폼에서 검증됨
- private profile restart 후 browsing data가 남지 않음
- tab close 후 browser registry/native surface leak 없음
- malicious fixture가 IPC/internal scheme을 탈출하지 못함
- release build에서 DevTools/unsafe debug route가 노출되지 않음

---

## 16. 자동화 테스트 전략

### 16.1 Rust unit tests

#### URL/security policy

테이블 기반 테스트:

허용:

- `https://example.com`
- `http://example.com`
- `http://localhost:3000`
- `http://127.0.0.1:8080`

거부:

- `file:///...`
- `javascript:...`
- `data:text/html,...`
- `tauri://...`
- `asset://...`
- `ipc://...`
- app dev origin `http://localhost:5173` when target is browser child

#### BrowserRegistry

- create metadata
- duplicate id/label 방지
- state update
- generation increment
- close idempotency
- unknown id
- close-all

#### profile

- profile id -> deterministic storage location/id
- path traversal 불가
- private/persistent capability mapping
- unsupported platform capability 반환

#### bounds

- finite positive logical rect만 허용
- NaN/negative/extreme value 거부/clip

#### resource selection

- browser leaf 하나 제거 시 마지막 ref에서만 destroy
- terminal/browser resource가 서로 prune되지 않음

### 16.2 Rust security contract test

가능하면 `include_str!` 기반으로 build/capability contract를 검증한다.

- app-shell capability가 `windows: ["main"]`을 사용하지 않음
- `webviews: ["main"]`만 사용
- `browser-*` allow pattern 없음
- remote capability 없음
- app command manifest가 browser뿐 아니라 기존 command도 ACL 대상으로 등록

### 16.3 Frontend Vitest

#### `layout.test.ts`

- mixed `WorkspaceTab` normalize
- browser tab activate/close
- browser pane split/close
- terminal resource selector와 browser resource selector 분리
- pane swap/ratio가 content kind를 잃지 않음

#### `workspaceStore.test.tsx`

- `openBrowserTab`
- browser create failure 시 dangling tab 없음
- close browser -> only browser close command
- close terminal -> only PTY close
- stale browser event 무시
- worktree delete -> owned browser cleanup
- `selectAgents` browser 제외

#### `TabBar.test.tsx`

- terminal/browser icon/label
- active state
- loading state
- close/new browser action

#### `BrowserToolbar.test.tsx`

- canGoBack/Forward disabled
- reload invoke
- address submit
- external open
- editing 중 URL event 처리
- error state

#### `BrowserSurfaceHost.test.tsx`

mock bridge로:

- mount -> bounds
- resize coalescing
- inactive -> hide
- active -> show
- unmount -> 직접 destroy하지 않고 owner lifecycle과 일치하는지 검증

### 16.4 IPC contract tests

기존 Tauri command DTO test 스타일에 맞춰:

- serde camelCase
- unknown field rejection이 필요한 request
- structured error serialization
- browser event payload serialization

---

## 17. 수동/E2E 검증 체크리스트

### 17.1 기본 탐색

- [ ] HTTPS 사이트 open
- [ ] HTTP 사이트 open
- [ ] localhost open
- [ ] redirect 후 URL sync
- [ ] title sync
- [ ] back
- [ ] forward
- [ ] reload
- [ ] zoom +/-/reset
- [ ] external browser open
- [ ] 404 / DNS error / connection refused 표시

### 17.2 layout

- [ ] terminal -> browser tab 전환
- [ ] browser -> terminal 전환
- [ ] browser hidden tab에서 history/login 유지
- [ ] horizontal split
- [ ] vertical/nested split
- [ ] pane ratio drag
- [ ] sidebar toggle
- [ ] main window resize
- [ ] minimize/restore
- [ ] fullscreen if supported
- [ ] 100%, 125%, 150%, 200% scale factor
- [ ] 멀티모니터에서 서로 다른 scale factor

### 17.3 focus/shortcut

- [ ] browser page click 후 Cmd/Ctrl+L
- [ ] browser page click 후 reload
- [ ] browser page click 후 close tab
- [ ] browser page click 후 next/previous tab
- [ ] address input typing 중 app shortcut 오작동 없음
- [ ] browser -> terminal 후 xterm keyboard 정상
- [ ] split된 browser pane 간 focus target 정확

### 17.4 session/profile

fixture server에서 cookie를 설정해 확인한다.

- [ ] 같은 Default profile tab 간 cookie 공유
- [ ] 다른 persistent profile 간 cookie 격리(지원 플랫폼)
- [ ] Private와 Default 격리
- [ ] app restart 후 Default 유지
- [ ] app restart 후 Private 제거
- [ ] clear profile data 동작

### 17.5 resource cleanup

- [ ] browser 20회 open/close 후 `cmd_browser_list` 0 또는 예상 count
- [ ] 닫힌 child가 화면에 남지 않음
- [ ] tab close 후 page audio/network activity가 계속되지 않음
- [ ] worktree 삭제 시 owned browser 제거
- [ ] app exit 시 child 정리
- [ ] child renderer crash 후 app 전체 crash 없음

### 17.6 security fixture

악의적 fixture page에서 다음을 시도한다.

- [ ] `cmd_terminal_spawn` invoke
- [ ] `cmd_terminal_write` invoke
- [ ] worktree delete invoke
- [ ] notification/remote command invoke
- [ ] browser IPC 직접 invoke
- [ ] Tauri core window permission 호출
- [ ] `tauri://` navigation
- [ ] `file://` navigation
- [ ] app dev URL navigation
- [ ] `javascript:` address navigation
- [ ] popup 생성
- [ ] 자동 download

기대 결과: 모두 거부되거나 제품에서 정의한 safe flow로 전환된다.

---

## 18. local test fixture 권장

외부 인터넷 사이트만으로 browser regression을 검사하면 결과가 불안정하다.

`src-tauri/tests` 또는 별도 test utility에 local HTTP fixture를 두어 다음 endpoint를 제공한다.

```text
/                 기본 HTML/title
/page-a
/page-b
/redirect
/push-state        history.pushState 테스트
/title-change      document.title 변경
/cookie/set
/cookie/read
/popup             window.open
/blocked-scheme    internal scheme link
/slow              loading state
/error-ish          broken subresource
```

이를 통해 history/title/cookie/popup/security behavior를 deterministic하게 검증한다.

---

## 19. 플랫폼별 리스크

### macOS

- WKWebView persistent profile 격리는 OS 버전에 따라 custom data store 지원 차이가 있음
- child focus observer가 Tauri high-level에 없음
- native webview z-order/toolbar overlap 검증 필요
- zoom은 macOS 11+ 제약 확인

### Windows

- WebView2 child 생성은 synchronous command/event context deadlock 주의
- browser data directory/profile mapping 검증
- WebView2 Runtime 버전 차이에 따른 incognito 지원 확인

### Linux

- WebKitGTK 설치/runtime 버전 차이
- distro별 child webview positioning/focus
- clipboard/permission behavior 차이

### 공통

- Tauri Child Webview API가 `unstable`
- native child는 DOM z-index/clipping을 따르지 않음
- app keyboard event가 child focus 중 전달되지 않음
- history API는 Tauri high-level에 없어 platform adapter 필요

따라서 Phase 0 spike를 형식적인 단계가 아니라 **release feasibility gate**로 둔다.

---

## 20. 성능/리소스 정책

MVP:

- active/inactive tab 모두 runtime 유지
- inactive child는 `hide()`
- 닫힌 tab은 즉시 `close()`

안정화 단계:

- visible browser 수와 total browser runtime 수 telemetry
- background browser LRU 상한 예: 4~8개부터 실측
- eviction 시 최소 URL/profile/zoom metadata를 남기고 필요 시 recreate
- audio/download/automation처럼 background 유지가 필요한 기능이 생기면 eviction veto 추가

원본 Orca의 live browser guest eviction 개념은 참고하되, rorca의 실제 memory footprint 측정 후 숫자를 결정한다.

---

## 21. observability / diagnostics

browser는 native resource leak을 찾기 어렵기 때문에 최소 diagnostics를 둔다.

### debug 정보

- browserId
- webview label
- workspace/worktree owner
- profile id
- current URL origin
- loading
- visible
- bounds
- generation
- createdAt

### diagnostics command

`cmd_browser_list`는 개발/테스트에서 registry를 조회할 수 있게 한다. 민감한 cookie/header 정보는 반환하지 않는다.

release UI에서 이 command를 노출할 필요는 없다.

---

## 22. 제품 UX 결정사항

구현 전에 다음 default를 고정한다.

1. **New Terminal**: 기존 Cmd/Ctrl+T 유지
2. **New Browser**: command palette + UI menu, 별도 shortcut은 충돌 조사 후 결정
3. 주소창 기본 검색 엔진 기능: MVP에서는 URL/host normalization만 제공하고 검색은 후속으로 미뤄도 됨
4. port click: embedded browser 기본, Shift+click system browser
5. browser tab은 worktree에 귀속
6. worktree 삭제 시 해당 browser tab도 닫음
7. browser tab close 후 profile cookie는 유지
8. Private profile은 명시적으로 선택했을 때만 사용

---

## 23. 최종 acceptance criteria

기능 완료는 다음 조건을 모두 만족해야 한다.

### 기능

- [ ] TerminalTab + BrowserTab 범용 tab 모델
- [ ] embedded HTTP(S) browsing
- [ ] Back/Forward/Reload
- [ ] URL/title/loading state sync
- [ ] external browser open
- [ ] zoom
- [ ] local port open
- [ ] tab/split/resize 연동
- [ ] focus/shortcut 정상

### 보안

- [ ] app command ACL 등록
- [ ] capability `webviews: ["main"]` 격리
- [ ] browser child label에 app/plugin permission 없음
- [ ] remote capability 없음
- [ ] internal/file/javascript/data scheme 차단
- [ ] malicious remote/localhost page의 Tauri IPC 호출 실패

### lifecycle

- [ ] inactive hide / active show
- [ ] close 시 native resource 해제
- [ ] worktree/app close cleanup
- [ ] stale event 안전
- [ ] registry leak 없음

### session

- [ ] Default profile persistence
- [ ] Private profile isolation
- [ ] 플랫폼별 persistent profile isolation 가능 여부를 정확히 보고
- [ ] unsupported 플랫폼에서 기능을 과장하지 않음

### 품질

- [ ] Rust unit/contract tests
- [ ] frontend Vitest
- [ ] security fixture
- [ ] macOS smoke
- [ ] Windows smoke
- [ ] Linux smoke 또는 명시적 지원 제외 결정
- [ ] HiDPI/multi-monitor 검증

---

## 24. 권장 구현 순서 요약

```text
0. pane-tree 안정화
   ↓
1. Tauri command ACL + capability webview scoping
   ↓
2. Child Webview / history / focus platform spike
   ↓
3. BrowserManager + browser IPC/event
   ↓
4. WorkspaceTab / PaneContentRef generic state
   ↓
5. TabBar + BrowserToolbar
   ↓
6. WorkspaceView + BrowserSurfaceHost bounds
   ↓
7. focus/accelerator 완성
   ↓
8. localhost port integration
   ↓
9. profile/cookie/cache isolation
   ↓
10. security/resource/platform stabilization
```

이 순서의 핵심은 **보안 경계를 먼저 닫고**, browser backend와 generic layout을 분리해서 검증한 다음 native surface를 붙이는 것이다.

---

## 25. 최종 설계 선택 요약

| 영역 | 선택 |
|---|---|
| Rendering | Tauri v2 Child Webview / Multi-webview |
| WebviewWindow | 주 구현에서 사용하지 않음 |
| iframe | 사용하지 않음 |
| Tab model | `WorkspaceTab = TerminalTab | BrowserTab` |
| Pane model | `PaneContentRef = terminal | browser` |
| Browser state source | Rust/native webview |
| Back/Forward | platform native history adapter |
| Bounds | React placeholder rect -> Rust `set_bounds` |
| Inactive tab | hide, destroy하지 않음 |
| Close | child `close()` + registry 제거 |
| Session | profile 단위 cookie/cache context |
| Security | app command ACL + main webview-only capability |
| Remote site IPC | 전면 금지 |
| CSP | app shell용; browser 때문에 완화하지 않음 |
| Popup | unmanaged popup 금지, main UI open request |
| Port preview | local resolver -> BrowserTab, future forwarding 분리 |
| Shortcut | app menu accelerator + focus owner routing |
| MVP browser model | 1 BrowserTab = 1 primary runtime, split runtime 확장 가능 |

이 설계는 원본 Orca의 핵심 사용자 경험인 **worktree-owned browser tab, persistent navigation state, toolbar, port preview, split-aware surface**를 유지하면서도 Electron guest 권한 모델을 그대로 모방하지 않고 Tauri v2의 보안 경계와 native webview 제약에 맞춘다.
