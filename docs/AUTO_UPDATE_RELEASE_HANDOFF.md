# Auto-update release handoff

Created: 2026-08-26T14:31:35.916963

The local implementation and actual plugin/bundle verification are complete. A public tag release is
intentionally **not** created yet: it changes remote GitHub state, must be approved explicitly, and
requires a commit that contains only the updater work.

## Required local-build release gate

Before creating any tag:

1. Stage only the updater paths/hunks below.
2. Re-run the verification commands in `docs/evidence/auto-update/`.
3. Commit with an atomic subject such as `feat(updater): add signed in-app updates`.
4. Push that commit without a tag. The next unused release tag is `v2026.08.26.1`.
5. Create an isolated temporary worktree at that commit, stamp only the temporary copy with
   `scripts/sync-version.mjs`, and build the signed macOS updater bundle locally.
6. Run `node scripts/assert-updater-archive-layout.mjs <bundle>.app.tar.gz` after every macOS
   archive is produced. If it reports an AppleDouble `._*` entry, discard the archive. If a
   notarization step requires a manual repackage, use `COPYFILE_DISABLE=1 tar` and create a fresh
   `.sig` for that replacement archive before generating the manifest.
7. Generate `latest.json`, then use `gh release create v2026.08.26.1 --target <commit>` to upload
   the bundle, sibling `.sig`, and manifest directly. Do not push a tag or invoke GitHub Actions.
8. Verify the GitHub Release contains `latest.json`, updater bundles, and their `.sig` files before
   asking a user to perform the desktop procedure in `docs/AUTO_UPDATE_MANUAL_E2E.md`.

## Whole-file updater paths

These files can be staged as whole files without mixing known unrelated work:

```text
.github/workflows/release.yml
src-tauri/Cargo.toml
src-tauri/capabilities/default.json
src-tauri/gen/schemas/capabilities.json
ui/bun.lock
ui/src/lib/updater.ts
scripts/sync-version.mjs
scripts/sync-version.test.mjs
scripts/build-latest-json.mjs
scripts/build-latest-json.test.mjs
scripts/release-workflow.test.mjs
scripts/fixtures/updater/Ferryx.app.tar.gz
scripts/fixtures/updater/Ferryx.app.tar.gz.sig
src-tauri/tests/updater_config_contract.rs
src-tauri/tests/updater_endpoint_contract.rs
ui/src/lib/updater.test.ts
ui/src/components/SettingsDialog.update.test.tsx
docs/AUTO_UPDATE_IMPLEMENTATION.md
docs/AUTO_UPDATE_MANUAL_E2E.md
docs/evidence/auto-update/
```

## Files requiring hunk-level staging

The following files contain pre-existing non-updater changes in the current dirty worktree. Stage
only the updater hunks identified below; do not stage the entire file blindly.

- `.gitignore`: stage `.env` and `.env.local`; leave the pre-existing `ui/coverage/` hunk alone.
- `src-tauri/tauri.conf.json`: stage `plugins.updater` and `bundle.createUpdaterArtifacts`; leave
  the pre-existing `bundle.resources` hunk alone.
- `src-tauri/src/lib.rs`: stage only `tauri_plugin_process::init()` and the `#[cfg(desktop)]`
  `tauri_plugin_updater::Builder` registration; leave the Cmd+digit work alone.
- `ui/package.json`: stage `@tauri-apps/plugin-updater` and `@tauri-apps/plugin-process`; leave
  `test:coverage` / `@vitest/coverage-v8` changes alone.
- `ui/src/components/SettingsDialog.tsx`: stage the `Download` / `updater` imports and the
  `SoftwareUpdateCard`/`updateStatusMessage` block only.
- `docs/CROSS_PLATFORM_RELEASE_GUIDE.md`: stage section 6 (`자동 업데이트`) plus the changed
  release-checklist bullets only.

## Current local proof

- actual updater crate E2E: `updater_endpoint_contract` 3/3 green;
- config/capability contract: 5/5 green;
- signed aarch64 `.app.tar.gz` plus `.sig` generated and verified with the configured public key;
- final UI: 87 files / 753 tests green;
- release scripts/workflow: 19 tests green, YAML and step-order gate green;
- full Rust command still ends at the same 3 pre-existing daemon socket contract failures; its
  library target and updater contracts are green. Full transcripts live in
  `docs/evidence/auto-update/`.

## Mixed-file diff inventory

### `.gitignore`

```diff
diff --git a/.gitignore b/.gitignore
index 5578b07..10bd800 100644
--- a/.gitignore
+++ b/.gitignore
@@ -9,6 +9,7 @@ node_modules/
 ui/node_modules/
 dist/
 ui/dist/
+ui/coverage/
 
 # OS
 .DS_Store
@@ -16,3 +17,7 @@ Thumbs.db
 
 # Agent session runtime state
 .omo/
+
+# Local secrets (updater signing key)
+.env
+.env.local
```
### `src-tauri/tauri.conf.json`

```diff
diff --git a/src-tauri/tauri.conf.json b/src-tauri/tauri.conf.json
index 5207d3a..0b1284f 100644
--- a/src-tauri/tauri.conf.json
+++ b/src-tauri/tauri.conf.json
@@ -30,8 +30,20 @@
       "csp": "default-src 'self' http://127.0.0.1:5173 http://localhost:5173; script-src 'self' 'unsafe-inline' 'unsafe-eval' http://127.0.0.1:5173 http://localhost:5173; style-src 'self' 'unsafe-inline' http://127.0.0.1:5173 http://localhost:5173; img-src 'self' data: http://127.0.0.1:5173 http://localhost:5173; font-src 'self' data: http://127.0.0.1:5173 http://localhost:5173; connect-src 'self' ipc: http://ipc.localhost ws://127.0.0.1:5173 ws://localhost:5173 http://127.0.0.1:5173 http://localhost:5173; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
     }
   },
+  "plugins": {
+    "updater": {
+      "endpoints": [
+        "https://github.com/Indosaram/ferryx/releases/latest/download/latest.json"
+      ],
+      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDEwQzk5REI1QzI1QzY3Q0IKUldUTFoxekN0WjNKRU8zWGhqWlo2VXEzclF0RXoyRmJCY2Z4eGwvK2FGbE5LSmVwcW9RTmoyWm0K",
+      "windows": {
+        "installMode": "passive"
+      }
+    }
+  },
   "bundle": {
     "active": true,
+    "createUpdaterArtifacts": true,
     "targets": [
       "dmg",
       "app",
@@ -52,6 +64,9 @@
       "icons/icon.icns",
       "icons/icon.ico"
     ],
+    "resources": {
+      "../ui/dist": "ui/dist"
+    },
     "windows": {
       "allowDowngrades": true,
       "webviewInstallMode": {
```
### `src-tauri/src/lib.rs`

```diff
diff --git a/src-tauri/src/lib.rs b/src-tauri/src/lib.rs
index a2e3f56..2333a24 100644
--- a/src-tauri/src/lib.rs
+++ b/src-tauri/src/lib.rs
@@ -136,6 +136,48 @@ pub fn is_unshifted_cmd_w_characters(characters: Option<&str>) -> bool {
 #[cfg(target_os = "macos")]
 pub const ANSI_KEY_CODE_W: u16 = 13;
 
+#[cfg(target_os = "macos")]
+pub const ANSI_KEY_CODE_1: u16 = 18;
+#[cfg(target_os = "macos")]
+pub const ANSI_KEY_CODE_5: u16 = 23;
+#[cfg(target_os = "macos")]
+pub const ANSI_KEY_CODE_9: u16 = 25;
+
+/// macOS keyCodes for the top-row digits 1..9, in order.
+#[cfg(target_os = "macos")]
+const ANSI_DIGIT_KEY_CODES: [u16; 9] = [18, 19, 20, 21, 23, 22, 26, 28, 25];
+
+/// Returns the pressed digit when the event is an unshifted Cmd+1..9.
+///
+/// The standard macOS Window menu owns Cmd+digit, so AppKit consumes those
+/// events and the webview never receives a `keydown`. The local key monitor uses
+/// this to claim them for worktree selection, exactly as it already does for
+/// unshifted Cmd+W.
+#[cfg(target_os = "macos")]
+pub fn unshifted_cmd_digit(
+    flags: objc2_app_kit::NSEventModifierFlags,
+    characters: Option<&str>,
+    key_code: u16,
+) -> Option<u8> {
+    if !is_unshifted_cmd_w_modifiers(flags) {
+        return None;
+    }
+
+    if let Some(digit) = characters
+        .and_then(|chars| chars.chars().next())
+        .and_then(|char| char.to_digit(10))
+    {
+        if (1..=9).contains(&digit) {
+            return Some(digit as u8);
+        }
+    }
+
+    ANSI_DIGIT_KEY_CODES
+        .iter()
+        .position(|candidate| *candidate == key_code)
+        .map(|index| index as u8 + 1)
+}
+
 #[cfg(target_os = "macos")]
 pub fn is_unshifted_cmd_w(
     flags: objc2_app_kit::NSEventModifierFlags,
@@ -164,6 +206,11 @@ fn install_macos_key_monitor<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::R
                 let _ = window.emit("menu_close_tab", ());
             }
             ptr::null_mut()
+        } else if let Some(digit) = unshifted_cmd_digit(flags, chars_str.as_deref(), key_code) {
+            if let Some(window) = app_handle.get_webview_window("main") {
+                let _ = window.emit("menu_select_worktree", digit);
+            }
+            ptr::null_mut()
         } else {
             event_ptr.as_ptr()
         }
@@ -188,6 +235,7 @@ pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Build
     // Lazily initialized: a machine with no audio output device must still launch.
     let notification_audio = Arc::new(NotificationAudioPlayer::new());
     let browser_manager = Arc::new(browser::BrowserManager::new());
+    let browser_cli_manager = Arc::clone(&browser_manager);
     #[cfg(feature = "native-terminal")]
     let native_terminal_surface_host = NativeTerminalSurfaceHostState::default();
 
@@ -207,22 +255,31 @@ pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Build
             install_app_menu(app)?;
             #[cfg(target_os = "macos")]
             install_macos_key_monitor(app)?;
+            ipc::browser_cli::start_browser_cli_server(
+                app.handle().clone(),
+                Arc::clone(&browser_cli_manager),
+            )?;
             Ok(())
         })
         // Rust-side plugins only. The frontend uses rorca's own typed commands,
         // so no broad JavaScript guest capability needs to be granted.
         .plugin(tauri_plugin_notification::init())
         .plugin(tauri_plugin_dialog::init())
+        .plugin(tauri_plugin_process::init())
         .manage(daemon_client)
         .manage(remote_manager)
         .manage(workspace_registry)
         .manage(notification_audio)
         .manage(browser_manager);
 
+    #[cfg(desktop)]
+    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
+
     #[cfg(feature = "native-terminal")]
     let builder = builder.manage(native_terminal_surface_host);
 
     builder.invoke_handler(tauri::generate_handler![
+        cmd_switch_debug_log,
         cmd_terminal_output_channel,
         cmd_terminal_spawn,
         cmd_terminal_attach,
@@ -285,6 +342,8 @@ pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Build
         cmd_browser_set_zoom,
         cmd_browser_focus,
         cmd_browser_get_state,
+        cmd_browser_automation_snapshot,
+        cmd_browser_automation_act,
         cmd_browser_close,
         cmd_browser_list,
         cmd_browser_open_external,
@@ -312,6 +371,47 @@ mod tests {
         }
     }
 
+    #[test]
+    #[cfg(target_os = "macos")]
+    fn cmd_digit_is_claimed_for_worktree_selection() {
+        use objc2_app_kit::NSEventModifierFlags;
+
+        // The standard macOS Window menu claims Cmd+1..9, so AppKit swallows the
+        // keystroke before the webview sees a keydown. The local monitor must
+        // recognize it and hand it to the frontend instead.
+        for (digit, key_code) in [(1u8, ANSI_KEY_CODE_1), (5u8, ANSI_KEY_CODE_5), (9u8, ANSI_KEY_CODE_9)] {
+            let label = digit.to_string();
+            assert_eq!(
+                unshifted_cmd_digit(
+                    NSEventModifierFlags::Command,
+                    Some(label.as_str()),
+                    key_code,
+                ),
+                Some(digit),
+                "Cmd+{digit} must map to worktree index {digit}"
+            );
+        }
+
+        // Rejected: any extra modifier, or a non-digit key.
+        assert_eq!(
+            unshifted_cmd_digit(
+                NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
+                Some("1"),
+                ANSI_KEY_CODE_1,
+            ),
+            None,
+        );
+        assert_eq!(
+            unshifted_cmd_digit(NSEventModifierFlags::Command, Some("w"), ANSI_KEY_CODE_W),
+            None,
+        );
+        // Rejected: no Command modifier at all (plain "1" typed into a terminal).
+        assert_eq!(
+            unshifted_cmd_digit(NSEventModifierFlags::empty(), Some("1"), ANSI_KEY_CODE_1),
+            None,
+        );
+    }
+
     #[test]
     #[cfg(target_os = "macos")]
     fn test_macos_cmd_w_shortcut_predicate_boundaries() {
```
### `ui/package.json`

```diff
diff --git a/ui/package.json b/ui/package.json
index c055ac1..54ddfe8 100644
--- a/ui/package.json
+++ b/ui/package.json
@@ -7,6 +7,7 @@
     "dev": "vite",
     "build": "tsc && vite build",
     "test": "vitest run --maxWorkers=1",
+    "test:coverage": "vitest run --maxWorkers=1 --coverage",
     "preview": "vite preview"
   },
   "dependencies": {
@@ -15,6 +16,8 @@
     "@dnd-kit/utilities": "^3.2.2",
     "@tauri-apps/api": "^2.0.0",
     "@tauri-apps/plugin-dialog": "^2",
+    "@tauri-apps/plugin-process": "^2",
+    "@tauri-apps/plugin-updater": "^2",
     "@types/qrcode": "^1.5.6",
     "lucide-react": "^0.475.0",
     "qrcode": "^1.5.4",
@@ -28,6 +31,7 @@
     "@types/react": "^18.3.18",
     "@types/react-dom": "^18.3.5",
     "@vitejs/plugin-react": "^4.3.4",
+    "@vitest/coverage-v8": "3.2.7",
     "autoprefixer": "^10.4.20",
     "clsx": "^2.1.1",
     "jsdom": "^26.0.0",
```
### `ui/src/components/SettingsDialog.tsx`

```diff
diff --git a/ui/src/components/SettingsDialog.tsx b/ui/src/components/SettingsDialog.tsx
index 344d68d..64ff2e1 100644
--- a/ui/src/components/SettingsDialog.tsx
+++ b/ui/src/components/SettingsDialog.tsx
@@ -7,6 +7,7 @@ import {
   Check,
   CheckCircle2,
   ChevronDown,
+  Download,
   ExternalLink,
   FolderGit2,
   Globe,
@@ -37,6 +38,16 @@ import {
 import { useGeneralSettings } from "../lib/generalSettings";
 import { useNotificationSettings } from "../lib/notificationSettings";
 import { SHORTCUTS, isMacShortcutPlatform, shortcutAliasesLabels, shortcutLabel } from "../lib/shortcuts";
+import {
+  checkForUpdate,
+  downloadAndInstallUpdate,
+  getCurrentVersion,
+  getUpdateStatus,
+  relaunchApp,
+  subscribeUpdateStatus,
+  type UpdateStatus,
+} from "../lib/updater";
+
 import {
   createPairingCode,
   detectAgents,
@@ -291,11 +302,106 @@ function GeneralSettings() {
             />
           </SettingRow>
         </div>
+        <SoftwareUpdateCard />
       </div>
     </section>
   );
 }
 
+function updateStatusMessage(status: UpdateStatus): string {
+  switch (status.state) {
+    case "checking":
+      return "Checking for updates…";
+    case "available":
+      return `Version ${status.version} is available.`;
+    case "downloading":
+      return `Downloading version ${status.version}…`;
+    case "downloaded":
+      return `Version ${status.version} is ready to install.`;
+    case "error":
+      return `Update failed: ${status.error}`;
+    case "idle":
+      return "Ferryx is up to date.";
+  }
+}
+
+function SoftwareUpdateCard() {
+  const [status, setStatus] = useState<UpdateStatus>(() => getUpdateStatus());
+  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
+
+  useEffect(() => subscribeUpdateStatus(setStatus), []);
+
+  useEffect(() => {
+    let active = true;
+    void getCurrentVersion().then((version) => {
+      if (active) setCurrentVersion(version);
+    });
+    return () => {
+      active = false;
+    };
+  }, []);
+
+  const busy = status.state === "checking" || status.state === "downloading";
+  const percent = Math.round((status.downloadProgress ?? 0) * 100);
+
+  return (
+    <div className="rounded-lg border border-border bg-card p-4">
+      <h3 className="text-[12px] font-semibold">Software Update</h3>
+      <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
+        Current version {currentVersion ?? "unknown"}. Updates are signed and verified before they install.
+      </p>
+      <p
+        data-testid="settings-update-status"
+        aria-live="polite"
+        className="mt-2 text-[12px] leading-5 text-foreground"
+      >
+        {updateStatusMessage(status)}
+      </p>
+      {status.state === "downloading" || status.state === "downloaded" ? (
+        <div
+          role="progressbar"
+          aria-label="Update download progress"
+          aria-valuemin={0}
+          aria-valuemax={100}
+          aria-valuenow={percent}
+          className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-accent"
+        >
+          <div className="h-full rounded-full bg-foreground transition-all" style={{ width: `${percent}%` }} />
+        </div>
+      ) : null}
+      <div className="mt-3 flex flex-wrap items-center gap-2">
+        <button
+          type="button"
+          onClick={() => void checkForUpdate()}
+          disabled={busy}
+          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
+        >
+          <RotateCw className="size-3" />
+          Check for Updates
+        </button>
+        <button
+          type="button"
+          onClick={() => void downloadAndInstallUpdate()}
+          disabled={status.state !== "available"}
+          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
+        >
+          <Download className="size-3" />
+          Download Update
+        </button>
+        <button
+          type="button"
+          onClick={() => void relaunchApp()}
+          disabled={status.state !== "downloaded"}
+          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
+        >
+          <RotateCcw className="size-3" />
+          Install and Relaunch
+        </button>
+      </div>
+    </div>
+  );
+}
+
 export function AppearanceSettings() {
   const { settings, updateSettings, resetSettings } = useAppearanceSettings();
```
### `docs/CROSS_PLATFORM_RELEASE_GUIDE.md`

```diff
diff --git a/docs/CROSS_PLATFORM_RELEASE_GUIDE.md b/docs/CROSS_PLATFORM_RELEASE_GUIDE.md
index 39f1f4e..6711af8 100644
--- a/docs/CROSS_PLATFORM_RELEASE_GUIDE.md
+++ b/docs/CROSS_PLATFORM_RELEASE_GUIDE.md
@@ -206,15 +206,25 @@ bun run --cwd site build
 
 ---
 
-## 6. End-to-End Release Checklist
+## 6. 자동 업데이트
+
+릴리스 워크플로는 태그 `vYYYY.MM.DD[.N]`을 `YYYY.MMDD.revision` SemVer로 변환해 번들과
+`latest.json`에 함께 주입한다. 예: `v2026.08.26.1`은 `2026.826.1`이다.
+
+- GitHub Secrets에 `TAURI_SIGNING_PRIVATE_KEY`와 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`를 등록한다.
+- `src-tauri/tauri.conf.json`의 공개키와 위 개인키는 같은 키쌍이어야 한다.
+- CI는 `.app.tar.gz`/`.nsis.zip`/`.AppImage.tar.gz`와 `.sig`를 수집하고, `latest.json`을 릴리스
+  자산으로 업로드한다. 서명이 없는 updater 아티팩트는 매니페스트에 넣지 않는다.
+
+## 7. End-to-End Release Checklist
 
 Before releasing a new version of Ferryx:
 
-- [ ] **1. Bump Version**: Update version in `package.json`, `ui/package.json`, `site/package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
+- [ ] **1. Verify Version Mapping**: `node scripts/sync-version.mjs --tag "$TAG" --conf <temp-conf> --cargo <temp-cargo>`가 유효한 SemVer를 출력하는지 확인한다. CI가 실제 번들 파일의 버전을 주입하므로 저장소 버전 파일을 수동 편집하지 않는다.
 - [ ] **2. Verify Rust Backend**: Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`. This includes `native_terminal_renderer_contract`, which needs real GPU hardware and is therefore skipped by CI — running it locally before a release is the only coverage it gets.
-- [ ] **3. Verify Frontend UI**: Run `bun run --cwd ui build` and `bun test --cwd ui`.
+- [ ] **3. Verify Frontend UI**: Run `bun run --cwd ui build` and `bun run --cwd ui test`.
 - [ ] **4. Verify Landing Page**: Run `bun test --cwd site` and `bun run --cwd site build`.
 - [ ] **5. Validate GitHub Workflows**: Ensure `.github/workflows/release.yml` and `build-test.yml` syntax are valid.
 - [ ] **6. Tag & Push**: Execute `TAG="v$(date +%Y.%m.%d)"; git tag -a "$TAG" -m "Release $TAG"` and `git push origin "$TAG"`. This is the only way to publish a release.
-- [ ] **7. Verify Release Artifacts**: Confirm all `.dmg`, `.exe`, `.msi`, `.msix`, `.AppImage`, `.deb`, and `SHA256SUMS.txt` are published on GitHub Releases.
+- [ ] **7. Verify Release Artifacts**: Confirm all `.dmg`, `.exe`, `.msi`, `.msix`, `.AppImage`, `.deb`, `SHA256SUMS.txt`, `latest.json`, updater bundles, and matching `.sig` files are published on GitHub Releases.
 - [ ] **8. Submit to Microsoft Store**: Upload the generated `Ferryx_<version>_x64.msix` to Microsoft Partner Center.
```


## Remote-state recheck (2026-08-26T14:33:03.803456)

- `refs/tags/v2026.08.26.1` remains absent at `origin`.
- `gh release view v2026.08.26.1` reports no release.
- The public `releases/latest/download/latest.json` endpoint does not yet serve this implementation;
  it can only be validated after the approved updater-only commit and date-tag push create a new
  release.
- Both required secret names remain registered in GitHub Actions.

**Publication boundary:** creating the updater-only commit, pushing it, and uploading
`v2026.08.26.1` with `gh release create` create durable repository and public Release state.


## Blocked audit (2026-08-26T14:35:32.483443)

This historical blocked audit was superseded when the user approved a local build and direct release
upload without GitHub Actions. It remains here only as the original handoff context.


## Repeated blocked audit (2026-08-26T14:37:00.309532)

The local-release procedure replaces the former tag-push route. The release must be built in an
isolated temporary worktree and published through the GitHub Release API without Actions.


## Approval wait check (2026-08-26T14:38:52.804822)

`git diff --check` remains clean; the worktree still has unrelated changes, and `v2026.08.26.1` remains absent at origin. No additional local action is safe or necessary.
