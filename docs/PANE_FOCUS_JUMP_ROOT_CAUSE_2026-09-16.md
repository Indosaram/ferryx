# 페인 포커스 점프 (타이핑 중 다른 페인으로 넘어감) 원인 분석 — 2026-09-16

## 결론

페인 포커스에는 **두 개의 독립된 "진실원"**이 있고, macOS 네이티브 터미널 환경에서 이 둘은
구조적으로 어긋난다. 어긋난 순간 키 라우팅은 **스토어 쪽(늦은 값)**을 따르며, 스토어 쪽 페인이
싱크 포커스를 강탈한다. 이것이 "타이핑 중 포커스가 다른 페인으로 넘어가는" 현상의 핵심이다.

- 진실원 A (스토어): `layout.layoutsByTabId[tabId].activeLeafId` → `isActive` prop
  (`ui/src/components/TerminalSplitView.tsx:917` — `groupFocused && tabLayout.activeLeafId === node.leafId`)
- 진실원 B (DOM/네이티브): 실제로 포커스를 쥔 `native-terminal-focus-sink` textarea +
  모듈 변수 `lastFocusedNativeTerminalSessionId` (`ui/src/components/NativeTerminalPane.tsx`)

## 어긋나는 이유 (코드 확정)

1. **네이티브 표면 클릭은 스토어를 갱신하지 못한다.**
   WGPU 자식 뷰가 포인터 이벤트를 흡수하므로 DOM `onPointerDown/onClick` → `focusPaneInput`
   → `FOCUS_PANE` 디스패치가 빠지는 경우가 있다(2026-08-29에 측정된 알려진 동작).
   그 대체재인 AppKit `LeftMouseUp` 모니터
   (`src-tauri/src/lib.rs:677~745`, `session_at_logical_point` hit-test)는
   `native_terminal_focus` 이벤트를 발행하고, 이를 받는 곳은 둘:
   - `NativeTerminalPane.tsx:1668` — 해당 페인 싱크 포커스(즉시+rAF+40ms). 스토어 불변.
   - `ui/src/state/workspaceStore.ts:542` — **`MARK_SESSION_ACTIVITY_SEEN`만 하고
     activeLeafId를 갱신하지 않는다.** (스토어↔네이티브 동기화 부재가 뿌리)

2. **WebKit이 싱크 포커스를 BODY로 떨어뜨리는 순간마다 스토어 쪽이 강탈한다.**
   키다운 문서 캡처 폴백(`NativeTerminalPane.tsx:1376~1560`)의 소유 판정:

   ```
   targetedPane ? targetedPane === containerRef
     : active !== undefined ? active            ← 항상 boolean → 항상 여기서 결정
       : hoveredPane ? ...
         : fallbackSessionId === targetSessionId   ← dead code
   ```

   `active` prop은 항상 boolean이므로 `lastFocusedNativeTerminalSessionId` 폴백과
   hovered 폴백은 이 분기에서 도달 불가능하다. target이 body(`activeElement === BODY`,
   역사적으로 가장 흔한 상태)일 때 **스토어의 activeLeafId 페인이 키를 가로채고
   `inputRef.current?.focus()`로 싱크를 뺏는다** → 커서/입력이 다른 페인으로 이동.

3. **역방향도 같다: 스토어 active가 바뀌면 타이핑 중에 강제로 싱크가 옮겨진다.**
   active-effect(`NativeTerminalPane.tsx:773~816`)는 `active`가 false→true로 바뀌는 즉시
   `focusInput()` + rAF + 40ms 타이머 3중 강제 포커스 + `sendFocus(true)`를 한다.
   오버레이 존중 가드는 "이미 active였던 같은 세션"에만 적용되므로, 페인이 바뀔 때는 무조건 강탈.
   스토어 activeLeafId를 타이핑 중에 바꾸는 경로:
   - 리모트/페어링 클라이언트의 selection 동기화: `onRemoteSelectionRequested`
     (`ui/src/App.tsx:1743~1836`) → `activateRemoteEntry` → `FOCUS_PANE`
     (데스크톱 사용 중 원격 클라이언트가 접속/재동기화하면 데스크톱 포커스를 끌어올 수 있음)
   - ⌘] / ⌘[ (`terminal.focusNext/Previous`, `ui/src/lib/shortcuts.ts:300,307`)
   - 탭/레이아웃 리듀서 부작용(ADD_TAB activate, CLOSE_PANE 폴백, 워크트리 전환)
   - `onBlur`(`NativeTerminalPane.tsx:2378`)는 조합 상태(`isComposingRef`, preedit)까지 리셋하므로
     한글 타이핑 도중이면 조합이 끊기는 피해가 겹친다.

## 타이핑 중 점프가 자주 일어나는 이유

- 싱크 blur 트리거가 잦다: WebKit 포커스 리셋(클릭 후 22~24ms BODY 복귀 — 2026-08-29 실측 기록),
  윈도우 전환, 패널 재부착, 리렌더/리마운트. blur 직후 첫 키가 항상 스토어 페인으로 감.
- 에이전트가 백그라운드 페인에서 계속 출력 중이면 위 트리거 노출 빈도가 올라간다.

## 확정 / 미확정

- **확정(코드)**: 소유 판정이 스토어 activeLeafId를 우위로 두는 것(2), 네이티브 포커스 이벤트가
  스토어를 갱신하지 않는 것(1), active 전환 시 삼중 강제 포커스(3). 이 세 가지의 조합이면
  증상이 필연적으로 재현된다.
- **미확정(라이브 확인 필요)**: 실제 블러를 유도하는 트리거의 빈도/종류. DEV 빌드에서
  재현 직후 `/tmp/ferryx-switch-debug.jsonl`을 보면 확정된다:
  - `terminal.surface.focus.blur` 직후 온 `terminal.surface.input.capture`의 소유 페인
  - `terminal.surface.focus.native`(마우스 업) 이벤트와 blur 시점의 상대 순서
  - (현재 이 세션에서는 트레이스 파일이 존재하지 않았다 — 2026-09-16 확인)

## 수정 방향 (요청 시 착수)

> **2026-09-16 밤 업데이트: 구현 + 코드 리뷰(FIX-FIRST) 반영 후 최종 확정.**
> - 방향 1(스토어 동기화)은 유지하되, 리뷰에서 발견된 결함을 수정: 파킹된 워크트리 레이아웃에만 존재하는 세션(지연 LeftMouseUp 레이스)과 leaf-less 히트(분할 없는 탭)는 조기 반환으로 드랍 — 그렇지 않으면 FOCUS_EXISTING_SESSION이 사용자를 이전 워크트리로 튕겨냄.
> - 방향 2(캡처 폴백 우선순위 교체)는 **구현 후 롤백**: `lastFocusedNativeTerminalSessionId` 우선은 `active`/hovered 분기를 데드 코드화하고, 터미널+브라우저 혼합 분할에서 비활성 터미널 페인이 body 키다운을 가로채는 회귀를 만듦. 방향 1만으로 원래 버그가 닫히므로 스토어가 body 키 라우팅의 유일한 권위로 남는다.
> - 회귀 잠금: ① 네이티브 포커스 → activeLeafId 동기화, ② 파킹 워크트리 히트 무시, ③ 비활성 마지막-포커스 페인의 키 강탈 금지. 3개 스위트 242/242 통과.

## 관련 기록

- `docs/TERMINAL_TYPING_INPUT_RECOVERY_2026-08-28.md` (키 전달 복구, 4중 원인 스택)
- ensureTabForWorktree 활성 강탈 수정(~2026-08-28) — "탭 4에서 타이핑하면 탭 1로 점프"의 선행 사례
- 2026-08-29 네이티브 분할 포커스 라우팅 수정(AppKit LeftMouseUp 모니터 도입) — 이번 결함의
  반쪽(스토어 미동기화)을 남긴 채 싱크 포커스만 해결한 상태
