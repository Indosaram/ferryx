# 프로세스 종료/Killed 시 "Failed to attach native terminal" 에러 발생 원인 분석

- **작성일자:** 2026-09-07
- **관련 커밋:** `df846aa` ("feat(native-terminal): recover terminal input attach and typing"), `1c838ed` ("chore: consolidate in-flight workspace, agent, and daemon work")
- **관련 소스 파일:**
  - `ui/src/components/NativeTerminalPane.tsx` (줄 1793-1835, 줄 2295-2308)
  - `ui/src/components/TerminalPane.tsx` (줄 81, 줄 146-170)
  - `ui/src/state/workspaceStore.ts` (줄 316-322, 줄 1939-1965)
  - `src-tauri/src/native_terminal/surface_host.rs` (줄 1068-1091, 줄 1550-1555)
  - `src-tauri/src/ipc/native_terminal.rs` (줄 449-491)
  - `src-tauri/src/terminal/pty.rs` (줄 217-268, 줄 284-297)
  - `src-tauri/src/daemon/server.rs` (줄 1407-1475, 줄 2491-2503)

---

## 1. 증상 요약 (Image #1 분석)

터미널 내에서 실행 중인 프로세스(쉘, 에이전트, 서브프로세스) 또는 백그라운드 데몬이 종료(`kill -9`, SIGKILL, SIGTERM, crash, exit 등)되었을 때, 정상적인 종료 화면("Shell exited" / "Session disconnected") 대신 터미널 우측 하단에 빨간색 경고 배지 버튼으로 다음 에러가 표시됩니다.

```text
Failed to attach native terminal
```

이 버튼은 `ui/src/components/NativeTerminalPane.tsx`의 2296~2308번째 줄에 위치한 `<button role="alert" ...>{error}</button>` 엘리먼트입니다.

---

## 2. 전체 발생 메커니즘 및 호출 흐름

### 2.1. 백엔드 및 데몬에서의 프로세스 종료 감지 및 정리
1. **PTY 프로세스 사망:**
   - 쉘이나 터미널 프로세스가 kill되면 PTY 마스터 파일 디스크립터가 EOF 상태가 됩니다.
2. **데몬(`pty.rs` / `service.rs`)의 정리:**
   - `PtyManager::start_lifecycle_watcher`가 `session.poll_exit_code()` 또는 EOF를 감지합니다.
   - `finalize_natural_exit` 및 `close_session`이 호출되어 데몬의 활성 세션 맵(`PtyManager.sessions`)과 `TerminalOutputHub`에서 해당 `session_id`가 완전히 제거됩니다.
   - 데몬 스트림 펌프가 클라이언트로 `DaemonStreamMessage::Exit { session_id, exit_code }`를 전송하고 스트림을 닫습니다.
3. **Rust GUI 네이티브 서피스 호스트(`surface_host.rs`):**
   - `DaemonStreamMessage::Exit`를 수신한 `surface_host`의 스트림 태스크와 펌프 태스크가 루프를 빠져나와 종료(`is_finished() == true`)됩니다.
4. **Tauri IPC 이벤트 발송(`ipc/terminal.rs`):**
   - 관리 펌프(`start_managed_pump`)가 `DaemonStreamMessage::Exit`를 수신하고 프론트엔드로 `terminal_lifecycle` 이벤트를 발송합니다:
     ```rust
     emit_terminal_exit(&app, &session_id_clone, exit_code);
     // state: TerminalLifecycleState::Exited
     ```

---

### 2.2. 프론트엔드 상태 관리 결함 (핵심 원인 1: `workspaceStore.ts`)

프론트엔드의 `terminalEventBus`가 `terminal_lifecycle` 이벤트를 수신하여 `workspaceStore`의 리듀서로 `SESSION_LIFECYCLE` 액션을 전달합니다.

```typescript
// ui/src/state/workspaceStore.ts:1939-1954
case "SESSION_LIFECYCLE": {
  const matchedSessionIds: string[] = [];
  const sessions = Object.fromEntries(
    Object.entries(state.sessions).map(([id, session]) => {
      if (session.backendSessionId !== action.backendSessionId) return [id, session];
      matchedSessionIds.push(id);
      return [
        id,
        {
          ...session,
          lifecycle: action.lifecycle, // "exited"로 변경됨
          reconnectLifecycle: action.lifecycle === "exited" ? "idle" : session.reconnectLifecycle,
        },
      ];
    }),
  ) as Record<string, TerminalSession>;
  ...
```

- **결함:** 세션의 `lifecycle`은 `"exited"`로 변경되지만, **`backendSessionId`는 여전히 이전의 죽은 세션 ID 문자열을 그대로 유지**합니다(`null`로 초기화되지 않음).

---

### 2.3. 컴포넌트 종료 조건 불일치 (핵심 원인 2: `TerminalPane.tsx`)

`TerminalPane` 컴포넌트는 세션이 종료되었는지(`isExited`)를 판단할 때 치명적인 버그가 있습니다.

```typescript
// ui/src/components/TerminalPane.tsx:81
const isExited = session.backendSessionId === null;
```

- **결함:** `TerminalPane`은 `session.lifecycle === "exited"` 여부를 전혀 보지 않고 오직 `session.backendSessionId === null`인지만 확인합니다.
- 앞서 2.2에서 `backendSessionId`는 `null`이 아닌 죽은 ID 문자열로 남아있기 때문에, `isExited`는 **`false`**가 됩니다!
- 그 결과, 원래 표시되어야 할 아래의 종료 오버레이 화면이 **표시되지 않습니다**:
  ```tsx
  {isExited ? (
    <div data-testid="terminal-pane-overlay">
      <h2>{isAgentSession ? "Session disconnected" : "Shell exited"}</h2>
      <Button onClick={handleOpenNewShell}>Open new shell</Button>
    </div>
  ) : null}
  ```
- 대신 `TerminalPane`은 죽은 세션 ID를 가진 `<NativeTerminalPane>`을 DOM에 그대로 유지합니다.

---

### 2.4. 죽은 세션에 대한 Attach 시도 및 에러 배지 노출 (핵심 원인 3: `NativeTerminalPane.tsx`)

`<NativeTerminalPane>`은 `session.lifecycle === "exited"` 상태임에도 불구하고 `targetSessionId`가 문자열로 존재하므로 동작 중인 세션으로 간주합니다.

```typescript
// ui/src/components/NativeTerminalPane.tsx:538
const targetSessionId = session ? (session.backendSessionId ?? null) : (sessionId ?? null);
```

이 상태에서 다음 상황 중 하나가 발생하면 `attemptAttach(0)`가 실행됩니다:
1. **탭 전환 또는 윈도우 포커스 복귀:**
   - 다른 탭으로 갔다가 돌아오거나 창을 전환하면 `visible`이 `false` -> `true`로 바뀌면서 `useEffect`가 실행되고 `attemptAttach(0)`를 호출합니다.
2. **리사이즈 및 렌더링 변경:**
   - 창 크기 변경, 사이드바 토글, 분할 영역 조정 등으로 `ResizeObserver`가 발화합니다.
3. **컴포넌트 리렌더링 및 재연결 시도:**
   - 세션 상태 갱신에 따라 다시 마운트/연결을 시도합니다.

`attemptAttach(0)`가 호출되면 백엔드로 `cmd_native_terminal_attach` IPC를 호출합니다:

```rust
// src-tauri/src/ipc/native_terminal.rs:469-481
match state.reattach_existing_session_with_bounds(&session_id, logical_bounds)? {
    true => return Ok(()),
    false => {}
}

let attachment = match daemon_client.attach(&session_id, after_seq).await {
    Ok(attachment) => attachment,
    Err(err) => return Err(err), // 여기서 에러 반환!
};
```

1. `reattach_existing_session_with_bounds`:
   - `surface_host.rs`의 1081~1091줄에서 `stream_task`와 `pump_task`의 생존 여부(`!task.is_finished()`)를 확인합니다.
   - 프로세스가 죽었으므로 두 태스크는 이미 종료되어 `stream_is_live == false`입니다.
   - 따라서 `Ok(false)`를 반환하고 데몬 attach로 넘어갑니다.
2. `daemon_client.attach`:
   - 데몬에게 해당 `session_id`로 attach를 요청합니다.
   - 데몬은 이미 프로세스 종료 후 세션을 삭제했으므로 `"Session '<id>' not found"` 에러를 반환합니다.
3. `cmd_native_terminal_attach`가 reject됩니다.
4. `NativeTerminalPane`의 재시도 루프:
   ```typescript
   // ui/src/components/NativeTerminalPane.tsx:1821-1825
   reportNativeTerminalIpcFailure("cmd_native_terminal_attach", error);
   const willRetry = retryCount < maxRetries;
   if (!willRetry || retryCount >= bannerRetryThreshold) {
     setError("Failed to attach native terminal");
   }
   ```
   - 2회 재시도(~750ms) 후 `bannerRetryThreshold`에 도달하여 `setError("Failed to attach native terminal")`가 실행됩니다.
   - 최종적으로 우측 하단에 Image #1의 빨간색 알림 배지가 노출됩니다.

---

## 3. 요약: 3가지 구조적 결함의 연쇄 반응

- **1차 결함 (State Reducer):**
  - 프로세스 종료 이벤트(`terminal_lifecycle: exited`)를 수신했을 때 `session.lifecycle`은 `"exited"`로 변경하지만, `session.backendSessionId`는 삭제(`null`)하지 않고 그대로 둠.
- **2차 결함 (UI Overlay Check):**
  - `TerminalPane.tsx`가 세션 종료 판단을 `session.backendSessionId === null`로만 판별하여, 프로세스가 죽었음에도 "Shell exited" 오버레이를 띄우지 못하고 계속 `NativeTerminalPane`을 렌더링함.
- **3차 결함 (Native Terminal Attach Fallthrough):**
  - 이미 종료된(`lifecycle === "exited"`) 세션에 대해 `NativeTerminalPane`이 attach를 방지하는 가드가 없어, 탭 전환이나 리사이즈 시 데몬에 존재하지 않는 세션 ID로 attach를 계속 시도하다가 실패하여 "Failed to attach native terminal" 배지를 노출함.

---

## 4. 권장 해결 방안 (Remediation)

1. **`TerminalPane.tsx`의 종료 판별식 수정:**
   ```typescript
   // ui/src/components/TerminalPane.tsx
   const isExited = session.backendSessionId === null || session.lifecycle === "exited";
   ```
   - 세션이 종료되면 즉시 "Shell exited" / "Session disconnected" 오버레이가 정상적으로 나타나며 새 쉘 열기 또는 재연결 버튼이 제공됩니다.

2. **`NativeTerminalPane.tsx`에서 종료된 세션에 대한 Attach 시도 차단:**
   ```typescript
   // ui/src/components/NativeTerminalPane.tsx
   const isExited = session?.lifecycle === "exited";
   if (isExited) return; // attemptAttach 실행 방지 및 error 초기화
   ```

3. **`workspaceStore.ts`의 `SESSION_LIFECYCLE` 처리 정리:**
   - 쉘 세션 종료 시 `backendSessionId`와 `lifecycle` 간의 일관성을 맞추거나, reconnect affordance 로직과 정합성을 유지하도록 상태 전이를 명확히 정립.
