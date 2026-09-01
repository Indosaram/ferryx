---
title: Technical Architecture
description: Why Ferryx uses Tauri v2, WGPU, a Rust PTY daemon, and libghostty.
---

:::note[한국어 요약: Ferryx 아키텍처 요약]
Ferryx는 여러 코딩 에이전트의 터미널 세션을 병렬로 관리하기 위해 설계된 워크스페이스입니다. 데스크톱 셸은 Tauri v2, 렌더링 파이프라인은 libghostty-vt 파서와 WGPU 네이티브 캔버스로 구성했습니다. 백그라운드 Rust 데몬이 PTY 세션을 독립적으로 관리하므로 GUI가 종료되거나 다시 로드되어도 에이전트 프로세스는 계속 실행됩니다. 연결이 끊기면 단조 증가 시퀀스 번호와 링 버퍼를 통해 누락된 출력을 재생합니다. 모바일 원격 클라이언트는 xterm.js 의존성을 제거하고 커스텀 DOM 그리드로 교체했습니다. 저장소에 측정되지 않은 타사 비교 벤치마크는 게시하지 않으며, 실측 수치와 측정 범위를 함께 공개합니다.
:::

Ferryx runs multiple coding-agent terminal sessions in parallel. Agents such as Claude Code, Codex, or Gemini CLI produce continuous ANSI streams, operate in separate Git worktrees, and may need remote steering.

Ferryx uses Tauri v2, a pinned `libghostty-vt` engine, a WGPU rendering pipeline, and an independent Rust PTY daemon. The sections below describe the measured parts of that architecture without claiming a performance advantage over another product.

Here is the exact architecture, along with real repository measurements and their limitations.

```text
+-------------------------------------------------------------------------+
|                        FERRYX SYSTEM TOPOLOGY                           |
|                                                                         |
|  +-------------------------------------------------------------------+  |
|  |                 Rust Headless PTY Daemon (tokio)                  |  |
|  |  * PTY process supervisor (Claude Code, Codex, shell sessions)   |  |
|  |  * Monotonic sequence counter & ring buffer for output replay     |  |
|  |  * Unix domain socket server with NDJSON protocol                 |  |
|  +---------------------------------+---------------------------------+  |
|                                    | (Local UDS Stream)                 |
|                                    v                                    |
|  +-------------------------------------------------------------------+  |
|  |                    Tauri v2 Desktop Host (Rust)                   |  |
|  |  +-------------------------------------------------------------+  |  |
|  |  |                 Daemon Client & IPC Router                  |  |  |
|  |  +------------------------------+------------------------------+  |  |
|  |                                 |                                 |  |
|  |        +------------------------+------------------------+        |  |
|  |        v                                                 v        |  |
|  |  +---------------------------+     +---------------------------+  |  |
|  |  | libghostty-vt Core (FFI)  |     | Axum WebSocket Gateway    |  |  |
|  |  | ANSI parser & grid state  |     | Authenticated token / PIN |  |  |
|  |  +-------------+-------------+     +-------------+-------------+  |  |
|  |                v                                 v                |  |
|  |  +---------------------------+     +---------------------------+  |  |
|  |  | WGPU Native Surface       |     | Remote Browser Client     |  |  |
|  |  | Metal / Vulkan / DX12     |     | Custom React DOM Grid     |  |  |
|  |  | CoreText font rasterizer  |     | (No xterm.js dependency)  |  |  |
|  |  +-------------+-------------+     +---------------------------+  |  |
|  |                | (Rendered surface)                               |  |
|  |                v                                                  |  |
|  |  +-------------------------------------------------------------+  |  |
|  |  | Native AppKit / OS Window Shell (Transparent Webview Pane)   |  |  |
|  |  | React 18 UI overlay: tabs, split panes, agent status badge   |  |  |
|  |  +-------------------------------------------------------------+  |  |
|  +-------------------------------------------------------------------+  |
+-------------------------------------------------------------------------+
```

## 1. Why Tauri v2 Over Electron

Electron packages Chromium and Node.js with an application. Ferryx instead uses the operating system webview for interface chrome and keeps terminal processing in Rust. We have not measured the memory difference against an Electron competitor, so this section describes the boundary rather than quantifying an advantage.

Tauri v2 splits the application into two clean layers:
- The desktop shell uses the operating system's native webview (WebKit on macOS, WebView2 on Windows) strictly for lightweight UI chrome like tabs, settings, and status docks.
- Heavy operations such as process management, socket streaming, VT parsing, and GPU rendering run directly in compiled Rust.

This boundary separates browser-rendered controls from process management, VT parsing, and native rendering.

## 2. Terminal Engine: libghostty-vt and WGPU

Ferryx bypasses the webview rendering engine for desktop terminal panes. The project has not measured this choice against another terminal renderer; the evidence below covers only Ferryx's own implementation.

### Pinned libghostty Parser Core

We statically link `libghostty-vt` at pinned commit `6a508fd5e34c7e222c052a6d00bb3891ff3feace` built with Zig 0.16.0 (`src-tauri/native_terminal/build_ghostty.rs`). This library handles VT sequence interpretation, alternate screens, scrollback history, SGR styling, and Unicode combining marks in native code.

Rust wraps the Ghostty FFI structures with typed guard destructors (`src-tauri/src/native_terminal/guards.rs`). To verify memory safety, we ran dynamic lifetime tests on macOS using `leaks` and GuardMalloc (`MallocScribble=1`):

- Test scenario: 50 repeated create, feed, snapshot, resize, and drop lifecycle iterations.
- Result: **0 leaks for 0 total leaked bytes** (`bench/terminal/evidence/native-terminal-phase1-safety-abi-audit.md:43-46`).

### Hardware-Accelerated WGPU Renderer

Terminal cells from `libghostty-vt` pass to a custom WGPU renderer (`src-tauri/src/native_terminal/renderer/renderer.rs`). The renderer maintains a dynamic glyph atlas and checks row-level dirty state before issuing draw calls:

- **Measured offscreen latency:** On an Apple M4 Max GPU (Metal backend, 800x480 resolution, 24 rows), a 50-frame headless benchmark recorded a **median (p50) frame time of 3.096 ms (~3.10 ms)** and a **p95 latency of 4.337 ms** (`bench/terminal/evidence/native-wgpu-phase2-summary.md:18`).
- **Dirty row optimization:** 1 rebuilt row and 23 reused rows during incremental updates, caching unchanged glyph quads.

This native WGPU surface renders directly onto the OS window beneath a transparent webview cutout, preventing UI re-renders from stalling terminal output.

## 3. Microbenchmark Throughput and Its Limits

During early architectural baselining, we evaluated in-memory data transmission pipelines between the host process and the UI (`bench/terminal/evidence/baseline-summary.md:15`):

| Pipeline Path | Median Time (10 MiB) | Throughput | Notes |
| :--- | :--- | :--- | :--- |
| **Direct Binary Pass-Through** | **0.024 ms** | **408,847.5 MiB/s** | Raw Uint8Array chunk stream |
| **Legacy Base64 Encoding** | 87.781 ms | 113.9 MiB/s | String serialization overhead |

### Methodological Limitation

We disclose this number transparently: **408,847.5 MiB/s measures pure in-memory JavaScript buffer passing**. That figure does not represent end-to-end terminal throughput. Neither VT escape sequence parsing, terminal grid state mutation, glyph rasterization, nor GPU surface presentation is included in this microbenchmark.

We publish it to demonstrate why Base64 string IPC was abandoned in favor of raw binary streams, not as a claim about terminal rendering speed.

## 4. Headless Rust PTY Daemon and Replay Architecture

Desktop interfaces crash or hot-reload during development. If your terminal emulator hosts PTY child processes inside the GUI window process, closing or reloading the window kills all active coding agents.

Ferryx solves this with a headless daemon architecture:

1. **Independent Process Lifetime:** The daemon process runs separately from the Tauri GUI (`src-tauri/src/daemon/server.rs`). Active subshells and agents continue running even if the window closes.
2. **Sequenced Stream Protocol:** Every terminal output chunk carries a monotonic sequence counter (`src-tauri/src/daemon/protocol.rs:249`).
3. **Ring Buffer Replay:** An in-memory ring buffer keeps recent session history. When the GUI reconnects, `DaemonClient::attach` passes the `last_seen_sequence` (`src-tauri/src/daemon/server.rs:874-885`). The daemon responds with `AttachOk`, sending missed chunks before resuming the live stream.
4. **Gap Recovery:** If the client was disconnected long enough for the ring buffer to wrap, the daemon emits an explicit `replayGap` payload so the frontend can request a full snapshot rather than displaying corrupted output.

This design gives developers the session persistence of a multiplexer with the comfort of a modern graphical interface.

## 5. Remote Web Companion: Custom DOM Grid Without xterm.js

On 2026-08-25, we eliminated `xterm.js` and all `@xterm/*` dependencies from Ferryx (`docs/xterm-removal-verification.md`).

Instead of embedding a bulky web terminal parser on mobile browsers, the remote companion uses a custom DOM terminal grid (`ui/src/remote/RemoteTerminal.tsx` and `ui/src/remote/terminalGridProtocol.ts`):

- The backend computes terminal grid frames and sends lightweight row diffs over an authenticated WebSocket connection.
- Browser clients render formatted text lines directly inside standard `<pre>` blocks with styled runs (`applyGridFrame`).
- Touch controls, swipe navigation, and the mobile key dock (`MobileKeyDock`) work without loading heavy client-side terminal state machines.

Removing the external library means the remote client no longer performs VT parsing or ships the `@xterm/*` dependency family. This article does not claim an unmeasured bundle-size or defect-rate improvement.

## 6. macOS-First Integrations on a Cross-Platform Core

The core Ferryx codebase targets macOS, Linux, and Windows. The following integrations are currently macOS-specific:

- **CoreText Glyph Rasterization:** Native subpixel font rasterization and Apple Color Emoji rendering (`src-tauri/src/native_terminal/renderer/coretext_raster.rs`).
- **Dock Badge Counters:** Live agent attention counters displayed on the macOS Dock icon (`src-tauri/src/notification/badge.rs`).
- **launchd Daemon Supervision:** Automatic background daemon bootstrapping on user login via generated `.plist` files (`src-tauri/src/daemon/launchd.rs:8-96`).
- **Window Vibrancy:** Native macOS visual effect material behind transparent toolbars and split gutters.

Linux and Windows support core terminal sessions and daemon communication, while platform-specific rasterizers and service managers are added incrementally.

## 7. Zero Competitor Benchmarks

You will not find benchmark charts claiming Ferryx is "10x faster than Warp" or "uses 50% less RAM than Cursor" on this site.

Here is why:
- We have performed **zero competitor benchmarks** under rigorous lab conditions.
- Synthetic microbenchmarks often distort real developer workflows.
- Single-maintainer benchmark wars usually distract from building honest software.

Every tool occupies a different trade-off space. `tmux` provides mature remote session persistence; visual workspaces and review queues address a different operating model.
