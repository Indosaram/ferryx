# Capy 0.8.0 - binary teardown (todo 16)

Route: PUBLIC_BINARY only (no OSS repo; see acquisition.json). The shipped binary is the only code evidence.
Root for all artifact paths: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/capy/`
App never launched. No Gatekeeper override, no install, no signup.

## Acquisition
- URL: https://downloads.capy.ai/stable/Capy-0.8.0-arm64.dmg (166876347 bytes, matches acquisition.json)
- sha256: `b08092f0f0eee7315eb5c0a248610ace6df6e527ad0aa10d4e01ca4ef8ade5e9` (re-hash OK, task-16-teardown.log)
- Copied out: `Capy-0.8.0-arm64/Capy.app` (351M). Raw metadata: `Capy-0.8.0-arm64/meta.txt`

## Signing
- Developer ID Application: Yingting Sun, **Team 9YWUN5FK23**; hardened runtime; notarization ticket stapled; signed 2026-11-11
- `spctl -a -vv`: accepted, source=Notarized Developer ID
- Entitlements: allow-jit, allow-unsigned-executable-memory, disable-library-validation, device.audio-input, associated-domains `webcredentials:capy.ai`
- Bundled helper `Contents/Resources/kappu-macos/kappu-macos` (universal x86_64+arm64, 5.4M) is signed by the same team

## Info.plist
- Bundle id `ai.capy.desktop`, version 0.8.0, LSMinimumSystemVersion 13.0, NSPrincipalClass AtomApplication
- URL scheme: `capy`
- Usage strings: microphone (voice messages), local network ("lets agents reach devices on your local network"), Desktop/Documents/Downloads/removable/network volumes ("when an agent transfers them to or from this Mac"), camera, Bluetooth, audio capture

## Framework verdict: **Electron 45.2.3**
- `Contents/Frameworks`: Electron Framework.framework (CFBundleVersion 45.2.3), Squirrel/Mantle/ReactiveObjC, 4 Helper apps
- `otool -L Contents/MacOS/Capy`: only `@rpath/Electron Framework.framework` + libSystem
- `app.asar` extracted with `bunx --bun @electron/asar extract` -> `Capy-0.8.0-arm64/asar/` (7 files, 3.6M). `asar/package.json`: `@capy/desktop` 0.8.0, "Capy runs your coding agents in the cloud.", author Scrapybara, Inc., main `out/main/main.js` (75555 lines)
- Native addons outside the asar (`Contents/Resources/node_modules`): `@lydell/node-pty` (+ darwin-arm64 prebuild), sharp, tree-sitter-bash, web-tree-sitter; `notification-access/notification-access.node`
- Renderer is a separate on-disk web build: `Contents/Resources/renderer/` (loaded from `process.resourcesPath/renderer`, `asar/out/main/main.js:73611`)

## Axis findings (cite `asar-path:line`; asar paths relative to `Capy-0.8.0-arm64/asar/`)
- (a) stack: Electron main in ESM with Effect-style generators; uses `node:sqlite` DatabaseSync for local state (`out/main/main.js:19`). Production endpoints hardcoded: `https://api.capy.ai`, `wss://realtime-production.cloud.capy.ai`, web origin `https://capy.ai` (`out/main/main.js:361-368`).
- (b) agent integration: **cloud-first**. Agents run in Capy's cloud VMs; the desktop app is a device bridge. Main reads local Claude/Codex login state by running `codex login status` / `claude ...` (`out/main/main.js:25520`) and resolves `CLAUDE_CONFIG_DIR`/`~/.claude` and `CODEX_HOME`/`~/.codex` (`out/main/main.js:60933`). Cloud API has `connectClaudeSeat` / `refreshClaudeSeat` at `/models/credentials/claude/...` (`out/main/main.js:35168-35172`). Reads `AGENTS.md`/`CLAUDE.md`, `.agents/skills`, `.claude/skills`, `.cursor/rules/`, `.capy/rules/` (`out/main/main.js:52728`). No ACP string (census acp=0).
- (c) worktree: managed worktree base with `kind: "worktree"` vs `"linked"` workspaces (`out/main/main.js:32804`, `44471-44476`); refuses linking a folder inside the managed worktree base (`out/main/main.js:44312`); uses `git worktree list --porcelain -z` to detect a branch checked out elsewhere (`out/main/main.js:45522-45527`). Device worktrees exposed to the cloud API: `/orgs/:orgId/devices/:deviceId/worktrees` (`out/main/main.js:31479`).
- (d) terminal: PTY via `@lydell/node-pty` imported in `out/main/sessions-Dr1920xF.js:9`; PTY streams are multiplexed over the device tunnel as `kind: "pty"` lanes (`out/main/main.js:57355`, `57658`) with attach/resize/ack/kill frames (`out/main/main.js:57170-57183`). Renderer terminal is a custom DOM/WebGPU renderer ("wterm") over **Ghostty's VT parser compiled to WASM**: `renderer/assets/ghostty-vt-CQK3cHgQ.wasm` (588K), loaded from `renderer/assets/guests-BPnrRl1X.js:19`. No xterm.js.
- (f) remote access: **the desktop app is a remotely-driven device**. Hardcoded tunnel `wss://device-tunnel-production.cloud.capy.ai/tunnel` (`out/main/main.js:73985`); tunnel URL must be wss (or ws on loopback) with no credentials (`out/main/main.js:44729-44736`). Cloud API grants per-device agent access and **computer-use** access: `/orgs/:orgId/devices/:deviceId/agent-access/...` and `/computer-use/:accessId/grant|deny|revoke` (`out/main/main.js:31262-31327`). Computer use on macOS is the bundled Rust helper `kappu-macos` (`out/main/main.js:73980`; launch/restart logging `60152-60189`); its strings reference `crates/kappu-macos/src/macos/input.rs`, `SCScreenshotManager`, Accessibility (artifact: `Capy-0.8.0-arm64/Capy.app/Contents/Resources/kappu-macos/kappu-macos`). Linux VM side uses the same `kappu` over `/run/capy/kappu.sock` under Xvfb (`out/main/main.js:22452`, `24137-24153`). No APNs (census apns=0).
- (g) attention: Electron `Notification` for desktop alerts (`out/main/main.js:74347`, `74855`) plus a `notification-access.node` addon to check notification permission (`out/main/main.js:74861`).
- (j) telemetry/update: electron-updater generic provider `https://d1lfowv2t69uz0.cloudfront.net/stable/` (`Capy.app/Contents/Resources/app-update.yml`); Sentry appears as an automation trigger source, not app telemetry (`out/main/main.js:29017-29020`).
- (e)/(h)/(i): not established from the binary. "checkpoint" hits are undici HTTP internals (`out/main/main.js:11345`); review/diff and session persistence live server-side and were not proven from the shipped code.

## Keyword census (`census.txt`; non-node_modules JS in asar, case-insensitive)
pty 295, worktree 193, ghostty 1, xterm 4, apns 0, websocket 260, claude 33, codex 40, gemini 3, mcp 106, acp 0

## Headline
Capy is an Electron shell that turns your Mac into a **cloud-controlled device**: Capy's cloud agents reach in over a hardcoded wss device tunnel to run PTYs, use worktrees, and (with a per-device grant) do macOS computer-use through a bundled Rust helper (`kappu-macos`). The terminal renders on Ghostty's VT core compiled to WASM.
