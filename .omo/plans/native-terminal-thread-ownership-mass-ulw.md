# mass-ulw: 네이티브 터미널 스레드 소유권 재설계 — 4단계 전량 구현 및 검증

**설계 원본:** `docs/NATIVE_TERMINAL_THREAD_OWNERSHIP_DESIGN_2026-09-19.md`
**티어:** HEAVY
**베이스:** `44cef470`
**시작:** 2026-09-19

## 하드 제약 (모든 노드 프롬프트에 복사할 것)

1. 공유 워킹 트리다. 다른 세션이 `src-tauri/src/{daemon/client.rs, session/mod.rs,
   ssh/direct.rs, ssh/direct_tests.rs, ssh/runtime.rs, terminal/remote.rs,
   terminal/remote_runtime_tests.rs}`를 미커밋 상태로 들고 있다. **읽지도 고치지도 말 것.**
   `git reset/restore/checkout -- /stash/clean` 전면 금지.
2. 실행 중인 `ferryx --daemon`(PID 910/16057/90242)과 GUI(PID 18189)를
   **절대 종료·재시작하지 말 것.**
3. 편집 직전 반드시 재독(re-read). 스테일 복사본으로 쓰기 금지.
4. 린트/타입/테스트 억제 금지. `#[allow]` 추가, `#[ignore]`, 테스트 삭제 금지.
5. 단일 체크아웃 + 단일 `cargo` 타깃 디렉터리다. **한 런 안에서 Rust 편집 노드는
   직렬화한다.** 쓰기 범위와 빌드 범위가 모두 겹치지 않을 때만 병렬.

## 런 순서

| 런 | 설계 문서 단계 | 내용 | 병렬성 |
|---|---|---|---|
| R1 | Phase 1 | FrameOutcome + FrameClock + 플랫폼 geometry/reveal 멱등화 | 직렬 3노드 |
| R2 | Phase 4 | ast-grep CI 게이트 + 계약 테스트 | 게이트/테스트 병렬 2노드 |
| R3 | Phase 2 | 스냅샷 트리플 버퍼, 렌더 경로 락 제거 | 직렬 |
| R4 | Phase 3 | 단일 GpuContext + GpuThread + `!Send` 토큰 | 직렬 |

R2를 설계 문서상 마지막 단계에서 두 번째로 당긴 이유: 게이트는 프로덕션 코드와
쓰기 범위가 겹치지 않고, R3/R4의 대규모 재구성이 R1이 세운 불변식을 침식하는 것을
먼저 막아주기 때문이다.

---

## R1 — Phase 1: 출혈 정지

### R1-N1 `core-frame-outcome` (직렬 1번, category: unspecified-high)

**쓰기 범위:** `src-tauri/src/native_terminal/surface_host.rs`,
`src-tauri/src/native_terminal/surface_error.rs`,
`src-tauri/src/ipc/native_terminal.rs`(호출부 수정 한정),
`src-tauri/tests/native_terminal_surface_host_contract.rs`

**작업**

1. RED 먼저. `native_terminal_surface_host_contract.rs`에 두 테스트를 추가하고
   실패를 캡처한다.
   - `idle_frame_clock_issues_no_render_passes` — dirty 세션이 없고 지오메트리 변화가
     없을 때 N tick 동안 렌더 패스 0회.
   - `present_timeout_never_redispatches_immediately` — `Timeout` 연속 주입 시
     디스패치 수가 프레임 클록 상한 이하, `Occluded` 주입 시 가시성 명령이 올 때까지
     추가 디스패치 0회.
2. `NativeTerminalSurfaceReceipt`의 `presented: bool` / `render_deferred: bool` 쌍을
   제거하고 `FrameOutcome`으로 대체한다.

   ```rust
   pub enum FrameOutcome {
       Presented,
       Retry(NextFrameAt),
       Suspend(WakeOn),
       Fatal(NativeTerminalError),
   }
   pub enum WakeOn { Visible, Resized, Reattached }
   pub struct NextFrameAt(std::time::Instant);
   ```
   `NextFrameAt`에 `now()` 생성자를 만들지 않는다. 생성자는
   `after_backoff(attempt: u32, refresh: Duration)` 하나뿐이고 최소 한 프레임 간격을
   강제한다. 즉시 재시도를 **표현할 수 없게** 하는 것이 이 노드의 핵심 산출물이다.
3. `surface_error.rs`의 분류를 갱신한다.
   `Timeout → Retry(after_backoff)`, `Occluded → Suspend(WakeOn::Visible)`,
   `Lost|Outdated → reconfigure 후 Retry`, `Validation → Fatal`.
4. 세션별 `RenderScheduleCoordinator`(`surface_host.rs:202-298`)를 제거하고 단일
   `FrameClock`으로 교체한다. dirty 세션 집합을 모아 tick당 1라운드로 그리고,
   dirty가 비면 tick을 건너뛴다. `Suspend`는 타이머가 아니라 가시성 명령으로만 해제.
5. `surface_host.rs:451-473`의 즉시 재디스패치 경로를 삭제한다.
6. `ipc/native_terminal.rs`의 `NativeTerminalSurfaceReceipt` 소비부를 새 타입에 맞춘다.
   IPC 응답 DTO의 외부 형태는 유지한다(프론트엔드 호환).

**VERIFY:** 1의 두 테스트 GREEN + `cargo test --manifest-path src-tauri/Cargo.toml
--test native_terminal_surface_host_contract` 전량 GREEN +
`cargo check --manifest-path src-tauri/Cargo.toml` exit 0.

**STOP WHEN:** 위 VERIFY가 전부 통과하고 RED/GREEN 캡처가 보고에 포함된 때.

### R1-N2 `platform-idempotence` (직렬 2번, dependsOn N1, category: unspecified-low)

**쓰기 범위:** `platform/macos.rs`, `platform/windows.rs`, `platform/linux.rs`,
`platform/wayland_child.rs`, `tests/native_terminal_child_surface_contract.rs`

**작업**

1. RED 먼저: "동일 bounds를 2회 적용하면 플랫폼 변형은 1회만 일어난다"를 세 플랫폼
   각각에 대해 단언하는 테스트를 추가하고 실패를 캡처한다. 플랫폼 FFI를 타지 않도록
   순수 로직(마지막 적용 bounds 비교)을 분리해 네이티브에서 전부 실행 가능하게 한다.
2. `macos.rs:159-186 apply_viewport`에 마지막 적용값 비교 조기 종료를 넣는다.
   `configure_window_background`, `setFrame`, `configure_terminal_layers` 모두
   실제 변경 시에만 실행.
3. `macos.rs:415-427 reveal()`을 최초 1회만 실행하도록 한다. `linux.rs`가 이미 쓰는
   `visibility.should_map_on_present()` 패턴을 재사용한다.
4. `windows.rs:293-333`, `linux.rs:357-410`에 같은 조기 종료를 적용한다.
   Wayland는 `wl_display_flush` 호출 횟수도 함께 줄인다.

**VERIFY:** 1의 테스트 GREEN + `native_terminal_child_surface_contract`,
`native_terminal_wayland_subsurface_contract` GREEN + `cargo check` exit 0.
설치된 크로스 타깃이 있으면 해당 타깃 `cargo check`도 exit 0.

### R1-N3 `verify-r1` (직렬 3번, dependsOn N2, category: unspecified-low)

전체 `native_terminal_*` 통합 테스트 + `cargo test --lib native_terminal` 실행,
변경 파일 LSP 진단 확인, 실측 증거 수집(SC5의 R1 몫: 유휴 CPU).

---

## R2 — Phase 4: CI 게이트

### R2-N1 `astgrep-policy` (병렬 가능, 쓰기 범위 = 신규 파일만)

`scripts/native-terminal-thread-policy.mjs` + ast-grep 규칙. 금지 패턴:
`run_on_main_thread` 클로저 본문 내부의 `get_current_texture` / `submit` /
`present` / `configure_surface` / `request_adapter` / `request_device`.
**자기 증명 필수:** GPU 호출을 일부러 심어 non-zero exit 캡처 → 되돌림 → clean tree
exit 0 캡처.

### R2-N2 `gate-contract-tests` (병렬 가능, 쓰기 범위 = tests/ 신규 파일)

`tests/native_terminal_frame_policy_contract.rs`: 유휴 0프레임, 재시도 상한,
`Occluded` 서스펜드를 계약으로 고정. 각 단언은 뮤테이션 증명(해당 가드를 일시로
부수어 RED 확인 후 복구)을 거친다.

---

## R3 — Phase 2: 락 제거

`SnapshotSlot` 트리플 버퍼 도입, VtThread publish / 렌더 consume, 렌더 경로에서
`sessions.lock()` / `hosts.lock()` 제거, `RenderSnapshot` 버퍼 재사용.
**주의:** `RenderSnapshot`은 `Serialize`라 원격 스냅샷 와이어에 실린다. 와이어 형태를
바꾸지 말 것 — 내부 표현만 교체하고 직렬화 표현은 보존한다.

## R4 — Phase 3: GpuThread 분리

프로세스당 단일 `GpuContext`(현재는 `renderer.rs:573`에서 패널마다 생성),
`pollster::block_on(request_adapter/request_device)`를 GpuThread 기동 1회로 이동,
`SurfaceCommand`/`FrameReport` 채널, `!Send` `UiThread`/`GpuThread` 토큰 도입,
`macos.rs:152-153`/`windows.rs:214-215`/`linux.rs:247-248`의 전면
`unsafe impl Send+Sync` 축소.

---

## 검증 기준 (설계 문서 §8)

| 항목 | 방법 | 통과 기준 |
|---|---|---|
| 유휴 CPU | poc 예제 60초, `ps -o time=` 델타 | < 1% (기준선 19.5%) |
| 메인 스레드 GPU | `sample` 후 QuartzCore/AGXMetal/IOGPU 프레임 검색 | 0 샘플 |
| 무한 스핀 | Timeout/Occluded 주입 계약 테스트 | 상한 이하 / 0 |
| 패널 확장성 | 1·4·8 패널에서 tick당 present 라운드 | 패널 수와 무관하게 1 |
| 회귀 | 기존 `native_terminal_*` 전량 | 전부 GREEN |


---

## 실행 결과 (2026-09-19 종료 시점) — 위 계획보다 이 절이 우선한다

계획 대비 가장 큰 변경: **R3를 단독 실행하지 않는다.** 설계 문서 §12.1 참조 — `sessions.lock()`은
GPU 작업 전에 드롭되고, 렌더 클로저 전체가 `run_on_main_thread` 안에서 돌기 때문에 직렬화의
주체는 뮤텍스가 아니라 메인 스레드다. R3를 먼저 해도 동시 렌더는 0에서 0이다. §12.7의 권장안
(hosts 맵을 GpuThread 소유로 이동)을 택하면 R3의 목표가 R4의 부수 효과로 달성된다.

### 완료 (22 커밋, 각각 변이 증명 또는 RED→GREEN 증거 보유)

| 런 | 상태 | 내용 |
|---|---|---|
| R1 | 완료 | 재시도 페이싱(8ms), 가려짐 시 suspend, 4개 플랫폼 지오메트리 래치, FrameClock(프레임 시작 기준), macOS reveal 멱등성 |
| R2 | 완료 | ast-grep 정책 게이트 + 자체 테스트, 사각지대(래퍼 표기) 수정, **CI 배선 완료** |
| R3 | 보류 | R4에 흡수. 단독 실행은 측정 가능한 효과 없음(§12.1) |
| R4 | 미착수 | §12.5/12.6/12.7에 완전 명세. 착수 전 아래 "남은 일" 참조 |

### 검증 기준 현황

- SC1 완료 — 상태 기계 + **시간 차원** 양쪽 고정. 변이(페이싱 ZERO 고정) 시 신규 테스트만 실패.
- SC2 완료 — 경계 재시도 + suspend + **재개**(정지가 편도가 아님) 까지.
- SC3 완료 — macOS/Windows/X11/Wayland 4경로. Wayland·child-surface 스위트를 Linux CI에 추가해
  로컬에서 컴파일 불가한 경로가 CI에서 실제로 빌드·실행되도록 함.
- SC4 테스트 절반 완료 — 312 테스트 0 실패 / 0 무시 / 0 스킵. LSP 절반은 환경적으로 불가
  (rust-analyzer 데몬이 세션 내내 사망, 6개 프로세스 정리 및 재색인 대기 후에도 복구 실패).
- SC5 방법론 검증됨, 결과는 비결정적 — POC는 `--window`에서 `ControlFlow::Wait`이고 PTY 구동이
  없어 설계상 유휴다. 측정법 자체는 양성 대조로 확인(실행 중 GUI에서 nextDrawable/Metal 검출).
  실증에는 변경분이 포함된 빌드 실행이 필요.
- SC6 완료 — 게이트가 자기 증명하고 **실제로 매 PR에서 실행됨**. 스캔 범위 = `src-tauri/src` 전체.

### 남은 일 (착수 전 필독)

1. §12.7 난점 A: `render_snapshot`의 `&mut host` 빌림이 UI leg 상태 변경과 연속 — "GPU 호출만
   이동"은 타입 검사를 통과하지 않는다.
2. §12.7 난점 B: 재예약과 `finish_render`가 워커 완료 경로로 함께 이동해야 한다. 디스패치
   직후 `finish_render`를 호출하면 유휴 0프레임 불변식이 깨진다.
3. 하네스는 `RenderDispatch`로 디스패치를 가로채므로 실제 wgpu/AppKit을 검증하지 못한다.
   실제 표면 검증은 변경분이 포함된 빌드를 띄워야 가능하다.
