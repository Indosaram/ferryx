# Orca 1.4.259 - binary teardown (todo 16)

Route: OSS_REPO + PUBLIC_BINARY. Scope: repo match + divergences; axes in `orca.md` (todo 15).
Artifact root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/orca/`. App never launched.

## Acquisition
- URL: https://github.com/stablyai/orca/releases/download/v1.4.259/orca-macos-arm64.dmg (283966513 bytes)
- sha256: `4118e9e4ec0e78349570f5e35bb0f1a6a6f8b09d7e1122348b9536805d436fb1` (re-hash OK)
- Copied: `orca-macos-arm64/Orca.app` (726M, the largest of the set). Metadata: `orca-macos-arm64/meta.txt`

## Signing
- Developer ID Application: Lovecast LLC, **Team 6CX3WHS9HZ**; notarized, stapled; signed 2026-11-15; spctl accepted
- Entitlements are the broadest of the set: apple-events automation, allow-jit, unsigned-exec-memory, audio-input, bluetooth, camera, usb, addressbook, calendars, location

## Info.plist
- `com.stablyai.orca`, 1.4.259, LSMinimumSystemVersion 13.4; URL scheme `orca`

## Framework verdict: **Electron 43.8.7**
- Frameworks: Electron Framework 43.8.7 + Squirrel stack. `Contents/MacOS` also holds the native helpers `orca-keyboard-layout`, `orca-notification-status`, `orca-process-footprint`
- Resources: nested `Orca Computer Use.app`, `agent-browser-darwin-arm64`, `orcad-template/` (daemon worker entries, e.g. `claude-conversation-fork-worker-entry.js`, `computer-sidecar.js`), `relay/wsl/`, `ripgrep`, `skills/`, `orca_shipit_scheduling.node`
- asar extracted -> `orca-macos-arm64/asar/` (5752 files); package orca 1.4.259, main `./out/main/index.js`; deps include @anthropic-ai/claude-agent-sdk, node-pty, @xterm/headless, ssh2, posthog-node, @linear/sdk, agent-browser

## Repo match (src clone 684d3627, committed 2026-11-15)
- Version: repo `package.json` says 1.4.214 while the shipped build says 1.4.259 (release published 2026-11-14 from main). DIVERGENCE: the repo version field is not what CI ships, so the release version is stamped at build time
- Electron: repo 43.8.7 = shipped 43.8.7. MATCH

## Axis findings (asar-path:line, relative to `orca-macos-arm64/asar/`)
- (d) terminal: dynamic `import("node-pty")` with load-failure listeners (`out/main/index.js:208`); reads the user's Ghostty config paths (`com.mitchellh.ghostty`, `config.ghostty`) to import terminal settings (`out/main/index.js:1835`)
- (c) worktree: `[..., "worktree", "add", ...]` including `--detach --no-checkout` (`out/main/index.js:447-448`)
- (f) push: device registration `POST /v1/devices` carries `token` and an optional `apnsEnvironment` (`out/main/index.js:4607`); a mobile pairing QR flow (`out/main/index.js:225`)
- (j) endpoints: `https://login.onorca.dev` (`out/main/index.js:228`), `https://connector.onorca.dev` (`out/main/index.js:3188`)

## Keyword census (`census.txt`)
pty 16258, worktree 35769, ghostty 128, xterm 794, apns 13, websocket 252, claude 6833, codex 6407, gemini 662, mcp 690, acp 270

## Headline
Electron 43.8.7 build matching the repo's Electron pin. The repo's package.json version (1.4.214) lags the shipped 1.4.259. The bundle is the most expansive of the set: a computer-use app, an agent browser, a daemon template, WSL relays, APNs device registration, and broad TCC entitlements.
