# Ferryx 터미널 세션 유지의 근본 해결 설계

작성일: 2026-08-23  
관련 원인 분석: [`SESSION_PERSISTENCE_ROOT_CAUSE.md`](../SESSION_PERSISTENCE_ROOT_CAUSE.md)

## 1. 결론

근본 해결책은 **PTY, 실행 중인 셸/에이전트 프로세스, 출력 버퍼의 소유권을 Tauri GUI 프로세스에서 독립 Ferryx 데몬으로 이전하는 것**이다.

최종 구조에서는 다음 원칙을 지켜야 한다.

1. **데몬이 유일한 터미널 세션 소유자다.**
2. **Tauri 앱과 React UI는 세션의 클라이언트일 뿐이다.**
3. **모든 화면 복구는 spawn이 아니라 list -> reconcile -> attach -> replay 순서로 수행한다.**
4. **새 셸 생성은 기존 데몬 세션이 없을 때만 허용한다.**
5. **레이아웃 저장과 프로세스 생존은 별개의 책임으로 관리한다.**

이 구조를 완성하면 아래 동작이 가능해진다.

- Vite HMR 후에도 같은 셸과 실행 중인 프로그램 유지
- webview 전체 reload 후 같은 세션에 재연결하고 누락 출력 replay
- Tauri 앱 종료, 충돌, 개발 백엔드 rebuild 후에도 셸/에이전트 PID 유지
- 모바일 remote와 데스크톱 UI가 같은 터미널 세션을 동시 구독
- 탭을 닫을 때만 명시적으로 해당 프로세스 그룹 종료

## 2. 왜 자체 Rust 데몬이어야 하는가

### 선택지 비교

| 항목 | Ferryx Rust 데몬 | tmux | dtach |
| --- | --- | --- | --- |
| 앱 재시작 후 프로세스 생존 | 가능 | 가능 | 가능 |
| 외부 설치 의존성 | 없음 | 필요 | 필요 또는 번들 필요 |
| 현재 `TerminalService` 재사용 | 직접 가능 | 별도 어댑터 필요 | 별도 어댑터 필요 |
| 바이트 단위 출력 replay | 현재 `TerminalOutputHub` 활용 | tmux history 변환 필요 | 기본 제공 안 함 |
| OSC/title/CWD/마우스 시퀀스 | 그대로 전달 | tmux escape 처리 필요 | 그대로 전달 |
| 모바일 remote 통합 | 같은 데몬에서 처리 가능 | 별도 bridge 필요 | 별도 bridge 필요 |
| 세션 메타데이터와 권한 제어 | Ferryx 도메인으로 표현 가능 | tmux 모델에 종속 | 거의 없음 |
| 장기 유지보수 | 기존 Rust 코드와 통합 | 외부 버전과 설정 영향 | 기능을 자체 보완해야 함 |

`src-tauri/src/daemon/`에는 이미 다음 기반이 존재한다.

- UDS 서버와 단일 인스턴스 lock: `src-tauri/src/daemon/server.rs`
- typed request/response 및 stream 메시지: `src-tauri/src/daemon/protocol.rs`
- reconnect 가능한 클라이언트: `src-tauri/src/daemon/client.rs`
- macOS launchd 지원 초안: `src-tauri/src/daemon/launchd.rs`
- PTY 및 출력 ring buffer: `src-tauri/src/terminal/pty.rs`, `src-tauri/src/terminal/output_hub.rs`
- history + live receiver attach: `src-tauri/src/terminal/service.rs:66`

따라서 tmux나 dtach를 새로 도입하기보다 기존 데몬을 제품의 실제 런타임으로 승격하는 것이 가장 작은 근본 변경이다.

## 3. 목표 아키텍처

```text
┌──────────────────────────────────────────────────────────────┐
│ React / xterm.js                                             │
│                                                              │
│ Workspace layout ─ TerminalHostManager ─ TerminalEventBus    │
└──────────────────────────┬───────────────────────────────────┘
                           │ typed Tauri commands/events
┌──────────────────────────▼───────────────────────────────────┐
│ Tauri GUI process                                             │
│                                                              │
│ menus / windows / dialogs / DaemonClient                     │
│ PTY 소유 금지                                                  │
└──────────────────────────┬───────────────────────────────────┘
                           │ Unix domain socket
┌──────────────────────────▼───────────────────────────────────┐
│ Ferryx daemon                                                 │
│                                                              │
│ SessionRegistry                                               │
│   ├─ TerminalService                                          │
│   │   ├─ PtyManager            -> shell/agent process groups │
│   │   └─ TerminalOutputHub     -> replay + live broadcast    │
│   ├─ WorkspaceRegistry                                      │
│   └─ RemoteGateway                                          │
└──────────────────────────────────────────────────────────────┘
```

### 컴포넌트별 책임

#### Ferryx 데몬

- PTY master와 child process handle 소유
- 세션 ID 생성 및 생명주기 관리
- 입력, resize, signal, close 수행
- 세션별 출력 history와 live stream 제공
- 세션 목록 및 메타데이터 제공
- GUI 클라이언트가 없어도 계속 실행
- 명시적 close 시 전체 프로세스 그룹 종료

#### Tauri GUI

- `DaemonClient`를 통해 데몬 명령 중계
- 데몬 연결 끊김을 UI에 구조화된 상태로 노출
- 데몬 자동 시작 및 버전 handshake 수행
- PTY를 직접 생성하거나 소유하지 않음
- GUI 종료 시 데몬과 터미널을 종료하지 않음

#### React UI

- 탭, pane tree, 활성 항목 같은 표현 상태 관리
- persisted backend session ID와 데몬 목록 reconcile
- 기존 세션은 attach, 없는 세션만 spawn
- attach history를 xterm에 먼저 적용한 뒤 live output 구독
- HMR registry는 빠른 UI 복구 최적화로만 사용

## 4. 반드시 유지해야 할 ID 모델

세 가지 ID를 하나로 합치면 안 된다.

| ID | 소유자 | 수명 | 용도 |
| --- | --- | --- | --- |
| `workspaceId` | 앱 도메인 | 영구 | 프로젝트/저장소 구분 |
| `localSessionId` | UI 레이아웃 | 레이아웃 수명 | pane/tab이 참조하는 안정 ID |
| `backendSessionId` | 데몬 | 프로세스 수명 | 실제 PTY 세션 식별 |

복구 시 규칙은 다음과 같다.

```text
persisted localSessionId + backendSessionId
                   │
                   ▼
        daemon ListSessions에 존재?
             │             │
            Yes            No
             │             │
          Attach       Spawn replacement
             │             │
      ID 그대로 유지   localSessionId 유지,
                      backendSessionId만 교체
```

레이아웃의 leaf는 `localSessionId`를 참조해야 하며, 데몬 재시작이나 해당 세션의 자연 종료가 발생해도 pane tree 자체는 유지되어야 한다.

## 5. 데몬 프로토콜

현재 `src-tauri/src/daemon/protocol.rs`의 v1 골격을 기반으로 하되, 실제 제품 전환 전 protocol version을 올리고 다음 계약을 확정한다.

### Control 명령

```text
Handshake { protocolVersion, clientVersion }
Ping
ListSessions
DescribeSession { sessionId }
Spawn { workspaceId, worktree, cwd, cols, rows, clientRequestId }
Attach { sessionId, afterSequence? }
Detach { attachmentId }
Write { sessionId, data }
Resize { sessionId, cols, rows }
Signal { sessionId, signal }
Close { sessionId }
SaveWorkspaceState { session }
LoadWorkspaceState
```

### Stream 메시지

```text
Attached {
  attachmentId,
  sessionId,
  historyStartSequence,
  historyEndSequence,
  history
}

Output {
  sessionId,
  sequence,
  data
}

Lifecycle {
  sessionId,
  state,
  exitCode?,
  reason?
}

ReplayGap {
  sessionId,
  requestedAfterSequence,
  availableFromSequence,
  history
}
```

### 왜 sequence number가 필요한가

현재 `AttachOk { history }` 뒤에 live broadcast를 연결하는 구조는 history snapshot과 live output 경계에서 중복 또는 누락 여부를 클라이언트가 판별할 수 없다. 세션별 단조 증가 `u64 sequence`를 추가해야 다음이 보장된다.

- attach 중 발생한 출력 누락 방지
- reconnect 후 마지막 수신 지점부터 replay
- history와 live stream 중복 제거
- broadcast lag 발생 시 명시적 gap 복구

`OutputHub`의 ring buffer는 단순 `Vec<u8>` snapshot이 아니라 sequence 범위를 가진 chunk deque를 보관해야 한다.

```rust
struct OutputChunk {
    sequence: u64,
    bytes: Vec<u8>,
}
```

### spawn idempotency

GUI reconnect 또는 IPC timeout 뒤 `Spawn`을 재시도할 때 셸이 중복 생성되면 안 된다. `clientRequestId`를 필수로 두고 데몬이 최근 spawn 결과를 짧은 TTL 동안 기억해야 한다.

```text
같은 clientRequestId -> 같은 backendSessionId
```

이 보장이 없으면 네트워크/UDS 응답 손실이 곧 중복 에이전트 실행으로 이어질 수 있다.

## 6. attach 및 replay 순서

올바른 cold attach는 다음 순서를 반드시 따른다.

1. UI가 데몬의 `ListSessions`를 가져온다.
2. persisted `backendSessionId`와 live session을 대조한다.
3. xterm renderer를 만들되 아직 live output을 표시하지 않는다.
4. `Attach { sessionId, afterSequence }`를 보낸다.
5. 데몬은 subscriber를 먼저 등록한다.
6. 같은 임계영역에서 history snapshot과 sequence 경계를 만든다.
7. `Attached` history를 반환한다.
8. UI가 history를 xterm에 기록한다.
9. 이후 `Output.sequence > historyEndSequence`인 live chunk만 적용한다.
10. attach 완료 후 입력을 활성화한다.

이 순서가 지켜져야 reload 중 출력이 빠지거나 두 번 표시되지 않는다.

`ui/src/lib/terminalTransport/types.ts`의 `TerminalAttachment.initialHistory`는 유지하되 다음처럼 확장한다.

```ts
type TerminalAttachment = {
  readonly attachmentId: string;
  readonly sessionId: string;
  readonly initialHistory: Uint8Array;
  readonly historyStartSequence: bigint;
  readonly historyEndSequence: bigint;
};
```

## 7. 저장 책임

### 데몬 메모리에만 존재하는 것

- PTY file descriptor
- child/process-group handle
- stdin writer
- reader task
- output ring buffer
- active attachments
- lifecycle state

### 디스크에 저장하는 것

- workspace/project 목록
- tab 및 pane tree
- `localSessionId -> backendSessionId` 매핑
- CWD 및 worktree identity
- 활성 tab/leaf/group
- 사용자 지정 tab title, pinned 상태
- 마지막 확인 output sequence

### 디스크에 저장해도 복구할 수 없는 것

- 프로세스 메모리
- 셸 내부의 export되지 않은 실행 상태
- REPL/에디터 내부 상태

이 항목은 데몬 프로세스가 살아 있어야만 유지된다. 따라서 layout JSON을 더 많이 저장하는 것으로는 앱 재시작 세션 보존을 해결할 수 없다.

## 8. 데몬 생명주기

### 앱 시작

1. 기존 socket에 연결한다.
2. handshake가 성공하면 기존 데몬을 사용한다.
3. socket은 있으나 연결할 수 없으면 lock 상태를 확인한다.
4. lock이 없는 stale socket만 제거한다.
5. 데몬이 없으면 설치된 Ferryx binary를 `--daemon` 모드로 실행한다.
6. bounded readiness timeout 내 다시 연결한다.

현재 `DaemonClient::connect_or_spawn`의 in-process fallback은 테스트와 한시적 전환용으로만 남기고, 세션 지속 기능이 활성화된 production 경로에서는 금지해야 한다. in-process fallback으로 생성된 PTY는 GUI 종료와 함께 사라지기 때문이다.

### 앱 종료

- 일반 `Cmd+Q`: GUI만 종료하고 데몬은 유지
- 명시적 `Quit and Terminate Sessions`: 모든 세션 close 후 데몬 종료
- 시스템 로그아웃/종료: OS가 데몬과 child process를 정리

### 데몬 자체 충돌

데몬이 충돌하면 이미 열린 PTY master도 사라지므로 해당 프로세스 상태를 완전 복구할 수 없다. launchd `KeepAlive`는 새 데몬을 시작할 수 있지만 과거 PTY를 되살릴 수는 없다.

따라서 UI는 데몬 epoch를 저장하고 다음을 구별해야 한다.

```text
same daemon epoch + session exists -> attach
new daemon epoch or session missing -> mark lost, user 선택 후 respawn
```

자동 respawn은 실행 중이던 명령을 다시 실행하지 않는다. 새 셸을 같은 CWD에서 여는 것만 허용한다.

## 9. 보안과 권한

- socket directory: 사용자 전용, mode `0700`
- socket file: mode `0600`
- symlink socket 거부
- socket/lock owner UID 검증
- protocol version 불일치 시 즉시 거부
- workspace path는 데몬의 `WorkspaceRegistry`로 canonicalize
- UI가 전달한 임의 명령 경로나 CWD를 신뢰하지 않음
- remote gateway는 동일 `TerminalService`를 사용하되 인증 후 접근
- 로그에 terminal input/output, pairing token, 환경 변수 기록 금지

현재 `/tmp/rorca-{uid}` 및 `com.rorca.daemon` 같은 내부 식별자는 호환성 마이그레이션을 별도 승인받기 전까지 유지할 수 있다. 사용자에게 표시되는 문구는 Ferryx만 사용한다.

## 10. 단계별 구현 순서

각 단계는 독립적으로 검증 가능해야 하며, 다음 단계로 넘어가기 전에 해당 gate를 통과해야 한다.

### 단계 A: cold restore와 HMR 정확성

대상:

- `ui/src/App.tsx`
- `ui/src/state/hmrWorkspaceState.ts`
- `ui/src/state/workspaceStore.ts`

작업:

1. StrictMode restore guard를 `idle/loading/restored/failed` 상태로 변경한다.
2. effect cleanup은 현재 attempt만 취소하며 다음 StrictMode attempt를 막지 않게 한다.
3. HMR registry는 workspace별 상태를 유지한다.
4. disk restore와 HMR restore가 동시에 실행되지 않게 단일 restore coordinator로 통합한다.

Gate:

- StrictMode에서 persisted restore 정확히 1회 완료
- HMR에서 spawn 0회, close 0회
- workspace 전환 후 각 workspace 상태 독립 유지

### 단계 B: 현재 Tauri backend에 attach IPC 추가

대상:

- `src-tauri/src/ipc/terminal.rs`
- `src-tauri/src/lib.rs`
- `ui/src/lib/tauri.ts`
- `ui/src/lib/terminalTransport/tauriTransport.ts`
- `ui/src/lib/terminalHostManager.ts`

작업:

1. `cmd_terminal_attach`를 추가한다.
2. `TerminalService::attach`의 history를 typed response로 반환한다.
3. renderer 생성 시 history를 먼저 기록하고 live output을 구독한다.
4. full webview reload 뒤 기존 backend session에 재연결한다.

Gate:

- webview reload 전후 shell PID 동일
- reload 중 출력 누락/중복 없음
- 기존 scrollback이 지정된 ring-buffer 범위 내 복구

### 단계 C: 데몬 프로토콜 v2 완성

대상:

- `src-tauri/src/daemon/protocol.rs`
- `src-tauri/src/daemon/server.rs`
- `src-tauri/src/daemon/client.rs`
- `src-tauri/src/terminal/output_hub.rs`

작업:

1. sequence 기반 replay 계약 추가
2. attach 전용 streaming connection 또는 request ID multiplexing 구현
3. `DescribeSession`, daemon epoch, lifecycle stream 추가
4. idempotent spawn 추가
5. control connection과 stream connection을 분리한다.

현재 서버의 `Attach`는 연결을 stream 전용으로 점유한다. 일반 control request와 output stream을 같은 connection/reader에서 동시에 다루지 말고 다음 중 하나를 택한다.

- 권장: control UDS + attachment별 stream UDS 분리
- 대안: 모든 frame에 request/attachment ID를 넣는 단일 multiplexed protocol

Gate:

- 동시에 여러 pane attach 가능
- attach 중 write/resize/close 가능
- lag 후 replay gap 복구 가능
- 동일 request ID spawn 재시도 시 세션 하나만 생성

### 단계 D: Tauri terminal 명령을 데몬으로 전환

대상:

- `src-tauri/src/lib.rs`
- `src-tauri/src/ipc/terminal.rs`
- `src-tauri/src/ipc/session.rs`

작업:

1. Tauri state의 직접 `PtyManager`/`TerminalService`를 제거한다.
2. `DaemonClient`를 managed state로 설치한다.
3. spawn/write/resize/signal/close/list/attach를 전부 데몬으로 route한다.
4. remote gateway도 데몬 소유 terminal service를 사용하게 한다.
5. 전환 기간에는 feature flag로 direct/daemon transport를 선택한다.

Gate:

- daemon mode에서 Tauri 프로세스의 direct child로 터미널 shell이 존재하지 않음
- 모든 shell의 ancestor가 daemon PID
- GUI 강제 종료 후 shell PID 유지

### 단계 E: 독립 데몬 실행과 배포

대상:

- `src-tauri/src/main.rs`
- `src-tauri/src/daemon/launchd.rs`
- Tauri bundle 설정

작업:

1. `ferryx --daemon` headless entrypoint 추가
2. macOS launchd bootstrap/bootout 구현
3. Linux user service 또는 안전한 detached process 실행 추가
4. binary 업그레이드 시 protocol compatibility 정책 추가
5. 앱 uninstall 시 daemon/LaunchAgent 정리 절차 제공

Gate:

- GUI를 세 번 재시작해도 동일 daemon PID와 shell PID 유지
- 앱 업데이트 후 호환 protocol이면 attach 유지
- 비호환 protocol이면 사용자에게 명시적 전환 안내 후 안전 종료

### 단계 F: direct backend 제거

모든 gate가 통과한 뒤에만 direct PTY 경로와 자동 dead-session respawn을 제거한다.

`App.tsx`의 현재 dead session respawn은 마지막 복구 수단으로 남겨두더라도 자동으로 실행하지 않는 편이 안전하다. 데몬 세션 손실은 실행 중인 작업 손실을 의미하므로 사용자가 확인한 뒤 새 셸을 열어야 한다.

## 11. 테스트 전략

### Rust 단위 테스트

- output sequence 단조 증가
- ring overflow 시 정확한 available range
- attach snapshot과 live stream 사이 중복/누락 없음
- duplicate `clientRequestId` spawn idempotency
- close 시 child process group 전체 종료
- protocol version 및 UID 검증

### Rust 통합 테스트

기존 `src-tauri/tests/daemon_persistence_contract.rs`를 확장한다.

1. daemon process를 별도 PID로 시작
2. shell spawn 후 PID 기록
3. 첫 client disconnect
4. 두 번째 client attach
5. history와 live output 검증
6. GUI 역할의 client process를 SIGKILL
7. shell PID 생존 검증
8. 재연결 후 입력/resize 검증
9. close 후 shell/process group 종료 검증

고정 sleep은 사용하지 않는다. daemon readiness, output sequence, process exit 이벤트를 구독하고 bounded timeout으로 기다린다.

### TypeScript 테스트

- restore coordinator StrictMode double-effect
- live backend ID는 attach, missing ID만 replacement 후보
- initial history가 live output보다 먼저 renderer에 기록됨
- sequence 중복 제거
- HMR remount 시 spawn/close 0회
- workspace별 HMR state 격리

### 실제 E2E

#### HMR

1. 터미널에서 지속 출력 프로그램 실행
2. React component 수정으로 실제 Vite HMR 발생
3. shell PID 동일 확인
4. 출력 sequence 연속성 확인

#### full reload

1. 터미널에서 counter 실행
2. webview reload
3. 기존 PID 유지 확인
4. reload 구간 출력이 replay되고 이후 live 출력이 이어지는지 확인

#### GUI app restart

1. 여러 tab/split에서 서로 다른 장기 작업 실행
2. shell/agent PID 기록
3. GUI 프로세스 SIGKILL
4. daemon 및 shell PID 생존 확인
5. GUI 재실행
6. 레이아웃, CWD, scrollback, 입력 반응 확인

데스크톱 UI 조작이 필요한 최종 E2E는 사용자가 직접 수행할 수 있도록 정확한 체크리스트를 제공해야 한다.

## 12. 완료 기준

다음 조건을 모두 만족해야 "세션 유지가 근본적으로 해결됐다"고 판단한다.

### HMR

- 열린 tab/pane 수와 배치가 동일
- backend session ID 및 shell PID 동일
- running `vim`, `top`, agent CLI 중단 없음
- HMR 중 terminal close/spawn 호출 0회

### full webview reload

- 같은 daemon 세션에 attach
- reload 중 출력 누락/중복 0
- ring buffer 범위의 scrollback 복구
- reload 후 즉시 입력 가능

### 앱 재시작/GUI crash

- daemon PID 유지
- shell 및 agent PID 유지
- 기존 process state 유지
- 레이아웃과 각 pane의 세션 매핑 유지
- 저장된 CWD와 실제 세션 CWD 일치

### 세션 종료

- tab/pane close 후 해당 shell과 descendants 종료
- 다른 pane 세션에는 영향 없음
- zombie 및 orphan process 없음

### 장애 처리

- daemon 없음: 자동 시작 후 연결
- stale socket: 안전하게 판별한 경우에만 제거
- protocol 불일치: 조용한 fallback 금지, 사용자에게 명확한 오류
- daemon crash: 기존 세션을 살아 있는 것처럼 표시하지 않음
- session missing: 자동 명령 재실행 금지

## 13. 구현 시 피해야 할 잘못된 해결

다음 방법들은 근본 해결이 아니다.

- `session_state.json`에 더 많은 UI 필드만 저장
- 앱 시작 시 모든 terminal을 무조건 respawn
- StrictMode 비활성화로 경쟁 조건을 숨김
- HMR 때 xterm DOM만 유지하고 backend identity를 검증하지 않음
- Tauri backend 안에서 background task만 분리
- `nohup`으로 shell만 분리하고 PTY master 소유권은 GUI에 남김
- 출력 attach 없이 session ID만 재사용
- 실패 시 direct in-process PTY로 조용히 fallback

특히 direct fallback은 기능이 동작하는 것처럼 보이지만 다음 앱 종료 시 다시 세션을 잃게 만든다. daemon mode가 활성화된 환경에서 daemon 연결 실패는 사용자에게 명시적으로 보여야 한다.

## 14. 권장 최종 상태

완료 후 코드의 핵심 불변식은 다음 한 문장으로 표현되어야 한다.

> Ferryx 터미널 프로세스는 GUI가 아니라 사용자 세션 단위 데몬에 속하며, 모든 UI는 안정적인 backend session ID와 sequence 기반 attach/replay를 통해 그 세션을 관찰하고 제어한다.

이 불변식이 코드와 E2E 테스트로 강제될 때 앱 재시작, webview reload, HMR의 차이는 UI 복원 경로의 차이일 뿐 터미널 프로세스 생존 여부에는 영향을 주지 않는다.
