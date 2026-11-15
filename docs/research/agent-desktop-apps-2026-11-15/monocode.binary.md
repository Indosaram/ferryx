# MonoCode 0.16.0 (+ Monocode Native 0.16.0-preview.14) - binary teardown (todo 16)

Route: OSS_REPO + PUBLIC_BINARY. Two public DMGs, both recorded. Scope: repo match + divergences; axes in `monocode.md` (todo 15).
Artifact root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/bin/monocode/`. Neither app launched.

## Acquisition
| DMG | bytes | sha256 |
|---|---|---|
| `MonoCode_0.16.0_aarch64.dmg` (GitHub release v0.16.0) | 18089912 | `914e671841889c92804e8fafa45c7717ab4b5889a66e525a4fcede9fbb877cf3` |
| `MonocodeNative_0.16.0-preview.14_aarch64.dmg` (prerelease native-v0.16.0-preview.14; the site's download button) | 32231282 | `99bb352afe1e4690a760b64966eddc45ce5f5e5cdda8d616049caf8ca200e856` |
Both re-hash OK. Copied: `MonoCode_0.16.0_aarch64/MonoCode.app` (41M), `MonocodeNative_0.16.0-preview.14_aarch64/Monocode Native.app` (85M). Metadata: `*/meta.txt`

## Signing (both)
- Developer ID Application: Nikolay Petkov, **Team JY692ZSF7B**; notarized, stapled; spctl accepted. MonoCode signed 2026-10-19, Native 2026-11-05
- Entitlements (both): allow-jit, allow-unsigned-executable-memory only

## Info.plist
- MonoCode: `com.monocode.desktop`, 0.16.0, LSMinimumSystemVersion 10.13, executable `monocode`, no URL scheme
- Native: `dev.usemono.gpui`, 0.16.0, no LSMinimumSystemVersion, NSPrincipalClass NSApplication, executable `monocode-gpui`

## Framework verdicts
- **MonoCode 0.16.0 = Tauri 2 (wry 0.55.1 / tao 0.35.3, system WKWebView).** No Frameworks dir; `otool -L` system-only incl. WebKit. Crates (`MonoCode_0.16.0_aarch64/crates.txt`): tauri, tauri-runtime-wry, tauri-plugin-{dialog,global-shortcut,opener,updater,window-state}, rusqlite, quinn (QUIC), rcgen, keyring, igd-next/natpmp (port mapping). The frontend is embedded in the binary (`__TAURI_INTERNALS__` x33)
- **Monocode Native = native Rust on gpui** (Zed's UI framework, vendored as `gpui-pre`; 146 `gpui-pre` hits) with Metal; crates (`MonocodeNative_0.16.0-preview.14_aarch64/crates.txt`): gpui-pre-*, alacritty_terminal, vte, rustix-openpty, tree-sitter, syntect, rquickjs-core, quinn, rusqlite, zed-font-kit, taffy. No Tauri runtime (the "tauri" strings are ignore patterns and file names)

## Repo match (src clone 09d0443)
- Version: workspace `Cargo.toml` 0.16.0 and `src-tauri/tauri.conf.json` 0.16.0 = both shipped builds 0.16.0. MATCH
- Framework: Tauri app = `src-tauri/` (identifier `com.monocode.desktop`, `tauri.conf.json:5`). MATCH. Native app = `crates/monocode-gpui` + `vendor/gpui-pre-apple`, bundled by `scripts/bundle-native-macos.sh` with `CFBundleIdentifier dev.usemono.gpui` (`scripts/bundle-native-macos.sh:63`). MATCH
- Divergence: the website ships the preview-only gpui Native line while GitHub "latest" is the Tauri line; both carry version 0.16.0 under different bundle ids, so they install side by side

## Axis findings (artifact paths relative to `bin/monocode/`)
- (d) terminal: Tauri build has its own Rust PTY host, with strings `monocode_lib::pty::PtyHost`, `src-tauri/src/pty.rs`, and `pty-data`/`pty-exit` events (`MonoCode_0.16.0_aarch64/MonoCode.app/Contents/MacOS/monocode`); the frontend uses @xterm/xterm 6 per repo `package.json:68-70`. Native build uses alacritty_terminal + rustix-openpty (`Monocode Native.app/Contents/MacOS/monocode-gpui`)
- (c) worktree: SQLite schema strings `worktree_cwd`, `worktreeRemoved`, `worktree_removed` in the Tauri binary
- (f) remote/mobile: `monocode-mobile`, `monocode-mobile-enroll`, `monocode-mobile-watch` strings + quinn/QUIC + rcgen self-signed certs + UPnP/NAT-PMP in both binaries, i.e. a direct P2P mobile link; apns 0 in both

## Keyword census (`*/census.txt`, `strings -a` of main executable)
| kw | MonoCode (Tauri) | Native (gpui) |
|---|---|---|
| pty | 131 | 715 |
| worktree | 107 | 189 |
| ghostty | 0 | 0 |
| xterm | 1 | 42 |
| apns | 0 | 0 |
| websocket | 31 | 5 |
| claude | 63 | 229 |
| codex | 77 | 190 |
| gemini | 2 | 44 |
| mcp | 32 | 101 |
| acp | 13 | 28 |

## Headline
Two parallel products at 0.16.0 from one repo, both signed by team JY692ZSF7B. The GitHub release is a 41M Tauri/WKWebView app with xterm.js. The site pushes an 85M native gpui rewrite with alacritty_terminal. Both carry a QUIC + UPnP direct mobile link and no APNs.
