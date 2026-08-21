# rorca Embedded Webview Browser 구현 및 검증 보고서

> **작성일:** 2026-08-21  
> **참조:** `BROWSER_SUPPORT_PLAN.md`  
> **상태:** 구현 완료 및 4종 검증 게이트 100% 통과 (PASS)

---

## 1. 구현 요약

`BROWSER_SUPPORT_PLAN.md`에 명시된 Tauri v2 Multi-webview 기반 내장 브라우저(Embedded Browser) 지원 아키텍처를 구현 완료하였습니다.

### (1) Phase 1: 보안 및 Capability 격리
- `src-tauri/capabilities/default.json`: main window 전체 대상이 아닌 `webviews: ["main"]`으로 capability 범위를 제한하여, 임의의 웹사이트를 로드하는 child webview가 앱 레벨 Tauri IPC 권한을 상속받지 못하도록 샌드박싱 적용.
- `src-tauri/src/browser/security.rs`: URL 스키마 검증기 구현 (`http:`, `https:`, `about:blank`만 허용, `tauri:`, `file:`, `asset:`, `javascript:` 차단).

### (2) Phase 2: Rust 백엔드 Webview 관리자 & IPC
- `src-tauri/src/browser/model.rs`: `BrowserState`, `BrowserProfileId`, `LogicalRect`, `CreateBrowserRequest`, `BrowserSessionSummary` 등 데이터 모델 정의.
- `src-tauri/src/browser/manager.rs`: `BrowserManager` 상태 레지스트리 구현 (생성, URL 변경, generation 관리, bounds 계산, zoom clamp, show/hide, destroy).
- `src-tauri/src/ipc/browser.rs`: Tauri IPC 명령어 구현 (`cmd_browser_create`, `cmd_browser_navigate`, `cmd_browser_reload`, `cmd_browser_set_bounds`, `cmd_browser_set_visible`, `cmd_browser_set_zoom`, `cmd_browser_focus`, `cmd_browser_get_state`, `cmd_browser_close`, `cmd_browser_list`, `cmd_browser_open_external`).
- `src-tauri/src/ipc/error.rs`: 브라우저 관련 정형 에러 코드 매핑 (`BROWSER_NOT_FOUND`, `BROWSER_URL_INVALID`, `BROWSER_URL_SCHEME_DENIED`, `BROWSER_BOUNDS_INVALID`, 등).
- `src-tauri/src/lib.rs`: `BrowserManager` state 관리 및 generate_handler 등록.

### (3) Phase 3 & 4: 프론트엔드 모델 & UI 컴포넌트
- `ui/src/lib/types.ts`: `WorkspaceTab = TerminalTab | BrowserTab` 유니온 모델 확장 및 `LayoutState` 제네릭 지원.
- `ui/src/lib/browserTauri.ts`: 브라우저 백엔드 IPC 호출 래퍼 함수군 구현.
- `ui/src/components/BrowserToolbar.tsx`: 주소창 입력, 뒤로가기/앞으로가기, 새로고침, 외부 시스템 기본 브라우저 열기 버튼 툴바 구현.
- `ui/src/components/BrowserPane.tsx`: `ResizeObserver` 및 윈도우 리사이즈 이벤트를 통한 `LogicalRect` bounds 실시간 측정 및 Tauri 백엔드 동기화.
- `ui/src/components/TabBar.tsx`: 터미널 탭(`TerminalSquare` 아이콘) 및 브라우저 탭(`Globe` 아이콘)을 통합 렌더링.
- `ui/src/components/TerminalSplitView.tsx`: 활성 탭 종류에 따라 `TerminalPane` / `BrowserPane`을 적절히 라우팅하여 렌더링.
- `ui/src/state/workspaceStore.ts`: `createBrowserTab`, `navigateBrowserTab`, `reloadBrowserTab`, `openWorkspacePortInBrowser` 액션 구현 및 에이전트 목록(`selectAgents`)에서 브라우저 탭 제외 처리.

---

## 2. 검증 게이트 결과 (4/4 PASS)

| 게이트 | 명령 | 결과 | 상세 |
|---|---|---|---|
| **Gate 1** | `cargo test --manifest-path src-tauri/Cargo.toml` | **PASS (Exit 0)** | 단위 테스트 + 통합 테스트 전수 통과 (URL 검증, BrowserManager 라이프사이클 테스트 포함) |
| **Gate 2** | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | **PASS (Exit 0)** | 0 warnings (Strict lint 통과) |
| **Gate 3** | `bun run --cwd ui test` | **PASS (Exit 0)** | 27 test files, 173 tests passed (BrowserToolbar 컴포넌트 테스트 포함) |
| **Gate 4** | `bun run --cwd ui build` | **PASS (Exit 0)** | TypeScript 컴파일 에러 0건, Vite 프로덕션 번들 정상 빌드 완료 |
