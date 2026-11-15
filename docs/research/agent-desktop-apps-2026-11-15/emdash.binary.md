# Emdash 1.2.11 - binary teardown (todo 16)

Route: OSS_REPO + PUBLIC_BINARY. Scope: repo match + divergences; axes in `emdash.md` (todo 15).
Artifact root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/emdash/`. App never launched.

## Acquisition
- URL: https://github.com/generalaction/emdash/releases/download/v1.2.11/emdash-arm64.dmg (235953859 bytes)
- sha256: `6f36052874fef79a34c802b9c0f34a3fe7f379747f5c89093d6f7b1cd1916b25` (re-hash OK)
- Copied: `emdash-arm64/Emdash.app` (838M). Metadata: `emdash-arm64/meta.txt`

## Signing
- Developer ID Application: Arne Strickmann, **Team 2AZ6648627**; signed 2026-11-06; spctl accepted, source=Notarized Developer ID. **No stapled notarization ticket** (codesign shows no `Notarization Ticket=stapled` line, unlike the other 6), so Gatekeeper validates notarization online
- Entitlements: app-sandbox false, allow-jit, unsigned-exec-memory, disable-library-validation, audio-input, get-task-allow false

## Info.plist
- `com.emdash.stable`, 1.2.11, LSMinimumSystemVersion 12.0; no URL scheme

## Framework verdict: **Electron 40.10.2**
- Frameworks: Electron Framework 40.10.2 + Squirrel stack; `otool -L`: Electron Framework + libSystem
- asar extracted -> `emdash-arm64/asar/` (41137 files, 645M; the asar ships a full node_modules including typescript, pyright, typescript-language-server); package `@emdash/emdash-desktop` 1.2.11, main `./out/main/index.js`; deps include @agentclientprotocol/sdk, node-pty, better-sqlite3, drizzle-orm, ssh2, @octokit/rest, @gitbeaker/rest, forgejo, @linear/sdk

## Repo match (src clone ccc6cce)
- Version: `apps/emdash-desktop/package.json` 1.2.11 = shipped 1.2.11. MATCH
- Electron: repo 40.10.2 = shipped 40.10.2. MATCH (oldest Electron of the five Electron apps)
- Divergence: none for version/framework; the missing stapled ticket is a release-process difference visible only in the binary

## Axis findings (asar-path:line, relative to `emdash-arm64/asar/`)
- (b) agents via ACP: `import { ClientSideConnection, ndJsonStream } from "@agentclientprotocol/sdk"` (`out/main/chunks/agents-8V6UmW1b.js:22`); bundled adapters `out/main/adapters/claude-acp.mjs`, `out/main/adapters/codex-acp.cjs`
- (d) terminal: `import "node-pty"` (`out/main/scripts-runtime.js:36`, `out/main/agent-config-runtime.js:19`)
- (c) worktree: `exec.exec(["worktree", "add", execution.worktreePath, execution.branch])` (`out/main/workspace-registry-runtime.js:3159`)
- (f) push: apns 0 in the census; no mobile push surface in the shipped JS

## Keyword census (`census.txt`)
pty 9081, worktree 1608, ghostty 38, xterm 163, apns 0, websocket 202, claude 3121, codex 584, gemini 24, mcp 1951, acp 1641

## Headline
Exact repo match (1.2.11, Electron 40.10.2). It is ACP-first, with bundled claude/codex ACP adapters. It is the only app of the 7 whose notarization ticket is not stapled.
