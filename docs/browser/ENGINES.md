# Browser engines

Ferryx browser tabs run on one of two engines. The system webview is the default and the fallback. The Chromium engine is an optional download that adds Chrome Web Store extensions and logins shared across tabs.

## The two engines

| | System webview | Chromium engine |
| --- | --- | --- |
| What it is | The OS webview, driven through wry | CEF 156.2.0 (Chrome style, via cef-rs), a separate process |
| macOS | WKWebView | Engine app in a frameless window placed over the pane |
| Windows | WebView2 | Engine as a child window of the Ferryx window |
| Linux | WebKitGTK | Engine window reparented into the Ferryx X11 window (X11 or XWayland); native Wayland falls back to the system webview |
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
| Linux | not recorded | not recorded |

### Update model

- The app pins the engine version it was built for. A manifest for any other version is refused.
- The download is checked against the manifest `sha256` and `size`. A mismatch aborts the install.
- The archive is unpacked into a `.staging-<uuid>` directory and renamed into place, so a half-finished install is never picked up. An archive entry with an absolute path or `..` rejects the whole archive.
- A new Ferryx version that needs a new engine version asks for a new download.
- Remove deletes the install dir. Open Chromium tabs keep running until they are closed.
- On Windows the install also grants read access to the AppContainer SIDs. The engine sandbox needs it.
- On macOS the engine and its helper apps keep their executable bits from the zip.

### Microsoft Store build

The Store package can't take the engine the way the direct download does without care, because Store policy 10.2.2 forbids changing the app's described function by fetching code later. Policy 10.1.5 allows acquiring add-ons or extensions that enhance the product, with user consent, after the first install. Two paths fit that.

| Path | How | Status |
| --- | --- | --- |
| Runtime download (chosen) | The same Settings > Browser > Chromium Engine download as the direct build: the user asks for it, the manifest is ed25519-signed, the archive is sha256-checked, and the engine lands in the app's data dir, never in the package's install dir. The Store listing names the optional Chromium engine and the download, so the function doesn't change behind the user's back | Same code as the direct build. Needs a release key in `TRUSTED_MANIFEST_KEYS` and the published manifest, the same as the direct build |
| MSIX optional package | Ship the engine as an optional package in a related set with the main package, so the Store installs and updates it | Not built. An optional package that runs code must be in a related set, and submitting optional packages or related sets to the Store needs Microsoft's approval. Revisit if certification rejects the runtime download |

Store policy 10.2.1 requires a browsing product to use Chromium or Gecko no more than 2 major versions behind upstream. The system webview (WebView2) is Chromium-based and updated by Windows. The pinned engine (CEF 156) has to be re-pinned in step with Chromium releases, which happens through the update model above.

The packaged app runs with `runFullTrust`, so the engine process and its AppContainer read grant should work as in the direct build, and the engine under the app data dir should go with the package when the Store app is removed. Neither has been measured on a Store build yet.

## Downloads: the reqwest re-fetch path

Page downloads normally go through the engine's own download handler: the CEF `DownloadHandler` on Chromium tabs, and WKDownload, the WebView2 download operation or WebKitDownload on system-webview tabs. Ferryx still has one path that fetches the file again itself with reqwest and the tab's cookies (`browser_download_with_session` calling `download::download_url_with_cookies`). It stays, for these reasons:

- On a system-webview tab, a link the trusted bridge intercepts (a download link the user clicked, or one that needs confirmation) is cancelled in the page and handed to Ferryx as a URL. The engine never starts that download, so Ferryx has to fetch it. The fetch sends the cookies the webview itself would send for that URL, `HttpOnly` included.
- "Ask where to save" on a system-webview tab: the native download hook runs on the main thread and can't wait for a save dialog, so the download is handed to the pane's Save prompt, which saves through this path.
- The browser CLI `download start` and the `cmd_browser_download_with_cookies` command save a URL that no page started.
- Unlike the webview's native downloads, the reqwest path can be cancelled, and it reports progress on the shelf. The system webviews give no handle to stop a native download, so those show on the shelf without Cancel.

Limits of the re-fetch: it is a second request, so a one-time URL or a POST-only download can fail where the engine's own download would work. Chromium tabs don't use it: their downloads go through the engine and support pause, resume and cancel.

## Capabilities per OS

"Yes" means it was checked by hand or by a gate in the 2026-11-01 plan status. "Not verified" means the sources don't say. "Not available" means the engine doesn't exist on that OS yet.

| Capability | macOS webview | macOS Chromium | Windows webview | Windows Chromium | Linux webview | Linux Chromium (X11 / XWayland) |
| --- | --- | --- | --- | --- | --- | --- |
| Page rendering | Yes | Yes | Yes | Yes | Yes | Yes, in CI under Xvfb and XWayland |
| Chrome Web Store extensions and popups | No | Yes | No | Yes | No | Yes, Web Store install and removal in the CI UI audit (Xvfb) |
| Extension management | No | Yes, Settings > Browser > Extensions | No | Yes, Settings > Browser > Extensions | No | Yes, Settings > Browser > Extensions |
| Shared logins and cookies across tabs of a profile | Not verified | Yes, one engine per profile | Not verified | Yes, one engine per profile | Not verified | One engine per profile, not verified by hand |
| Cookie persistence across restart | Not verified | Yes, checked by killing the engine | Not verified | Yes, checked by killing the engine | Not verified | Not verified |
| Popups, target=_blank, window.open as Ferryx tabs | Not verified | Yes | Not verified | Yes | Not verified | Not verified |
| DevTools | Not verified | Not verified | Not verified | Not verified | Not verified | Not verified |
| Named profiles | No, macOS WebKit rejects them | Yes, `browser-cef/<profile>` | Yes | Yes, `browser-cef/<profile>` | Yes | Yes, `browser-cef/<profile>` |
| Sandbox | Not verified | Not verified, engine is ad-hoc signed and needs a separately signed app | Not verified | Yes, after the AppContainer read grant | Not verified | Namespace sandbox where user namespaces are allowed, otherwise off (see Linux) |

### Linux

The Chromium engine runs on Linux. `platform_key()` in `src-tauri/src/browser/engine_install.rs` returns `linux-x86_64` or `linux-aarch64`, so Linux installs the same way as the other OSes from Settings > Browser > Chromium Engine. `scripts/browser-engine/release-engine.mjs` bundles `engine-linux-<arch>.zip` with `ferryx_engine` at its root.

Placement (`src-tauri/src/browser/cef_engine/linux.rs`): the engine reparents each tab's window into the Ferryx toplevel X window and places it in pixels, as on Windows. That needs an X window id, so it works on X11 and on XWayland (Ferryx started with `GDK_BACKEND=x11`).

Native Wayland fallback: a native Wayland Ferryx window has no X window id (`parent_xid` returns `None`), and another client's surface can't be placed inside it. In that session an installed engine reports `Installed` with `inactive: "wayland-session"` (`WAYLAND_REASON`), Settings says Chromium tabs need an X11 window and to start Ferryx with `GDK_BACKEND=x11`, and every browser tab uses WebKitGTK. A tab that asked for Chromium shows the fallback notice in the pane.

Sandbox (`crates/ferryx-browser-engine/src/engine/linux_sandbox.rs`): the engine lives in the user's data dir, where `chrome-sandbox` can never be setuid root, so the setuid sandbox isn't shipped. Chromium's namespace sandbox is used when unprivileged user namespaces are allowed. Where the kernel or its policy denies them (for example Ubuntu's AppArmor restriction), the engine runs without the sandbox and says so on stderr. Not yet checked on a real GPU desktop.

The `.deb` declares `libxtst6`, which the engine needs for input.

### macOS signing

The macOS engine is ad-hoc signed today. A Developer ID signed engine app, with the JIT entitlement, is still to be packaged.

## Extensions

- Chrome Web Store installs work in Chromium tabs. The Chrome-style toolbar extension menu is drawn inside the pane.
- Settings > Browser > Extensions lists the profile's extensions and enables, disables, pins and removes them. Remove asks for confirmation in Ferryx first. Details and options open as a Ferryx Chromium tab.
- These changes go through the engine. CEF 156 has no `CefExtension` API (it was removed upstream with the Alloy runtime), so the engine keeps one hidden browser on Chrome's `chrome://extensions` WebUI and runs each command there (`crates/ferryx-browser-engine/src/engine/extensions.rs`). Chromium shows its own uninstall confirmation on top while a removal runs.
- System-webview tabs can't run Chrome extensions. They show an "Install engine" prompt that leads to Settings > Browser > Chromium Engine.
- Measured on macOS: uBlock Origin Lite installed from the Web Store in one tab appears in another tab's Extensions menu, and it survives an engine restart. On Windows, uBlock Origin Lite and a VPN extension popup worked.

## Media and DRM

| Case | Status |
| --- | --- |
| H.264 and AAC | Not supported. The standard CEF build has proprietary codecs off, so a site that serves only H.264 or AAC fails to play |
| Widevine (Netflix, Disney+, Spotify web) | Not supported, needs an owner decision |

Enabling H.264 and AAC needs a custom CEF build with `proprietary_codecs=true` and `ffmpeg_branding=Chrome`. That build comes with codec licensing questions that need an owner decision before anyone builds it.

Widevine playback needs the host app to be VMP-signed. That is Google Widevine VMP signing, for example through castLabs EVS, and it needs Google approval. Ferryx has no such signing, so the Chromium engine does not play DRM video.

## Google sign-in

Google may block sign-in inside embedded browsers with "This browser or app may not be secure".

- The Chromium engine uses the engine-default user agent with no spoofing, and the full Chrome-style runtime. That's the best chance of passing, but it isn't guaranteed.
- The system webview is more likely to be blocked.
- Signing the browser itself into a Google account, or Chrome Sync, isn't available in CEF.

When a system-webview tab lands on an identity provider's refusal page (Google
`disallowed_useragent` or "This browser or app may not be secure"; Apple, Microsoft or Okta
"unsupported browser"), a bar above the page offers a handoff that keeps the user in Ferryx.
`src-tauri/src/browser/sign_in_handoff.rs` builds the offer and
`ui/src/components/BrowserSignInHandoffBar.tsx` shows it:

| Platform | Engine installed | Offered first | Then |
| --- | --- | --- | --- |
| macOS | yes | Sign in with Chromium engine (same profile) | Sign in with macOS (`ASWebAuthenticationSession`), then Open in default browser after "Not now" |
| macOS | no | Install Chromium engine and sign in | Sign in with macOS, then Open in default browser after "Not now" |
| Windows, Linux | yes | Sign in with Chromium engine (same profile) | Open in default browser after "Not now" |
| Windows, Linux | no | Install Chromium engine and sign in | Open in default browser after "Not now" |
| Any | unusable or unsupported | Sign in with macOS where available, otherwise Open in default browser | Open in default browser after "Not now" |

Windows and Linux have no OS web-authentication session
(`src-tauri/src/browser/auth_session/unsupported.rs` reports it as unavailable), so the Chromium
engine is their in-app path. Cancelling any handoff leaves the original tab as it was.

## Fallback behavior

| Situation | Result |
| --- | --- |
| Engine not installed | New tabs use the system webview |
| Platform without an engine build (any OS/arch `platform_key()` doesn't name) | Tabs use the system webview, status is `Unsupported` |
| Linux on a native Wayland session | Tabs use WebKitGTK, status is `Installed` with `inactive: "wayland-session"` |
| Engine process exits or refuses a command | Ferryx closes that profile's Chromium tabs |
| Engine window closed inside the engine | Ferryx reaps the engine and closes the Ferryx tab |

## Known gaps

- Linux on native Wayland: no in-pane Chromium (see Linux).
- Developer ID signing of the downloaded macOS engine.
- The release manifest asset: `MANIFEST_URL` points at the `browser-engine-156.2.0-ferryx.1` GitHub release, which isn't published yet.

## Source

| Topic | File |
| --- | --- |
| Install, verify, unpack | `src-tauri/src/browser/engine_install.rs` |
| Engine process driver | `src-tauri/src/browser/cef_engine/mod.rs` |
| Engine process | `crates/ferryx-browser-engine/src/engine/mod.rs` |
| Data dirs and install commands | `src-tauri/src/ipc/browser.rs` (`engine_data_dir`, `cmd_browser_engine_*`) |
| Settings UI | `ui/src/components/settings/BrowserEngineGroup.tsx` |
