# Paseo 0.15.4 - binary teardown (todo 16)

Route: OSS_REPO + PUBLIC_BINARY. Scope: repo match + divergences; axes in `paseo.md` (todo 15).
Artifact root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/paseo/`. App never launched.

## Acquisition
- URL: https://github.com/getpaseo/paseo/releases/download/v0.15.4/Paseo-0.15.4-arm64.dmg (186129307 bytes)
- sha256: `b689017a0137480e6ba8231d97016b94570e8dfc6d0e23c4d833867765cfbcc1` (re-hash OK)
- Copied: `Paseo-0.15.4-arm64/Paseo.app` (520M). Metadata: `Paseo-0.15.4-arm64/meta.txt`

## Signing
- Developer ID Application: Mohamed Boudra Ziani, **Team 99ZMJMKU9Y**; notarized, stapled; signed 2026-11-10; spctl accepted
- Entitlements: allow-jit, allow-unsigned-executable-memory, audio-input (no disable-library-validation)

## Info.plist
- `sh.paseo.desktop`, 0.15.4, LSMinimumSystemVersion 13.0.0; URL scheme `paseo`

## Framework verdict: **Electron 44.2.0**
- Frameworks: Electron Framework 44.2.0 + Squirrel stack; `otool -L`: Electron Framework + libSystem
- Resources besides the asar: `app-dist/` (Expo web build of the shared mobile/web app), `builtin-plugins/` (usage sources for claude/codex/copilot/cursor/grok/kimi/minimax/opencode, providers antigravity/hoplite/muse, worktree-include), `bin/paseo` (shell-script CLI shim)
- asar extracted -> `Paseo-0.15.4-arm64/asar/` (9386 files); package `@getpaseo/desktop` 0.15.4, main `dist/main.js`; the daemon ships as `node_modules/@getpaseo/server` 0.15.4 and `@getpaseo/cli` 0.15.4

## Repo match (src clone 1c175b3)
- Version: clone HEAD is ahead of the release: `packages/desktop/package.json` 0.16.0-beta.2 vs shipped 0.15.4. DIVERGENCE BY TIME (release predates HEAD), not a build mismatch
- Electron: repo 44.2.0 = shipped 44.2.0. MATCH
- **Packaging defect in the shipped build:** `@electron/asar extract` fails with 11 ENOENT errors because the asar header marks files as unpacked that are missing from `app.asar.unpacked/`: the whole `@anthropic-ai/claude-agent-sdk-darwin-arm64/` (including its `claude` binary; the directory exists but is empty) and the non-darwin-arm64 node-pty prebuilds (log: `Paseo-0.15.4-arm64/asar-extract.log`). The darwin-x64 and other-platform prebuild gaps don't matter on arm64. The missing bundled `claude` binary means the SDK's bundled Claude CLI is not shipped, so the app must rely on a user-installed `claude`. This is inferred from the static layout and was not run.

## Axis findings (paths relative to `Paseo-0.15.4-arm64/asar/node_modules/@getpaseo/server/dist/server/`)
- (b) agents: provider adapters for ACP (generic, copilot, cursor, kimi, kiro, trae), codex app-server, claude, opencode, omp, pi (`server/agent/providers/` listing)
- (d) terminal: `import * as pty from "node-pty"` (`terminal/terminal-worker-process.js:1`)
- (c) worktree: `runGitCommand(["worktree", "add", ...])` (`utils/worktree.js:979`)
- (f) remote/push: default relay `relay.paseo.sh:443` with TLS (`server/bootstrap.js:1338-1340`); mobile push through Expo `https://exp.host/--/api/v2/push/send` (`server/push/push-service.js:1`), so APNs is handled by Expo, not directly

## Keyword census (`census.txt`)
Desktop shell JS only: pty 8, worktree 16, websocket 10, others 0. Daemon (`@getpaseo/**`): pty 972, worktree 2942, xterm 50, apns 0, websocket 395, claude 1040, codex 944, gemini 3, mcp 1193, acp 699

## Headline
Electron 44.2.0 shell over the same Node daemon as the repo, with relay.paseo.sh and Expo push. The shipped 0.15.4 asar declares an unpacked `claude-agent-sdk-darwin-arm64` (bundled `claude` binary) that is missing from the bundle.
