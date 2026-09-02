# Mobile Remote Top Chrome Cleanup Receipt (2026-09-02)

## Request
1. 모바일 뷰 상단 여백/크롬을 더 깔끔하게 정리
2. "Ferryx Remote" row에 워크스페이스/워크트리 선택을 합치기
3. "Desktop terminal" row 제거

## Changes (scope: ui/src/remote/ only)

### RemoteApp.tsx
- Header 좌측을 단일 트리거 버튼으로 통합: `[F]` 로고 + "Ferryx Remote" + 현재 컨텍스트(`workspace / worktree`, mono, truncate) + ChevronDown.
  - `aria-label="Change workspace context"`, `aria-expanded` 유지. 컨텍스트 span은 기존 테스트 계약대로 `aria-label="Current desktop context"`.
- "Following Ferryx Desktop" 부제 삭제 (헤더 정리).
- `selectorOpen` 상태를 App으로 끌어올려 `RemoteWorkspaceMirror`에 controlled prop으로 전달.

### RemoteSessionList.tsx (RemoteWorkspaceMirror)
- 독립 컨텍스트 선택 row(`border-b bg-card px-2 py-1` 래퍼 + Monitor 아이콘 버튼) 완전 삭제 → 상단이 헤더 → 탭 스트립 → 터미널 3단으로 정리.
- 선택 popover(dialog)는 헤더 바로 아래에 뜨도록 `top-1.5`로 조정. 옵션 선택/닫기 시 `onSelectorOpenChange(false)`.
- `contextName` export.

### RemoteTerminal.tsx
- embedded 모드에서 `h-8` "Desktop terminal" 타이틀 bar 제거.
- 연결 상태는 터미널 서피스 상단 중앙에 떠 있는 "Connecting" pill(`role="status"`)로 대체 — 연결되면 사라져 크롬 최소화.
- standalone(비embedded) 모드 타이틀 bar는 기존 그대로(테스트 계약 보호).

## Verification
- `bunx vitest run src/remote`: **5 files / 78 tests passed** (RemoteUI 37, Terminal.contract 18, Attention 9, Gestures 10, gridProtocol 4).
- `bunx tsc --noEmit`: clean.
- 기존 테스트 수정/삭제 없이 통과 (path-leak, 브랜딩, 컨텍스트 선택 플로우 계약 모두 유지).
- 구현 위임: Gemini 3.7 Flash (hephaestus), 검증은 리드 세션에서 재실행.

## Automated mobile-browser QA (2026-09-02, 사용자 지적으로 수행)

"실기 확인만 수동"이라던 초기 주장을 정정하고 자동화 검증을 수행했다. 방법:

- 격리 QA 인스턴스: debug 바이너리 `ferryx --daemon` (runtime dir `/tmp/rorca-501-dev/`, `FERRYX_SESSION_DIR=/tmp/ferryx-qa-remote-chrome`) — 실행 중인 release 데몬과 무충돌.
- UDS v3 핸드셰이크 → `remoteConfigure` (gateway 43831) → `registerWorkspace` → `spawn` (실제 PTY) → `remoteSetActiveSelection` (Desktop 역할 대행) → `remoteCreatePairingCode` → HTTP `pair/exchange`로 토큰 획득. 게이트웨이는 새로 빌드한 `ui/dist`를 서빙.
- `agent-browser` 모바일 에뮬레이션 (390px viewport)으로 실제 브라우저에서 검증:

| 어설션 | 결과 |
|---|---|
| 헤더 통합 트리거 `header button[aria-label="Change workspace context"]` 존재 | ✓ |
| 컨텍스트 표시 `[aria-label="Current desktop context"]` | "qa-remote-chrome / main" |
| 헤더에 "Ferryx Remote" 텍스트 유지 | ✓ |
| "Desktop terminal" row 부재 (`document.body`에서 문자열 미발견) | ✓ |
| 터미널 그리드 미러링 (`remote-terminal-grid`) | ✓ |
| 트리거 클릭 → popover(`role=dialog` "Choose worktree") 열림, `aria-expanded=true` | ✓ |
| popover 옵션 라벨 | "qa-remote-chrome / main" |

- 증거 스크린샷: `receipt-assets/remote-chrome/shot-1-main.png` (헤더+탭+터미널 3단), `shot-2-selector.png` (popover). Connecting pill은 로컬 WS 접속이 1초 미만이라 캡처 놓침 — live 전환 후 사라지는 것은 확인(어설션 false), 렌더 조건은 vitest 임베디드 경로로 커버.
- 정리: QA PTY close + `listSessions` 빈 것 확인, 데몬 종료, 브라우저 close.

## Manual E2E (실기 라스트마일만)
1. 데스크톱 Ferryx 실행 후 모바일/브라우저에서 remote 접속 → 상단에 헤더 1줄 + (탭 있으면) 탭 스트립 + 터미널만 보이는지.
2. 헤더 좌측(로고+Ferryx Remote+컨텍스트명) 탭 → 워크스페이스/워크트리 선택 popover가 헤더 바로 아래에 뜨는지, 선택 후 닫히는지.
3. 터미널 상단에 "Desktop terminal" row가 없고, 접속 중일 때만 "Connecting" pill이 떴다가 Live 전환 후 사라지는지.
