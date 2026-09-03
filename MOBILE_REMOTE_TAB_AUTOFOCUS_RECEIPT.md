# Mobile Remote Tab Switch Auto-Focus Receipt (2026-09-02)

## Request
모바일에서 탭 전환(탭 바 칩, Prev/Next 버튼, 스와이프 제스처, 워크트리 셀렉터 등) 시 터미널 영역을 따로 탭/클릭하지 않아도 즉시 타이핑이 가능하도록 자동 포커스 구현.

## Root Cause
- 모바일 리모트 클라이언트는 한글/CJK 조합 입력 및 소프트 키보드 연결을 위해 숨겨진 `<textarea data-testid="remote-terminal-input-sink">`를 사용함.
- 사용자가 탭 버튼이나 이전/다음 버튼을 탭하면 브라우저 포커스가 해당 `<button>` 요소로 이동하거나 터미널 밖으로 빠져나감.
- 터미널 서피스를 다시 터치(`onPointerDown`)하기 전까지는 `inputSink`에 포커스가 없어 모바일 소프트 키보드가 닫히거나 물리/소프트 입력이 PTY로 전달되지 않음.

## Changes Made
1. **`ui/src/remote/RemoteTerminal.tsx`**:
   - `activeTabId` prop 추가 (`RemoteTerminalProps`).
   - `useLayoutEffect`를 통해 컴포넌트 마운트 시, `sessionId` 변경 시, `activeTabId` 변경 시 즉시 `focusInput({ preventScroll: true })` 호출.
   - 소켓 연결 완료(`connected === true`) 시점에도 `focusInput()` 실행.
   - 좌우 스와이프 탭 전환(`handleTouchEnd`) 제스처 인식 직후에도 `focusInput()` 호출.
   - `<textarea>` 입력 싱크에 터미널 입력 표준 하이진 속성 추가 (`autoCapitalize="none"`, `autoComplete="off"`, `autoCorrect="off"`, `spellCheck={false}`).
2. **`ui/src/remote/RemoteApp.tsx`**:
   - `RemoteTerminal`에 `activeTabId={model.context.activeTabId}` 전달.
   - Attention 배지 탭 클릭 후 즉시 `inputSink`로 포커스 이동.
3. **`ui/src/remote/RemoteSessionList.tsx`**:
   - `focusTerminalInput()` 헬퍼 함수 추가.
   - 탭 바 칩 클릭(`onClick`), Prev/Next 버튼 클릭, 워크트리 셀렉터 옵션 선택 시 `onSelect` 직후 즉시 `focusTerminalInput()` 호출.
   - 이미 활성화된 탭을 다시 탭했을 때도 터미널 입력 싱크로 포커스 복원.
4. **`ui/src/components/MobileKeyDock.tsx`**:
   - 하단 모바일 키 독 버튼(`KeyButton`)의 `onPointerDown`에 `e.preventDefault()` 적용.
   - Ctrl, Alt, Esc, Tab, 화살표 등 키 독 버튼을 터치하더라도 터미널 textarea의 포커스가 풀리거나 모바일 가상 키보드가 닫히지 않도록 방지.
5. **`ui/src/remote/RemoteTerminal.contract.test.tsx`**:
   - 마운트 시, `activeTabId` 변경 시, `sessionId` 변경 시 input sink 자동 포커스 및 포커스 유지 단위 테스트 2건 추가.

## Verification
- `bunx vitest run src/remote`: **5개 파일 89개 테스트 전체 통과** (RemoteTerminal.contract 25건, RemoteUI 39건, RemoteTerminalGestures 10건, RemoteAttention 9건, terminalGridProtocol 6건).
- `bun run build`: 클린 빌드 완료 (`RemoteApp` 번들 43.14 kB).
