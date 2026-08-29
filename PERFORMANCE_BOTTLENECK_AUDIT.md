# Performance Bottleneck Synthesis & Architecture Audit

**Target Worktree:** `/Users/indo/orca/workspaces/orca-lite/perf-bottleneck-audit`  
**Date:** 2026-08-29  
**Audits Synthesized:**
1. `audit-build.md` (Cargo profiles, Tauri packaging, Vite bundler & TypeScript dev loop)
2. `audit-native-terminal.md` (WGPU rendering pipeline, Libghostty FFI, macOS AppKit host, and IPC)
3. `audit-react-state.md` (React store subscriptions, persistence debouncing, drag-and-drop, and tree re-renders)
4. `audit-remote.md` (Axum WebSocket gateway, mirror locking, framing, and event streaming)
5. `audit-rust-core.md` (Daemon IPC client, startup readiness pipes, handshake timeouts, and blocking I/O)
6. `audit-terminal-ui.md` (Frontend stream ingestion, backlog copy amplification, and lifecycle listeners)

---

## 1. End-to-End Architectural Survey

Orca Lite operates as a hybrid native desktop application with remote-companion capabilities:

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                                ARCHITECTURAL DATA FLOW                                 │
│                                                                                        │
│   [PTY / CLI Processes]                                                                │
│             │                                                                          │
│             ▼                                                                          │
│   [Ferryx Rust Daemon] ──(Unix Domain Socket / NDJSON)──> [DaemonClient (Async Mutex)] │
│             │                                                        │                 │
│             ▼                                                        ▼                 │
│   [Native Terminal Host (WGPU)] <───[Tauri IPC / State]───> [Tauri App Backend]        │
│             │                                                        │                 │
│             │ (Direct Metal / Surface)                               ▼ (Axum WS)       │
│             ▼                                                [Remote Gateway]          │
│   [OS Native Viewport / AppKit]                                      │                 │
│                                                                      ▼                 │
│   [React 18 + Vite Frontend] <─────────────────────────── [Remote Web Client]          │
│     (useWorkspaceStore / DOM UI)                                                       │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

### Module Boundaries & Blast Radii
1. **Compilation & Packaging Boundary (`src-tauri/Cargo.toml`, `tauri.conf.json`, `ui/vite.config.ts`)**:
   Controls binary artifact generation, release optimizations (LTO, strip, codegen units), and asset embedding. Zero runtime blast radius beyond packaging and build times.
2. **Daemon IPC Boundary (`src-tauri/src/daemon`)**:
   Mediates communication between the GUI backend and the background Ferryx daemon over Unix domain sockets. A stall here blocks all terminal commands and lifecycle operations.
3. **Native Terminal & Rendering Engine (`src-tauri/src/native_terminal`, `src-tauri/src/ipc`)**:
   Hosts Libghostty VT terminal instances and WGPU render passes onto OS surfaces (AppKit on macOS). High-frequency output feeds, frame presentation, and per-cell FFI extraction live on this hot path.
4. **Remote Companion Gateway (`src-tauri/src/remote`)**:
   Axum WebSocket service providing raw and grid terminal streams to browser/mobile clients.
5. **Frontend State & UI Composition (`ui/src/state`, `ui/src/components`, `ui/src/lib`)**:
   React store hierarchy (`useWorkspaceStore`), persistence orchestration (`sessionPersistence.ts`), and terminal event fan-out (`terminalEvents.ts`).

---

## 2. Architectural Design Options & Trade-Off Analysis

### Design Option 1: Monolithic System Restructuring (Enterprise Manager Abstraction)
- **Concept:** Implement large new coordination subsystems (`terminalOutputScheduler`, `terminalHostManager`), rewrite the React state layer into slice-based Zustand stores with complete state migrations, introduce multiplexed binary daemon protocols, and restructure Rust into a multi-crate workspace.
- **Trade-offs:**
  - *Coupling:* High risk of breaking Tauri v2 plugin registrations and subtle event ordering across components.
  - *Testability:* Low immediate testability; invalidates 100+ existing component integration tests and requires massive test rewrites.
  - *Migration Cost:* Prohibitive (multi-week refactoring across all application layers).
  - *Failure Modes:* Protocol version skew, state synchronization drift between isolated stores, and regressions in drag-and-drop or terminal lifetime management.

### Design Option 2: Targeted Seam Hardening & In-Place Pipeline Optimization (RECOMMENDED)
- **Concept:** Apply surgically bounded, high-ROI fixes directly at proven bottleneck sites without introducing new abstraction layers:
  - Configure Cargo release profiles and eliminate redundant asset duplication in `tauri.conf.json`.
  - Add bounded timeouts and non-blocking stderr handling in `DaemonClient`.
  - Narrow mutex lock scopes in Remote Gateway and Native Terminal.
  - Gate persistence triggers on structural layout mutations rather than raw streaming ticks.
  - Stabilize component callback references and memoize expensive derivations.
- **Trade-offs:**
  - *Coupling:* Zero architectural disruption; preserves all established module boundaries and contracts.
  - *Testability:* High; immediately verifiable using existing Vitest suites and Cargo unit tests with minimal targeted test adjustments.
  - *Migration Cost:* Very low (<1 day per package); work packages are completely disjoint.
  - *Failure Modes:* Isolated to individual files; low regression risk.

### Architectural Recommendation
**Adopt Design Option 2 (Targeted Seam Hardening).**
Option 2 addresses all critical bottlenecks (memory growth, lock contention, startup hangs, render churn) while adhering to the principle of simplest sufficient architecture. Rejecting massive cross-cutting redesigns prevents regressions and keeps implementation nodes completely disjoint.

---

## 3. Deduplicated & Ranked Bottleneck Inventory

Findings from all six audits have been cross-checked, verified against actual workspace source code, deduplicated, and ranked by **Impact**, **Confidence**, and **Risk**.

```
Rank Scale:
- Impact:     CRITICAL (Process hang / OOM / Severe UI stutter) > HIGH (Measurable lag / Churn) > MEDIUM (Overhead)
- Confidence: HIGH (Code-evidenced & reproducible) > MEDIUM (Heuristic / Config)
- Risk:       LOW (Isolated fix) > MEDIUM (Requires careful boundary testing)
```

| ID | Origin Audit | File : Line | Bottleneck Summary | Impact | Confidence | Risk |
|:---|:---|:---|:---|:---:|:---:|:---:|
| **B-01** | Build | `src-tauri/tauri.conf.json:71` | Duplicate UI asset bundle packaging | HIGH | HIGH | LOW |
| **B-02** | Build | `src-tauri/Cargo.toml:1` | Missing `[profile.release]` optimizations | HIGH | HIGH | LOW |
| **B-03** | Build | `ui/vite.config.ts:21` | High-frequency 100ms dev server polling | MEDIUM | HIGH | LOW |
| **B-04** | Build | `ui/tsconfig.json:2` | Non-incremental TypeScript compilation | MEDIUM | HIGH | LOW |
| **RC-01** | Rust Core | `src-tauri/src/daemon/client.rs:411-413` | Daemon handshake read lacks timeout | CRITICAL | HIGH | LOW |
| **RC-02** | Rust Core | `src-tauri/src/daemon/client.rs:340-363` | Spawned daemon stderr pipe not drained | HIGH | HIGH | LOW |
| **RC-03** | Rust Core | `src-tauri/src/daemon/client.rs:317-320` | Blocking `std::fs::symlink_metadata` in async connect | MEDIUM | HIGH | LOW |
| **RM-01** | Remote | `src-tauri/src/remote/server.rs:998-1011` | Blocking mirror mutex held across JSON serialization | HIGH | HIGH | LOW |
| **RM-02** | Remote | `src-tauri/src/remote/server.rs:1050-1056` | Unbounded queue on grid WebSocket backpressure | CRITICAL | HIGH | LOW |
| **RM-03** | Remote | `src-tauri/src/remote/server.rs:1062-1100` | Grid batching has time limit but no byte ceiling | HIGH | HIGH | LOW |
| **NT-01** | Native Term | `src-tauri/src/native_terminal/surface_host.rs:674-743` | `consume_render` called before presentation completes | CRITICAL | HIGH | MEDIUM |
| **NT-02** | Native Term | `src-tauri/src/native_terminal/surface_host.rs:233-289` | Full terminal snapshot & text scan on every output chunk | CRITICAL | HIGH | MEDIUM |
| **NT-03** | Native Term | `src-tauri/src/ipc/native_terminal.rs:624-680` | Pointer motion synchronously awaits full render receipt | HIGH | HIGH | LOW |
| **RS-01** | React State | `ui/src/App.tsx:780-796` | Persistence effect re-arms on every terminal output tick | HIGH | HIGH | LOW |
| **RS-02** | React State | `ui/src/components/TerminalSplitView.tsx:210-240` | Inline unmemoized split helpers and per-move allocations | HIGH | HIGH | LOW |
| **RS-03** | React State | `ui/src/components/Sidebar.tsx:90-135` | Sidebar drag resize updates React state on every mousemove | MEDIUM | HIGH | LOW |
| **RS-04** | React State | `ui/src/components/TabBar.tsx:75-180` | Inline callbacks passed to `SortableTab` defeat `React.memo` | MEDIUM | HIGH | LOW |
| **TU-01** | Terminal UI | `ui/src/lib/terminalEvents.ts:106-149` | Full-backlog concatenation and copy on subscription | HIGH | HIGH | LOW |
| **TU-02** | Terminal UI | `ui/src/components/NativeTerminalPane.tsx:759-790` | Resize bounds success immediately cascades into scrollbar IPC | MEDIUM | HIGH | LOW |

---

## 4. Speculative & Broad Findings Rejected from Execution Scope

To maintain execution reliability and prevent non-overlapping conflicts, the following proposals from individual audits are explicitly **deferred / rejected** for this run:

1. **Full Async Protocol Multiplexing in DaemonClient (`audit-rust-core.md:RC-01`)**:
   *Reason:* Requires breaking changes to the daemon wire format and multi-channel response routing. Single-connection timeout hardening (RC-01/RC-02) provides immediate safety with zero wire blast radius.
2. **Whole Store Slicing / Zustand Migration (`audit-react-state.md:RS-01`)**:
   *Reason:* Rewriting `useWorkspaceStore` into fine-grained Zustand slices breaks ~40 component contracts. Optimizing persistence triggers, memoizing callback references, and isolating drag state achieves 90% of the render reduction with 5% of the risk.
3. **New Global Singletons (`terminalOutputScheduler`, `terminalHostManager`) (`audit-terminal-ui.md`)**:
   *Reason:* Introducing new global manager architectures creates sprawling file touches across the UI layer. Targeted fixes inside `terminalEvents.ts` and `NativeTerminalPane.tsx` eliminate copy amplification and cascade calls directly.
4. **C-ABI Bulk POD Cell Extraction in Libghostty (`audit-native-terminal.md:NT-PERF-05`)**:
   *Reason:* Modifying the Zig C-ABI export in `libghostty-vt.a` requires Zig toolchain rebuilds and cross-platform binary distribution changes. Render state reuse and coordinator coalescing solve the frame-rate issue without FFI modification.
