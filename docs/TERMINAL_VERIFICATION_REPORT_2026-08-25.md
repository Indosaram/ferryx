# Terminal Verification Report — 2026-08-25

## Outcome

The native desktop terminal keeps Ghostty-derived theme and cursor-style preferences, coalesces burst output into one pending main-thread render, and remains the only desktop terminal surface. The dependency-free remote terminal correctly removes Ferryx OSC 777 metadata, handles fragmented WebSocket envelopes, and encodes editing and modified-navigation input. No xterm implementation or package references remain in the scanned frontend or lockfiles.

## Repairs validated

- `src-tauri/src/native_terminal/surface_host.rs` constructs `RendererTheme` from complete `TerminalPreferences`, retaining imported `cursor_style` as well as theme colors.
- Native output scheduling is guarded by `RenderScheduleCoordinator`: a burst schedules once; consuming the pending render permits a trailing render.
- `ui/src/remote/RemoteTerminal.tsx` retains a streaming UTF-8 decoder, buffers fragmented OSC 777 metadata, sends only payload to the terminal surface, maps Backspace/Delete, and preserves Ctrl/Alt navigation modifiers.
- Mobile dock `Ctrl`/`Alt` navigation actions now use the same standard CSI modifier sequences as physical keyboard navigation; a failing regression test was captured before the repair.
- `ui/src/components/MobileKeyDock.tsx` clears both modifier latches after a sent action and respects the mobile bottom safe area.

## Automated evidence

| Check | Result |
| --- | --- |
| Remote/mobile contracts | `bun run --cwd ui test src/remote/RemoteTerminal.contract.test.tsx src/remote/RemoteUI.test.tsx`: 12 passed |
| Split/tab/native-pane contracts | `bun run --cwd ui test src/components/TerminalSplitView.test.tsx src/components/TerminalPane.test.tsx src/components/TabBar.test.tsx`: 35 passed |
| Full UI suite | `bun run --cwd ui test`: 78 files, 620 tests passed |
| Frontend production build | `bun run --cwd ui build`: passed |
| UI diagnostics | `lsp_diagnostics ui/src`: 0 errors |
| Native renderer contract | `ZIG="$(command -v zig)" cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract`: 19 passed |
| Native surface-host contract | `ZIG="$(command -v zig)" cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract`: 10 passed |
| Ghostty import integration | `ZIG="$(command -v zig)" cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract`: 13 passed |
| Rust typecheck | `cargo check --manifest-path src-tauri/Cargo.toml`: passed (4 pre-existing dead-code warnings) |
| xterm-free audit | `git grep -in -E '(@xterm|xterm\\.js|xterm|terminalHostManager|terminalInstanceFactory|terminalOutputScheduler|terminalRenderer|applyTerminalSettings)' -- ui/src ui/package.json ui/bun.lock site/package.json site/bun.lock`: zero matches (exit 1 expected) |

## Performance profile

`bun ui/src/lib/terminalThroughput.bench.ts` processed 10 MiB in 320 × 32 KiB chunks:

| Path | Median total | Throughput |
| --- | ---: | ---: |
| `Uint8Array` pass-through | 0.026 ms | 384,615.4 MiB/s |
| legacy base64 decode | 88.098 ms | 113.5 MiB/s |

Checksums matched. The active binary pass-through path is not a frontend transport bottleneck; the removed base64 path is about 3,400× slower in this deterministic microbenchmark.

## Manual QA — desktop and remote/mobile

Desktop interaction is intentionally user-run; no desktop automation was performed.

1. Launch the rebuilt Ferryx desktop app and open a terminal in a workspace.
2. Confirm the imported Ghostty font family, text size, foreground/background colors, and cursor shape match Ghostty; resize the window through several widths and heights. PASS: text remains sharp, the terminal fills its viewport without top/bottom padding, and no residual pixels use a different background.
3. Split the terminal right, then split one child downward. Send distinct commands in all panes, switch pane focus, and resize each divider. PASS: each pane keeps its own session/output; switching focus never swaps content or input targets.
4. Create at least two terminal tabs, switch repeatedly, and return to the split tab. PASS: the selected tab retains its previous pane tree and the active pane receives keyboard input.
5. From a remote/mobile client, connect to the terminal, use arrows, Ctrl+Arrow, Backspace, Delete, paste, Ctrl-C, and the mobile Ctrl/Alt dock controls. PASS: command-line editing/navigation behave normally; no `OSC 777` metadata appears in terminal output; modifier state does not stick after a key is sent.

## Fresh completion audit

The current live tree was re-audited after the original report. A newly found gap in mobile-dock `Ctrl`/`Alt` navigation was proven RED (`ctrl-left` was sent literally), repaired at the shared remote key boundary, then verified GREEN. The full UI suite, production build, split/tab contracts, remote/mobile contracts, native renderer and surface-host contracts, Ghostty import integration, Rust typecheck, LSP diagnostics, throughput profile, and zero-xterm scan were rerun against this live tree.

## Known validation limitation

Visual desktop confirmation requires the user because direct desktop automation is prohibited. The automated native renderer, surface-host, split/tab, remote/mobile, build, and diagnostic checks above are clean.
