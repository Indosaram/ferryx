# Ferryx mobile companion (Android + iOS)

A separate Tauri v2 shell embeds the existing RemoteApp bundle. It does not run
PTYs, the desktop daemon, native GPU terminals, or desktop filesystem commands.
Use an existing Ferryx desktop/relay to sign in and control remote sessions.
Production network access is HTTPS/WSS only; insecure LAN HTTP hosts are
intentionally not supported yet.

The only native capability is `tauri-plugin-opener` (`=2.7.2`, requires tauri
`^2.12`), scoped in `src-tauri/capabilities/mobile.json` to `opener:allow-open-url`
for `https://*` and `mailto:*` (privacy policy, support mail), plus `core:default`.

## Identity and versioning

| Field | Value |
| --- | --- |
| Bundle / application id | `com.ferryx.mobile` (permanent once published) |
| Display name | `Ferryx` (`productName`) |
| Store version | `1.0.0` (semver; `versionName` / `CFBundleShortVersionString`) |
| Android `versionCode` | `FERRYX_ANDROID_VERSION_CODE`, else Tauri's value derived from the version (1.0.0 -> 1000000) |
| iOS build number | `--build-number <n>` on `ios:build` |

Desktop releases use calendar versions (`2026.1024.1`); the stores get plain
semver so the first public build is `1.0.0`. Every upload must raise the
versionCode / build number, even when the version string is unchanged.

## Prerequisites

Install Bun, Rust (>= 1.90), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
From the repository root run `bun install` and `bun install --cwd ui`. Mobile uses
the root Tauri CLI (`@tauri-apps/cli`), run on Bun via `scripts/tauri.mjs`.

Android: JDK 17, SDK platform 36, Build Tools 35+, and **NDK 28.2.13676358**:

```sh
export JAVA_HOME="$(/usr/libexec/java_home -v 17)"   # or /opt/homebrew/opt/openjdk@17
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
```

iOS: Xcode with iOS Rust targets (`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`),
XcodeGen and CocoaPods as required by Tauri, and the owner's Apple Developer team.

## Generated native projects

`src-tauri/gen/` is gitignored. Every native setting is applied by the init
scripts, never by hand. Rerun them after deleting `gen/`; never run raw
`tauri android init` / `tauri ios init`.

`bun run android:init` (`scripts/init-android.mjs`, transforms in `android-patches.mjs`):

- compileSdk / targetSdk **36**, `ndkVersion = "28.2.13676358"` (r28 links with 16 KB ELF alignment).
- AGP **8.13.2** + Gradle **8.14.3**. AGP 8.11+ supports API 36 and needs Gradle >= 8.13
  ([AGP 8.11 release notes](https://developer.android.com/build/releases/agp-8-11-0-release-notes));
  8.13.2 is the newest 8.x on Google Maven, and Gradle 8.14.3 is the wrapper that
  tauri-cli 2.11.4 itself generates.
- Release signing from `keystore.properties` (see below); without it Gradle logs
  a `WARNING: Ferryx release signing disabled` line and release outputs are unsigned.
- `versionCode` from `FERRYX_ANDROID_VERSION_CODE`.
- Window/splash/system bar background `#23262D`, edge-to-edge `MainActivity` with keyboard padding.
- Committed launcher icons (`src-tauri/icons/android`, incl. adaptive icon on `#23262D`) copied into `res/`.

`bun run ios:init` (`scripts/init-ios.mjs`, transforms in `ios-patches.mjs`) runs
`tauri ios init --ci` only when `gen/apple` is missing (project generation, no compile), then:

- `TARGETED_DEVICE_FAMILY = 1` (iPhone only), `DEVELOPMENT_TEAM` from `APPLE_DEVELOPMENT_TEAM`.
- `ITSAppUsesNonExemptEncryption = false` (only OS-provided HTTPS/WSS).
- Copies `ios-templates/PrivacyInfo.xcprivacy` (no tracking; email address collected,
  linked, for app functionality; UserDefaults reason `CA92.1` for WebKit storage) and bundles it.
- LaunchScreen background `#23262D`; committed alpha-free AppIcon set copied into `Assets.xcassets`.
- Regenerates the Xcode project with `xcodegen`.

`bun run --cwd mobile test` checks every transform against the tauri-cli 2.11.4
templates in `scripts/fixtures/`.

## Icons

```sh
cd mobile && bun run icons
```

Runs `tauri icon icons.json` (source `ui/public/ferryx-icon.png`, background
`#23262D`), then flattens the iOS set with ImageMagick and checks that
`AppIcon-512@2x.png` (the 1024 marketing icon) has no alpha. App Store Connect
rejects marketing icons that have an alpha channel. The output is committed.

## Android back navigation

Android 13+ (predictive back, default at targetSdk 36) is handled by Tauri's
`AppPlugin` `OnBackPressedCallback`: it calls `WebView.goBack()` while history
exists, otherwise it finishes the activity. The UI must `history.pushState` for
in-app screens (session view, settings, dialogs) so Back pops a screen instead of
closing the app.

## Development

```sh
cd mobile
bun run android:init
bun run android:dev        # or: bun run ios:init && bun run ios:dev
```

Frontend-only validation: `bun run --cwd ui build:mobile`.

## Release build (owner machine only)

Releases are local-only, the same as desktop (see
[LOCAL_RELEASE_RUNBOOK](../docs/releases/LOCAL_RELEASE_RUNBOOK.md#mobile-androidios)).
No hosted workflow builds, signs, or uploads mobile binaries.

### Android upload key (once)

```sh
keytool -genkeypair -v -keystore ~/secure/ferryx-upload.jks -alias ferryx-upload \
  -keyalg RSA -keysize 4096 -validity 10000
```

Use Play App Signing: this is the *upload* key, so Google holds the app signing key.
Back up the `.jks` and its passwords offline; never commit them (`*.jks` and
`keystore.properties` are gitignored). Create `mobile/keystore.properties`, or
put it elsewhere and set `FERRYX_ANDROID_KEYSTORE_PROPERTIES=/path/keystore.properties`:

```properties
storeFile=/Users/<you>/secure/ferryx-upload.jks
storePassword=...
keyAlias=ferryx-upload
keyPassword=...
```

A relative `storeFile` resolves against the directory containing `keystore.properties`.

### Android AAB

```sh
cd mobile
export FERRYX_ANDROID_VERSION_CODE=1000000   # must increase on every upload
bun run android:init                          # only after gen/ was removed
bun run android:build -- --aab --target aarch64 --target armv7 --target x86_64
AAB=src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab
jarsigner -verify "$AAB"                       # must report "jar verified"
# 16 KB page alignment of the native libraries:
bundletool build-apks --bundle "$AAB" --output /tmp/ferryx.apks --mode universal
unzip -o /tmp/ferryx.apks universal.apk -d /tmp/ferryx-apk
"$ANDROID_HOME/build-tools/36.0.0/zipalign" -c -P 16 -v 4 /tmp/ferryx-apk/universal.apk
```

`zipalign -c -P 16` must end in `Verification successful`.

### iOS archive / export

```sh
cd mobile
export APPLE_DEVELOPMENT_TEAM=XXXXXXXXXX
bun run ios:init
bun run ios:build -- --export-method app-store-connect --build-number 1
# IPA: src-tauri/gen/apple/build/arm64/Ferryx.ipa
xcrun altool --validate-app -f src-tauri/gen/apple/build/arm64/Ferryx.ipa -t ios --apiKey <KEY> --apiIssuer <ISSUER>
```

Upload with Transporter or `xcrun altool --upload-app`. Use
`--export-method release-testing` for TestFlight-only builds.

### Owner-only steps checklist

- [ ] Create and back up the Android upload keystore; enroll in Play App Signing.
- [ ] Play Console: app `com.ferryx.mobile`, name "Ferryx", store listing (short/full
      description, 512px icon, 1024x500 feature graphic, phone screenshots), category, contact email.
- [ ] Play privacy policy URL `https://ferryx.dev/privacy/`; Data safety form: email address
      collected, linked to the user, for app functionality and account management, encrypted
      in transit, account deletion available; no data shared, no tracking/ads.
- [ ] Content rating questionnaire, target audience (not children), ads = none, app access
      instructions with a demo account and relay URL.
- [ ] Closed testing track with >= 12 testers for 14 days (new personal accounts) before production.
- [ ] App Store Connect: app record for bundle id `com.ferryx.mobile`, name "Ferryx",
      privacy policy URL, App Privacy answers matching `PrivacyInfo.xcprivacy`, age rating,
      export compliance (exempt, matches `ITSAppUsesNonExemptEncryption=false`).
- [ ] App Review notes: explain that Ferryx is a remote companion for the Ferryx desktop
      app, and provide a demo account plus an always-on demo desktop/relay so the reviewer can
      sign in and see a live session; mention in-app account deletion.
- [ ] TestFlight internal test, then submit for review.

## QA status

- Shell configured for store release (SDK 36, signing, icons, privacy manifest);
  config transforms are covered by `bun run --cwd mobile test`.
- 2026-10-19 baseline: an Android ARM64 debug APK built and the remote UI suite passed
  (1,221 tests). No signed release AAB/IPA has been produced since this change, and no
  real-device, emulator, keyboard/IME, background/resume, or iOS/Xcode QA has been done.
