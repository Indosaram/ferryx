# Ferryx 전수 패리티 감사 — Orca 대비 차이점 및 오구현

> 작성일: 2026-08-21
> 범위: main `c40af0d` + 미커밋 레이어 포함 현재 working tree 전체 (프론트 `ui/src/**`, 백엔드 `src-tauri/**`, 웹/리모트 표면)
> 기준선: `ui/original-dist/` (원본 Orca Electron 렌더러 = canonical parity 기준), `plugin-manifest-Bs-50M_g.js`(키바인딩), `useSettingsNavigationMetadata-CkcKmDGe.js`(설정 섹션), `I18nProvider-TFirEJhb.css`(디자인 토큰)
> 방법: 병렬 감사 에이전트 3개(프론트 패리티 / 백엔드 정합성 / 미커밋 레이어) + 로컬 게이트 4종 + 코디네이터 직접 교차검증. 에이전트 소견 중 오진 1건(MOVE_TAB_TO_SPLIT 아키텍처 불일치)은 소스 재검으로 기각함.
> 이전 감사: `ORCA_LITE_PARITY_AUDIT.md` (D1~D11, 전부 해소됨). 이번 감사는 그 이후 누적된 커밋(HMR/탭 팝오버, 사이드바 접기, 브랜딩)과 대량의 미커밋 레이어(포인터 탭 드래그·split-to-edge, HTML5 드롭 폴백, 세션 영속화, BrowserPane, 알림, 리모트 PWA UI)까지 포괄.

## 1. 종합 판정

**부분 준수 — 백엔드 불변식은 견고하나, 프론트에서 런타임 차단 버그 1건(세션 복원), 메모리/리스너 누수군, 원본 대비 단축키·설정 커버리지 격차, 사용자 노출 "rorca" 브랜딩 잔존이 확인됨. 현재 상태로는 릴리스 승인 불가.**

- 백엔드: 1-Writer-1-Worktree lease, PTY 수명주기/좀비 방지, 파괴적 삭제 가드 체인, 이벤트 방출, 경로 검증 — **전부 코드·테스트 양쪽에서 실증**.
- 게이트: cargo test **153 passed / 0 failed**, clippy `-D warnings` **0 경고**, bun test **249 passed (249)**, build **성공** (tsc 진단 없음).
- 프론트: 앱 재시작 시 복원된 터미널 탭이 죽은 PTY에 붙는 **블로커 1건**(F1) 포함 결함 다수.

## 2. 차단급 / 주요 결함

### F1 [BLOCKER] 앱 재시작 시 복원된 터미널 탭이 죽은 세션에 부착됨 (코디네이터 재검 확인)
- `App.tsx:78-125`: `restoreWorkspace(restoredState)`로 탭을 먼저 복원한 뒤, 각 탭에 대해 `alreadyOpen`(`App.tsx:106-109`)이 true → `ensureTabForWorktree`(= spawn)를 건너뜀.
- `sessionPersistence.ts:108-116`: 복원된 세션은 `backendSessionId: sess.sessionId`(이전 프로세스의 ID)를 그대로 지님. 백엔드 PTY는 Tauri 프로세스 내부 PtyManager에서 살므로(`src-tauri/src/ipc/terminal.rs`, 데몬의 PtyManager는 별개 `daemon/server.rs:81`) 재시작 시 전부 소멸.
- `listTerminalSessions()` 결과를 버림(`App.tsx:91` — `.catch(() => [])` 후 미사용) → 생존 여부 검증·재spawn 어디에도 없음. `TauriTerminalTransport.listSessions()`(`tauriTransport.ts:10-17`)도 구현만 되고 앱에 안 붙음(고아 코드).
- **결과**: 재시작 후 복원된 터미널은 출력 없음·키 입력 무시 상태. (dev HMR 복구 시나리오에서는 백엔드가 살아있어 겉으로 문제 없음 — 실기 재시작 테스트 필요한 이유)
- **수정**: 복원 후 `listTerminalSessions()`와 대조해 live하지 않은 backendSessionId는 null 처리 + 해당 탭 재spawn. transport 계약에 attach 시 liveness 검증 추가 권장.

### F2 [MAJOR] agentTitle 서브스트링 오탐 — 일반 셸을 AI 에이전트로 분류 (재검 확인)
- `agentTitle.ts:121-135`: `agent.pattern.test(lower)`가 문자열 **어디든** 매칭(`/\bcursor\b/i`, `/\bpi\b/i`, `/\bomo\b/i` 등). 시작 위치 고정 taskPattern이 실패해도 `isAgent: true` 반환.
- 예: 탭 제목 `git commit -m "fix cursor position"` → `cursor` 에이전트 활동으로 오표시.
- **수정**: 패턴 매칭을 선두 토큰 또는 엄격 prefix로 한정.

### F3 [MAJOR] terminalHostManager 중복 리스너 + 스테일 클로저 (재검 확인)
- `terminalHostManager.ts:160-168`: 위임형 onBell/onTitleChange(150-159행)에 더해 직접 클로저로 **2회 중복 등록** → 벨/타이틀 콜백 2중 발화, 최신 props 아닌 초기 클로저 고착.
- **수정**: 중복 등록 제거(위임형만 유지).

### F4 [MAJOR] xterm 인스턴스 누수 — `destroy()` 호출부 0건 (재검 확인)
- `terminalHostManager.ts:233-242`: destroy는 resizeObserver/disposables/WebGL/terminal.dispose()를 정리하지만 tab close(CLOSE_TAB)/pane close(CLOSE_PANE) 어디서도 호출되지 않음 (`workspaceStore.ts:295-345`, `:712+`). 인스턴스가 세션 내내 `this.instances`에 잔존.
- **수정**: 탭/페인 닫기 경로에서 destroy 연결.

### F5 [MINOR→MAJOR 군] 구독/리스너 잔존
- `terminalHostManager.ts:212-220`: updateSession에서 backendSessionId 변경 시 **이전 output 구독을 해제하지 않고** 새 구독만 push → 이벤트 버스 구독 누적.
- `TerminalSplitView.tsx:838-871`: PaneResizeDivider가 window pointermove/up/cancel을 pointerDown 시 등록, pointerUp에서만 해제 — 드래그 중 언마운트(단축키로 탭 닫기 등) 시 리스너·`document.body.style.cursor` 잔존.

### F6 [BLOCKER(web/remote)] 삭제된 `rorca-icon.svg` 참조 — SW 설치 실패 + 브랜딩 위반 (재검 확인)
- `ui/public/sw.js:7`: `cache.addAll([... '/src/assets/rorca-icon.svg'])` → 404로 install 자체가 실패. SW는 비-Tauri 환경(`main.tsx:15-17`) 즉 **웹/리모트 클라이언트 모드**에서 등록됨.
- `ui/public/manifest.webmanifest`: name `"rorca Remote"`, short_name `"rorca"`, icons가 동일 삭제 자산 참조.
- **수정**: `ferryx-icon.svg`로 교체 + manifest 이름 Ferryx Remote로 변경.

## 3. Orca 원본 대비 패리티 차이 (by-design gap 제외)

### 3.1 키보드 단축키 — 원본 60여 바인딩 중 ~20개만 구현 (parity-gap, major)
근거: `shortcuts.ts:3-150` vs `original-dist/assets/plugin-manifest-Bs-50M_g.js`.

| 구현됨 | 미구현 (심각도순) |
|---|---|
| ⌘T 신규 터미널, ⌘⇧B 신규 브라우저, ⌘W 탭 닫기, ⌃PageUp/Down 탭 이동, ⌘D/⌘⇧D split right/down, ⌘⌥D unsplit, ⌘B 사이드바, ⌘K 팔레트, selectByIndex 1-4 | **pane 포커스 이동 ⌘]/⌘[**, **⌘F terminal search(xterm search addon 미통합)**, ⌘, 설정 열기, ⌘N workspace 생성, ⌘⇧T 닫은 탭 reopen, ⌘⌥W 전체 닫기, ⌘R 탭 rename, ⌘⇧Enter pane 최대화, zoom ⌘=/⌘-/⌘0, worktree navigate/history 계열, ⌃Tab MRU, tab.newAgent 동적 바인딩 |
| 충돌: 원본 ⌘K = pane clear → Ferryx는 CommandPalette로 재할당 (minor) | |

### 3.2 설정 표면 — 원본 30+ 섹션 중 6개 (parity-gap)
근거: `SettingsDialog.tsx:41-100` vs `useSettingsNavigationMetadata-CkcKmDGe.js`.
- 구현: general(app 버전·업데이트·telemetry·데이터 리셋), terminal(폰트/스크롤백/Ghostty import/Option-as-Alt), shortcuts(뷰어), workspace, notifications(권한/사운드/probe), remote(gateway/pairing/QR).
- gap 중 결함 아닌 것: Monaco/PR/Automations/Agents/SSH 등(by-design). **결함성 gap**: Appearance(테마/accent), Quick Commands, Browser 설정(검색엔진/zoom).

### 3.3 탭 관리 (부분 패리티)
- 컨텍스트 메뉴(`TabBar.tsx:550-650`): Split/Pin/Rename/Close(+Others/Right/Left) 구현. 원본 대비 **Duplicate Tab, Restart Process, Clear Buffer, CWD 복사, 새 창으로 이동** 부재 (minor).
- 신규탭 팝오버(`NewTabPopover.tsx`): 원본의 Simulator/Markdown/per-agent 런처 대신 URL 감지 검색+Terminal/Browser 퀵액션 — 재설계된 표면(parity-gap).
- 탭 드래그: 포인터 기반 4-edge split + WebKit text/plain 폴백 — 신규 구현, 테스트로 행위 고정됨.

### 3.4 디자인 토큰 (minor)
- `index.css:18`: `--background: #23262d` vs 원본 `#0a0a0a`. 나머지 zinc 토큰(card #171717, muted #262626, worktree-sidebar #2a2a2a/#353535, border #ffffff12)은 일치. 의도적 Ferryx 톤일 수 있으나 원본 대비 확실한 차이 — 디자인 의사결정 필요.

### 3.5 i18n (gap-by-design/note)
- 원본 i18next + ja/ko/zh/es 로케일. Ferryx는 하드코딩 영문.

### 3.6 사이드바 (gap-by-design)
- 원본 activity bar(File Explorer/Search/Source Control/Checks/Ports) 부재 — worktree/project 전용으로 축소. 접기·리사이즈·아코디언은 구현.

## 4. 기타 오구현/버그

| ID | 심각도 | 내용 | 근거 |
|---|---|---|---|
| F7 | minor | 복원 세션 `worktree: null` → Sidebar/Header identity 매칭 실패 가능 | sessionPersistence.ts:113 |
| F8 | minor | 레거시 스키마 폴백이 leafId 아닌 **탭 id**를 sessionIdsByLeafId 키로 사용 → 페인 세션 조회 실패 | sessionPersistence.ts:138 |
| F9 | minor | 비에이전트 타이틀이 무조건 `state: "working"` → 상태 도트 왜곡 | agentTitle.ts:144-151 |
| F10 | minor | 저장소 키 3중 혼용: `orca.sidebar.width`, `orca.terminal.settings`, `rorca.projects`, `rorca:settings:notifications:v1`, `ferryx.sidebar.collapsedProjects` + App.test.tsx:101이 실제 키와 다른 키를 목(테스트 fidelity) | 각 파일 |
| F11 | minor | WorktreeList 브랜치 필터가 `ferryx`/`rorca` 누락(WorkspaceHeader와 불일치) | WorktreeList.tsx:33 vs WorkspaceHeader.tsx:27 |
| F12 | minor | 리모트 모듈 async 핸들러에서 동기 subprocess/socket 호출(tailscale status, axum 내 동기 git) → worker 스레드 기아 가능 | ipc/remote.rs:64,157; remote/server.rs:188,218 |
| F13 | note | 데몬 프로토콜 `serde_json::to_string(...).unwrap()` 다수(직렬화 불가 케이스 없음이나 명시적 에러 처리 권장) | daemon/client.rs:59; daemon/server.rs 5건 |
| F14 | note | `TauriTerminalTransport` 전체가 앱에 미연결 고아 코드(F1과 동일 증분으로 정리 가능) | tauriTransport.ts |

## 5. UI↔백엔드 계약 커버리지

- 백엔드 43개 IPC 명령(`lib.rs`) → 래퍼 커버리지 **43/43 (100%)**: `tauri.ts` 32개 + `browserTauri.ts` 11개.
- **고아 래퍼 7개** (래핑만 되고 UI 호출부 0건 — 재검 확인): `listRemoteDevices`, `revokeRemoteDevice`(페어링 기기 관리 UI 부재), `getTailscaleStatus`(진단 표시 부재), `setBrowserZoom`, `focusBrowser`, `getBrowserState`, `listBrowsers`.
- 이벤트: `worktree_changed`(Created/Deleted/DestructivelyDeleted/DirtyChanged/Pruned), `terminal_output`(base64), `terminal_lifecycle` — 방출 경로 실증. lock/branchChanged는 트리거 IPC 부재로 미구현이 정당(by-design).

## 6. 브랜딩 위반 (Ferryx 마스터 규격 대비)

사용자에게 노출되는 "rorca" 문자열 잔존:
1. `sw.js`/`manifest.webmanifest` — "rorca Remote" 전면 (F6, blocker급)
2. `PairingPage.tsx:47` — "Enter the 6-digit PIN from your **rorca Desktop** settings"
3. `RemoteSessionList.tsx:41,74` — "Connected to rorca native engine", "desktop rorca app"
4. `SettingsDialog.tsx:101,186` — footer "rorca · local desktop", "rorca uses the native charcoal desktop palette."
5. `terminalHostManager.ts:184` — 비-Tauri 프리뷰 배너 "rorca"
6. `ProjectDialogs.tsx:205` — placeholder="orca-lite"
7. 백엔드: 세션 저장 dir `~/*/rorca/session_state.json`, 에러 문자열 "rorca daemon" (note — 데이터 마이그레이션 수반)
- 내부 MIME `application/x-orca-tab`/`x-orca-pane`, localStorage 키 prefix는 비노출 hence note 수준이나 F10처럼 정합성 문제는 수정 대상.

## 7. by-design gap (결함 아님 — 스코프 경계)

에디터/Monaco · PR 뷰 · 에이전트 오케스트레이션(dashboard/kanban/map/notes) · 자동화 · 우측 사이드바 · SSH/Servers 관리 · i18n. 브라우저 팬·알림·리모트는 **이번 레이어에서 구현 완료**되어 gap에서 제외됨(단 §5의 고아 래퍼와 §6 브랜딩은 여전히 결함).

## 8. 수정 우선순위 권고

1. **P0**: F1(복원 세션 liveness 검증+재spawn), F6(sw.js/manifest 자산·브랜딩), F3/F4(terminalHostManager 중복 제거+destroy 연결)
2. **P1**: F2(agentTitle prefix 매칭), F5(구독 해제), 단축키 격차 중 ⌘]/⌘[·⌘F·⌘,, 고아 래퍼 7개 UI 연결(특히 remote device 관리)
3. **P2**: F7~F11, Appearance/Quick Commands/Browser 설정 섹션, WorktreeList 필터 정렬, 배경 토큰 결정(#0a0a0a vs #23262d)

## 9. 실기 미검증 항목 (정적 감사 한계 — 릴리스 전 필수)

- macOS 네이티브 앱 **완전 재시작** 후 복원 탭 동작(F1 실증 — 이번 감사의 최우선 실기 항목)
- 리모트 페어링(PIN/QR/기기 revoke) end-to-end, 브라우저 팬 webview 실제 탐색
- 장시간 탭 open/close 반복 시 F4 누수의 실메모리 증가율

---
*감사 산출: 게이트 로그 `/tmp/ferryx-sweep/*.log`(휘발성). 모든 file:line 근거는 working tree 기준이며 코디네이터가 소스 직접 재독으로 검증함.*
