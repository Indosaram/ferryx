# 모바일 원격 터미널 3종 버그 수정 — 이력 깨짐 / 스크롤바 / 마우스 휠 스크롤

**날짜:** 2026-09-02
**범위:** 원격(모바일) 그리드 클라이언트 + 원격 게이트웨이 + 데스크톱 네이티브 터미널 휠

## 증상 (사용자 보고)

1. 모바일 원격 접속 시 유니코드(한글) 이력 깨짐
2. 화면에 스크롤바가 좌우/상하로 나타남
3. 마우스 스크롤로 터미널 화면 스크롤이 안 됨

## 근원 원인

### 1. 이력 깨짐 — 원격 미러가 데스크톱 폭 히스토리를 모바일 폭에 통짜 재생

- `handle_terminal_grid_socket` (`src-tauri/src/remote/server.rs`)는 접속 시
  `RemoteTerminalMirror::new(mobile_cols, mobile_rows)`를 만들고 ring buffer의
  flat `snapshot.history`를 통짜로 feed했다.
- 히스토리 바이트는 데스크톱 폭(예: 190컬럼)에서 에이전트 TUI가 절대 커서 주소
  (CUP/CHA/EL)로 기록한 것이므로, 모바일 폭(예: 50컬럼)에서 재파싱하면 셀이
  엉뚱한 좌표에 흩어진다 (= 흩어진 글자 조각, 한글 포함 "이력 깨짐").
- 데스크톱 attach 경로는 2026-08-31에 resize-ledger 세그먼트 재생
  (`feed_attachment_history`, `HistorySegment{cols,rows,bytes}`)으로 이미 해결돼
  있었는데 원격 미러 경로만 flat feed로 남아 있었다. `snapshot.history_segments`는
  이미 attachment에 포함되어 오고 있었다(무시만 된 상태).
- 동일 결함이 Lagged 복구 경로(브로드캐스트 밀림 후 히스토리 재생)의 양쪽 분기에도 존재.

### 2. 스크롤바 — 원격 그리드 surface의 CSS 오버플로

- 원격 그리드 surface div가 `overflow-auto`. 셀 폭은 숨은 스팬의 `1ch`로 측정하는데
  한글/이모지 등 실제 글리프 진도가 이 측정치와 어긋나면 줄 내용이 가로로 넘침 →
  가로 스크롤바 노출 → 스크롤바 높이만큼 세로 공간이 줄어 세로 스크롤바까지 연쇄 노출.
- 그리드는 현재 뷰포트만 렌더링하므로 CSS 스크롤 자체가 없어야 함.

### 3. 휠 스크롤 불가 — 두 표면 모두 스크롤 경로 부재

- 데스크톱: `NativeTerminalPane`의 `onWheel` → `cmd_native_terminal_scroll` 경로가
  존재하지만 네이티브 WGPU 자식 서피스 위에서는 DOM에 휠 이벤트가 도달하지 않음
  (2026-08-29 클릭 스왑 실측과 동일한 AppKit 이벤트 흡수 메커니즘).
- 원격: 그리드 프로토콜에 스크롤백/스크롤 명령이 아예 없음 (휠·터치 스크롤 미구현).

## 수정 내용

| # | 파일 | 변경 |
|---|------|------|
| 1 | `src-tauri/src/remote/mirror.rs` | `feed_segments(&[HistorySegment])` 추가 — 세그먼트 기록 크기로 리사이즈 후 feed. `dimensions()` 추가. RED→GREEN 테스트 2건 (`feed_segments_replays_at_recorded_geometry_then_resizes`, `flat_history_replay_corrupts_relative_to_segmented_replay`) |
| 1 | `src-tauri/src/remote/server.rs` | 초기 프레임 + Lagged 복구 양쪽에서 `history_segments` 우선 재생, 빈 경우(구버전 데몬) flat fallback |
| 2 | `ui/src/remote/RemoteTerminal.tsx` | surface `overflow-auto` → `overflow-hidden` |
| 3 | `ui/src/remote/RemoteTerminal.tsx` | 휠(`deltaY/20`, ±10 클램프)·단일 손가락 세로 드래그(8px 임계, 33ms 스로틀, 셀 높이 환산) 스크롤 → WS TEXT `{"type":"scroll","rows":N}` (+ = 아래로/최신 방향). 가로 스와이프 탭 전환과 축 우위로 상호배타 |
| 3 | `src-tauri/src/remote/protocol.rs` / `server.rs` / `mirror.rs` | `ClientControlMessage::Scroll{rows}` 추가(±50 클램프), `RemoteTerminalMirror::scroll(rows)` = `ScrollViewport::Delta` + full frame |
| 3 | `src-tauri/src/lib.rs` | `install_macos_terminal_scroll_monitor` — `NSEventMask::ScrollWheel` 로컬 모니터, `session_at_logical_point` 히트테스트, 데스크톱 스크롤 IPC와 동일 경로로 스크롤+렌더. 터미널 뷰포트 위에서만 이벤트 소비(이중 스크롤 방지), 그 외 통과. 순수 변환 fn `macos_wheel_scroll_rows` 단위 테스트 |

## 부수 수정 (사전 존재 결함)

- `src-tauri/src/terminal/shell.rs`: HEAD에 `test_custom_shell_keeps_no_login_flag`
  중복 정의가 커밋되어 lib 테스트 타깃 빌드가 깨져 있었음(stash 복구 커밋 e108414 유래 추정).
  모순된 두 복사본 중 의미가 올바른 쪽(커스텀 셸 + no-login 유지 기대)을 남기고 제거.

## 검증

- `cargo test --lib remote` — 65 passed (신규 mirror/protocol 테스트 포함)
- `cargo test --lib native_terminal` — 89 passed (휠 델타 변환 테스트 포함)
- 전체 `cargo test --lib` — **504 passed / 0 failed** (독립 재실행 확인)
- `cargo check` — 신규 경고 0
- `bunx tsc --noEmit` / `bun run --cwd ui test src/remote` (72) / 전체 `ui test` (1288) / `ui build` — 전부 통과
- 변경집계: 10 files, +668 / -26

## 배포 주의

- 원격 게이트웨이(그리드 소켓·미러)는 **데몬 프로세스 측**에서 동작하므로 수정 효과를
  받으려면 실행 중인 데몬을 새 바이너리로 완전 재시작해야 한다 (데몬은 GUI 재시작으로는
  교체되지 않음).
- 데스크톱 휠 모니터는 GUI 바이너리 소속 — `bun tauri dev`로 실행해야 확인 가능.

## 남은 알려진 사항 (범위 밖)

- 원격 접속 시 활성 PTY가 모바일 지오메트리로 리사이즈되는 기존 설계(Active Desktop Lock
  단일 소비자 모델)는 유지 — 접속 해제 후 데스크톱 폭 복원은 별도 과제.
- Windows/Linux에서 네이티브 자식 서피스 위 휠은 플랫폼 모니터(macOS 전용 구현)에 해당하는
  플랫폼 경로가 필요 — 이벤트/IPC 공통 경로는 이미 크로스 플랫폼.

## 사용자 수동 확인 절차 (자동화 불가 항목)

1. 데스크톱: `bun tauri dev` 실행 → 터미널에서 스크롤백 쌓인 상태로 마우스 휠 →
   스크롤되는지, 사이드바 위 휠은 그대로 동작하는지 확인.
2. 모바일: 원격 접속 → 접속 순간 상단 이력(한글 포함)이 흩어지지 않는지 확인.
3. 모바일: 화면에서 좌우/상하 스크롤바가 더 이상 나타나지 않는지 확인.
4. 모바일: 세로 드래그로 이전 출력 스크롤, 가로 스와이프로 탭 전환 분리 동작 확인.

## 데몬 무중단 교체 시도와 한계 (2026-09-02 추가)

요구: 데몬 재시작 시 "현재 실행 중인 리모트 연결 유지".

- **WebSocket 무중단은 구조적으로 불가** — 게이트웨이 WS는 데몬 프로세스 안에서 종단하므로
  프로세스 교체 = 연결 단절. exec() 핸드오프 업그레이드(docs/DAEMON_SESSION_SURVIVAL 참고)는
  검증된 경로일 뿐 미구현.
- **구현한 최선책**: 원격 클라이언트 자동 재접속(비정상 close 시 1→2→4→8→10s 상한 백오프로
  무한 재다이얼, 성공 시 리셋, unmount/세션 전환 시 취소, 이중 다이얼 방지). 재접속 후 서버가
  세그먼트 재생으로 이력을 온전히 복원하므로 화면이 자동 회복된다.
- **데몬 재시작 부작용(불가피)**: PtySession이 pty master fd의 유일 소유자라 데몬 사망 시
  모든 PTY 자식이 커널 수준에서 사멸(실증 검증됨). 재시작 시점의 활성 세션 수는 읽기 전용
  probe(listSessions)로 사전 확인 가능.
- **교체 절차** (실행 중 GUI는 그대로 둔다):
  1. `bun run --cwd ui build` (자동 재접속 포함 새 클라이언트 → 데몬이 ui/dist를 디스크에서
     서빙하므로 폰은 페이지 새로고침 1회로 새 JS 획득)
  2. `cargo build --manifest-path src-tauri/Cargo.toml` (프로토콜 v3 동일 — 기존 GUI 호환)
  3. 구 데몬 종료 → 즉시 새 데몬 headless 기동(nohup detached, FERRYX_DAEMON_READY 대기)
  4. 검증: flock/소켓 인계, :43821 재바인드(cloudflared 터널 자동 재접속), v3 probe,
     GUI 재부착, 폰 자동 복귀 + 이력 정상 표시
- 주의: GUI가 살아 있는 상태에서 데몬만 죽이면 GUI의 자동 스폰이 구 번들 바이너리로
  데몬을 띄울 수 있으므로(번 재검증된 함정), 새 데몬을 먼저 소켓을 잡게 하거나 GUI를
  새 번들로 교체해야 한다.

### 실제 교체 기록 (2026-09-02 완료)

- 디버그 데몬은 데이터/소켓이 `/tmp/rorca-501-dev/`로 격리(dev/release 인스턴스 분리)되어
  프로덕션 원격 경로를 서빙할 수 없음 → 프로덕션 교체는 **릴리스 빌드 데몬**으로 수행.
- `cargo build --release` (38.6s) → 구 데몬 종료 후 `target/release/ferryx --daemon` 기동
  (PID 64874, 프로덕션 소켓 /tmp/rorca-501/daemon.sock + :43821 인계).
- 검증: v3 handshakeOk(새 epoch), /api/v1/health 200, SPA 자산 = 새 ui/dist
  (index-BM977CXH.js — 자동 재접속 포함 클라이언트),
  **공용 터널 https://code.checka.cc/api/v1/health → 200 (0.47s)**, cloudflared↔데몬
  ESTABLISHED 재확립. 원격 수정분(이력 세그먼트 재생·스크롤바·원격 스크롤·자동 재접속)은
  전부 데몬+웹 자산 측이라 즉시 활성 — 폰은 페이지 새로고침 1회 후 적용.
- 데스크톱 GUI는 교체 과정에서 종료된 상태 — 재실행 시 v3 호환으로 새 데몬에 부착.
  데스크톱 네이티브 휠 수정은 GUI 바이너리 소속이라 번들 업데이트 시 적용.

### 앱 번들 교체까지 완료 (2026-09-02 사용자 지시)

`cargo tauri build --bundles app` (1m04s, 업데이터 아카이브+sig 포함) → 타우리 번들 서명
결함("code has no resources") ad-hoc 재서명 → GUI·데몬 종료 → `/Applications/Ferryx.app`
ditto 교체(codesign --deep --strict 통과) → 번들 데몬 headless 기동(PID 1411).
검증: v3 handshakeOk(새 epoch), 번들 Resources의 새 ui/dist 서빙(index-BM977CXH.js),
tunnel health 200 (0.38s), cloudflared 연결 재확립. 데스크톱·원격 전 경로가 신규 코드로
일원화 — 앱 실행은 사용자가 직접 수행(번들 데몬에 자동 부착).

### 에이전트 자동 resume 구현 (2026-09-02 추가)

요구: 데몬 재시작 후 에이전트 resume이 클릭 없이 자동으로 되어야 한다.
- 기존: 수동 어포던스만 존재(reconnectAgentSession — 검증된 per-agent argv, id 미팅 하드룰 준수).
- 구현: 복원 완료 후 `backendSessionId === null && agentType` 세션(=데몬 사망 고아, 사용자
  종료 세션은 상태에서 제거되므로 오탐 없음)을 자동 resume. ui/src/lib/agentAutoResume.ts —
  어포던스 canReconnect 검사, 활성 탭 우선 정렬, 400ms 스태거·최대 8개, 스냅숏 토큰/
  HMR 이중 발화 방지, 실패 시 조용히 수동 어포던스로 강등. 일반 셸은 기존대로 수동.
- 검증: UI 전체 1307 passed(신규 13개 포함, 독립 재실행 포함), tsc 0 에러, 빌드 성공.
- 번들 재빌드·교체 완료(index-CilUE755.js, 서명 검증 통과) — 실행 중 앱은 건드리지 않았고
  다음 앱 실행부터 자동 resume 활성. 데몬(1411)·터널 무영향(health 200).
