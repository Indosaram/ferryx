# Audit: notification
Repo: /Users/indo/code/project/orca-lite
Scanned:
- src-tauri/src/notification/service.rs
- src-tauri/src/notification/audio.rs
- src-tauri/src/notification/model.rs
- src-tauri/src/notification/permission.rs
- src-tauri/src/notification/mod.rs
- src-tauri/src/notification/tests.rs
- src-tauri/src/ipc/notifications.rs
- src-tauri/src/ipc/preferences.rs
- src-tauri/src/terminal/preferences.rs
- src-tauri/Cargo.toml
Date: 2026-08-22

## Findings

### F-notification-01
- Severity: Low
- File: src-tauri/src/terminal/preferences.rs:252
- Mechanism: `load_terminal_preferences()` attempts to run `try_load_from_ghostty_cli()` first on every preferences load. Spawning `Command::new("ghostty").arg("+show-config").output()` invokes a full external GUI application process (fork/exec, dynamic link loading, font and graphic subsystem init) which introduces 50ms to 200ms of synchronous subprocess execution before checking fast static config files on disk.
- Hot path: no
- Suggested fix: Check local filesystem candidate paths (`~/.config/ghostty/config`, macOS Application Support) first and only fall back to `ghostty +show-config` if files are absent, or cache the loaded preferences in memory.
- Write scope: src-tauri/src/terminal/preferences.rs
- RED proof:
  ```rust
  pub fn load_terminal_preferences() -> TerminalPreferences {
      if let Some(cli_config) = try_load_from_ghostty_cli() {
          return TerminalPreferences::imported(cli_config, PathBuf::from("ghostty"));
      }

      let candidates = ghostty_config_candidates();
      if let Some(path) = candidates.iter().find(|path| path.is_file()) {
          return load_terminal_preferences_from_path(path);
      }
      ...
  }
  ```
  Spawning the Ghostty process precedes file system inspection on every invocation.

### F-notification-02
- Severity: Low
- File: src-tauri/src/notification/permission.rs:88
- Mechanism: `MacosPermissionProvider::status()` executes `query_settings()`, which constructs an ObjC block and blocks the calling thread on `rx.recv_timeout(CALLBACK_TIMEOUT)` with a 5-second timeout waiting for `UNUserNotificationCenter.getNotificationSettingsWithCompletionHandler`. Because `NotificationService` is reconstructed per command invocation in `service_for(&app)`, every notification dispatch and status query issues a synchronous cross-process OS permission query without any in-process caching.
- Hot path: no
- Suggested fix: Cache the authoritative `NotificationPermissionStatusDto` in the permission provider or application state with an invalidation policy (e.g. refresh on explicit permission request or probe, or short TTL), avoiding blocking cross-thread synchronization on every dispatch.
- Write scope: src-tauri/src/notification/permission.rs, src-tauri/src/notification/service.rs
- RED proof:
  ```rust
  let (tx, rx) = mpsc::sync_channel::<NotificationPermissionStatusDto>(1);
  let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
      let status = read_settings(unsafe { settings.as_ref() });
      let _ = tx.try_send(status);
  });

  let dispatched = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
      let center = UNUserNotificationCenter::currentNotificationCenter();
      center.getNotificationSettingsWithCompletionHandler(&handler);
  }));
  ...
  match rx.recv_timeout(CALLBACK_TIMEOUT) {
      Ok(status) => Some(status),
      Err(_) => {
          tracing::warn!("UNUserNotificationCenter settings query timed out");
          None
      }
  }
  ```
  Every `service.dispatch(&request)` calls `permissions.status()`, executing this synchronous block-and-wait sequence.

## Non-findings / accepted

- **PTY backlog parsing / agent output polling**: The notification backend does not poll agent output or scan terminal PTY backlogs. Notifications are driven by discrete IPC invocations from the frontend (`cmd_notification_dispatch`) containing pre-structured fields (`agent_label`, `worktree_label`, `terminal_title`).
- **Audio decode offloading**: Audio playback (`cmd_notification_play_sound`) delegates file loading, path canonicalization, and Rodio decoding to `run_blocking` (`tokio::task::spawn_blocking`), preventing runtime reactor or UI thread blocking.
- **Rapid sound deduplication**: `NotificationAudioPlayer::play` evaluates `self.is_deduped()` (400ms dedupe window) before performing path validation, disk I/O, or audio decoding, avoiding wasted decode work on rapid notification bursts.
- **Audio player lock hold times**: `NotificationAudioPlayer` acquires `self.output` lock only after the audio file is fully opened and decoded into memory. Device sink creation occurs lazily once during initial playback and the lock is released immediately after detaching the player mixer.
- **Text sanitization overhead**: `sanitize_text` and `truncate_text` execute single-pass character iterators over bounded strings (maximum title 80 chars, body 160 chars) with initial string allocation capacity matching input length.

## Scan coverage

- `src-tauri/src/notification/service.rs`: Preflight validation, backend submission dispatch, probe delivery, service struct lifecycles.
- `src-tauri/src/notification/audio.rs`: `NotificationAudioPlayer` mutex locking, audio sink initialization, file validation, volume clamping, 400ms dedupe window.
- `src-tauri/src/notification/model.rs`: Text sanitization, title/body truncation, audio path and size limits (20MB threshold), data transfer objects.
- `src-tauri/src/notification/permission.rs`: Cross-platform permission queries, macOS `UNUserNotificationCenter` dispatch and block synchronization, fallback providers.
- `src-tauri/src/notification/mod.rs`: Module exports and system settings opening commands across macOS, Windows, and Linux.
- `src-tauri/src/notification/tests.rs`: Unit tests verifying sanitization, truncation, wire serialization, and volume calculation.
- `src-tauri/src/ipc/notifications.rs`: IPC command handlers (`cmd_notification_dispatch`, `cmd_notification_play_sound`, `cmd_notification_pick_audio`, etc.) and `run_blocking` usage.
- `src-tauri/src/ipc/preferences.rs`: `cmd_terminal_preferences` IPC command wrapping terminal configuration retrieval in `run_blocking`.
- `src-tauri/src/terminal/preferences.rs`: Ghostty configuration parser, `ghostty +show-config` CLI execution, candidate path discovery.
- `src-tauri/Cargo.toml`: Dependency verification for `rodio`, `tauri-plugin-notification`, `objc2`, and `tokio`.
