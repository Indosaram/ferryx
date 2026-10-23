# Ferryx mobile companion (Android + iOS)

A separate Tauri v2 shell embeds the existing RemoteApp bundle. It does not run
PTYs, the desktop daemon, native GPU terminals, or desktop filesystem commands.
Use an existing Ferryx desktop/relay to sign in and control remote sessions.
No native IPC capabilities or plugins are granted. Production network access is
HTTPS/WSS only; insecure LAN HTTP hosts are intentionally not supported yet.

## Build

Install Bun, Rust, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
From the repository root install dependencies (`bun install`, then
`bun install --cwd ui`). Mobile uses the root Tauri CLI, run on Bun.

Android requires JDK 17, SDK platforms 35 and 36 (the Tauri runtime uses 36),
Build Tools 34/35, NDK 27.2.12479018, and JAVA_HOME/ANDROID_HOME/NDK_HOME.

```sh
cd mobile
bun run android:init
bun run android:build -- --debug --target aarch64 --apk
```

The init wrapper pins Gradle 8.9 and AGP 8.7.3 instead of unavailable preview
SDK defaults, and fixes the generated Tauri CLI callback path. The Tauri runtime's
compileSdk 36 emits an AGP compatibility warning. Generated SDK projects are
ignored and recreated locally. Do not rerun raw `tauri android init` afterward.

On macOS with Xcode and iOS Rust targets:

```sh
cd mobile
bun run ios:init
bun run ios:dev
# Distribution also needs the owner's Apple team/signing setup.
bun run ios:build
```

iOS is configured, NOT compiled or run in this change. No Apple team or signing
identity is guessed, and no App Store/TestFlight deployment is performed.

Frontend-only validation: `bun run --cwd ui build:mobile`.
`bun run --cwd ui build` continues to build the desktop/web UI separately.

## QA status (2026-10-19)

- Mobile frontend and existing desktop frontend builds pass.
- ARM64 Android Rust native library cross-compiles successfully.
- Android ARM64 debug APK builds with the normal `android:build` command.
  APK signature v2 verifies, package `com.ferryx.mobile` / version 0.1.0,
  min SDK 24 / target SDK 35; ARM64 native library is present.
  This is a debug-signed package, not a store/distribution release.
- 47 targeted tests pass (account sign-in/session, remote preferences, runtime
  routing, settings). Runtime routing: 100% statements/branches/functions/lines.
  Settings bridge: 100% statements/functions/lines, 83.33% branches. These are
  focused-module numbers, not whole-project coverage.
- Production mobile bundle, with native internals present: login screen renders
  without desktop IPC or JS errors at 360x800, 390x844 and 844x390, no horizontal
  overflow. Landscape requires normal vertical scrolling.
- Complete remote UI suite: 1,221 tests pass, 0 fail, 0 skip (JSON reporter).
- No real phone/emulator, keyboard/IME, safe-area hardware, background/resume,
  authenticated live relay, push notifications or iOS/Xcode QA has been done.
- Bundle-size warnings remain. This is a draft companion, not store-ready.
