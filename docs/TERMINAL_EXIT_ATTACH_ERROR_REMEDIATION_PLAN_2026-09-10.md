# Terminal Exit / Native Attach Error 개선 검토 및 구현계획서

- 작성일: 2026-09-10
- 검토 기준: `5d2ab1bd2181545eb82c0a41af21f83023af835c`의 작업 트리
- 대상: PTY 세션 종료 후 나타나는 `Failed to attach native terminal` 경고 및 복구할 수 없는 클릭 동작
- 산출물 범위: **현행 구현 검토와 구현 계획만 작성한다. 이 문서 작성 작업은 런타임 코드나 테스트 코드를 수정하지 않는다.**
- 용어: 아래의 **현행**은 직접 확인한 코드이고, **권장/제안**은 후속 구현 사항이다. 새 API·타입·테스트 이름은 명시적으로 제안으로 표시한다.

## 1. 검토 결론

기존 수정 방향인 “PTY의 생명주기와 native surface의 생명주기를 분리하고, 종료를 정상적인 pane 상태로 보여준다”는 타당하다. 다만 `isExited` 조건을 한 번 더 추가하거나 `SESSION_NOT_FOUND`를 일괄 무시하는 수정만으로는 남은 문제가 해결되지 않는다.

핵심 발견은 다음과 같다.

1. **2026-09-07 분석의 일부 결함은 이미 수정되었다.** `TerminalPane`의 로컬 종료 판별과 `NativeTerminalPane`의 attach 대상 차단뿐 아니라, 현행 `SESSION_LIFECYCLE` 리듀서도 로컬 종료 시 `backendSessionId`를 `null`로 바꾼다. 이를 미구현 사항으로 다시 제안하면 안 된다. SSH는 별도 분기다. [S1, S2, S3]
2. **실제 daemon attach 경로에서는 오류 코드가 소실된다.** 데몬은 `Error { message }`를 보내고, `DaemonClient::attach`는 이를 `INTERNAL_ERROR`로 만든다. 따라서 현재 증상의 대표 응답은 `{ code: "INTERNAL_ERROR", message: "Session '<id>' not found" }`다. `PtyError -> IpcError`에 `SESSION_NOT_FOUND` 매핑이 있다는 사실만 보고 frontend 입력도 그 코드라고 가정하면 안 된다. [S5, S6, S7]
3. **macOS 프레임 보존 때문에 effect가 계속 살아 있는 종료 레이스가 남는다.** `attemptAttach`는 시작할 때만 `attachmentOwnerRef.current?.live`를 검사한다. await 이후 catch는 `isSubscribed`만 확인한다. 표시된 macOS frame을 보존하면 종료 시 surface effect가 반드시 cleanup되는 것은 아니므로, 이미 종료된 pane에 늦은 실패가 배지를 다시 설정할 수 있다. [S1, S4]
4. **배지 억제와 store 상태 수렴을 함께 해야 한다.** 종료 이벤트보다 attach 거절이 먼저 오거나 이벤트가 유실되면 컴포넌트만 조용히 return해서는 `Shell exited` 오버레이와 실제로 동작하는 새 쉘 버튼이 보장되지 않는다. 확인된 backend 부재를 현재 binding에 한정해 store에 반영해야 한다. [S1, S3]
5. **기존 세션 목록은 부재 판정의 안전한 근거가 아니다.** `cmd_terminal_list`는 세션 ID 목록을 받은 뒤 `describe_session`이 성공한 항목만 반환한다. 상세 조회 실패로 목록에서 빠진 살아 있는 세션을 종료 처리하면 안 된다. [S10]

**권장안:** attach 응답 경계에서 의미를 보존하고, frontend의 commit-scoped 소유권 검증과 단일 재시도를 강화하며, 확인된 로컬 세션 부재를 compare-and-set 방식으로 store의 종료 전이에 연결한다. macOS의 마지막 frame과 Windows/Linux의 surface yielding은 그대로 유지한다. 전역 오류 숨김이나 daemon protocol 전체 변경은 채택하지 않는다.

## 2. 문제 정의와 수용 기준

### 2.1 무엇을 종료로 판단하는가

대상은 쉘 또는 PTY 세션을 소유하는 프로세스가 종료되어 backend 세션 자체가 더 이상 attach 가능하지 않은 경우다. **대화형 쉘 내부의 자식 프로세스만 SIGKILL/crash되고 쉘이 계속 실행되는 경우는 terminal session 종료가 아니다.** 이 경우 쉘 프롬프트와 입력을 유지해야 한다.

또한 daemon 연결 EOF, timeout, SSH transport 단절은 그 자체로 PTY 종료의 증거가 아니다. 정상 종료든 비정상 종료든 세션의 부재가 확인되면 종료 UI로 수렴하되, 원인을 확인하지 않고 exit code를 `0` 또는 `137`로 만들어내지 않는다.

### 2.2 사용자에게 보일 결과

| 상황 | 목표 UI와 실제 동작 |
| --- | --- |
| 일반 로컬 shell 세션 종료 | `Shell exited` + `Open new shell`. 클릭은 새 backend를 생성하고 기존 로컬 pane을 새 backend에 연결한다. |
| 로컬 agent 세션 종료 | `Session disconnected` + 기존 provider resume 가능 여부에 따른 `Reconnect` / `Retry` 또는 이유 설명을 보존한다. |
| SSH transport 일시 단절 | 기존 `Reconnecting SSH...` / `SSH disconnected` 및 `Reconnect SSH` 정책을 보존한다. 로컬 shell 종료로 변환하지 않는다. |
| SSH remote 세션의 실제 소멸 | remote status의 `missing` / `expired` / `legacyLost` 의미에 맞는 기존 UI를 유지한다. |
| 살아 있는 현재 pane의 실제 surface/IPC 오류 | 오류 보고와 유효한 복구 동작을 유지한다. 세션 종료로 위장하지 않는다. |

`Open new shell`은 현행 일반 로컬 shell 교체 경로에 대한 필수 수용 기준이다. 현행 `replaceExitedShellSession`은 agent와 SSH를 명시적으로 거부한다. agent에도 동일한 버튼을 제공하는 UX 확장은 이 함수를 그대로 연결하지 말고, **종료된 agent의 resume 기록을 보존하면서 별도의 일반 shell pane을 생성하는 전용 액션**으로 설계해야 한다. 본 개선의 기본 범위에서는 기존 agent 재연결 계약을 바꾸지 않는다. [S2, S11]

### 2.3 불변 조건

- 현재 binding의 종료가 store에 반영된 다음 React commit에서는 종료 overlay가 표시되어야 한다. 재시도 예산이나 임의의 grace delay가 끝날 때까지 기다리지 않는다.
- 명확한 attach 부재 응답이 먼저 도착하면 그 응답 처리에서 재시도·입력을 차단하고 store 전이를 요청한다. 이후에는 종료 이벤트가 없어도 overlay로 수렴한다.
- 오래된 owner의 success/catch/timer는 새 pane의 error, focus, attach, lifecycle에 영향을 주지 않는다. 같은 문자열 ID로 돌아오는 A→B→A도 포함한다.
- 확인된 종료는 error severity의 native attach 실패나 클릭 가능한 retry 배지가 아니다. 진단을 위한 저심각도 기록은 유지할 수 있다.
- macOS의 `presentation`은 과거 표시된 frame의 정체성이고 `backendSessionId`는 현재 live PTY binding이다. 둘을 동일한 상태로 취급하지 않는다.
- “즉시”는 프로세스가 죽은 물리적 순간을 frontend가 예지한다는 의미가 아니다. 최초의 신뢰 가능한 종료/부재 관측 이후 별도의 backoff 없이 UI가 전이한다는 의미다.

## 3. 현행 아키텍처와 코드 근거

아래 줄 범위는 검토 기준 리비전의 탐색 기준이다. 후속 수정 시 줄 번호보다 함께 적은 심볼을 우선한다.

| 근거 | 파일 / 위치 | 확인한 내용 |
| --- | --- | --- |
| S1 | `ui/src/components/NativeTerminalPane.tsx:500-558, 748-789, 850-900, 1690-1985, 2320-2385` | live target / retained presentation / attachment·input owner 분리, performAttach, 입력 복구, attach 재시도와 공통 error 배지 |
| S2 | `ui/src/components/TerminalPane.tsx:81-147, 175-330` | 로컬/SSH 종료 분기, overlay, agent reconnect, shell 교체 callback |
| S3 | `ui/src/state/workspaceStore.ts:308-340, 2019-2140` | 동기 stateRef dispatch, lifecycle 구독, remote status, 로컬 backend null 처리, backend rebind |
| S4 | `ui/src/lib/nativeTerminalLifecycle.ts:1-350` | backend별 직렬 queue, attach promise 공유, generation, presentation 대기 detach. PTY 생존 판정기는 아님 |
| S5 | `src-tauri/src/ipc/native_terminal.rs:479-531` | warm reattach 후 daemon attach. 오류를 그대로 반환하거나 surface 오류를 internal로 변환 |
| S6 | `src-tauri/src/daemon/server.rs:1594-1673` | attach 시 service/legacy router 분기, `Error { message }`, 부재 메시지 생성 |
| S7 | `src-tauri/src/daemon/client.rs:1000-1151`; `src-tauri/src/ipc/error.rs:199-229` | attach 응답의 INTERNAL_ERROR 변환과 별개인 PtyError 매핑. surface `SessionDetached`도 SESSION_NOT_FOUND를 사용 |
| S8 | `src-tauri/src/terminal/pty.rs:217-297`; `src-tauri/src/ipc/terminal.rs:194-209, 272-287` | lifecycle watcher/reap/registry 제거, stream Exit의 frontend 이벤트 전달 |
| S9 | `src-tauri/src/native_terminal/surface_host.rs:1089-1111` | 기존 surface의 stream/pump가 끝났으면 warm reattach를 사용하지 않음 |
| S10 | `src-tauri/src/ipc/terminal.rs:1068-1083`; `ui/src/lib/tauri.ts:483-493, 694-739` | 목록의 describe 실패 누락, no-op `waitForTerminalExit`, IPC 정규화/타입 가드 |
| S11 | `ui/src/lib/shellReplacement.ts:29-78`; `ui/src/App.tsx:2356-2374` | null backend인 일반 로컬 shell만 교체, spawn/persist/rebind, 중복 요청 억제 및 실패 시 생성 세션 정리 |
| S12 | `ui/src/lib/nativeTerminalVisibility.tsx:1-77` | macOS 표시 유지/입력 차단, 다른 플랫폼 yielding, 명시적 owner visibility 우선 |
| S13 | `ui/src/components/NativeTerminalPane.presentation.test.tsx:91-224`; `ui/src/lib/nativeTerminalLifecycle.test.ts:25-173` | 프레임 보존, 늦은 receipt, no-op geometry retry, queue/StrictMode/형제 pane 회귀 기준 |
| S14 | `ui/src/components/TerminalSplitView.tsx`의 `onOpenNewShell` prop 전달 경로 | outer view, group, leaf/sortable pane을 거쳐 TerminalPane까지 callback 전달 |

### 3.1 정상 종료 전달 경로

```text
PTY watcher: poll_exit_code / reader 종료 감지
  → close I/O, mark_exited, registry 제거
  → daemon stream의 Exit
  → GUI의 managed pump가 terminal_lifecycle 전달
  → terminalEventBus → SESSION_LIFECYCLE
  → 로컬: lifecycle=exited, backendSessionId=null, reconnectLifecycle=idle
  → TerminalPane overlay + NativeTerminalPane의 live attach/input 차단
```

이 경로와 native surface용 daemon attach 요청은 하나의 원자적 트랜잭션이 아니다. 세션 삭제와 frontend 이벤트 commit 사이의 간격은 자연스럽게 존재한다. 이벤트 채널 종료만 있는 경우와 실제 `Exit`가 있는 경우도 구별해야 한다. [S3, S5-S9]

### 3.2 실제 attach 오류 경로

```text
NativeTerminalPane.performAttach
  → cmd_native_terminal_attach
  → warm surface의 stream/pump가 끝났다면 daemon_client.attach
  → daemon: Error { message: "Session '<id>' not found" }
  → DaemonClient::attach: IpcError(INTERNAL_ERROR, message)
  → NativeTerminalPane.attemptAttach.catch
  → 진단 로그 + backoff + generic 빨간 배지
```

현행 `maxRetries = 5`, `bannerRetryThreshold = 2`다. 최초 시도 외에 최대 5회 재시도하며, 지연은 250 / 500 / 1000 / 2000 / 4000ms다. 다른 트리거가 개입하지 않는 실패 연쇄에서는 약 750ms 뒤 배지가 처음 표시되고 재시도는 계속된다. “2회 재시도 후 종료”가 아니다. [S1]

### 3.3 기존 문서·커밋과의 차이

- [2026-09-07 원인 분석](TERMINAL_PROCESS_KILLED_ATTACH_ERROR_ROOT_CAUSE_2026-09-07.md)은 당시의 `isExited`와 backend ID 불일치를 설명한 역사적 근거다. 현행 코드의 결함 목록으로 그대로 복사하지 않는다.
- `af164e5`는 종료 overlay 판별과 dead-session attach 차단을 변경했다. 현행 로컬 리듀서의 null 처리도 확인되지만, 그 변경까지 해당 커밋의 작업이라고 귀속하지 않는다.
- `0e605bb`와 [2026-09-09 overlay 보존 문서](TERMINAL_OVERLAY_RETENTION_2026-09-09.md)는 macOS 표시·입력·attachment 소유권 분리의 근거다. 이 계약을 유지한 상태에서 await 이후의 가드를 보완해야 한다.

## 4. 남은 결함과 위험 분석

### 4.1 종료 이벤트가 attach 실패보다 늦는 경우

store는 아직 running이고 owner도 live이므로 기존 시작 가드로 막을 수 없다. 코드만 보고 실패를 숨기면 store가 갱신되지 않아 overlay가 안 뜰 수 있다. **부재 분류와 상태 전이를 결합**해야 한다.

### 4.2 종료 후 진행 중인 attach가 실패하는 경우

이미 frame을 표시한 macOS pane에서 강제 재attach를 시작한 뒤 종료한다고 가정한다. `targetSessionId`는 null로 바뀌지만 `surfaceSessionId`는 보존 frame의 ID를 유지할 수 있다. surface effect의 dependency가 유지되면 `isSubscribed`도 true다. `isExited` effect가 `setError(null)`을 실행한 뒤 늦은 catch가 error를 다시 넣을 수 있다.

배지 임계점 전이라도 obsolete timer와 error severity 로그가 생길 수 있다. 특히 retryCount가 임계점 이상인 in-flight 실패에는 배지 재등장 가능성이 있다. 이는 코드 경로 분석이며, 이번 문서 작업에서 새로운 재현 테스트나 실제 SIGKILL 실험을 실행한 결과는 아니다. [S1]

### 4.3 no-op attach가 성공으로 해석되는 경우

`performAttach`는 owner가 유효하지 않으면 resolved promise를 반환하고, queue 내부 owner 가드도 단순 return한다. `nativeTerminalLifecycle`는 resolve를 attach 성공으로 취급한다. 호출자는 실제 IPC 실행 없이 `isAttached=true`나 후속 bounds를 진행할 수 있으며 cache에는 attached 예약이 남을 수 있다. 단순히 catch만 수정하지 말고 **cancelled와 attached를 구별**해야 한다. [S1, S4]

### 4.4 여러 진입점과 취소되지 않은 작업

mount, ResizeObserver, retry timer, 배지/geometry retry, 입력 복구가 attach로 진입한다. lifecycle helper가 IPC를 직렬화·공유하더라도, 같은 promise를 기다리는 여러 `attemptAttach`가 각각 타이머를 만들 수 있다. owner별 하나의 attach 시도와 하나의 retry timer라는 정책이 필요하다.

입력 복구는 `details.inputWritten === false`일 때만 재전송을 허용한다. 이 기존 안전장치를 유지하며, recovery attach에서 확인된 종료를 다시 `Failed to send terminal input`으로 바꾸지 않는다. [S1]

### 4.5 retained frame의 복구 버튼과 교체 실패

현행 presentation 테스트에는 종료 후 bounds 오류의 alert 버튼을 클릭해도 dead PTY에 reattach하지 않는 것을 확인하는 사례가 있다. 안전성은 있지만 버튼이 실제 복구를 하지 않는 UX가 남는다. 종료된 frame에는 attach retry 버튼을 제공하지 않는다. 실제 geometry 결함의 진단 자체는 삭제하지 않는다.

또한 lifecycle helper는 attach reject 시 `releasePresentationWaiters`를 호출한다. 정상적인 새 frame 표시를 기다리는 경로와, 새 attach 자체가 실패한 경로의 frame 보존은 같지 않다. 취소/실패 처리를 바꿀 때 이전 frame을 조기 detach하거나, 반대로 진짜 unmount의 surface를 영구 보존하지 않도록 명시적으로 검증해야 한다. [S4, S13]

## 5. 권장 수정 전략

### 5.1 1단계: attach 전용 오류 계약 보존

**기본안은 daemon wire protocol을 바꾸지 않는 소규모 수정이다.** `DaemonClient::attach`의 attach 응답 처리 arm에서만, 응답 메시지가 요청 ID의 정확한 `Session '<requested-id>' not found`와 일치할 때 다음 구조로 변환한다.

```ts
// 제안하는 Tauri 오류 형태. wire response 자체의 신규 필드가 아니다.
{
  code: "SESSION_NOT_FOUND",
  message: "Session 'backend-a' not found",
  details: {
    source: "daemon.attach",
    kind: "pty_session_missing",
    sessionId: "backend-a"
  }
}
```

handshake의 Error, socket read/write/EOF, protocol mismatch, surface 초기화/렌더 오류에는 이 매핑을 적용하지 않는다. `cmd_native_terminal_attach`는 의미를 보존한 오류를 그대로 반환한다. 서버의 부재 메시지 형식과 client 변환을 Rust 테스트로 함께 고정한다. 이 방식은 기존 실행 중인 daemon과도 호환되며 protocol version 변경이나 daemon 재시작을 요구하지 않는다.

장기적으로 daemon 오류를 typed protocol로 옮길 수는 있지만, 이는 직렬화/구버전 호환 범위를 넓히므로 본 UX 수정의 선행 조건으로 삼지 않는다. 단순한 `message.contains("not found")`는 금지한다.

### 5.2 2단계: frontend 분류 정책 추출

제안 신규 파일: `ui/src/lib/nativeTerminalAttachPolicy.ts`. React와 store를 import하지 않는 순수 함수로 만든다. 분류 결과는 제안상 `confirmed-missing | unverified-missing | operational-error`이며, stale owner 판정은 실행 컨트롤러에서 먼저 수행한다.

| 입력 | 분류 / 처리 |
| --- | --- |
| SESSION_NOT_FOUND + source/kind/sessionId가 현재 attach 요청과 모두 일치 | confirmed-missing |
| 현행 legacy 형태인 INTERNAL_ERROR + 요청 ID까지 완전히 일치하는 부재 메시지 | confirmed-missing. 현재 bridge 호환을 위한 제한된 경로 |
| SESSION_NOT_FOUND + 정확한 legacy 부재 메시지, 충돌하는 details 없음 | confirmed-missing으로 호환 가능 |
| SESSION_NOT_FOUND지만 `Gone`, surface detached 등 부재 대상/출처가 불명확 | unverified-missing. 자동 종료 금지 |
| 메시지 ID가 다르거나 details가 요청과 충돌 | operational-error로 남기고 진단. 새 세션 종료 금지 |
| GPU/renderer, IO_ERROR, DAEMON_PROTOCOL_MISMATCH, 임의 INTERNAL_ERROR, raw string/Error | operational-error. 일반 오류 정책 유지 |
| set_bounds/send_input 등 다른 명령의 SESSION_NOT_FOUND | attach용 종료 분류를 적용하지 않음. 해당 명령의 소유권·오류 정책 적용 |

`isStructuredIpcError`를 이용한다. 현행 `toIpcError`가 raw string을 JSON으로 파싱해 주는 것으로 가정하지 않는다. fallback에서 메시지 포맷을 해석하는 책임은 이 함수 한 곳에만 두며, 신규 typed 응답과 legacy 응답에 대한 테스트를 별도로 둔다. [S7, S10]

**목록 polling은 기본안에 추가하지 않는다.** `listTerminalSessions()`의 빈 결과나 `waitForTerminalExit()`의 resolve는 종료 증거가 아니다. 모호한 오류는 기존 오류 경로로 남기므로 실제 장애를 조용히 삼키지 않는다. 향후 모호한 부재까지 조정해야 한다면, 목록 summary가 아니라 epoch와 원자적인 ID 존재 여부를 반환하는 별도 probe를 설계하고 `unknown/failed`를 `missing`과 분리해야 한다.

### 5.3 3단계: 비동기 owner 검증과 단일 재시도

각 attach 작업은 시작 시 다음 정보를 고정한다.

```ts
// 제안 타입. owner는 문자열 ID가 아니라 commit-scoped 객체 정체성이다.
type AttachBinding = {
  workspaceId: string;
  localSessionId: string;
  backendSessionId: string;
  daemonEpoch: string | null;
};
```

실행 전, lifecycle queue가 실제 IPC를 시작하기 전, await 성공/실패 후, retry timer 및 사용자 retry 진입 시 모두 **동일한 owner 객체 + 동일 binding + live + 구독 중**을 검사한다. A→B→A나 hide→show 후 문자열만 같다고 이전 작업을 부활시키지 않는다. callback 전달은 안정된 함수 또는 committed ref로 관리하여 매 렌더마다 surface effect를 재생성하지 않는다.

`isExited`를 기존 surface effect dependency에 무조건 추가하여 cleanup으로 문제를 덮지 않는다. 그러면 macOS frame 보존이 깨질 수 있다. live attach 취소와 retained surface geometry lifetime은 별도로 관리한다.

권장 처리 순서는 다음과 같다.

```text
시작: 현재 live owner인가? 아니면 cancelled
  → owner별 in-flight attach가 있으면 그 결과를 공유
  → performAttach / queue / native IPC
결과: 작업 owner가 현재도 같은가? 아니면 stale, UI/store 변경 없이 무시
  → 실제 IPC를 하지 않은 cancelled인가? success 처리 금지
  → confirmed-missing인가?
      즉시 해당 binding quarantine
      retry timer 제거, attach/input 복구 차단
      해당 binding의 attach error 제거
      로컬이면 onBackendSessionUnavailable(binding) 1회 호출
      remote이면 기존 remote 복구 흐름으로 전달
      return (generic error 보고나 새 timer 없음)
  → 실제 오류인가?
      원인 진단 보존, 기존 제한된 backoff와 배지 정책 적용
```

quarantine은 parent/store commit을 기다리는 짧은 간격의 재attach·입력을 차단하기 위한 owner-scoped 상태다. 전역 `Set<backendId>`만으로 영구 보관하지 않는다. 새로운 binding/owner에서는 해제하고, 진짜 unmount에서는 정리한다.

같은 owner에는 attach promise 하나, retry timer 하나만 둔다. 수동 retry와 ResizeObserver가 동시 발생해도 retry budget이 무한히 초기화되거나 여러 timer가 생기지 않게 한다. 실제 오류의 현행 250~4000ms backoff 및 배지 임계점은 이번 변경에서 임의로 늘리거나 줄이지 않는다.

### 5.4 4단계: store의 종료 상태로 안전하게 수렴

제안 신규 액션은 다음과 같다. 실제 backend `Exit`를 받지 않았으므로 가짜 exit payload를 event bus에 발행하지 않는다.

```ts
// 제안: WorkspaceAction에 추가
{
  type: "SESSION_BACKEND_UNAVAILABLE",
  workspaceId,
  sessionId,                 // 로컬 pane ID
  expectedBackendSessionId,
  expectedDaemonEpoch,       // null도 정확히 비교
  reason: "attach-session-missing"
}
```

`NativeTerminalPane → TerminalPane → TerminalSplitView → App의 dispatchWorkspaceAction`으로 `onBackendSessionUnavailable` callback을 전달한다. 표시 컴포넌트가 store를 직접 import하여 또 다른 전역 상태 경로를 만들지 않는다. standalone NativeTerminalPane 사용에는 callback이 없어도 attach 억제는 동작해야 하지만, store overlay 보장은 실제 앱의 callback 연결에 대해 통합 테스트로 검증한다.

리듀서는 workspace/pane/backend/epoch가 현재 값과 모두 일치하고 로컬 세션일 때만 전이한다. 그 사이 새 backend로 rebind되거나 workspace가 바뀌었으면 동일 state를 반환한다. 컴포넌트의 owner token 검증과 reducer의 compare-and-set을 함께 사용한다. reducer의 ID 비교만으로 A→B→A 레이스를 해결했다고 주장하지 않는다.

로컬 종료 전이 코드는 기존 `SESSION_LIFECYCLE`과 공유하는 작은 helper로 정리한다. 두 경로 모두 `lifecycle=exited`, `backendSessionId=null`, `reconnectLifecycle=idle` 및 기존 activity 완료 처리를 적용한다. cwd/worktree/title/provider reference와 다른 pane은 보존한다. 중복 부재 응답, 이후 도착한 실제 Exit는 idempotent해야 한다.

SSH의 `SESSION_LIFECYCLE(exited)`는 현행대로 `remoteConnectionState=reconnecting`, `remoteGeneration=null` 경로다. 새 로컬 unavailable 액션을 이 분기에 재사용하지 않는다. remote attach 부재를 발견했을 때는 기존 remote status/retry 컨트롤러에 재평가를 요청하고 `missing/expired/legacyLost` 등 remote 상태가 최종 UI를 결정하게 한다.

### 5.5 5단계: queue 취소와 presentation 소유권 보존

`nativeTerminalLifecycle.ts`는 PTY 상태 관리자가 아니라 surface 작업 조정자라는 역할을 유지한다. store나 종료 메시지 판별을 이 모듈에 넣지 않는다.

다만 호출자가 native IPC를 실행하지 않은 작업을 성공으로 반환하지 않도록 `attached | cancelled` 결과를 명시적으로 전달하는 최소 확장을 제안한다. queue의 cancelled 예약은 해당 promise/작업이 현재 cache 엔트리일 때만 제거한다. 이후 세대의 attach promise나 실제 표시된 surface를 지우면 안 된다. `performAttach`의 mount/retry 및 input recovery 호출부를 모두 이 결과에 맞춘다.

이미 발행된 IPC는 JavaScript promise 취소만으로 취소되지 않는다. stale completion이라고 즉시 `cmd_native_terminal_detach`를 보내면 새 owner가 공유하는 surface를 없앨 수 있으므로 금지한다. 실제 teardown은 기존 직렬 queue와 ownership으로 수행한다.

교체 attach 실패 시의 outgoing frame은 **해당 pane의 명시적인 presentation lease**와 함께 처리한다. macOS의 표시된 outgoing frame은 새 frame의 `presented=true`, 명시적 owner hide, 또는 진짜 unmount가 있어야 해제한다. cancelled/failed replacement가 그 lease를 무조건 해제하지 않게 한다. lease를 추가할 경우 pane identity를 키에 포함하고, 단순히 `releasePresentationWaiters` 호출을 전부 삭제하여 형제 pane의 detach나 unmount 정리를 막지 않는다. cold pane은 보존할 frame이 없으므로 opaque fallback을 쓴다.

이 queue 확장은 cancellation/cache/교체 실패 Red 테스트를 먼저 만든 뒤 필요한 범위만 구현한다. 생존 PTY를 임의로 종료하거나 보존 frame을 daemon 세션처럼 재attach하는 동작은 추가하지 않는다.

### 5.6 6단계: 사용자 오류와 진단의 경계

error는 문자열 하나보다 `{ kind, owner/binding, userMessage }` 형태로 구분하는 것이 안전하다. attach 성공, input 성공, bounds 성공이 다른 owner/종류의 실제 오류를 무조건 지우지 않게 한다.

| 상태 | 사용자 UI | 진단 |
| --- | --- | --- |
| stale / 이미 종료된 owner의 attach 결과 | 변화 없음, retry 없음 | 필요 시 sampled debug |
| 현재 로컬 binding의 확인된 부재 | 종료 overlay, attach 배지 없음 | `terminal.surface.attach.session_missing` 같은 저심각도 이벤트 1회 |
| 현재 live owner의 실제 attach/renderer/IPC 실패 | 기존 오류 및 유효한 retry | command, code, stage, owner generation, retryCount 보존 |
| 종료 후 retained frame의 실제 geometry 실패 | 종료 overlay 유지, attach하는 alert 버튼 없음. 필요 시 비대화형 상태 설명 | 실제 오류 원인은 진단에 남김 |
| 새 shell spawn/persist 실패 | 종료 overlay 내 실제 교체 오류 + 다시 시도 가능한 버튼 | 실제 오류 보고 유지 |

raw IPC 메시지, filesystem 경로, backend ID를 사용자용 generic 문구로 그대로 옮기지 않는다. `reportNativeTerminalIpcFailure`를 전역적으로 무음 처리하지 않고, stale/confirmed-missing만 그 앞에서 분기한다. 실제 장애의 상세 원인은 진단 채널에 보존한다.

## 6. 파일별 상세 변경 계획

아래는 **후속 구현에서 변경할 파일**이며, 이번 문서 작성 작업에서 변경한 파일 목록이 아니다.

| 파일 | 권장 변경 | 완료 조건 |
| --- | --- | --- |
| `ui/src/lib/nativeTerminalAttachPolicy.ts` (신규 제안) | attach 전용 typed/legacy 분류 순수 함수 | request ID 일치, 출처 구분, 부정 사례 단위 테스트 |
| `ui/src/components/NativeTerminalPane.tsx` | 전/후 owner 검사, single-flight/timer, quarantine, unavailable callback, binding별 오류, cancelled 결과 처리 | 종료 이후 badge/timer/input 복구 없음, live 실제 오류 유지 |
| `ui/src/components/TerminalPane.tsx` | callback 전달, 종료 UI 우선순위 및 접근성 검증. 기존 local/SSH 판별을 하나로 합치지 않음 | 일반 shell overlay와 Open new shell 실제 동작, agent reconnect 유지 |
| `ui/src/components/TerminalSplitView.tsx` | outer/group/leaf/sortable pane의 callback prop을 빠짐없이 전달 | split 이동·재마운트에도 올바른 pane binding 전달 |
| `ui/src/App.tsx` | unavailable callback을 guarded workspace action으로 연결 | workspace 변경/새 shell rebind를 옛 응답이 덮지 않음 |
| `ui/src/state/workspaceStore.ts` | unavailable 액션, local-exit helper 공유, CAS/idempotency | null binding/활동 완료/metadata 보존 및 remote 분기 불변 |
| `ui/src/lib/nativeTerminalLifecycle.ts` | cancelled 결과와 cache 정리; 필요한 경우 pane-scoped presentation lease 최소 확장 | no-op 성공 오인·cache 오염·형제 surface detach 없음 |
| `src-tauri/src/daemon/client.rs` | attach 응답 arm의 정확한 부재 메시지만 SESSION_NOT_FOUND + details로 변환 | handshake/IO/parse/일반 internal 오류 불변 |
| `src-tauri/src/ipc/native_terminal.rs` | typed daemon 오류 그대로 전달함을 테스트. catch-all Ok(())로 변경 금지 | 실제 surface 초기화 오류와 부재 오류를 구별 |
| `src-tauri/src/daemon/server.rs`, `src-tauri/src/ipc/error.rs` | 메시지 및 context 구분 회귀 테스트. wire protocol과 전역 오류 매핑은 기본적으로 유지 | surface SessionDetached를 PTY 종료로 오판하지 않음 |
| `src-tauri/src/terminal/pty.rs`, `native_terminal/surface_host.rs` | 기본안에서 production 로직 변경 없음 | reap/close 순서를 UX 해결책으로 지연시키지 않음 |
| `ui/src/lib/shellReplacement.ts` 및 해당 테스트 | 기본 동작 유지, unavailable 전이 후 실제 교체의 통합 회귀 확인 | 중복 클릭·persist 실패·stale spawn 정리 보존 |
| `nativeTerminalVisibility.tsx`, CSS 및 기존 presentation 테스트 | 표시/입력 분리 유지. exited geometry no-op alert 기대는 새 UX로 갱신 | macOS retain / Windows·Linux yield / opaque fallback 유지 |

## 7. 엣지 케이스 및 레이스 컨디션 대응 매트릭스

| ID | 순서 / 상황 | 필수 방어와 기대 결과 |
| --- | --- | --- |
| R01 | Exit commit 후 첫 mount | attach 0회, 일반 shell overlay 및 새 shell 버튼 |
| R02 | registry 삭제 → attach missing → Exit 지연/유실 | 최초 확인된 부재에서 quarantine·CAS 종료 전이, 추가 retry 0회 |
| R03 | attach 대기 → Exit commit → 늦은 reject | owner 검증에서 버림. macOS effect가 살아 있어도 error/timer를 다시 만들지 않음 |
| R04 | attach 대기 → Exit commit → 늦은 resolve | attach 성공 UI/focus/input 복구 없음. cold pane의 늦은 첫 frame은 보존하지 않음 |
| R05 | retry timer 예약 → Exit | timer 제거. fake timer를 모든 backoff 구간까지 진행해도 추가 attach 없음 |
| R06 | pane A → backend B rebind → A의 늦은 부재 | B의 binding/lifecycle/error는 불변 |
| R07 | A→B→A, 또는 hide→show | 동일 ID라도 과거 commit의 owner 결과는 무효 |
| R08 | 새 shell 생성·persist 중 과거 Exit/부재 | 오래된 backend 액션은 새 binding에 적용되지 않음. 생성 실패 시 기존 overlay 유지 |
| R09 | ResizeObserver + timer + 수동 retry 동시 | owner당 in-flight 1개, retry timer 1개. 실패 예산 무한 초기화 금지 |
| R10 | 이미 표시한 macOS pane에서 강제 attach 중 종료 | 마지막 frame 유지, 새 배지 없음, input owner 해제 |
| R11 | macOS retained frame의 resize 오류와 클릭 | 진단은 남기되 dead PTY attach용 버튼은 없음 |
| R12 | Windows/Linux에서 종료 또는 covering overlay | surface yield/detach로 DOM controls 노출. macOS 보존 로직을 이식하지 않음 |
| R13 | macOS modal/search가 attach 대기 중 열림 | underlying presentation은 허용하되 focus 탈취 및 terminal 입력은 금지 |
| R14 | 다른 pane의 detach와 신규 attach 교차 | 한 pane의 종료/교체가 형제 split surface를 해제하지 않음 |
| R15 | StrictMode setup→cleanup→setup | queue promise 공유, cancelled 작업의 cache 오염·중복 종료 액션 없음 |
| R16 | 이전 frame A 보존 → replacement B attach 실패 | mounted macOS pane은 A lease 보존. 진짜 unmount 시 모두 해제 |
| R17 | daemon socket EOF/timeout/protocol mismatch | 종료로 단정하지 않음. 실제 통신 오류와 기존 복구 정책 유지 |
| R18 | SSH transport 단절 또는 remote attach 부재 | 로컬 unavailable 액션 금지, backend identity/generation과 remote 상태 정책 보존 |
| R19 | SSH `missing/expired/legacyLost` 확정 | remote 상태에 맞는 overlay, 로컬 shell 교체 함수 호출 금지 |
| R20 | 자식 프로세스만 kill, shell/PTY 생존 | 종료 overlay 없음, 정상 입력/프롬프트 유지 |
| R21 | live renderer/GPU/geometry 결함 | 일반 오류 로그와 유효한 복구 affordance 유지, lifecycle exited로 변경 금지 |
| R22 | 부재 메시지의 ID 불일치 / details 충돌 | 자동 종료·조용한 억제 금지, 현재 binding 보호 |
| R23 | 세션 목록에서 describe 실패 항목 누락 | 목록 부재로 종료 판정하지 않음 |
| R24 | workspace 전환 후 늦은 callback / 중복 Exit | workspace CAS 및 owner 검증. 복귀 시 실제 attach의 부재 응답으로 수렴, 다른 프로젝트 변경 금지 |
| R25 | `inputWritten=false` 복구 attach가 missing | 재전송하지 않고 현재 로컬 종료로 수렴. Ctrl+C 전달 불명확 시 재전송 금지 유지 |
| R26 | 취소된 queue operation 또는 늦은 native success | cancelled를 attached로 cache하지 않음. 새 owner의 surface를 직접 detach하지 않음 |

숨겨진 다른 workspace의 lifecycle snapshot 동기화 전체를 이번 작업에서 재설계하지 않는다. 복귀 시 active binding의 명확한 부재 응답으로 수렴하는 것을 검증하고, snapshot 전역 동기화는 별도 범위로 기록한다.

## 8. Vitest Red–Green 테스트 계획

### 8.1 테스트 작성 원칙

실제 daemon이나 OS 프로세스를 unit test에서 죽이지 않는다. `invoke` mock, deferred promise, fake timer, ResizeObserver mock, 실제 reducer를 사용하는 작은 harness로 이벤트 순서를 결정적으로 만든다. macOS 테스트는 **처음부터 exited로 mount하는 사례만으로 끝내지 말고**, 먼저 `presented=true` receipt를 받은 뒤 강제 reattach와 종료를 교차시킨다.

여기서 **Red 예상**은 현행 코드 분석으로 예측한 실패이며 이번 작업에서 실행한 신규 실패 결과가 아니다. 먼저 현행 코드에서 의도한 assertion이 실패하는지 확인하고 그 로그를 남긴다. 새 API가 아직 없어서 import/type 오류만 나는 것은 동작 재현 Red의 대체 증거가 아니다.

### 8.2 신규/확장 테스트 목록

| 테스트 위치 | 사례와 assertion | 현행 예상 / Green 변경 |
| --- | --- | --- |
| `nativeTerminalAttachPolicy.test.ts` (신규) | 새 typed 부재, legacy INTERNAL_ERROR 정확 일치, 요청 ID 불일치, details 충돌, raw Error/string, GPU·IO·protocol 오류 | 새 순수 정책 단위 테스트. import 실패가 아닌 입력/출력 계약으로 검증 |
| `NativeTerminalPane.exitAttach.test.tsx` (신규) | running mount에 legacy 부재 reject, Exit를 전혀 보내지 않음. callback 1회, alert 없음, timer 진행 후 attach 증가 없음 | Red 예상: 현행 generic retry. Green: 분류+quarantine |
| 위 통합 harness | callback이 실제 workspaceReducer를 호출하고 updated session으로 rerender. Shell exited와 enabled Open new shell 확인 | Red 예상: 현행 store reconciliation 없음. Green: callback chain+CAS |
| `NativeTerminalPane.presentation.test.tsx` 확장 | 먼저 present → 재시도 임계점의 in-flight attach 생성 → exit commit → reject. alert·추가 timer 없음, frame 보존 | Red 예상: macOS isSubscribed만으로는 늦은 실패를 막지 못함 |
| 위 파일 확장 | 먼저 present → attach pending → exit → resolve. focus/input/bounds 신규 attach 흐름 불가 | Red 예상 경로를 확인한 뒤 post-await owner 가드 |
| `NativeTerminalPane.exitAttach.test.tsx` | 250ms timer 예약 뒤 exit; 500/1000/2000/4000ms까지 bounded advance | 기존 cleanup 통과 사례와 retained macOS 사례를 분리 |
| 위 파일 | A→B→A / hide→show / workspace switch 뒤 옛 reject | 현재 owner만 UI·store 변경. 문자열 ID 일치만 검사하는 구현을 잡음 |
| 위 파일 | ResizeObserver, 배지, timer 동시 실행 | in-flight/timer 각각 1개, budget 보존, 부재 전이 1회 |
| `workspaceStore.terminalExit.test.ts` (신규) | local lifecycle Exit / 새 unavailable 액션의 결과 동등성 | backend null, lifecycle exited, reconnect idle, activity done, metadata 보존 |
| 위 파일 | wrong workspace/pane/backend/epoch, 중복 missing, 실제 Exit 후 missing, missing 후 실제 Exit | state idempotency, rebind된 B 불변, 다른 pane 불변 |
| 위 파일 + `sshRecovery.test.ts` | SSH lifecycle Exit와 unavailable 후보 | remote identity 유지, 로컬 종료 helper 적용 금지 |
| `NativeTerminalPane.test.tsx` 확장 | generic attach 장애가 초기 retry에서 회복하면 배지 없음; 지속 실패하면 기존 threshold 이후 배지·로그 | 기존 Green 회귀 기준 유지, 무조건 return하는 수정 방지 |
| `NativeTerminalPane.lifecycle.test.tsx` 확장 | live geometry/renderer 실패 보고·유효 retry 유지 | 오류 숨김 범위가 과도하지 않음 |
| 위 파일 확장 | inputWritten=false 복구 attach 부재, cancelled recovery, 전달 불명확 Ctrl+C | 종료 시 재전송 0회. 실제 전달 불명확 정책은 기존 Green 유지 |
| `NativeTerminalPane.presentation.test.tsx` 수정 | retained-frame geometry 실패 후 attach retry alert가 없음, 진단은 존재 | 현재 “alert 클릭 후 attach 안 함” 테스트를 새 UX assertion으로 강화 |
| `nativeTerminalLifecycle.test.ts` 확장 | queue에서 owner 무효화된 operation의 cancelled 결과 후 새 attach | 새 작업이 false attached cache를 재사용하지 않음 |
| 위 파일 확장 | cancelled/failed B replacement, sibling remount, true unmount, A 복귀 | lease 누수·조기 detach·형제 pane teardown 없음 |
| `TerminalPane.test.tsx` 확장 | local lifecycle exited지만 stale backend ID; 일반 shell vs agent overlay | 기존 종료 판별 회귀. 새 버튼 정책을 agent 교체 함수와 혼동하지 않음 |
| `TerminalPane.exitAttach.integration.test.tsx` (신규) | 실제 TerminalPane/NativeTerminalPane/reducer, late missing → shell 교체 → 새 backend present | overlay→유효 버튼→새 frame까지 연결. 자식 컴포넌트를 mock하지 않음 |
| `shellReplacement.test.ts` 확장 | unavailable 상태에서 더블 클릭, spawn/persist 실패, 도중 rebind | single-flight, 잔여 backend 정리, 실제 오류 노출 유지 |
| visibility/presentation 기존 suites | macOS modal/search 유지, Linux/Windows yielding, cold fallback, 명시 hide | 기존 Green 기준 유지 |
| 정책/통합 negative test | bare SESSION_NOT_FOUND 또는 partial list를 주입해도 자동 local exit 없음 | 증거 없는 종료 판정 방지 |

새 통합 테스트는 pane의 실제 `role=region` overlay와 버튼을 검사한다. “alert가 없다”만 확인하면 UI가 통째로 사라진 구현도 통과하므로 overlay, 입력 비활성화, 실제 복구 callback, frame/opaque 상태를 함께 검사한다.

### 8.3 테스트 절차 예시

아래는 **작성할 테스트의 시나리오**이며 현행 저장소에 존재하는 helper/API를 가장한 실행 코드는 아니다.

```text
1. fake timers, lifecycle cache reset, 명확한 geometry와 macOS platform mock을 준비한다.
2. 실제 TerminalPane + reducer harness를 running backend-a로 mount한다.
3. invoke(attach)를 legacy INTERNAL_ERROR / Session 'backend-a' not found로 reject한다.
4. Exit 이벤트는 보내지 않는다. promise/React commit을 act로 flush한다.
5. Shell exited region, Open new shell 버튼, disabled input을 확인한다.
6. attach alert와 attach-error severity 로그가 없고 unavailable 액션이 1회인지 확인한다.
7. 모든 backoff 구간을 bounded advance한 뒤 추가 attach가 없는지 확인한다.
8. Open new shell을 클릭하고 spawn/persist/rebind를 mock 성공시킨다.
9. backend-b present 전에는 macOS의 기존 frame을 보존하고, present 후 교체를 확인한다.
10. backend-a의 늦은 reject/Exit를 추가 전달해 backend-b가 running인지 확인한다.
```

fake timer를 무한 `runAllTimers`로 돌려 렌더 RAF loop와 혼합하지 않는다. 각 시간 구간 및 microtask를 `act` 안에서 진행하고 테스트 종료 시 timer, observer, event subscription, lifecycle global cache를 정리한다.

### 8.4 Rust 계약 테스트

Vitest mock만으로 backend 오류 형태의 정확성을 증명할 수 없으므로 다음을 후속 구현에 추가한다.

- client attach 응답 분류: 정확한 요청 ID 부재만 SESSION_NOT_FOUND + source/kind/sessionId, 다른 Error는 INTERNAL_ERROR 유지.
- handshake Error, protocol mismatch, attach EOF/read/parse 오류가 missing으로 변환되지 않음.
- server의 존재하지 않는 local session attach 응답과 client 매핑을 함께 확인. 메시지 형식 회귀도 테스트한다.
- IPC native attach는 daemon의 typed 오류를 보존하고 renderer 초기화 오류를 성공/종료로 변환하지 않음.
- surface SessionDetached의 기존 geometry 의미와 PTY 부재 의미가 섞이지 않음.

예시 실행 명령은 `cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::client`다. native-terminal feature가 필요한 계약 테스트는 프로젝트의 해당 feature 구성으로 추가 실행한다. 이 문서 작업에서는 Rust 코드를 바꾸지 않았고 이 신규 계약 테스트도 실행하지 않았다.

## 9. 실행 순서와 완료 게이트

| 순서 | 구현 단위 | 완료 게이트 |
| --- | --- | --- |
| P1 | 현행 legacy 부재와 retained macOS late reject의 동작 Red 추가 | 실패가 assertion에 의한 것임을 확인; 기존 focused suites는 Green |
| P2 | client attach 오류 계약 + frontend 순수 분류 | Rust/TS positive·negative 계약 테스트 Green |
| P3 | owner 검증, single-flight/quarantine, cancelled 결과 | R03-R09, R25-R26 Green. 실제 오류 보고 보존 |
| P4 | unavailable callback chain + CAS local-exit helper | Exit 유실 시 overlay/실제 새 shell 버튼, rebind/workspace/SSH 보호 Green |
| P5 | retained geometry 배지와 replacement lease 정합성 | macOS retain/true unmount/형제 split/non-mac yield Green |
| P6 | 전체 UI 테스트·build, Rust 계약 검증, desktop acceptance | 환경별 결과와 미실행 항목을 분리하여 기록 |

P2만 배포하고 P4 없이 “배지를 숨겼으니 완료”로 종료하지 않는다. 가드만 추가한 긴급 패치는 증상을 줄일 수 있지만, 종료 이벤트 유실 시 상태 수렴까지 보장하는 최종 해결책은 아니다.

## 10. 검증 명령과 이번 검토의 증거 범위

### 10.1 기존 회귀 기준

문서 작성 전, 아래 12개 기존 Vitest suite를 함께 실행하여 **272 tests passed / 12 files passed / exit 0**를 확인했다. 오류 경로를 의도적으로 테스트하는 일부 사례가 stderr에 IPC 실패 로그를 출력했지만 assertion 실패는 없었다.

```sh
npm test --prefix ui -- \
  src/components/NativeTerminalPane.test.tsx \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/components/NativeTerminalPane.presentation.test.tsx \
  src/components/TerminalPane.test.tsx \
  src/components/TerminalPane.sshReconnect.test.tsx \
  src/components/TerminalSplitView.dragFeedbackVisibility.test.tsx \
  src/lib/nativeTerminalLifecycle.test.ts \
  src/lib/nativeTerminalVisibility.test.tsx \
  src/lib/agentResumeAffordance.test.ts \
  src/lib/sshRecovery.test.ts \
  src/nativeTerminalPresentationBacking.test.ts \
  src/nativeTerminalPlatformTransparency.test.ts
```

사전 실행은 위와 같은 suite 목록에 대해 `npm --prefix ui test -- ...` 형태를 사용했다. 최종 문서 저장 후에도 현재 workspace revision에서 focused test와 문서 diff를 다시 검증한다. shell 교체 범위의 별도 회귀는 `src/lib/shellReplacement.test.ts`를 추가한다.

### 10.2 후속 구현의 검증 명령

```sh
# 신규 테스트 추가 후 전체 UI 회귀
npm test --prefix ui

# TypeScript 및 production bundle
npm run build --prefix ui

# 변경 범위/공백 점검
git diff --check
git status --short
```

기존 테스트가 통과한다는 것은 새 레이스가 이미 해결되었다는 증거가 아니다. 이번 산출물은 계획서이고, **신규 Red–Green 테스트 실행, Rust 변경 검증, 실제 GPU/OS surface 동작 확인은 후속 구현의 수용 조건**이다.

## 11. Desktop / 플랫폼 수동 검증 계획

실제 compositor 동작은 jsdom 테스트로 증명하지 않는다. 격리된 테스트 세션에서 수행하고, 사용자의 작업 프로세스나 background daemon을 임의로 종료하지 않는다. 기존 프로젝트 지침의 debug 실행 경로는 `bun tauri dev`이며 이번 문서 작업에서는 앱을 실행하거나 daemon을 재시작하지 않았다.

| 플랫폼/상황 | 확인할 결과 |
| --- | --- |
| macOS 표시된 shell을 정상 exit 또는 해당 테스트 shell PID만 강제 종료 | 마지막 frame + Shell exited + 유효한 Open new shell. 빨간 attach badge 없음 |
| macOS 테스트 전용 PTY 프로세스 crash, attach 지연과 종료 교차 | 종료 인지 후 추가 attach/error 깜빡임 없음; 앱 UI 및 다른 세션 정상 |
| 쉘의 자식 프로세스만 종료 | 살아 있는 shell 프롬프트 유지; 잘못된 종료 overlay 없음 |
| macOS agent 종료 | provider reference와 reconnect affordance 유지, 모달/검색 focus 탈취 없음 |
| macOS cold restored exited pane | native attach 없이 opaque terminal-theme fallback |
| macOS 종료 → 새 shell → 새 frame 표시 전/후 | 교체 전 기존 frame, 실제 presented receipt 이후 기존 surface 1회 해제 |
| Windows / Linux 종료, 검색/모달, split 이동 | native surface가 controls를 덮지 않도록 기존 yielding 유지 |
| live surface/IPC 오류를 fault injection | 실제 오류 진단과 유효한 retry 유지, 종료 판정 없음 |
| SSH disconnect/reconnect 및 expired 상태 | 기존 remote 문구/복구 정책 유지, 로컬 새 shell 교체 호출 없음 |
| hidden tab/workspace 복귀, pane close | stale backend 재attach에서 안전하게 수렴; 진짜 close 시 보존 surface 누수 없음 |

프로세스 종료·이벤트 수신·store commit·최초 overlay 시점, attach 호출 수, retry timer 수, error 분류, detach/present 횟수를 테스트 증거로 남긴다. 시간 목표는 UI 응답성을 위한 수용 기준으로 측정하며 단위 테스트의 임의 대기시간을 실제 OS 성능 보장으로 쓰지 않는다.

## 12. 채택하지 않는 접근과 잔여 경계

`SESSION_NOT_FOUND` 전역 무시, 모든 attach catch의 무조건 return, 종료 시 backend ID를 과거 값으로 되돌려 유지, 모든 플랫폼 강제 detach, 재시도 시간을 늘려 이벤트를 기다리기, 목록 부재만으로 전체 store를 종료 처리하기는 채택하지 않는다. 각각 실제 장애 은폐, overlay 미표시, state 불일치, macOS frame 손실, 지연된 UX 또는 살아 있는 세션 오판을 만들 수 있다.

최소 수정의 성공 기준은 **“종료된 세션의 내부 attach 실패가 사용자 오류로 새지 않고, 현재 pane이 실제로 실행 가능한 종료 후 행동을 제공하며, 살아 있는 세션의 진짜 오류는 계속 드러나는 것”**이다. 모든 daemon/remote 상태 체계의 재설계, 영구 스크린샷 보관, agent의 일반 shell 전환 UX 확장은 별도 범위로 유지한다.
