# 앱 재시작 시 running 상태 판별 결함 원인 분석 및 해결 보고서

- **작성일자:** 2026-09-07
- **대상 파일:**
  - `src-tauri/src/ipc/terminal.rs` (`TerminalSessionSummary`, `cmd_terminal_list`)
  - `src-tauri/src/ipc/tests.rs`
  - `ui/src/lib/types.ts` (`TerminalSessionSummary`)
  - `ui/src/lib/terminalTransport/types.ts` (`TerminalTransport.listSessions`)
  - `ui/src/lib/terminalTransport/tauriTransport.ts` (`TauriTerminalTransport.listSessions`)
  - `ui/src/state/workspaceRestore.ts` (`defaultListLiveBackendSessionIds`)
  - `ui/src/lib/sessionPersistence.ts` (`deserializeWorkspaceState`)
  - `ui/src/state/workspaceStore.ts` (`selectAgents`)
  - `ui/src/lib/sessionPersistence.test.ts`

---

## 1. 문제 증상

앱(Ferryx Desktop) 실행 중 터미널/에이전트가 `working`/`running` 상태로 동작하고 있을 때 앱을 재시작하면,
백그라운드 데몬(`ferryx --daemon`) 및 PTY 프로세스가 정상적으로 살아있음에도 불구하고:
1. 사이드바 프로젝트 헤더의 `running` 카운트(`{activity.runningCount} running`)가 `0`으로 소실되거나 배지가 표시되지 않음.
2. 탭 바의 실행 중 스피너(working indicator)가 표시되지 않음.
3. 활성 에이전트 상태 목록(`selectAgents`)에서 실행 중인 에이전트가 `exited` 또는 비활성 상태로 오인됨.
4. 반대로 데몬 내부에서 프로세스가 이미 종료(`running: false`)된 PTY 세션이 앱 재시작 시 `working`으로 오판되어 데드 세션에 attach를 시도하다 에러가 발생하는 문제.

---

## 2. 근본 원인 (Root Causes)

### 원인 1: 역직렬화 시 `activity.state === "working"` 무조건 `done` 강제 변환
- **위치:** `ui/src/lib/sessionPersistence.ts` (`deserializeWorkspaceState`)
- **분석:**
  - 이전 커밋(`ba90ceb`, `6e0f76d`)에서 앱 시작 시 attention frame(하이라이트 테두리)이 켜지는 현상을 막기 위해 `working` 상태를 "재시작 후에는 아무것도 돌고 있지 않으므로 stale한 상태"로 가정하고 무조건 `state: "done"`으로 강제 변환(`isStaleRunClaim`)했습니다.
  - 하지만 Ferryx는 데몬 프로세스가 GUI 재시작 후에도 PTY 세션을 계속 유지하는 구조입니다.
  - 따라서 앱 재시작 시점에 백엔드 데몬에서 실제로 살아있는(`lifecycle !== "exited"`) 세션의 `working` 상태까지 모두 `done`으로 지워버려, 재시작 직후 running 상태 판별이 불가능해졌습니다.

### 원인 2: 백엔드 `cmd_terminal_list`의 `running` 속성 누락
- **위치:** `src-tauri/src/ipc/terminal.rs`
- **분석:**
  - 데몬의 `describe_session`은 각 PTY의 실제 실행 여부(`details.running: bool`)를 정확히 알고 있습니다(`PtySessionState::Starting | PtySessionState::Running`).
  - 그러나 프론트엔드가 활성 세션을 조회하는 `cmd_terminal_list` IPC 명령은 `TerminalSessionSummary { session_id, worktree_path }`만 반환하고 `running` 여부를 누락했습니다.
  - 그 결과 프론트엔드 역직렬화기(`deserializeWorkspaceState`)는 데몬 테이블에 남아있으나 PTY 프로세스가 이미 종료된 세션을 살아있는 `lifecycle = "working"` 세션으로 오판별했습니다.

### 원인 3: `selectAgents`의 라이프사이클 매핑 불일치
- **위치:** `ui/src/state/workspaceStore.ts`
- **분석:**
  - `session.lifecycle`은 `"working"`과 `"running"`을 모두 가질 수 있는데, `selectAgents`는 `session.lifecycle === "running"`만 확인하고 `"working"`을 놓쳐 에이전트 상태를 올바르게 `"working"`으로 매핑하지 못하는 엣지 케이스가 존재했습니다.

---

## 3. 해결 내용

### 1) 백엔드 IPC DTO 및 세션 목록 반환 개선
- `src-tauri/src/ipc/terminal.rs`:
  - `TerminalSessionSummary` 구조체에 `#[serde(default)] pub running: bool` 필드 추가.
  - `cmd_terminal_list`에서 `details.running` 값을 `summaries`에 포함하여 전달.
- `src-tauri/src/ipc/tests.rs`:
  - `cmd_terminal_list` 호출 시 반환된 세션의 `session.running == true` 검증 assertion 추가.

### 2) 프론트엔드 전송 및 역직렬화 파이프라인 연계
- `ui/src/lib/types.ts`:
  - `TerminalSessionSummary`에 `running?: boolean` 추가.
- `ui/src/lib/terminalTransport/types.ts` & `tauriTransport.ts`:
  - `listSessions()` 인터페이스 및 구현체에서 `running: s.running ?? true` 전달.
- `ui/src/state/workspaceRestore.ts`:
  - `defaultListLiveBackendSessionIds()`에서 `running` 속성을 보존하여 역직렬화기로 전달.

### 3) 세션 생존 상태 기반 정밀 running 판별
- `ui/src/lib/sessionPersistence.ts`:
  - `liveSessionMap`에서 각 세션의 `running` 여부를 추적.
  - `epochMatches && isProcessRunning`인 경우에만 `lifecycle = "working"`, 프로세스가 이미 종료된 경우 `lifecycle = "exited"` 및 `backendSessionId = null` 처리.
  - `activityBySessionId` 복원 시:
    ```typescript
    const isSessionLive = Boolean(session && session.backendSessionId !== null && session.lifecycle !== "exited");
    const isStaleRunClaim = activity.state === "working" && !isSessionLive;
    restoredActivity[sessionId] = {
      state: isStaleRunClaim ? "done" : activity.state,
      ...
    };
    ```
    - 데몬에 살아있고 실제로 실행 중인 세션은 `activity.state === "working"`을 그대로 유지하여 사이드바 `runningCount`와 탭 바 스피너가 정확히 복원됨.
    - 세션이 사망했거나 프로세스가 종료된 경우에만 `done`으로 정상 정착.

### 4) `selectAgents` 라이프사이클 처리 보완
- `ui/src/state/workspaceStore.ts`:
  - `session.lifecycle === "running" || session.lifecycle === "working"` 모두 agentState `"working"`으로 매핑.

---

## 4. 검증 결과

1. **Rust Backend:**
   - `cargo check --manifest-path src-tauri/Cargo.toml`: 에러 0건 (Clean)
   - `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::tests`: 15 passed, 0 failed
2. **Frontend Test Suite:**
   - `sessionPersistence.test.ts`: 27 passed (생존 세션 running 유지, 사망 세션 done 변환, running: false 감지 테스트 포함)
   - `workspaceStore.test.tsx`: 47 passed
   - `startupPaneAttention.integration.test.tsx`: 1 passed (phantom attention 미발생 확인)
   - `workspaceRestore.test.tsx`: 17 passed
   - `workspaceActivity.test.tsx`: 14 passed
   - `Sidebar.activity.test.tsx`: 4 passed
   - `sessionEpochRestore.test.ts`: 4 passed
   - `TerminalPane.test.tsx`: 18 passed
   - **총 8개 테스트 파일 132개 테스트 전원 통과**
3. **Frontend Production Build:**
   - `bun run --cwd ui build`: TypeScript typecheck 및 Vite 프로덕션 빌드 성공 (소요시간 2.49초)
