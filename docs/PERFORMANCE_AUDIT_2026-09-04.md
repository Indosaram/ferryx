# Ferryx 성능 병목 감사 보고서

- 작성일: 2026-09-04
- 대상: orca-lite (Tauri v2 데스크톱 앱 + Tokio 헤드리스 데몬 + React 프론트엔드)
- 방법: 코드 기반 정적 분석 (읽기 전용, 구현 없음). 프론트엔드 / Rust 백엔드 / IPC 데이터 경로 3개 영역을 독립 감사한 뒤 중복을 통합함.
- 주의: 실행 프로파일링(실측)이 아닌 코드 증거 기반 분석이다. 각 항목에 file:line 근거를 명시했다.

## 아키텍처 전제 (감사 중 확인된 사실)

- **xterm.js는 존재하지 않는다.** 데스크톱 터미널은 Rust/WGPU 네이티브 컴포지터가 렌더링한다. AGENTS.md의 `terminalOutputScheduler.ts` / `terminalHostManager.ts` 언급은 구형 문서이며 해당 파일은 존재하지 않는다 (PERFORMANCE_BOTTLENECK_AUDIT.md에서 이미 "rejected design"으로 기록됨).
- PTY 출력 경로: PTY → 64 KiB 블로킹 read (`terminal/session.rs:77`) → mpsc → `output_hub.publish` → 512 KiB 링 버퍼 + 브로드캐스트. 데몬은 별도 프로세스이며 UDS로 통신한다.
- **네이티브 데스크톱 모드에서 세션당 데몬 attach가 2개** 열린다:
  - Attach #1 (프론트엔드 IPC): `cmd_terminal_spawn` → `start_managed_pump` → 10ms/32KiB 코얼레서 → 20바이트 바이너리 프레임 → Tauri Channel → `terminalEventBus`
  - Attach #2 (네이티브 렌더): `cmd_native_terminal_attach` → 두 번째 `daemon_client.attach()` → `terminal.feed()` → GPU 렌더
- 데몬은 출력 청크를 **JSON + base64**로 직렬화해 attach당 한 번씩 소켓에 쓴다 (`daemon/server.rs:2248`, `daemon/protocol.rs` `base64_serde`).

---

## HIGH — 즉시 손봐야 할 병목

### H1. 네이티브 모드에서 소비자가 0인 PTY 출력 스트림을 이중 처리 (데몬 + 프론트엔드 양쪽 낭비)

- **위치:** `src-tauri/src/ipc/terminal.rs:689,734` (`start_managed_pump`), `ui/src/lib/terminalEvents.ts:207-330`
- **현재:** 데스크톱 네이티브 모드에서 세션당 데몬 attach가 2개 열리고, 데몬은 출력 100%를 JSON+base64로 **두 번** 직렬화한다. Attach #1의 바이트는 웹뷰로 전달되어 매 청크마다 (1) 전체 UTF-8 디코드, (2) OSC 타이틀 정규식 스캔 (`scanTerminalOscTitles`), (3) 세션당 512 KiB 롤링 백로그 유지/트림이 수행된다.
- **왜 병목인가:** 이 결과물의 소비자가 없다. `subscribeOutput`/`subscribeTitle`/`onOutput`의 비테스트 호출자가 0개이며(grep 검증), 타이틀과 에이전트 상태는 네이티브 이벤트(`native_terminal_title`, `workspaceStore.ts:315,348`)로 이미 별도 도착한다. 즉 대량 출력 시 렌더러 메인 스레드가 디코드+정규식+배열 churn + 512 KiB × 세션 수의 메모리를 **버리기 위해** 소비한다.
- **해결 방안:** 펌프를 opt-in으로. 출력/타이틀 리스너가 없으면 채널 등록을 건너뛰고(`ensureBinaryOutputChannel` 스킵), Rust 측에서도 네이티브 호스팅 세션에 대해 `start_managed_pump`를 시작하지 않는다(또는 "렌더러가 바이트를 원함" 플래그로 게이트). 리모트 경로는 별도 전송이 있으므로 영향 없음.
- **기대 효과:** 세션당 데몬 직렬화 CPU/소켓 대역폭/base64 작업 약 50% 절감 + 프론트엔드 메인 스레드의 per-chunk 작업과 512 KiB × N 힙 제거. **단일 최고 레버리지 항목.**

### H2. 데몬 소켓 구간에서 PTY 바이트가 base64-in-JSON으로 전송됨

- **위치:** `src-tauri/src/daemon/protocol.rs:62` (`base64_serde`), 직렬화 `daemon/server.rs:2248-2256`, 역직렬화 `daemon/client.rs:1044`. 프론트엔드 이벤트 경로도 base64: `ui/src/lib/terminalEvents.ts:255`, `ui/src/lib/terminalOutput.ts:20-30`
- **현재:** 모든 출력 청크가 base64(+33% 바이트) + JSON 이스케이프로 인코딩되어 소켓을 지나고, 앱에서 JSON 파싱 + base64 디코드된다. H1과 결합 시 바이트당 2회 수행. 프론트엔드 최악 경로는 `atob` + 문자 단위 JS 콜백(`Uint8Array.from(binary, char => char.codePointAt(0))`)으로 청크 전체에 O(n) JS가 메인 스레드에서 실행된다.
- **해결 방안:** 데몬 소켓에 길이-접두 **바이너리 프레이밍** 도입. 이미 검증된 20바이트 바이너리 프레임(`ipc/terminal.rs:470 encode_terminal_output_frame` / `terminalOutput.ts decodeTerminalOutputFrame`)이 코드베이스에 있으므로 이를 데몬 소켓에도 미러링. 최소한 프론트엔드에서는 `Uint8Array.fromBase64` 패스트 패스를 보장하고 문자 콜백 폴리시를 제거.
- **기대 효과:** 대량 출력 시(빌드 로그, `cat`, TUI 리페인트) 페이로드 33% 절감 + per-byte 인코드/디코드 제거.

### H3. 매 출력 청크마다 전체 화면 스냅샷 + 에이전트 감지 실행

- **위치:** `src-tauri/src/native_terminal/surface_host.rs:1243` → `take_native_terminal_events` (`:423-493`), `snapshot.rs:112` (`row_text`), `snapshot.rs:98-103` (`RenderSnapshot`)
- **현재:** 데몬 출력 펌프가 `Output` 청크마다 `take_native_terminal_events`를 **전역 sessions 뮤텍스를 잡은 채** 호출한다. `agent_reports_own_state == false`(기본값)인 세션은 무조건: (1) `render_snapshot()` — 전체 그리드 FFI 복사, 모든 `CellSnapshot`이 힙 `String` 소유, (2) 행마다 `String` 할당하는 `row_text` collect, (3) 전체 화면 대상 에이전트 감지 룰셋 실행.
- **왜 병목인가:** 출력 도착 경로에서 O(rows × cols) 할당 + 룰 평가가 청크마다 실행된다. 렌더 코얼레서(`RenderScheduleCoordinator`)는 GPU 프레임만 조절할 뿐 이 경로는 조절하지 않는다. 잔청크를 많이 뿌리는 프로세스에서 청크당 전체 그리드 스냅샷 1회 + 전체 화면 감지 1회가 모든 세션이 공유하는 뮤텍스 아래에서 돌아간다.
- **해결 방안:** 감지를 feed와 분리해 조절(throttle/coalesce): 최대 N ms마다 1회, 또는 렌더 코디네이터가 실제 프레임을 스케줄할 때만 실행. 마지막 화면 해시를 캐시해 가시 그리드가 변하지 않으면 감지를 건너뛰기. "커서 행/타이틀 변경 시에만" 게이트하는 것도 대안.
- **기대 효과:** per-chunk 핫 패스에서 전체 그리드 할당 + 룰 패스 제거.

### H4. 세션 지속화: 레이아웃 변경마다 전체 파일 read-modify-write + 이중 fsync

- **위치:** `ui/src/App.tsx:1017-1030` (`persistSessionStrict`), `:1108-1146` (디바운스 이펙트), `src-tauri/src/session/mod.rs:172-232` (`save_session_to_path`), `ui/src/lib/sessionPersistence.ts:32-259`
- **현재:** 저장 한 번이 (1) `loadSession()` — 전체 `session_state.json`(모든 워크스페이스) 디스크 읽기 + JSON 파싱 IPC 왕복, (2) 활성 워크스페이스 + 모든 parked `worktreeLayouts` 전체 재직렬화(`normalizeLayout` 레이아웃당 호출, `referencedSessionIds`/`persistedTerminalSessions` 전체 패스 2회, 다른 모든 워크스페이스 스프레드 병합), (3) `serde_json::to_string_pretty` + `write_all` + `file.sync_all()` + rename + **부모 디렉터리 fsync** 순으로 실행된다.
- **트리거가 매우 넓다:** 500ms 디바운스 이펙트의 deps가 `state.layout`, `state.worktreeLayouts`, `state.worktrees`, `state.activeWorktreePath`, **`state.sessions`**, `state.workspaceId`. pane 리사이즈(`SET_PANE_RATIO`), 포커스 변경(`FOCUS_PANE`)도 `state.layout`을 변경하고, `state.sessions`는 에이전트 상태 변동(working→waiting→done)마다 새 객체가 된다. 활성 에이전트 턴 중 500ms마다 전체 파일 read + 재직렬화 + write + fsync 2회가 반복된다. 추가로 `App.tsx:1123-1128`의 비디바운스 분기는 프로젝트 전환 시 즉시 전체 저장을 실행한다.
- **해결 방안:** (a) 마지막으로 로드한 `PersistedWorkspaceSession`을 ref에 캐시해 핫 패스의 `loadSession()` 재읽기 제거(직렬화 큐가 이미 쓰기를 직렬화하므로 인메모리 복사본은 자기 자신과 레이스하지 않음), (b) 이펙트 deps에서 `state.sessions`를 제거하고 저비용 파생 키(세션 id + backendSessionId + lifecycle 조인)로 대체 — 에이전트 상태 churn은 지속화 내용을 바꾸지 않음, (c) `to_string_pretty` → compact 직렬화, (d) 파일+부모 디렉터리 이중 fsync를 UI 레이아웃 자동저장 기준으로는 과도하므로 단일 durable write로 축소, (e) 순수 지오메트리 변경(ratio/focus)과 구조 변경(탭 추가/닫기)의 디바운스를 분리, (f) parked `worktreeLayouts` 항목은 레이아웃 identity 키로 `serializeLayout` 메모이즈.
- **기대 효과:** 인터랙티브 레이아웃 경로에서 전체 파일 디스크 읽기 + fsync 2회 제거.

### H5. `switchDebug`가 키스트로크/마우스/리사이즈마다 동기 console.info + 직렬화된 Tauri IPC

- **위치:** `ui/src/lib/switchDebug.ts:53-80`, 호출부는 `NativeTerminalPane.tsx`에만 33곳 (`:705`, `:1057`, `:1703`, `:1427`, `:1505` 등), `App.tsx:653-676`
- **현재:** `import.meta.env.DEV` 또는 `VITE_SWITCH_DEBUG=1`이면 활성. 로거가 enabled 체크를 날리기 **전에** 호출부가 인자 객체를 eager하게 구성한다: 키다울때마다 문자열 concat/slice, 입력 전송 시 `Array.from(text).map(codePointAt → hex).join(",")`(대형 붙여넣기에 O(n) per-codepoint 문자열 할당), 포인터다운 시 `getBoundingClientRect()`(강제 레이아웃), ResizeObserver 틱마다 bounds 로깅. 각 엔트리는 단일 전역 promise 체인(`sinkTail`)에 직렬화된 IPC 왕복 1회.
- **영향:** 개발 환경에서는 입력 핫 패스에 직접 걸리는 HIGH. 릴리스 빌드에서는 `VITE_SWITCH_DEBUG=1`을 설정하는 빌드 스크립트가 없어 실질적으로 비활성(검증됨) — MEDIUM.
- **해결 방안:** 호출부에서 enabled 체크를 먼저 하거나 `switchDebug("...", () => ({...}))` 형태의 thunk로 변경해 비활성 시 인자 구성 비용을 0으로. hex dump 제거 또는 상한 설정. IPC 싱크를 idle 콜백 배치 flush로 전환.

### H6. `[profile.release]` 부재: LTO 없음, codegen-units 16, 심볼 미제거

- **위치:** `src-tauri/Cargo.toml` 전체 (`[profile` 검색 결과 없음; 워크스페이스 루트 Cargo.toml / `.cargo/config.toml`에도 없음)
- **현재:** 릴리스 빌드가 cargo 기본값: `opt-level=3`이지만 `lto=false`, `codegen-units=16`, `panic="unwind"`, 심볼 미제거. 의존 트리가 크고 크레이트 경계를 넘는 핫 패스(tokio full, axum, reqwest+rustls, wgpu, portable-pty, objc2-*, rodio, notify)가 많은데 LTO 없이 16 codegen unit이면 PTY→브로드캐스트→프레이밍→채널 핫 패스의 크로스-크레이트 인라이닝이 불가능하고, 출시 바이너리에 디버그 심볼이 남는다.
- **해결 방안:**
  ```toml
  [profile.release]
  lto = "thin"        # 최대치는 "fat" (링크 느림)
  codegen-units = 1
  strip = "symbols"
  panic = "abort"     # unwind 캐치에 의존하는 곳이 없는지 확인 후
  ```
  (`panic = "abort"`만 별도 판단 필요)
- **기대 효과:** 바이트 복사/브로드캐스트 경로의 상시 CPU와 바이너리 크기/시작 시간 개선. 한 파일 변경으로 광범위한 이득.

### H7. 데몬 클라이언트의 단일 전역 연결 뮤텍스 — 제어 평면 head-of-line 블로킹

- **위치:** `src-tauri/src/daemon/client.rs:604-627` (`send_request` — 전체 요청→응답 왕복 동안 `self.connection.lock().await` 보유), `write_terminal` (`:1082`)
- **현재:** attach를 제외한 모든 요청(Write 키스트로크, Resize, Ping, ListSessions, Spawn, remote 호출)이 단일 연결 + 단일 `tokio::sync::Mutex`를 공유하며 락이 전체 왕복(최대 15초 타임아웃) 동안 유지된다.
- **왜 병목인가:** 느린 요청 하나가 뒤따르는 모든 요청을 막는다. 최악은 `Spawn`: 데몬 측에서 `spawn_lock` + 블로킹 `fs::canonicalize` + git 해석 + PTY spawn(M10) 동안 이 클라이언트 뮤텍스가 잠겨 있어, spawn 뒤에 큐잉된 키스트로크 Write가 spawn 완료까지 대기한다. attach 스트림은 별도 전용 스트림(`client.rs:1037`)이라 스트리밍은 영향 없지만 입력 쓰기는 영향을 받는다.
- **해결 방안:** 연결 위에 요청 파이프라이닝(요청 쓰기 + 매칭 응답용 oneshot 큐잉, 읽기 측 분리) 또는 단기 요청용 소규모 연결 풀.
- **기대 효과:** 동시 spawn/resize 활동 중 인터랙티브 지연 제거.

---

## MEDIUM — 구조적 개선 항목

### M1. `publish_with_read_timestamp`가 청크를 발행당 최대 3회 복사

- **위치:** `src-tauri/src/terminal/output_hub.rs:335-351`, `BoundedBuffer::push_with_read_timestamp` (`:88-115`)
- **현재:** PTY 출력 청크(최대 64 KiB)마다 (1) 링 유지용 `push_back(chunk.clone())` (`:104`), (2) 시퀀스 브로드캐스트 `hub.sender.send(chunk.clone())` (`:346`), (3) 레거시 raw 브로드캐스트 `hub.raw_sender.send(chunk.bytes.clone())` (`:348`) — 세션 `RwLock` 쓰기 가드 아래에서 최대 3회 전체 복사. 특히 raw 채널의 유일한 소비자는 바이트를 버리고 종료 신호만 기다린다(`daemon/server.rs:2000-2003`).
- **해결 방안:** 청크 바이트를 `Arc<[u8]>`(또는 `bytes::Bytes`)로 저장해 링 유지와 두 브로드캐스트가 하나의 할당을 공유. raw 채널의 close 감지는 `watch`/oneshot으로 대체하거나 데몬 경로에 구독자가 없으면 채널 자체를 제거.
- **기대 효과:** 상시 출력 경로에서 청크당 복사 2회 제거.

### M2. PTY read 루프의 read당 `to_vec()` 할당

- **위치:** `src-tauri/src/terminal/session.rs:76-89`
- **현재:** 블로킹 리더가 재사용하는 64 KiB 스택 버퍼로 읽은 뒤 `reader_tx.blocking_send(buf[..n].to_vec())` — read syscall마다 새 힙 할당 + 복사.
- **해결 방안:** owned-`Vec` 채널에서는 불가피하지만, M1의 `Arc<[u8]>`와 결합하면 이 할당이 청크 전체 수명(read → 링 → 브로드캐스트 → 프레이밍)의 **유일한** 할당이 된다. 또는 풀링/`Bytes` 기반 버퍼 사용.
- **기대 효과:** 고처리량에서 allocator 압력 절감 (M1과 함께 실현).

### M3. attach마다 링 버퍼 전체 재구축 + 세그먼트 이력 이중 패스

- **위치:** `src-tauri/src/terminal/output_hub.rs:135-141` (`snapshot`), `subscribe` (`:395`), `snapshot_after_segmented` (`:150+`), `segment_history` (`:216-289`)
- **현재:** `snapshot()`이 최대 512 KiB 전체를 할당해 매 청크 `extend_from_slice`(읽기 가드 아래). `subscribe_with_sequence`는 `segment_history`를 추가로 실행해 `first_chunk.bytes` 클론 + 전체 청크 재누적 — 이력 전체에 대한 2차 패스/복사. 신규 attach나 갭 후 재접속마다 최대 512 KiB 재구축 비용.
- **해결 방안:** 풀 리플레이 경로는 `history`를 한 번만 구축하고 세그먼트는 해당 단일 버퍼의 오프셋 슬라이스로 유도. 세그먼트 버퍼 `with_capacity` 사전 할당.
- **기대 효과:** pane 분할/복원, 재접속 스톰 시 attach 지연 개선.

### M4. `segment_history`의 중첩 필터로 인한 O(chunks × ledger)

- **위치:** `src-tauri/src/terminal/output_hub.rs:216-289`
- **현재:** 리플레이 대상 각 청크마다 전체 `resize_ledger`(최대 4096개)를 재스캔하는 필터 + 시작/후행 포인트 별도 스캔. 최악 이차 복잡도, attach 크리티컬 섹션에서 실행.
- **해결 방안:** 청크와 ledger가 모두 시퀀스 정렬이므로 단일 전진 커서로 merge-walk (O(chunks + ledger)).
- **기대 효과:** 리사이즈 포인트가 많이 누적된 세션에서 attach 비용 절감.

### M5. 데몬 출력 펌프가 청크마다 flush — BufWriter 묠뜰화

- **위치:** `src-tauri/src/daemon/server.rs:2194` (`pump_sequenced_stream_with_agent_state`), 출력 분기 `:2244-2262`
- **현재:** 소켓을 `BufWriter`로 감싼 뒤 청크마다 `serde_json::to_string`(새 `String` 할당 + base64) → `write_all` → **`flush().await`**. 청크당 write syscall 1회가 강제되고 버퍼링이 무의미해진다. 네이티브 렌더 펌프(`surface_host.rs:1231`)는 데몬 측 코얼레싱이 전혀 없고(10ms/32KiB 코얼레서는 프론트엔드 attach #1에만 존재), 원격 WS 펌프는 프레임마다 flush하지 않고 그리드는 33ms 코얼레싱까지 한다(`remote/server.rs:911-919`) — 더 나은 패턴이 이미 코드베이스에 존재.
- **해결 방안:** `rx`에 즉시 읽을 다음 청크가 없을 때만 flush(drain-then-flush) 또는 짧은 타이머 기반. 반복 간 스크래치 `String`/`Vec` 재사용으로 per-frame 할당 제거. 인터랙티브 지연 보존을 위해 큐 drain 시에는 반드시 flush.
- **기대 효과:** 대량 출력 시 syscall/직렬화 호출을 배치 배수만큼 절감.

### M6. 네이티브 attach가 복원 시 항상 전체 512 KiB 링 리플레이 (× H1의 이중화)

- **위치:** `src-tauri/src/ipc/native_terminal.rs:447` (`daemon_client.attach(&session_id, None)` — 항상 시퀀스 0부터), `ui/src/lib/sessionPersistence.ts` (`lastOutputSequence`는 지속화하지만 네이티브 attach가 무시)
- **현재:** 레이아웃 복원 시 살아있는 세션의 `NativeTerminalPane`이 마운트되며 전체 512 KiB 스크롤백을 base64/JSON으로 받아 첫 프레임 전에 VTE 파싱. H1의 attach #1도 리플레이하므로 세션당 최대 2×512 KiB. 첫 프레임까지의 시간이 세션 수 × 512 KiB에 비례.
- **해결 방안:** 지속화된 `lastOutputSequence`를 네이티브 attach에 전달해 증분 리플레이(데몬은 이미 `after_sequence`와 갭 감지 지원). H1 수정으로 리플레이도 1회로 통합.
- **기대 효과:** 복원 세션 수가 많을수록 커지는 시작 지연 절감.

### M7. 전역 `spawn_lock`이 블로킹 FS/git 작업과 `.await`를 가로질러 유지됨

- **위치:** `src-tauri/src/daemon/server.rs:1853` → `handle_spawn` 종료까지 (`:1832-2019`)
- **현재:** 프로세스 전역 `tokio::sync::Mutex<()>` 하나가 **모든 워크스페이스의 모든 spawn**을 직렬화. `fs::canonicalize`(`:1925`), git 기반 `resolve_terminal_target`(`:1908`), `terminal_service.spawn_in_worktree` 등 동기 블로킹 호출을 async 런타임 위에서 락을 잡은 채 실행(`spawn_blocking` 없음).
- **왜 병목인가:** 시작 시 여러 세션 복원 같은 동시 spawn이 직렬로 하나씩, 각각 블로킹 canonicalize/git 지연을 순차 지불. H7과 결합해 클라이언트 측 head-of-line 블로킹 연장.
- **해결 방안:** 락 범위를 idempotency-cache/claim 크리티컬 섹션으로 축소하고 canonicalize/git 해석은 락 밖(또는 `spawn_blocking`)에서 수행 후 삽입 전 짧게 락을 다시 잡아 재확인. **주의:** 이 락이 idempotency/claim 레이스를 방어하므로 설계 리뷰 필요 — 기계적 변경 금지.
- **기대 효과:** 병렬 spawn, 런타임 스톨 제거.

### M8. 백엔드 복구 시 N+1 터미널 spawn IPC

- **위치:** `ui/src/state/workspaceStore.ts` `ensureSessionBackends` (`Promise.all(targets.map(... services.spawnTerminal(...)))`), `App.tsx:853`
- **현재:** 백엔드를 잃은 세션 M개에 대해 `spawnTerminal` invoke를 M회 개별 발행(동시성은 있지만 per-invoke IPC 직렬화 + 백엔드 spawn 경쟁은 그대로). 배치 명령이 없음. 다중 pane 복원 시 콜드 스타트 경로에서 M회 왕복 버스트.
- **해결 방안:** 배치 IPC 명령(`cmd_terminal_spawn_batch`)으로 spawn 요청 배열을 한 번에 받아 backend session id 배열 반환 + `REBIND_SESSION_BACKEND` 배치 디스패치.
- **기대 효과:** 시작 invoke M회 → 1회.

### M9. pane 분할마다 직렬 IPC 2회 (getTerminalCwd → spawn)

- **위치:** `ui/src/state/workspaceStore.ts` `splitPane`
- **현재:** 분할마다 `getTerminalCwd(backendSessionId)` 왕복 후 `spawnTerminal` 왕복이 직렬로 실행. cwd 조회가 spawn을 블로킹하는 동기 의존.
- **해결 방안:** `spawnTerminal`에 `inheritFromSessionId` 파라미터를 추가해 백엔드가 cwd를 서버 측에서 해석, invoke 1회로 통합.
- **기대 효과:** 분할당 IPC 왕복 절반.

### M10. 원격 그리드 diff가 매 프레임 전체 라인 재계산 + 클론

- **위치:** `src-tauri/src/remote/mirror.rs:96-160` (`frame_from_snapshot`), `build_runs` (`:~185`), 프레임 간격 `server.rs` 33ms
- **현재:** `render=grid` 원격 경로가 33ms마다 전체 그리드의 모든 셀에 `build_runs`를 실행해 `current_lines` 전체를 재구축하고, diff 경로도 프레임마다 전체 `Vec<Vec<RemoteGridRun>>` 할당 + 변경 라인의 runs 클론 + `last_lines` 전체 사본 유지. 비용이 변경 셀 수가 아닌 cols×rows에 비례.
- **해결 방안:** 엔진에서 dirty row를 추적해 변경 행에만 `build_runs` 실행, 변경 없는 라인은 `Arc` 공유로 베이스라인 클론 제거.
- **기대 효과:** 원격 그리드 클라이언트를 호스팅하는 데스크톱의 per-frame CPU/할당 절감(30fps 상한이 있어 지연 cliff는 아님).

### M11. 알림 디스패치마다 미캐시 블로킹 FFI 권한 조회

- **위치:** `src-tauri/src/notification/service.rs:82-83` (`dispatch` → `permissions.status()`), macOS 구현 `notification/permission.rs:173-201` (`query_settings`)
- **현재:** 알림마다 `permissions.status()` 호출. macOS에서는 `getNotificationSettingsWithCompletionHandler` 후 호출 스레드를 `rx.recv_timeout(CALLBACK_TIMEOUT = 5s)`로 **블로킹**. 메모이제이션 없음. 알림 센터가 느리면 최대 5초 블로킹.
- **해결 방안:** 마지막 권한 상태를 짧은 TTL로 캐시하고 비동기 갱신. stale-but-authorized는 디스패치 가능으로 처리. 명시적 권한 플로우에서만 fresh 조회 강제.
- **기대 효과:** 알림당 블로킹 FFI 홉 제거.

### M12. store 디스패치마다 리듀서 2회 실행 + 전역 캐시 2개 쓰기

- **위치:** `ui/src/state/workspaceStore.ts:240-250`
- **현재:** `dispatch`가 `workspaceReducer`를 먼저 직접 실행(reduce #1)해 `stateRef`와 `setHmrWorkspaceState`(window 전역 + `import.meta.hot.data` 양쪽 쓰기), `setWorkspaceSnapshot`(모듈 Map 쓰기)을 갱신한 뒤 `reactDispatch(action)`으로 React 안에서 **같은 리듀서를 다시** 실행(reduce #2). `SET_WORKTREES`는 세션/activity/모든 parked 레이아웃 재구축 + stale 탭마다 `layoutReducer`, `CLOSE_TAB`은 `isSessionReferencedOutsideTab` 경유 O(tabs² × leaves) — 이 비용이 디스패치마다 2배(dev StrictMode에서는 4배).
- **해결 방안:** precomputed `nextState`를 React에 넘겨 재리듀스를 제거하거나, eager 갱신을 `reactDispatch` 결과 기반 `useEffect`로 이동. 단, async 콜백(`openTab`, `splitPane`)이 디스패치 직후 `stateRef.current`를 읽으므로 동기 freshness 요구사항은 유지 필요.
- **기대 효과:** 무거운 액션의 디스패치 비용 절반.

### M13. store 셀렉터가 전체 state 객체에 키잉되어 매 액션마다 재계산 + 일부 이차 복잡도

- **위치:** `ui/src/state/workspaceStore.ts:1067-1083`, 셀렉터 `:1180-1330`
- **현재:** `agents`, `tabActivity`, `worktreeActivity`, `activityNotificationTargets` 4개 `useMemo`가 전부 `renderedState` 전체에 키잉. 리듀서의 모든 분기가 새 최상위 객체를 반환하므로 `SET_PANE_RATIO`/`FOCUS_PANE` 같은 무관한 액션에도 4개 전부 재계산. `selectActivityNotificationTargets`는 activity 항목마다 `findTabIdForSession` → 탭 전체 순회 + `getTabSessionIds`(parked `worktreeLayouts` 전체 스캔 가능) = O(activities × tabs × parkedLayouts). `selectWorktreeActivitySummariesAcrossWorkspaces`는 캐시된 **모든 워크스페이스 스냅샷**에 전체 셀렉터 실행 — N개 프로젝트 세션은 액티브 프로젝트의 모든 액션에서 N배 비용.
- **해결 방안:** 각 memo를 실제로 읽는 슬라이스(`activityBySessionId`, `layout`, `unreadTabIds`, `sessions`, `worktrees`)에 키잉. `sessionId → tabId` 인덱스를 레이아웃 변경 시 1회 구축해 재사용. parked 워크스페이스는 스냅샷 identity로 메모이즈.
- **기대 효과:** 프로젝트 수 × 탭 수에 비례해 커지는 상시 재계산 비용 절감.

### M14. `normalizeLayout`이 `TerminalSplitView` 렌더마다 전체 트리 워크

- **위치:** `ui/src/components/TerminalSplitView.tsx:211`, `ui/src/state/layout.ts:598,682,865`
- **현재:** fast path(`isNormalizedLayoutState`) 판정 자체가 전체 검증 패스: 탭 id Set 구축 + 모든 그룹/탭 순회 + 탭마다 전체 pane 트리 워크(`collectLeafIds`) — 매 렌더. fast path 실패 시 전체 재구축. `TerminalSplitView`는 워크스페이스 상태 변경마다 리렌더되므로 activity flap, 리사이즈 드래그, 포커스 변경에 모두 실행. `collisionDetection`이 `[normalizedLayout]`에 메모되어 새 객체 시 dnd-kit collision detector도 재구축.
- **해결 방안:** 경계에서 메모이즈: `useMemo(() => normalizeLayout(layout), [layout])` (`layoutReducer`는 이미 정규화된 상태를 반환). 더 나은 방법: 정규화 상태에 non-enumerable 브랜드를 달아 `isNormalizedLayoutState`를 단락.
- **기대 효과:** 렌더당 전체 트리 워크 제거.

### M15. 마운트된 터미널 pane마다 document.body 서브트리 MutationObserver 1개

- **위치:** `ui/src/lib/nativeTerminalVisibility.tsx:43-64`, 소비처 `NativeTerminalPane.tsx:488`
- **현재:** hook 인스턴스마다 `MutationObserver`를 `document.body`의 `childList+subtree`에 설치하고, 콜백은 `document.querySelectorAll('[role="dialog"], [role="search"], [role="menu"]')` + 매치마다 `closest` 실행. 4-pane 분할이면 observer 4개가 **문서 전체의 모든 DOM 변경**(React 커밋, 토스트, 탭바 갱신, dnd-kit 드래그 오버레이 churn)마다 전체 문서 쿼리를 pane 수만큼 곱해 실행.
- **해결 방안:** 모듈 레벨 단일 구독으로 승격 — observer 1개 + 캐시된 boolean + `useSyncExternalStore`로 fan-out. 관찰 범위를 전용 포털 루트로 한정하거나 `update`를 마이크로태스크로 디바운스해 React 커밋의 다수 mutation을 1회 쿼리로 합산.
- **기대 효과:** pane 수에 비례하는 mutation 콜백 비용 제거.

---

## LOW — 비용이 작거나 조걶부

### L1. pane당 `useTerminalSettings()` 인스턴스 — 강제 IPC(캐시 우회) + 리스너 2개씩, 필요한 건 boolean 1개
- `NativeTerminalPane.tsx:497`, `ui/src/lib/terminalSettings.ts:222-266`. pane마다 `refreshNativePreferences(true)`가 모듈 캐시를 우회해 IPC 재호출, window 커스텀 이벤트 + `storage` 리스너 각각 등록, `localStorage` 읽기, 같은 background 값에 대한 `documentElement.style.setProperty` 쓰기 — 사용 필드는 `copyOnSelect` 하나. 해결: `useCopyOnSelect()` 같은 좁은 hook을 단일 모듈 스토어(`useSyncExternalStore`)로 제공하고, 전체 hook(강제 갱신 포함)은 `App`/`SettingsDialog`만 소유.

### L2. 네이티브 스크롤바 이벤트가 스크롤 중 프레임 레이트로 React state 갱신
- `NativeTerminalPane.tsx:571-575`, `:929-1008`. 매 이벤트가 새 객체로 `setScrollbar` → pane 리렌더 + `scrollToTrackPosition` 클로저 재생성 → `window`의 `pointermove`/`pointerup`/`pointercancel` 리스너가 **스크롤바 틱마다 해제/재등록**. 해결: scrollbar를 ref + `useSyncExternalStore`로 옮겨 thumb 서브트리만 리렌더, 포인터 리스너는 pane 수명 동안 1회 등록.

### L3. 전역 에이전트 상태 브로드캐스트를 구독자마다 필터링
- `daemon/server.rs:2213-2237`. attach된 스트림 펌프 N개가 전역 `agent_state_tx`(용량 64)를 구독해 session_id 문자열 비교로 필터 — 보고 M건에 O(N×M) wakeup. 저빈도 이벤트라 실비용은 작음. 해결: 세션별 채널 또는 session_id 키 맵으로 라우팅.

### L4. 원격 이벤트 WebSocket 재접속에 백오프 없음
- `ui/src/lib/remoteClient.ts:112-127`. close 시 고정 3초 재접속, 지터/지수 백오프 없음 — 서버 다운 시 모든 원격 클라이언트의 3초 드럼비트. 해결: 지터 포함 지수 백오프, 상한 ~30초.

### L5. 원격 출력 프레임의 청크당 JSON 메타데이터
- `src-tauri/src/remote/server.rs:104-130` (`encode_remote_terminal_frame`). 출력 청크마다 `serde_json::to_vec(&RemoteTerminalFrameMetadata)` + OSC-777 접두/종결. 비그리드 원격 경로만 해당. 해결: 출력 프레임은 컴팩트 바이너리 헤더(1바이트 kind + varint sequence), JSON은 리플레이/갭 프레임에만.

### L6. 데드 코드: 미사용 `WebSocketTerminalTransport`
- `ui/src/lib/terminalTransport/remoteTransport.ts`. `src/` 어디에서도 인스턴스화되지 않음(grep 검증). 사용 예정이라면 (a) `TextEncoder` 공유로 승격, (b) 서버가 붙이는 OSC-777 메타데이터 헤더 파싱 누락 수정 필요 — 아니면 삭제. **현재 도달 불가이므로 추정 라벨.**

### L7. `App.tsx`의 `workspace.render` 디버그 이펙트가 매 상태 변경에 할당
- `App.tsx:653-676`. `tabIds: .map()`, `sessionCount: Object.keys()`를 enabled 체크 전에 구성(H5와 같은 근본 원인). 해결: 호출부 가드.

### L8. Vite dev 서버 파일시스템 폴리ing 100ms
- `ui/vite.config.ts:24-28` (`usePolling: true, interval: 100`). 감시 트리를 초당 10회 walk하는 상시 dev CPU drain. macOS에서는 네이티브 FSEvents가 더 저렴. 해결: `usePolling` 제거 또는 필요 환경 한정 env var로 게이트. **dev 전용, 출시 앱 무관.**

### L9. 번들: `qrcode`가 runtime dependency, `manualChunks` 없음
- `ui/package.json`, `ui/vite.config.ts`. `SettingsDialog`는 이미 코드 스플릿 + hover preload로 잘 처리됨. `qrcode`는 원격 페어링(`PairingPage.tsx`) 전용인지 번들 분석으로 확인 후 아니면 동적 import. React/`@dnd-kit/*`/`@radix-ui/*`가 단일 메인 청크 — `manualChunks`로 분리 시 콜드 스타트 파싱 시간 개선(Tauri 로컬 로드라 네트워크 비용은 없음). **부분적 추정 — 실제 청크 구성을 빌드로 계측하지 않음.**

---

## 병목이 아닌 것으로 검증된 항목 (조치 불필요)

- **IPC `run_blocking` 규율:** `ipc/`의 모든 `#[tauri::command]`에서 git/ssh/파일 I/O가 `run_blocking`(`spawn_blocking`)으로 올바르게 래핑됨. async 런타임 스레드 위 동기 작업 위반 없음.
- **20바이트 바이너리 프레임 오버헤드:** 10ms/32KiB 코얼레싱으로 헤더 비용 < 0.1%. 프론트엔드 구간 Tauri Channel은 base64가 아닌 raw 바이트(base64는 무손실 폴리시로만 존재).
- **터미널 메트릭스 전역 뮤텍스:** `cfg!(debug_assertions) && FERRYX_TERMINAL_METRICS=1` 게이트로 릴리스에서 캐시된 atomic load + 분기 1회.
- **렌더 스케줄링:** `RenderScheduleCoordinator`가 버스트를 단일 pending 프레임으로 코얼레싱, VTE `feed()`는 tokio 펌프에서 실행되고 GPU present만 메인 스레드 홉. 프론트엔드 rAF 사용은 포인터 드래그/포커스뿐이며 출력 경로에 없음.
- **`remote/state.rs` 워크스페이스 스냅샷 캐시:** 2초 TTL + single-flight(`snapshot_lock` + atomic) + git 스캔 `spawn_blocking` 오프로드. 요청당 git 서브프로세스 없음.
- **원격 WS 펌프:** 그리드 33ms 코얼레싱, 뮤텍스 범위 최소, 프레임 용량 사전 계산, `Vec<u8> → Bytes` 문브(복사 아님).
- **worktree git 호출 빈도:** 캐시된 스냅샷 뒤에서 호출되며 스냅샷 빌드 중 `git status` 미실행.
- **세션당 블로킹 리더 스레드 1개:** 기본 런타임의 max_blocking_threads 512로 현실적 세션 수에서는 문제없음(수백 개 동시 PTY 예상 시에만 해당).

---

## 우선순위 로드맵 제안

1. **H1 (이중 attach/소비자 없는 스트림 제거)** — 단일 최고 레버리지. 데몬 직렬화 ~50% + 프론트엔드 메인 스레드 per-chunk 작업 제거.
2. **H6 (`[profile.release]`)** — 한 파일 변경, 광범위 이득. 리스크 거의 없음.
3. **H4 (세션 지속화)** — 인터랙티브 경로의 디스크 읽기 + 이중 fsync 제거.
4. **H2 (데몬 소켓 바이너리 프레이밍)** — 대량 출력 경로. H1 선행 시 적용 범위가 절반으로 줄어듦.
5. **H3 (에이전트 감지 스로틀)** — per-chunk 전체 그리드 스냅샷 제거.
6. **H5 (switchDebug 호출부 가드)** — 개발 환경 입력 지연 제거. 변경 작고 안전.
7. **H7 (데몬 클라이언트 파이프라이닝)** — 인터랙티브 지연. 설계 작업 동반.
8. **M1+M2 (`Arc<[u8]>` 청크 공유)** — 상시 출력 경로 복사 제거. 함께 진행.
9. **M3+M4 (single-pass 세그먼트 스냅샷)** — attach/재접속 지연.
10. **M7 (spawn_lock 축소)** — idempotency 레이스 방어 설계 리뷰 선행 필수.

### 구현 시 주의 플래그
- **M7 (spawn_lock):** 락이 idempotency/provider-claim 레이스를 방어함 — 기계적 이동 금지, 설계 리뷰 필요.
- **M5 (flush 케이던스):** 큐 drain 시에는 반드시 flush해 인터랙티브 지연을 보존할 것.
- **H6 (`panic = "abort"`):** unwind 캐치 의존 여부 확인 후 적용.
- **H1:** 리모트/웹 전송 경로는 별도이므로 영향 없음을 재확인한 뒤 적용.
