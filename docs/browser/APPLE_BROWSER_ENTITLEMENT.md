# Apple browser entitlement

Status: request prepared, not submitted. Wiring is behind the cargo feature
`apple-web-browser-entitlement`, which is off by default.

## Exact entitlement names

| Entitlement | Type | What it grants | Needed by Ferryx |
|---|---|---|---|
| `com.apple.developer.web-browser` | Boolean | "Default Web Browser" entitlement: the app may be chosen as the system default browser on iOS/iPadOS and is treated as a full browser (unrestricted web navigation). | Requested for parity and future iPad client; macOS lets any app with `CFBundleURLTypes` http/https become the default without it. |
| `com.apple.developer.web-browser.public-key-credential` | Boolean | Lets a browser make WebAuthn/passkey requests on behalf of any relying party (macOS 13.3+ / iOS 17.4+). | Yes, for passkey sign-in in system-webview tabs. Request together. |

Both are granted per team by Apple on request; a build signed with either entitlement
but a provisioning profile that lacks it fails to launch. That is why the feature is off by
default.

Reference: Apple Developer documentation, "Preparing your app to be the default web browser"
and "com.apple.developer.web-browser" / "com.apple.developer.web-browser.public-key-credential"
entitlement keys.

## Request text

Submit at https://developer.apple.com/contact/request/ (Default browser entitlement and
Web browser public key credential request).

> **App name:** Ferryx
> **Bundle ID:** com.ferryx.app
> **Platform:** macOS (Developer ID distribution), later iPadOS
>
> Ferryx is a developer workspace that includes a full web browser. Users open any http(s)
> URL in browser tabs with an address bar, history, profiles, downloads, find, print and
> per-site permissions. Ferryx can be set as the default browser (it registers
> `CFBundleURLTypes` for http and https and asks Launch Services to become the handler) and
> opens links handed to it by the system in a new tab. Navigation is unrestricted: users
> may visit any site; there is no allow-list.
>
> We request `com.apple.developer.web-browser` so Ferryx qualifies as a browser app, and
> `com.apple.developer.web-browser.public-key-credential` so users can sign in with passkeys
> on any website inside Ferryx tabs, as they can in other browsers. Ferryx uses WebKit
> (WKWebView) for its system engine and keeps the standard Safari-compatible user agent.
>
> Ferryx complies with the browser requirements: it renders arbitrary web content, has a
> URL field, supports search, and does not alter page content beyond user-requested
> features.

## Wiring

- `src-tauri/Info.plist` (merged by Tauri into the bundle) declares `CFBundleURLTypes` for
  http/https. This needs no entitlement and is always on.
- `src-tauri/entitlements/web-browser.entitlements` holds `com.apple.developer.web-browser`.
- `src-tauri/entitlements/tauri.web-browser.conf.json` points the macOS bundle at that file.
- Enable both together for a release once Apple grants the entitlement and the provisioning
  profile includes it:

  ```bash
  cargo tauri build --features apple-web-browser-entitlement \
    --config entitlements/tauri.web-browser.conf.json
  ```

  Add `com.apple.developer.web-browser.public-key-credential` to the same file once it is granted.

## Google sign-in

Google rejects sign-in in embedded webviews (`disallowed_useragent`, "This browser or app
may not be secure") regardless of these entitlements. Ferryx keeps its user-agent spoof and,
when a system-webview tab lands on that page, offers "Sign in with Chromium engine": the same
sign-in in a Chromium engine tab of the same profile (Settings > Browser > Chromium Engine).
