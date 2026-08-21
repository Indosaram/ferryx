# rorca Remote Terminal — LAN / Tailscale 구현 계획

## 1. 목표

rorca 데스크탑에서 실행 중인 실제 PTY 터미널을 휴대폰·태블릿·다른 브라우저에서 안전하게 조회하고 조작할 수 있게 합니다.

1차 지원 방식은 두 가지입니다.

- **Local Network (LAN)**: 같은 신뢰된 로컬 네트워크에서 직접 접속
- **Tailscale**: rorca gateway는 loopback에만 열고 Tailscale Serve로 tailnet 내부 HTTPS 접속

중앙 relay 서버, 공개 인터넷 노출, Tailscale Funnel은 v1 범위에서 제외합니다.

---

## 2. 현재 rorca 기반

현재 native terminal contract에는 이미 다음 기능이 있습니다.

```text
cmd_terminal_spawn
cmd_terminal_write
cmd_terminal_resize
cmd_terminal_signal
cmd_terminal_close
cmd_terminal_list
terminal_output
terminal_lifecycle
```

Frontend에는 이미 다음 계층이 있습니다.

```text
ui/src/lib/tauri.ts
ui/src/components/TerminalPane.tsx
ui/src/lib/terminalRenderer.ts
```

따라서 원격 기능은 별도 shell/PTY 엔진을 새로 만들지 않고 기존 `PtyManager`와 workspace/worktree safety boundary를 재사용합니다.

핵심 원칙:

```text
Remote client
    ↓
RemoteGateway
    ↓
TerminalService
    ↓
PtyManager
```

다음 구조는 금지합니다.

```text
RemoteGateway
    ↓
직접 Command::new / 별도 PTY spawn
```

원격 기능이 기존 `WorkspaceRegistry`, canonical path 검증, writer lease, dirty-worktree 보호를 절대 우회하지 않도록 합니다.

---

## 3. 목표 아키텍처

```text
                           ┌──────── Desktop Tauri UI
                           │
PTY ─ TerminalService ─ OutputHub
                           │
                           └──────── RemoteGateway
                                      │
                     ┌────────────────┴────────────────┐
                     │                                 │
             Trusted LAN mode                  Tailscale mode
             explicit opt-in                  127.0.0.1 only
                     │                                 │
                  Browser                       Tailscale Serve
                                                       │
                                                     HTTPS
                                                       │
                                                 Phone/Browser
```

---

# Phase 1 — Terminal core 분리

## 4. TerminalService 추출

### 목적

현재 Tauri command가 직접 `PtyManager`를 조작하는 구조에서 공통 service 계층을 만듭니다.

예상 신규 파일:

```text
src-tauri/src/terminal/service.rs
```

예상 API:

```rust
TerminalService::list_sessions()
TerminalService::attach(session_id)
TerminalService::write(session_id, bytes)
TerminalService::resize(session_id, cols, rows)
TerminalService::signal(session_id, signal)
TerminalService::close(session_id)
```

기존 Tauri IPC도 이 service를 사용하도록 변경합니다.

### 완료 조건

기존 desktop terminal 동작이 완전히 동일해야 합니다.

- spawn
- input
- resize
- signal
- close
- lifecycle
- writer lease

기존 테스트는 수정 없이 계속 통과해야 합니다.

---

# Phase 2 — PTY output fan-out

## 5. TerminalOutputHub

현재 `cmd_terminal_spawn()`은 PTY output receiver를 하나의 task가 소비하여 Tauri event로 보냅니다.

현재 구조:

```text
PTY
 │
 ▼
mpsc receiver
 │
 ▼
Tauri event emitter
```

원격 attach를 지원하려면 여러 consumer가 동시에 같은 output을 받아야 합니다.

목표:

```text
                         ┌─ Desktop/Tauri subscriber
PTY ─ TerminalOutputHub ─┼─ Remote subscriber #1
                         ├─ Remote subscriber #2
                         └─ bounded replay history
```

예상 신규 파일:

```text
src-tauri/src/terminal/output_hub.rs
```

### 구현 조건

- `tokio::sync::broadcast` 또는 동등한 fan-out 구조
- 느린 remote client가 PTY reader를 block하지 않음
- subscriber lag 감지 가능
- session lifecycle 종료 시 hub cleanup
- output byte ordering 보존

---

## 6. bounded replay buffer

모바일이 나중에 attach해도 직전 터미널 내용을 볼 수 있어야 합니다.

권장 기본값:

```text
512 KiB ~ 1 MiB / session
```

구조:

```text
PTY output
   ├─ in-memory bounded ring buffer
   └─ live broadcast
```

attach 시 순서:

1. live subscription 확보
2. history snapshot 확보
3. history 전송
4. live stream 전환

중간 output loss/duplication을 테스트로 검증합니다.

### 보안 원칙

- 기본적으로 디스크에 terminal log 저장 금지
- 메모리 bounded history만 사용
- password/token 등의 terminal output이 영구 저장되지 않게 함

---

# Phase 3 — Remote Gateway

## 7. Rust remote module

예상 구조:

```text
src-tauri/src/remote/
├── mod.rs
├── server.rs
├── protocol.rs
├── auth.rs
├── state.rs
└── tailscale.rs
```

후보 dependency:

```text
axum
futures-util
tower-http
rand
sha2 또는 blake3
```

HTTP/WebSocket server는 `axum` 기반으로 구현하는 것을 우선 검토합니다.

---

## 8. v1 HTTP API

```text
GET  /api/v1/health
GET  /api/v1/sessions
POST /api/v1/pair/exchange
GET  /api/v1/devices
POST /api/v1/devices/:id/revoke
WS   /api/v1/terminal/:sessionId
```

### Session DTO

remote API에 native object를 그대로 노출하지 않습니다.

예:

```ts
type RemoteTerminalSession = {
  sessionId: string;
  title: string | null;
  projectId: string | null;
  worktreeLabel: string | null;
  running: boolean;
};
```

기본 응답에서는 absolute filesystem path를 노출하지 않습니다.

---

# Phase 4 — WebSocket terminal protocol

## 9. 세션별 WebSocket

Endpoint:

```text
/api/v1/terminal/{sessionId}
```

한 WebSocket은 한 terminal session에 attach합니다.

### Server → Client

PTY output은 WebSocket **binary frame**으로 전달합니다.

```text
raw PTY bytes
```

기존 Tauri event는 현재 Base64 contract를 유지합니다.

```text
Desktop: bytes → Base64 → terminal_output
Remote : bytes → WebSocket binary
```

### Client → Server

일반 입력은 binary frame:

```text
raw terminal input bytes
```

control message만 JSON text frame:

```json
{
  "type": "resize",
  "cols": 90,
  "rows": 30
}
```

```json
{
  "type": "signal",
  "signal": "interrupt"
}
```

```json
{
  "type": "ping"
}
```

v1 remote protocol에서는 다음 destructive operation은 제공하지 않습니다.

- kill
- arbitrary shell spawn
- worktree delete
- branch delete
- arbitrary cwd
- raw shell command execution endpoint

---

# Phase 5 — 인증 / pairing

## 10. 인증 모델

Tailscale을 사용해도 rorca 자체 device authorization을 둡니다.

```text
Tailscale authorization
        +
rorca device authorization
```

### 최초 pairing

Desktop Settings에서:

```text
Remote Access

[Pair new device]

QR CODE
Expires in 5 min
```

QR URL 예:

```text
https://host/#pair=<one-time-secret>
```

one-time secret은 URL query가 아니라 fragment 사용을 우선합니다.

Browser JS가 fragment를 읽고:

```text
POST /api/v1/pair/exchange
```

로 교환합니다.

성공 후 device credential/session 발급.

### 보안 요구사항

- one-time pairing secret
- 짧은 expiration
- constant-time token comparison 고려
- secret 평문 영구 저장 금지
- revoked device 즉시 차단
- authenticated client만 session list 조회 가능
- cookie 사용 시 HttpOnly / SameSite / Secure(Tailscale HTTPS) 검토

---

## 11. Device 권한

최소 2단계:

```text
view
control
```

### view

허용:

- session list
- attach
- output

금지:

- input
- Ctrl-C
- resize를 제외한 control operation

### control

허용:

- view 기능
- terminal input
- resize
- interrupt(Ctrl-C)

v1에서는 `Kill` 권한을 remote에 노출하지 않습니다.

---

# Phase 6 — LAN mode

## 12. Local Network

Settings:

```text
Remote Access

Mode
○ Off
○ Local Network
○ Tailscale
```

LAN mode는 명시적 opt-in일 때만:

```text
0.0.0.0:<configured-port>
```

로 listen합니다.

예시 기본 포트:

```text
43821
```

UI 표시:

```text
http://192.168.0.14:43821
[Copy URL]
[Show QR]
```

### LAN 안전 조건

- default OFF
- explicit trusted-LAN 경고
- pairing/auth 필수
- 무인증 WebSocket 금지
- CORS/origin 정책 제한
- rate limiting 적용
- listener 활성 상태 명확히 표시

LAN은 Tailscale보다 낮은 보안 수준으로 표시합니다.

---

# Phase 7 — Tailscale mode

## 13. 권장 구조

rorca gateway 자체는 network interface에 직접 공개하지 않습니다.

```text
rorca RemoteGateway
127.0.0.1:43821
       │
       ▼
Tailscale Serve
       │
       ▼
https://<device>.<tailnet>.ts.net
```

즉 Tailscale mode에서 listener invariant는:

```text
127.0.0.1 only
```

이어야 합니다.

---

## 14. 1차 Tailscale 연동: user-managed Serve

처음에는 rorca가 Tailscale 설정을 자동 변경하지 않습니다.

rorca Settings에서:

```text
Tailscale detected: Yes
Remote gateway: 127.0.0.1:43821

Run:
tailscale serve --bg 43821
```

을 안내하고 상태만 조회합니다.

조회 후보:

```text
tailscale status --json
tailscale serve status --json
```

### 장점

- 사용자의 기존 Serve 설정을 건드리지 않음
- 자동화 실수로 tailnet exposure 변경 위험 감소
- 구현/QA 범위 단순화

---

## 15. 2차 Tailscale 연동: managed Serve

user-managed mode가 안정화된 뒤:

```text
[Enable via Tailscale]
```

자동화 지원을 추가합니다.

### 원칙

- 기존 사용자 Serve config 보존
- rorca가 생성한 mapping만 추적/삭제
- `tailscale serve reset` 사용 금지
- 실패 시 기존 Serve 설정을 destructive하게 복원/초기화하지 않음
- executable missing / logged out / daemon unavailable 상태를 명시적으로 처리

### adapter 설계

```rust
trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, RemoteError>;
}
```

Production:

```text
SystemCommandRunner
```

Tests:

```text
FakeTailscaleRunner
```

테스트에서 실제 machine Tailscale 설정은 절대 수정하지 않습니다.

---

# Phase 8 — Frontend transport abstraction

## 16. TerminalPane의 Tauri 직접 의존 제거

현재 frontend는 대략 다음 API를 직접 사용합니다.

```text
spawnTerminal
writeTerminal
resizeTerminal
signalTerminal
listTerminalSessions
onTerminalOutput
onTerminalLifecycle
```

신규 구조:

```text
ui/src/lib/terminalTransport/
├── types.ts
├── tauriTransport.ts
└── remoteTransport.ts
```

공통 interface 예:

```ts
interface TerminalTransport {
  listSessions(): Promise<TerminalSession[]>;
  attach(sessionId: string): Promise<TerminalAttachment>;

  write(sessionId: string, data: Uint8Array): void;
  resize(sessionId: string, cols: number, rows: number): void;
  interrupt(sessionId: string): void;

  onOutput(
    sessionId: string,
    listener: (data: Uint8Array) => void,
  ): Unsubscribe;

  onLifecycle(
    listener: (event: TerminalLifecycle) => void,
  ): Unsubscribe;
}
```

Desktop:

```text
TauriTerminalTransport
```

Remote browser:

```text
WebSocketTerminalTransport
```

`TerminalPane`은 transport 종류를 몰라야 합니다.

---

# Phase 9 — Remote Web UI

## 17. 모바일/browser shell

예상 구조:

```text
ui/src/remote/
├── RemoteApp.tsx
├── PairingPage.tsx
├── RemoteSessionList.tsx
├── RemoteTerminal.tsx
└── remote.css
```

기존 `terminalRenderer.ts`와 xterm.js를 재사용합니다.

### Session list

```text
rorca
────────────────────
MacBook Pro
Connected via Tailscale

TERMINALS
● orca-lite · main
  Terminal 1

● kiwitalk · feature/mobile
  Terminal 2
────────────────────
```

### Terminal view

```text
┌────────────────────────┐
│ orca-lite / main       │
├────────────────────────┤
│                        │
│ $ cargo test           │
│ ...                    │
│ █                      │
│                        │
├────────────────────────┤
│ Ctrl Esc Tab ← ↑ ↓ →  │
└────────────────────────┘
```

---

# Phase 10 — 모바일 UX

## 18. 터치용 special-key bar

최소 지원:

```text
[Ctrl] [Esc] [Tab] [←] [↑] [↓] [→]
```

빠른 조합:

```text
Ctrl-C
Ctrl-D
```

### resize

- `ResizeObserver`
- xterm fit
- keyboard open/close 대응
- portrait / landscape 대응
- 최종 cols/rows를 remote gateway에 전송

---

# Phase 11 — PWA

## 19. Browser 설치 경험

Remote browser UI 안정화 후:

```text
manifest.webmanifest
service worker
PWA icons
```

추가.

목표:

- iPhone Safari 홈 화면 추가
- Android Chrome 설치
- standalone display
- reconnect UX

별도 iOS/Android native app은 v1 이후로 미룹니다.

---

# Phase 12 — Desktop Remote Access Settings

## 20. Settings UI

예:

```text
Remote Access

Remote access                  [ON]

Connection
● Tailscale
○ Local network

Status
Tailscale: Connected
Gateway: Running
URL: https://my-mac.example.ts.net

[Copy URL] [Show QR]

Permissions
Default new device: View only

Authorized devices
──────────────────────────────
iPhone        Control    [Revoke]
Galaxy        View only  [Revoke]
```

Native IPC 후보:

```text
cmd_remote_status
cmd_remote_enable
cmd_remote_disable
cmd_remote_pairing_create
cmd_remote_devices
cmd_remote_device_revoke
cmd_tailscale_status
```

---

# Phase 13 — 테스트 우선 전략

## 21. Native RED → GREEN

구현 전 최소 failing tests:

```text
terminal_output_fanout_reaches_desktop_and_two_remote_subscribers
slow_remote_subscriber_does_not_block_pty_output
attach_replays_bounded_history_then_live_output
remote_unknown_session_is_rejected
unauthenticated_session_list_is_rejected
expired_pairing_secret_is_rejected
revoked_device_cannot_reconnect
view_only_device_cannot_write_input
remote_terminal_input_uses_existing_session_not_raw_shell
tailscale_mode_binds_gateway_to_loopback_only
lan_mode_is_disabled_by_default
```

### 반드시 유지할 기존 safety coverage

```text
writer lease
PTY lifecycle
raw cwd rejection
dirty worktree deletion protection
unmerged branch protection
canonical workspace boundary
worktree isolation
```

---

## 22. Frontend RED → GREEN

```text
TerminalPane works through TerminalTransport
Tauri transport preserves existing desktop behavior
Remote transport connects WebSocket and receives binary output
Remote transport writes raw input bytes
Remote terminal reconnects after temporary disconnect
mobile Ctrl-C emits correct control action
view-only device cannot type
view-only device cannot interrupt
remote session list does not expose filesystem mutation operations
```

---

## 23. Tailscale adapter tests

Fake command runner로 다음 상태를 검증합니다.

```text
tailscale executable missing
tailscale daemon unavailable
logged out
connected
serve disabled
serve enabled
malformed status JSON
existing unrelated Serve mappings present
```

실제 `tailscale serve`를 CI/unit test에서 실행하지 않습니다.

---

# Phase 14 — 구현 순서

## 24. 권장 execution sequence

### Wave 1 — Core refactor

1. `TerminalService` 추출
2. 기존 Tauri IPC를 service 사용으로 변경
3. 기존 cargo/native tests GREEN

### Wave 2 — Output fan-out

4. `TerminalOutputHub`
5. bounded replay buffer
6. multiple subscriber tests
7. lagging client safety

### Wave 3 — Remote server

8. `remote/` module 생성
9. localhost health endpoint
10. session list
11. authenticated WebSocket attach
12. input / resize / interrupt

### Wave 4 — Auth

13. pairing secret
14. device credential
15. device revoke
16. view/control authorization

### Wave 5 — Frontend abstraction

17. `TerminalTransport` interface
18. `TauriTerminalTransport`
19. existing desktop TerminalPane regression
20. `WebSocketTerminalTransport`

### Wave 6 — Remote Web UI

21. Pairing page
22. Session list
23. xterm remote attach
24. reconnect/history
25. mobile special-key bar

### Wave 7 — Network modes

26. LAN opt-in listener
27. LAN URL/QR
28. Tailscale loopback mode
29. Tailscale status display
30. user-managed Serve instructions

### Wave 8 — Managed Tailscale (optional v1.1)

31. safe Tailscale adapter
32. rorca-owned mapping tracking
33. enable/disable UI
34. unrelated Serve config preservation tests

### Wave 9 — PWA / final QA

35. PWA manifest/service worker
36. iPhone Safari QA
37. Android Chrome QA
38. desktop + mobile simultaneous output QA
39. reconnect QA
40. final safety regression

---

# Phase 15 — Verification gates

## 25. Required automated gates

Native:

```text
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Frontend:

```text
bun run --cwd ui test
bun run --cwd ui build
```

변경 Rust/TS/TSX 파일에 LSP diagnostics 0건.

---

## 26. Real-device acceptance QA

### LAN

1. Desktop에서 Remote Access → LAN ON
2. 같은 Wi-Fi의 휴대폰으로 QR 접속
3. pairing
4. 기존 terminal attach
5. output 확인
6. mobile input 확인
7. Ctrl-C 확인
8. disconnect 후 desktop PTY가 살아 있는지 확인
9. reconnect 후 history replay 확인

### Tailscale

1. rorca gateway가 `127.0.0.1`에만 listen하는지 확인
2. Tailscale Serve를 통해 HTTPS URL 접속
3. Wi-Fi가 아닌 LTE/5G에서 tailnet 접속
4. pairing/auth 확인
5. terminal attach/input/output 확인
6. revoked device 재접속 실패 확인

---

# Phase 16 — 완료 기준

## 27. Functional acceptance

아래 시나리오가 전부 작동해야 합니다.

```text
Desktop terminal:
$ printf "hello-remote\n"

→ Desktop xterm에 출력
→ 연결된 Phone browser에도 동일 출력
```

```text
Phone:
$ echo phone

→ 같은 Desktop PTY에 입력됨
→ Desktop과 Phone 양쪽에 결과 출력
```

```text
Phone disconnect
→ PTY는 종료되지 않음

Phone reconnect
→ 같은 session attach
→ bounded recent history replay
→ live output 계속 수신
```

```text
View-only device
→ session/output 조회 가능
→ keyboard input 불가
→ Ctrl-C 불가
```

```text
Revoked / unauthenticated device
→ session list 접근부터 거절
```

```text
Tailscale mode
→ RemoteGateway = loopback only
→ Tailscale Serve HTTPS를 통해 접근
```

---

# Phase 17 — 명시적 비범위

v1에서 하지 않습니다.

- rorca 중앙 cloud relay
- 공개 인터넷 Funnel
- arbitrary remote shell command endpoint
- remote destructive worktree operations
- remote branch deletion
- remote `kill -9`
- automatic terminal transcript disk persistence
- full Ghostty SSH replacement
- 별도 native iOS/Android 앱

---

# Phase 18 — 주요 위험과 대응

## 28. 원격 shell 권한 위험

터미널 control은 사실상 데스크탑 사용자 권한으로 shell을 조작하는 기능입니다.

대응:

- pairing 필수
- device revoke
- view/control permission
- default remote access OFF
- LAN 명시적 opt-in
- Tailscale 우선

## 29. 느린 client backpressure

대응:

- remote subscriber가 PTY pump를 block하지 않는 broadcast 구조
- bounded queues
- lag detection

## 30. output history 민감정보

대응:

- memory-only
- bounded
- session 종료 시 삭제
- disk persistence 금지

## 31. Tailscale 설정 파괴 위험

대응:

- v1은 user-managed Serve 우선
- managed mode는 rorca-owned mapping만 조작
- `tailscale serve reset` 금지
- 실제 Tailscale 설정은 unit test에서 수정 금지

## 32. Desktop regression

대응:

- Tauri/Remote가 같은 `TerminalService`를 사용
- 기존 terminal/worktree safety tests 모두 필수 GREEN
- TerminalPane은 transport abstraction 뒤에서도 기존 desktop UX 유지

---

# 최종 권장 우선순위

구현 우선순위는 다음과 같습니다.

```text
1. TerminalService
2. OutputHub + replay
3. Authenticated RemoteGateway
4. Tauri transport regression
5. Browser remote UI
6. Tailscale mode
7. LAN mode
8. Mobile UX / PWA
9. Managed Tailscale Serve
```

제품 기본 추천 연결 방식은 **Tailscale**로 하고, LAN은 trusted-network용 보조 옵션으로 제공합니다.

최종 핵심 원칙은 하나입니다.

> 원격 기능은 기존 rorca terminal/worktree safety boundary를 우회하지 않고, 동일한 PTY 세션에 안전하게 attach하는 transport 계층으로 구현합니다.
