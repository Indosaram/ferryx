# Ferryx 네이티브 터미널 최종 구현 계획

작성일: 2026-09-20  
상태: **현행 코드 조사 완료 / 아래 수정안은 구현 전 계획**  
기준: HEAD `666a46d06fab5ee93b47147bd3251cbc0527945a`와 조사 시점의 미커밋 작업 트리  
원본: [네이티브 터미널 스레드 소유권 설계](NATIVE_TERMINAL_THREAD_OWNERSHIP_DESIGN_2026-09-19.md)

## 1. 결론과 작업 범위

**기존 GPU 워커 분리는 유지하고, 세션별 VT 소유권과 불변 스냅샷 발행을 완성해야 한다. macOS 전용 Metal 서피스 경로를 새로 만드는 것은 이 문제의 해결책이 아니다.**

현재 네이티브 경로는 실제로 `GpuWorker`에서 acquire/encode/submit/present를 수행하며, 그 전에 전역 `hosts` 가드를 해제한다. 따라서 “GPU 렌더 전체가 여전히 메인 스레드에서 실행된다”는 초기 진단을 현재 상태에 그대로 적용하면 안 된다. 반면 메인 스레드의 렌더 준비와 GPU 완료 처리에 `sessions.lock()`이 남아 있고, VT pump 역시 같은 전역 락 아래에서 feed·history replay·화면 추출·상태 감지를 수행한다. **GPU를 옮겼어도 UI가 다른 세션의 VT 작업을 기다리는 경로가 남아 있다.** 근거: [surface_host.rs](../src-tauri/src/native_terminal/surface_host.rs), `dispatch_scheduled_render` 402–627행, `session_render_snapshot` 1032행 이후, daemon pump 1860행 이후.

추가로, GPU 비동기화 과정에서 **bounds IPC의 완료 계약이 끊긴 문제**가 확인된다. 실제 Native 분기는 항상 `render_deferred=true`를 돌려주지만 GPU의 최종 영수증을 bounds 대기자에게 전달하지 않는다. 기존 주입형 테스트는 다른 동기 경로를 통과하므로 테스트 통과만으로 이 문제를 배제할 수 없다. 이 부분은 Phase 2와 함께 최우선 수정 대상이다.

이번 변경은 **이 문서 하나의 추가**다. 생산 코드·테스트·기존 설계 문서·다른 세션의 미커밋 변경은 수정하지 않았다. 아래 RED 테스트와 새 구조체는 향후 구현 지침이지, 이미 추가하거나 실행한 결과가 아니다.

### 1.1 조사 기준과 증거의 강도

- 직접 확인: 원본 설계 전체, native terminal 모듈 구조, 주요 변경 파일, VT/렌더/IPC/프런트엔드 호출 경로, 네 플랫폼 compositor 코드, 기존 테스트와 CI 정책.
- 직접 실행: macOS 환경의 단위 테스트 219개, host/renderer 계약 테스트 합계 46개, AST 정책 테스트 11개. 모두 통과했다. 명령은 §10에 기록한다.
- 미실행: 실제 Ferryx GUI의 지연 재현·프로파일링, Windows/X11/Wayland 빌드와 실기기 실행, 새로 제안한 RED/GREEN 테스트. 이 문서는 해당 검증까지 끝난 구현 승인서가 아니다.
- 관련 경로의 기존 diff는 26개 파일, `+2052/-701`이었다. 범위는 `src-tauri/src/native_terminal/`, native IPC, Cargo.toml, 정책 스크립트다. 저장소 전체에는 이 작업과 무관한 변경도 다수 있다. 일괄 revert/reset/format을 하지 않는다.

핵심 소스 재현용 SHA-256:

| 파일 | 조사 시점 SHA-256 |
|---|---|
| `src-tauri/src/native_terminal/surface_host.rs` | `d8c6a49de3d24cfdedfee4046ef98954a7ebcb44e83c642e9d10553ae74618fc` |
| `src-tauri/src/native_terminal/thread_ownership.rs` | `30c46eee460be0729b7dc9c07527a9a4b42521edd8e5720344ac5921d1173bac` |
| `src-tauri/src/native_terminal/renderer/gpu_context.rs` | `eea31a16057a1e953854fab0c6f17dcc61803685ddeaedfb31af516104af2bef` |
| `src-tauri/src/ipc/native_terminal.rs` | `0f71d451058ed126f9183fa1a4a768480731ebf5cee191683eaddfdeda02859e` |

행 번호는 이 작업 트리 기준이다. 후속 구현에서는 함수 이름과 새 diff를 함께 확인한다.

## 2. Phase별 실제 구현 상태

| 영역 | 확인된 구현 | 아직 충족하지 못한 계약 | 판정 |
|---|---|---|---|
| Phase 1: 출혈 방지 | coordinator의 예약/실행/후속 프레임 상태, 세션별 `FrameClock`, 8ms retry 간격, off-thread 지연 dispatch, occluded/fatal 분류, geometry latch | `FrameOutcome` 타입은 실제 소스에 없음. 영수증 불린과 `SurfaceFrameAction`을 사용한다. 영속적인 Suspended/Fatal 상태와 이벤트 기반 재개는 미완성 | 부분 완료 |
| Phase 2: SnapshotSlot | 렌더 시 VT에서 소유된 `SessionRenderInput`을 생성해 GPU closure로 넘김 | 생성 자체가 메인 스레드의 전역 `sessions` 락 안에 있음. 세션별 발행 슬롯·캐시된 render handle·VT 생산자 격리 없음 | 핵심 미완료 |
| Phase 3: GPU worker | `GpuWorker`, `GpuThread`, `GpuLeg`, `LentSlot`/RAII loan, GPU 전 전역 hosts 가드 해제, 공유 Instance/Device/Queue | bounds 완료 통지, 폐기 실행 스레드, shutdown join, layout/attachment 세대, context 복구, 제한된 큐 계약 등 | 핵심 경로 구현 / 경계 미완료 |
| Phase 4: CI gate | ast-grep 정책, 11개 테스트, workflow 연결 | 간접 호출의 스레드 소유권·sessions 락·실제 Native 경로의 동작을 증명하지 못함 | 기반 완료 / 보강 필요 |

`FrameOutcome`는 `src-tauri/src/native_terminal` 전역 검색에서, `SnapshotSlot`은 `src-tauri/src` 전역 검색에서 일치 항목이 없었다. 검색은 잘림 없이 완료됐다. 따라서 작업 배경의 “FrameOutcome 도입”은 **동등한 일부 분기 처리 도입**과 구분해야 한다.

원본 설계에는 여러 차례의 후속 정정이 함께 들어 있다. 초기의 “모든 렌더가 UI”, 중간의 “sessions 락을 GPU 전에 놓으므로 제거 효과가 적다”, 후반의 “Drop에서 동기 대기”를 동시에 최종 요구사항으로 채택하지 않는다. 현재 코드와 아래 소유권 계약이 기준이다. 특히 acquire가 멈출 수 있는 상황에서 UI가 worker 반환을 동기적으로 기다리는 변경은 퇴행이다.

## 3. 현재 실행 경로와 남은 병목

### 3.1 실제 예약 렌더 경로

```text
Daemon output / 입력·상태 변경
  → RenderScheduleCoordinator
  → UI: dispatch_scheduled_render
       → sessions.lock → session_render_snapshot → unlock
       → hosts.lock → native target 생성/geometry 적용/GpuLeg 대여
       → hosts unlock
  → GpuWorker: configure → acquire → encode → submit → present
       → loan 반환
  → UI completion
       → retired 확인
       → sessions.lock으로 attached 확인
       → hosts.lock으로 reveal / responder 복구
       → coordinator finish / 필요한 후속 dispatch
```

**좋아진 부분:** 실제 Native GPU 구간은 전역 hosts 가드 없이 실행되고, host 엔트리 자체를 map에서 잠시 제거하지도 않는다. 입력이 host를 못 찾아 사라지는 방식의 수정을 다시 도입하지 않는다.

**남은 부분:** 시작과 완료가 둘 다 전역 sessions에 의존한다. 완료 시점의 짧은 lookup도 다른 pump가 그 락을 오래 소유하면 UI를 정지시킨다. 단순히 `session_render_snapshot` 호출 하나만 다른 함수로 옮겨서는 끝나지 않는다.

### 3.2 전역 락의 실제 사용자

| 경로 | 현재 락 내부 작업 | 이행 방향 |
|---|---|---|
| 예약 렌더 준비 | `session_render_snapshot`, selection, cursor/preedit, scrollbar, synchronized-output 조회 | 캐시된 render handle에서 이미 발행된 frame만 읽기 |
| 예약 렌더 완료 | 세션 attached lookup | attachment epoch/활성 상태를 가진 render handle 사용 |
| daemon pump | VT feed, bracketed-paste 조회, 이벤트 추출, 상태 감지 | 세션별 VT 소유자만 변경; registry 가드 없이 수행 |
| attach / Lagged / Gap | reset, 전체 history replay, resize, scrollbar, 상태 추출 | 같은 세션의 직렬화된 VT 작업으로 이동 |
| `render`, `render_current_with_focus` | hosts를 잡은 채 layout 처리와 sessions 접근 | UI geometry와 VT layout 명령 분리; snapshot 추출 금지 |
| `lock_attached_hosts` | hosts 획득 후 `ensure_surface_attached`의 sessions 획득 | hosts와 VT 잠금의 중첩 제거 |
| `get_receipt`, `snapshot_for_session` | 메타데이터 응답에도 VT 전체 snapshot 추출 | 마지막 발행 메타데이터 / 마지막 present 영수증을 목적별로 분리 |
| `with_session_terminal*`, `encode_input` | 입력·붙여넣기·선택·휠·검색 등의 VT 접근 | 비동기 세션 명령 또는 UI 밖의 세션별 접근 |
| 테마 / scrollback 재적용 | 전체 sessions map을 소유한 채 각 VT 갱신 | handle 목록만 복제하고 가드 해제 후 각 세션에 전달 |
| detach / close / teardown | hosts와 sessions 중첩, task abort, target 제거 | lifecycle 상태 변경, VT 종료, GPU retirement를 별도 절차로 연결 |

근거: [surface_host.rs](../src-tauri/src/native_terminal/surface_host.rs) 1203–1470, 1657–2420, 2456–2872행. 호출부는 [native IPC](../src-tauri/src/ipc/native_terminal.rs) 720–869, 989, 1381행 및 [lib.rs](../src-tauri/src/lib.rs) 879행의 직접 VT 접근까지 포함한다. IPC 파일만 옮기고 플랫폼 이벤트 경로를 남기지 않는다.

다음 두 조건을 따로 검증한다.

1. A의 VT 작업이 길어도 B의 이미 발행된 프레임과 UI completion은 진행한다.
2. A의 VT 작업이 길어도 B의 VT feed·입력·새 snapshot 발행은 진행한다.

SnapshotSlot만 추가하면 첫 번째 일부는 개선할 수 있지만, pump가 전역 map 락을 계속 잡으면 두 번째는 해결되지 않는다.

### 3.3 bounds IPC 완료 누락: 별도의 우선 결함

코드상 연결은 다음과 같다.

1. [host의 `render_snapshot`](../src-tauri/src/native_terminal/surface_host.rs), 2964행 이후의 Native 분기는 GPU를 직접 실행하지 않고 항상 `render_deferred=true`를 반환한다.
2. [`cmd_native_terminal_set_bounds`](../src-tauri/src/ipc/native_terminal.rs)는 최초 `render` 이후, deferred이면 VT의 `update_sender` 또는 detach를 기다린다. 다음 반복의 `render_current`도 Native에서는 deferred다.
3. GPU completion은 영수증으로 reveal/retry만 처리한다. bounds 대기자가 읽을 마지막 present 영수증이나 별도 completion 채널에 이를 발행하지 않는다.
4. [NativeTerminalPane.tsx](../ui/src/components/NativeTerminalPane.tsx) 1860–1968행은 bounds Promise가 끝나야 `finally`에서 `inFlight=false`로 바뀐다. 1999행 전후의 후속 geometry는 그동안 `pendingGeometry`에 머문다.

**결론:** Native 경로에서 추가 출력이 없으면 대기 종료 통지가 없으며, 출력이 있어도 재호출은 다시 deferred 경로다. 최초 표시의 프런트엔드 확정과 후속 resize가 막힐 수 있다. 이는 소스 경로로 확인한 결함이며, 이번 작업에서 실제 GUI RED 재현을 실행했다고 주장하지 않는다.

해결은 `presented=true`를 임의로 반환하거나 VT update를 한 번 더 보내는 것이 아니다. **요청 세대에 대응하는 실제 GPU FrameReport를 기다리도록 계약을 연결**해야 한다.

### 3.4 기존 테스트가 놓치는 이유

[주입형 테스트 경로](../src-tauri/src/native_terminal/surface_host.rs) 3416–3555행은 Native와 달리 host 가드 안에서 즉시 acquire/present 영수증을 만든다. harness는 심지어 “hosts가 잠겨 있음”을 검증한다. 따라서 기존 bounds 테스트의 성공이 비동기 Native 완료 경로의 성공을 뜻하지 않는다.

`no_global_hosts_mutex_held_during_gpu_work`(4829행)는 실제 dispatch 대신 임의의 worker job을 넣고 map을 `try_lock`한다. 일반 worker가 map을 잡지 않는 사실은 검증하지만, 렌더 준비·완료 경로가 전역 락을 다시 잡지 않는지는 검증하지 못한다.

`surface_host_drop_order_guarantees_surface_drops_before_target`의 독립 mock 구조체 및 generic LentSlot 테스트도 OS별 폐기 스레드나 부모 창 수명까지 보증하지 않는다. 기존 테스트를 버리지 말고, **테스트 대역을 GPU 프레임 동작에만 주입하여 실제 dispatch/receipt/retirement 흐름을 공유**하도록 보강한다.

## 4. 유지할 구현과 철회할 구현

### 4.1 유지

- `GpuWorker`와 `GpuLeg` 분리, `GpuThread` 권한 전달, GPU 전에 hosts 가드를 놓는 경계.
- `LentSlot`의 loan 반환 및 retired completion 차단 취지. 오래된 프레임이 재부착된 패널의 예약을 지우지 않는 보호.
- 공유 Instance/Device/Queue. 패널마다 새 device를 만드는 구조로 되돌리지 않는다.
- macOS frame latch, Windows/X11 geometry latch, Wayland geometry/scale 처리, 첫 present 후 reveal 및 pointer transparency.
- 기존 UNORM 색상 정책, 글꼴·atlas·한글 IME·wide cell·Kitty 이미지·scrollback 처리. 스레드 작업과 무관한 시각 회귀를 만들지 않는다.
- background detach에서 VT와 agent 상태 처리를 유지하고, close에서만 세션을 종료하는 구분.

[renderer 생성부](../src-tauri/src/native_terminal/renderer/renderer.rs) 646–664행에서 Device/Queue는 공유되지만 pipeline/atlas는 여전히 renderer별이다. 이를 “모든 GPU 리소스 공유”로 표현하지 않는다. atlas 통합은 이번 필수 작업이 아니다.

### 4.2 macOS 직접 CAMetalLayer 경로만 좁게 철회

현재 작업 트리에 아래 코드가 실제로 남아 있다.

| 파일 | 철회할 부분 | 보존할 부분 |
|---|---|---|
| [platform/macos.rs](../src-tauri/src/native_terminal/platform/macos.rs) | `CAMetalLayer::new`, 직접 `addSublayer`, `layer_ptr` retain/release, 직접 레이어 frame/scale 동기화 | native NSView, UI dispatch, frame latch, 기존 gravity/filter 설정 |
| [platform/mod.rs](../src-tauri/src/native_terminal/platform/mod.rs) 126–139행 | `surface_layer_ptr` 추상화 | `surface_target()` raw-window-handle 경로 |
| [surface_host.rs](../src-tauri/src/native_terminal/surface_host.rs) 3089행 이후 | `CoreAnimationLayer`를 선택하는 unsafe 분기 | UI에서 표준 `instance.create_surface(target.surface_target())` 호출 |
| [Cargo.toml](../src-tauri/Cargo.toml) | 이번 시도만을 위해 추가된 quartz-core 의존성/feature는 전체 사용처 확인 후 제거 | 다른 기능이 이미 사용하는 objc/dispatch 의존성 |

wgpu의 서피스 추상화는 그대로 사용한다. `create_surface`와 `configure/acquire/present`의 실행 스레드는 구분한다. raw-window-metal의 NSView 기반 layer 생성은 메인 스레드 요구가 명시되어 있으므로, **surface 생성까지 무조건 GPU로 옮기지 않는다**. 공식 API 확인 자료는 §12에 둔다.

직접 레이어 코드의 KVO 경합 설명은 그 변경의 주장이지, 이번 조사에서 확인된 wgpu 결함 재현이 아니다. 되돌린 뒤 실제 backend 문제를 재현하면 사용 중인 wgpu/raw-window-metal 버전에 맞춰 좁게 조사한다. 추측만으로 별도 Metal backend를 유지하지 않는다.

## 5. 목표 구조: registry는 찾기만, VT는 만들기만, GPU는 소비만

```text
Lifecycle registry: session_id → Arc<SessionHandle>
  생성/lookup/remove만 수행; Arc 복제 직후 가드 해제

세션별 VT 실행 소유자
  output + 입력/선택/resize/theme 명령을 순서대로 처리
  → 완성된 PublishedFrame 생성
  → SnapshotSlot 원자적 교체
  → render handle에 coalesced wake

UI host / pane binding
  Arc<RenderHandle>를 attach 시 캐시
  native child 생성·geometry·visibility·focus·최종 제거
  VT mutex, 전역 sessions, GPU 결과에 대한 동기 대기 없음

GpuWorker
  준비된 surface/geometry 세대 확인
  → slot 최신 Arc 한 번 load
  → configure/acquire/render/present
  → FrameReport 및 retirement ack 발행

UI completion / 비동기 IPC 대기자
  해당 attachment/layout 세대 확인
  → reveal, 마지막 present 영수증 반영, 대기 종료
```

전역 map 자체를 반드시 없애거나 DashMap으로 바꿀 필요는 없다. **수명 관리용 짧은 registry 락은 허용하되, 렌더 dispatch·GPU completion에는 registry 참조 자체를 넘기지 않는 것**이 최종 조건이다.

### 5.1 세션별 분리의 최소 이행

첫 번째 작은 변경은 map의 값을 `Arc<SessionHandle>`로 바꾸고, mutable VT 상태를 세션별 mutex로 격리하는 것이다. 모든 사용처는 `lookup → Arc clone → registry unlock → 세션 작업` 순서를 따른다. registry를 잡은 채 세션 mutex를 획득하거나 callback을 실행하지 않는다. close도 registry 밖에서 abort/retirement를 진행한다.

다만 이것은 중간 상태다. 메인 스레드가 자기 세션 mutex에서 기다리면 그 패널의 VT burst가 여전히 UI를 멈춘다. 최종적으로 기존 세션별 pump를 확장해 VT 변경·snapshot 생성을 UI 밖에서 직렬화한다. `focus`, `preedit`, `resize`, selection/wheel 등의 명령과 비동기 응답을 연결한다. 새 OS 스레드를 패널마다 만들거나 별도 actor framework를 도입할 필요는 없다.

VT feed와 history replay는 byte/time budget으로 나누고 다른 세션 및 control 명령에 실행 기회를 준다. Tokio task 안의 동기 CPU 작업이 cooperative scheduling을 자동으로 제공한다고 가정하지 않는다. 필요 시 제한된 blocking 실행기를 쓰되, 세션마다 동시 VT 접근은 하나만 허용한다. snapshot은 출력 chunk마다 전체 복사하지 말고 render demand/dirty 상태에 맞춰 합친다. 조용해진 마지막 dirty frame을 발행하는 deadline은 반드시 유지한다.

### 5.2 SnapshotSlot: 첫 구현은 원자적 Arc 교체

첫 구현으로 `ArcSwapOption<PublishedFrame>` 기반의 latest-only 슬롯을 권장한다. 공식 `ArcSwapOption`은 `Option<Arc<T>>`의 원자적 저장소이고 `load_full()`은 소유된 Arc를 얻는 API다. 관련 API의 단일 load 일관성 지침을 따른다(§12). 커스텀 `AtomicPtr`/`Arc::from_raw` 또는 reader pinning 없는 `UnsafeCell` 트리플 버퍼는 만들지 않는다.

이 선택은 저장소에 이미 존재하는 구현이 아니라 **새 의존성을 포함한 제안**이다. 추가 시 프로젝트 Rust 버전과 lockfile 호환성을 확인한다. 명확한 publisher/consumer 래퍼를 두고 외부에 slot의 임의 store 권한을 노출하지 않는다.

아래는 이름과 책임을 설명하는 설계 스케치이며 그대로 적용 가능한 완성 패치가 아니다.

```rust
struct PublishedFrame {
    session_incarnation: u64,
    attachment_epoch: u64,
    frame_generation: u64,
    layout_epoch: u64,
    render_config_epoch: u64,
    layout: SurfaceCompositionLayout,
    logical_bounds: LogicalBounds,
    cell_metrics: CellMetrics,
    renderer_config: RendererConfig,
    input: SessionRenderInput, // 완성된 snapshot + selection + overlay
}

struct SnapshotSlot {
    latest: arc_swap::ArcSwapOption<PublishedFrame>,
}

impl SnapshotSlot {
    // 실제 publish는 세션의 단일 VT 생산자에게만 허용한다.
    fn publish(&self, frame: PublishedFrame) {
        self.latest.store(Some(std::sync::Arc::new(frame)));
    }

    fn load_latest(&self) -> Option<std::sync::Arc<PublishedFrame>> {
        self.latest.load_full()
    }
}
```

**필수 불변식:**

1. 한 세션의 발행은 단일 생산자가 직렬화한다. frame generation을 먼저 발급한 두 작업이 역순 store하여 최신 화면을 되돌리지 못한다.
2. 발행 이후 frame 내부는 불변이다. GPU가 보유한 이전 Arc는 다음 publish·resize·close 후에도 유효하다. terminal/FFI 포인터, mutex guard, 가변 VT borrow를 넣지 않는다.
3. GPU는 frame 하나를 한 번 load하여 grid·selection·cursor·metrics를 함께 사용한다. 서로 다른 load나 전역 설정 재조회로 세대를 혼합하지 않는다.
4. 슬롯 발행이 GPU 완료를 기다리지 않는다. 다만 allocation과 오래된 Arc의 마지막 drop 비용은 존재한다. 전체 프레임 파이프라인의 시간 상한이 자동 보장되는 것은 아니다.
5. latest, 실행 중 frame, 생산 중 frame의 수와 진단 reader 수명을 제한한다. 원자적 Arc 슬롯이 엄밀히 “메모리 세 벌만 사용”하는 트리플 버퍼는 아니다. 오래된 Arc를 큐에 계속 쌓지 않는다.

[현재 RenderSnapshot](../src-tauri/src/native_terminal/snapshot.rs) 36–105행의 `Vec<Vec<CellSnapshot>>`, `String`, 이미지 placement를 첫 단계에는 보존한다. 셀 flattening, inline grapheme, row 단위 COW, 정교한 트리플 버퍼는 할당 측정 뒤 별도 최적화한다. 스레드 경합 제거에 반드시 필요한 변경이 아니며, grapheme/한글/이미지 수명을 동시에 바꾸면 회귀 범위가 커진다.

### 5.3 발행 트리거를 feed로 한정하지 않는다

| 변경 | 발행 내용 / 요구사항 |
|---|---|
| output, attach history, Lagged, Gap/reset | 재구성된 VT와 cursor/selection/scrollbar의 완성본 |
| resize, DPI, 글꼴, theme | VT grid·cell metrics·renderer 설정이 같은 layout/config 세대인 frame |
| focus, preedit 설정/수정/해제 | cursor style과 한글 조합 overlay를 포함. 조합이 사라진 마지막 상태도 발행 |
| selection, drag, wheel, scroll-to-bottom | 출력 없이도 새 frame 발행 |
| 입력 인코딩 후 viewport 이동 | `encode_input`가 수행하던 스크롤 변경을 누락하지 않음 |
| scrollbar visibility, attention frame | overlay만 바뀌어도 발행 |
| 재부착, synchronized-output 종료/만료, stream 종료 | 마지막 완성 frame을 다시 발행하고 필요한 wake 보장 |

현재 `session_render_snapshot`은 위 상태를 합성한다. 이 함수를 없애기보다 **VT 생산자 측의 완성 frame 생성 함수로 이동**하는 것이 작다.

DEC synchronized output 중간 화면은 publish하지 않는다. GPU는 마지막 완성본을 유지한다. 종료 시퀀스·기존 bounded deadline·stream 종료에서 dirty 화면을 발행한다. replay 흡수·startup PTY 응답 억제/보존·remote generation 확인은 기존 프로토콜 의미를 유지한다. 특히 `DaemonStreamMessage::Exit` 경로도 최종 sync/dirty 처리가 빠지지 않도록 검사한다.

### 5.4 서로 다른 세대를 섞지 않는다

- `session_incarnation`: 동일 session ID의 close 후 재생성을 구분한다.
- `attachment_epoch`: detach/reattach 및 surface 교체를 구분한다.
- `layout_epoch`: resize/DPI 요청과 적용을 구분한다.
- `frame_generation`: 완성 snapshot 발행 순서다.
- `render_config_epoch`: 실제 사용한 글꼴·테마 설정을 구분한다.

이 번호들은 remote reconnect의 `remote_generation`과 다른 책임이다. 같은 필드로 재사용하지 않는다.

UI가 geometry epoch N을 적용하고, VT가 동일한 N에 맞춘 frame을 발행한 경우에만 해당 surface에서 렌더한다. worker가 잡은 snapshot N에 UI의 최신 metrics N+1을 덧씌우지 않는다. 아직 geometry/frame이 맞지 않으면 이전 정상 화면을 유지하고 새 일치 frame을 기다린다. 오래된 완료는 새 layout 영수증을 덮거나 새 surface를 reveal하지 않는다.

## 6. 렌더 경로의 전역 sessions 제거와 완료 통지

### 6.1 함수 경계부터 바꾼다

현재 예약 함수의 `sessions: Arc<Mutex<HashMap<...>>>` 인자를 제거하고 attach 때 만든 `Arc<RenderHandle>`을 넘긴다. RenderHandle에는 slot, coordinator, attachment 식별자/활성 상태, layout 준비 정보, frame-report 발행기가 들어간다. mutable VT는 들어가지 않는다.

`defer_scheduled_render`, GPU completion, direct render의 재예약 helper도 같은 handle을 사용한다. `surface_attached`, coordinator, 영수증을 알아내려고 전역 sessions로 돌아가는 fallback을 남기지 않는다. debug/query API는 별도 경로로 분류하고 UI에서 동기 호출하지 않는다.

UI host map을 유지하는 동안에는 geometry/엔트리 변경에만 짧게 사용한다. 가드 안에서 VT 처리, renderer 생성, GPU 작업, await, join, 임의 callback, 무거운 destructor를 실행하지 않는다. host map 제거보다 **가드의 책임 축소**가 우선이다.

### 6.2 wake와 completion의 경합

순서는 `완성 frame store → dirty/wanted generation 표시 → coalesced wake`다. GPU 큐에는 frame의 깊은 복사 대신 해당 handle을 넣고, 실제 실행 시점에 사용할 최신 준비 frame을 load한다. 패널별 queued/in-flight 작업은 합쳐서 최대 하나의 실행 예약, 그리고 후속 dirty 표시만 유지한다.

completion은 자신이 시작한 attachment/job 세대의 예약만 해제한다. 처리한 frame generation보다 새로운 frame이 있으면 후속 예약 하나를 만든다. 다음 두 interleaving을 반드시 테스트한다.

- publish가 completion의 pending 해제 직전에 발생한다.
- publish가 pending 해제 직후 발생한다.

어느 경우에도 마지막 frame을 잃거나 중복 job을 쌓아서는 안 된다. 기존 coordinator 상태 전이를 재사용하되 generation/epoch 검증을 붙인다. 오래된 콜백의 무조건 `consume_render()`는 금지한다.

현재 worker는 unbounded `std::sync::mpsc`와 임의 closure를 받는다([thread_ownership.rs](../src-tauri/src/native_terminal/thread_ownership.rs)). 처음에는 render 예약 상한을 실제 dispatcher로 증명하고, shutdown/control 명령은 별도로 제한한다. bounded 채널로 바꿀 때 `try_send(Full)` 후 dirty를 지우거나 detach/retire를 버리면 안 된다. 프레임·중복 geometry는 합칠 수 있어도 PTY 바이트·키 입력·종료·retirement는 유실하면 안 된다.

### 6.3 FrameReport와 bounds 계약

VT `update_sender`와 **GPU FrameReport**는 분리한다. GPU 완료 시 `(incarnation, attachment_epoch, layout_epoch, frame_generation, outcome, receipt)`를 발행하고, 대기자는 명령 전달 전에 report를 구독한다. 성공한 마지막 present 영수증은 별도로 보관한다.

`cmd_native_terminal_set_bounds`는 다음과 같이 바꾼다.

1. layout 요청에 번호를 부여하고 VT resize/UI geometry 적용을 예약한다. UI closure는 이를 위해 VT를 기다리지 않는다.
2. 요청과 같은 attachment 및 해당 layout을 충족하는 실제 FrameReport를 비동기적으로 기다린다.
3. Presented이면 그 실제 영수증을 반환한다. unrelated VT output으로 재렌더를 호출하는 현재 loop를 제거한다.
4. 새 layout이 이전 요청을 대체하면 `Superseded`로 종료하고, 프런트엔드는 현재 geometry를 다시 확인한다. 이전 요청의 rectangle을 최신 결과로 잘못 캐시하지 않는다.
5. Fatal, Detached, Closed, worker/dispatch 실패는 구조화된 오류로 끝낸다. 결과를 받을 수 없는 대기자를 남기지 않는다.
6. Suspended는 명시적인 상태로 응답한다. 프런트엔드는 `inFlight`를 풀고 rAF 재시도하지 않으며 visibility/geometry 복구 이벤트에서 한 번 재요청한다. backend와 frontend를 같은 변경 단계에서 맞춘다.

현재 IPC에는 `render_suspended`가 없고, 프런트엔드는 일반 `presented=false` 응답에 rAF 재시도를 건다. 따라서 suspend를 그저 false로 반환하면 새로운 루프가 생긴다. 기존 필드를 호환용으로 유지하면서 명시적인 `renderState` 또는 동등한 typed 상태를 추가하고, 실제 present가 아닌 receipt로 `setPresentation`을 호출하지 않도록 한다. visible 정상 경로의 “실제 present 뒤 Promise 완료” 의미는 보존한다.

## 7. Phase 3의 잔여 경계 조건

### 7.1 폐기와 shutdown: UI 동기 대기 금지

현재 `LentSlot`은 GPU 대여 중 target을 보유하고, loan 반환 시 GPU leg를 먼저 drop한 다음 disposer를 실행한다. 순서 보호는 유효하다. 그러나 **disposer 실행 스레드가 native target의 요구 스레드와 같다는 보장은 별개**다.

| 플랫폼 | 현재 코드 | 최종 계약 |
|---|---|---|
| macOS | target Drop이 main queue로 NSView 제거를 전달 | raw-window-handle 경로 복원 후에도 surface 수명 동안 NSView 유지. UI에서만 native view 변경/최종 제거 |
| Windows | [windows.rs](../src-tauri/src/native_terminal/platform/windows.rs) 351–367행에서 private `PostMessageW`로 child owner에게 파괴 요청 | 현재 비동기 전달 유지. 부모 HWND가 GPU 사용 중 먼저 파괴되는 close 순서도 보호 |
| X11 | [linux.rs](../src-tauri/src/native_terminal/platform/linux.rs) 137–145행에서 XUnmap/XDestroy/XFlush 직접 실행 | GTK가 소유한 display/native child의 조작과 폐기를 UI 소유 경로로 일원화 |
| Wayland | [wayland_child.rs](../src-tauri/src/native_terminal/platform/wayland_child.rs) 411–434행에서 proxy/queue 직접 파괴 | surface/proxy/queue 소유권과 wgpu 사용 종료의 ack를 연결; parent/display보다 먼저 안전하게 정리 |

가장 명료한 최종 형태는 **UI가 retired target을 보관하고 GPU가 surface 해제 완료 ack를 보내는 것**이다. GPU closure가 native target 전체를 캡처해 임의 스레드에서 drop하지 않는다. 슬롯에 leg가 집에 있는 경우도 즉시 UI에서 GPU 리소스를 무겁게 해제하지 않도록 retirement 경로를 통일한다.

일반 순서:

```text
UI: epoch 무효화 / 새 프레임 접수 중단 / retired target 보관
GPU: 해당 job 종료 → SurfaceTexture 정리 → surface/GPU leg 해제 → retirement ack
UI: 동일 epoch의 target 제거 / 부모 창 종료 절차 진행
```

wgpu backend의 실제 drop 스레드 제약은 사용 버전과 플랫폼 검증으로 확인한다. backend가 UI에서 최종 surface 해제를 요구하는 것으로 확인되면, 같은 공통 retirement 절차의 마지막 단계만 UI에 전달한다. 이를 이유로 GPU 렌더나 present를 UI로 되돌리지 않는다.

현재 `GpuWorker::Drop → shutdown → join`은 마지막 소유자가 UI일 때 GPU 지연을 UI 대기로 전파할 수 있다. 명시적 stop/ack와 UI 밖의 join으로 종료를 정리한다. 부모 창은 ack 전에 파괴되지 않도록 close를 비동기적으로 조정한다. driver가 무한 정지한 경우, 안전하지 않은 강제 surface/target 파괴로 타임아웃을 충족했다고 하지 않는다. 오류 보고와 종료 정책을 정하고 수명 보호를 우선한다.

### 7.2 에러 분류와 재시도

현재 [surface_error.rs](../src-tauri/src/native_terminal/surface_error.rs)는 Timeout/ Occluded를 구분하고, [GpuLeg](../src-tauri/src/native_terminal/surface_host.rs) 3168행 이후는 Lost/Outdated에 configure 후 한 번 재획득한다. 무조건 무한 루프였던 상태와는 다르다.

그 위에 내부 `FrameOutcome`와 surface 상태를 완성한다. 아래 표는 **새 목표 정책**이다.

| 결과 | 목표 처리 |
|---|---|
| Presented | 실제 receipt 발행. 새로운 dirty generation이 없으면 idle |
| Timeout | 미래 deadline으로 retry. 즉시 재귀/무한 tight loop 금지 |
| Occluded / 명시적 hidden / zero-size | Suspended 저장. VT는 계속 처리하되 output만으로 GPU를 계속 깨우지 않음 |
| Lost / Outdated | 제한된 reconfigure, 반복 실패는 paced recovery 또는 typed failure |
| Suboptimal | 얻은 정상 frame은 present하고 다음 안전한 시점의 configure 필요 상태 기록 |
| Fatal / device 오류 / frame panic | typed report로 대기 종료; output마다 무한 재시도하지 않음. 명시적 재부착/복구 절차로 이동 |
| queue 거절 / UI dispatch 실패 | job 예약·loan·대기자를 정리. 중단된 epoch가 새 예약을 지우지 않음 |

현재 8ms는 구현된 retry 기준이지 모든 디스플레이의 refresh interval이 아니다. fake clock 테스트로 최소 간격과 long-frame 후 다음 deadline을 검증한다. 새로운 publish가 기존 retry deadline을 무시하고 계속 즉시 재시도하게 만들지 않는다.

Occluded 재개 이벤트는 macOS/Windows/X11/Wayland 각각에서 검증한다. visibility/restore/expose 또는 실제 geometry 변화가 render handle을 다시 깨우도록 연결해야 한다. 그 연결 없이 output wake만 제거하면 화면이 영구 정지할 수 있다. 기존 `a_suspended_surface_renders_again_when_output_returns` 테스트는 현재 동작의 증거이며, 최종 이벤트 기반 정책에서는 새 재개 테스트로 의도를 갱신한다.

### 7.3 공유 context: 공유와 복구는 다르다

[GpuContext](../src-tauri/src/native_terminal/renderer/gpu_context.rs) 43–97행은 공유 context를 `OnceLock<Result<...>>`에 저장하고, adapter 선택 시 `compatible_surface: None`을 사용한다. 초기 실패도 영구 캐시되며, device 교체 세대는 없다.

필수 보완은 첫 native surface에 대한 adapter 호환성 확인, 후속 surface capabilities 검증, 초기 실패의 명시적 재시도 상태다. 지원되지 않는 surface 때문에 새 device를 패널별로 무조건 만들지 않는다. 복구 불가능한 경우 명확한 오류를 보고한다.

Device-lost 복구를 지원하려면 reset 가능한 GPU 소유 context 상태와 `device_epoch`가 필요하다. 교체 시 관련 renderer/atlas/surface configuration을 함께 무효화하고 마지막 snapshot으로 재구성한다. 첫 구현에서 자동 복구까지 넣지 않는다면 적어도 실패를 latch하고 모든 영향받는 대기자를 종료해야 하며, “자동 device 복구 완료”로 표기하지 않는다.

### 7.4 타입 권한과 실제 스레드

`GpuThread`와 `UiThread`의 비전송 토큰은 좋은 기반이지만 현재 모든 native API가 토큰을 요구하는 것은 아니다. UI에서만 가능한 create/update/reveal/destroy는 `&UiThread`, GPU 실행·configure·renderer 초기화는 `&GpuThread`를 받는 좁은 경계로 모은다. `UiThread::assume_current`를 임의 호출자가 권한을 만드는 우회로로 사용하지 않는다.

플랫폼 target의 광범위한 `unsafe impl Send/Sync`는 단순 raw handle 전달과 native object 조작 권한을 혼동하게 한다. 전부 무작정 삭제하기보다 UI target과 GPU용 최소 handle을 나누고, 남는 unsafe 근거를 수명·스레드·호출 순서로 제한한다.

worker의 macOS 전용 `CATransaction flush`도 현재 존재한다. 그것이 단일 프레임 submit/present 뒤 반드시 필요한지 실제 runtime으로 확인한다. 근거 없이 플랫폼 우회 코드를 추가하거나 일괄 삭제하지 않는다. worker의 전체 job 경계와 retirement까지 panic/실패 정리가 연결되는지도 검사한다.

### 7.5 단일 GPU worker의 한계

전역 sessions 경합 제거는 **VT burst가 다른 패널과 UI를 잠그는 문제**를 해결한다. 그러나 하나의 GPU worker에서 A의 acquire가 오래 걸리면 뒤의 B GPU job도 기다릴 수 있다. `AutoVsync`와 단일 공유 queue를 사용한다고 N개 surface의 지연이 독립적이 되는 것은 아니다.

첫 단계는 패널별 latest-only 예약, 공정한 순서, hidden/occluded 작업 제외, acquire/queue 대기 측정이다. 이 측정에서 driver acquire의 head-of-line 지연이 실제 병목으로 남을 때만 다음 구조 변경을 검토한다. 처음부터 패널별 GPU 스레드·커스텀 present mode·전용 Metal backend를 도입하지 않는다. 특히 `NonBlocking`처럼 현재 사용 API에서 검증하지 않은 옵션을 계획에 가정하지 않는다.

## 8. 가장 작은 실행 순서와 파일별 변경 범위

각 단계는 생산 경로를 공유하는 RED 테스트가 먼저이며, 해당 GREEN을 확인한 뒤 다음 단계로 진행한다. 기존 foreign diff를 통째로 되돌리지 않는다.

| 단계 | 구체적 작업 | 주 변경 대상 | 완료 조건 |
|---|---|---|---|
| 0. 기준선 고정 | 현행 테스트·작업 트리 보존, Native 비동기 경로를 공유하는 테스트 seam 추가 | `surface_host.rs` 테스트, host 계약 테스트 | §9의 R1/R2/R5가 구버전에서 의도한 실패를 드러냄 |
| 1. 경계 회귀 차단 | 직접 CAMetalLayer 분기만 철회; retirement의 플랫폼 수명 보호; GPU FrameReport와 bounds 대기 연결 | `platform/macos.rs`, `platform/mod.rs`, `surface_host.rs`, `ipc/native_terminal.rs`, `NativeTerminalPane.tsx` | 무출력 bounds가 실제 present로 종료, detach/fatal/suspend도 종료; 새 surface를 stale completion이 건드리지 않음 |
| 2. 세션 격리 | registry 값을 Arc handle로 변경; 모든 feed/history/query/control의 전역 락 범위 축소 | `surface_host.rs`, native IPC, `lib.rs` 직접 VT 호출부 | A VT를 정지시켜도 B VT 진행. registry 가드 아래 VT/세션 락 없음 |
| 3. SnapshotSlot 생산 | 기존 pump에 제어 명령과 dirty/publish 연결; 모든 visual invalidation 포함 | 새 `snapshot_slot.rs` 또는 동등 모듈, session/pump, Cargo.toml/lockfile | 불변 frame 수명·일관성·최신값·sync 출력·비출력 트리거 검증 |
| 4. 소비 경로 전환 | 예약/직접 렌더/재시도/completion을 RenderHandle+slot 기반으로 전환; 메타데이터 캐시 | `surface_host.rs`, `ipc/native_terminal.rs`, renderer 경계 | 실제 render 준비와 완료에서 전역 sessions/VT lock 접근 0, UI에서 VT 작업 0 |
| 5. 경계 마감 | typed outcome, deadline/suspend 재개, 제한된 예약, 실패/종료, context 호환성·실패 상태 | `thread_ownership.rs`, `surface_error.rs`, `gpu_context.rs`, 플랫폼 이벤트 연결 | 실패가 spin/영구 대기/UAF를 만들지 않음 |
| 6. 게이트/실기기 | AST 및 권한 compile-fail, 실제 dispatcher 테스트, 4개 backend 시나리오 | 정책 rules/test, workflow, 테스트·측정 기록 | §10–11의 완료 기준 충족 |

1단계의 completion report는 4단계에서 버릴 임시 receipt 경로를 따로 만드는 것이 아니라, 최종 RenderHandle의 일부로 먼저 도입한다. 세션별 mutex 격리는 이행 수단이고, UI가 그 mutex를 기다리는 상태에서 전체 작업을 완료 처리하지 않는다.

새 파일을 다수 만들기 위한 모듈 분할은 하지 않는다. 먼저 `snapshot_slot.rs`와 기존 모듈의 작은 타입 경계로 구현하고, 거대한 `surface_host.rs` 분리는 독립적으로 리뷰 가능한 범위에서만 한다.

## 9. RED → GREEN 검증 설계

아래는 **추가해야 할 테스트**다. 이번 문서 작업에서는 구현하거나 실행하지 않았다. timeout은 교착 시 테스트를 끝내는 안전장치로 쓰고, 정상 성공 판정은 barrier/channel 순서로 한다. scheduler 성능을 1ms 벽시계 assertion으로 판정하지 않는다.

| ID | RED: 현행 코드/잘못된 수정에서 잡아야 할 실패 | GREEN: 통과 조건 |
|---|---|---|
| R1 `published_b_frame_ignores_global_registry_lock` | A 또는 테스트가 registry를 보유하면 B의 실제 render prepare/completion이 정지 | 캐시된 B handle로 prepare와 completion이 registry 해제 전 진행. 실제 dispatcher 경로 사용 |
| R2 `vt_b_progresses_while_vt_a_is_paused` | A feed/history 구간을 barrier로 멈추면 B feed가 같은 전역 락에서 대기 | B feed·입력 응답·publish 완료 신호를 A 해제 전에 받음 |
| R3 `ui_heartbeat_ignores_vt_and_gpu_stalls` | UI가 VT guard 또는 GPU 완료에 동기 대기 | A VT와 GPU acquire를 각각 정지해도 UI heartbeat가 먼저 처리됨 |
| R4 `snapshot_latest_is_owned_and_coherent` | snapshot 조각 세대 혼합, 오래된 메모리 재사용, producer의 consumer 대기 | GPU가 old Arc를 pin한 동안 다수 publish 가능; old 불변, 최신은 최대 generation; reader drop 후 회수 |
| R5 `native_bounds_completes_without_vt_output` | Native처럼 최초 deferred를 반환하는 seam에서 첫 present 후에도 bounds 미완료 | 추가 output 없이 실제 GPU report만으로 Presented 응답. 프런트엔드 inFlight 해제 및 후속 resize 진행 |
| R6 `bounds_terminal_outcomes_are_settled` | fatal/worker stop/detach/suspend 뒤 대기 누수 또는 rAF 재시도 폭주 | 각각 typed 종료; suspend는 재개 이벤트 전 재시도 0; 구독-before-completion 경합 보장 |
| R7 `publish_racing_completion_keeps_final_frame` | pending 해제 양쪽 경합에서 마지막 dirty frame 유실/중복 | 출력이 완전히 멎어도 최신 generation까지 present, 패널별 예약 상한 유지 |
| R8 `stale_epoch_cannot_mutate_replacement` | close 후 같은 ID 재생성, detach/reattach, resize 교체의 old completion이 새 예약/영수증을 덮음 | old report는 폐기되고 새 surface reveal·예약·layout 결과는 보존 |
| R9 `all_visual_mutations_publish` | feed에만 publish하여 focus/IME/selection/scroll/overlay 화면이 갱신되지 않음 | 각 변경을 출력 없는 상태에서 수행해 다음 완성 frame 내용·generation 확인 |
| R10 `sync_output_publishes_only_complete_frames` | 동기 출력의 중간 화면 노출 또는 deadline/EOF에서 마지막 frame 유실 | old 완성본 유지 후 end/expiry/EOF에서 완성본 발행. resize·replay도 같은 규칙 |
| R11 `retry_and_visibility_are_bounded` | persistent Timeout 즉시 루프, occluded output마다 acquire, 재개 누락 | fake clock으로 retry 간격 검증; 이벤트 기반 wake; Lost 반복 횟수 제한; Fatal latch |
| R12 `production_retirement_orders_resources` | native target/parent를 GPU 사용 종료 전에 제거, Linux에서 잘못된 소유 스레드 폐기 | 실제 retirement seam에서 gpu_done → surface_drop → UI target_drop 확인. pending loan·queue 거절·window close 포함 |
| R13 `worker_failure_settles_every_reservation` | enqueue 실패/panic/완료 dispatch 실패로 loan 또는 대기자가 남음 | 예약과 waiter가 정리되고 resource는 정확히 한 번 반환/폐기. 새 epoch 예약은 살아 있음 |
| R14 `shared_gpu_failure_has_defined_recovery` | 최초 context 오류가 재부착에도 영구 캐시, 부적합 adapter, shared device 오류의 일부 pane만 방치 | 호환 surface 검증, 실패 latch/명시 재시도, 영향 pane의 typed 결과; 자동 복구 구현 시 device epoch 갱신 |
| R15 `thread_boundary_is_enforced` | UI helper 내부의 숨은 GPU/VT 호출, unsafe 토큰 우회 | scoped AST fixture + compile-fail 권한 테스트 + 실행 스레드 assertion |

### 9.1 테스트 seam의 요구사항

대역은 `GpuLeg`의 실제 GPU frame 실행 결과와 blocking 지점만 바꾼다. Native/Injected가 서로 다른 scheduling/receipt 논리를 갖지 않도록 공통 코드를 추출한다. UI 예약, handle lookup, slot load, worker 실행, report, lifecycle 검증, IPC wait는 생산 경로를 그대로 사용한다.

R1에서 단순히 `GpuWorker.enqueue(|| ...)`를 호출하거나, 전역 락 없는 독립 mock 함수를 호출한 뒤 성공으로 판정하지 않는다. R5는 반드시 “최초 결과 deferred → worker completion → IPC resolve”를 통과해야 한다. R12도 순서가 올바른 별도 mock 구조체만 테스트해서는 안 된다.

### 9.2 기존 회귀를 유지할 항목

한글 preedit의 wide cell/tail, cursor geometry, 선택 영역·스크롤 위치, DPI-only resize의 PTY pixel reply, synchronized-output deadline, replay 후 pane dimensions 복구, fresh startup의 PTY 응답 보존, background agent state, remote generation, 첫 present 전 hidden, pointer transparency, glyph atlas/이미지 rendering 계약을 계속 실행한다.

## 10. 실행한 검증과 후속 실행 명령

이번 조사에서 아래 명령을 실제 실행했다. 모두 exit code 0이었다. 기존 unused/dead-code 등의 compiler warning은 있었으며, warning 정리를 위한 unrelated 수정을 하지 않았다.

| 명령 | 실제 결과 | 증명하지 못하는 범위 |
|---|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::` | 219 passed, 0 failed, 0 ignored | 모든 GUI Native 비동기 동작·다른 OS 실행 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract --test native_terminal_renderer_contract --quiet` | 26개 + 20개, 합계 46 passed | 실제 창/OS compositor 경합 전체 |
| `bun test scripts/native-terminal-thread-policy.test.mjs` | 11 passed, 0 failed, 21 assertions | 간접 호출·스레드 권한·전역 VT 락 부재 |

정책 테스트에는 shipped backend tree 검사도 포함된다. CI 연결은 [.github/workflows/build-test.yml](../.github/workflows/build-test.yml) 43, 48행에서 확인했다.

향후 구현 후 기본 검증 명령:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract --test native_terminal_renderer_contract
bun scripts/native-terminal-thread-policy.mjs
bun test scripts/native-terminal-thread-policy.test.mjs
cargo check --manifest-path src-tauri/Cargo.toml
```

추가한 R1–R15와 `NativeTerminalPane` 관련 프런트엔드 회귀 테스트를 실제 사용 중인 runner로 실행한다. 각 OS의 compile과 live-window 검증을 별도로 기록한다. macOS에서 headless/MockRuntime 테스트가 통과했다는 이유로 Windows/Linux GREEN을 기록하지 않는다.

### 10.1 CI 게이트 보강

[현재 AST 규칙](../scripts/native-terminal-thread-policy.rules.yml)은 `run_on_main_thread` 및 `dispatch_render_on_main_thread`의 lexical 내부에서 특정 GPU 메서드를 탐지한다. 함수 본문을 따라가는 call-graph 분석은 아니다. 간접 helper, constructor, 전역 sessions 경합은 빠진다.

보강 시 실제 UI prepare/completion 함수의 경계를 좁혀 registry/VT 접근 금지 fixture를 추가한다. GPU 권한은 타입과 runtime assertion으로도 확인한다. `render_snapshot`이라는 이름을 모든 lexical 자손에서 금지하면, UI가 enqueue한 GPU closure까지 오탐할 수 있으므로 합법적인 nested worker fixture도 추가한다. 정규식으로 안전성을 대체하지 않는다.

## 11. 최종 완료 기준과 성능 확인

**기능 완료 조건:** 실제 Native 준비/완료 경로에서 전역 sessions/VT 락을 획득하지 않고, VT 생산자도 전역 registry를 잡고 작업하지 않는다. UI가 VT/GPU에 동기 대기하지 않는다. bounds는 실제 report로 끝나며, latest frame·세대·동기 출력·IME·detach 의미를 보존한다. 표준 wgpu surface 경로가 네 플랫폼에서 유지된다. retirement/실패가 spin, stale reveal, 미완료 waiter를 만들지 않는다.

**실기기 시나리오:** macOS Metal, Windows DX12, Linux X11, Linux Wayland에서 각각 1/4/8개 패널을 열고 A 대량 출력, B 입력 및 TUI redraw, 빠른 resize/DPI 변경, tab 전환·detach/reattach, 창 최소화/복원, 출력 없는 첫 표시, GPU frame 중 close를 확인한다. 실행 중인 daemon이나 PTY를 성능 측정 목적으로 재시작하지 않는다.

측정 항목은 UI heartbeat 지연, 입력→화면 반영 p50/p95/p99, VT feed/snapshot 시간, registry 및 per-session guard 보유 시간, worker queue depth, snapshot age, acquire/submit/present 소요, retry/occlusion 횟수, 유지 중 frame 수와 메모리다. raw 로그와 하드웨어/OS/backend/패널 수를 함께 남긴다.

원본 설계의 CPU·프레임 예산은 목표/과거 관측이지 이번 실행 결과가 아니다. 첫 수정 전후 같은 환경에서 비교하고 제품 성능 예산을 고정한다. idle에서는 렌더 자체가 잠들어야 한다. 단일 GPU worker의 backend 대기 때문에 남는 패널 간 지연은 별도 측정값으로 보고하며, “락 제거 완료”를 “모든 acquire 대기 제거”로 바꾸어 말하지 않는다.

## 12. 공식 API 확인 자료와 적용 범위

아래 자료는 API 사용 방향 확인용이다. 저장소의 구현 상태와 테스트 결과는 위에 명시한 실제 소스/명령이 근거다.

- [ArcSwapOption 공식 API](https://docs.rs/arc-swap/1.9.2/arc_swap/type.ArcSwapOption.html): nullable Arc의 원자적 저장소. 새 슬롯 구현의 후보이며 현재 Ferryx에 이미 도입된 라이브러리라는 뜻이 아니다.
- [ArcSwapAny load/store 및 consistency](https://docs.rs/arc-swap/1.9.2/arc_swap/struct.ArcSwapAny.html): 소유된 포인터 load와 관련 값을 한 번의 load로 읽는 일관성 지침. 세션별 generation, 메모리 상한, wake 프로토콜은 Ferryx에서 따로 설계·검증해야 한다.
- [wgpu 30.0.0 Instance API](https://docs.rs/wgpu/30.0.0/wgpu/struct.Instance.html): 표준 window-handle `create_surface`와 unsafe target 경로가 구분되어 있다. 코드의 wgpu 30 의존성과 맞춰 확인했다.
- [raw-window-metal Layer API](https://docs.rs/raw-window-metal/latest/raw_window_metal/struct.Layer.html): `from_ns_view`의 메인 스레드 요구. 이것만으로 Ferryx 전체의 surface drop·native target 수명 안전성이 증명되는 것은 아니다.

## 13. 최종 구현 방침

**직접 Metal 레이어 우회를 걷어내고, 기존 GPU worker를 재사용한다. registry를 수명 관리로 축소하고, 세션별 VT 생산자가 완성 frame을 원자적으로 발행하게 한다. 렌더 준비와 완료는 캐시된 render handle만 사용한다. 실제 GPU report를 bounds IPC에 연결하고, 스레드·세대·폐기 계약을 생산 경로 테스트로 증명한다.**

이 순서가 현재 구현을 가장 많이 보존하면서 실제 남은 전역 락과 비동기 경계 결함을 해결하는 경로다. 커스텀 renderer/backend, 패널별 GPU device, 대규모 snapshot 표현 변경은 이 작업의 선행 조건으로 삼지 않는다.
