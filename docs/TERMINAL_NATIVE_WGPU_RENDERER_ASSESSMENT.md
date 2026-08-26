# Native wgpu Terminal Renderer 성능·비용 평가

## 요약

Orca-lite의 현재 terminal stack을 xterm.js/WebView 중심 구조에서 native terminal engine + `wgpu` renderer 구조로 완전히 옮기면, **성능 상한(performance ceiling)은 현재보다 상당히 높아질 가능성이 큽니다.** 특히 초고속 대량 stdout, 여러 terminal의 동시 출력, 높은 refresh-rate 환경에서는 CPU 사용량·frame latency·처리량 측면에서 큰 개선 여지가 있습니다.

다만 **`wgpu`를 사용한다는 사실 자체가 성능 향상을 보장하는 것은 아닙니다.** 현재 Orca-lite는 이미 xterm.js의 `@xterm/addon-webgl`을 사용하므로 glyph를 GPU로 그리는 부분만 native `wgpu`로 교체하는 정도라면 투자 대비 효과가 제한적일 수 있습니다.

큰 성능 차이를 만들려면 다음 전체 hot path를 native로 옮겨야 합니다.

```text
현재
PTY -> Rust -> Tauri IPC -> WebView -> JS -> xterm parser/buffer -> WebGL renderer

완전한 native terminal engine
PTY -> Rust terminal parser/grid -> wgpu -> GPU
```

따라서 native renderer는 단순한 렌더러 최적화가 아니라 **terminal emulator architecture 자체를 새로 구축하거나 기존 native engine을 통합하는 프로젝트**로 봐야 합니다.

---

## 현재 Orca-lite 상태

terminal stdout의 WebView 전달 경로는 이미 binary Tauri Channel로 개선되었습니다.

기존 hot path는 다음과 같았습니다.

```text
PTY bytes
  -> Rust Vec<u8>
  -> base64 encode
  -> Tauri JSON event
  -> WebView JS string
  -> base64 decode
  -> TextDecoder
  -> scheduler
  -> xterm.write(string)
```

현재 live stdout path는 다음과 같이 바뀌었습니다.

```text
PTY bytes
  -> Rust Vec<u8>
  -> compact binary Tauri Channel frame
  -> ArrayBuffer / Uint8Array
  -> scheduler
  -> xterm.write(Uint8Array)
```

따라서 native renderer를 검토할 때는 예전 JSON/base64 IPC 비용을 포함한 상태가 아니라, **binary Channel 적용 후 남은 병목을 기준으로 판단해야 합니다.**

또한 daemon Unix socket protocol에는 여전히 JSON/base64 경계가 존재하므로, terminal throughput 병목을 정확히 분석할 때는 다음을 분리해서 측정해야 합니다.

1. PTY/daemon 생산 속도
2. daemon -> desktop Rust 전달 비용
3. Rust -> WebView binary Channel 비용
4. JS scheduling 비용
5. xterm parser/buffer 비용
6. xterm WebGL rendering 비용

---

## 왜 native renderer가 더 빠를 수 있는가

### 1. WebView와 JavaScript main thread를 terminal hot path에서 제거할 수 있음

현재 xterm.js 기반 구조에서는 출력량이 증가할수록 다음 비용이 누적됩니다.

- Tauri -> WebView IPC dispatch
- JavaScript callback/event dispatch
- `Uint8Array` batching
- xterm.js terminal parser
- xterm.js buffer/grid state 관리
- JavaScript object allocation 및 GC
- WebGL renderer 호출 전후의 JS-side layout/state 처리

native terminal engine에서는 terminal parser와 screen state를 Rust/native 쪽에 둘 수 있으므로 이 계층의 상당 부분을 제거할 수 있습니다.

### 2. parser, grid, renderer의 메모리 모델을 통합할 수 있음

xterm.js에서는 parser/buffer/renderer가 JavaScript/WebGL 환경의 제약을 받습니다. native engine에서는 parser가 생성한 screen state를 renderer가 직접 소비하도록 데이터 구조를 설계할 수 있습니다.

예를 들어 다음과 같은 최적화가 가능합니다.

- compact cell/grid representation
- allocation 최소화
- contiguous dirty-row tracking
- glyph atlas 직접 관리
- partial redraw
- zero/low-copy render submission
- SIMD-friendly parsing/data layout
- JS GC와 독립된 predictable memory management

### 3. 여러 terminal 동시 출력에서 유리함

한 개의 shell에서 평범한 interactive 작업을 할 때는 현재 xterm.js WebGL도 충분히 빠를 수 있습니다.

하지만 여러 pane/tab에서 동시에 빌드·로그·agent output이 발생하면 JS main thread와 WebView event loop가 공유 자원이 됩니다. native engine은 parsing과 rendering architecture를 별도로 설계하여 contention을 줄일 여지가 훨씬 큽니다.

---

## 기대 효과가 큰 상황과 작은 상황

| 사용 상황 | native wgpu 기대 효과 |
| --- | --- |
| 일반 shell 입력, `ls`, 짧은 명령 | 체감 차이가 작을 가능성이 큼 |
| 일반적인 build output | frame 안정성·CPU 사용량 개선 가능 |
| `cat`/대형 log 등 초고속 stdout | 큰 처리량 차이가 날 가능성이 높음 |
| 여러 terminal 동시 대량 출력 | 큰 차이가 날 가능성이 높음 |
| 120/144Hz에서 지속적인 화면 갱신 | native 쪽이 유리 |
| WebView JS GC pressure | native 쪽이 크게 유리 |
| 단순 키 입력 latency | 이미 충분히 빠르면 차이가 작을 수 있음 |

핵심은 **평범한 interactive latency보다 sustained high-throughput output에서 native architecture의 이점이 커진다**는 점입니다.

---

## `wgpu` renderer만 교체하면 충분한가?

아닙니다.

현재도 xterm.js는 `@xterm/addon-webgl`을 사용하므로, 병목이 단순히 "GPU를 사용하지 않아서" 발생하는 것은 아닙니다.

다음 구조처럼 xterm parser와 JS buffer model을 유지한 채 draw backend만 native `wgpu`로 바꾸는 경우:

```text
PTY
  -> Tauri/WebView
  -> xterm/JS parser
  -> xterm/JS buffer
  -> native wgpu renderer
```

다음 비용은 그대로 남습니다.

- WebView IPC
- JS event scheduling
- xterm parser
- xterm screen buffer
- JS allocation/GC
- JS/native renderer synchronization

따라서 투자 대비 효과가 제한될 수 있습니다.

진짜 높은 성능 상한을 노리려면 다음과 같이 **parser + terminal state + renderer를 함께 native화**해야 합니다.

```text
PTY
  -> native parser
  -> native terminal grid / scrollback
  -> native dirty tracking
  -> wgpu renderer
```

---

## Ghostty가 빠른 이유를 단순히 GPU라고 보면 안 되는 이유

Ghostty 계열의 성능은 단순히 Metal/OpenGL/Vulkan/GPU API를 선택했기 때문만이 아닙니다.

성능에 중요한 것은 terminal 전체 architecture입니다.

- parser 처리량
- screen/grid representation
- scrollback 관리
- glyph shaping/rasterization
- glyph atlas
- dirty-region tracking
- render submission
- input/event loop
- memory allocation 전략

따라서 Orca-lite가 Ghostty 수준의 native terminal performance를 원한다면, "xterm renderer를 `wgpu`로 교체"하는 범위를 넘어서는 작업이 필요합니다.

---

## 구현 비용

완전한 native terminal renderer를 직접 구현하려면 최소한 다음 subsystem이 필요합니다.

### Terminal engine

- ANSI/VT parser
- terminal grid/cell state
- scrollback
- alternate screen
- cursor state
- SGR/color attributes
- hyperlinks/OSC handling
- resize/reflow
- wide characters / combining characters
- Unicode width handling

### Text rendering

- font loading
- font fallback
- glyph shaping
- ligatures 정책
- glyph rasterization
- glyph atlas/cache
- emoji/color glyph 처리
- DPI scaling

### GPU rendering

- `wgpu` device/surface lifecycle
- vertex/instance buffers
- cell background rendering
- glyph rendering
- selection/cursor overlays
- dirty-region or frame invalidation strategy
- context/device-loss recovery

### Desktop interaction

- IME
- keyboard input
- mouse reporting
- selection
- clipboard
- links
- drag/drop
- accessibility
- focus
- window scaling
- macOS/Windows/Linux 차이

### Orca-lite integration

- pane/tab lifecycle
- session restore
- terminal title/activity tracking
- search
- split resize
- theme/settings
- remote terminal behavior
- existing xterm-based tests/behavior parity

즉, 자체 구현은 **IPC 최적화가 아니라 terminal emulator subsystem을 하나 새로 만드는 수준**입니다.

---

## 먼저 해야 할 benchmark

현재 binary Channel이 적용됐으므로, native renderer 투자 여부는 실제 병목 측정 후 결정하는 것이 가장 안전합니다.

### 테스트 workload 예시

```bash
yes "012345678901234567890123456789" | head -n 1000000
```

또는 실제 workload와 가까운 다음 데이터를 사용하는 것이 좋습니다.

- 50~500 MB build log
- ANSI color가 많은 log
- long-line output
- Unicode/Korean/emoji 혼합 output
- 2/4/8개 terminal의 동시 burst
- foreground/background pane 혼합

### 측정할 지표

#### Rust/daemon

- PTY input bytes/sec
- daemon relay bytes/sec
- Channel send bytes/sec
- batch size distribution

#### Frontend scheduler

이미 추가된 dev-only counters를 이용해 다음을 측정합니다.

- received chunks
- received bytes
- xterm writes
- frame flush count
- threshold flush count
- coalesced chunks
- average frame wait
- max frame wait

#### WebView/xterm

- `xterm.write()` 처리 시간
- JS main-thread utilization
- long task 발생 횟수
- GC pressure
- frame time
- dropped/stalled frame
- CPU usage
- GPU usage
- memory usage

---

## native renderer 투자 판단 기준

예를 들어 측정 결과가 다음과 같다면 native renderer 투자 가치가 높습니다.

```text
PTY / Rust / Channel: 300~500 MB/s 이상 처리 가능
xterm parser/render:   30~50 MB/s에서 포화
JS main thread:        sustained high utilization
frame stalls:          반복 발생
```

이 경우 병목이 WebView/xterm 쪽에 명확하게 있으므로 native terminal engine이 큰 성능 향상을 만들 가능성이 높습니다.

반대로 다음과 같은 결과라면 우선순위가 낮습니다.

```text
실제 workload:        5~15 MB/s 이하
binary Channel 이후: frame drop 거의 없음
JS main thread:       충분한 여유
사용자 체감 latency: 이미 양호
```

이 상태에서 몇 달 규모의 native terminal engine을 만드는 것은 과투자일 가능성이 큽니다.

---

## 추천 진행 순서

### 1단계 — 현재 xterm + binary Channel 기준선 확립

현재 구현에서 실제 throughput/frame benchmark를 먼저 확보합니다.

목표는 다음 질문에 답하는 것입니다.

> 현재 병목이 daemon/IPC인가, scheduler인가, xterm parser인가, renderer인가?

### 2단계 — xterm-side 최적화

native rewrite 전에 비용이 낮은 최적화를 먼저 검토할 수 있습니다.

- batching behavior 조정
- unnecessary title/decode work 확인
- hidden terminal output 정책
- xterm option/scrollback tuning
- render/update frequency 측정

단, fixed coalesce interval은 측정 없이 임의로 추가하지 않습니다.

### 3단계 — native prototype

병목이 xterm/WebView로 확인되면 전체 replacement 전에 작은 prototype을 만드는 것이 좋습니다.

prototype 목표 예시:

- 한 개 PTY
- 기본 ANSI color
- 고정 font
- scrollback 최소화
- selection/IME/accessibility 제외
- 동일 workload에서 raw throughput/frame time 비교

prototype만으로도 native architecture의 실질적인 성능 상한을 검증할 수 있습니다.

### 4단계 — architecture 결정

prototype 결과가 충분히 크면 다음 중 하나를 선택합니다.

1. 자체 Rust terminal engine + `wgpu`
2. 기존 native terminal engine/library 활용
3. Ghostty 계열 engine/embed 가능성 조사
4. xterm.js 유지 + 현재 최적화 지속

---

## 결론

**완전한 native terminal engine + `wgpu` renderer는 현재 xterm.js/WebView 구조보다 성능 상한이 훨씬 높을 가능성이 큽니다.** 특히 대량 stdout과 다중 terminal workload에서는 큰 차이가 날 가능성이 높습니다.

그러나 현재 Orca-lite는 이미 live stdout의 app -> WebView 경로를 binary Tauri Channel로 바꾸었고 xterm.js도 WebGL renderer를 사용하고 있습니다. 따라서 다음 단계는 곧바로 native rewrite를 시작하는 것이 아니라, **현재 구조에서 남은 병목을 수치로 확인하는 것**입니다.

판단 기준은 단순합니다.

> binary Channel 이후에도 xterm/WebView가 명확한 throughput/frame bottleneck으로 남는다면 native renderer 투자는 기술적으로 타당합니다.

그리고 그때의 목표는 "`wgpu`로 그림만 그리는 renderer"가 아니라 **parser, terminal state, glyph pipeline, renderer까지 포함한 native terminal subsystem**이어야 큰 성능 향상을 기대할 수 있습니다.
