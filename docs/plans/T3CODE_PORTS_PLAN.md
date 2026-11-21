# t3code → Ferryx 이식 계획

**작성:** 2026-11-15
**대상:** Ferryx HEAD `922f175db`
**참조:** pingdotgg/t3code (MIT) — `docs/user/*`, `docs/internals/*`, `apps/server/src/**`

## 1. 판단 요약

Ferryx를 버리고 t3code로 갈 이유는 없다. 두 제품의 moat가 다르다.

| 축 | Ferryx | t3code |
|---|---|---|
| 터미널 | libghostty-vt FFI + WGPU 네이티브 표면, 데몬이 PTY 소유, rolling handover | xterm.js 계열 웹 렌더, 서버 소유 PTY (5,000줄 / 8 MiB) |
| 원격 | 자체 relay + PIN 페어링 + 데스크톱 세션 락 | T3 Connect 상용 (3/10/100 머신), LAN/Tailscale |
| 에이전트 제어 | `agent_control/` + 16개 TOML manifest 기반 화면 감지 | 정식 오케스트레이터 + 이벤트 로그 + 체크포인트 |

**Ferryx가 이미 이기는 것:** 네이티브 터미널 렌더 경로, 데몬 PTY 수명 관리, 세션 무손실 handover, 셸 통합(워크트리/SSH/원격 데스크톱). 이건 건드리지 않는다.

**t3code가 이기는 것:** 에이전트 제어 평면 전체 — 내구성 있는 오케스트레이션, 스레드 생명주기(settle/snooze/pin/archive), provider 정규화, 체크포인트/되돌리기, 원격 RPC 권한 스코프, 프로젝트 액션.

결론: **터미널 코어는 유지하고 제어 평면만 이식한다.**

## 2. 이식 항목

규모: S(≤1일) / M(2~4일) / L(1주+). 모트 충돌: 터미널 렌더 경로를 건드리면 blocking.

### P0 — 내구성/정합성 (없으면 나머지가 무너짐)

| ID | t3code 기능 | 증거 | Ferryx 현재 | 대상 | 규모 | 충돌 |
|---|---|---|---|---|---|---|
| P0-1 | 오케스트레이터: 명령 직렬화 + 결정론적 이벤트 발생, I/O 없음 | `docs/internals/overview.md`, `Orchestrator.ts` | `src-tauri/src/agent_control/mod.rs`(1049줄)가 op 즉시 실행, 이벤트 로그 없음 | `agent_control/orchestrator.rs` 신규 | L | 없음 |
| P0-2 | EventSink: 이벤트+프로젝션+커맨드 영수증+아웃박스를 **한 트랜잭션**에, 재시도 멱등 | `EventSink.ts`, `docs/internals/effect-services.md` | `events.rs`(1262줄) 인메모리 `AgentEventBus`, 재시작 시 소실 | `crates/ferryx-hub` 마이그레이션 + `events.rs` | L | 없음 |
| P0-3 | EffectWorker: 커밋 후 부수효과 실행 | `EffectWorker.ts` | `delivery.rs`(1366줄)가 전송과 기록을 같이 함 | `agent_control/effect_worker.rs` | M | 없음 |
| P0-4 | 이벤트 로그 프루닝: 스트림당 최신 유지 + 커서 하회 시 스냅샷 재개 | `ProjectionMaintenance.ts` | 없음 | `ferryx-hub` 신규 테이블 | M | 없음 |
| P0-5 | 비동기 user_input_request를 턴 종료/재시작 넘어 영속, 응답 커밋과 사용자 메시지 원자화 | `docs/internals/providers.md` | `ipc/agent_prompt.rs`(41줄)는 화면 검증 후 즉시 전달, 재시작 시 소실 | `agent_control/pending_input.rs` | M | 없음 |
| P0-6 | ProviderAdapter 경계: driver-kind / instance 분리, per-provider 프로세스·계정 격리 | `docs/internals/adding-a-provider.md`, `ProviderAdapter.ts` | `agent_detect/`(3051줄) 16 TOML manifest, 화면 스크레이프만 | `agent_detect/adapter.rs` | L | 없음 |
| P0-7 | capability 정직성: 체크포인트 ≠ 롤백, 불가 시 FS 변경 전 거부 | `docs/user/providers-claude.md` | 없음 | `agent_control/policy.rs`(607줄) 확장 | S | 없음 |

### P1 — 사용자 체감 생명주기

| ID | t3code 기능 | 증거 | Ferryx 현재 | 대상 | 규모 | 충돌 |
|---|---|---|---|---|---|---|
| P1-1 | 스레드 settle/snooze/pin/archive + 5초 undo, 서버 소유 순서 | `docs/user/thread-sidebar.md` | `ui/src/components/sidebar/sidebarModel.ts`(148줄)는 파생 뷰만, 영속 생명주기 없음 | `sidebarModel.ts` + hub 스키마 | M | 없음 |
| P1-2 | ThreadSettlementService: 7일 무활동/PR 머지 시 자동 settle, 스레드별 규칙 | `ThreadSettlementService.ts` | 없음 | `ferryx-hub` 신규 서비스 | M | 없음 |
| P1-3 | 체크포인트: 히든 git ref + provider 대화 좌표 동시 되돌리기 | `checkpointing/CheckpointStore.ts` | `crates/ferryx-hub/src/checkpoint.rs`(1159줄)는 hub.sqlite zstd tar 백업 — 대화 되돌리기 없음 | `checkpoint.rs` 확장 | L | 없음 |
| P1-4 | PR 링크 동기화: 머지/클로즈 후 재조회, forge별 close 명령 | `PullRequestSyncReactor.ts` | `ipc/github_issue.rs`만, `gh pr` 표면 없음 | `ipc/github_pr.rs` 신규 | M | 없음 |
| P1-5 | 터미널 히스토리: 서버 5,000줄/8 MiB 상한, 재생 시 query/response 제거, 불완전 UTF-8 꼬리 스킵 | `docs/internals/terminal-runtime.md` | `output_hub.rs` 512 KiB 링버퍼 + `ReplayGap`; `retained_history.rs`(664줄)는 텍스트/이미지 예산만 | `retained_history.rs` 확장 | M | 부분(렌더 경로 불변) |
| P1-6 | 사이드 채팅(`/side`, `/btw`) | `docs/user/composer.md` | 없음 | `ui/src/remote/chat/*` | M | 없음 |
| P1-7 | 서브에이전트/Agents 뷰 + Working 폴드 | `docs/user/thread-sidebar.md` | `agent_control`의 `Lineage`/`spawn_child_depth`만, UI 없음 | `ui/src/components/sidebar/*` | M | 없음 |
| P1-8 | `t3.json` 프로젝트 액션 (`stopOnSettle`/`runOnSettle`) | `docs/user/project-settings.md` | 없음 | `src-tauri/src/worktree/` + 신규 로더 | S | 없음 |

### P2 — 원격/확장

| ID | t3code 기능 | 증거 | Ferryx 현재 | 대상 | 규모 | 충돌 |
|---|---|---|---|---|---|---|
| P2-1 | RPC 권한 스코프 ~15종 (`orchestration:read/operate`, `terminal:operate`, `filesystem:*`, `source-control:write`) | `docs/internals/environment-auth.md`, `packages/contracts/src/rpc.ts` | `DevicePermission{View,Control}` + `DeviceAccessScope{Mirror,Machine}` 2단 | `daemon/protocol.rs` + `remote/` | M | 없음 |
| P2-2 | 커맨드 팔레트에서 환경 간 검색 | `docs/user/keybindings.md` | `CommandPalette.tsx`(180줄) 로컬만 | 동일 파일 | S | 없음 |
| P2-3 | 사용량 한도 시 "reset까지 대기"/"resume at reset" | `docs/user/usage.md` | `ui/src/remote/PlanLimitNotice.tsx` 고지 UI만 | 동일 + `ferryx-hub` 스케줄러 | S | 없음 |
| P2-4 | 멀티모델 fan-out (Shift-클릭 → 스레드+워크트리 분기) | `docs/user/composer.md` | 없음 | `workspaceStore.ts`(5216줄) | M | 없음 |
| P2-5 | MCP 서버로 외부 에이전트에 노출 | `docs/user/outside-agents.md` | `ferryx-cli mcp` 존재(`hub_runtime.rs`) — 비교 후 스코프만 보강 | `crates/ferryx-hub` | S | 없음 |
| P2-6 | installer-ownership 게이트 업데이터 | `docs/internals/server-updates.md` | Tauri updater, 소유권 검사 없음 | `src-tauri/src/` | S | 없음 |

합계: **21개** (P0 7 / P1 8 / P2 6)

## 3. 실행 순서

**Phase 1 (P0-1..4, 1~2주)** — 이벤트 로그 스키마를 `ferryx-hub` 마이그레이션으로 추가하고, `agent_control`의 op를 오케스트레이터 경유로 바꾼다. 이 단계가 끝나면 데몬 재시작 후 에이전트 상태가 복원된다. 기존 `AgentEventBus`는 오케스트레이터의 인메모리 캐시로 격하.

**Phase 2 (P0-5..7, 1주)** — pending input 영속화, adapter 경계 도입(기존 16 manifest를 adapter 뒤로 이동, 매니페스트 자체는 유지), capability 협상.

**Phase 3 (P1-1..4, 1주)** — 스레드 생명주기 + settle 서비스 + 체크포인트. UI는 `sidebarModel.ts` 파생 규칙을 서버 상태 기준으로 전환.

**Phase 4 (P1-5..8, 1주)** — 터미널 히스토리 상한/정리, 사이드 채팅, Agents 뷰, 프로젝트 액션.

**Phase 5 (P2-*, 선택)** — 권한 스코프는 원격 게스트 링크를 열 때만 필요. 나머지는 여유 있을 때.

각 Phase는 독립적으로 ship 가능해야 한다. Phase 1 없이 Phase 3을 시작하지 않는다(체크포인트가 이벤트 로그에 의존).

## 4. 검증 계획

- Rust: `scripts/build-remote.sh`로 **maho-mac**에서 `cargo check`, `cargo test --manifest-path src-tauri/Cargo.toml --lib`, `cargo clippy`. 로컬 컴파일 금지.
- 이벤트 로그: 재시작 복원 테스트 — 오케스트레이터에 명령 주입 → 프로세스 재기동 → 프로젝션 동일성 검증(가상 시계 사용, sleep 금지).
- 체크포인트: FS 변경 + 대화 좌표 되돌리기 후 양쪽이 같은 revision인지 확인. 롤백 불가 provider는 FS 변경 전 거부하는지 별도 케이스.
- 터미널 히스토리: 5,000줄 초과 시 절단 위치, 재생본에 query/response 미포함, 잘린 UTF-8 꼬리 처리.
- UI: `bun run --cwd ui test`. 사이드바 생명주기는 reducer 단위 테스트 + 원격 호스트 GUI 확인.
- 원격/권한: 스코프별 거부 케이스(읽기 전용 토큰으로 `terminal:operate` 시도 → 거부).

## 5. 하지 않을 것

- **Electron/웹 셸로 전환** — 네이티브 터미널이 moat. libghostty-vt/WGPU 경로는 유지.
- **xterm.js 계열 렌더러 도입** — React를 터미널 프레임에서 빼는 t3code 원칙은 이미 Ferryx가 더 강하게 달성.
- **T3 Connect 상용 요금제/릴레이 종속** — Ferryx relay + PIN 페어링 유지.
- **headless Chrome 원격 브라우저 탭** — Ferryx는 CEF/WebView 기반 브라우저 엔진이 이미 있음(`crates/ferryx-browser-engine`).
- **t3code의 전체 권한 모델을 그대로** — Ferryx는 데스크톱 세션 락이 있어 스코프 모델이 더 단순해도 된다. P2-1은 게스트 링크 도입 시에만.
- **기존 16개 agent manifest 폐기** — adapter 뒤로 옮길 뿐, 화면 감지 자체는 Ferryx 자산.

## 6. 리스크

| 리스크 | 영향 | 완화 |
|---|---|---|
| 이벤트 로그 도입 중 데몬 프로토콜 v5 호환성 | 기존 GUI 연결 끊김 | 프로토콜 버전 올리고 양방향 지원 유지, 롤링 handover 경로로 배포 |
| `agent_control` 1049줄 리팩터가 진행 중인 다른 세션 작업과 충돌 | 공유 워킹트리 오염 | Phase 1은 별도 워크트리에서, 머지 전 `git status` 확인 |
| 체크포인트가 대화 되돌리기를 못 하는 provider | 사용자 기대 불일치 | P0-7 capability 협상으로 사전 거부 |
| 터미널 히스토리 상한 변경이 기존 `ReplayGap` 소비자와 충돌 | 재연결 시 화면 깨짐 | 상한은 추가만 하고 512 KiB 링버퍼는 그대로 둠 |
