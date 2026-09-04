# Code Review (Round 2): Terminal Pane Click Focus & Keyboard Typing Resolution

**Review Date**: 2026-09-04
**Review Round**: Round 2 (Post-Hardening & Edge-Case Implementation)
**Scope**:
- `src-tauri/src/lib.rs` (`install_macos_terminal_focus_monitor`)
- `ui/src/components/TerminalSplitView.tsx` (`isInteractiveTarget`, `PaneLeafView`, `focusPaneInput`, timer refs)
- `ui/src/components/NativeTerminalPane.tsx` (`active` prop, `ownsInput` strict evaluation, `sendFocus` symmetry, 40ms timer confirmation)
- `ui/src/components/TerminalSplitView.paneFocus.test.tsx` (6 unit/integration tests)
- `ui/src/components/NativeTerminalPane.test.tsx` (117 unit/integration tests)

---

## 1. Executive Summary & Verdict

**Verdict**: **Approved (승인 - 프로덕션 배포 준비 완료)**

1차 코드 리뷰에서 도출되었던 핵심 엣지 케이스(검색 오버레이 입력창 포커스 탈취, 비활성 페인의 hover 입력 소유권 폴스루, rAF/setTimeout 수명 주기 정리 누락)가 완벽히 보완되었으며, 관련 회귀 방지 테스트가 작성되어 전체 100% 그린 상태를 확인했습니다.

코드 품질, 성능 영향도, 플랫폼 호환성, 레이스 컨디션 방어 등 전 영역에서 매우 우수한 완성도를 갖추었습니다.

---

## 2. 세부 검토 영역별 분석 (Detailed Analysis)

### A. 대화형 입력 요소 가드 (`isInteractiveTarget`)
- **코드**:
  ```tsx
  function isInteractiveTarget(target: HTMLElement | null): boolean {
    return Boolean(
      target?.closest(
        "button, input, select, textarea:not([data-testid='native-terminal-focus-sink']), [contenteditable='true'], [role='search']",
      ),
    );
  }
  ```
- **평가**:
  - `closest()`를 활용하여 O(depth)의 가벼운 네이티브 DOM 탐색으로 대화형 요소를 정확히 식별합니다.
  - `button`, `input`, `select`, `contenteditable`, `[role='search']`뿐만 아니라 `textarea:not([data-testid='native-terminal-focus-sink'])`를 지정하여 터미널 자체의 숨김 싱크와 외부 입력 컨트롤을 정밀하게 구분합니다.
  - Cmd+F 검색창 입력 시 포커스가 터미널로 강제 탈취되는 1차 리뷰 이슈가 완벽히 차단되었습니다.

### B. 타이머 수명 주기 및 재진입 관리 (`focusPaneInput`)
- **코드**:
  ```tsx
  const leafRef = React.useRef<HTMLDivElement | null>(null);
  const focusFrameRef = React.useRef<number | null>(null);
  const focusTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(() => {
    return () => {
      if (focusFrameRef.current !== null) cancelAnimationFrame(focusFrameRef.current);
      if (focusTimerRef.current !== null) clearTimeout(focusTimerRef.current);
    };
  }, []);
  ```
- **평가**:
  - 빠른 연속 클릭 시 이전 예약 프레임 및 타이머를 취소(`cancelAnimationFrame`, `clearTimeout`)한 후 갱신하도록 설계되어 불필요한 타이머 콜백 누적을 방지합니다.
  - 컴포넌트 언마운트 시 활성화된 비동기 콜백을 안전하게 해제하여 메모리 누수 및 죽은 DOM 노드 접근을 원천 방지했습니다.

### C. 전역 키다운 캡처 소유권 엄격 분기 (`ownsInput`)
- **코드**:
  ```tsx
  const ownsInput = targetedPane
    ? targetedPane === containerRef.current
    : active !== undefined
      ? active
      : hoveredPane
        ? hoveredPane === containerRef.current
        : fallbackSessionId === targetSessionId;
  ```
- **평가**:
  - `active`가 명시된 경우(`active !== undefined`), `hoveredPane` 상태보다 `active` 불리언 값을 우선적으로 평가합니다.
  - 분할 페인 환경에서 활성 페인(Pane 1)에 타이핑 중 마우스 커서가 우연히 비활성 페인(Pane 2) 위에 올라가 있더라도, Pane 2가 입력을 가로채지 못하도록 완벽히 격리되었습니다.

### D. AppKit 네이티브 포커스 모니터 및 듀얼 이벤트 전송
- **코드**:
  ```rust
  NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp
  ```
  ```rust
  let _ = window.emit(NATIVE_TERMINAL_FOCUS_EVENT, &session_id);
  let _ = app_handle.emit(NATIVE_TERMINAL_FOCUS_EVENT, &session_id);
  ```
- **평가**:
  - 마우스 클릭 다운 즉시 포커스 이벤트를 발행하여 마우스 드래그 선택 및 고속 타이핑 시 첫 글자 유실을 방지합니다.
  - `window`와 `app_handle` 양쪽으로 발행하여 전역 Tauri 리스너 등록 상태에 상관없이 결정론적 이벤트 수신을 보장합니다.

---

## 3. 마이너 권장 사항 (Optional Nitpicks)

프로덕션 배포에 지장을 주지는 않으나, 추가적인 안정성을 위해 고려할 수 있는 경미한 개선점:

- **`focus({ preventScroll: true })` 옵션 고려**:
  `sink?.focus()` 호출 시 `{ preventScroll: true }`를 전달하면, WebKit이 숨김 텍스트 영역의 오프셋 좌표로 뷰포트를 미세하게 자동 스크롤하려는 잠재적 렌더링 부작용을 사전에 차단할 수 있습니다.
- **브라우저 페인 분할 시 안전성**:
  `TerminalSplitView`의 분할 대상이 터미널이 아닌 브라우저 페인(`content.kind === "browser"`)인 경우, `leafRef.current?.querySelector('textarea[data-testid="native-terminal-focus-sink"]')`는 `null`을 반환하고 옵셔널 체이닝(`sink?.focus()`)으로 안전하게 무시됩니다. 브라우저 페인은 자체 Tauri 웹뷰 포커스 메커니즘을 사용하므로 동작상 문제없음이 확인되었습니다.

---

## 4. 검증 결과 요약

- `TerminalSplitView.paneFocus.test.tsx`: 6/6 통과
- `TerminalSplitView.test.tsx`: 20/20 통과
- `NativeTerminalPane.test.tsx`: 117/117 통과
- `cargo check --manifest-path src-tauri/Cargo.toml`: 0 에러
- `bun run --cwd ui build`: 0 에러 (성공적으로 빌드 완료)
