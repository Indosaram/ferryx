# Browser engines

Ferryx browser tabs run on one of two engines. The system webview is the default and the fallback. The Chromium engine is an optional download that adds Chrome Web Store extensions and logins shared across tabs.

## The two engines

| | System webview | Chromium engine |
| --- | --- | --- |
| What it is | The OS webview, driven through wry | CEF 156.2.0 (Chrome style, via cef-rs), a separate process |
| macOS | WKWebView | Engine app in a frameless window placed over the pane |
| Windows | WebView2 | Engine as a child window of the Ferryx window |
| Linux | WebKitGTK | Not available yet |
| Ships with Ferryx | Yes | No, downloaded on demand |
| Process model | Inside the Ferryx process | One engine process per browser profile, shared by all tabs of that profile |

The system webview is a standing decision. See `OS_WEBVIEW_LIMITS.md` for what it can't do.

## Chromium engine install

Download it from Settings > Browser > Chromium Engine. The button reads Download, then Remove once the engine is installed.

| Item | Value |
| --- | --- |
| Engine version | `156.2.0-ferryx.1` (`ENGINE_VERSION` in `src-tauri/src/browser/engine_install.rs`) |
| Install dir | `<app data>/browser-engine/156.2.0-ferryx.1` |
| Profile data | `<app data>/browser-cef/<profile>` |
| Private tabs | In memory, no profile dir |
| Data root override | `FERRYX_DATA_DIR` replaces `<app data>` for the install dir and the profile dirs |
| Manifest override | `FERRYX_BROWSER_ENGINE_MANIFEST_URL` points a test or staging build at another manifest |

Once the engine is installed, new tabs use it. Tabs that are already open keep the engine they started with.

### Measured sizes

| OS | Download | On disk |
| --- | --- | --- |
| macOS | about 150 MB zip | about 332 MB |
| Windows | not recorded | about 459 MB, 240 files |

### Update model

- The app pins the engine version it was built for. A manifest for any other version is refused.
- The download is checked against the manifest `sha256` and `size`. A mismatch aborts the install.
- The archive is unpacked into a `.staging-<uuid>` directory and renamed into place, so a half-finished install is never picked up. An archive entry with an absolute path or `..` rejects the whole archive.
- A new Ferryx version that needs a new engine version asks for a new download.
- Remove deletes the install dir. Open Chromium tabs keep running until they are closed.
- On Windows the install also grants read access to the AppContainer SIDs. The engine sandbox needs it.
- On macOS the engine and its helper apps keep their executable bits from the zip.

## Capabilities per OS

"Yes" means it was checked by hand or by a gate in the 2026-11-01 plan status. "Not verified" means the sources don't say. "Not available" means the engine doesn't exist on that OS yet.

| Capability | macOS webview | macOS Chromium | Windows webview | Windows Chromium | Linux webview | Linux Chromium |
| --- | --- | --- | --- | --- | --- | --- |
| Page rendering | Yes | Yes | Yes | Yes | Yes | Not available |
| Chrome Web Store extensions and popups | No | Yes | No | Yes | No | Not available |
| Extension management | No | Yes, via chrome://extensions | No | Yes, via chrome://extensions | No | Not available |
| Shared logins and cookies across tabs of a profile | Not verified | Yes, one engine per profile | Not verified | Yes, one engine per profile | Not verified | Not available |
| Cookie persistence across restart | Not verified | Yes, checked by killing the engine | Not verified | Yes, checked by killing the engine | Not verified | Not available |
| Popups, target=_blank, window.open as Ferryx tabs | Not verified | Yes | Not verified | Yes | Not verified | Not available |
| DevTools | Not verified | Not verified | Not verified | Not verified | Not verified | Not available |
| Named profiles | No, macOS WebKit rejects them | Yes, `browser-cef/<profile>` | Yes | Yes, `browser-cef/<profile>` | Yes | Not available |
| Sandbox | Not verified | Not verified, engine is ad-hoc signed and needs a separately signed app | Not verified | Yes, after the AppContainer read grant | Not verified | Not available |

### Linux

The Linux Chromium engine isn't available. `platform_key()` returns `None` on Linux, so the status is `Unsupported`. There is also no window placement (X11 child or wl_subsurface) and no packaging. Linux uses WebKitGTK only.

### macOS signing

The macOS engine is ad-hoc signed today. A Developer ID signed engine app, with the JIT entitlement, is still to be packaged.

## Extensions

- Chrome Web Store installs work in Chromium tabs. The Chrome-style toolbar extension menu is drawn inside the pane.
- The extension list in Settings > Browser reads the profile's installed extensions from the profile directory on disk.
- Enable, disable, remove and options open Chrome's own `chrome://extensions` page as a Ferryx Chromium tab. CEF 156 has no `CefExtension` API (it was removed upstream with the Alloy runtime), so the Chrome-style page is the supported surface.
- System-webview tabs can't run Chrome extensions. They show an "Install engine" prompt that leads to Settings > Browser > Chromium Engine.
- Measured on macOS: uBlock Origin Lite installed from the Web Store in one tab appears in another tab's Extensions menu, and it survives an engine restart. On Windows, uBlock Origin Lite and a VPN extension popup worked.

## Media and DRM

| Case | Status |
| --- | --- |
| H.264 and AAC | Not supported. The standard CEF build has proprietary codecs off, so a site that serves only H.264 or AAC fails to play |
| Widevine (Netflix, Disney+, Spotify web) | Not supported, needs an owner decision |

Enabling H.264 and AAC needs a custom CEF build with `proprietary_codecs=true` and `ffmpeg_branding=Chrome`. That build comes with codec licensing questions that need an owner decision before anyone builds it.

Widevine playback needs the host app to be VMP-signed. That is Google Widevine VMP signing, for example through castLabs EVS, and it needs Google approval. Ferryx has no such signing, so the Chromium engine does not play DRM video.

The description text of the Chromium Engine row in Settings currently says tabs "play protected video". That's wrong while this limit stands, and the string in `ui/src/components/settings/BrowserEngineGroup.tsx` should be reworded.

## Google sign-in

Google may block sign-in inside embedded browsers with "This browser or app may not be secure".

- The Chromium engine uses the engine-default user agent with no spoofing, and the full Chrome-style runtime. That's the best chance of passing, but it isn't guaranteed.
- The system webview is more likely to be blocked.
- Signing the browser itself into a Google account, or Chrome Sync, isn't available in CEF.

## Fallback behavior

| Situation | Result |
| --- | --- |
| Engine not installed | New tabs use the system webview |
| Platform without an engine (Linux) | Tabs use the system webview, status is `Unsupported` |
| Engine process exits or refuses a command | Ferryx closes that profile's Chromium tabs |
| Engine window closed inside the engine | Ferryx reaps the engine and closes the Ferryx tab |

## Known gaps

- Linux engine: window placement and packaging.
- Developer ID signing of the downloaded macOS engine.
- The release manifest asset. The plan notes the manifest pointed at a local test server until the asset is published.
- Permission prompts (camera, microphone, screen, notifications, location).
- Agent automation over the Chromium engine.

## Source

| Topic | File |
| --- | --- |
| Install, verify, unpack | `src-tauri/src/browser/engine_install.rs` |
| Engine process driver | `src-tauri/src/browser/cef_engine/mod.rs` |
| Engine process | `crates/ferryx-browser-engine/src/engine/mod.rs` |
| Data dirs and install commands | `src-tauri/src/ipc/browser.rs` (`engine_data_dir`, `cmd_browser_engine_*`) |
| Settings UI | `ui/src/components/settings/BrowserEngineGroup.tsx` |
