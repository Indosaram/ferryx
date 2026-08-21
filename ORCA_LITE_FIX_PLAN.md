# Orca Lite 수정 계획

> 기준: 2026-08-20 현재 working tree 구현 리뷰
> 대상: Tauri v2 + Rust (`portable-pty`, Git worktree backend) + React/xterm.js UI
> 목적: 현재 구현을 안정적인 멀티터미널·멀티워크트리 런타임으로 끌어올리고 데이터 손실, 세션 누수, 상태 불일치, 권한 경계 문제를 제거합니다.

## 1. 현재 상태와 핵심 목표

현재 구현의 기본 방향은 좋습니다.

- Rust backend가 `terminal/`, `worktree/`, `ipc/`로 분리되어 있습니다.
- `portable-pty` 멀티세션 spawn/write/resize/kill이 동작합니다.
- Git은 `Command::new("git").args(...)` 방식으로 실행되어 일반적인 shell injection에 강합니다.
- `orca/<ws>/<slug>` branch namespace와 Git ref validation이 구현되어 있습니다.
- dirty worktree는 `force=true`여도 삭제를 차단합니다.
- xterm.js + WebGL + FitAddon + ResizeObserver 기본 lifecycle이 구현되어 있습니다.
- 현재 코드에서 Rust unit test 19개 + E2E 1개, clippy, TypeScript/Vite build가 통과합니다.

Production 수준으로 가기 전에 다음 invariant를 코드와 테스트로 강제해야 합니다.

1. **논리 terminal tab 하나에는 PTY session이 정확히 하나만 존재해야 합니다.**
2. **PTY close는 child 종료, reap, reader 종료, registry 제거까지 책임져야 합니다.**
3. **worktree 하나에는 동시에 하나의 writer owner만 존재해야 합니다.**
4. **clean working tree라는 이유만으로 미병합 branch commit을 삭제해서는 안 됩니다.**
5. **Tauri IPC가 임의 filesystem path 또는 raw shell execution으로 바로 이어지지 않아야 합니다.**
6. **worktree/terminal/agent 상태는 단일 source of truth에서 UI와 동기화되어야 합니다.**

---

## 2. 우선순위

| 우선순위 | 항목 | 이유 |
|---|---|---|
| P1 | Split terminal session identity | 동일 tab에 PTY가 중복 생성되고 split 해제 시 작업 shell이 사라질 수 있습니다. |
| P1 | PTY lifecycle/RAII | orphan/zombie/stale session 및 blocking reader 누수 가능성이 있습니다. |
| P1 | Branch 삭제 안전화 | `git branch -D`가 clean-but-unmerged commit을 삭제할 수 있습니다. |
| P1 | 1-Writer-1-Worktree ownership | 현재는 dirty 삭제 보호만 있고 writer 독점이 없습니다. |
| P1 | Tauri IPC/path/CSP hardening | WebView compromise의 영향이 shell/path 접근으로 확대될 수 있습니다. |
| P2 | Output listener race | shell 초기 출력이 listener 등록 전에 유실될 수 있습니다. |
| P2 | PTY raw-byte streaming | UTF-8 multibyte 문자가 chunk 경계에서 깨질 수 있습니다. |
| P2 | Worktree realtime binding | 현재 초기 load/생성 이후에만 refresh됩니다. |
| P2 | Tab/split reducer | 마지막 tab close 및 secondary tab 상태가 불안정합니다. |
| P2 | IPC camelCase contract | request/response/event naming이 혼재합니다. |
| P3 | Bundle 및 blocking Git I/O 최적화 | 초경량 목표와 runtime responsiveness를 개선합니다. |

---

# Phase 0. Regression test 기반 확장

수정 전에 현재 숨어 있는 lifecycle/security invariant를 테스트로 먼저 표현합니다.

## 0.1 Rust terminal tests

대상:

- `src-tauri/src/terminal/tests.rs`
- `src-tauri/src/ipc/tests.rs`

추가 테스트:

- child 자연 종료 후 manager registry에서 자동 제거
- explicit close 후 child가 실제 종료되고 reap됨
- close 2회 호출이 안전한 idempotent behavior
- output receiver가 먼저 drop되어도 reader/session 정상 정리
- 빠른 spawn → close race
- foreground process group interrupt 동작

## 0.2 Worktree safety tests

대상:

- `src-tauri/src/worktree/mod.rs`
- `src-tauri/tests/e2e_agent_workflow.rs`

추가 테스트:

- clean이지만 미병합 branch를 기본 삭제가 거부
- explicit force branch delete만 미병합 branch 삭제 허용
- 동일 canonical worktree에 writer 2개 acquire 시 두 번째 요청 거부
- owner release 후 새 writer acquire 허용
- symlink/path traversal로 allowed root 밖 생성/삭제 차단
- leading-dash branch/ref/path 방어

## 0.3 Frontend test 도입

권장:

- Vitest
- React Testing Library

검증 항목:

- tab 1개에서 split 활성화 시 secondary pane 생성
- split을 켜도 `cmd_terminal_spawn` 횟수가 논리 tab 수보다 증가하지 않음
- split orientation 변경 시 backend PTY session ID 유지
- 마지막 tab close 후 valid tab 자동 선택/생성
- secondary tab close 후 valid split state 유지

### Phase 0 완료 조건

- 이후 수정 대상의 실패 시나리오가 테스트로 재현됩니다.
- 각 phase 구현 후 해당 regression test가 green이 됩니다.

---

# Phase 1. PTY Session lifecycle 재설계

대상:

- `src-tauri/src/terminal/session.rs`
- `src-tauri/src/terminal/pty.rs`
- `src-tauri/src/ipc/terminal.rs`

## 1.1 Session state model

권장 상태:

```text
Starting -> Running -> Closing -> Exited
                     -> Failed
```

예:

```rust
pub enum PtySessionState {
    Starting,
    Running,
    Closing,
    Exited { code: Option<i32> },
    Failed { reason: String },
}
```

`kill`, `close`, natural exit가 하나의 lifecycle 경로로 수렴하도록 합니다.

## 1.2 `close_session()` 단일 종료 API

현재:

```text
cmd_terminal_close
  -> kill() 오류 무시
  -> remove_session()
```

개선 후:

```text
PtyManager::close_session(id)
  1. state = Closing
  2. child/process group 종료 요청
  3. writer/master close
  4. blocking reader 종료 확인
  5. child wait/reap
  6. state = Exited
  7. registry remove
```

`cmd_terminal_close()`는 이 API만 호출하고 kill 오류를 삼키지 않습니다.

## 1.3 `spawn_blocking` reader shutdown

현재 `JoinHandle::abort()`만으로 실행 중 blocking `read()`가 확실히 중단되지 않습니다.

개선:

- master FD close를 먼저 보장해 `read()`가 EOF/EIO로 반환하도록 합니다.
- reader 종료를 기다릴 수 있는 Join/signaling 구조를 둡니다.
- shutdown wait에는 bounded timeout을 둡니다.

## 1.4 Natural exit cleanup

shell/command가 스스로 종료하면:

- output channel 종료
- child reap
- registry 제거
- frontend lifecycle event 전달

을 자동 수행합니다.

## 1.5 Signal semantics

일반 Ctrl-C 입력(`\x03`)은 PTY line discipline 경로를 유지합니다.

별도 signal IPC가 필요하다면 arbitrary `i32` 대신 enum을 사용하고, Unix에서는 단일 shell PID가 아니라 foreground process group 의미를 보장합니다.

```text
Interrupt
Terminate
Kill
```

### Phase 1 완료 조건

- 자연 종료/명시 close 모두 registry에서 제거됩니다.
- close 이후 child/orphan process가 남지 않습니다.
- blocking reader가 유실되지 않습니다.
- close가 idempotent합니다.

---

# Phase 2. Output transport 및 Split architecture 수정

대상:

- `src-tauri/src/ipc/terminal.rs`
- `ui/src/components/TerminalPane.tsx`
- `ui/src/App.tsx`
- `ui/src/lib/types.ts`

## 2.1 Output listener race 제거

현재 순서:

```text
backend spawn
-> output emit 시작
-> invoke가 session ID 반환
-> frontend listener 등록
```

따라서 초기 prompt가 유실될 수 있습니다.

권장안:

1. Tauri Channel 기반 stream으로 전환하거나,
2. app 전역 고정 `terminal_output` listener를 spawn 전에 등록하고 `sessionId`로 multiplexing합니다.

Dynamic event name을 유지한다면 listener-ready handshake가 필요합니다.

## 2.2 Raw bytes 보존

현재 backend는 각 read chunk를 `String::from_utf8_lossy`로 변환합니다. UTF-8 문자가 두 read 사이에 나뉘면 `�`로 손상될 수 있습니다.

변경:

- `Vec<u8>` 또는 binary-safe channel 사용
- 필요하면 base64 transport 사용
- frontend에서 xterm에 byte-safe하게 전달

한글·일본어·emoji를 포함한 multibyte output regression test를 추가합니다.

## 2.3 Terminal session과 React view 분리

현재 구조의 핵심 문제:

```text
TerminalPane mount == PTY spawn
```

Split view가 동일 tab을 다시 렌더링하면 새로운 PTY가 생깁니다.

목표 모델:

```text
TerminalSession
  id
  cwd
  backendSessionId
  lifecycle

TerminalTab
  id
  label
  sessionId

Pane
  id
  tabId
```

핵심 invariant:

> `TerminalTab.sessionId` 하나는 view 개수와 무관하게 backend PTY 하나만 소유합니다.

React component mount/unmount가 backend process ownership을 직접 결정하지 않게 합니다.

## 2.4 Layout reducer

현재 `tabs`, `activeTabId`, `secondaryTabId`, `splitOrientation`이 독립 `useState`라 invalid combination이 생길 수 있습니다.

예:

```ts
type LayoutState = {
  tabs: TerminalTab[];
  primaryTabId: string | null;
  secondaryTabId: string | null;
  split: "none" | "horizontal" | "vertical";
};
```

Action:

```text
ADD_TAB
CLOSE_TAB
ACTIVATE_PRIMARY
ACTIVATE_SECONDARY
ENABLE_SPLIT
ROTATE_SPLIT
DISABLE_SPLIT
```

Reducer가 항상 다음을 보장합니다.

- primary/secondary ID는 존재하는 tab만 참조
- split 활성 시 유효한 secondary ID 존재
- 마지막 tab close 정책이 원자적으로 적용

## 2.5 Single-tab split 수정

단일 tab 상태에서 split 요청 시 하나의 action에서:

1. 새 tab/session 생성
2. 새 ID를 secondary에 할당
3. split 활성화

를 처리합니다.

### Phase 2 완료 조건

- tab 2개 + split에서 backend PTY도 정확히 2개입니다.
- split orientation 변경으로 session ID가 바뀌지 않습니다.
- split 해제로 shell이 종료되지 않습니다.
- 단일 tab에서 첫 split이 즉시 표시됩니다.
- 마지막 tab close 후 stale ID가 남지 않습니다.
- multibyte terminal output이 깨지지 않습니다.

---

# Phase 3. Worktree 안전성 및 1-Writer-1-Worktree

대상:

- `src-tauri/src/worktree/manager.rs`
- `src-tauri/src/worktree/model.rs`
- `src-tauri/src/worktree/git.rs`
- `src-tauri/src/ipc/worktree.rs`
- terminal/worktree binding 경로

## 3.1 Writer lease registry

현재 dirty 삭제 차단은 있지만 writer ownership은 없습니다.

예:

```rust
struct WorktreeLeaseRegistry {
    writers: RwLock<HashMap<PathBuf, WriterLease>>,
}

struct WriterLease {
    owner_id: String,
    acquired_at: SystemTime,
}
```

key는 canonical worktree path를 사용합니다.

API:

```text
acquire_writer(path, owner_id)
release_writer(path, owner_id)
writer_owner(path)
```

동일 worktree에 다른 owner가 acquire하면 거부합니다.

## 3.2 PTY lifecycle과 lease 연결

Writable terminal/agent 생성:

1. worktree canonicalize
2. writer lease acquire
3. PTY spawn
4. session metadata에 owner/worktree 저장
5. PTY close/natural exit에서 lease release

spawn 실패 시 lease를 rollback합니다.

## 3.3 Delete serialization

현재 dirty check와 `git worktree remove` 사이에 다른 writer가 파일을 변경할 수 있습니다.

개선:

- active writer가 있으면 삭제 거부
- delete lock을 획득
- lock 안에서 dirty check → remove 수행

## 3.4 Branch deletion policy

현재 `delete_worktree_and_branch()`는 최종적으로 `git branch -D`를 사용할 수 있습니다.

기본 삭제는:

```text
git branch -d <branch>
```

로 변경합니다.

미병합 commit까지 버리는 동작은 별도 explicit destructive API로 분리합니다.

Force UI에는 최소한 다음 정보를 표시합니다.

- branch name
- HEAD commit
- upstream
- merge 여부 또는 ahead/behind

### Phase 3 완료 조건

- 동일 canonical worktree에 writer 2개가 존재할 수 없습니다.
- PTY 실패/종료 시 lease가 반드시 release됩니다.
- active writer가 있는 worktree 삭제가 거부됩니다.
- clean but unmerged branch가 일반 삭제로 손실되지 않습니다.

---

# Phase 4. Path 및 Git argument hardening

대상:

- `src-tauri/src/worktree/git.rs`
- `src-tauri/src/worktree/manager.rs`
- `src-tauri/src/ipc/worktree.rs`
- `src-tauri/src/ipc/terminal.rs`

## 4.1 Workspace registry

IPC가 raw `repoRoot`를 신뢰하는 대신:

```text
WorkspaceRegistry
  workspaceId -> canonical repo root
```

를 둡니다.

가능하면 worktree/terminal command는 `workspaceId`를 받고 backend가 root를 resolve합니다.

## 4.2 Allowed-root validation

모든 filesystem operation 전에:

1. registered root canonicalize
2. candidate path canonicalize
3. candidate가 allowed root 내부인지 확인
4. symlink escape 거부

아직 존재하지 않는 신규 worktree target은 parent를 canonicalize하고 마지막 component를 별도 검증합니다.

## 4.3 Git option injection hardening

현재 `.args()` 사용으로 shell injection은 이미 강하게 방어되어 있습니다.

추가:

- branch/ref/path leading `-` 검사 확대
- 지원되는 위치에서는 `--` option delimiter 사용
- `baseRef`는 `git check-ref-format`/`rev-parse --verify` 계열로 검증
- error log용 command 문자열의 control character escape

## 4.4 Repo root 검증

`WorktreeManager`는 사용 전에 다음을 보장합니다.

- path 존재
- directory
- Git repository
- canonical root 확보

### Phase 4 완료 조건

- IPC로 registered workspace 밖 Git/filesystem operation을 실행할 수 없습니다.
- symlink escape가 차단됩니다.
- leading-dash 값이 Git option으로 해석되지 않습니다.

---

# Phase 5. Tauri IPC 및 WebView 보안 경계

대상:

- `src-tauri/tauri.conf.json`
- `src-tauri/capabilities/default.json`
- `src-tauri/src/ipc/*`

## 5.1 CSP 활성화

현재 `csp: null`을 제거하고 production asset에 필요한 최소 CSP를 설정합니다.

기본 방향:

```text
default-src 'self'
외부 script 차단
불필요한 remote connect 차단
inline script 최소화
```

Tauri/WebGL/xterm 요구사항에 맞춰 실제 app에서 검증합니다.

## 5.2 Raw `/bin/sh -c` surface 축소

현재 terminal spawn request의 `command: Option<String>`은 WebView compromise 시 강한 shell execution primitive가 됩니다.

기본 interactive terminal은:

```text
spawn_terminal(workspaceId, worktreeId)
```

형태로 제한하고 shell executable은 backend가 결정합니다.

특정 command 실행 기능이 필요하면 raw string 대신:

```text
program + argv[]
```

형태와 allowlist/permission policy를 사용합니다.

## 5.3 Structured IPC errors

현재 `Result<T, String>` 위주의 error를 안정적인 DTO로 변경합니다.

```ts
{
  code: "DIRTY_WORKTREE",
  message: "...",
  details: {...}
}
```

Frontend는 error string parsing이 아니라 `code`로 분기합니다.

### Phase 5 완료 조건

- CSP가 활성화됩니다.
- 일반 UI IPC에 raw shell string execution surface가 없습니다.
- IPC가 registered workspace 범위를 벗어나지 않습니다.
- structured error code로 UI가 안정적으로 분기합니다.

---

# Phase 6. Worktree/Agent realtime binding

대상:

- `src-tauri/src/ipc/worktree.rs`
- `ui/src/App.tsx`
- `ui/src/components/Sidebar.tsx`
- `ui/src/components/WorktreeList.tsx`
- `ui/src/components/AgentCards.tsx`

## 6.1 Backend mutation events

다음 작업 성공 후 worktree 변경 event를 발생시킵니다.

- create
- remove
- dirty state change
- lock/prune
- branch change

## 6.2 External Git 변경

경량 초기 전략:

- app/window focus 시 refresh
- backend mutation 성공 시 즉시 event
- 필요할 때만 활성 window에서 2~5초 저빈도 polling

규모가 커지면 filesystem watcher + debounce를 검토합니다.

## 6.3 Hardcoded agent 제거

현재 샘플 agent 대신 backend session/agent metadata에서 실제 상태를 만듭니다.

예:

```text
starting
working
waiting
exited
failed
```

## 6.4 Single state source

`App.tsx`의 orchestration state를 context/reducer 또는 `useWorkspaceStore` 계층으로 분리합니다.

현재 규모에서는 무거운 외부 상태 라이브러리는 필수 사항이 아닙니다.

### Phase 6 완료 조건

- Orca 내부 worktree mutation이 Sidebar에 즉시 반영됩니다.
- 외부 Git 변경도 정의한 refresh policy 내에서 반영됩니다.
- hardcoded agent가 없습니다.
- 삭제된 worktree를 tab/agent가 계속 참조하지 않습니다.

---

# Phase 7. IPC serialization contract 통일

대상:

- Rust IPC DTO 전반
- `ui/src/lib/types.ts`
- `ui/src/lib/tauri.ts`

## 7.1 camelCase 통일

Rust DTO에 명시적으로 적용합니다.

```rust
#[serde(rename_all = "camelCase")]
```

대상:

- terminal request/response/event
- worktree request/response
- dirty state/file
- lifecycle state
- structured errors

Frontend는 `sessionId`, `isDirty`, `statusCode`, `wsId`, `baseRef`로 통일합니다.

## 7.2 IPC wrapper 단일화

`App.tsx` direct `invoke()`와 `ui/src/lib/tauri.ts` wrapper 혼용을 제거합니다.

UI component는 command name을 직접 알지 않고 wrapper만 사용합니다.

### Phase 7 완료 조건

- UI component에 직접 Tauri command string이 남지 않습니다.
- Rust/TS DTO field가 일치합니다.
- snake_case compatibility shim이 없습니다.

---

# Phase 8. 성능 및 초경량 목표

## 8.1 Frontend bundle

현재 production build에서 약 650 KiB minified JS chunk warning이 있습니다.

분석 대상:

- xterm
- WebGL addon
- lucide imports
- unused UI code

권장:

- terminal bundle lazy load
- 필요한 경우 manual chunk 분리
- unused dependency/component 제거

Bundle size뿐 아니라 idle memory와 process count도 함께 측정합니다.

## 8.2 WebGL resource lifecycle

Phase 2 이후 다음을 측정합니다.

- tab/split 수 증가에 따른 WebGL context 수
- hidden tab의 renderer resource
- context loss 후 Canvas fallback

## 8.3 Blocking Git I/O

현재 async Tauri command 안의 `std::process::Command::output()`은 동기 I/O입니다.

대규모 repository 또는 느린 Git operation이 늘어나면:

- `spawn_blocking`
- dedicated Git worker

중 하나로 이동합니다.

### Phase 8 완료 조건

- bundle warning을 제거하거나 의도된 threshold를 문서화합니다.
- tab/split 증가와 PTY/xterm/WebGL resource 수가 예측 가능합니다.
- Git operation이 UI/runtime responsiveness를 장시간 막지 않습니다.

---

## 3. 권장 구현 순서

```text
Phase 0  Regression tests
   ↓
Phase 1  PTY lifecycle
   ↓
Phase 2  Output transport + split/session identity
   ↓
Phase 3  Writer lease + branch data protection
   ↓
Phase 4  Path/Git hardening
   ↓
Phase 5  Tauri/WebView security boundary
   ↓
Phase 6  Realtime worktree/agent binding
   ↓
Phase 7  IPC contract cleanup
   ↓
Phase 8  Performance/bundle cleanup
```

**Phase 1~5는 release blocker 성격으로 취급합니다.** Phase 6~8은 core invariant가 안정화된 후 진행합니다.

---

## 4. 최종 architecture 목표

### Backend

```text
Tauri IPC
  │
  ├─ TerminalService
  │    ├─ PtyManager
  │    ├─ SessionRegistry
  │    ├─ lifecycle watcher
  │    └─ byte output channel
  │
  ├─ WorkspaceService
  │    ├─ WorkspaceRegistry
  │    ├─ WorktreeManager
  │    ├─ WriterLeaseRegistry
  │    └─ path security policy
  │
  └─ Event/State Bridge
       ├─ worktree events
       ├─ terminal lifecycle
       └─ agent lifecycle
```

### Frontend

```text
Workspace Store / Reducer
  │
  ├─ worktrees
  ├─ agents
  ├─ terminal sessions
  ├─ tabs
  └─ pane layout

TerminalSession 1 ───── PTY 1
       │
       └─ referenced by Tab/Pane
```

가장 중요한 설계 원칙은 **React component mount/unmount가 backend process ownership을 직접 결정하지 않도록 하는 것**입니다.

---

## 5. Release acceptance checklist

### Terminal

- [ ] tab 하나당 backend PTY가 정확히 하나입니다.
- [ ] split/unsplit으로 session이 재생성되지 않습니다.
- [ ] terminal 자연 종료 시 registry에서 제거됩니다.
- [ ] explicit close 후 child/orphan process가 남지 않습니다.
- [ ] reader task가 정상 종료됩니다.
- [ ] Ctrl-C가 foreground job에 정상 전달됩니다.
- [ ] 한글/일본어/emoji output이 chunk boundary에서 깨지지 않습니다.
- [ ] spawn 직후 shell prompt가 유실되지 않습니다.

### Worktree

- [ ] branch namespace가 `orca/<ws>/<slug>`를 유지합니다.
- [ ] dirty worktree 삭제가 항상 거부됩니다.
- [ ] active writer가 있는 worktree 삭제가 거부됩니다.
- [ ] 동일 worktree에 writer가 2개 생기지 않습니다.
- [ ] clean but unmerged branch가 일반 삭제로 손실되지 않습니다.
- [ ] force branch delete는 별도 destructive action입니다.
- [ ] allowed workspace 밖 path 접근이 차단됩니다.
- [ ] symlink escape가 차단됩니다.

### IPC / Security

- [ ] CSP가 활성화되어 있습니다.
- [ ] 일반 UI IPC에 raw `/bin/sh -c` surface가 없습니다.
- [ ] raw `repoRoot`/arbitrary path override를 신뢰하지 않습니다.
- [ ] Rust/TS field naming이 camelCase로 통일됩니다.
- [ ] structured IPC error code가 있습니다.

### React UI

- [ ] single-tab split이 정상 작동합니다.
- [ ] split 전환으로 shell state가 초기화되지 않습니다.
- [ ] 마지막 tab close 이후 valid state가 유지됩니다.
- [ ] WorktreeList가 backend 상태와 자동 동기화됩니다.
- [ ] hardcoded agent 샘플이 제거됩니다.

### Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] frontend unit/component tests
- [ ] `bun run --cwd ui build`
- [ ] macOS 실제 Tauri PTY interactive smoke test
- [ ] terminal 10개 이상 반복 open/close leak test
- [ ] worktree 5개 이상 동시 lifecycle E2E
- [ ] dirty/unmerged/path-escape destructive-operation security test

---

## 6. Definition of Done

전체 수정은 단순히 build가 통과하는 것으로 완료로 보지 않습니다.

1. **Terminal ownership invariant**가 코드와 테스트에서 보장됩니다.
2. **1-Writer-1-Worktree invariant**가 실제 lease/lock으로 강제됩니다.
3. clean/unmerged branch와 dirty worktree 모두 데이터 손실 방어가 존재합니다.
4. PTY 종료가 child/reaper/reader/registry까지 완결적으로 처리됩니다.
5. Tauri IPC의 arbitrary path 및 arbitrary shell execution 권한 면적이 최소화됩니다.
6. Split UI가 backend session identity와 분리됩니다.
7. Worktree/agent 상태가 backend와 동기화됩니다.
8. Rust tests, frontend tests, clippy, production build가 모두 통과합니다.
9. 실제 macOS Tauri runtime에서 terminal open/resize/split/close 및 worktree create/delete가 정상입니다.
10. 반복 open/close 및 worktree lifecycle에서 process/session/lease 누수가 없습니다.

이 기준을 만족하면 `orca-lite`는 기능 prototype을 넘어 후속 AI agent orchestration을 올릴 수 있는 안정적인 경량 runtime 기반으로 볼 수 있습니다.
