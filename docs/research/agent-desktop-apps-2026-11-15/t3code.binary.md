# T3 Code 0.1.9 - binary teardown (todo 16)

Route: OSS_REPO + PUBLIC_BINARY. Scope: check the shipped build matches the repo (version, framework) and note divergences; axis analysis lives in `t3code.md` (todo 15).
Artifact root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/t3code/`. App never launched.

## Acquisition
- URL: https://github.com/pingdotgg/t3code/releases/download/v0.1.9/T3-Code-0.1.9-arm64.dmg (195374334 bytes)
- sha256: `b0a15cb7aac2461425f670b14ac52d7a207a0e2008250882c850b4cd62aa926a` (re-hash OK)
- Copied: `T3-Code-0.1.9-arm64/T3 Code (Beta).app` (481M). Metadata: `T3-Code-0.1.9-arm64/meta.txt`

## Signing
- Developer ID Application: T3 Tools, Inc., **Team ARK85ZXQ4Z**; notarized, stapled; signed 2026-11-11; spctl accepted (Notarized Developer ID)
- Entitlements: allow-jit, allow-unsigned-executable-memory, disable-library-validation, audio-input, camera, associated-domains `webcredentials:clerk.t3.codes`, keychain group `ARK85ZXQ4Z.com.t3tools.t3code.webauthn` (passkeys via Clerk)

## Info.plist
- `com.t3tools.t3code`, 0.1.9, LSMinimumSystemVersion 13.0; URL schemes `t3code`, `t3code-dev` (also registers http/https handler)

## Framework verdict: **Electron 44.10.1**
- Frameworks: Electron Framework 44.10.1, Squirrel/Mantle/ReactiveObjC, 4 helpers. `otool -L` main binary: Electron Framework + libSystem only (run via a /tmp symlink because the Xcode otool wrapper splits paths containing spaces; noted in meta.txt)
- asar extracted -> `T3-Code-0.1.9-arm64/asar/` (2841 files). `asar/package.json`: t3code 0.1.9, main `apps/desktop/dist-electron/boot.cjs`; deps include node-pty, @cursor/sdk, @clerk/electron-passkeys, playwright-core, transcribe-cpp, @napi-rs/keyring

## Repo match (src clone 256dce7f)
- Version: repo `apps/desktop/package.json` 0.1.9 = shipped 0.1.9. MATCH
- Electron: repo pins 44.10.1 = shipped 44.10.1. MATCH
- Divergence: none found for version/framework. The asar bundles the server (`apps/server/dist`) and web client inside the desktop app.

## Axis findings (asar-path:line, relative to `T3-Code-0.1.9-arm64/asar/`)
- (d) terminal: server loads node-pty lazily (`apps/server/dist/server-bi3w5spO.mjs:26562`); renderer uses **libghostty-vt compiled to WASM** (`apps/server/dist/client/assets/ghostty-vt-DdA0Zryv.wasm`, `ghostty-write-pty-hgyAvVbe.wasm`, referenced at `apps/server/dist/client/assets/ThreadTerminalDrawer-uSTjWNKI.js:1`).
- (f) push: a relay contract with APNs result fields and iOS Live Activity delivery kinds `live_activity_start|update|end` and `push_notification` (`apps/desktop/dist-electron/main.cjs:32517-32530`).
- (c) worktree: managed worktree creation falls back to `git worktree add` (`apps/server/dist/ManagedEndpointRuntime-B-62Mn5X.mjs:137601`, `137690`).

## Keyword census (`census.txt`, non-node_modules JS)
pty 8145, worktree 2956, ghostty 198, xterm 49, apns 21, websocket 1027, claude 3963, codex 2260, gemini 182, mcp 5536, acp 3697

## Headline
Shipped build matches the repo exactly (0.1.9, Electron 44.10.1). The notable binary-only facts are the Clerk passkey and webauthn keychain entitlements, and that the terminal renders on a Ghostty VT WASM core.
