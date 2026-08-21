# rorca Native Notification Implementation Plan

## 1. Objective

Add native desktop notification support to rorca with behavior comparable to Orca while fitting rorca's current Tauri v2 + React architecture.

The implementation should provide:

- native OS notifications for agent task completion and terminal bells;
- no OS notification when the rorca window is foreground-focused;
- unread indicators for background/inactive terminal tabs and their worktrees;
- a Notifications settings pane with a master switch, per-source toggles, permission status, system/custom sound selection, volume, preview, and test notification;
- a stable frontend IPC facade equivalent in purpose to Orca's `window.api.notifications.*` bridge;
- reliable macOS authorization status instead of treating Tauri's desktop permission helper as authoritative;
- custom sound playback that does not require exposing arbitrary local files to the WebView;
- deterministic unit/integration coverage plus packaged-app platform verification.

This document is a plan only; it does not implement the feature.

---

## 2. Current rorca State Verified

### Backend

The relevant backend structure is:

- `src-tauri/src/lib.rs`
  - constructs the Tauri builder;
  - manages `PtyManager` and `WorkspaceRegistry`;
  - registers commands directly in `tauri::generate_handler![...]`;
  - currently initializes no Tauri plugins.
- `src-tauri/src/ipc/mod.rs`
  - central IPC module/export point;
  - provides `run_blocking(...)` for blocking native work.
- `src-tauri/src/ipc/preferences.rs`
  - exposes terminal preference retrieval only.
- `src-tauri/Cargo.toml`
  - currently has no notification, dialog, audio, or macOS UserNotifications dependency.
- `src-tauri/capabilities/default.json`
  - currently grants only `core:default` and window drag support.
- `src-tauri/tauri.conf.json`
  - product name: `rorca`;
  - bundle identifier: `com.orca.lite`;
  - current CSP does not expose the Tauri asset protocol or arbitrary local media.

The resolved Tauri version in the current lock graph is **Tauri 2.11.5**.

### Frontend

The relevant frontend structure is:

- `ui/src/lib/tauri.ts`
  - is the existing typed frontend/native bridge;
  - wraps custom Tauri commands through `invokeCommand(...)`;
  - exposes browser fallbacks where appropriate.
- `ui/src/lib/terminalSettings.ts`
  - demonstrates rorca's current settings persistence pattern: localStorage + normalization + a custom same-window event + the browser `storage` event.
- `ui/src/lib/terminalEvents.ts`
  - owns the singleton terminal output/lifecycle event bus.
- `ui/src/state/workspaceStore.ts`
  - owns worktrees, sessions, terminal tabs, terminal lifecycle, and active layout state;
  - has no unread state today.
- `ui/src/state/workspaceRuntime.ts`
  - refreshes native workspace state on window focus.
- `ui/src/components/TerminalPane.tsx`
  - already subscribes to xterm's `onTitleChange`;
  - does **not** subscribe to xterm's `onBell`.
- `ui/src/components/TerminalSplitView.tsx`
  - can render the same session more than once for nested split presentation, which means a naive per-`TerminalPane` bell listener can double-fire.
- `ui/src/components/TabBar.tsx`
  - has no unread indicator.
- `ui/src/components/WorktreeList.tsx`
  - has status/agent indicators but no unread indicator.
- `ui/src/components/SettingsDialog.tsx`
  - currently contains General, Terminal, Keyboard Shortcuts, and Workspace sections;
  - has no Notifications section.
- `ui/src/lib/agentTitle.ts`
  - identifies several agent CLIs and maps titles to `working`/`waiting`-style states;
  - currently loses the distinction between `needs input` and explicit `done/completed/idle` for some agents.

A search of `src-tauri/` and `ui/src/` confirmed there is no existing native notification implementation or unread notification state.

### Original Orca behavior checked for parity

The bundled original Orca code under `ui/original-dist/` confirms the supplied architecture and adds a few useful details:

- it uses `document.visibilityState === "visible" && document.hasFocus()` as the foreground-focused test;
- it marks worktree/pane/tab attention state separately from OS delivery;
- it suppresses duplicate/obsolete agent completion events;
- it defers terminal bell notifications while an agent-completion notification is pending so one event does not produce two alerts;
- it plays a custom sound only after notification dispatch reports success;
- it has a once-per-session blocked-notification fallback directing the user to System Settings;
- its settings include `enabled`, `agentTaskComplete`, `terminalBell`, `customSoundId`, `customSoundPath`, and `customSoundVolume` semantics;
- the current original bundle also contains built-in sound presets such as `two-tone`, `bong`, `thump`, `blip`, `sonar`, `blop`, `ding`, `clack`, and `beep`. Those presets are optional parity work, not required for the first rorca implementation.

---

## 3. Technical Feasibility and Important Constraints

### 3.1 Tauri v2 native notifications: feasible

Use `tauri-plugin-notification` v2 from Rust. The current rorca toolchain is compatible:

- resolved Tauri: 2.11.5;
- current workspace Rust: 1.92.0;
- Tauri notification plugin requires Rust 1.77.2 or newer.

Recommended dependency shape:

```toml
tauri-plugin-notification = "2"
```

Initialize it in `src-tauri/src/lib.rs` with:

```rust
.plugin(tauri_plugin_notification::init())
```

The frontend should **not** call `@tauri-apps/plugin-notification` directly. Instead, all notification operations should go through rorca's own typed commands in `ui/src/lib/tauri.ts`.

Why:

1. rorca already uses a custom command facade;
2. it keeps macOS/Windows/Linux differences out of React;
3. it gives tests one mockable contract;
4. it avoids granting the full `notification:default` guest permission unless direct JavaScript plugin calls are later introduced.

If a future implementation chooses to call the plugin from JavaScript, then add `@tauri-apps/plugin-notification` to `ui/package.json` and the minimum required notification capabilities. That is not the recommended first implementation.

### 3.2 Tauri desktop permission APIs are not authoritative on macOS

This is the most important feasibility finding.

In current `tauri-plugin-notification` 2.3.3 desktop source, both:

- `request_permission()`; and
- `permission_state()`

return `PermissionState::Granted` on desktop.

Therefore `isPermissionGranted()` / `permissionState()` cannot be used as the sole source of truth for a user who has disabled rorca notifications in macOS System Settings.

For macOS, query Apple's native `UNUserNotificationCenter` / `UNNotificationSettings` API instead. Current Rust bindings are available through `objc2-user-notifications` 0.3.x and expose:

- `UNUserNotificationCenter::currentNotificationCenter()`;
- `getNotificationSettingsWithCompletionHandler(...)`;
- `requestAuthorizationWithOptions_completionHandler(...)`;
- `UNNotificationSettings.authorizationStatus()`;
- alert/sound/notification-center settings.

Recommended target-specific dependency shape:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2-user-notifications = "0.3"
```

Add compatible `objc2`/`block2` support crates only if the implementation needs them directly to construct the callback block.

The macOS native helper must be isolated under `#[cfg(target_os = "macos")]` so Windows/Linux builds never link Apple frameworks.

### 3.3 Permission request must be explicit

Do not prompt for notifications at application startup.

A permission request may occur only following an explicit user action, such as:

- turning on the Notifications master switch; or
- clicking `Send test notification` / `Allow notifications`.

Recommended macOS authorization options are alert + sound. If authorization has already been denied, a second request will not produce another system prompt; the UI should offer `Open System Settings` instead.

### 3.4 “Delivered” cannot mean “the user saw a banner”

Tauri's desktop implementation is fire-and-forget, and OS Focus/Do Not Disturb modes may suppress presentation even when the app is authorized.

Therefore do not define the native result as a guarantee that a banner was visually delivered.

Use semantics such as:

- `submitted`: the request passed rorca permission/preflight checks and was submitted to the native backend;
- `blocked-by-system`: an authoritative permission query says the OS has denied notifications;
- `permission-required`: macOS is still `notDetermined` and no implicit prompt was performed;
- `unsupported`;
- `failed`.

A `probeDelivery`-style API can report readiness and optionally submit a visible test notification, but it must not claim to detect whether Focus mode actually presented a banner.

### 3.5 macOS development builds are not sufficient permission QA

The current Tauri notification plugin source intentionally uses `com.apple.Terminal` as the macOS application identity in development and the configured app identifier in packaged builds.

Consequences:

- dev-mode notification attribution/permission state can differ from the installed `.app`;
- authoritative acceptance testing for `com.orca.lite` must use a packaged/signed-or-local-installed `.app`;
- unit tests should not depend on the host machine's real notification authorization.

### 3.6 Windows requires installed-app verification

Tauri's notification documentation explicitly notes that Windows notifications work correctly for installed applications; development behavior uses the PowerShell identity/icon.

Windows acceptance criteria must therefore be run against an installed bundle, not only `tauri dev`.

### 3.7 Custom audio is feasible, but separate it from the WebView

Tauri's notification `sound` option is platform-specific:

- macOS expects system sound names or sounds available to the app bundle;
- Linux uses XDG sound names or file paths depending on the notification server;
- Windows uses toast/native sound behavior and paths.

It is not a reliable cross-platform solution for an arbitrary user-picked audio file **plus an application-controlled volume slider**.

Recommended approach: use a small Rust audio service for custom playback.

`rodio` 0.22.2 is compatible with the current Rust 1.92.0 toolchain. Use only required features rather than the default recording feature set, for example:

```toml
rodio = { version = "0.22", default-features = false, features = [
  "playback",
  "wav",
  "mp3",
  "flac",
  "vorbis",
  "mp4",
] }
```

Benefits:

- the selected path stays on the Rust side during decoding/playback;
- the current CSP does not need `asset:` / local-media exceptions;
- no `convertFileSrc(...)` access scope is required;
- preview and real notification playback use the same code path;
- volume can be normalized to `0.0..=1.0` in one place;
- playback is not subject to WebView autoplay policy.

The audio output stream must be kept alive in managed state; do not create a temporary `OutputStream` that is dropped immediately after starting playback. Initialize it lazily so machines with no current audio output device can still launch rorca.

### 3.8 Custom application audio has an OS-policy caveat

A separately played `rodio` sound is application audio, not an OS notification-channel sound. It may not automatically obey every OS notification-sound setting or Focus mode rule.

This is also a concern for any WebAudio-based implementation.

Before shipping custom-file audio, explicitly decide that rorca's own `customSoundVolume` preference is intended to control application audio. Document that behavior and test Focus/Do Not Disturb. If strict OS Focus compliance is a product requirement, custom arbitrary-file + independent-volume playback should be reconsidered in favor of native notification sounds only.

### 3.9 Prevent double sound

When `customSoundId === "system"`:

- do not invoke the Rust custom audio player;
- let the native notification backend use normal OS behavior.

When a built-in/custom rorca sound is selected:

- the OS notification should be submitted without a separate custom native sound;
- then rorca plays one application sound after successful submission.

Because default/silent semantics differ between notification backends, add a platform acceptance test confirming there is exactly one audible alert. If a Tauri-plugin transport cannot reliably suppress its own default sound on a target, keep the IPC contract but add a target-specific notification adapter capable of silent submission rather than accepting double playback.

On Linux, the standard `suppress-sound` notification hint is available when the desktop notification server supports sound. On Windows the underlying WinRT API can create a silent toast. macOS `UNNotificationContent.sound = nil` is the native silent representation. These are fallback implementation paths if the generic plugin path is insufficient.

### 3.10 File picker is feasible without exposing filesystem APIs to React

Use `tauri-plugin-dialog` v2 from Rust:

```toml
tauri-plugin-dialog = "2"
```

Register the plugin, then expose a narrow rorca command such as `cmd_notification_pick_audio` that uses `DialogExt` and returns only the selected path/name.

Recommended file filters:

- WAV;
- MP3;
- FLAC;
- OGG/Vorbis;
- M4A/MP4 if the `rodio` mp4 feature is enabled.

Do not expose a general arbitrary file read command.

### 3.11 System Settings opening should be narrow and best-effort

Expose `cmd_notification_open_system_settings`, not a generic shell-open command.

- macOS: open the Notifications settings pane using the current System Settings URI/deep-link strategy, with a fallback to the System Settings application if the pane URI fails;
- Windows: open the Notifications settings page (`ms-settings:` family) using a fixed target;
- Linux: return `unsupported` unless a desktop-specific settings target is deliberately added.

The command must not accept an arbitrary URI from the frontend.

---

## 4. Recommended Architecture

Keep OS integration in Rust and notification decision/orchestration in TypeScript.

```text
Terminal output/title/bell/lifecycle
          |
          v
ui notification coordinator
  - settings gate
  - completion-edge detector
  - bell dedupe
  - foreground gate
  - unread marking
          |
          +------------------------+
          |                        |
          v                        v
custom Rust IPC              workspace unread state
  notification_dispatch      -> TabBar
  permission_status          -> WorktreeList
  request_permission
  probe_delivery
  open_system_settings
  pick_audio
  play_sound
          |
          v
Rust notification service
  - content sanitization
  - OS notification adapter
  - macOS UserNotifications permission adapter
  - custom audio player
```

### Responsibility rule

- **React/TypeScript decides whether an application event should notify.**
- **Rust decides how to talk to the OS safely.**
- **Workspace reducer owns unread state.**
- **Settings persistence stays in the frontend**, matching the existing terminal settings architecture.

Do not put React state, active-tab knowledge, or user event dedupe into Rust.

---

## 5. Backend Design

### 5.1 Add a notification domain module

Add:

```text
src-tauri/src/notification/
  mod.rs
  audio.rs
  permission.rs
  platform/
    mod.rs
    macos.rs
```

Possible responsibilities:

- `mod.rs`
  - public DTO-independent service abstractions;
  - content formatting/sanitization helpers;
  - native notification submission.
- `audio.rs`
  - lazy `rodio` output stream/mixer;
  - path validation;
  - decode/play;
  - volume normalization;
  - short replay dedupe if necessary.
- `permission.rs`
  - normalized cross-platform permission model.
- `platform/macos.rs`
  - `UNUserNotificationCenter` settings/request implementation.

Name the module `notification` or `notifications` consistently; avoid mixing both forms in command names/files.

### 5.2 Add IPC commands

Add `src-tauri/src/ipc/notifications.rs` and export it from `src-tauri/src/ipc/mod.rs`.

Recommended command surface:

```text
cmd_notification_dispatch
cmd_notification_permission_status
cmd_notification_request_permission
cmd_notification_probe_delivery
cmd_notification_open_system_settings
cmd_notification_pick_audio
cmd_notification_play_sound
```

This intentionally mirrors the useful semantics of original Orca's:

```text
notifications.dispatch
notifications.getPermissionStatus
notifications.probeDelivery
notifications.openSystemSettings
notifications.playSound
shell.pickAudio
```

The explicit `request_permission` command is an rorca improvement so requesting authorization is separated from checking it.

### 5.3 IPC request/result types

Use serde camelCase DTOs.

Suggested dispatch types:

```rust
#[serde(rename_all = "camelCase")]
enum NotificationSource {
    AgentTaskComplete,
    TerminalBell,
    Test,
}

#[serde(rename_all = "camelCase")]
struct DispatchNotificationRequest {
    source: NotificationSource,
    notification_id: Option<String>,
    workspace_label: Option<String>,
    worktree_label: Option<String>,
    terminal_title: Option<String>,
    agent_label: Option<String>,
}

#[serde(rename_all = "camelCase")]
struct DispatchNotificationResult {
    submitted: bool,
    reason: Option<NotificationDispatchReason>,
}
```

Suggested reason values:

```text
permission-required
blocked-by-system
unsupported
backend-error
```

Do not send raw agent prompts, tool input, shell output, or absolute working-directory paths into OS notification text by default. Native notifications may be visible on a lock screen.

### 5.4 Content formatting and sanitization

Format final title/body in one pure Rust or TypeScript helper, not ad hoc across event sources.

Recommended content policy:

Agent completion:

```text
Title: <Agent> finished
Body:  <worktree/branch label>
```

Terminal bell:

```text
Title: Terminal needs attention
Body:  <worktree/branch label> · <short terminal title if safe>
```

Test:

```text
Title: rorca notifications are working
Body:  Test notification
```

Requirements:

- strip C0/control characters;
- collapse pathological whitespace;
- truncate title/body to bounded lengths;
- never include full shell output;
- avoid absolute repository paths when a branch/worktree display label exists.

Unit-test this formatter independently of Tauri.

### 5.5 Permission status DTO

Do not collapse all platforms into a misleading boolean.

Suggested shape:

```text
NotificationPermissionStatus
  platform: "macos" | "windows" | "linux" | "other"
  supported: boolean
  authorization:
    "not-determined" | "authorized" | "denied" | "provisional" | "unknown"
  alertsEnabled: boolean | null
  soundsEnabled: boolean | null
  requested: boolean
  authoritative: boolean
  canOpenSettings: boolean
```

macOS mapping:

- `UNAuthorizationStatusNotDetermined` -> `not-determined`;
- `Denied` -> `denied`;
- `Authorized` -> `authorized`;
- `Provisional` -> `provisional` if relevant on the running macOS version;
- unrecognized future value -> `unknown`.

Also map `alertSetting()` and `soundSetting()` to nullable booleans without incorrectly treating `sound=false` as notification authorization denial.

For Windows/Linux in the first implementation:

- `supported = true` when the backend is available;
- `authorization = unknown` unless an authoritative platform query is deliberately implemented;
- `authoritative = false`.

Do **not** return `authorized` simply because Tauri's desktop `permission_state()` returns Granted.

### 5.6 Permission request implementation

On macOS:

1. run the UserNotifications call from a safe native/main-thread context;
2. call `requestAuthorization` for alert + sound;
3. marshal the callback result into a plain Rust DTO via a one-shot channel;
4. re-query `UNNotificationSettings` after the request resolves;
5. return the normalized status, not only the callback boolean.

On Windows/Linux desktop, no fake prompt is required for the first implementation. Return the current normalized status.

### 5.7 Probe semantics

`cmd_notification_probe_delivery` should be intentionally limited.

Recommended behavior:

- always re-query permission/readiness;
- if blocked -> return `blocked-by-system` without sending;
- if not determined -> return `permission-required` unless the call explicitly represents a user-requested test flow;
- if ready and `sendTest=true` -> submit one visible test notification;
- return `submitted`, not `visibly-delivered`.

Do not run a multi-minute hidden polling loop on every Settings mount. Instead:

- query once when the Notifications pane opens;
- refresh when the window regains focus after System Settings;
- refresh immediately after permission request;
- use `Send test notification` for user-visible verification.

### 5.8 Notification submission adapter

Initial implementation may use:

```rust
use tauri_plugin_notification::NotificationExt;

app.notification()
    .builder()
    .title(...)
    .body(...)
    .show()?;
```

Before submission on macOS, perform the authoritative permission preflight so rorca can return `blocked-by-system` or `permission-required` instead of blindly claiming success.

Keep submission behind an internal trait/interface, for example:

```text
trait NativeNotificationBackend {
    fn submit(...);
}
```

That seam allows:

- a fake backend for unit tests;
- a future platform-specific silent adapter when custom application audio is selected;
- implementation changes without changing frontend IPC.

### 5.9 Audio player

Manage a single lazy audio player in Tauri state.

Suggested API:

```text
play(path, volume, force) -> PlaySoundResult
```

Suggested result:

```text
played: boolean
reason?:
  "not-found"
  "unsupported-format"
  "decode-failed"
  "no-output-device"
  "deduped"
  "backend-error"
```

Requirements:

- normalize frontend integer volume `0..100` to `0.0..1.0`;
- reject non-files;
- validate/canonicalize the selected path before playback;
- use a bounded maximum file size suitable for a notification sound (choose and test a constant, e.g. 20 MiB);
- restrict picker/accepted formats to the enabled decoder set;
- do not panic if the saved file was moved/deleted;
- do not log the full user path at normal info level;
- keep the audio output stream alive in managed state;
- allow `force=true` for Settings preview even if a recent automatic sound was deduped.

### 5.10 File picker

`cmd_notification_pick_audio` should:

1. open a single-file native dialog;
2. apply the supported audio extension filter;
3. return `null` on cancel;
4. return `{ path, displayName }` on success;
5. not read the file into the WebView.

### 5.11 Register backend pieces

Update `src-tauri/src/lib.rs` to:

- export the notification domain module;
- initialize `tauri-plugin-notification`;
- initialize `tauri-plugin-dialog`;
- manage the audio service/state;
- register all notification commands in `generate_handler![...]`.

### 5.12 Capability policy

If the frontend uses **only custom rorca commands**, do not add the broad JavaScript guest permission `notification:default` or `dialog:default` merely because the Rust plugins are initialized.

If direct guest plugin calls are later introduced, grant only the minimum plugin command capabilities that are actually needed.

This keeps `src-tauri/capabilities/default.json` narrow.

### 5.13 CSP policy

The recommended Rust audio design requires **no CSP expansion**.

Do not enable `assetProtocol` or add broad `media-src asset:` access solely for notification sounds.

---

## 6. Frontend IPC and Settings State

### 6.1 Add notification DTOs

Add notification types to `ui/src/lib/types.ts` or a dedicated `ui/src/lib/notificationTypes.ts` if the type set becomes large.

Types should mirror Rust camelCase DTOs exactly.

At minimum:

```text
NotificationSource
NotificationPermissionStatus
DispatchNotificationRequest
DispatchNotificationResult
NotificationProbeResult
SelectedNotificationAudio
PlayNotificationSoundRequest
PlayNotificationSoundResult
```

### 6.2 Extend `ui/src/lib/tauri.ts`

Add typed wrappers:

```text
dispatchNotification(request)
getNotificationPermissionStatus()
requestNotificationPermission()
probeNotificationDelivery(options?)
openNotificationSystemSettings()
pickNotificationAudio()
playNotificationSound(request)
```

Browser-preview behavior:

- permission status -> `supported:false`, `authorization:"unknown"`;
- dispatch -> `submitted:false`, `reason:"unsupported"`;
- picker -> `null`;
- play -> `{ played:false, reason:"unsupported" }`.

Do not make browser preview throw simply because notifications are unavailable.

### 6.3 Add `ui/src/lib/notificationSettings.ts`

Follow the same architecture as `terminalSettings.ts`:

- localStorage-backed;
- strict parsing/normalization;
- same-window custom event;
- browser `storage` event;
- hook API for Settings and App/coordinator.

Recommended storage key:

```text
orca.notification.settings
```

Recommended shape:

```ts
type NotificationSoundId = "system" | "custom";

type NotificationSettings = {
  enabled: boolean;
  agentTaskComplete: boolean;
  terminalBell: boolean;
  customSoundId: NotificationSoundId;
  customSoundPath: string | null;
  customSoundName: string | null;
  customSoundVolume: number; // integer 0..100
};
```

Defaults:

```text
enabled: true
agentTaskComplete: true
terminalBell: true
customSoundId: system
customSoundPath: null
customSoundName: null
customSoundVolume: 100
```

These match Orca's feature-on semantics without producing an unsolicited macOS prompt: the coordinator must not request permission automatically. If authorization is still `not-determined`, automatic dispatch returns `permission-required` and the user can explicitly authorize from Settings.

Alternative product decision: if rorca requires explicit opt-in before even attempting notifications, set only `enabled:false`. Keep the two child defaults true so turning the master switch on behaves predictably. Decide this before implementation and encode it in tests.

Normalization rules:

- invalid booleans -> defaults;
- invalid/missing custom path with `customSoundId:"custom"` -> fall back to system;
- clamp volume to integer `0..100`;
- preserve child toggles while master is off.

### 6.4 Optional built-in sounds

The current original Orca bundle has several bundled sound IDs in addition to `system` and `custom`.

Do not make those a blocker for first native notification support. If parity is desired later:

- package built-in audio files as Tauri resources;
- extend `NotificationSoundId` with explicit known IDs;
- resolve each ID to a backend resource path in Rust;
- reuse the same `playNotificationSound` path and volume behavior.

---

## 7. Event Detection and Notification Coordinator

### 7.1 Add a dedicated coordinator

Add a module/hook such as:

```text
ui/src/lib/notificationDispatcher.ts
ui/src/state/useNotificationCoordinator.ts
```

The coordinator owns side effects and short-lived dedupe state; the workspace reducer remains pure.

Inputs:

- current notification settings;
- workspace state snapshot;
- terminal bell callbacks;
- terminal/session lifecycle changes;
- parsed terminal titles;
- `document.visibilityState` / `document.hasFocus()`;
- workspace unread action callbacks.

Outputs:

- `dispatchNotification(...)` calls;
- optional `playNotificationSound(...)` calls;
- unread actions;
- once-per-session blocked-system UI event/toast state.

### 7.2 Foreground rule

Reuse Orca's foreground definition:

```ts
function isRorcaWindowForegroundFocused() {
  return document.visibilityState === "visible" && document.hasFocus();
}
```

For the first implementation:

- **OS notification:** only if the rorca window is not foreground-focused;
- **Unread state:** mark if the source tab is not currently visible **or** the app is not foreground-focused;
- **Foreground active tab:** neither OS notification nor unread marking.

This directly satisfies the supplied behavioral requirement and avoids macOS foreground-presentation inconsistencies.

If a future product decision wants OS notifications for inactive tabs while the app itself is focused, add that separately and test foreground macOS presentation behavior.

### 7.3 Terminal bell detection

xterm 6 exposes `terminal.onBell`.

Modify the callback chain:

```text
TerminalPane
  -> TerminalSplitView/TerminalPaneFrame
  -> App/coordinator
```

Add:

```text
TerminalPaneProps.onBell?: () => void
TerminalSplitViewProps.onBell?: (tabId: string) => void
```

Register and dispose `terminal.onBell(...)` alongside `onData` and `onTitleChange`.

### 7.4 Bell dedupe is mandatory

`TerminalSplitView` can mount duplicate visual panes for the same terminal session, so the same terminal BEL can be parsed by more than one xterm instance.

The coordinator must dedupe terminal bells by backend session ID (or stable session ID) and a short time window, for example 250-500 ms.

Do not dedupe globally: two different sessions ringing at the same time are separate events.

### 7.5 Agent completion detection must not use `working -> waiting` blindly

Current `parseAgentTitle(...)` maps both actual completion and several attention states into `waiting`-like values. For example:

- needs input;
- permission/approval required;
- prompt/waiting;
- done/completed/idle.

Therefore this rule is unsafe:

```text
working -> waiting == task complete
```

It would produce false completion notifications.

For phase 1, fire `agent-task-complete` only from a **reliable edge**:

1. a known agent was previously observed in explicit working state;
2. a later terminal title contains an explicit completion signal such as `done`, `completed`, or `idle`; **or**
3. the backing terminal process exits/fails after the session had an explicit known-agent working state;
4. that completion generation has not already notified.

Extend parsing so the display state and notification signal are separate, for example:

```ts
type AgentNotificationSignal =
  | "none"
  | "working"
  | "needs-input"
  | "explicit-complete";
```

`parseAgentTitle` can continue returning the state used by the sidebar while also returning a more precise notification signal.

For Claude/Codex/other agents whose title protocol does not expose an explicit completion token, do not invent certainty. Process exit remains a reliable fallback. Later, when rorca gains original-Orca-style agent hook lifecycle events, feed those events into this same coordinator to improve completion coverage without changing native IPC.

### 7.6 Arm/disarm completion generations

Maintain transient coordinator state per stable terminal session:

```text
sessionId -> {
  sawAgentWorking: boolean
  lastAgentType: string | null
  lastCompletionIdentity: string | null
}
```

Rules:

- observing a fresh working state arms the session;
- explicit completion/process exit while armed fires once and disarms;
- repeated title changes in the completed state do not re-fire;
- a new working state re-arms the session for the next task;
- closing a session removes coordinator state.

This is the rorca-equivalent of Orca's more elaborate stale/superseded completion protection.

### 7.7 Agent completion takes precedence over an adjacent bell

Agents commonly emit BEL near the same time they change to a completion title.

Add a short per-session coordination window:

- bell arrives -> queue briefly;
- if a reliable agent completion for the same session arrives during that window, drop the bell and emit only the agent completion;
- otherwise emit the terminal bell when the short window expires.

Keep this delay small enough that a normal terminal bell still feels immediate.

### 7.8 Settings gates

Before any OS dispatch:

```text
settings.enabled must be true
AND source toggle must be true
AND app must not be foreground-focused
```

Source mapping:

```text
agent-task-complete -> settings.agentTaskComplete
terminal-bell       -> settings.terminalBell
test                -> explicit Settings action; bypass source toggles but not backend permission safety
```

Unread marking is an attention feature and should not be lost merely because OS notification delivery is blocked. Decide whether the master switch disables unread markers; recommended behavior is **no**: unread attention tracks background activity independently, while the master switch controls desktop notification delivery/sound.

### 7.9 Dispatch + sound sequence

For an automatic event:

1. pass frontend settings/foreground gates;
2. mark unread if appropriate;
3. call `dispatchNotification(...)`;
4. if result is `submitted`:
   - if `customSoundId === "system"`, do nothing else;
   - if `customSoundId === "custom"`, call `playNotificationSound(...)` with path + volume;
5. if result is `blocked-by-system`, trigger the blocked fallback at most once per application session;
6. if `permission-required`, do not auto-prompt from the background event;
7. log `backend-error` at warning level without crashing the terminal flow.

A failed custom sound must not retroactively mark an otherwise submitted native notification as failed.

---

## 8. Unread State

### 8.1 Store tab unread state once

Add to `WorkspaceState`:

```ts
unreadTabIds: Record<string, true>;
```

Recommended actions:

```text
MARK_TAB_UNREAD
CLEAR_TAB_UNREAD
```

Do not separately store redundant worktree unread booleans. Derive worktree unread state from:

```text
unread tab
-> tab.sessionId
-> session.cwd
-> worktree.path
```

This avoids tab/worktree flags drifting out of sync.

### 8.2 Reducer cleanup rules

- `CLOSE_TAB`: remove that tab ID from `unreadTabIds`;
- `SET_WORKTREES`: remove unread IDs for tabs/sessions that were removed;
- replacement last-tab flow: replacement starts read;
- new tab: starts read.

### 8.3 Clear behavior

Clear a tab's unread state when the user activates it while the rorca window is foreground-focused.

If selecting a worktree activates/creates its tab, clear the resulting visible tab once the activation finishes.

Do not clear every tab in a worktree just because one tab was viewed.

### 8.4 UI indicators

`TabBar.tsx`:

- render a small unread dot/badge on unread inactive tabs;
- use an accessible label/state, not color alone.

`WorktreeList.tsx`:

- accept derived `unreadPaths` / `hasUnread` information;
- render a compact unread dot without replacing the existing agent `StatusDot`.

Pass state through:

```text
App
-> TerminalSplitView
-> TabBar

App
-> Sidebar
-> WorktreeList
```

Keep the existing active/agent/dirty indicators semantically distinct from unread attention.

---

## 9. Settings UI Plan

### 9.1 Add Notifications navigation

Extend `SettingsSection` in `SettingsDialog.tsx` with:

```text
notifications
```

Add a Bell/BellRing icon and place Notifications near General/Terminal.

### 9.2 Notifications pane layout

Recommended order:

1. **Desktop notifications**
   - master switch;
   - explanatory text.
2. **System permission status**
   - Checking;
   - Allowed;
   - Permission required;
   - Blocked by system;
   - Unsupported/unknown.
3. **Notify me when**
   - Agent task completes;
   - Terminal rings bell.
4. **Notification sound**
   - System Default;
   - Custom file;
   - Choose/Replace file;
   - display basename only.
5. **Volume**
   - 0-100 slider for rorca custom/built-in sounds;
   - disabled for `system` because OS controls system notification volume.
6. **Actions**
   - Preview sound;
   - Send test notification;
   - Open System Settings when available.

### 9.3 Master switch behavior

When turned off:

- do not erase child toggle values;
- disable child notification/sound controls visually;
- do not request OS permission.

When turned on:

- persist the app setting first;
- if macOS authorization is `not-determined`, an explicit switch click may call `requestNotificationPermission()`;
- if denied, keep the rorca setting enabled but show the blocked card so re-enabling notifications in System Settings works without another app-setting change.

### 9.4 Permission status hook

Add a small hook such as `useNotificationPermissionStatus(open)` that:

- queries when the Notifications pane becomes active/open;
- refreshes after explicit permission request;
- refreshes on window `focus` while the pane is open;
- does not poll continuously;
- ignores stale async results after unmount/section change.

### 9.5 Test notification flow

`Send test notification`:

1. if permission is not determined, request it because the click is explicit;
2. if denied, show blocked state and do not claim success;
3. submit a test notification;
4. if custom sound is selected and submission succeeds, play the same custom sound using the saved volume;
5. show an inline result/toast that says `submitted`/`blocked` rather than guaranteeing the OS displayed a banner.

### 9.6 Custom audio picker UX

- `Choose custom audio...` -> `pickNotificationAudio()`;
- cancel -> no setting change;
- successful selection -> persist `customSoundId:"custom"`, path, and basename;
- immediately preview using `force:true`;
- if preview reports invalid/missing/decode failure, keep the UI recoverable and offer to choose another file or reset to System Default.

Never render the full selected local filesystem path unless a dedicated advanced detail is intentionally added.

### 9.7 Volume slider

- normalized persisted value: integer 0-100;
- debounce or commit on pointer/key release so dragging does not write localStorage on every pixel if unnecessary;
- preview on commit, not every change, unless performance testing proves continuous preview is acceptable;
- keyboard accessible.

---

## 10. Error and Fallback Behavior

### Notification backend failure

- terminal/agent operation must continue normally;
- emit a warning only;
- never turn an agent completion into an application error dialog.

### macOS blocked notifications

- show a non-modal warning at most once per application session for automatic dispatch;
- message: notifications are blocked by macOS;
- action: Open System Settings;
- the Settings pane always shows the current permission card when opened.

### Permission not yet requested

- automatic background event: do not prompt;
- Settings action: may explicitly prompt.

### Custom audio path missing

- native notification still submits;
- `playSound` returns `not-found`;
- Settings displays recoverable warning the next time sound controls are opened/previewed;
- do not crash and do not repeatedly spam a global toast on every event.

### No audio output device

- native notification still submits;
- `playSound` returns `no-output-device`;
- lazy initialization allows rorca itself to start normally.

### Unsupported platform behavior

- backend returns structured `unsupported`/`unknown` states;
- frontend does not invent success.

---

## 11. File-by-File Implementation Sequence

### Phase 1 — Native backend contract

1. Update `src-tauri/Cargo.toml`:
   - `tauri-plugin-notification = "2"`;
   - `tauri-plugin-dialog = "2"`;
   - `rodio = "0.22"` with only playback/decoder features;
   - target-specific `objc2-user-notifications = "0.3"` for macOS.
2. Add `src-tauri/src/notification/` service modules.
3. Add `src-tauri/src/ipc/notifications.rs` DTOs and command signatures.
4. Update `src-tauri/src/ipc/mod.rs` exports.
5. Update `src-tauri/src/lib.rs` plugin/state/handler registration.
6. Add pure tests for DTO serialization, content formatting, volume/path validation, and permission mapping.

**Acceptance:** backend compiles on the primary development target; pure tests do not show real OS notifications.

### Phase 2 — macOS permission correctness

1. Implement `UNUserNotificationCenter` status query.
2. Implement explicit request authorization.
3. Normalize authorization/alert/sound settings.
4. Implement fixed System Settings opener + fallback.
5. Preflight macOS dispatch against authoritative status.
6. Add fake/native-adapter tests so CI never depends on the host's personal permission state.

**Acceptance:** packaged macOS build can distinguish not-determined, denied, and authorized; returning from System Settings refreshes the UI state.

### Phase 3 — Audio + picker

1. Implement lazy `NotificationAudioPlayer`.
2. Implement supported-format/file-size validation.
3. Implement `cmd_notification_pick_audio`.
4. Implement `cmd_notification_play_sound`.
5. Add backend tests for invalid volume/path/format and a decoder fixture where practical.

**Acceptance:** preview can play supported files at multiple volumes; missing/invalid files return structured failure without application failure.

### Phase 4 — TypeScript bridge + settings model

1. Add notification DTOs.
2. Add typed bridge functions in `ui/src/lib/tauri.ts`.
3. Add `ui/src/lib/notificationSettings.ts`.
4. Add settings normalization/persistence tests.

**Acceptance:** browser preview remains functional; settings synchronize across same-window consumers and storage events.

### Phase 5 — Terminal bell path

1. Add `onBell` to `TerminalPane` and dispose listener correctly.
2. Thread callback through `TerminalSplitView`.
3. Add per-session bell dedupe in coordinator.
4. Add foreground/settings gates.
5. Add unit tests including duplicate visual panes for one session.

**Acceptance:** one BEL from one backend session produces at most one automatic notification event, even if the session is rendered twice.

### Phase 6 — Agent completion path

1. Extend `agentTitle.ts` with notification-specific signal classification.
2. Add per-session working/completion generation tracking.
3. Notify only on explicit completion or qualifying process exit for a previously active known agent.
4. Suppress repeated/stale completion titles.
5. Add bell-vs-completion precedence window.
6. Add table-driven tests for OMO, Claude, Codex, known generic agents, needs-input, explicit completion, and normal shell titles.

**Acceptance:** needs-input/approval does not generate a false `agent-task-complete`; a reliable completion edge emits exactly once.

### Phase 7 — Unread state

1. Add `unreadTabIds` to `WorkspaceState`.
2. Add reducer actions and cleanup.
3. Derive unread worktree paths.
4. Clear on foreground tab activation.
5. Thread state into `TabBar` and `WorktreeList`.
6. Add reducer/component tests.

**Acceptance:** background activity marks the correct tab/worktree; viewing one tab clears only that tab; closing a tab removes stale unread state.

### Phase 8 — Notifications settings UI

1. Add Notifications navigation/heading.
2. Add master/source toggles.
3. Add permission card and focus refresh.
4. Add System Settings action.
5. Add sound selector/custom picker.
6. Add volume slider and preview.
7. Add test notification action.
8. Add accessibility/component tests.

**Acceptance:** all controls persist and stay synchronized with the coordinator; permission error states are actionable and do not block the Settings dialog.

### Phase 9 — Platform sound and package hardening

1. Verify system-default sound behavior on packaged macOS/Windows/Linux target(s).
2. Verify custom playback produces exactly one audible sound.
3. If needed, implement target-specific silent native submission behind the existing backend trait.
4. Validate application name/icon attribution.
5. Validate no CSP/asset-protocol broadening was introduced.

**Acceptance:** automatic custom-sound notifications do not double-play; system sound and custom sound modes are clearly distinct.

---

## 12. Automated Test Strategy

### Rust unit tests

Test pure logic without OS side effects:

- notification title/body formatter;
- control-character stripping and truncation;
- source serialization;
- permission enum -> DTO mapping;
- volume clamp/conversion;
- custom audio extension/size/path validation;
- dispatch preflight decisions;
- backend failure -> structured IPC reason.

### Rust adapter tests

Introduce fake traits/backends for:

- permission provider;
- native notification submitter;
- audio player where command-level behavior is tested.

Test cases:

- denied mac status blocks before submit;
- not-determined returns permission-required without implicit prompt;
- authorized calls submitter once;
- backend submit failure is propagated as structured failure;
- custom audio failure does not change native submission result.

Do not display real notifications in `cargo test`.

### Existing Tauri mock style

rorca already uses `tauri::test::mock_builder()` in backend tests. Follow that pattern for command wiring where practical, but keep actual OS adapters behind fakes because notification plugins/dialogs are inappropriate for deterministic headless tests.

### TypeScript unit tests

`notificationSettings.test.ts`:

- defaults;
- malformed localStorage;
- volume clamp;
- invalid custom path state fallback;
- custom event synchronization;
- storage event synchronization.

`notificationDispatcher.test.ts` / coordinator tests:

- master off;
- source toggle off;
- foreground active -> no OS notification;
- background -> notification;
- unread still marks correctly;
- permission-required -> no automatic prompt;
- blocked fallback only once;
- bell duplicate within dedupe window;
- different sessions are not globally deduped;
- agent explicit completion once;
- repeated completion ignored;
- new working generation can notify again;
- needs-input does not count as task complete;
- process exit fallback only for previously active known agent;
- agent completion suppresses adjacent bell;
- custom sound only after native submission.

### Reducer/component tests

`workspaceStore`:

- mark/clear unread;
- close cleanup;
- worktree sync cleanup.

`TerminalPane` / `TerminalSplitView`:

- bell callback wiring/disposal;
- duplicated visual session does not become two coordinator events.

`SettingsDialog`:

- Notifications navigation;
- master/child disabled behavior;
- permission cards;
- choose custom audio cancellation/success;
- volume clamping/commit;
- preview/test action results;
- open settings action availability.

`TabBar` / `WorktreeList`:

- unread indicator visible and accessible;
- existing active/agent state remains intact.

---

## 13. Manual Platform Verification Matrix

### macOS — packaged `.app` is mandatory

Test on a fresh notification permission state if possible:

1. launch: no unsolicited permission prompt;
2. open Notifications settings pane in rorca;
3. status shows not-determined/permission-required;
4. explicit enable/test shows the macOS authorization prompt;
5. allow -> status becomes authorized;
6. deny -> status becomes blocked;
7. Open System Settings reaches the Notifications settings area;
8. re-enable rorca in System Settings;
9. return to rorca -> focus refresh detects new status;
10. foreground active terminal bell -> no OS notification;
11. background rorca terminal bell -> one OS notification;
12. background reliable agent completion -> one OS notification;
13. adjacent completion + BEL -> one agent completion notification;
14. inactive/background event marks unread;
15. viewing the tab clears unread;
16. System Default uses OS behavior;
17. custom file preview respects 0/25/50/100 volume behavior;
18. custom automatic alert produces no duplicate sound;
19. moved/deleted custom file fails gracefully;
20. test with Focus mode enabled and document the observed custom-application-audio behavior.

Also verify that packaged notifications are attributed to **rorca / `com.orca.lite`**, not Terminal.

### Windows — installed bundle is mandatory

1. install bundle;
2. verify rorca name/icon attribution;
3. background bell/completion;
4. foreground suppression;
5. notification settings disabled behavior;
6. fixed Open Settings target;
7. system vs custom sound;
8. exactly one audible custom alert;
9. missing custom file handling;
10. unread behavior.

Do not treat PowerShell-attributed development toasts as final acceptance.

### Linux

Test at least one supported desktop notification server (preferably GNOME and/or KDE if Linux is a release target):

1. notification submission;
2. no notification daemon / backend unavailable error;
3. foreground suppression;
4. XDG system sound behavior;
5. custom application audio;
6. `suppress-sound` capability if required to prevent duplicates;
7. System Settings action should be hidden/unsupported unless a stable DE-specific route is implemented.

---

## 14. Verification Commands for the Future Implementation

Run from workspace root unless noted.

Backend:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Frontend:

```bash
cd ui
bun run test
bun run build
```

Final source hygiene:

```bash
git diff --check
```

Where CI supports multiple targets, add at least compile/check coverage for macOS and Windows because the permission/system-settings modules are target-specific.

---

## 15. Definition of Done

Native notification support is complete only when all of the following are true:

- [ ] Rust exposes typed notification dispatch, permission status/request, probe/test, System Settings, audio picker, and audio playback commands.
- [ ] Tauri notification integration is registered without unnecessarily exposing broad JavaScript plugin capabilities.
- [ ] macOS authorization is queried through `UNUserNotificationCenter`, not inferred from Tauri desktop `permission_state()`.
- [ ] no permission dialog appears automatically at application startup or from a background event.
- [ ] agent completion notifications are based on a reliable completion edge and are deduped.
- [ ] needs-input/approval states are not mislabeled as task completion.
- [ ] terminal BEL events are wired through xterm and deduped per backend session.
- [ ] an adjacent agent completion + terminal bell yields one user alert.
- [ ] OS notifications are suppressed while the rorca window is foreground-focused.
- [ ] background/inactive terminal events mark unread state.
- [ ] unread state appears on terminal tabs and worktrees and clears predictably when viewed.
- [ ] master, agent-completion, and terminal-bell settings persist and gate delivery.
- [ ] custom audio can be picked, previewed, persisted, volume-controlled, and recovered from a stale path.
- [ ] system sound and custom sound do not double-play.
- [ ] custom audio does not require exposing arbitrary local file paths through Tauri's WebView asset protocol.
- [ ] blocked macOS notifications produce an actionable Settings state/fallback rather than silent failure.
- [ ] browser/Vite preview remains usable without native notification APIs.
- [ ] Rust tests, frontend tests, frontend build, and source hygiene checks pass.
- [ ] packaged macOS acceptance tests pass.
- [ ] installed Windows acceptance tests pass if Windows is a supported release target.
- [ ] custom application-audio behavior under Focus/Do Not Disturb is explicitly accepted/documented or the sound design is changed to comply with the desired OS policy.

---

## 16. Explicit Non-Goals for the First Pass

Keep these out of the initial implementation unless required separately:

- clicking an OS notification to deep-link/focus a specific worktree/tab;
- notification action buttons;
- notification history inside rorca;
- mobile Android/iOS notification channels/permissions;
- per-worktree notification preferences;
- full original-Orca hook lifecycle parity for every agent CLI;
- all original built-in sound presets;
- background notification scheduling when the rorca process is not running.

The proposed IPC and coordinator seams leave room for these later without redesigning the core path.

---

## 17. Main Risks and Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Treating Tauri desktop `permission_state()` as real macOS permission | Blocked users appear enabled | Use `UNUserNotificationCenter` status on macOS |
| Calling `working -> waiting` completion | False agent-complete alerts for prompts/approval | Separate notification signal from display state; require explicit completion or process exit |
| Same backend session rendered twice | Duplicate BEL notifications | Per-session short-window dedupe |
| Agent emits BEL at completion | Two alerts for one event | Completion precedence window |
| Tauri submission reports success before user sees banner | Misleading `delivered` UI | Use `submitted`; document Focus/DND limitations |
| Dev macOS/Windows app identity differs | False permission/branding QA | Require packaged/installed acceptance tests |
| Custom path exposed via WebView asset protocol | Unnecessary local-file attack surface | Decode/play in Rust; keep CSP unchanged |
| Custom file moved/deleted | Repeated runtime failures | Structured `not-found`, recoverable Settings UX |
| No audio output device | App startup/playback failure | Lazy audio initialization and structured failure |
| Native notification plus custom app audio both make sound | Double alert | Verify silent/custom policy per platform; add target-specific silent adapter if needed |
| App audio ignores OS notification sound/Focus policy | Unexpected audible sound | Explicit product decision + Focus QA; use native-only sounds if strict compliance required |
| Continuous permission probing | Battery/noise/test notifications | Query on pane open/focus/explicit actions only |
| Full prompt/tool text in notification | Lock-screen privacy leak | Send only minimal sanitized labels by default |

---

## 18. Validation References

Checked against current project/tooling and current documentation on 2026-08-21:

- Tauri v2 Notifications plugin: https://v2.tauri.app/plugin/notification/
- Tauri notification 2.3.3 desktop source, including desktop permission behavior: https://docs.rs/tauri-plugin-notification/latest/src/tauri_plugin_notification/desktop.rs.html
- Tauri v2 Dialog plugin: https://v2.tauri.app/plugin/dialog/
- Apple `UNNotificationSettings`: https://developer.apple.com/documentation/usernotifications/unnotificationsettings
- Apple `UNUserNotificationCenter.getNotificationSettings`: https://developer.apple.com/documentation/usernotifications/unusernotificationcenter/getnotificationsettings(completionhandler:)
- Rust `objc2-user-notifications`: https://docs.rs/objc2-user-notifications/latest/objc2_user_notifications/
- `rodio` 0.22.x: https://docs.rs/rodio/latest/rodio/
- Desktop Notifications Specification sound/suppress-sound hints: https://specifications.freedesktop.org/notification/latest/hints.html

The implementation should re-check plugin/crate APIs when coding begins if dependency resolution changes from the versions validated here.
