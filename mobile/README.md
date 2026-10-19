# Ferryx mobile companion (Android + iOS)

A separate Tauri v2 shell embeds the existing RemoteApp bundle. It does not run
PTYs, the desktop daemon, native GPU terminals, or desktop filesystem commands.
Use an existing Ferryx desktop/relay to sign in and control remote sessions.
No native IPC capabilities or plugins are granted. Production network access is
HTTPS/WSS only; insecure LAN HTTP hosts are intentionally not supported yet.

## Build

Install Node, Rust, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
From the repository root install dependencies (`npm install`, then
`npm --prefix ui install`). Mobile uses the root Tauri CLI.

Android requires JDK 17, SDK platforms 35 and 36 (the Tauri runtime uses 36),
Build Tools 34/35, NDK 27.2.12479018, and JAVA_HOME/ANDROID_HOME/NDK_HOME.

```sh
cd mobile
npm run android:init
npm run android:build -- --debug --target aarch64 --apk
```

The init wrapper pins Gradle 8.9 and AGP 8.7.3 instead of unavailable preview
SDK defaults, and fixes the generated Node callback path. The Tauri runtime's
compileSdk 36 emits an AGP compatibility warning. Generated SDK projects are
ignored and recreated locally. Do not rerun raw `tauri android init` afterward.

On macOS with Xcode and iOS Rust targets:

```sh
cd mobile
npm run ios:init
npm run ios:dev
# Distribution also needs the owner's Apple team/signing setup.
npm run ios:build
```

iOS is configured, NOT compiled or run in this change. No Apple team or signing
identity is guessed, and no App Store/TestFlight deployment is performed.

Frontend-only validation: `npm --prefix ui run build:mobile`.
`npm --prefix ui run build` continues to build the desktop/web UI separately.

## QA status (2026-10-19)

- Mobile frontend and existing desktop frontend builds pass.
- ARM64 Android Rust native library cross-compiles successfully.
- APK packaging not yet verified: Gradle compilation exceeds this environment's
  bounded build attempts. No APK/device-run claim is made.
- 47 targeted tests pass (account sign-in/session, remote preferences, runtime
  routing, settings). Runtime routing: 100% statements/branches/functions/lines.
  Settings bridge: 100% statements/functions/lines, 83.33% branches. These are
  focused-module numbers, not whole-project coverage.
- Production mobile bundle, with native internals present: login screen renders
  without desktop IPC or JS errors at 360x800, 390x844 and 844x390, no horizontal
  overflow. Landscape requires normal vertical scrolling.
- Broader remote test sweep was interrupted by the execution budget; not green.
- No real phone/emulator, keyboard/IME, safe-area hardware, background/resume,
  authenticated live relay, push notifications or iOS/Xcode QA has been done.
- Bundle-size warnings remain. This is a draft companion, not store-ready.
