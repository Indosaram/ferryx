# Orca Native Terminal Migration 구현 계획서

## 0. 문서 목적

이 문서는 Orca desktop terminal을 현재의 `legacy DOM + WebGL + WebView` 구조에서 **`libghostty-vt + wgpu` 기반 native terminal renderer**로 단계적으로 전환하기 위한 구현 계획이다.

핵심 결정은 다음과 같다.

```text
Desktop Orca (macOS / Windows / Linux)
  -> libghostty-vt + Orca wgpu renderer

Web / Mobile Browser Remote Terminal
  -> web remote renderer 유지

PTY / daemon / session / remote protocol
  -> 공통 backend 유지
```

목표는 단순히 렌더러를 바꾸는 것이 아니다. Desktop terminal hot path에서 WebView/JavaScript/DOM renderer를 제거하고, terminal state와 rendering lifecycle을 Rust/native 쪽에서 직접 소유하도록 구조를 바꾼다.

동시에 웹/모바일 브라우저 원격 접속은 web remote renderer를 사용하므로 기존 remote terminal 기능을 포기하지 않는다.

이 문서는 기존 분석 문서 `docs/TERMINAL_NATIVE_WGPU_RENDERER_ASSESSMENT.md`의 후속 구현 계획이다.

---

## 1. 최종 목표

### 1.1 Desktop hot path

현재:

```text
PTY
  -> Orca daemon
  -> daemon client
  -> Rust/Tauri IPC
  -> binary Channel
  -> WebView
  -> JavaScript scheduler
  -> web terminal parser/state
  -> WebGL renderer
  -> screen
```

목표:

```text
PTY
  -> Orca daemon
  -> daemon client
  -> NativeTerminalHost (Rust)
  -> libghostty-vt
  -> RenderModel / Damage
  -> Orca wgpu renderer
  -> GPU
  -> screen
```

Desktop native 모드에서는 정상 stdout hot path가 JavaScript/WebView를 거치지 않아야 한다.

### 1.2 Web / Mobile Browser path

웹은 현재 구조를 유지한다.

```text
Orca daemon / remote gateway
  -> authenticated WebSocket
  -> binary terminal output
  -> browser
  -> web terminal
  -> WebGL / browser renderer
```

즉 Desktop에서 legacy DOM renderer를 제거해도 모바일 Safari/Chrome 등에서 원격 terminal을 보는 기능에는 영향이 없다.

### 1.3 Product-level architecture

```text
                         Orca PTY/session backend
                                  |
                    +-------------+-------------+
                    |                           |
                    v                           v
            Desktop Native                Web / Mobile Web
                    |                           |
             libghostty-vt                    web terminal
                    |                           |
              Orca wgpu                       WebGL
                    |                           |
      Metal / D3D12 / Vulkan                 Browser
```

---

## 2. 현재 코드 기준선

현재 repository에서 terminal 관련 책임은 다음 경계에 집중되어 있다.

### Frontend

- `ui/src/components/TerminalPane.tsx`
  - React terminal mount surface
  - active/focus/refit lifecycle
  - terminal search overlay 연결
- `ui/src/lib/terminalHostManager.ts`
  - legacy terminal instance lifecycle
  - visible/inactive 상태
  - warm cache/LRU
  - output subscription attach/detach
  - settings/refit
- `ui/src/lib/terminalInstanceFactory.ts`
  - legacy terminal 생성
  - FitAddon/SearchAddon/WebGL addon
  - ResizeObserver
  - keyboard/data forwarding
  - title/bell
- `ui/src/lib/terminalOutputScheduler.ts`
  - raw output batching
  - attach/replay sequence 처리
- `ui/src/lib/terminalEvents.ts`
  - Tauri Channel output event bus

### Rust/Tauri

- `src-tauri/src/ipc/terminal.rs`
  - spawn/attach/write/resize/close
  - daemon attachment pump
  - binary Tauri Channel stdout
  - replay-gap/lifecycle event
- `src-tauri/src/terminal/output_hub.rs`
  - sequence 기반 output buffering
  - default replay buffer: 512 KiB
  - broadcast subscriber
- `src-tauri/src/remote/server.rs`
  - authenticated remote terminal WebSocket
  - binary output frame
  - text control frame
  - active-session authorization

따라서 migration의 핵심은 **PTY/backend를 교체하는 것이 아니라 desktop renderer consumer를 하나 추가하고 이후 legacy consumer를 desktop에서 제거하는 것**이다.

---

## 3. 설계 원칙

### 3.1 PTY/session backend는 renderer와 독립적이어야 한다

Renderer가 바뀌어도 다음 contract는 유지한다.

- `backendSessionId`
- daemon epoch
- output sequence
- attach/replay-gap
- resize
- write/input
- signal
- close
- session restore metadata

Desktop native, desktop fallback, browser remote 모두 동일 backend session에 붙을 수 있어야 한다.

### 3.2 libghostty-vt API를 Orca 코드 전체에 노출하지 않는다

현재 `libghostty-vt` C API는 공식적으로 work-in-progress이고 breaking change 가능성이 있다.

따라서 다음 구조를 강제한다.

```text
Ghostty C ABI
    |
    v
orca_ghostty_vt adapter
    |
    v
Orca-owned TerminalEngine trait
    |
    +-> NativeTerminalHost
    +-> Renderer
    +-> Tests
```

Ghostty API 변경 시 adapter 한 곳만 수정한다.

### 3.3 renderer는 terminal engine과 분리한다

```rust
trait TerminalEngine {
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
    fn render_update(&mut self) -> RenderUpdate;
    fn encode_key(&mut self, event: KeyEvent) -> Option<Vec<u8>>;
    fn encode_mouse(&mut self, event: MouseEvent) -> Option<Vec<u8>>;
}

trait TerminalRenderer {
    fn resize_surface(&mut self, size: PhysicalSize);
    fn render(&mut self, update: &RenderUpdate, target: &wgpu::TextureView);
}
```

이 separation은 다음을 가능하게 한다.

- Ghostty 교체/업데이트
- renderer benchmark 독립 실행
- headless tests
- 향후 WebGPU/WASM backend
- OSS extraction

### 3.4 처음부터 별도 OSS crate 구조로 과도하게 분리하지 않는다

현재 `src-tauri`는 Cargo workspace가 아닌 단일 package다.

초기 구현은 repository 내부 module로 시작한다.

```text
src-tauri/src/native_terminal/
  mod.rs
  engine.rs
  ghostty_vt.rs
  render_model.rs
  renderer.rs
  glyph_cache.rs
  compositor.rs
  host.rs
  input.rs
  registry.rs
  metrics.rs
  platform/
```

API가 안정되고 native terminal이 production default가 된 후에만 다음과 같이 crate extraction을 고려한다.

```text
crates/
  orca-terminal-core
  orca-terminal-wgpu
  orca-terminal-host
```

---

## 4. Ghostty dependency 전략

### 4.1 POC

초기 POC에서는 Ghostty source를 **정확한 commit SHA로 pin**한다.

`main` branch를 직접 따라가지 않는다.

필요한 build output:

```text
libghostty-vt static library
+ public C headers
```

Ghostty upstream은 `zig build -Demit-lib-vt`를 제공하고 있고 C API에 terminal/render-state API가 존재한다.

### 4.2 Rust FFI layer

Rust에서 직접 광범위한 bindgen 결과를 사용하지 않는다.

권장 구조:

```text
native_terminal/ghostty_vt.rs
  - raw FFI declarations or generated bindings
  - ownership wrapper
  - error mapping
  - panic-safe Drop
  - UTF-8/grapheme access wrapper
  - render-state traversal wrapper
```

외부 module은 다음 정도의 Orca-owned API만 본다.

```rust
pub struct GhosttyTerminal { ... }

impl GhosttyTerminal {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Result<Self>;
    pub fn feed(&mut self, bytes: &[u8]);
    pub fn resize(&mut self, cols: u16, rows: u16);
    pub fn take_render_update(&mut self) -> RenderUpdate;
    pub fn cursor(&self) -> CursorState;
    pub fn encode_key(&mut self, key: KeyEvent) -> Vec<u8>;
}
```

### 4.3 Production dependency gate

Native renderer를 default로 만들기 전에 다음을 결정한다.

- Ghostty source pin 방식
- exact Zig version
- static vs dynamic linking
- CI cache
- release artifact packaging
- macOS universal build
- Windows MSVC artifact
- Linux glibc baseline
- license notice packaging

Network access가 필요한 build는 production release pipeline에 허용하지 않는다. Release build는 reproducible해야 한다.

---

## 5. Native terminal engine layer

### 5.1 Responsibilities

`NativeTerminalEngine`은 다음만 담당한다.

- raw PTY bytes ingest
- terminal VT state mutation
- resize/reflow
- render update 생성
- cursor state
- title/bell/OSC event
- key/mouse encoding
- selection-compatible grid query
- scrollback query

다음은 담당하지 않는다.

- PTY spawn
- daemon lifecycle
- tabs
- split layout
- workspace restore policy
- React state
- remote authentication

### 5.2 Render model

Renderer가 Ghostty C API에 직접 의존하지 않도록 Orca-owned render model을 둔다.

예:

```rust
pub struct RenderUpdate {
    pub full_redraw: bool,
    pub dirty_rows: Vec<DirtyRow>,
    pub cursor: CursorState,
    pub scroll_offset: i64,
    pub title_change: Option<String>,
}

pub struct RenderCell {
    pub grapheme: SmallString,
    pub width: u8,
    pub fg: TerminalColor,
    pub bg: TerminalColor,
    pub attrs: CellAttributes,
}
```

실제 struct shape는 libghostty-vt render-state API를 확인한 뒤 최소 copy 구조로 맞춘다.

가능하면 full grid copy 대신 incremental dirty state를 사용한다.

---

## 6. wgpu renderer 설계

### 6.1 목표

- macOS: Metal backend
- Windows: D3D12 backend
- Linux: Vulkan 우선, 필요 시 GLES fallback 검토
- renderer source는 공통
- terminal별 별도 GPU device를 만들지 않음

### 6.2 Window당 하나의 compositor

최종 목표는 **pane마다 wgpu device/surface를 만드는 것이 아니라 app window당 하나의 terminal compositor**를 두는 것이다.

```text
NativeTerminalCompositor
  |- shared wgpu Device / Queue
  |- shared glyph atlas
  |- pane A render region
  |- pane B render region
  |- pane C render region
  `- pane D render region
```

장점:

- glyph atlas 공유
- GPU resource 공유
- split pane 증가 시 surface explosion 방지
- 한 frame에서 모든 visible terminal batch 가능
- background pane throttling 제어 용이

### 6.3 Rendering passes

초기 pipeline:

1. cell background pass
2. glyph pass
3. decorations pass
   - underline
   - strikethrough
4. selection pass
5. cursor pass

최적화:

- dirty row only rebuild
- instance buffer reuse
- glyph atlas LRU
- background pane frame skip
- unchanged pane draw skip
- one Queue submit per compositor frame 가능 여부 측정

### 6.4 Text shaping / glyph rasterization

Ghostty VT core는 terminal semantics를 담당하지만 Orca renderer는 font pipeline을 가져야 한다.

권장 방향:

- `cosmic-text` 기반 shaping/fallback 검토
- `swash`/compatible rasterization
- custom wgpu glyph atlas

POC에서는 ASCII/monospace만으로 surface integration을 먼저 검증하되, production gate는 반드시 다음을 포함한다.

- Korean Hangul
- Japanese
- CJK fallback
- emoji
- combining marks
- double-width cells
- grapheme clusters
- Nerd Font/private-use glyphs

---

## 7. Tauri native surface integration

이 항목이 전체 migration의 **가장 큰 기술 리스크**다.

### 7.1 문제

현재 layout:

```text
Tauri Window
  `- WebView
      `- React
          `- TerminalPane DOM
              `- legacy DOM/WebGL
```

목표:

```text
Tauri native window
  |- React/WebView chrome
  |   |- tabs
  |   |- sidebar
  |   |- dialogs
  |   `- layout geometry source
  |
  `- Native terminal compositor
      |- pane A
      |- pane B
      `- pane C
```

React DOM element 안에 wgpu native surface를 직접 넣을 수는 없으므로 native view/window composition이 필요하다.

### 7.2 POC에서 반드시 검증할 것

macOS:

- WKWebView parent hierarchy 안에 native `NSView` sibling/child 배치 가능 여부
- surface clipping
- Retina scale
- focus/IME

Windows:

- child `HWND` 또는 equivalent host view
- D3D12 surface
- DPI scaling
- z-order/focus

Linux:

- GTK/WebKitGTK parent hierarchy
- X11/Wayland 각각의 handle
- native child drawing widget/surface 가능 여부

### 7.3 Surface composition 후보

POC에서 다음 순서로 검토한다.

#### Option A — one native child compositor view per app window

우선순위 1.

```text
main native window
  |- WebView
  `- terminal compositor child view
```

하나의 compositor가 모든 terminal pane을 렌더링하고 React에서 받은 rectangle을 scissor region으로 사용한다.

#### Option B — pane별 child native view

Option A가 clipping/z-order 문제로 어려울 때만 검토한다.

단점:

- surface 수 증가
- resize/focus lifecycle 복잡
- split 변화 시 view churn

#### Option C — separate borderless child/owned native window

마지막 fallback.

구현은 쉬울 수 있지만 다음 문제를 가진다.

- z-order
- modal/dialog
- focus
- window move synchronization
- multi-monitor DPI

### 7.4 WebView overlay contract

Native compositor가 WebView 위에 올라가는 구조라면 React overlay가 가려질 수 있다.

따라서 POC에서 반드시 결정한다.

- terminal compositor를 WebView 아래에 놓고 WebView terminal 영역을 transparent하게 만들 수 있는가?
- 불가능하면 native compositor가 위에 있을 때 modal/search overlay를 어떻게 처리할 것인가?
- native mode에서 terminal search/selection UI 일부를 native로 옮길 것인가?

이 문제를 해결하지 못하면 full migration에 들어가지 않는다.

---

## 8. Native terminal lifecycle

### 8.1 New Rust registry

```text
NativeTerminalRegistry
  Map<frontendSessionId, NativeTerminalHost>
```

`NativeTerminalHost` 예상 state:

```rust
struct NativeTerminalHost {
    backend_session_id: String,
    daemon_epoch: Option<u64>,
    last_output_sequence: Option<u64>,
    engine: NativeTerminalEngine,
    pane_id: PaneId,
    geometry: PaneGeometry,
    visible: bool,
    focused: bool,
    output_task: JoinHandle<()>,
}
```

### 8.2 Lifecycle commands

초기 Tauri IPC는 control-plane 전용으로 사용한다.

예상 command:

```text
cmd_native_terminal_create
cmd_native_terminal_attach
cmd_native_terminal_set_geometry
cmd_native_terminal_set_visible
cmd_native_terminal_focus
cmd_native_terminal_set_settings
cmd_native_terminal_destroy
```

stdout payload는 이 command path로 보내지 않는다.

### 8.3 Output handling

Native mode:

```text
daemon attachment receiver
  -> Rust task
  -> engine.feed(bytes)
  -> mark dirty
  -> compositor wake/request frame
```

따라서 기존 Tauri binary Channel과 `terminalOutputScheduler`는 desktop native stdout path에서 사용하지 않는다.

기존 Channel은 다음 용도로 유지한다.

- desktop fallback
- migration A/B
- test/preview compatibility

---

## 9. React integration

### 9.1 TerminalPane를 renderer selector로 변경

최종 형태 예:

```tsx
function TerminalPane(props) {
  const renderer = useTerminalRendererMode();

  if (isTauriRuntime() && renderer === "native") {
    return <NativeTerminalPane {...props} />;
  }

  return <LegacyTerminalPane {...props} />;
}
```

초기에는 기존 `TerminalPane` 구현을 `LegacyTerminalPane`로 옮기고 기능 변경 없이 유지한다.

### 9.2 NativeTerminalPane의 책임

React native pane은 terminal text를 렌더하지 않는다.

다음만 한다.

- pane geometry 측정
- viewport/window coordinate 변환
- visible/hidden 전달
- active/focus 전달
- renderer error/fallback UI
- split layout lifecycle 연결

즉:

```text
React = where/when
Native = what/how to render
```

### 9.3 Geometry update

`ResizeObserver` + window layout 변화에서:

```text
DOMRect
  -> physical pixel rect
  -> cmd_native_terminal_set_geometry
  -> NativeTerminalCompositor pane rect
```

매 frame command를 보내지 않는다.

geometry가 실제 변경될 때만 update한다.

### 9.4 Focus

가능하면 keyboard/IME event는 native view가 직접 수신한다.

WebView에서 모든 key event를 Tauri command로 forwarding하는 구조는 최종 목표가 아니다.

React는 pane activation/focus intent만 전달한다.

---

## 10. Input / IME / clipboard

### 10.1 Keyboard

Host native event:

```text
OS keyboard event
  -> platform adapter
  -> Orca KeyEvent
  -> libghostty-vt key encoder
  -> daemon write_terminal
```

반드시 검증:

- Ctrl
- Alt/Option
- Meta/Cmd
- function keys
- arrows
- application cursor mode
- Kitty keyboard protocol
- tmux
- vim/neovim

### 10.2 IME

IME는 production 전환의 hard gate다.

검증:

- 한글 조합 중 composition 표시
- 조합 확정
- backspace/edit
- 일본어 IME
- CJK candidate window 위치
- pane 이동 후 IME anchor 위치
- Retina/DPI 환경에서 candidate window 좌표

IME가 macOS/Windows/Linux 중 하나에서 제대로 되지 않으면 해당 OS에서는 native를 default로 활성화하지 않는다.

### 10.3 Clipboard

- Cmd/Ctrl+C selection copy
- terminal SIGINT와 copy conflict
- paste
- bracketed paste
- unsafe paste warning policy
- OSC 52 policy

---

## 11. Selection / search / hyperlinks

### 11.1 Selection

Native engine grid coordinates를 기준으로 selection model을 별도 구현한다.

```text
pointer -> pixel -> cell -> selection range
```

Renderer는 selection range를 별도 overlay pass에서 그린다.

### 11.2 Search

초기 migration 동안 search UX는 두 단계로 가져간다.

Phase A:

- native renderer에서 search 기능을 disabled/experimental로 두고 legacy fallback 제공 가능

Phase B:

- engine scrollback traversal 기반 search index
- next/previous match
- viewport reveal
- highlight ranges

Desktop native default 전에 현재 `TerminalSearchOverlay`와 동등한 핵심 기능을 확보한다.

### 11.3 Hyperlinks

- OSC 8 hyperlink
- URL detection
- modifier-click policy
- security routing

현재 Orca link routing behavior와 동등해야 한다.

---

## 12. Session restore 설계

이 부분은 migration의 두 번째 큰 리스크다.

### 12.1 현재 제약

`TerminalOutputHub`의 default replay buffer는 512 KiB로 bounded다.

즉 renderer process가 오래 분리되어 있으면 full terminal history를 무조건 재생할 수 있는 것은 아니다.

현재 sequence/replay-gap semantics는 유지해야 하지만 native engine의 deterministic reconstruction에는 추가 전략이 필요할 수 있다.

### 12.2 HMR/WebView reload

Native terminal state를 Rust/Tauri process가 소유하면 WebView HMR/reload는 훨씬 간단해진다.

```text
WebView reload
  -> NativeTerminalRegistry survives
  -> React remount
  -> same native terminal host rebind
```

이 경우 terminal parser/grid/scrollback은 다시 만들 필요가 없다.

### 12.3 Full desktop app restart

Full process restart는 native terminal engine state도 사라진다.

Native default 전 다음 중 하나를 선택해야 한다.

#### Strategy A — current bounded replay parity

현재 legacy behavior와 같은 수준으로 512 KiB replay를 적용한다.

장점:

- 구현 단순

단점:

- 오래된 state/scrollback 완전 복원 불가

#### Strategy B — persistent terminal checkpoint

주기적으로 terminal render/state checkpoint + output sequence를 저장한다.

재시작:

```text
load checkpoint at sequence N
  -> apply output N+1..
  -> live attach
```

Ghostty C API로 lossless state serialization이 가능한지 먼저 검증해야 한다.

#### Strategy C — daemon owns canonical terminal engine

Daemon process가 `libghostty-vt` state까지 소유한다.

Desktop renderer는 daemon의 render snapshot/damage를 소비한다.

장점:

- UI/app restart와 terminal state 완전 분리

단점:

- render-state binary protocol 필요
- raw PTY bytes보다 payload가 커질 수 있음
- daemon/renderer coupling 증가

### 12.4 권장 순서

1. Native POC에서는 Strategy A로 기존 behavior와 parity 확보
2. Ghostty state serialization capability 조사
3. Strategy B가 가능하면 B 우선
4. B가 불가능하거나 session restore requirement가 더 강하면 C benchmark

**Full app restart 후 현재보다 session restore가 퇴행하면 native renderer를 production default로 만들지 않는다.**

---

## 13. Web / Mobile Web 유지 계획

### 13.1 Remote path는 migration 대상이 아님

현재 remote server는 terminal WebSocket에서 이미 binary output을 보내고 control message를 별도로 처리한다.

따라서 desktop native migration에서 다음 코드를 불필요하게 변경하지 않는다.

- remote auth
- remote pairing/device permission
- active session enforcement
- terminal WebSocket binary protocol
- browser remote rendering

### 13.2 Browser remote terminal dependency 유지

최종 dependency policy:

```text
Desktop Tauri native runtime
  -> fallback code lazy-loaded only for fallback if enabled

Web/browser runtime
  -> browser remote mandatory
```

가능하면 bundling을 분리해 desktop normal startup에서 legacy WebGL addon을 로드하지 않는다.

### 13.3 향후 optional target

Native renderer가 안정된 뒤에만 다음을 별도 프로젝트로 검토한다.

```text
shared TerminalEngine abstraction
  + wgpu WebGPU/WASM renderer
  -> browser remote replacement
```

이 작업은 현재 migration scope에 포함하지 않는다.

---

## 14. Feature flag / rollback

### 14.1 Runtime selection

초기부터 renderer selector를 둔다.

예:

```text
ORCA_TERMINAL_RENDERER=auto
ORCA_TERMINAL_RENDERER=native
ORCA_TERMINAL_RENDERER=legacy
```

또는 hidden developer setting:

```text
terminal.renderer = auto | native | legacy
```

### 14.2 Rollout

1. development only native
2. internal dogfood
3. opt-in experimental
4. OS별 native default
5. legacy fallback 유지
6. 충분한 안정화 후 desktop fallback 제거 여부 결정

### 14.3 OS별 rollout 가능

세 OS를 동시에 default로 만들 필요가 없다.

예:

```text
macOS   native default
Windows native experimental
Linux   legacy fallback
```

처럼 surface/IME 안정도에 따라 독립적으로 rollout한다.

---

## 15. 구현 Phase

## Phase 0 — Baseline / freeze

목표: migration 기준선을 고정한다.

작업:

- 현재 terminal 기능 목록 작성
- 현재 rendering bug 목록 고정
- terminal settings contract 기록
- existing benchmark capture
- current terminal screenshot/video fixtures 저장
- legacy renderer 구조적 refactor는 critical bug 외 중단

Benchmark baseline:

- single terminal idle CPU
- 10/50/100 MB output
- ANSI-heavy output
- four-pane concurrent output
- input latency
- tab switch
- split resize
- app/HMR restore

Exit gate:

- native와 비교 가능한 baseline 결과 확보

---

## Phase 1 — libghostty-vt integration spike

목표: GUI 없이 terminal core를 Rust에서 안정적으로 구동한다.

작업:

- pinned Ghostty revision
- Zig build integration
- static library link
- safe Rust wrapper
- terminal create/feed/resize/free
- render-state/grid traversal
- cursor/title/bell event
- key/mouse encode

Test fixtures:

- ASCII
- colors/SGR
- cursor movement
- alternate screen
- scrollback
- resize/reflow
- wide characters
- combining characters
- OSC title

Exit gate:

- `cargo test`에서 headless terminal state tests 통과
- no memory leak/use-after-free under repeated create/destroy
- Ghostty API가 필요한 terminal features를 충분히 expose함을 확인

Stop condition:

- required grid/render state를 C API로 얻을 수 없고 upstream extension 없이는 진행 불가능한 경우

---

## Phase 2 — Standalone wgpu renderer

목표: Tauri와 분리된 native window에서 renderer 성능/정확성을 검증한다.

작업:

- wgpu device/queue
- test native window
- terminal grid -> instance data
- background/glyph/cursor
- glyph atlas
- font loading
- basic selection
- dirty update

POC workload:

```bash
yes "012345678901234567890123456789" | head -n 1000000
```

추가:

- ANSI color flood
- Unicode flood
- long lines

Exit gate:

- stable rendering
- no GPU validation errors
- no unbounded atlas growth
- legacy baseline 대비 throughput/frame behavior가 충분히 유망

---

## Phase 3 — Tauri native surface composition spike

목표: 실제 Orca window 안에서 native wgpu surface가 React layout과 공존할 수 있는지 검증한다.

이 Phase가 architecture go/no-go gate다.

작업:

- 한 개 terminal rectangle
- React DOM geometry -> native rect
- window move/resize
- clipping
- focus
- keyboard
- IME prototype
- modal overlay behavior
- DPI scaling

플랫폼:

1. macOS
2. Windows
3. Linux X11/Wayland

Exit gate:

- terminal surface가 React pane geometry와 pixel-accurate sync
- split resize 중 visual tearing 없음
- focus loss/steal 없음
- modal/dialog strategy 확정
- IME composition 가능

Stop condition:

- Tauri/WebView/native view layering이 OS 하나 이상에서 안정적으로 해결 불가능

이 경우 fallback architecture로 WebGPU/WASM 또는 fallback 유지 재검토.

---

## Phase 4 — Single terminal end-to-end

목표: 실제 Orca daemon session 하나를 native terminal에 연결한다.

작업:

- NativeTerminalRegistry
- daemon attach
- raw bytes -> Ghostty
- native output loop
- resize -> daemon
- native keyboard -> daemon input
- title/bell -> UI event
- native renderer mode selector

중요:

Desktop native path에서는:

```text
PTY output -> JavaScript = 0
```

이어야 한다.

Exit gate:

- shell 사용 가능
- resize 안정
- vim/neovim 정상
- tmux 정상
- Ctrl+C/keyboard 정상
- title/bell 정상

---

## Phase 5 — Terminal UX parity

목표: legacy desktop과 실사용 feature parity 확보.

작업:

- selection
- clipboard
- search
- hyperlinks
- cursor styles
- theme/settings
- font size/family
- scrollback
- mouse reporting
- bracketed paste
- Unicode/CJK
- Korean/Japanese IME
- accessibility 최소 contract

Exit gate:

- day-to-day development workflow에서 legacy fallback 필요 없음

---

## Phase 6 — Multi-pane compositor / lifecycle

목표: Orca의 실제 강점인 split/tab/session lifecycle을 native renderer에서 안정화한다.

작업:

- 2/4 pane
- per-pane clipping/scissor
- shared glyph atlas
- hidden pane throttling
- tab switch
- warm native cache
- pane close/reopen
- workspace switch
- settings live update
- monitor/DPI 이동

React의 기존 `TerminalHostManager` 역할을 나눈다.

```text
LegacyHostManager
  -> browser/fallback only

NativeTerminalRegistry
  -> desktop native lifecycle
```

Exit gate:

- 반복 tab switch/resize에서 blank terminal 없음
- stale framebuffer 없음
- hidden pane CPU/GPU 사용 억제
- terminal session state가 React remount와 독립

---

## Phase 7 — Restore / crash / HMR hardening

목표: 현재 Orca session restore보다 퇴행하지 않도록 한다.

작업:

- WebView HMR/reload native state persistence
- frontend crash/remount
- Tauri app restart
- daemon restart/epoch mismatch
- replay gap
- checkpoint strategy 결정
- renderer device loss recovery

Exit gate:

- HMR에서 terminal content/state 유지
- app restart에서 current product behavior 이상
- replay-gap UI/state corruption 없음
- daemon epoch mismatch 안전 처리

---

## Phase 8 — Performance A/B

목표: 구조 전환이 실제 성능 이점을 만드는지 숫자로 확인한다.

동일 workload를 legacy/native에서 반복한다.

측정:

- bytes/sec
- output completion time
- CPU
- GPU
- memory
- p50/p95/p99 frame time
- stalls > 50 ms / 100 ms
- input latency under output flood
- 1/2/4/8 pane scaling

Native-specific:

- feed time
- render update generation
- glyph cache hit ratio
- atlas uploads
- dirty cells/frame
- draw calls/frame
- queue submissions/frame

Exit gate 권장:

- desktop stdout 정상 path에서 WebView/JS terminal processing 제거 확인
- native가 legacy baseline보다 sustained output throughput에서 명확한 우위
- 4-pane workload에서 visible frame stall 현저히 감소
- idle resource usage 합리적
- memory leak 없음

정확한 숫자 threshold는 Phase 0 baseline을 보고 확정한다.

---

## Phase 9 — Production rollout

목표: native renderer를 OS별 default로 활성화한다.

작업:

- telemetry/diagnostic counter
- startup failure fallback
- crash-loop fallback
- settings migration
- release CI
- updater artifact verification
- release notes

Fallback rule 예:

```text
native init failure
  -> same session remains alive
  -> switch renderer to fallback
  -> report diagnostic
```

Backend session을 renderer failure와 함께 종료하면 안 된다.

---

## Phase 10 — Desktop legacy terminal cleanup

Native가 충분히 안정된 후에만 실행한다.

삭제/축소 대상:

- desktop legacy terminal instance lifecycle
- desktop WebGL renderer initialization
- desktop terminal output scheduler
- desktop terminal binary Channel normal path

유지:

- browser remote terminal
- mobile web remote terminal
- optional emergency desktop fallback 여부는 product decision

Bundle이 가능하면 runtime별 code split한다.

---

## Phase 11 — OSS extraction

Migration 완료 후 별도 오픈소스화를 검토한다.

제품 positioning:

> Cross-platform native embeddable terminal component for Rust/wgpu applications, backed by a proven VT engine.

예상 public architecture:

```text
terminal-core API
  + ghostty-vt adapter
  + wgpu renderer
  + platform host
```

Potential consumers:

- Tauri apps
- winit apps
- custom Rust desktop apps
- future GPUI integration

OSS extraction은 Orca-specific daemon/session code를 포함하지 않는다.

Library boundary:

```text
bytes in
pixels out

input events in
PTY bytes out
```

---

## 16. Test matrix

### Core terminal tests

- SGR
- cursor
- scroll
- alt screen
- clear operations
- resize/reflow
- modes
- mouse
- keyboard
- OSC
- Unicode

### Rendering tests

Deterministic test font를 사용한다.

- cell backgrounds
- cursor shape
- selection
- bold/italic
- underline
- wide chars
- combining chars
- glyph clipping
- high DPI

가능하면 headless texture render + pixel/hash golden을 사용한다.

System font screenshot만으로 CI 판정을 하지 않는다.

### Integration tests

- shell prompt
- `vim`
- `nvim`
- `tmux`
- `htop`/TUI equivalent
- large `cargo build` output
- ANSI progress bars
- terminal resize during active output

### Lifecycle tests

- tab A -> B -> A
- repeated split create/delete
- hide/show
- HMR
- app restart
- daemon reconnect
- output replay gap
- native renderer failure -> legacy fallback

### Cross-platform CI

Minimum:

```text
macOS arm64
Windows x86_64
Linux x86_64
```

Release 전 추가 검토:

- macOS x86_64/universal
- Windows arm64
- Linux Wayland
- Linux X11

---

## 17. Performance benchmark suite

Repository 안에 repeatable benchmark command를 만든다.

예상 위치:

```text
bench/terminal/
  generate_ascii.py
  generate_ansi.py
  generate_unicode.py
  run_burst.sh
  README.md
```

Workload:

### B1 — ASCII throughput

```bash
yes "012345678901234567890123456789" | head -n 1000000
```

### B2 — ANSI-heavy

- foreground/background color changes
- cursor movement
- line clears

### B3 — Unicode

- Korean
- Japanese
- emoji
- combining marks

### B4 — long line

- 10K+ columns logical output

### B5 — parallel panes

- 2 panes
- 4 panes
- 8 panes

### B6 — interactive under flood

Background pane에서 continuous output 중 foreground pane typing latency 측정.

---

## 18. Diagnostics / instrumentation

Native renderer에서 최소한 다음 counters를 제공한다.

```text
terminal.feed.bytes
terminal.feed.duration
terminal.render.frames
terminal.render.dirty_cells
terminal.render.full_redraws
terminal.render.frame_time
terminal.render.glyph_cache_hits
terminal.render.glyph_cache_misses
terminal.render.atlas_upload_bytes
terminal.render.draw_calls
terminal.render.queue_submits
terminal.surface.recreates
terminal.device.losses
```

Dev overlay 또는 structured log로 볼 수 있어야 한다.

Production telemetry 사용 여부는 별도 privacy/product policy에 따른다.

---

## 19. 주요 리스크와 대응

| Risk | Severity | 대응 |
| --- | --- | --- |
| Tauri/WebView/native surface layering | Critical | Phase 3에서 조기 POC, 실패 시 full migration 중단 |
| libghostty-vt API instability | High | pinned SHA + isolated FFI adapter |
| IME/CJK | High | production hard gate, 초기부터 real-language test |
| full app restart restore | High | checkpoint/daemon-state 전략 별도 gate |
| Linux Wayland/X11 차이 | High | platform adapter 격리, OS별 rollout |
| font fallback/emoji | Medium-High | shaping/rasterization layer 독립 |
| accessibility | Medium-High | parity checklist에 포함 |
| GPU device loss | Medium | compositor recreate + session 유지 |
| legacy/native behavior mismatch | Medium | dual-render golden/integration tests |
| build complexity due Zig | Medium | exact version + CI cache + reproducible packaging |

---

## 20. 하지 말아야 할 것

### 20.1 VT parser를 새로 작성하지 않는다

Ghostty core를 사용하는 목적 자체가 terminal semantics의 장기 유지비를 줄이는 것이다.

### 20.2 처음부터 Ghostty 전체 GUI/rendering stack에 종속되지 않는다

Desktop cross-platform renderer는 Orca-owned `wgpu` layer로 유지한다.

### 20.3 legacy path를 먼저 삭제하지 않는다

Native path가 production gate를 통과할 때까지 legacy path는 fallback/reference implementation이다.

### 20.4 remote protocol을 migration과 동시에 재설계하지 않는다

웹/모바일 remote path는 이미 별도로 동작한다. Scope를 불필요하게 키우지 않는다.

### 20.5 React state를 native terminal state의 source of truth로 만들지 않는다

React는 layout/control plane이다.

Terminal state는 native host가 소유한다.

### 20.6 surface POC 전에 renderer 전체를 만들지 않는다

가장 큰 uncertainty는 VT parser가 아니라 Tauri native composition이다.

따라서 반드시:

```text
Phase 1 core spike
Phase 2 renderer spike
Phase 3 surface spike
```

에서 go/no-go를 판단한 뒤 production implementation을 확대한다.

---

## 21. 추천 파일 구조

초기 production implementation 예상:

```text
src-tauri/src/
  native_terminal/
    mod.rs
    engine.rs
    ghostty_vt.rs
    render_model.rs
    renderer.rs
    glyph_cache.rs
    compositor.rs
    registry.rs
    host.rs
    input.rs
    metrics.rs
    platform/
      mod.rs
      macos.rs
      windows.rs
      linux.rs

  ipc/
    native_terminal.rs
```

Frontend:

```text
ui/src/
  components/
    TerminalPane.tsx
    NativeTerminalPane.tsx
    LegacyTerminalPane.tsx

  lib/
    terminalRendererMode.ts
    nativeTerminal.ts
    nativeTerminalGeometry.ts
    terminalHostManager.ts          # legacy/browser/fallback
```

기존 backend files는 최대한 유지:

```text
src-tauri/src/terminal/
src-tauri/src/daemon/
src-tauri/src/remote/
```

---

## 22. Migration dependency graph

```text
Phase 0 baseline
      |
      v
Phase 1 ghostty-vt core
      |
      v
Phase 2 standalone wgpu renderer
      |
      v
Phase 3 Tauri surface POC  <---- CRITICAL GO/NO-GO
      |
      v
Phase 4 single real PTY
      |
      v
Phase 5 UX parity
      |
      v
Phase 6 multi-pane/lifecycle
      |
      v
Phase 7 restore/hardening    <---- PRODUCTION GO/NO-GO
      |
      v
Phase 8 perf A/B             <---- DEFAULT GO/NO-GO
      |
      v
Phase 9 rollout
      |
      v
Phase 10 desktop legacy cleanup
      |
      v
Phase 11 OSS extraction
```

---

## 23. Production default 완료 조건

Native renderer를 desktop default로 바꾸려면 최소한 다음 조건을 모두 충족해야 한다.

### Architecture

- desktop output hot path에서 JS/WebView terminal processing 없음
- backend session lifecycle와 renderer lifecycle 분리
- native renderer failure가 PTY session을 종료하지 않음

### Functionality

- shell
- vim/neovim
- tmux
- selection
- clipboard
- search
- resize
- split
- tab switch
- title/bell
- mouse
- Korean/Japanese IME
- CJK/emoji

### Restore

- HMR/remount 안전
- app restart가 현재 동작보다 퇴행하지 않음
- daemon epoch/replay gap 안전

### Cross-platform

해당 OS에서 native default를 켜기 전:

- renderer 안정
- IME 안정
- DPI 안정
- surface layering 안정

### Performance

- legacy baseline 대비 sustained output에서 명확한 이점
- multi-pane에서 frame stalls 개선
- idle resource regression 없음
- memory leak 없음

### Fallback

- renderer runtime flag
- startup failure fallback
- backend session 유지

---

## 24. 첫 구현 iteration의 정확한 범위

첫 번째 coding iteration은 크게 잡지 않는다.

### Iteration 1

목표:

> `libghostty-vt`를 Rust에서 구동하고 renderable cell snapshot을 얻는다.

Deliverables:

- pinned Ghostty build
- Rust FFI wrapper
- 80x24 terminal
- bytes feed
- resize
- row/cell dump
- cursor
- ANSI/Unicode tests

이 단계에서는 Tauri, wgpu, React를 건드리지 않는다.

### Iteration 2

목표:

> standalone wgpu window에서 Ghostty terminal state를 그린다.

Deliverables:

- one native window
- one terminal
- basic font
- ANSI colors
- cursor
- actual PTY or deterministic byte stream
- throughput metrics

### Iteration 3

목표:

> Orca Tauri window 안에 1개의 native terminal rectangle을 삽입한다.

Deliverables:

- React placeholder rectangle
- native surface geometry sync
- resize
- focus
- basic keyboard
- IME spike
- overlay/z-order report

**Iteration 3이 성공한 이후에만 full migration implementation을 시작한다.**

---

## 25. 결론

Orca의 목표가 macOS/Windows/Linux desktop application이고 현재 web terminal의 renderer/lifecycle 문제를 계속 수정하는 비용이 커지고 있다면, 장기적으로는 다음 구조가 더 적합하다.

```text
Desktop
  -> libghostty-vt + Orca wgpu native renderer

Web / Mobile Browser
  -> web terminal

Backend
  -> current Orca daemon/session/remote architecture
```

이 migration에서 가장 어려운 부분은 terminal VT parser가 아니다. Ghostty core를 사용하면 그 문제는 상당 부분 제거된다.

가장 먼저 검증해야 하는 것은:

1. `libghostty-vt` C API가 필요한 render state를 충분히 제공하는가
2. `wgpu` renderer의 text/CJK pipeline이 요구 수준을 만족하는가
3. **Tauri WebView와 native compositor를 macOS/Windows/Linux에서 안정적으로 조합할 수 있는가**
4. app restart 시 bounded replay를 넘어 terminal state를 어떻게 복원할 것인가

특히 3번은 Phase 3에서 반드시 조기에 검증한다.

이 POC가 성공하면 native migration은 단순 성능 개선을 넘어 다음 문제를 동시에 해결하는 방향이 된다.

- legacy/WebView renderer lifecycle 복잡성 제거
- tab/split/restore rendering bug 감소
- desktop terminal hot path의 JS 제거
- 높은 throughput ceiling
- 하나의 wgpu renderer로 desktop cross-platform 지원
- 향후 embeddable native terminal OSS로 분리 가능한 architecture

따라서 구현 전략은 **legacy를 더 크게 고친 뒤 언젠가 옮기는 방식이 아니라, legacy를 production fallback으로 유지한 상태에서 native path를 지금부터 병렬로 세우고 조기에 go/no-go를 판정하는 방식**으로 진행한다.

---

## 26. 외부 기술 기준

구현 시 기준으로 확인할 upstream 문서:

- libghostty-vt C API: https://github.com/ghostty-org/ghostty/blob/main/include/ghostty/vt.h
- libghostty-vt build/CMake integration: https://github.com/ghostty-org/ghostty/blob/main/CMakeLists.txt
- Ghostty repository: https://github.com/ghostty-org/ghostty
- wgpu repository: https://github.com/gfx-rs/wgpu

`libghostty-vt` API는 현재 unstable이므로 실제 구현 시 이 문서에 적힌 conceptual API보다 **pinned revision의 header를 source of truth로 사용한다.**
