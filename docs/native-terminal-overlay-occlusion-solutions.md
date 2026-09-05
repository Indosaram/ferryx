# Native Terminal Overlay Occlusion — 근본 해결 설계 (조사 전용, 미구현)

> **Status**: Investigation / design only. No code changes in this document.
> **Date**: 2026-09-03
> **Scope**: 데스크톱(Tauri) 네이티브 터미널 위에 React DOM 오버레이(컨텍스트 메뉴, New Tab 팝오버, 검색 오버레이, 설정 모달, DnD 피드백)를 띄울 때 터미널 화면이 통째로 사라지는 문제의 근본 해결책.

---

## 1. 문제 정의

### 1.1 증상
- 탭바 `+` 버튼 → `NewTabPopover`가 열리는 동안 터미널 화면 전체가 사라진다.
- 터미널 본문 우클릭 → `ContextMenu`(role="menu")가 열리는 동안 동일하게 터미널이 사라진다.
- 설정 모달(role="dialog"), 터미널 검색 오버레이(role="search")도 같은 경로로 터미널을 숨긴다.

### 1.2 원인 체인 (코드 근거)
1. **Z-오더 제약(Airspace 문제)**: 모든 플랫폼에서 네이티브 터미널 서피스는 웹뷰 **위**에 그려진다.
   - macOS: `FerryxNativeTerminalView`가 `content_view.addSubview_positioned_relativeTo(..., NSWindowOrderingMode::Above, None)`로 WKWebView 위에 배치됨 (`src-tauri/src/native_terminal/platform/macos.rs:398`).
   - Windows: 자식 HWND + 독립 wgpu 스왑체인 (`platform/windows.rs`), WebView2도 자체 자식 HWND 체인이라 DOM z-index로는 어떤 네이티브 HWND도 덮을 수 없음.
   - Linux: X11 자식 윈도우 / Wayland subsurface (`platform/linux.rs`, `platform/wayland_child.rs`) — subsurface는 부모 서피스 위에 스택됨.
2. **억제 로직**: `ui/src/lib/nativeTerminalVisibility.tsx`의 `useNativeTerminalVisibility()`가 DOM에 `[role="dialog"], [role="search"], [role="menu"]`가 마운트되면(MutationObserver) `visible=false`를 반환한다.
3. **화면 소거**: `visible=false` → `NativeTerminalPane`이 detach 수행 → `cmd_native_terminal_detach` → `NativeTerminalSurfaceHostState::detach_session` (`surface_host.rs:1399`) → `hosts.remove(session_id)` → 플랫폼 타깃 Drop:
   - macOS: `removeFromSuperview()` — NSView가 즉시 사라짐.
   - Windows: `ShowWindow(hwnd, SW_HIDE)` (`platform/windows.rs:335`).
   - Linux: X11 unmap / Wayland subsurface destroy (`platform/linux.rs:418` Drop).
4. **재부착 비용**: 오버레이가 닫히면 re-attach + WGPU 서피스 재생성 + 첫 프레임 대기 → 깜빡임. (과거 "split darkening"·"dead detach/attach race" 회귀의 근원이었던 바로 그 경로다.)

### 1.3 현재 임시 조치의 한계
- `NewTabPopover`에 단 `data-native-terminal-yield="off"`를 준 상태는 **거짓 해결**이다: 서피스를 숨기지 않으면 팝오버가 터미널 영역 위로 낼려갈 때 네이티브 뷰 뒤에 파묻혀 보이지 않는다. jsdom 테스트는 레이어드 렌더링이 없어 이를 검증하지 못한다.

---

## 2. 해결 방안 비교

### 방안 A — "오버레이 사각형 마스킹"(권장, 근본 해결)
오버레이가 열리는 동안 네이티브 서피스를 **숨기지 않고**, 해당 오버레이의 화면 사각형만큼만 네이티브 렌더링에서 제외한다.

- **구현 개념**:
  1. 프런트: 오버레이(ContextMenu, NewTabPopover, TerminalSearchOverlay, 설정 모달)가 자신의 실측 `getBoundingClientRect()`를 IPC로 보고 (`cmd_native_terminal_set_overlay_exclusions: Vec<LogicalRect>`). MutationObserver 억제 로직은 제거.
  2. 백엔드: 세션별 occlusion rect 목록을 유지. 렌더 패스에서 해당 rect 영역을 터미널 배경색(테마 bg)으로 채우거나(wgpu scissor로 간단히 구현 가능 — 현재 `SurfaceCompositionLayout.physical_bounds` scissor 인프라 재사용), 플랫폼 서피스를 rect만큼 축소.
  3. DOM 오버레이는 여전히 네이티브 뷰 **뒤**에 있지만, 덮이는 영역이 배경색으로 칠해지므로 사용자에게는 오버레이가 정상적으로 위에 보이는 것과 동일한 결과. — **주의: 이 방식은 오버레이 영역이 네이티브 뷰보다 뒤에 있으므로 실제로는 보이지 않는다.** 따라서 A안은 단독으로는 불완전하며 아래 A′이 진짜 형태다.

- **방안 A′ (실현 가능한 변형) — "정지 프레임 스냅샷 + 부분 숨김 없음"**:
  오버레이가 열리는 순간 해당 세션의 마지막 렌더 프레임을 wgpu 텍스처 readback → RGBA 비트맵으로 프런트에 전달(또는 공유 메모리), 프런트가 그 비트맵을 터미널 위치의 `<canvas>`/`<img>`에 표시한 뒤 네이티브 서피스를 숨긴다. 오버레이가 닫히면 re-attach.
  - 장점: 어느 플랫폼에서나 동일하게 동작, 오버레이 DOM이 실제로 위에 보임, 터미널이 "사라진" 것이 아니라 "멈춘 화면"으로 보임.
  - 단점: 오버레이가 열린 동안 터미널 출력이 화면에 반영되지 않음(정지 화면). 컨텍스트 메뉴/팝오버처럼 수 초 이내의 짧은 상호작용에는 문제없음. readback 지연(수 ms)과 대형 서피스의 메모리 비용 존재. re-attach 깜빡임은 여전히 존재(단, 스냅샷이 그 자리를 메우고 있으므로 체감 깜빡임은 크게 줄어듦).

### 방안 B — 플랫폼별 "위에 있는" 네이티브 오버레이 (최고 품질, 비용 최대)
- macOS: 오버레이 DOM rect에 맞춘 투명 `NSPanel`/자식 `NSWindow`를 네이티브 뷰 위에 띄우고, 그 안에 별도 웹뷰(또는 네이티브 렌더링)로 React 메뉴를 표시. 사실상 "네이티브 메뉴"의 다른 얼굴이라 사용자 요구(네이티브 싫음)와 충돌.
- Windows: layered window(`WS_EX_LAYERED` + `UpdateLayeredWindow`)로 per-pixel alpha 오버레이 윈도우. 기술적으로 DOM 없이는 메뉴를 다시 그려야 함.
- 평가: 크로스플랫폼 비용이 가장 크고 "React로 통일" 요구에 반함. **기각**.

### 방안 C — 렌더러를 DOM 위로 올리는 아키텍처 전환 (진짜 근본, 장기)
- macOS 한정으로는 WKWebView를 투명하게 만들고(`"transparent": true`는 이미 설정됨, `tauri.conf.json`) 터미널 NSView를 웹뷰 **아래**(`NSWindowOrderingMode::Below`)에 두면, 터미널 영역 DOM이 이미 `background: transparent !important`이므로 터미널이 웹뷰를 뚫고 보이고, DOM 오버레이는 자연스럽게 위에 렌더된다. CSS 인프라(`html.platform-macos ... transparent` 규칙, commit `43071bcd`)가 이미 이 방향으로 존재한다.
- 그러나 **Windows에서는 불가능**: WebView2 컨트롤은 자체 자식 HWND로 합성되며 그 영역은 항상 불투명하다. 웹뷰 아래 놓인 자식 HWND는 WebView2가 그린 픽셀에 가려 절대 보이지 않는다. Linux X11도 웹뷰 자체가 자식 윈도우 체인이라 동일 제약.
- 결론: C안은 macOS 전용 특례로는 가능하나, 크로스플랫폼 일관성(프로젝트 핵심 컨벤션)을 깨고 플랫폼별 분기 렌더링 정책을 유지해야 하므로 **장기 과제로만 가치**가 있다. Windows/WebView2의 CompositionController(`ICoreWebView2CompositionController`)로 전환하면 시각 트리 합성이 가능해지나 이는 WebView2 교체급 공사.

### 방안 D — 렌더러 교체: 네이티브 서피스 폐기, DOM/WebGL(xterm)로 회귀
- 오버레이 문제가 원천 소멸(모든 것이 DOM). 리모트 웹 클라이언트가 이미 이 모델.
- 단점: 네이티브 마이그레이션(commit `38f887e`)의 목적이었던 렌더링 품질/성능(글리프 합성, IME, 스루풋)을 포기. 사용자가 이미 "완전 네이티브" 방향을 확인한 바 있어 **기각**.

---

## 3. 권장안

**단기(이번 사이클): 방안 A′ — 스냅샷 교체(overlay freeze-frame)**
1. `nativeTerminalVisibility`의 role 기반 전역 억제를 제거하고, 오버레이 소유자가 명시적으로 "이 세션/이 화면 영역을 얼린다"를 선언하는 API로 교체.
2. 백엔드에 `cmd_native_terminal_capture_frame(session_id) -> { rgba bytes, width, height }` 추가 (wgpu `copy_texture_to_buffer` readback; offscreen 렌더 테스트 인프라 `tests/native_terminal_renderer_contract/offscreen_render.rs`가 이미 readback 경로를 증명).
3. `NativeTerminalPane`는 오버레이 open 시: 캡처 → 캔버스에 표시 → detach(숨김). close 시: re-attach → 첫 프레임 present 후 캔버스 제거(기존 `reveal()` 타이밍과 동일한 순서 보장).
4. 적용 대상: `ContextMenu`(role="menu"), `NewTabPopover`(role="dialog"), `TerminalSearchOverlay`(role="search"), 설정 모달. DnD 피드백의 per-pane yield(`NativeTerminalVisibilityProvider visible={!showsDropFeedback}`)도 같은 스냅샷 경로로 옮기면 "드래그 중 다른 터미널 안 보임" 회귀 없이 드래그 타깃만 얼릴 수 있다.
5. 회귀 방지: 캡처 실패 시에는 현재 동작(그냥 숨김)으로 폴 back — 절대 입력 경로를 막지 않음.

**장기: 방안 C의 macOS Below-webview 전환 타당성을 별도 스파이크로 검증** (Windows CompositionController 전환과 함께 평가해야 진정한 크로스플랫폼 근본 해결).

---

## 4. 검증 계획 (구현 시)
- 단위: readback 캡처의 픽셀 계약 테스트(기존 offscreen_render 계약 테스트 패턴 재사용).
- 프런트: 오버레이 open/close 시 캔버스↔네이티브 교체 순서를 상태 머신 테스트로 고정(jsdom에서 검증 가능한 부분만).
- **실기기 QA 필수**: jsdom은 레이어드 렌더링이 없어 이 클래스의 버그를 절대 못 잡는다. `bun tauri dev` 디버그 앱에서 (1) 터미널 우클릭 컨텍스트 메뉴, (2) 탭바 `+` 팝오버, (3) 검색 오버레이, (4) 설정 모달, (5) 팬 DnD 피드백 — 각각 화면이 멈춘 채 유지되는지 사용자 확인.

---

## 5. 부록: 현재 억제 트리거 일람
- `ui/src/lib/nativeTerminalVisibility.tsx` — `[role="dialog"], [role="search"], [role="menu"]` 전역 MutationObserver. `data-native-terminal-yield="off"` opt-out 존재(거짓 해결 상태로 NewTabPopover에 적용됨).
- `ContextMenu.tsx:204` — role="menu" (터미널 우클릭, `NativeTerminalPane.tsx:2014`).
- `NewTabPopover.tsx:115` — role="dialog".
- `TerminalSearchOverlay` — role="search" (TerminalPane 내 `searchOpen`).
- DnD: `TerminalSplitView`의 `dropFeedbackLeafId` → per-pane `NativeTerminalVisibilityProvider`.
