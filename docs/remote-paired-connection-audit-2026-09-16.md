# Remote Paired Machine 접속 감사 보고서

- 감사일: 2026-09-16
- 대상: Ferryx Rust/Tauri v2 backend + React/TypeScript frontend의 paired daemon 연결 경로
- 범위: public relay, direct/LAN gateway, pairing/auth, socket-ticket, terminal attach, session lifecycle, worktree/workspace, reconnect/replay, UI 오류 전파, cross-platform
- 방법: 현재 저장소 코드만 읽어 실제 실행 경로를 추적했다. 과거 이슈가 현재 코드에 이미 수정된 경우에는 결함으로 재기록하지 않았다.
- 변경 제약: 본 감사에서 production code/test는 수정하지 않고 이 보고서 파일만 생성했다.

## 요약

현재 코드에서 **20개 문제**를 확인했다. 심각도는 **Blocker 0 / Major 16 / Minor 4**다. 가장 큰 위험은 (1) paired session mutation의 timeout 계층이 서로 어긋나 원격 PTY가 생성/종료되었는지 알 수 없는 상태가 구조적으로 발생하는 점, (2) 그 ambiguous mutation을 spawn/cleanup 경로가 operation journal로 reconciliation하지 않아 원격 shell이 유출될 수 있는 점, (3) live paired proxy가 끊어지면 exact remote target descriptor가 함께 사라져 자동 reattach가 불가능해지는 점, (4) pairing/revoke 성공을 디스크 영속화 성공과 분리해 보고하는 점이다.

relay 자체의 control reconnect, single-use socket-ticket, pairing lease 상한, 동일 pairing registration 재시도, ownership store locking, canonical remote data-dir 분리는 현행 코드에서 방어가 확인되었다. 그러나 아래 결함 때문에 end-to-end paired-machine UX는 여전히 relay 재시작, 네트워크 지연, daemon 재시작, custom relay, LAN 연결, disk I/O 오류에서 취약하다.

---

## P01. Relay pairing claim이 downstream 교환 실패 시 복구되지 않아 유효 PIN이 소모된다

- **심각도:** Major
- **근거:** `src-tauri/src/remote/relay_server.rs:742-821`, `src-tauri/src/remote/relay_server.rs:1333-1388`
- **Root cause:** relay는 pair exchange 시작 시 `Ready` registration을 먼저 `Claimed`로 바꾼 뒤 daemon 쪽 exchange를 수행한다. downstream 응답이 성공일 때만 token cache를 갱신하고 registration을 `Consumed`로 바꾼다. proxy/daemon 오류나 비-성공 HTTP 응답에서는 `Claimed -> Ready` rollback이 없다.
- **실패 시나리오:** 사용자가 정상 PIN을 입력한 순간 relay↔daemon control/data handoff가 일시 실패하거나 daemon pair endpoint가 5xx를 반환한다. 첫 요청은 실패하고 같은 PIN 재시도는 lease가 아직 유효해도 `Ready`가 아니므로 거절된다. 사용자는 새 PIN을 다시 발급해야 한다.
- **수정 방향:** claim을 generation/token으로 fenced 된 임시 lease로 만들고 downstream 성공 시에만 consume한다. 실패/timeout 시 동일 generation인 경우 원자적으로 `Ready`로 rollback하거나, daemon exchange 자체를 idempotent transaction으로 만들어 relay claim 상태와 함께 commit한다.

## P02. Pairing/revoke 성공 응답이 auth state의 durable persistence 성공을 보장하지 않는다

- **심각도:** Major
- **근거:** `src-tauri/src/remote/auth.rs:501-629`, `src-tauri/src/remote/auth.rs:757-775`, `src-tauri/src/remote/auth.rs:832-845`
- **Root cause:** token/device 변경 후 `persist_best_effort()`를 호출하지만 write 오류는 warning log만 남기고 호출자에게 전달하지 않는다. 즉 메모리 변경 성공과 durable commit 성공이 분리되어 있다.
- **실패 시나리오:** disk full, read-only profile, Windows AV/backup tool의 일시 lock 등으로 auth JSON 저장이 실패한다. UI에서는 새 device pairing 또는 revoke가 성공한 것으로 보이지만 daemon 재시작 후 새 pairing은 사라지거나, revoke 전 파일이 다시 로드되어 폐기했다고 믿은 credential이 되살아날 수 있다.
- **수정 방향:** pairing/revoke를 durable transaction으로 처리한다. atomic write+fsync/rename 성공 후에만 success를 반환하고, 실패 시 structured persistence error를 반환한다. revoke는 최소한 durable revocation journal/WAL로 fail-closed 해야 한다.

## P03. 정상적인 auth 파일 I/O 실패가 daemon panic으로 승격된다

- **심각도:** Major
- **근거:** `src-tauri/src/remote/auth.rs:800-831`
- **Root cause:** `begin_transaction()`에서 `create_dir_all`, lock-file open, file lock에 `expect(...)`를 사용한다. `validate_token`, pair, revoke 등 runtime auth 경로가 이 함수를 통과한다.
- **실패 시나리오:** Windows에서 lock file이 보안 제품에 의해 잠겨 있거나, macOS/Linux에서 data-dir permission이 바뀌거나 파일시스템이 read-only가 되면 단순 인증 요청이 프로세스 panic을 유발할 수 있다. remote connection 문제 하나가 daemon 전체 가용성 문제로 확대된다.
- **수정 방향:** 모든 runtime filesystem/lock 오류를 `Result<AuthError>`로 전파하고 HTTP/IPC structured error로 변환한다. startup에서만 fatal로 볼 오류와 request-scoped 오류를 분리한다.

## P04. Desktop의 Add Machine pairing은 custom relay를 선택할 수 없고 public default를 강제한다

- **심각도:** Major
- **근거:** `ui/src/lib/pairedHostInventory.ts:6`, `ui/src/components/settings/AddMachineModal.tsx:249-257`, 반대편 incoming 설정은 `ui/src/components/settings/RemoteAccessSection.tsx:242-246`
- **Root cause:** Add Machine의 PIN pairing 요청은 `relayOrigin: DEFAULT_RELAY_ORIGIN`을 하드코딩한다. 반면 이 머신의 incoming remote gateway는 사용자 relay URL을 저장할 수 있어 inbound/outbound 설정 모델이 비대칭이다.
- **실패 시나리오:** machine B가 사설/self-hosted relay에 등록되어 PIN을 표시한다. machine A의 Add Machine에 그 PIN을 입력해도 요청은 `https://relay.checka.cc`로 전송되므로 B를 찾지 못한다. 사용자가 settings에서 custom relay를 이미 사용 중이어도 outbound pair에 반영되지 않는다.
- **수정 방향:** Add Machine에 relay origin 입력/선택을 제공하거나 `https://relay/#pair=...` 형식에서 origin을 추출한다. paired host별 relay origin을 native inventory에 canonical하게 저장하고 이후 연결에도 동일 origin을 사용한다.

## P05. Pairing 실패 원인이 renderer에서 boolean으로 소실되어 모두 같은 오류로 보인다

- **심각도:** Minor
- **근거:** `ui/src/lib/pairedHostInventory.ts:99-112`, `ui/src/components/settings/AddMachineModal.tsx:249-263`
- **Root cause:** inventory `pair()`가 native exception 종류를 모두 catch하여 `false`로 바꾸고, modal은 이를 단일 `PAIR_FAILED` 메시지로 표시한다.
- **실패 시나리오:** PIN 만료, 잘못된 relay, relay offline, machine grant 문제, stale generation, local daemon unavailable가 모두 동일한 pairing 실패로 보인다. 사용자는 재시도/새 PIN/relay 변경 중 무엇을 해야 하는지 알 수 없다.
- **수정 방향:** `{code,message,details,retryable}` structured IPC error를 그대로 renderer까지 보존하고 상태별 UX를 제공한다. boolean success API를 제거한다.

## P06. Paired terminal의 socket-ticket 발급 실패가 무조건 Authorization WebSocket fallback으로 숨겨진다

- **심각도:** Major
- **근거:** `src-tauri/src/paired_host/client.rs:315-390`, relay ticket 요구 경로 `src-tauri/src/remote/relay_server.rs:906-932`, direct gateway credential 경로 `src-tauri/src/remote/server.rs:1279-1308`
- **Root cause:** `attach_terminal()`은 `/api/v1/socket-ticket` 요청/HTTP status/body/JSON 오류를 모두 `None`으로 축약한 뒤 ticket 없는 WebSocket에 `Authorization` header를 붙여 재시도한다. 이 fallback은 Rust direct gateway에는 의미가 있지만 relay browser/reverse-WebSocket endpoint는 query ticket을 요구하므로 relay에서는 실패 원인을 가리는 두 번째 실패일 뿐이다.
- **실패 시나리오:** relay가 ticket endpoint에서 401/429/503을 반환한다. caller는 해당 상태를 받지 못하고 Authorization fallback WS가 다시 실패한 뒤 `HOST_UNAVAILABLE`류로 보게 된다. revoke, rate limit, relay outage의 구분이 사라져 recovery가 잘못된다.
- **수정 방향:** transport가 relay인지 direct인지 명시적으로 구분한다. relay에서는 ticket mint 실패를 typed error로 즉시 반환한다. direct fallback은 명시적 legacy capability/404에만 허용한다.

## P07. Native daemon attach 오류 분류가 여전히 literal error string에 의존한다

- **심각도:** Major
- **근거:** `src-tauri/src/daemon/client.rs:359-386`, `ui/src/lib/nativeTerminalAttachPolicy.ts:39-129`
- **Root cause:** Rust의 `parse_attach_error_response()`는 `Session '<id>' not found` 또는 `PTY session '<id>' not found` 문자열을 정확히 비교해야 `SESSION_NOT_FOUND`로 만든다. UI도 structured code가 불완전할 때 동일 문구를 다시 비교하는 legacy 분기를 유지한다.
- **실패 시나리오:** daemon 메시지 문구가 prefix 추가, 대소문자 변경, localization, 다른 PTY backend 표현으로 바뀌면 실제 dead session이 `INTERNAL_ERROR`/operational error가 된다. 자동 stale-session 정리와 재생성 정책이 작동하지 않는다.
- **수정 방향:** daemon wire response 자체를 `{code,message,details}`로 만들고 `SESSION_NOT_FOUND`를 source에서 발행한다. UI 정책은 code + typed details만 사용하도록 바꾸고 문자열 fallback은 호환 기간 후 제거한다.

## P08. Paired spawn/reattach 오류가 IPC 경계에서 `INTERNAL_ERROR(code-string)`로 축약된다

- **심각도:** Major
- **근거:** `src-tauri/src/ipc/terminal.rs:938-975`, `src-tauri/src/ipc/terminal.rs:990-1052`
- **Root cause:** `ClientError`가 가진 `code`, `machine_error`, `ambiguous`, `request_id`를 typed `IpcError`로 매핑하지 않고 `IpcError::internal(e.code)`로 바꾼다. reattach 실패도 같은 방식이다.
- **실패 시나리오:** 원격이 `SESSION_EXPIRED`, `PARENT_SESSION_MISMATCH`, `TIMEOUT`, `HOST_UNAVAILABLE`, `OPERATION_OUTCOME_UNKNOWN`을 정확히 반환해도 renderer는 `INTERNAL_ERROR`와 문자열만 받는다. retry 가능 여부와 ambiguity를 구분하지 못하고 세션을 잘못 폐기하거나 무의미한 재시도를 한다.
- **수정 방향:** machine/paired error code별 `IpcErrorCode`를 추가하고 `details`에 `requestId`, `ambiguous`, `machineError`, host/generation 정보를 보존한다. renderer는 code 기반 상태 머신만 사용한다.

## P09. Native↔daemon paired-host IPC timeout(35s)이 내부 remote mutation budget보다 짧아 ambiguity가 구조적으로 발생한다

- **심각도:** Major
- **근거:** `src-tauri/src/daemon/client.rs:435-457`, `src-tauri/src/daemon/client.rs:486-509`, paired mutation route `src-tauri/src/paired_host/client.rs:174-299`
- **Root cause:** `paired_host_request()` 전체가 35초 timeout인데, 코드 주석 자체가 inner HTTP mutation이 그보다 오래 걸릴 수 있음을 인정하고 mutation transport error를 `ambiguous=true`로 표시한다. handshake/capability 왕복까지 동일 35초에 포함된다.
- **실패 시나리오:** relay 지연으로 createSession/closeSession이 원격에서 35초 이후 완료된다. local IPC는 먼저 timeout으로 끝나고, 원격 side effect는 나중에 commit된다. caller는 실제 상태를 모른다.
- **수정 방향:** outer deadline을 inner 최대 budget + handshake margin보다 길게 맞추는 것만으로 끝내지 말고, mutation은 request-id journal reconciliation을 필수화한다. cancellation 여부와 ‘remote may have committed’를 명확히 모델링한다.

## P10. Ambiguous createSession을 reconciliation하지 않아 원격 PTY가 orphan될 수 있다

- **심각도:** Major
- **근거:** `src-tauri/src/ipc/terminal.rs:938-975`, ambiguity 생성 `src-tauri/src/daemon/client.rs:486-509`, 원격 journal/unknown 처리 `src-tauri/src/daemon/session_service.rs:319-370`
- **Root cause:** paired create에 client request id가 있지만 local spawn 경로는 ambiguous transport error가 나면 operation journal의 `Operation { request_id }`를 조회하지 않고 즉시 실패한다. 아직 remote target을 모르는 상태라 cleanup close도 하지 못한다.
- **실패 시나리오:** 원격 daemon이 shell을 생성하고 journal을 commit하는 동안 local 35초 deadline이 먼저 끝난다. UI에는 spawn 실패가 뜨지만 원격 PTY는 실행 중이고 local proxy/descriptor는 만들어지지 않는다. 반복 시 shell 누적/자원 누수가 발생한다.
- **수정 방향:** ambiguous create는 동일 request id로 journal을 조회해 Completed/Unknown/Pending을 reconciliation한다. Completed면 반환된 target에 reattach하고, 사용자 취소 상태면 그 target을 명시적으로 close한다. outcome을 모른 채 새 request id로 재생성하지 않는다.

## P11. Reattach 실패 후 cleanup `CloseSession`의 결과를 버려 cleanup 자체의 ambiguity를 복구하지 못한다

- **심각도:** Major
- **근거:** `src-tauri/src/ipc/terminal.rs:998-1052`
- **Root cause:** remote session 생성 후 proxy reattach가 실패하면 `CloseSession`을 보내지만 결과를 `let _ = ...await`로 폐기한다. close 역시 P09의 timeout 계층을 통과하므로 실패/ambiguous일 수 있다.
- **실패 시나리오:** create는 성공했고 reattach는 transient error로 실패한다. cleanup close가 network timeout을 맞거나 실제로 remote에서 늦게 commit한다. local은 어느 쪽인지 확인하지 않고 원래 reattach error만 반환하므로 remote shell이 남을 수 있고 이후 cleanup 재시도 근거도 없다.
- **수정 방향:** cleanup close에도 request id를 보존하고 journal reconciliation을 수행한다. cleanup 성공/unknown을 별도 structured result로 기록하며, unknown이면 bounded recovery/reaper가 재확인하도록 한다.

## P12. 일반 “terminal close”는 paired proxy만 detach하고 원격 PTY를 종료하지 않으며 production UI에 명시적 closeSession 경로가 없다

- **심각도:** Major
- **근거:** `src-tauri/src/ipc/terminal.rs:1577-1585`, `src-tauri/src/terminal/service.rs:274-289`, `src-tauri/src/terminal/paired_runtime.rs:157-173`; production usage 검색에서 `.closeSession(`은 `ui/src/lib/pairedDaemonProject.test.ts:145` 테스트만 확인됨
- **Root cause:** `daemon-session:*`의 `close_session()`은 `paired.detach()`만 호출한다. paired runtime 코드도 “Detach only; deliberately not remote session close”라고 명시한다. 그러나 production terminal-close UX에서 remote terminate와 local detach를 구분하는 경로가 연결되어 있지 않다.
- **실패 시나리오:** 사용자가 paired terminal 탭을 닫아 프로세스를 종료했다고 생각하지만 실제 remote PTY는 계속 실행된다. 여러 탭을 열고 닫으면 remote session inventory에 shell이 누적된다.
- **수정 방향:** UX/API에서 **Detach**와 **Terminate remote session**을 명시적으로 분리한다. 일반 close의 제품 의미가 종료라면 remote `CloseSession` + reconciliation을 연결하고, detach가 의도라면 지속 세션 manager에 종료 액션과 상태를 노출한다.

## P13. Paired proxy가 일시 단절로 종료되면 exact remote target descriptor도 같이 삭제되어 자동 reattach가 끊긴다

- **심각도:** Major
- **근거:** descriptor가 live owner에만 저장됨 `src-tauri/src/terminal/paired_runtime.rs:13-57`; receive/ping 오류 시 owner task 종료 및 reap 삭제 `src-tauri/src/terminal/paired_runtime.rs:73-111`; descriptor 조회는 live owner만 반환 `src-tauri/src/terminal/paired_runtime.rs:49-58`; proxy backend id는 one-way hash `src-tauri/src/remote/machine_protocol.rs:93-97`; renderer persistence는 paired session에 backend id만 유지 `ui/src/lib/sessionPersistence.ts:396-416`
- **Root cause:** durable identity라고 주석된 `Descriptor {host_id,generation,target,after_sequence}`가 runtime Owner에만 있고 owner task가 끝나면 map entry와 함께 사라진다. persisted `daemon-session:<sha256>`는 target을 역산할 수 없는 hash라 exact target을 복구할 수 없다.
- **실패 시나리오:** relay restart/짧은 network outage로 proxy `receive()`/keepalive가 실패하고 owner actor가 종료된다. renderer에는 기존 backendSessionId가 남아 `reconnecting`으로 복원되지만 daemon의 `paired_terminal_descriptor(id)`는 `None`을 반환한다. remote PTY가 살아 있어도 exact daemonEpoch/sessionId를 잃어 자동 reattach가 불가능하다.
- **수정 방향:** credential-free Descriptor를 durable native store 또는 daemon session metadata에 proxy id와 함께 저장한다. actor death와 descriptor ownership을 분리하고, reconnect 시 host generation/remote target을 재검증한 뒤 `after_sequence`부터 reattach한다. daemon restart 후에도 복구 가능한 형식이어야 한다.

## P14. Renderer paired-host inventory가 app-wide로 갱신되지 않아 generation/online/auth가 Settings 밖에서 stale해질 수 있다

- **심각도:** Major
- **근거:** `ui/src/lib/pairedHostInventory.ts:62-171`, `ui/src/components/settings/RemoteSection.tsx:175-183`, `ui/src/components/settings/PairedMachinesSection.tsx:105`
- **Root cause:** inventory는 startup에 한 번 refresh되고, 이후 자동 refresh는 Remote Settings component가 mount된 동안의 window focus 이벤트에 의존한다. native control-channel/generation 변경을 renderer로 push하는 app-wide subscription이 없다.
- **실패 시나리오:** app이 foreground인 상태에서 remote daemon 또는 relay가 재시작해 native generation이 바뀐다. 사용자가 Settings를 열지 않으면 renderer host는 계속 `online=true`/old generation으로 남고 paired project/worktree/session 요청은 stale generation으로 실패한다.
- **수정 방향:** native inventory generation/online/auth 변경 이벤트를 app root에서 subscribe한다. reconnect/forget/revoke/daemon restart 시 host entry를 원자적으로 갱신하고 in-flight callbacks를 generation fence로 취소한다.

## P15. Remote web `listWorktrees(workspaceId)`가 인자를 무시하고 현재 active workspace의 worktree만 반환한다

- **심각도:** Major
- **근거:** `ui/src/lib/remoteClient.ts:62-87`, 호출부 `ui/src/state/workspaceRuntime.ts:155-220`, server state는 active workspace만 계산 `src-tauri/src/remote/server.rs:798-953`
- **Root cause:** remote client의 `listWorktrees(_workspaceId)`는 요청한 workspace id를 사용하지 않고 `/api/v1/workspace/state`의 top-level `worktrees`를 반환한다. server의 top-level `worktrees`는 `active_ws`에 대해서만 생성된다.
- **실패 시나리오:** remote web client가 workspace B를 list하려는 시점에 desktop active workspace가 A이면 A의 worktree rows를 B 요청 결과로 소비할 수 있다. 프로젝트 전환/사이드바 캐시가 잘못 표시되거나 잘못된 worktree 선택으로 이어진다.
- **수정 방향:** `/api/v1/workspace/worktrees?workspaceId=...` 같은 명시적 endpoint를 제공하거나 workspace-state의 `projects[]`에서 requested id를 정확히 선택한다. `listWorktrees` 인자를 무시하는 API는 금지한다.

## P16. Workspace snapshot cache는 refresh interval이 지나도 stale snapshot을 먼저 반환한다

- **심각도:** Minor
- **근거:** refresh interval `src-tauri/src/remote/state.rs:517-518`, stale-while-revalidate 로직 `src-tauri/src/remote/state.rs:760-805`
- **Root cause:** registry revision이 같으면 snapshot age가 2초 이상이어도 background rebuild만 spawn하고 현재 요청에는 기존 snapshot을 즉시 반환한다. 외부 git/worktree 변화가 registry revision을 올리지 않는 경우 첫 조회는 의도적으로 stale하다.
- **실패 시나리오:** paired machine에서 CLI로 worktree를 생성/삭제한 뒤 remote client가 목록을 새로고침한다. refresh window가 지났어도 첫 응답은 이전 목록이고 background rebuild가 끝난 뒤 다음 요청에서야 반영된다.
- **수정 방향:** 명시적인 list/refresh 요청은 age 만료 시 rebuild를 await하거나 filesystem/event revision을 cache key에 포함한다. UI에는 stale snapshot 여부/revision을 제공할 수 있다.

## P17. Inactive paired worktree 조회 실패 시 sidebar가 기존 rows를 무기한 보존하며 stale 표시도 하지 않는다

- **심각도:** Minor
- **근거:** `ui/src/state/inactiveProjectWorktrees.ts:213-235`, paired loader `ui/src/state/pairedProjectWorktrees.ts:6-23`
- **Root cause:** paired list가 offline/stale generation/incomplete inventory로 `null`이 되면 reducer는 기존 `current[id]`를 의도적으로 보존한다. 데이터 손실 방지에는 유리하지만 row를 stale/offline로 표기하거나 TTL로 무효화하는 모델이 없다.
- **실패 시나리오:** remote host가 offline인 동안 다른 곳에서 worktree가 삭제된다. desktop sidebar에는 오래된 worktree가 계속 정상 항목처럼 남고 사용자가 클릭/삭제하면 뒤늦게 operation error를 받는다.
- **수정 방향:** cached rows에 freshness/generation 상태를 붙이고 stale/offline UI를 표시한다. reconnect 후 authoritative refresh로 교체하고, 오래된 actionable row는 비활성화한다.

## P18. Active-session focus watcher는 주석과 달리 `None` 전환에도 terminal socket을 닫아 reconnect churn을 만든다

- **심각도:** Minor
- **근거:** `src-tauri/src/remote/server.rs:1814-1840` (동일 패턴이 `src-tauri/src/remote/server.rs:2214-2235`에도 존재)
- **Root cause:** 최초 검사에서는 `Some(other)`일 때만 focus 이동으로 취급하고, 주석도 “no focused session id is NOT focus moved away”라고 명시한다. 그러나 `changed()` loop에서는 `current.as_deref() != Some(target)`이면 break하므로 `None`도 종료 조건이다.
- **실패 시나리오:** desktop selection 갱신 중 session id가 잠깐 `None`이 되면 정상 remote terminal WebSocket이 닫힌다. client가 exponential reconnect를 시작하고, selection이 안정화될 때까지 403/재접속 churn이 발생한다.
- **수정 방향:** loop도 initial check와 동일하게 `Some(current) if current != target`일 때만 종료한다. `None`은 유지/대기하고 selection generation을 사용해 진짜 focus 이동과 transient unset을 구분한다.

## P19. Direct LocalNetwork gateway는 TLS 없이 외부 LAN interface에 HTTP/WebSocket을 노출한다

- **심각도:** Major
- **근거:** plain `TcpListener` + `axum::serve` `src-tauri/src/remote/server.rs:2709-2744`, LocalNetwork/Tailscale 외부 interface bind `src-tauri/src/remote/server.rs:2750-2839`
- **Root cause:** direct gateway listener에 TLS layer가 없다. LocalNetwork mode는 사용자가 선택한 LAN interface에 동일 HTTP/WebSocket router를 직접 bind한다.
- **실패 시나리오:** 신뢰하지 않는 Wi-Fi/LAN에서 direct mode를 사용하면 pairing exchange, bearer-authenticated REST, socket-ticket, terminal stream이 transport-level 암호화 없이 전달된다. 같은 L2 구간 공격자가 passive capture/active MITM을 할 수 있다.
- **수정 방향:** LAN direct mode에 TLS를 제공하고 device pairing에서 certificate/public-key pinning을 설정한다. 최소한 HTTPS/WSS가 없는 non-loopback direct mode를 명시적 insecure opt-in으로 제한하고 강한 경고를 표시한다. Tailscale처럼 별도 암호화 overlay가 증명되는 모드와 일반 LAN을 구분한다.

## P20. Custom relay URL은 `http://`를 허용하고 control/data 채널을 자동으로 plaintext `ws://`로 바꾼다

- **심각도:** Major
- **근거:** UI 전달 `ui/src/components/settings/RemoteAccessSection.tsx:242-246`, backend URL 선택 `src-tauri/src/remote/server.rs:2778-2855`, scheme 변환 `src-tauri/src/remote/relay_client.rs:598-607`
- **Root cause:** relay URL의 HTTPS/WSS 요구가 없다. `http://`는 의도적으로 `ws://`로 변환된다.
- **실패 시나리오:** 사용자가 인터넷상의 self-hosted relay 주소를 `http://relay.example`로 입력한다. machine control channel, reverse data tunnel 및 HTTP credential exchange가 암호화되지 않은 경로를 사용한다. device token/terminal content confidentiality가 relay 앞 네트워크에서 보장되지 않는다.
- **수정 방향:** non-loopback relay는 `https://`/`wss://`만 허용한다. 개발용 insecure relay는 별도 flag와 localhost/RFC1918 제한을 사용하고 UI에 명시한다.

### Relay revoke cache에 대한 별도 확인

`src-tauri/src/remote/relay_server.rs:193,341-382,906-932`에서 relay가 validation 성공 device token을 30초 cache하여 revoke 직후에도 socket-ticket을 발급할 수 있다. 그러나 reverse tunnel 안쪽 gateway는 `src-tauri/src/remote/server.rs:1279-1308`에서 device token을 다시 `validate_token()`하므로, 현행 코드 기준으로는 revoke된 credential이 실제 terminal WebSocket admission을 통과하는 fail-open 근거는 찾지 못했다. 즉 이를 별도 보안 결함으로 과장하지 않았다. 다만 relay가 최대 30초 동안 결국 사용할 수 없는 ticket을 발행해 revoke UX가 지연/혼동될 수 있으므로 TTL 제거 또는 revoke invalidation push가 더 일관적이다.

---

## 현행 코드에서 확인되어 결함으로 제외한 항목

다음은 과거 취약점/우려가 현재 코드에서는 방어되고 있음을 확인했다.

1. **Relay control reconnect:** `src-tauri/src/remote/relay_client.rs:441-466`은 control channel 종료/오류 후 exponential backoff로 재접속한다.
2. **Pairing lease bound:** `src-tauri/src/remote/relay_client.rs:130-160`은 mirror 60초, machine scope 최대 600초로 상한을 둔다.
3. **Single-use pairing capability:** `src-tauri/src/remote/auth.rs:501-529`은 lookup/실패 accounting/consume을 동일 pairing-window write lock에서 수행한다.
4. **Pairing registration idempotence/generation:** relay registration은 generation과 동일 payload를 검증하고 재등록 충돌을 구분하는 코드가 존재한다.
5. **Auth ownership store RMW race:** auth transaction은 process mutex + 파일 lock을 사용하고 lock 획득 후 persisted state를 다시 읽는다(`src-tauri/src/remote/auth.rs:800-831`). P03의 panic 문제와는 별개로 lost-update 방어 자체는 존재한다.
6. **Canonical remote auth dir:** `FERRYX_DATA_DIR` 아래 remote authority를 별도 경로로 canonicalize하는 코드가 있어 과거 `/remote` suffix 혼선은 현행 결함으로 잡지 않았다.
7. **Permanent device token in browser WebSocket URL:** production remote UI는 `/api/v1/socket-ticket`으로 single-use ticket을 발급받아 URL에 넣는다. 테스트 전용 dummy-token branch는 production credential path가 아니다.
8. **daemonEpoch on native paired terminal:** `src-tauri/src/paired_host/client.rs:333-347`에서 paired descriptor의 daemon epoch와 after-sequence가 terminal socket query에 포함된다.
9. **ReplayGap decoding:** `src-tauri/src/terminal/paired_daemon.rs:104-136`은 binary replay frame의 gap을 hub에 publish하고 end sequence로 descriptor cursor를 갱신한다. 문제는 P13처럼 그 descriptor 자체의 생존성이다.
10. **Paired worktree sidebar 기본 지원:** `ui/src/state/inactiveProjectWorktrees.ts`와 `ui/src/state/pairedProjectWorktrees.ts`에 paired worktree listing 경로가 있어 “paired worktree가 아예 sidebar에서 숨겨짐”은 현행 코드로 입증되지 않았다.

---

## 우선 수정 순서

1. **세션 side-effect 정확성:** P09 → P10 → P11 → P12. timeout budget을 정리하고 create/close를 operation-journal reconciliation 기반으로 만든다.
2. **재접속/복구:** P13 → P14 → P18. descriptor를 durable하게 만들고 generation 이벤트를 app-wide push하며 focus watcher의 transient `None` 종료를 고친다.
3. **인증 durability:** P02 → P03 → P01. pairing/revoke durable commit, I/O panic 제거, relay claim rollback을 하나의 transactional model로 정리한다.
4. **transport/error contract:** P06 → P07 → P08 → P05. relay/direct를 명시적으로 구분하고 structured error만 사용한다.
5. **workspace correctness:** P15 → P16 → P17. requested workspace identity와 cache freshness를 authoritative하게 만든다.
6. **endpoint/security:** P04 → P19 → P20. outbound custom relay 지원과 TLS 정책을 함께 정리한다.

## 결론

현재 구현은 relay reconnect, ticketization, generation fencing, ReplayGap 등 핵심 building block은 상당 부분 갖추고 있다. 그러나 paired machine 연결에서 가장 위험한 부분은 **‘원격 mutation이 실제로 실행됐는지 모르는 상태’를 정상적인 timeout 조합이 만들어 내고, 상위 계층이 그 ambiguity를 끝까지 reconciliation하지 않는 것**이다. 이 때문에 네트워크가 느리거나 relay/daemon이 재시작되는 현실적인 상황에서 사용자는 단순 연결 오류를 보지만 원격에는 PTY가 남거나, 반대로 살아 있는 세션의 descriptor를 잃어 재접속하지 못할 수 있다. 이 문제군을 먼저 해결한 뒤 auth durability, structured errors, workspace cache, TLS/custom relay를 정리하는 것이 필요하다.
