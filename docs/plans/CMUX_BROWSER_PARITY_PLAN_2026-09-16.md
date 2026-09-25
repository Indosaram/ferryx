# CMUX Browser Parity Plan

**Date:** 2026-09-16
**Updated:** 2026-09-25
**Goal:** 외부 CLI/에이전트가 Ferryx 내장 브라우저 탭을 직접 열고(evaluate), 조작하고, 상태를 읽을 수 있게 한다. 기준선은 cmux의 `cmux browser` 커맨드 그룹.
**Status:** Implemented (2026-09-25). Phase 1~3의 핵심 역량이 제품 코드에 구현 완료되었습니다.
- CLI 서브커맨드 대폭 확장: `src-tauri/src/cli.rs`의 `BrowserCliCommand`에 `Open { url, workspace_id, worktree_path }`, `Navigate`, `Close`, `Identify`, `Url`, `Title`, `Snapshot`, `Click`, `Fill`, `Keypress`, `Eval`, `Wait`, `Screenshot { out_path }`, `Console { errors_only, clear }`, `Errors`, `Focus`, `Cookies`, `Storage` 구현 완료.
- 소켓 IPC 요청 확장: `src-tauri/src/ipc/browser_cli.rs`의 `BrowserCliRequest`에 `List`, `Snapshot`, `Act`, `Open`, `Navigate`, `Close`, `Identify`, `Eval`, `Wait`, `Console` 등이 구현되고 응답 DTO에 `ConsoleEntries` 등 포함.
- 스크린샷 캡처: `src-tauri/src/browser/screenshot.rs` 및 `cmd_browser_snapshot_capability` (`src-tauri/src/ipc/browser.rs`).
- 콘솔 캡처: `src-tauri/src/browser/guest.rs` 내 게스트 콘솔 링 버퍼(최대 500개 엔트리) 수집 및 CLI `Console`/`Errors` 연동 (단위 테스트 `test_console_drain_script_and_parser` 포함).
- 원격 브라우저 제어: `src-tauri/src/browser/remote_driver.rs`, `remote_input.rs`, `remote_bridge_protocol.rs`로 원격 브리지 및 입력 전달 파이프라인 구현 완료.

---

## 1. 현재 상태 (검증 완료)

### 1.1 근본 원인 분석 및 해결 현황 (2026-09-25 해결 완료)

*2026-09-16 작성 당시:*
`/tmp/rorca-501/browser.sock`에 `{"command":"open","url":"..."}` 전송 시
`BROWSER_CLI_REQUEST_INVALID: unknown variant 'open', expected one of 'list','snapshot','act'`.

- 당시 `src-tauri/src/ipc/browser_cli.rs`의 `BrowserCliRequest`는 internally-tagged
  (`#[serde(tag="command")]`) enum으로 variant가 정확히 `List | Snapshot { browser_id } | Act { request }` 3개였음.
- 디스패치 `execute_request`도 동일하게 3개만 매칭.
- CLI 표면 (`src-tauri/src/cli.rs`): `ferryx browser <list|snapshot|click|fill|keypress>`만 존재.
- 탭 생성은 `ui/src/state/workspaceStore.ts`의 `createBrowserTab` → Tauri invoke
  `cmd_browser_create` (`src-tauri/src/ipc/browser.rs:857`) → 웹뷰 생성인 **프론트엔드 전용 경로**였음.

*2026-09-25 현황:*
- **해결 완료**: `BrowserCliRequest`(`src-tauri/src/ipc/browser_cli.rs`)에 `Open`, `Navigate`, `Close`, `Identify`, `Eval`, `Wait`, `Console` 등이 모두 추가되어 CLI/소켓 경로로 탭 생성/제어/종료가 가능해졌습니다.
- CLI 인터페이스(`src-tauri/src/cli.rs`) 역시 `open`, `navigate`, `close`, `identify`, `url`, `title`, `eval`, `wait`, `screenshot`, `console`, `errors`, `focus`, `cookies`, `storage`를 포함하여 전면 확장되었습니다.

### 1.2 Ferryx 브라우저 백엔드 보유 역량 (감사 결과)

| 기능 | 상태 | 근거 |
|---|---|---|
| 세션 생성/웹뷰 스폰 | 있음 (Tauri command) | `cmd_browser_create` browser.rs:857 |
| 네비게이션 | 있음 (Tauri command) | `cmd_browser_navigate` browser.rs:1160 |
| back/forward/reload | 있음 | browser.rs:1272,1281,1341 |
| 세션 종료 | 있음 | `cmd_browser_close` browser.rs:1656 |
| 상태 조회 | 있음 | `cmd_browser_get_state` browser.rs:1495 |
| 스냅샷(DOM/요소 ref) | 있음 (CLI 노출) | `BrowserAutomationSnapshot` model.rs:217 — url, title, elements[{reference,role,name,tagName}] |
| 클릭/입력/키 | 있음 (CLI 노출) | `BrowserAutomationAction` model.rs:227 — Click/Fill/Keypress (generation 기반 낙관적 잠금 포함) |
| 세션 목록 | 있음 (CLI 노출) | `BrowserSessionSummary` |
| 쿠키 임포트 | 있음 (Tauri command) | browser/cookies.rs |
| 줌/바운드/가시성/포커스 | 있음 | browser.rs:1370,1415,1461,1479 |
| 스크린샷 | 있음 (2026-09-25) | `src-tauri/src/browser/screenshot.rs`, `cmd_browser_snapshot_capability` (`src-tauri/src/ipc/browser.rs`), CLI `Screenshot { browser_id, out_path }` (`src-tauri/src/cli.rs`) |
| 콘솔/네트워크 로그 | 부분 있음 (2026-09-25) | 콘솔 로그: `src-tauri/src/browser/guest.rs` (500개 링 버퍼 수집 및 `test_console_drain_script_and_parser`), CLI `Console`/`Errors` (`src-tauri/src/cli.rs`, `src-tauri/src/ipc/browser_cli.rs`). 네트워크 인터셉트는 WKWebView 한계로 (여전히 없음) |
| JS eval | 있음 (2026-09-25) | CLI `Eval { browser_id, script }` (`src-tauri/src/cli.rs`, `src-tauri/src/ipc/browser_cli.rs`) |
| wait (조건 대기) | 있음 (2026-09-25) | CLI `Wait { browser_id, condition, timeout_ms }` (`src-tauri/src/cli.rs`, `src-tauri/src/ipc/browser_cli.rs`, `src-tauri/src/browser/model.rs` `BrowserWaitCondition`) |
| CLI 소켓 open/navigate/close | 있음 (2026-09-25) | `BrowserCliRequest::Open`/`Navigate`/`Close` (`src-tauri/src/ipc/browser_cli.rs`), CLI `open`/`navigate`/`close` (`src-tauri/src/cli.rs`) |

### 1.3 소켓 서버 아키텍처 (변경 지점)

- unix: UDS `browser.sock` / windows: loopback TCP `browser.port` (`ipc/browser_cli.rs:126-135`)
- 캐퍼빌리티 토큰: `browser.token` (0600, 0700 런타임 dir) — 소유만으로 부족하다는 설계 의도 명시
- `start_browser_cli_server_at_path` (browser_cli.rs:242)가 `AppHandle<R>` + `Arc<BrowserManager>`를
  이미 보유 → `cmd_browser_create`이 요구하는 것과 동일한 재료. **open 추가는 기술적 작은 변경.**
- 응답 프로토콜: 요청 한 줄(JSON+`\n`) → 응답 한 줄(JSON+`\n`), `BrowserCliResponse` enum.

### 1.4 탭 소유권 문제 (설계상 핵심)

`createBrowserTab`이 하는 일: workspace ID + worktree path 부착 → hidden으로 웹뷰 생성 →
프론트엔드 이진 pane 트리에 삽입 → geometry 지정 후 visible 전환.

소켓 서버는 pane 트리 상태를 모른다. CLI가 탭을 만들면 "어느 워크스페이스/pane에 속할지" 결정 주체가 없다.
→ 이것이 open 구현의 실질 설계 결정 지점이다. (§3.2에서 결정)

---

## 2. CMUX 기준선 (parity 타깃)

cmux `browser` 커맨드 그룹 전체 (cmux.com/docs/browser-automation):

- **Navigation/targeting**: identify, **open**, **open-split**, **navigate**, back, forward, reload,
  url, focus-webview, is-webview-focused, zoom, focus-mode, react-grab, devtools
- **Waiting**: wait (--load-state / --selector / --text / --url-contains / --function)
- **DOM interaction**: click, dblclick, hover, focus, check, uncheck, scroll-into-view, type, fill,
  press, keydown, keyup, select, scroll
- **Inspection**: snapshot (--interactive/--compact/--selector/--max-depth), screenshot,
  get (title/url/text/html/value/attr/count/box/styles), is (visible/enabled/checked), find, highlight
- **JS/injection**: eval, addinitscript, addscript, addstyle
- **Frames/dialogs/downloads**: frame, dialog, download
- **State/session**: cookies get/set/clear, storage local/session, state save/load, history
- **Tabs/logs**: tab list/new/switch/close, console, errors

cmux 운영 모델:
- `browser open`은 호출 터미널의 워크스페이스(`CMUX_WORKSPACE_ID` 환경변수)에 타깃팅, `--workspace`로 오버라이드
- surface 핸들(`surface:N`)로 탭 식별, `tab` 서브커맨드는 활성 탭 그룹에 매핑
- mutating action에 `--snapshot-after` → 응답에 즉시 새 스냅샷 포함
- WKWebView에서도 못 하는 것: viewport.set, geolocation, offline, network.route, screencast, raw input injection
  → **WKWebView 기반인 Ferryx도 동일 한계 공유, parity에서 제외해야 현실적**

### cmux 대 Ferryx 격차 요약

*(2026-09-25 업데이트: 아래 치명적 격차 및 주요 기능 격차(open, navigate, close, eval, wait, screenshot, console/errors, cookies/storage, title/url, identify)가 모두 제품 코드에 구현 완료되어 해소됨)*

- **치명적 격차 (에이전트 루프 자체가 시작 불가)**: open (새 탭 생성), navigate (기존 탭 이동), tab list/close
- **기능 격차**: eval, wait, screenshot, console/errors, cookies/storage CLI 노출, get title/url 단독, identify
- **이미 충족**: snapshot, click, fill, keypress (ferryx browser CLI로 존재)

---

## 3. 구현 설계 (범위 확정)

### 3.1 Phase 1 — CLI open/navigate/close (치명적 격차 해소)

**목표:** 에이전트가 Ferryx 안에서 "브라우저 탭을 열고, 그 탭을 조작하고, 끝내는" 최소 루프를 완성.

- `BrowserCliRequest`에 variant 추가:
  - `Open { url, workspace_id: Option<String> }` → 응답 `Opened { browser: BrowserSessionSummary }`
  - `Navigate { browser_id, url }` → `Navigated`
  - `Close { browser_id }` → `Closed`
- `cmd_browser_create`의 세션 등록+웹뷰 생성 로직을 `create_browser_session(app, manager, request)` 헬퍼로 추출해 CLI와 공유 (browser_cli.rs 서버가 이미 AppHandle+manager 보유).
- `cmd_browser_navigate`/`cmd_browser_close`도 동일 헬퍼화.
- **탭 소유권 결정 (CLI open의 workspace 귀속):**
  - 기본: 활성 데스크톱 워크스페이스에 귀속 (프론트엔드에 브라우저 상태 이벤트 브로드캐스트 → 활성 워크스페이스 pane 트리에 삽입, 기존 `createBrowserTab` 후반부 로직 재사용)
  - `workspace_id` 파라미터로 특정 워크스페이스 오버라이드 (cmux의 `--workspace` 상등)
  - 워크스페이스 식별 불가 시: hidden 세션으로만 생성하고 `list`로 조회 가능 (pane 미삽입, 2급 fallback)
- URL 스크립 검증: `http://`/`https://`만 허용, `file://`·`javascript:`·기타 스킴 차단 (신뢰된 앱 웹뷰 내 피싱 표면 방지)
- CLI 서브커맨드 추가: `ferryx browser open --url <url> [--workspace <id>]`,
  `ferryx browser navigate --browser-id <id> --url <url>`, `ferryx browser close --browser-id <id>`
- 에러 코드: `BROWSER_CLI_REQUEST_INVALID` 재사용, 새 코드는 `BROWSER_URL_REJECTED` 정도.

**완료 기준:**
- 소켓으로 `{"command":"open","url":"https://example.com"}` 전송 → Ferryx에 새 브라우저 탭 표시, `browser_id` 반환
- `ferryx browser snapshot --browser-id <id>` → 해당 탭 DOM 요소 반환
- `ferryx browser click/fill/keypress` → 동작
- `navigate`/`close` → 동작
- 기존 browser_cli 직렬화/소켓 테스트에 Open/Navigate/Close 케이스 추가, RED→GREEN

### 3.2 Phase 2 — 에이전트 워크플로우 개선 (권장)

- `get` 축소판: `ferryx browser url/title --browser-id <id>` (snapshot 전체 없이 현재 상태만)
- `Act` 응답에 post-action snapshot 첨부 옵션 (cmux `--snapshot-after` 상등)
- `Identify` (포그라운드 브라우저 세션 반환)
- PTY 자식에 `FERRYX_WORKSPACE_ID` 환경변수 노출 → `browser open`이 기본 워크스페이스 자동 결정
  (cmux `CMUX_WORKSPACE_ID`와 동일한 패턴)
- URL 생략 시 기본 페이지(about:blank 또는 설정된 홈페이지)로 열기

### 3.3 Phase 3 — WebKit 한계 내 심화 (선택)

- `eval`: WKWebView에서 `evaluateJavaScript`로 구현 가능. 단 게스트 브리지 nonce 인증 체계를 우회하지 않도록
  주의 — eval은 임의 JS 실행이므로 캐퍼빌리티 토큰으로 이미 인증된 CLI 트래픽에 한정.
- `wait`: 조건 폴링(0.25s interval, timeout)으로 snapshot 재시도 — eval 없이도 text/url-contains 구현 가능
- `screenshot`: WKWebView `takeSnapshot` → PNG 저장
- `console/errors`: guest bridge에 `console.*`/`window.onerror` 수집 추가
- cookies/storage CLI 노출
- tab list/new/switch/close CLI (Ferryx는 "브라우저 탭 = 개별 세션"이라 cmux의 탭 그룹 개념과 매핑 필요)

### 3.4 Parity 제외 (WKWebView 공통 한계)

cmux가 `not_supported`로 명시한 것과 동일: viewport.set, geolocation, offline, network.route,
screencast, raw mouse/keyboard/touch injection. Ferryx도 WebKit이므로 동일하게 제외.

---

## 4. 보안

- 신규 open/navigate/eval/close 모두 기존 캐퍼빌리티 토큰 인증 체계 재사용 (토큰 파일 교체·권한 유지)
- URL 스크립 검증 필수 (§3.1)
- eval은 별도 위험 등급: 토큰 보유 = 이미 act 권한이므로 등급 상승 아님. 단 에러 응답에 JS 실행 결과 원문을
  반환하지 않도록 크기 제한.

---

## 5. 파일 변경 예상 목록 (Phase 1)

- `src-tauri/src/ipc/browser_cli.rs` — variant 3개 추가, execute_request 확장
- `src-tauri/src/ipc/browser.rs` — create/navigate/close 로직 헬퍼 추출, CLI에서 호출 가능한 형태로
- `src-tauri/src/cli.rs` — 서브커맨드 3개 추가
- `src-tauri/src/browser/model.rs` — 필요 시 요청 DTO 추가 (OpenRequest 등)
- `src-tauri/tests/` — browser_cli 관련 신규 테스트
- 프론트엔드: pane 트리 삽입을 위한 브라우저 상태 이벤트 수신 (workspaceStore의 createBrowserTab 후반부 재사용)

---

## 6. 미결정 사항 (착수 전 사용자 결정 필요)

*(2026-09-25 업데이트: Phase 1~3의 핵심 기능들이 제품 코드에 구현 완료되었으며, 결정 사항들이 실장으로 해결되었습니다.)*

1. Phase 1 착수 여부 (본 계획서는 범위 확정서이며 착수 지시 별도)
2. open의 기본 workspace 귀속 정책: "활성 데스크톱 워크스페이스"로 할지, 항상 hidden + list 조회로 할지
3. Phase 2/3 범위 포함 여부 (권장: Phase 2는 Phase 1과 함께, Phase 3은 별도 트랙)
