# Zeron 0.3.6 - binary teardown (todo 16)

Route: OSS_REPO + PUBLIC_BINARY. Scope: repo match + divergences; axes in `zeron.md` (todo 15).
Artifact root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/zeron/`. App never launched.

## Acquisition
- URL: https://github.com/zeronsh/zeron/releases/download/v0.3.6/zeron-0.3.6-macos-arm64.dmg (93783112 bytes)
- sha256: `6878a03efad2f82565fd754a2e1ad49a4c7556dea550b02d6be4b8931b83cb6e` (re-hash OK)
- Copied: `zeron-0.3.6-macos-arm64/Zeron.app` (193M). Metadata: `zeron-0.3.6-macos-arm64/meta.txt`

## Signing
- Developer ID Application: 1499487 B.C. Ltd., **Team 5XY3M483YQ**; notarized, stapled; signed 2026-11-15; spctl accepted
- Second executable `Contents/MacOS/Zeron Engine` (68M), id `sh.zeron.app.engine`, same team
- Entitlements: audio-input, files.downloads.read-write, files.user-selected.read-write (no JIT; the only app here without allow-jit)

## Info.plist
- `sh.zeron.app`, 0.3.6, LSMinimumSystemVersion 13.0; URL scheme `zeron`

## Framework verdict: **native Rust (zgx: wgpu/Metal + Luau UI scripts, Ghostty terminal); not Electron, not Tauri**
- No `Contents/Frameworks`; `otool -L zeron`: system frameworks only (AppKit, Metal, QuartzCore, WebKit, CoreML, AVFoundation, ...) + libc++
- Rust crate census from `strings -a` (`zeron-0.3.6-macos-arm64/crates.txt`, 201 entries): wgpu/naga, winit-style accesskit_winit, objc2-* incl. objc2-web-kit, mlua (Luau), tokio, tungstenite, rusqlite, ort + parakeet-rs (local speech-to-text), cpal; the git dep `zgx` at rev `d391ea7`
- Ghostty is statically linked into the UI binary: 43 `_ghostty_*` symbols in `nm` (e.g. `_ghostty_app_new`), plus strings `zgx: ghostty_init failed` and `xterm-ghostty` terminfo
- UI ships as Luau scripts on disk: `Contents/Resources/ui/` (1457 files, 22M) and `plugins-builtin/{git,github,parallax-scenes}`
- Engine (`Zeron Engine`) crates: portable-pty 0.8.1, alacritty_terminal 0.26.0, vte 0.15.0, git2 0.21.0, loro 1.17.1 (CRDT sync), tungstenite 0.24.0

## Repo match (src clone 68e460a)
- Version: workspace `Cargo.toml` 0.3.6 = shipped 0.3.6. MATCH
- **Framework divergence vs repo docs:** `ARCHITECTURE.md` says the frontend is **gpui** (pinned Zed rev), and `release.yml:171` runs `scripts/package-macos.sh` (`cargo build -p zeron`, which has no Resources/ui). The shipped bundle instead has the layout of `scripts/package-zgx-macos.sh` (Resources `ui`, `plugins-builtin`, `zeron.icns`, `licenses`; invoked by `.github/workflows/zgx-build.yml:107`): 224 `zgx` string hits and only 3 `gpui` hits in the UI binary. The 0.3.6 DMG was therefore built from the zgx interface (`apps/zgx`, crate `zeron-zgx`, Ghostty feature per `apps/zgx/Cargo.toml:161-165`), not the gpui app the architecture doc describes

## Axis findings (artifact paths relative to `zeron-0.3.6-macos-arm64/`)
- (d) terminal: Ghostty embedded in the UI (`Zeron.app/Contents/MacOS/zeron`, `_ghostty_*` symbols); PTY and VT state in the engine via portable-pty + alacritty_terminal (`Zeron.app/Contents/MacOS/Zeron Engine` strings)
- (b) agents: engine strings include `claude-acp`, `codex-acp`, and `codex app-server` (`Zeron Engine`); engine census acp 139, claude 361, codex 245
- (f) remote/push: Loro CRDT sync + websocket (tungstenite) in the engine; apns 0 in both binaries (the 2 Luau "apns" hits are base64 noise in `ui/assets/icons/**/folder-redis.svg`)
- (a) local ML: ort + parakeet-rs, with license `Resources/licenses/parakeet-v3.txt` (on-device voice transcription)

## Keyword census (`census.txt`)
UI binary: pty 586, worktree 3, ghostty 63, xterm 9, apns 0, websocket 51, claude 10, codex 10, gemini 6, mcp 32, acp 29. Luau UI: worktree 194, claude 368, codex 294, acp 150. Engine: pty 475, worktree 73, claude 361, codex 245, mcp 151, acp 139

## Headline
A fully native Rust app built as two binaries (UI + background engine) with zero Chromium. The terminal is Ghostty embedded via zgx, and the UI is hot-reloadable Luau. The shipped 0.3.6 is the **zgx** build, not the gpui frontend that the repo's ARCHITECTURE.md and release.yml describe.
