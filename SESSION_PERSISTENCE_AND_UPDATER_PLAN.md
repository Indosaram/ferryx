# Orca-Lite (rorca) 세션 복원 및 자동 업데이트 가드 구현 계획

## 1. 개요 및 목적
`orca-lite(rorca)`에서 예기치 않은 앱 크래시, OS 재시작 또는 자동 업데이트(`window.api.updater`)에 의한 재시작 시에도 사용자의 작업 컨텍스트(프로젝트, 워크트리, 탭, 분할 레이아웃, 터미널 세션 상태)가 유실되지 않도록 **세션 영속화(Session Save & Restore)** 및 **업데이트 전 종료 보호 가드(Updater Beforeunload Guard)**를 구현합니다.

---

## 2. 세션 데이터 모델 (Session State Schema)

### 2.1 영속화 대상 데이터 구조
```typescript
export interface PersistedWorkspaceSession {
  version: 1;
  timestamp: number;
  activeWorkspaceId: string;
  workspaces: Record<string, PersistedWorkspace>;
}

export interface PersistedWorkspace {
  workspaceId: string;
  repoRoot: string;
  worktrees: PersistedWorktree[];
  activeWorktreePath: string | null;
  layout: PersistedLayout;
  terminalSessions: Record<string, PersistedTerminalSession>;
}

export interface PersistedWorktree {
  path: string;
  branch: string;
  head: string;
  isMain: boolean;
  isLocked: boolean;
}

export interface PersistedLayout {
  splitMode: "none" | "horizontal" | "vertical";
  primaryTabId: string | null;
  secondaryTabId: string | null;
  tabs: PersistedTab[];
}

export interface PersistedTab {
  id: string;
  sessionId: string;
  label: string;
  customTitle?: string;
  worktreePath: string;
}

export interface PersistedTerminalSession {
  sessionId: string;
  worktreePath: string;
  cwd: string;
  lastCommand?: string;
  // 복원 시 이전 출력 복구를 위한 최근 스크롤백 버퍼 요약 (최대 100~500줄)
  recentScrollback?: string;
  createdAt: number;
}
```

---

## 3. 백엔드(Rust) 저장소 아키텍처 및 IPC 설계

### 3.1 저장소 엔진 및 경로
- **저장 위치**: Tauri `app_data_dir()` / `session_state.json` (원자적 쓰기 `Atomic Write` 적용: `.tmp` 파일 작성 후 원자적 rename).
- **디바운스 저장 (Debounced Auto-Save)**: UI 상태 변경 이벤트 수신 시 500ms 디바운스 후 디스크 기록.
- **크래시 복구 가드**: 손상된 JSON 감지 시 `.backup` 파일에서 폴백 복원.

### 3.2 백엔드 IPC 커맨드 추가
1. `cmd_session_save(state: PersistedWorkspaceSession) -> Result<(), IpcError>`
2. `cmd_session_load() -> Result<Option<PersistedWorkspaceSession>, IpcError>`
3. `cmd_session_clear() -> Result<(), IpcError>`
4. `cmd_terminal_respawn_batch(requests: Vec<RespawnTerminalRequest>) -> Result<Vec<RespawnTerminalResponse>, IpcError>`

---

## 4. 복원(Restore) 라이프사이클 및 PTY 재연결 전략

### 4.1 복원 플로우
```
[App Launch]
    │
    ▼
`cmd_session_load()` 호출
    │
    ├── 저장된 세션 없음 ──► 초기 프로젝트 선택/등록 화면 표시
    │
    └── 저장된 세션 존재
            │
            ├─ 1. WorkspaceRegistry에 repoRoot 등록 및 워크트리 목록 동기화
            ├─ 2. 저장된 탭에 대해 PTY 터미널 세션 일괄 재스폰 (CWD 기반)
            ├─ 3. UI 레이아웃(Primary/Secondary 탭, 분할 모드) 복원
            └─ 4. 이전 스크롤백 텍스트(선택적) 터미널 화면에 주입 후 준비 완료
```

### 4.2 CWD 유효성 검증
- 복원 시점에 워크트리 또는 디렉터리가 삭제되었거나 이동한 경우, 안전하게 `repoRoot`를 fallback CWD로 설정하고 알림 처리.

---

## 5. 자동 업데이트(Updater) 및 Beforeunload 가드 연동

### 5.1 `window.api.updater` 구조 매핑
- **상태 관리**: `idle` ➔ `checking` ➔ `available` ➔ `downloading` ➔ `downloaded` ➔ `error`
- **Tauri v2 연동**: `tauri-plugin-updater`를 활용하여 원격 릴리즈 매니페스트 확인 및 백그라운드 다운로드.

### 5.2 종료 보호 가드 (`updater-beforeunload guard`)
1. 사용자가 "Restart to Update" 클릭 또는 자동 재시작 트리거 시:
2. `registerWindowCloseGuard`를 활성화하여 즉시 프로세스 종료를 막음.
3. 현재 `workspaceStore`의 최신 상태를 `cmd_session_save()`로 동기적/즉시 플러시(Flush).
4. 실행 중인 터미널 PTY 세션들에 대해 안전 종료 신호(SIGHUP/SIGTERM) 전달 및 정리.
5. 저장 완료 확인 후 `tauri::process::restart` 또는 새 바이너리 실행 진행.

---

## 6. 단계별 구현 로드맵 (Phases)

| Phase | 주요 작업 내용 | 산출물 |
|---|---|---|
| **Phase 1: 데이터 모델 및 백엔드 스토리지** | • `session_state.json` 원자적 입출력 모듈 (`src-tauri/src/session/` 또는 `ipc/session.rs`)<br>• 백엔드 IPC (`cmd_session_save`, `cmd_session_load`) 구현 | Rust 단위 테스트 및 IPC 단위 테스트 |
| **Phase 2: UI 상태 영속화 및 자동 저장** | • `ui/src/state/sessionPersistence.ts` 작성<br>• `workspaceStore` 상태 변경 시 디바운스 자동 저장 연동 | 상태 직렬화/역직렬화 테스트 |
| **Phase 3: 앱 부팅 시 세션 복원 플로우** | • 앱 초기화 시 `cmd_session_load` 호출 및 레이아웃/터미널 일괄 복구<br>• 유효하지 않은 워크트리 경로 예외 처리 | 복원 시나리오 Vitest / E2E 테스트 |
| **Phase 4: 자동 업데이트 & Beforeunload 가드** | • `window.api.updater` / `tauri-plugin-updater` 연동<br>• `UpdateCard` UI 및 재시작 전 세션 세이프가드 플러시 구현 | 업데이트 재시작 시뮬레이션 검증 |
| **Phase 5: E2E 통합 검증 & 하드닝** | • 강제 종료(SIGKILL) 후 재시작 시 복원 검증<br>• 터미널 분할 및 다중 탭 상태 유지 회귀 테스트 | 종합 검증 완료 보고서 |
