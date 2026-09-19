# 네이티브 터미널 스레드 소유권 재설계 — 간헐적 UI 무응답/지연의 근본 해결

**작성일:** 2026-09-19
**대상:** `src-tauri/src/native_terminal/` 전체, `src-tauri/src/ipc/native_terminal.rs`
**상태:** 설계 제안 (미승인, 코드 변경 없음)
**HEAD:** 44cef470

---

## 1. 현상과 실측 증거

증상: 간헐적으로 TUI가 무응답이 되거나 극심하게 지연된다.

실행 중인 릴리스 앱(PID 18189)을 두 번 프로파일했다. 설치본
`/Applications/Ferryx.app/Contents/MacOS/ferryx`와 로컬 빌드
`src-tauri/target/release/ferryx`의 UUID가
`BDFC6DDD-DA1B-3FFD-BC40-BCED20AC4671`로 일치하므로, 샘플은 현재 소스와 동일한
빌드다. 바이너리는 스트립되어 ferryx 프레임은 심볼이 없지만 시스템 프레임
(QuartzCore / AGXMetalG16X / IOGPU / AppKit / WebKit)은 해석된다.

### 1.1 메인 스레드가 GPU 프레임을 돌리고 있다

1차 샘플(6초, 메인 스레드 482 샘플):

```
102 __CFRUNLOOP_IS_CALLING_OUT_TO_AN_OBSERVER_CALLBACK_FUNCTION__
 └ 102 (in ferryx) …
    ├ 16 -[CAMetalLayer nextDrawable]
    │    → CAMetalLayerPrivateNextDrawableLocked
    │    → _dispatch_semaphore_wait_slow → semaphore_timedwait_trap
    ├ -[AGXG16XFamilyCommandQueue commandBuffer]
    ├ -[AGXG16XFamilyCommandBuffer renderCommandEncoderWithDescriptor:]
    ├ -[AGXG16XFamilyCommandBuffer commit] → -[IOGPUMetalCommandBuffer commit]
    ├ IOGPUResourceCreate
    └ -[NSView _layoutSubtreeWithOldSize:] / NSPerformVisuallyAtomicChange
```

- 메인 스레드 시간의 **21%(102/482)** 가 tao 런루프 옵저버 콜백 안에서 ferryx의
  렌더 작업을 수행 중.
- 그중 **16 샘플은 `nextDrawable`의 세마포어에서 잠들어 있음** — 메인 스레드가
  드로어블을 기다리며 완전히 블로킹된 상태.
- `_layoutSubtreeWithOldSize:` 15 샘플, `NSPerformVisuallyAtomicChange` 24 샘플 —
  프레임마다 AppKit 레이아웃 패스가 돈다.

2차 샘플(5초, 약 6분 뒤, 사용자 조작 없음): 동일 경로에 **106 샘플**, 유휴
`mach_msg2_trap` 301 샘플. 지속적인 현상이다.

### 1.2 웹뷰 IPC가 그 뒤에 줄을 선다

1차 샘플에서 **45/482 샘플**이 다음 스택에 묶여 있었다:

```
-[WKURLSchemeTaskImpl didReceiveResponse:] / didFinish
 → getExceptionTypeFromMainRunLoop
 → WTF::callOnMainRunLoopAndWait
 → WTF::ThreadCondition::timedWait
```

터미널 출력 채널(`ipc/terminal.rs:488-505`)은 Tauri `Channel`로, WebKit 커스텀 URL
스킴 응답 경로를 탄다. 그 완료 콜백이 메인 런루프를 기다린다. 즉 **메인 스레드가
렌더 중이면 PTY 출력 전달 자체가 정지한다.** 렌더와 출력 전달이 같은 자원을 두고
서로를 굶기는 구조다.

### 1.3 유휴 상태에서 CPU 20%를 계속 태운다

| 구간 | CPU 시간 | 벽시계 | 점유율 |
|---|---|---|---|
| t0 → t1 | 3:50.28 → 3:54.19 | 20초 | 19.6% |
| t1 → t2 | 3:54.19 → 4:58.61 | 약 5.5분 | 19.5% |

사용자 조작이 없는 상태에서 코어 하나의 20%를 지속 소모한다.

### 1.4 환경 증폭 요인 (관측값)

- 데몬 3중 체인: `910`(가동 11:53:48, CPU 20:41, 세션 14개 보유), `90242`(세션 2개),
  `16057`(현재). `/tmp/rorca-501/handover_routes.json` 기준 대부분의 출력이
  `PTY → 구데몬 → UDS → 현데몬 → UDS → GUI`로 프로세스 홉 2개를 더 거친다.
- GUI 스레드 90개(tokio-rt-worker 71, notify-rs fsevents loop 13).
- 머신 메모리 압박: `PhysMem: 47G used, 116M unused, 3060M compressor`,
  swapouts 7,371,981. 이 상태에서 CoreAnimation/GPU 드라이버 지연이 커지면
  §1.1의 드로어블 대기가 수 ms에서 수백 ms~초로 늘어난다. 증상이 "항상"이 아니라
  "간헐적"인 이유다.

---

## 2. 근본 원인 (구조적)

> **이 절은 작성 시점(1·4단계 착수 전)의 분석이다.** C2·C3는 이후 해소되었다.
> 각 원인의 현재 상태는 [12.9 대조표](#129-c1c7--단계-현재-상태-대조표)를 먼저 볼 것.

### C1. 프레임 전체가 메인 스레드 클로저 안에 있다

`surface_host.rs:390`의 `dispatch_render_on_main_thread(&window, move || { … })`,
`surface_host.rs:506-515`의 `window.run_on_main_thread(task)`.

이 클로저 하나가 다음을 전부 수행한다:

1. `hosts.lock()` (`:394`), `sessions.lock()` (`:396`)
2. `session_render_snapshot(session)` (`:722-768`) — VT 전체 화면 스냅샷 생성
3. `host.update_viewport(...)` — AppKit geometry 변경
4. `NativeSurfaceFrameTarget::render_snapshot` (`:2664-`) — 설정, 인스턴스 재구성,
   아틀라스 업로드
5. `self.surface.get_current_texture()` (`:2708`) — **드로어블 대기 블로킹**
6. `renderer.present(frame)`, `reveal_after_present()`, `restore_first_responder()`
   (`:2757-2759`)

`gpu_context.rs:129-140`이 `present_mode: wgpu::PresentMode::AutoVsync`,
`desired_maximum_frame_latency: 2`이므로 5번은 **정의상 호출 스레드를 블로킹한다.**
그 호출 스레드가 메인 스레드다.

### C2. present 실패가 무제한 즉시 재시도로 이어진다

`surface_host.rs:2708-2742`:

```rust
let frame = match self.surface.get_current_texture() {
    Success(frame) | Suboptimal(frame) => Some(frame),
    Lost | Outdated => { /* reconfigure 후 1회 재시도 */ }
    ref other => match classify_surface_error(other)? { SurfaceFrameAction::Drop => None },
};
…
let Some(frame) = frame else {
    return Ok(NativeTerminalSurfaceReceipt::from_snapshot(…)); // presented:false, render_deferred:false
};
```

`surface_error.rs:12-14`는 `Timeout | Occluded`를 `Drop`으로 분류한다. 그러면
영수증 기본값(`surface_host.rs:179-180`)이 `presented:false, render_deferred:false`다.

호출자(`surface_host.rs:451-473`):

```rust
Ok(receipt) => {
    if !receipt.presented && !receipt.render_deferred {
        coordinator.schedule_render();      // RENDERING → RENDER_FOLLOW_UP
    }
}
…
if coordinator.finish_render() {            // RENDER_FOLLOW_UP → RENDER_SCHEDULED, true
    tauri::async_runtime::spawn(async move { dispatch_scheduled_render(…) });
}
```

**sleep도, 백오프도, 상한도 없다.** 창이 가려지거나(Occluded) GPU/컴포지터가
막히면(Timeout) 이 경로가 영구 루프가 되고, 매 반복이 메인 스레드에서 스냅샷 +
AppKit 레이아웃 + 드로어블 시도를 반복한다. 앱 전체 무응답의 직접 경로다.

### C3. 프레임마다 AppKit 레이아웃을 강제한다

`update_viewport`는 프레임당 두 번 호출된다 (`dispatch_scheduled_render` 내부 1회,
`surface_host.rs:2675` 1회).

`platform/macos.rs:159-186`의 `apply_viewport`에는 **변경 비교 조기 종료가 없다**:

```rust
unsafe fn apply_viewport(view: &FerryxNativeTerminalView, bounds: Option<LogicalBounds>) {
    if let Some(window) = view.window() {
        configure_window_background(&window, …);   // 매 프레임
    }
    …
    view.setFrame(NSRect::new(…));                  // 값이 같아도 매 프레임
    configure_terminal_layers(view, bounds.scale_factor);  // CALayer 속성 매 프레임
}
```

`setFrame:`은 동일 값이어도 뷰 트리 레이아웃을 무효화한다. §1.1의
`_layoutSubtreeWithOldSize:` / `NSPerformVisuallyAtomicChange` 재귀 스택이 그
결과다. `reveal()` (`macos.rs:415-427`)의 `setHidden(false)`도 매 프레임 무조건이다.

### C4. 전 세션 공용 뮤텍스 하나가 렌더와 VT 공급을 직렬화한다

`surface_host.rs:350-351`:

```rust
hosts:    Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
sessions: Arc<Mutex<HashMap<String, NativeTerminalSession>>>,
```

- 메인 스레드 렌더가 스냅샷을 뜨려고 `sessions.lock()` (`:396`)
- 모든 PTY 펌프가 출력 청크마다 같은 락을 잡고
  `sess.terminal.feed(&data)` (`:1637`) + `take_native_terminal_events(…)` (`:1650`)
- `hosts_guard`는 획득(`:394`)부터 해제(`:463` `drop(hosts_guard)`)까지 GPU 렌더와
  present 전 구간 동안 유지된다

**한 패널의 렌더가 길어지면 모든 터미널의 출력 처리가 동시에 멈춘다.**

### C5. 패널마다 wgpu 디바이스를 새로 만든다

`renderer/renderer.rs:570-575`:

```rust
pub fn new(config: RendererConfig) -> Result<Self, NativeTerminalError> {
    config.validate()?;
    let gpu = GpuContext::new()?;                 // 인스턴스 + 어댑터 + 디바이스 + 큐
    let pipelines = RenderPipelines::new(&gpu.device);
    let atlas = GlyphAtlas::new(&gpu.device);
    …
}
```

`gpu_context.rs:57,64,81`의 `pollster::block_on(instance.request_adapter(…))`,
`pollster::block_on(adapter.request_device(…))`는 **블로킹 호출**이다. 그리고
`NativeSurfaceFrameTarget::new` (`surface_host.rs:2629-2661`)는
`dispatch_scheduled_render`(`:379-`)의 **메인 스레드 클로저 안에서 레이지로 호출된다**
(`:420`, `Entry::Vacant` 분기). 즉 패널의 서피스 호스트가 처음 생길 때마다
어댑터·디바이스 생성이 메인 스레드를 수십~수백 ms 동안 멈춘다.

결과적으로 N개 패널 = N개 `wgpu::Instance` + N개 `Device`/`Queue` + N개
`GlyphAtlas` + N개 파이프라인 세트. 패널 간 글리프 아틀라스 공유가 없어 같은 글리프를
패널 수만큼 중복 래스터화한다.

### C6. 스냅샷을 프레임마다 새로 할당한다

`snapshot.rs:98-106`:

```rust
pub struct RenderSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor: CursorSnapshot,
    pub grid: Vec<Vec<CellSnapshot>>,
    pub images: Vec<ImagePlacementSnapshot>,
}
```

`CellSnapshot.text`는 `String`이다(`snapshot.rs:38-41`). 200×50 그리드면 프레임마다
`Vec` 51개 + `CellSnapshot` 10,000개 + 힙 `String` 다수를 새로 할당한다. 이 할당이
메인 스레드에서 프레임마다 일어난다(샘플의 `libsystem_malloc.dylib` 13 샘플).

### C7. 왜 반드시 재발하는가

위 여섯 가지는 전부 **관례 위반**이지 **컴파일 에러**가 아니다.
`run_on_main_thread` 클로저 안에 GPU 코드를 넣어도, `presented: bool`을 보고
즉시 재시도를 걸어도, `setFrame`을 프레임마다 호출해도 전부 컴파일된다.
`src-tauri/src/native_terminal/AGENTS.md`의 ANTI-PATTERNS 목록이 계속 길어지는 것 자체가
"컴파일러가 안 막아준다"는 증거다. 규칙을 문서로 관리하는 한 같은 결함이 다시 들어온다.

---

## 3. 목표 / 비목표

### 목표

- G1. 메인 스레드에서 GPU 프레임 작업을 **구조적으로 불가능**하게 만든다.
- G2. 출력이 없을 때 프레임 수가 0이 된다(유휴 CPU 0에 수렴).
- G3. present 실패가 즉시 재시도로 이어지는 코드를 **작성 불가능**하게 만든다.
- G4. 렌더 경로에서 전역 뮤텍스를 제거한다. 한 패널의 부하가 다른 패널의 출력 처리를
      막지 않는다.
- G5. macOS / Windows / X11 / Wayland에 **동일한 규칙**이 적용된다. 플랫폼별 예외
      분기를 만들지 않는다.
- G6. 위반이 CI에서 검출된다.

### 비목표

- 렌더러 픽셀 파이프라인(셰이더, 감마 보정, 아틀라스 포맷) 변경. 현재 동작 유지.
- 원격/페어드 세션 프로토콜 변경.
- 데몬 핸드오버 체인 정리(별건, §9에 기록).

---

## 4. 설계: 소유권 3분할과 단일 명령 경계

### 4.1 소유자

| 소유자 | 담당 | OS 제약 근거 |
|---|---|---|
| **UiThread** (Tauri 메인) | 차일드 뷰/HWND/subsurface 생성·파괴, geometry, 표시·숨김, first responder, `create_surface`, surface reconfigure 명령 발행 | macOS AppKit 전용 / Windows HWND owner / X11·Wayland 커넥션 소유자 |
| **VtThread** (세션별 태스크) | PTY 바이트 → `terminal.feed`, 이벤트 추출, **스냅샷 생산 및 publish** | 없음. `NativeTerminal`은 이미 `Send` (`terminal.rs:125`) |
| **GpuThread** (프로세스당 1개) | 단일 `GpuContext` 소유, acquire·encode·submit·present, 아틀라스 업로드, 프레임 페이싱 | 없음. surface *생성*만 UiThread |

셋은 **서로를 기다리지 않는다.**

### 4.2 경계 자료구조

```
VtThread ──(SnapshotSlot: 트리플 버퍼, publish only)──▶ GpuThread
UiThread ──(SurfaceCommand: 바운드 채널, 최신값 우선)──▶ GpuThread
GpuThread ──(FrameReport: 논블로킹)──────────────────▶ UiThread / 스케줄러
```

**SnapshotSlot (세션당 1개)**

- 슬롯 3개: `writing` / `ready` / `reading`. VtThread는 `writing`에 쓰고 원자적으로
  `ready`와 교체한다. GpuThread는 `ready`를 `reading`과 교체해 읽는다.
- VtThread는 **절대 블로킹되지 않는다.** 렌더가 느리면 중간 세대가 덮어써질 뿐이다.
  터미널은 최종 화면 상태만 정확하면 되므로 중간 프레임 폐기가 안전하다.
- 버퍼는 재사용한다. C6의 프레임당 재할당이 사라진다. `CellSnapshot.text: String`는
  `SmallVec<[u8; 8]>` 또는 인라인 grapheme 버퍼로 교체해 힙 할당을 제거한다.
- 이로써 GpuThread는 **`NativeTerminal`을 전혀 건드리지 않는다** → C4의 `sessions`
  락이 렌더 경로에서 완전히 사라진다. `hosts`는 GpuThread 단독 소유가 되므로 뮤텍스
  자체가 없어진다. 락을 잘 잡는 게 아니라 **공유를 없앤다.**

**SurfaceCommand (UiThread → GpuThread)**

```rust
enum SurfaceCommand {
    Attach { session: SessionId, surface: SendableSurface, size: PhysicalSize },
    Resize { session: SessionId, size: PhysicalSize },   // 실제 변경 시에만
    Visibility { session: SessionId, visible: bool },
    Detach { session: SessionId },
}
```

geometry가 **실제로 변할 때만** 발행한다. C3의 프레임당 `setFrame`이 사라진다.

**FrameReport (GpuThread → UiThread)**

```rust
enum FrameReport {
    FirstPresent { session: SessionId },   // UiThread가 reveal() 1회만 수행
    Fatal { session: SessionId, error: NativeTerminalError },
}
```

`reveal()`은 최초 present 1회만. `restore_first_responder`도 present 훅에서 분리해
포커스 변경 이벤트에만 반응하게 한다.

### 4.3 프레임 클록

세션별 `RenderScheduleCoordinator`(`surface_host.rs:202-298`)를 제거하고 GpuThread에
`FrameClock` 하나를 둔다.

- dirty 세션 집합(`HashSet<SessionId>`)을 모아 **한 tick에 전부** 그린다.
- dirty가 비면 tick 자체를 건너뛴다 → G2 달성.
- tick 소스:
  - macOS: `CVDisplayLink` (또는 `CADisplayLink`)
  - Windows: DXGI waitable swapchain object / `DwmFlush` 폴백
  - Linux: `wp_presentation` feedback / 고정 상한 폴백
- 상한은 디스플레이 리프레시. 여러 패널이 있어도 **tick당 1회 present 라운드**이므로
  패널 수에 프레임 레이트가 반비례하지 않는다.

---

## 5. 재발 차단 장치 (G3, G6)

### 5.1 `!Send` 토큰으로 스레드 소속을 타입에 새긴다

```rust
/// UiThread에서만 발급된다. !Send 이므로 다른 스레드로 옮길 수 없다.
pub struct UiThread(PhantomData<*const ()>);

/// GpuThread 워커 루프에서만 발급된다.
pub struct GpuThread(PhantomData<*const ()>);
```

API 시그니처를 바꾼다:

```rust
impl PlatformCompositorTarget {
    pub fn set_geometry(&self, _: &UiThread, bounds: LogicalBounds);
    pub fn set_visible(&self, _: &UiThread, visible: bool);
    pub fn create_surface(&self, _: &UiThread, gpu: &GpuContext)
        -> Result<SendableSurface, NativeTerminalError>;
}

impl SurfaceFrame {
    pub fn acquire(&mut self, _: &GpuThread) -> FrameOutcome;
    pub fn present(self, _: &GpuThread);
}
```

GpuThread 루프에는 `UiThread` 토큰이 존재하지 않으므로 `setFrame`/`SetWindowPos`
호출이 **컴파일 에러**가 된다. 반대로 UiThread 클로저에는 `GpuThread` 토큰이 없으므로
`get_current_texture` 호출이 컴파일 에러가 된다.

동시에 `macos.rs:152-153`, `windows.rs:214-215`, `linux.rs:247-248`의 전면
`unsafe impl Send + Sync`를 걷어낸다. 실제로 스레드를 넘어야 하는 것은 wgpu surface
핸들과 raw window handle뿐이므로, 그 조각만 `SendableSurface`로 좁게 감싼다.

### 5.2 "즉시 재시도"를 표현 불가능하게 만든다

C2의 원인은 `presented: bool`이다. 불린을 보고 호출자가 정책을 결정한다. 값을 바꾼다:

```rust
pub enum FrameOutcome {
    Presented,
    /// 반드시 미래 시각을 들고 있어야 한다. `NextFrameAt`은 `Instant`로부터만
    /// 생성되며 생성자가 최소 간격을 강제한다.
    Retry(NextFrameAt),
    /// Occluded/최소화. 시간으로 깨어나지 않고 이벤트로만 깨어난다.
    Suspend(WakeOn),
    Fatal(NativeTerminalError),
}

pub enum WakeOn { Visible, Resized, Reattached }

pub struct NextFrameAt(Instant);
impl NextFrameAt {
    /// 최소 한 프레임 간격 뒤로만 만들 수 있다. `now()` 생성자는 존재하지 않는다.
    pub fn after_backoff(attempt: u32, refresh: Duration) -> Self { … }
}
```

매핑 (`surface_error.rs` 대체):

| `wgpu::CurrentSurfaceTexture` | 현재 | 변경 후 |
|---|---|---|
| `Success` / `Suboptimal` | present | `Presented` |
| `Timeout` | drop → **즉시 재시도** | `Retry(NextFrameAt::after_backoff(…))` |
| `Occluded` | drop → **즉시 재시도** | `Suspend(WakeOn::Visible)` |
| `Lost` / `Outdated` | reconfigure 1회 | reconfigure 후 `Retry(…)` |
| `Validation` | 에러 | `Fatal(…)` |

`FrameClock`은 `NextFrameAt` 없이는 다음 프레임을 예약할 수 없고, `Suspend`는 타이머가
아니라 `SurfaceCommand::Visibility` 수신으로만 해제된다. **무한 스핀이 문법적으로
작성 불가능해진다.**

### 5.3 CI 게이트 (문서가 아니라 검사)

이 저장소는 이미 `scripts/release-workflow-policy.mjs`로 소스에서 정책을 강제하는
패턴을 쓴다. 같은 방식으로:

1. **ast-grep 규칙** — `run_on_main_thread($$$)` 클로저 본문에
   `get_current_texture` / `submit` / `present` / `configure_surface` /
   `request_adapter` / `request_device` 호출 금지.
2. **계약 테스트** — 기존 테스트 심(`surface_host.rs:2849-2864`의 `RenderDispatch`,
   이미 `owner_thread` 비교 단언을 가지고 있다)을 확장해
   "UiThread에서 실행된 GPU 호출 수 == 0"을 단언.
3. **유휴 예산 테스트** — 출력이 없는 상태로 N tick 동안 `FrameClock`이 발행한 프레임
   수 == 0.
4. **재시도 상한 테스트** — `Timeout`을 연속 주입해도 단위 시간당 프레임 수가 리프레시
   상한을 넘지 않고, `Occluded` 주입 시 프레임 수가 0이 되는 것을 단언.

---

## 6. 플랫폼별 계약

네 플랫폼 모두 **"창·지오메트리 = UiThread, 픽셀 = GpuThread, 경계는 명령 큐 하나"**로
수렴한다. 플랫폼별 분기가 필요 없다는 것이 이 설계의 핵심 근거다.

### macOS

- `CAMetalLayer nextDrawable` / present는 임의 스레드에서 호출 가능(MTKView도 전용
  스레드 드로잉을 지원). 메인 전용은 NSView 생성, `setFrame`, `setHidden`,
  first responder뿐 — 전부 UiThread 담당으로 이미 분류된다.
- `MacosCompositorTarget`은 이미 `Send + Sync`(`macos.rs:152-153`)이므로 핸들 이동은
  가능하다. 토큰 도입 시 이 전면 허용을 좁힌다.
- `surface.configure`는 CAMetalLayer 속성을 건드리므로 암묵적 CATransaction이 생긴다.
  `SurfaceCommand::Resize`로 받아 GpuThread에서 명시적 `CATransaction` 안에서 수행하거나,
  UiThread에 마샬링한다. 어느 쪽이든 **프레임 경로 밖**이다.

### Windows

- `SetWindowPos`(`windows.rs:302,328`)는 HWND 소유 스레드에 **동기 메시지**를 보낸다.
  렌더 스레드에서 호출하면 UI 메시지 펌프에 블로킹된다 — macOS와 정확히 같은 병목이
  방향만 바꿔 재현된다. geometry를 UiThread 전용으로 못 박는 것이 Windows에서는
  선택이 아니라 **필수**다.
- `windows.rs:11`이 이미 "child HWND는 Tauri 메인 스레드에서 생성/파괴"라고 문서화하고
  있어 방향이 일치한다. 토큰은 이 문서를 타입으로 승격시키는 것이다.
- DX12 백엔드일 경우 `IDXGIFactory::MakeWindowAssociation(DXGI_MWA_NO_WINDOW_CHANGES)`로
  DXGI의 윈도우 메시지 후킹을 끊어야 고전적인 렌더/UI 데드락을 피한다.

### Linux / X11

- Vulkan present는 임의 스레드에서 가능.
- Xlib를 두 스레드에서 호출하는 것이 위험 요소인데, X 커넥션 조작
  (`linux.rs:357` `update_viewport`, `:397` `reveal`)을 UiThread 단독 소유로 두면
  `XInitThreads()` 문제 자체가 발생하지 않는다.

### Linux / Wayland

- 넷 중 가장 까다롭다. `wl_display_flush`(`wayland_child.rs:355,392,406,432`)와
  subsurface commit은 부모 surface commit과 순서가 묶인다.
- 해법: **desynchronized subsurface + GpuThread 전용 `wl_event_queue`**.
  `wl_subsurface.set_position`과 부모 커밋은 UiThread가 담당한다.
- 컴포지터 구현 편차가 있어 desync가 기대대로 동작하지 않을 수 있다. §9 R1 참조.

---

## 7. 이행 계획

순서에 의미가 있다. 락을 먼저 걷어내지 않고 스레드를 분리하면 데드락 표면이 오히려 늘어난다.

### 1단계 — 출혈 정지 (작고 즉효)

- `FrameOutcome` 타입 도입, `presented: bool` 제거. §5.2 매핑 적용.
- `FrameClock` 도입, 세션별 `RenderScheduleCoordinator` 제거.
- `apply_viewport`(`macos.rs:159-186`), `windows.rs:293-333`, `linux.rs:357-410`에
  이전 bounds 비교 조기 종료 추가.
- `reveal()`을 최초 present 1회로 제한. `restore_first_responder`를 present 훅에서 분리.
- **기대 효과:** C2 무한 스핀 제거, §1.3의 유휴 20% CPU 제거, C3의 프레임당 AppKit
  레이아웃 제거. 스레드 구조는 그대로.

### 2단계 — 렌더 경로에서 락 제거

- `SnapshotSlot` 트리플 버퍼 도입. VtThread가 publish, 렌더가 consume.
- `RenderSnapshot`을 플랫 버퍼 + 인라인 grapheme 저장으로 교체(C6).
- 렌더 경로에서 `sessions.lock()` / `hosts.lock()` 제거.
- **기대 효과:** C4, C6 해소. 한 패널 폭주가 전체를 멈추는 문제 소멸.

### 3단계 — GpuThread 분리

- 프로세스당 단일 `GpuContext`로 통합(C5). `NativeTerminalRenderer::new`가
  `GpuContext::new()`를 호출하지 않고 공유 컨텍스트를 참조하도록 변경.
- `pollster::block_on(request_adapter/request_device)`를 GpuThread 기동 시 1회로 이동 —
  메인 스레드에서 영구히 제거.
- GpuThread 워커 + `SurfaceCommand` / `FrameReport` 채널 도입.
- `UiThread` / `GpuThread` 토큰 도입, 전면 `unsafe impl Send + Sync` 축소.
- **기대 효과:** C1, C5 해소. G1 달성.

### 4단계 — 게이트

- §5.3의 ast-grep 규칙 + 계약 테스트 4종.
- `src-tauri/src/native_terminal/AGENTS.md`의 해당 ANTI-PATTERNS 항목을 "규칙" 서술에서
  "검사 위치" 참조로 교체.

1단계만으로도 실측된 증상의 큰 부분이 사라지고, 3·4단계가 "재발 불가능"을 담보한다.

---

## 8. 검증 기준

| 항목 | 방법 | 통과 기준 |
|---|---|---|
| 유휴 CPU | 출력 없는 상태로 60초, `ps -o time=` 델타 | < 1% (현재 19.5%) |
| 메인 스레드 GPU 작업 | `sample` 후 메인 스레드 스택에서 QuartzCore/AGXMetal/IOGPU 프레임 검색 | 0 샘플 (현재 nextDrawable 16 + Metal 다수) |
| 웹뷰 IPC 블로킹 | `sample`에서 `callOnMainRunLoopAndWait` 샘플 수 | 유의미하게 감소 (현재 45/482) |
| 무한 스핀 | `Timeout`/`Occluded` 연속 주입 계약 테스트 | 리프레시 상한 이하 / `Occluded` 시 0 |
| 패널 수 확장성 | 패널 1·4·8개에서 tick당 present 라운드 수 | 패널 수와 무관하게 1 |
| 크로스플랫폼 | Windows/Linux 런타임에서 동일 계약 테스트 | 전부 통과 |

플랫폼별 런타임 검증은 macOS 로컬 + Windows/Linux 대상 호스트에서 각각 수행한다.

---

## 9. 리스크와 폴백

**R1. Wayland desynchronized subsurface의 컴포지터 편차 (중)**
기대대로 동작하지 않는 컴포지터가 있을 수 있다. 폴백은 synchronized subsurface +
UiThread present이고, 그 폴백은 현재와 같은 성능 특성을 갖는다.
→ 예외를 허용하되 `WaylandSyncFallback` 토큰 타입으로 명시해 암묵적으로 번지지 않게 한다.

**R2. 단일 `GpuContext` 통합 시 디바이스 로스트 영향 범위 확대 (중)**
현재는 패널별 디바이스라 한 패널의 디바이스 로스트가 격리된다. 통합하면 전체가 영향을
받는다. → `FrameOutcome::Fatal` 수신 시 GpuThread가 컨텍스트를 재생성하고 모든 세션
surface를 재부착하는 복구 경로를 1급으로 설계한다. 계약 테스트로 커버.

**R3. 규모 (중)**
`surface_host.rs` 5,486줄 재구성. 단계별로 쪼개도 2·3단계는 크다.
→ 단계마다 기존 계약 테스트
(`native_terminal_surface_host_contract`, `native_terminal_renderer_contract`)를
그린으로 유지하는 것을 게이트로 삼는다.

**R4. `input.rs:257`의 `thread_local! LOCAL_INPUT_STATE` (낮음)**
스레드가 갈리면 조용히 깨지는 패턴이지만, 현재 `#[allow(dead_code)]`이고 참조가
`input.rs` 내부뿐이다. 3단계 전에 제거하거나 세션 소유 상태로 이전한다.

**R5. 공유 워킹 트리 (낮음)**
현재 `src-tauri/src/ssh/direct.rs`, `direct_tests.rs`, `runtime.rs`에 다른 세션의
미커밋 변경이 있다. 이 설계의 변경 범위와 겹치지 않는다.

---

## 10. 이 설계 범위 밖의 별건

투자 대비 효과가 커서 기록해 둔다. 위 설계와 독립적으로 처리 가능하다.

1. **데몬 핸드오버 체인** — 현재 3개 데몬이 동시 구동 중이고 구데몬 `910`이 아직
   세션 14개를 보유한다. 앱 교체마다 프록시 홉이 늘어난다. 세션 마이그레이션 완료 후
   구데몬을 은퇴시키는 경로가 필요하다.
2. **`emit_terminal_replay_gap`의 대형 페이로드** — `ipc/terminal.rs:536-560`이 링버퍼
   히스토리 전체를 base64 JSON으로 `app.emit` 한다(512KiB → 약 683KB 문자열).
   ReplayGap은 출력이 폭주할 때 발생하므로 **가장 바쁜 순간에** 메인 스레드로 대형
   페이로드를 밀어 넣는다. 바이너리 채널 경로로 통일해야 한다.
3. **Tauri 이벤트 리스너 경쟁** — `/tmp/rorca-501/boot-trace.log`에
   `TypeError: undefined is not an object (evaluating 'listeners[eventId].handlerId')`가
   1초 간격 3회 기록됐다. async `listen`/`unlisten` 경쟁으로 인한 구독 누수 신호다.

---

## 11. 결론

가능하다. 그리고 플랫폼별 특수 분기 없이 하나의 규칙으로 떨어진다:

> **창·지오메트리는 UiThread, 픽셀은 GpuThread, VT는 VtThread. 경계는 명령 큐와
> 스냅샷 슬롯뿐. 각 경계는 `!Send` 토큰으로 타입에 새긴다.**

"근본적"인 이유는 메인 스레드 블로킹이 성능 튜닝이 아니라 **타입 시스템에서 제거**되기
때문이고, "재발 불가능"한 이유는 잘못된 스레드에서의 호출과 즉시 재시도가 둘 다
**컴파일되지 않기** 때문이다. 문서에 규칙을 적는 대신 컴파일러와 CI가 강제한다.


---

## 12. 구현 중 확인된 정정 사항 (2026-09-19, 코드 실측 기반)

이 절은 1~11절을 **덮어쓴다**. 아래 항목은 추측이 아니라 코드를 읽고 측정해서 확인한
결과이며, 원안의 전제가 틀린 부분을 바로잡는다.

### 12.1 2단계(락 제거)를 3단계(GpuThread)보다 먼저 하면 얻는 것이 없다

원안은 2단계에서 "전 세션 공용 뮤텍스"를 제거하면 세션이 병렬로 렌더된다고 가정한다.
실제 코드는 그렇지 않다.

- `surface_host.rs`의 `sessions.lock()`은 블록 스코프 안에 있고 **GPU 작업 전에 이미
  드롭된다.** 스냅샷만 뜨고 나간다. 즉 C4가 지목한 락은 렌더를 직렬화하지 않는다.
- GPU 작업 구간을 실제로 덮고 있는 것은 `hosts.lock()` 쪽이다.
- 그러나 **그 클로저 전체가 `dispatch_render_on_main_thread` → `window.run_on_main_thread`
  안에서 돈다.** 모든 세션의 렌더가 메인 스레드 하나에 큐잉되므로, 뮤텍스를 없애도
  동시에 도는 렌더는 여전히 0이다.

결론: **직렬화의 주체는 뮤텍스가 아니라 메인 스레드 깔때기다.** 2단계를 단독으로 수행하면
측정 가능한 개선이 없다. 3단계(GpuThread 분리)를 먼저 하고, 2단계는 그 뒤의 정리 작업으로
내려야 한다.

### 12.2 CI 게이트(5.3)만으로는 이 불변식을 지킬 수 없다

ast-grep 규칙은 **어휘적(lexical)** 이다. 실제 위반은 `render_snapshot` 안에 있고, 이는
디스패치 클로저와 **다른 함수**다. 어떤 패턴 규칙도 그 호출을 따라갈 수 없다.

또한 최초 규칙은 `$W.run_on_main_thread($$$)` 직접 표기만 잡았는데, 이 저장소는 렌더
지점에서 그 이름을 쓰지 않고 `dispatch_render_on_main_thread`에 클로저를 넘긴다. 그래서
게이트가 **위반을 품은 트리를 깨끗하다고 보고했다.** 래퍼 표기를 규칙에 추가해 그 사각지대는
닫았지만, 함수 경계를 넘는 위반은 원리적으로 못 잡는다.

결론: 게이트는 직접적인 실수를 막는 백스톱일 뿐이다. "메인 스레드에서 GPU 작업 금지"를
실제로 강제하는 것은 5.1의 `!Send` 토큰뿐이며, 이것이 선택이 아니라 필수다.

### 12.3 8절 검증 기준 중 POC 관련 항목은 그대로는 실행 불가능하다

- `cargo run --example native_terminal_renderer_poc`는 **헤드리스로 돌고 1초 안에 종료**한다
  (50프레임 렌더 → PNG 저장 → exit). 이미 끝난 프로세스는 `sample <pid> 5`로 뜰 수 없다.
  지속 실행에는 `--window`가 필요하다.
- 그런데 `--window` 모드는 `ControlFlow::Wait`이고 PTY 출력이 없어서 **설계상 유휴**다.
  여기서 나온 CPU 0%와 Metal 샘플 0건은 POC가 조용하다는 뜻이지, 1단계가 스핀을
  제거했다는 증거가 아니다. 실측 기준선(코어의 19.5%, `nextDrawable` 블로킹 16샘플)은
  실제 GUI 프로세스에서 나온 값이므로, 검증도 실제 앱에서 해야 의미가 있다.

### 12.4 유휴 0프레임(8절)은 현재 코드에서 이미 성립한다

`finish_render()`는 렌더 중 새 출력이 없었으면 `false`를 반환하므로, 완료된 프레임이 스스로
재예약하지 않는다. 즉 이 항목은 RED를 만들 수 없고, 회귀 방지용 특성화 테스트로만 의미가
있다. 유휴 CPU를 태우는 실제 원인은 "재예약"이 아니라 **합침이 시간 기반이 아니었던 것**이며,
이는 `FrameClock`(프레임 시작 기준 페이싱)으로 처리했다.

### 12.5 3단계(GpuThread)는 "클로저를 옮기는 것"이 아니다 — 프레임당 2회 스레드 홉이 필요하다

실측으로 확인한 두 가지 사실:

1. **`NativeTerminalSurfaceHost`는 이미 `Send`다.** 컴파일 프로브
   (`fn require_send<T: Send>(); require_send::<NativeTerminalSurfaceHost>();`)가 통과한다.
   즉 호스트를 워커 스레드로 옮기는 것 자체는 타입 수준에서 가능하다.

2. **그러나 렌더 경로는 GPU 작업과 AppKit 변형을 번갈아 수행한다.**
   - `surface_host.rs:456` — 렌더 직전 `host.update_viewport(...)` (플랫폼 뷰 변형)
   - 이어서 acquire / encode / present (GPU)
   - `surface_host.rs:2937` — present 직후 `self.target.reveal_after_present()` (AppKit 변형)

따라서 클로저 전체를 워커로 옮기면 **AppKit 변형이 메인 스레드 밖에서 실행된다.** macOS에서
이는 정의되지 않은 동작이며 크래시나 화면 깨짐으로 나타난다.

올바른 구조는 프레임당 두 번의 홉이다:

```
UiThread : update_viewport (지오메트리 확정)
   -> GpuThread : acquire -> encode -> present
   -> UiThread : reveal_after_present (첫 present 후 노출)
```

이것이 4.1절의 3분할 소유권이 옳다는 근거이자, 3단계가 단순 이동이 아니라 **프레임 상태
머신**을 필요로 하는 이유다. `UiThread` / `GpuThread` 토큰은 바로 이 두 홉의 경계를
타입으로 고정하기 위한 것이므로, 토큰 없이 이 이행을 시도하면 어느 구간이 어느 스레드에
속하는지 사람이 눈으로 추적해야 한다.

구현 순서 권고: 먼저 위 3단 구조를 스케줄링 수준에서 만들고(주입된 가짜 GPU로 홉이
실제로 다른 스레드에서 실행되는지 검증), 그 다음 실제 wgpu 호출을 GpuThread 구간으로
옮긴다. 현재 테스트 하네스는 `RenderDispatch`로 디스패치를 가로채므로 실제 wgpu/AppKit
경로는 타지 않는다는 점을 유념할 것. 실제 표면 검증은 변경분이 포함된 빌드를 실행해야만
가능하다.

### 12.6 "래치가 걸렸으니 이제 GPU 스레드로 옮겨도 된다"는 함정

12.5의 2홉 구조를 보고 다음과 같이 단축하고 싶어질 수 있다:

> 지오메트리 래치와 reveal 멱등성 가드를 넣었으니, 정상 상태(기하 불변 + 이미 노출됨)에서는
> 두 AppKit 호출이 모두 조기 반환한다. 그러면 정상 상태 프레임은 순수 GPU 작업이므로
> 통째로 GpuThread로 옮겨도 된다.

**틀렸다.** 코드를 확인하면 `macos.rs`의 `apply_viewport`는 래치를 확인하기 **전에** 이미
AppKit을 건드린다:

```
:240  let superview_bounds = superview.bounds();     // AppKit read
:241  let is_flipped       = superview.isFlipped();  // AppKit read
:242  let appkit_frame     = bounds.to_appkit_frame(superview_bounds.size.height, is_flipped);
:245  if !latch.needs_apply(AppKitFrameLatch::key(appkit_frame …)) { return; }   // 여기서 단락
```

이 순서는 실수가 아니라 필수다. 래치 키는 들어온 `LogicalBounds`가 아니라 **계산된 AppKit
프레임**이어야 하고(창 크기가 바뀌면 논리 좌표는 그대로여도 실제 프레임은 움직인다), 그
프레임을 계산하려면 superview의 높이와 flipped 여부를 먼저 읽어야 한다.

따라서 정상 상태 프레임도 여전히 **프레임당 2회의 AppKit 읽기**를 수행한다. NSView 지오메트리
읽기도 메인 스레드 전용이므로, 래치가 있다고 해서 이 구간을 워커로 옮길 수는 없다.

결론: 12.5의 2홉 구조는 최적화가 아니라 **정확성 요구사항**이다. 지오메트리 레그는 AppKit을
읽기 때문에 UiThread에 남아야 하고, GpuThread로 넘어가는 것은 acquire/encode/present 구간
뿐이다. 래치가 줄여준 것은 AppKit *쓰기*(setFrame, 레이어 재구성, setHidden)이지 *읽기*가 아니다.

### 12.7 3단계의 진짜 난점: `&mut host` 빌림과 영수증 되돌리기

12.5/12.6이 "어느 구간이 어느 스레드인가"를 정했다면, 실제 구현을 막는 것은 그 다음 두 가지다.
둘 다 현재 코드를 읽어야만 보이고, 옮기기 시작한 뒤에 발견하면 되돌리기 비싸다.

#### 난점 A — GPU 호출이 `&mut host` 빌림 한가운데에 있다

```
:449  if let Some(host) = host {            // hosts 맵에서 꺼낸 &mut 빌림
:453      host.layout = Some(layout);        // UI leg: 호스트 상태 변경
:454      host.logical_bounds = Some(effective_bounds);
:455      host.update_viewport(Some(effective_bounds));   // UI leg: AppKit
:456      match host.render_snapshot(…)                   // <-- GPU leg
```

`render_snapshot`은 `&mut host` 를 필요로 하고, 그 빌림은 UI leg의 상태 변경과 **연속**돼
있다. 따라서 "GPU 호출만 워커로 보낸다"는 표현은 빌림 검사기에서 성립하지 않는다. 선택지는
사실상 셋이다.

1. **호스트를 워커로 이동시켰다가 돌려받기** — 잡에 `host`를 `move`로 넘기고 완료 시 맵에
   되돌린다. 이동 중 그 세션 키는 비어 있으므로, 그 사이 도착한 요청을 어떻게 처리할지
   (합치기/버리기) 정해야 한다.
2. **hosts 맵 자체를 GpuThread 소유로 옮기기** — UI leg는 지오메트리 계산에 필요한 값만 미리
   뽑아 명령으로 보낸다. 2절 C4가 지적한 뮤텍스가 렌더 경로에서 자연히 사라진다는 점에서
   **12.1과 가장 잘 맞는 선택지이며 권장안**이다.
3. 호스트를 GPU 부분과 UI 부분으로 쪼개기 — 가장 깨끗하지만 가장 큰 변경이다.

#### 난점 B — 재예약 결정이 영수증에 동기적으로 의존한다

```
Ok(receipt) => {
    if !receipt.presented && !receipt.render_deferred && !receipt.render_suspended {
        coordinator.schedule_render();
    }
}
```

지금은 렌더가 끝난 **그 자리에서** 영수증을 보고 재예약 여부를 정한다. GPU leg가 워커로
가면 영수증은 **비동기로** 도착하므로, 이 분기는 워커 잡의 완료 경로로 옮겨야 한다. 같이
움직여야 하는 것들:

- `coordinator.finish_render()` — 프레임 종료 표시. 워커 완료 시점으로 가야 한다. 지금처럼
  디스패치 직후에 호출하면 **프레임이 끝나기 전에 끝났다고 표시**되어 유휴 0프레임 불변식이
  깨진다.
- `FrameClock::mark_frame_started` — 프레임 **시작** 기준이므로 UI leg에 남는다.
- `reveal_after_present` — present 성공 후의 AppKit 변형이므로 **UI 스레드로 되돌아와야**
  한다. 다만 노출 가드가 생긴 뒤로는 첫 프레임 이후 조기 반환하므로, 이 복귀 홉은 매 프레임이
  아니라 가시성 전이 때만 실제 일을 한다.

#### 검증 순서 권고

하네스는 `RenderDispatch`로 디스패치를 가로채므로 실제 wgpu/AppKit을 타지 않는다. 그래서
**먼저 스케줄링 수준에서** 위 구조를 만들고, 주입된 가짜 GPU로 "GPU leg가 디스패치 스레드와
다른 스레드에서 실행된다"를 스레드 ID로 검증한 뒤, 마지막에 실제 wgpu 호출을 옮긴다.
실제 표면 검증은 변경분이 포함된 빌드를 띄워야만 가능하다.

### 12.8 왜 "가끔" 멈추는가 — 블로킹 acquire × 패널 수

1.1절은 "메인 스레드가 GPU 프레임을 돌린다"까지 말한다. 실측을 다시 보면 한 단계 더
구체적인 설명이 나온다.

- `gpu_context.rs:134` — `present_mode: wgpu::PresentMode::AutoVsync`. 이 저장소에서
  present mode가 언급되는 유일한 지점이며, 설정 가능하지 않다.
- AutoVsync는 FIFO 계열로 내려가고, FIFO에서 `get_current_texture()`는 **드로어블이 생길
  때까지 블록**한다. Metal에서 그 대기가 바로 실측에 찍힌
  `-[CAMetalLayer nextDrawable]` → `semaphore_timedwait_trap` 이다.
- `renderer.rs:573` — `GpuContext::new()`가 렌더러마다 호출된다. 즉 **패널마다 별도의 wgpu
  디바이스와 스왑체인**이 있다(C5).

세 가지를 합치면 증상의 형태가 설명된다. 프레임마다 메인 스레드는 패널 하나당 최대 한
리프레시 간격(60Hz면 약 16.7ms)까지 블록될 수 있고, 드로어블 풀이 비면 더 길어진다. 패널이
N개면 그 대기가 **직렬로 N번** 쌓인다. 평소에는 눈에 안 띄다가, 여러 패널이 동시에 출력을
쏟아내는 순간 메인 스레드가 수십 ms 단위로 사라진다 — 이것이 사용자가 말한 "간헐적"
무응답의 형태다. 부하가 아니라 **동시에 그리는 패널 수**가 방아쇠다.

따라서:

1. 이 대기는 3단계(GpuThread 분리)로만 근본 제거된다. 대기 자체를 없애는 게 아니라
   **메인 스레드에서 치우는** 것이 핵심이다. vsync 대기는 GPU 스레드에서라면 정상이다.
2. `AutoNoVsync`로 바꾸는 것은 **완화책이지 해결책이 아니다.** 대기를 줄이지만 티어링을
   감수해야 하고, 메인 스레드가 여전히 GPU 작업을 한다는 구조는 그대로다. 다만 이제
   `FrameClock`이 페이싱을 담당하므로, vsync를 빼도 렌더가 자유 실행되지는 않는다 — 즉
   이 완화책은 1단계 이전보다 지금이 더 안전하다. 채택한다면 임시 조치임을 명시할 것.
3. C5(패널마다 디바이스)는 단순한 메모리 낭비가 아니라 **이 블로킹의 증폭 계수**다. 3단계
   이후 재평가할 것.

### 12.9 C1~C7 / 단계 현재 상태 대조표

2절과 7절은 착수 전 분석이다. 아래가 코드 실측 기준 현재 상태다.

| 원인 | 상태 | 근거 |
|---|---|---|
| C1 프레임 전체가 메인 스레드 클로저 | **미해결** | 3단계 전부. §12.5~12.7 |
| C2 present 실패 즉시 재시도 | **해결** | `FrameClock` 페이싱. b0d0c926, c07bba1e |
| C3 프레임마다 AppKit 레이아웃 | **해결(4개 플랫폼)** | 지오메트리 래치 dfb69bb7(Win), 8d10c576(X11), 2643e63b(macOS), 44c69e23(Wayland), b7425ada(reveal 멱등) |
| C4 전 세션 공용 뮤텍스 | **미해결** | 3단계에 흡수(§12.1). 단독 수행은 무의미 |
| C5 패널마다 wgpu 디바이스 | **미해결** | 낭비가 아니라 C1의 **증폭 계수**(§12.8) |
| C6 프레임마다 스냅샷 할당 | **미착수** | 손대지 않았다 |
| C7 왜 재발하는가 | **부분 해결** | `!Send` 토큰 0280264a + CI 게이트 87eabebd, c2c516d0, 518b219e |

| 단계 | 상태 |
|---|---|
| 1단계 출혈 정지 | 완료 — C2·C3 해소 |
| 2단계 락 제거 | 3단계로 흡수(§12.1) |
| 3단계 GpuThread 분리 | **미착수.** §12.5~12.8이 명세 |
| 4단계 게이트 | 완료. `ui-check`에 연결되고 게이트 자신의 테스트도 CI에서 돈다 |

즉 **C1이 남아 있는 한 사용자가 겪는 간헐적 무응답은 사라지지 않는다.** 1단계는 유휴 CPU와
스핀을 없앴을 뿐, 메인 스레드가 블로킹 acquire를 수행한다는 사실은 그대로다(§12.8).

### 12.10 막힘 A의 "쉬워 보이는" 출구는 입력을 삼킨다

12.7의 막힘 A(`&mut host` 빌림)를 보면 더 간단한 우회가 바로 떠오른다:

```rust
let host = hosts.remove(&id);              // 맵에서 빼서
worker.submit(move |gpu| {                 // 통째로 GPU 잡에 넘기고
    host.render_snapshot(gpu, ..);
    return_host(host);                     // 끝나면 돌려받아 재삽입
});
```

빌림 충돌이 아예 사라지고, 호스트가 `Send`라는 것도 12.5에서 확인됐으니 컴파일도 된다.
**쓰지 말 것.** 프레임이 도는 동안 호스트가 맵에 **없다**. 그 사이 들어오는 조회는 전부
빈손으로 돌아간다 — 입력, 리사이즈, 가시성 명령이 조용히 무시된다. 컴파일도 되고 테스트도
통과하는데 사용자는 **부하가 걸릴 때만 가끔 키 입력이 씹힌다.** 지금 고치려는 증상과 구별이
안 되는 새 버그를, 그것도 더 진단하기 어려운 형태로 만드는 셈이다.

빠진 동안의 조회를 "나중에 재생"하는 식으로 덧대는 것도 답이 아니다. 입력 순서 보장이
깨지고, 결국 12.7이 권하는 것(호스트 맵 자체를 `GpuThread` 소유로 옮겨 조회도 그쪽에서
처리)을 어설프게 재구현하게 된다.

교훈: 이 리팩터에서 **컴파일이 통과한다는 것은 거의 아무것도 보장하지 않는다.** 빌림 검사기는
소유권 충돌은 잡아주지만 "프레임 동안 이 자료구조가 조회 가능해야 한다"는 요구는 타입에
적혀 있지 않다.

### 12.11 막힘 C — `render_snapshot`이 `tauri::Window`를 받는다 (12.7 권고 수정)

12.7은 막힘 A의 출구로 "호스트 맵을 `GpuThread` 소유로 옮기라"고 권했다. **그대로는 안 된다.**

`dispatch_scheduled_render`(:395)는 `window: Window<R>`를 **값으로** 받아 두 번 복제하고
(:402, :403), 그 복제본이 렌더 클로저에 잡힌다. 쓰이는 곳이 두 군데인데 둘 다 문제다.

- **:436 `NativeTerminalSurfaceHost::new(&surface_window, scale)`** — 호스트 생성이 Tauri
  윈도우를 요구한다. 네이티브 자식 뷰를 얻어야 하므로 **정의상 메인 스레드 작업**이다.
  ⇒ 호스트 맵을 통째로 GPU 스레드로 옮길 수 없다. **생성은 UI 레그에 남아야 한다.**
- **:458 `render_snapshot(&surface_window, layout, &snapshot, ..)`** — 3단계가 옮기려는 바로
  그 호출이 `&Window<R>`를 인자로 받는다.

따라서 3단계는 호출 지점을 옮기는 작업이 아니라 **`render_snapshot`의 시그니처를 바꾸는**
작업이다. 윈도우에서 파생되는 값(서피스 핸들 등)은 UI 레그에서 미리 뽑아 **평범한 데이터로**
넘겨야 하고, GPU 레그는 Tauri 타입을 아예 보지 않아야 한다.

`Window<R>`이 `Send`라서 컴파일이 통과할 수도 있다는 점이 특히 위험하다. 컴파일된다는 것은
**그 윈도우 메서드를 워커 스레드에서 불러도 된다는 뜻이 아니다.** 12.10과 같은 함정이며,
이번에는 타입 검사가 잡아주지 못한다.

착수 순서 수정: 시그니처에서 Tauri 의존을 걷어내는 것이 **가장 먼저**다. 그 전에는 2회 홉
구조를 짜봐야 컴파일 단계에서 막힌다.
