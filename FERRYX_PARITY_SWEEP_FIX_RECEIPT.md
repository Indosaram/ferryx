# Ferryx 패리티 스윕 수정 접수 증서 (FERRYX_PARITY_FULL_SWEEP.md 기반)

> 작성일: 2026-08-22
> 실행 방식: mass-ulw DAG (`key: ferryx-parity-sweep`, `run_id: dag_7f7c4bdf-427c-487e-9729-a6c00aeb366e`) — 구현 9노드 전부 `hephaestus` + `quotio/gemini-3.7-flash-high` 직결 (호스트 카테고리 라우팅 캐시 불량으로 우회). 코디네이터가 모든 노드 산출을 소스 재독 + 자체 게이트 재실행으로 검증.
> 증거 로그: `/tmp/ferryx-sweep-verify/gates.log`, `gates-final.log`

## 1. 수정 내역 (감사 결함 ID 기준)

| ID | 내용 | 구현 요약 | 핵심 파일 |
|---|---|---|---|
| F1 [P0] | 복원 탭이 죽은 PTY에 부착 | 복원 시 `listTerminalSessions()`로 live 집합 조회(`App.tsx:88-90`) → 비-live backendSessionId는 null 처리 후 재spawn, live면 유지 | App.tsx, sessionPersistence.ts, tauriTransport |
| F6 [P0] | SW 설치 실패 + rorca Remote | sw.js 프리캐시를 실존 `ferryx-icon.svg` 등으로 교체, manifest "Ferryx Remote"/"Ferryx", 아이콘 실존 검증 테스트 추가 | ui/public/sw.js, manifest.webmanifest |
| F3 [P0] | 벨/타이틀 콜백 2중 등록 | 직접 클로저 등록 제거, 위임형만 유지 (`terminalHostManager.ts:150-159`) | terminalHostManager.ts |
| F4 [P0] | xterm 인스턴스 누수 | 탭/페인 닫기 경로 4곳에서 `terminalHostManager.destroy()` 호출 (`workspaceStore.ts:276/287/309/360`) | workspaceStore.ts |
| F5 | 구독/리스너 누수 | updateSession의 구 output 구독 해제(`unsubscribeOutput`), PaneResizeDivider 언마운트 시 window 리스너+body cursor 정리(`TerminalSplitView.tsx:812-849`) | terminalHostManager.ts, TerminalSplitView.tsx |
| F2 [P1] | agentTitle 서브스트링 오탐 | 선두 토큰 기준으로 한정 — `git commit -m "fix cursor position"` → isAgent false (테스트 고정) | agentTitle.ts |
| F7 [P2] | 복원 세션 worktree null | worktreePath/cwd ↔ worktree 목록 대조로 identity 매칭 복원 | sessionPersistence.ts |
| F8 [P2] | 레거시 폴백이 탭 id 사용 | `collectLeafIds(paneTree)`로 leafId 키 매핑 | sessionPersistence.ts |
| F10 [P2] | 스토리지 키 3중 혼용 | `ui/src/lib/storageKeys.ts` 신설(ferryx.* 통일) + `getMigratedItem()` 레거시 읽기→신규 쓰기 마이그레이션, 호출부 5곳 + App.test.tsx 정렬 | storageKeys.ts(+test), App.tsx, Sidebar.tsx, notificationSettings.ts, terminalSettings.ts |
| F11 [P2] | WorktreeList 필터 누락 | 공유 `ui/src/lib/branchFilter.ts` 추출, WorktreeList/WorkspaceHeader 양측 import 정렬 | branchFilter.ts(+test) |
| 단축키 [P1] | ⌘]/⌘[ · ⌘F · ⌘, | terminal.focusNext/focusPrevious, terminal.search(@xterm/addon-search + TerminalSearchOverlay), settings.toggle 엔드투엔드 배선 | shortcuts.ts, App.tsx, terminalHostManager.ts, TerminalPane.tsx, package.json |
| 고아 래퍼 7종 [P1] | UI 호출부 0건 | Remote 설정 섹션: 기기 목록+Confirm Revoke+Tailscale 상태 / Browser 섹션: 줌(setBrowserZoom)+세션 목록(listBrowsers)+Focus(focusBrowser) / BrowserToolbar: Zoom·Focus·Sync state(getBrowserState) — **7/7 전부 배선** | SettingsDialog.tsx, BrowserToolbar.tsx (+각 test) |
| §6 브랜딩 | rorca 잔존 | PairingPage("Ferryx Desktop"), RemoteSessionList, SettingsDialog footer("Ferryx · local desktop"), terminalHostManager 배너, ProjectDialogs placeholder(neutral), **추가 발견분** RemoteApp 헤더 "🦀 Ferryx Remote" + 토큰 키 ferryx_remote_token(레거시 rorca 토큰 마이그레이션) | 각 파일 |

## 2. 실패 우선(Red→Green) 증거

모든 행위 변경은 해당 테스트를 먼저 작성해 RED를 캡처한 뒤 수정 후 GREEN을 캡처 (노드별 보고서 원문에는 RED 발산문 포함 — 예: F7 `expected null to deeply equal {wsId:'default', slug:'feature-branch'}`, F11 `Unable to find element "main"`, RemoteApp `Received: 🦀 rorca Remote`). 베이스라인 249 → 최종 **307 tests / 46 files 전부 통과**.

## 3. 최종 게이트 (코디네이터 직접 실행, gates-final.log)

| 게이트 | 결과 |
|---|---|
| `cd ui && bun run test` (vitest) | **46 files / 307 tests PASS**, exit 0 |
| `bunx tsc --noEmit` | 오류 0, exit 0 |
| `cargo test` | 모든 바이너리 ok 0 failed, exit 0 |
| `cargo clippy -- -D warnings` | 경고 0, exit 0 |

## 4. rorca 그렙 잔존 (전부 사용자 노출 아님 — 허용)

- `storageKeys.ts` LEGACY_STORAGE_KEY_MAP — 마이그레이션용 레거시 키 데이터
- `branchFilter.ts` — 분기명 비교 로직 문자열
- `RemoteApp.tsx:7` LEGACY_TOKEN_KEY — 기존 사용자 토큰 마이그레이션 폴백
- `notificationSettings.ts:15` 커스텀 이벤트명 `"rorca:notifications:settings-changed"` — 비노출 내부 이벤트(노트 수준, 추후 정리 가능)
- 테스트 파일 내부 어설션(부재 검증용) 제외

## 5. 설계 결정

1. **배경 토큰**: `--background: #23262d` 유지 (Orca 원본 #0a0a0a 대비 의도적 Ferryx 톤) — index.css에 주석 명기.
2. **비에이전트 타이틀 상태(F9)**: 활동 신호 없으면 "working" 대신 중립 idle 계열 상태 부여 (테스트로 고정).
3. **스토리지 키**: ferryx.* 단일 네임스페이스 + 읽기 시점 레거시 마이그레이션 (파괴적 데이터 이동 없음).
4. **TauriTerminalTransport(F14)**: listSessions()를 F1 복원 경로에 연결하여 고아 해소.

## 6. 남은 수기 QA (정적 검증 한계 — 감사 §9, 릴리스 전 필수)

- macOS 네이티브 앱 **완전 재시작** 후 복원 탭 출력/입력 동작 (F1 실물 검증)
- 리모트 페어링(PIN/QR/기기 Revoke) end-to-end
- 장시간 탭 open/close 반복 시 메모리 증가율 (F4 실측)
- ⌘]/⌘[ 포커스 순환, ⌘F 검색, ⌘, 설정 — 실기 키보드 확인

## 7. 커밋 관련

이번 변경분은 working tree의 기존 미커밋 레이어와 섞여 있어 코디네이터가 임의 커밋하지 않았음 (사용자 요청 시 분리 커밋 필요 — `git add -p` 권장).
