# Code Review — macOS Permissions Subsystem, Settings Panel Integration, Bottom-Right Toast Guidance

- **Reviewer role:** omo-senpi-code-reviewer (task `st_01a06fcd`)
- **Date:** 2026-09-05
- **Repo:** `/Users/indo/code/project/orca-lite` (Ferryx)
- **Base commit:** `fd7f60f` (changes are uncommitted working-tree state)
- **Host used for verification:** macOS 26.6 (25G72), Apple M4 Max, arm64
- **Verdict:** `BLOCK` / `REQUEST_CHANGES`

---

## 0. Evidence Verification (independent re-run)

Every claim in the handoff was re-executed in this session. Nothing was accepted on report alone.

| Claim | Command I ran | Result |
|---|---|---|
| `cargo test --test permissions_contract` 2/2 | `cd src-tauri && cargo test --test permissions_contract` | **CONFIRMED** — `2 passed; 0 failed` |
| `PermissionsSection.test.tsx` 3/3 | `bunx vitest run src/components/settings/PermissionsSection.test.tsx` | **CONFIRMED** — 3 passed |
| `permissionsToast.test.ts` 4/4 | `bunx vitest run src/lib/permissionsToast.test.ts` | **CONFIRMED** — 4 passed (7 total across both files) |
| `tsc` 0 errors | `cd ui && bunx tsc --noEmit` | **CONFIRMED** — exit 0, no output |
| `cargo check` 0 errors | `cd src-tauri && cargo check` | **CONFIRMED** — finished, 10 pre-existing warnings, 0 errors |

**Additional checks the handoff did not run:**

| Check | Result |
|---|---|
| `bunx vitest run src/App.test.tsx` | **12 failed / 76 passed** |
| Attribution of those 12 failures | **PRE-EXISTING.** I reverted only `ui/src/App.tsx` (`git checkout -- ui/src/App.tsx`), re-ran, got the identical 12 failures (session-restore/HMR reconciliation suite). Then restored the file and confirmed the diff is intact (`16 insertions(+), 3 deletions(-)`). **Not caused by this change.** |
| `bunx vitest run src/components/Sidebar.test.tsx src/components/ui/sonner.test.tsx` | Passed |
| `rustfmt --check` on the three new Rust files | **FAILS** — new files are not rustfmt-clean (see LOW-1) |
| `cargo clippy --all-targets` | No new lints attributable to `permissions` |

**Evidence integrity note:** the handoff's test claims are accurate. However, the handoff presented the work as complete without running the app-level test suite or exercising the modified `handleOpenSettings` signature through its existing callers — which is exactly where the blocking defect lives (CRITICAL-1). The claimed evidence is true but **not sufficient** to support the implied "done" verdict.

---

## 1. Skill-Perspective Check (required disclosure)

I attempted to load the `remove-ai-slops` and `programming` skills:

```
ls ~/.omo/skills/*/            -> agy-image, computer-use, general-video, hyperframes*, impeccable, media-use, orca-cli, product-launch-video
find / -maxdepth 6 -type d -name "remove-ai-slops"  -> no results
find <repo> -maxdepth 3 -iname "*slop*"              -> no results
```

**Result: the skills are NOT installed on this workstation and could not be loaded.** Per the task instruction, I applied their documented criteria from the prompt/context instead. Both perspectives were applied and both are violated:

- **`remove-ai-slops` — VIOLATED.** Tautological/self-mirroring tests (MEDIUM-1), tests that only mirror implementation constants (MEDIUM-2), and unnecessary production data extraction that the goal does not require (MEDIUM-3, dead `systemSettingsUrl`).
- **`programming` — VIOLATED.** Implementation-mirroring assertions (MEDIUM-2), brittle prose-pinning assertions on toast copy (MEDIUM-1), dead/unreachable branch retained as if meaningful (MEDIUM-4), and an untyped-in-practice escape hatch where a widened callback signature silently accepts a `MouseEvent` (CRITICAL-1).

---

## 2. Findings by Severity

### CRITICAL

#### CRITICAL-1 — Widening `handleOpenSettings` breaks the sidebar Settings button; Settings dialog renders a blank pane

**Files:** `ui/src/App.tsx:1547-1551`, `ui/src/App.tsx:1761`, `ui/src/components/Sidebar.tsx:402`, `ui/src/components/SettingsDialog.tsx:46`

The change widened the callback:

```ts
// ui/src/App.tsx:1547
const handleOpenSettings = useCallback((section?: SectionId) => {
  preloadSettingsDialog();
  setSettingsInitialSection(section);   // <-- now stores whatever arg 0 is
  setIsSettingsOpen(true);
}, []);
```

It is passed to `Sidebar` at `App.tsx:1761` as `onOpenSettings={handleOpenSettings}`. `Sidebar` forwards it **directly as a DOM click handler**, not wrapped:

```tsx
// ui/src/components/Sidebar.tsx:402
<IconButton label="Settings" size="sm" onClick={onOpenSettings}>
```

React therefore invokes `handleOpenSettings(syntheticMouseEvent)`, and the event object is stored in `settingsInitialSection`. `SettingsDialogBody` seeds state from it:

```tsx
// ui/src/components/SettingsDialog.tsx:46
const [section, setSection] = useState<SectionId>(initialSection ?? "general");
```

Since the value is an object, every `section === "general" | "appearance" | ... ` guard at `SettingsDialog.tsx:134-159` is false and **the entire content pane renders empty**.

TypeScript does not catch this: `Sidebar`'s prop is `onOpenSettings?: () => void` (`Sidebar.tsx:92`), and a `(section?: SectionId) => void` is assignable to `() => void` under bivariant/fewer-params assignability. This is precisely the "untyped escape hatch" the `programming` perspective rejects — the type system was made to lie.

**Empirically reproduced in this session (both probes run under vitest, then deleted):**

1. `IconButton` forwards the event: `received typeof: object  isEvent: true`
2. Rendering `<SettingsDialog open initialSection={<event-like object>} />` yields:
   `PANE_CHILDREN= 0  TEXT= ""` — a completely blank Settings body.

**Impact:** the primary, most-used entry point to Settings (the sidebar gear icon) now opens an empty dialog. This is a user-visible functional regression introduced by this change, in a code path the new tests do not cover. `TabBar.tsx:185` is safe only by accident (`() => onOpenSettings?.()`).

**Required fix (either):**
- Wrap at the call site: `onOpenSettings={() => handleOpenSettings()}` at `App.tsx:1761` (and audit `1820`), **and**
- Harden the sink so this class of bug cannot recur: validate in `SettingsDialogBody`, e.g. seed with `initialSection && SECTION_IDS.includes(initialSection) ? initialSection : "general"`, or narrow the prop types on `Sidebar`/`TerminalSplitView` to `(section?: SectionId) => void` so the mismatch is a type error.

A regression test that clicks the sidebar Settings button and asserts the General section renders should accompany the fix.

---

### HIGH

#### HIGH-1 — Clicking the toast's "Open Settings" action permanently suppresses the toast

**Files:** `ui/src/lib/permissionsToast.ts:48-72`

The toast registers `onDismiss` to persist the "never show again" flag:

```ts
onDismiss: () => { savePermissionsToastDismissed(options.storage); },
```

In sonner 2.0.8 (`ui/node_modules/sonner/dist/index.mjs:875-887`), the **action** button handler calls `deleteToast()` after `action.onClick`, unless the click handler calls `event.preventDefault()`:

```js
onClick: (event) => {
  toast.action.onClick?.call(toast.action, event);
  if (event.defaultPrevented) return;
  deleteToast();
}
```

`deleteToast()` sets `toast.delete`, and the effect at `index.mjs:682-685` then fires `toast.onDismiss`. The handler in `permissionsToast.ts:57-59` does **not** call `preventDefault`.

**Consequence:** a user who clicks "Open Settings" — the cooperative, desired action — is treated identically to a user who clicked "Later". The flag is written to `ferryx.permissions.toast-dismissed`, and because `checkAndPromptPermissions` short-circuits on that flag (`permissionsToast.ts:79-82`) and nothing ever calls `resetPermissionsToastDismissed` outside tests, **the guidance never reappears** — even if the user then closes Settings without granting anything. This directly defeats stated goal #2.

Auto-close after 15s is correctly *not* affected (sonner separates `onAutoClose` from `onDismiss`), so the bug is specific to action/cancel/close-button/swipe paths.

Note the same mechanism means the all-granted branch's `toast.dismiss(PERMISSIONS_TOAST_ID)` (`permissionsToast.ts:87`) also writes the dismissed flag — harmless in outcome, but it confirms the handler cannot distinguish intents.

**Required fix:** move the persistence out of `onDismiss` into only the paths that mean "stop nagging me" — i.e. keep it in `cancel.onClick` and add it to an explicit close-button path — and have `action.onClick` call `event.preventDefault()` (or otherwise flag the click) so the "Open Settings" path does not persist suppression. Add a test asserting that the action path leaves the flag unset.

#### HIGH-2 — New module silently reverses a deliberate, documented product decision about opening macOS System Settings

**Files:** `src-tauri/src/permissions/mod.rs:223-260`, vs. `src-tauri/src/notification/mod.rs:23-33`

The pre-existing notification path explicitly refuses to launch System Settings on macOS, with a comment recording why:

```rust
// src-tauri/src/notification/mod.rs:24-32
#[cfg(target_os = "macos")]
{
    // On macOS, running `open x-apple.systempreferences:...` or opening System Settings
    // when the app is unbundled or unnotified causes unwanted disruptive UI popups.
    // Explicitly report unsupported to avoid unexpected System Settings window launches.
    OpenSystemSettingsResult { opened: false, reason: Some("opening system settings is disabled in dev/standalone mode".into()) }
}
```

The new `open_system_settings_for_target` does exactly what that comment forbids — including for `target == "notifications"`, which is the *same* destination:

```rust
// src-tauri/src/permissions/mod.rs:246-250
let opened = crate::util::no_window_command("open").arg(url).status()...
```

There are now two IPC commands (`cmd_notification_open_system_settings` and `cmd_permissions_open_settings("notifications")`) with **opposite behavior for the same user intent**. Whichever policy is correct, one of them is wrong, and the divergence is undocumented.

Aggravating factor: `src-tauri/tauri.conf.json` has no `signingIdentity`/notarization config in `bundle.macOS` (only `minimumSystemVersion` and `hardenedRuntime`), so the unbundled/unnotarized scenario that motivated the original guard is live for local and dev builds.

**Required:** decide the policy explicitly. Either (a) relax the notification guard and delete the stale comment, or (b) gate the new opener behind the same bundle-identity check (`notification::permission::macos::has_bundle_identity()`), and record the rationale in code. Do not leave two contradictory behaviors.

#### HIGH-3 — Two distinct `OpenSystemSettingsResult` types are glob-re-exported from `crate::ipc`

**Files:** `src-tauri/src/permissions/mod.rs:33-40`, `src-tauri/src/notification/model.rs:281`, `src-tauri/src/ipc/mod.rs:53,55`

`ipc/mod.rs` does `pub use notifications::*;` and `pub use permissions::*;`. Both submodules re-export a type named `OpenSystemSettingsResult`, but with **different shapes**: the notification one has `{opened, reason}`; the new one adds `target`. It compiles today only because each `ipc` submodule imports its own concretely, and glob-vs-glob ambiguity is deferred in Rust until the ambiguous name is actually used unqualified from `crate::ipc`.

This is a latent landmine: the first `use crate::ipc::OpenSystemSettingsResult;` anywhere will produce an `E0659` ambiguity error, and the two types serialize to different JSON, so any future consumer confusing them is a silent contract bug on the frontend. Note `ui/src/lib/types.ts` declares a **single** `OpenSystemSettingsResult` with an optional `target` — the frontend already collapses two backend types into one, which is how such bugs become invisible.

**Required:** rename one (e.g. `PermissionsOpenSettingsResult`) or stop glob-re-exporting these DTOs from `ipc`.

---

### MEDIUM

#### MEDIUM-1 — `permissionsToast.test.ts` pins prose and asserts implementation constants (slop)

**File:** `ui/src/lib/permissionsToast.test.ts:127-129`

```ts
expect(callArgs[0]).toContain("Permissions");
expect(callArgs[1].id).toBe(PERMISSIONS_TOAST_ID);
expect(callArgs[1].description).toContain("Photo Library");
```

- `callArgs[0]).toContain("Permissions")` and `description).toContain("Photo Library")` pin **user-facing prose**. Both perspectives explicitly forbid this: prose is not a machine-consumed value, and rewording the copy — a zero-risk change — breaks the test.
- `callArgs[1].id).toBe(PERMISSIONS_TOAST_ID)` compares the implementation's own exported constant against itself. It is tautological: it cannot fail for any change that keeps using the constant, and it does not encode a contract.

The valuable assertions in this test are `prompted === true`, `mockToast.warning` was called, and `action.onClick()` invokes `onOpenSettings` — those should stay. **MEDIUM, not HIGH**: the tests currently pass and cause no correctness failure today; the cost is future churn.

Additionally, this suite would not have caught HIGH-1, because it invokes `action.onClick()` directly rather than through sonner's button, bypassing the `deleteToast()`→`onDismiss` chain. That is a mock that does not preserve the behavior under test.

#### MEDIUM-2 — `permissions_contract.rs` is largely tautological

**File:** `src-tauri/tests/permissions_contract.rs:8-64`

The bulk of `permissions_contract_reports_valid_statuses` asserts that a value of a four-variant `enum` is one of those four variants:

```rust
assert!(matches!(fda_direct,
    PermissionStatus::Granted | PermissionStatus::Denied
    | PermissionStatus::NotDetermined | PermissionStatus::Unsupported));
```

This is exhaustive over the enum and **cannot fail** — the type system already guarantees it. It is repeated four times. `assert!(!status.platform.is_empty())` is similarly guaranteed by three `cfg` branches that all assign a non-empty literal.

The only assertions with real content are the macOS URL-anchor checks (`Privacy_AllFiles`, `Privacy_Accessibility`, `preference.notifications`) and `permissions_open_settings_rejects_malformed_target`, which does verify a genuine branch. Those two are worth keeping; the enum-membership blocks are noise that inflates the perceived coverage of a subsystem whose real risk surface (FDA canary correctness, FFI, cross-platform `all_granted`) is untested.

**MEDIUM**: no correctness failure today, but the suite creates a false confidence signal — the handoff cited "2/2 passed" as evidence of correctness when the tests assert almost nothing.

Missing tests that *would* have value and are absent:
- `open_system_settings_for_target` returns `opened: false` with a reason on Linux/non-macOS (currently untested; the `cfg` blocks are unexercised on CI runners).
- `all_granted` semantics when a platform reports `Unsupported` (see MEDIUM-5).

#### MEDIUM-3 — `systemSettingsUrl` is dead production data on both sides of the IPC boundary

**Files:** `src-tauri/src/permissions/mod.rs:22,186-193,204,211,218`; `ui/src/lib/types.ts:654`

The backend constructs three URL strings, allocates them into `Option<String>`, serializes them across IPC on every status poll, and the frontend declares the field in `PermissionItemStatus`. Grepping non-test frontend sources:

```
grep -rn "systemSettingsUrl" ui/src --include=*.tsx --include=*.ts | grep -v test
-> ui/src/lib/types.ts:654    (the declaration only)
```

**Nothing consumes it.** The UI always routes through `openPermissionsSystemSettings(target)`, which re-derives the identical URLs server-side in `open_system_settings_for_target`. So the URLs are duplicated in two places in `permissions/mod.rs` (lines 186-193 and 226-234) — the exact "unnecessary production data extraction the goal does not require" the `remove-ai-slops` perspective flags, plus a copy-paste divergence risk.

Compounding: the only substantive assertions in `permissions_contract.rs` test *this dead field*. The test suite's most meaningful checks validate data no one reads.

**Recommendation:** delete `system_settings_url` from the DTO and the TS type, and retarget the contract test at `open_system_settings_for_target`'s returned/derived URL instead (a single source of truth).

#### MEDIUM-4 — Dead `EPERM` arm and over-broad error→`Denied` mapping in the FDA canary

**File:** `src-tauri/src/permissions/mod.rs:50-67`

```rust
match std::fs::read_dir(&safari_dir) {
    Ok(_) => PermissionStatus::Granted,
    Err(err) if err.raw_os_error() == Some(libc::EPERM) => PermissionStatus::Denied,
    Err(_) => PermissionStatus::Denied,
}
```

The guarded `EPERM` arm and the catch-all arm return the **same value**, so the guard is unreachable in effect — it reads as if it distinguishes TCC denial from other failures, but it does not. Either drop the arm, or make it meaningful by mapping non-`EPERM` errors to `NotDetermined` (which is the honest answer: `EMFILE`, `ENOTDIR`, `EIO` are not permission signals, and reporting them as `Denied` will show the user a false "Required" badge and re-trigger the toast).

Secondary concern on the same block: the canary is gated on `path.exists()`. On macOS, `stat()` of `~/Library/Safari` is not TCC-restricted (only the read is), so `exists()` is a reasonable gate — but on a machine where neither `~/Library/Safari` nor `~/Library/Suggestions` exists (fresh account, Safari never launched, MDM-managed), the function returns `NotDetermined`, `fda_granted` is `false`, and the user is shown a permanent "Required" badge and a toast they cannot satisfy. Worth a comment at minimum; ideally add a third canary such as `~/Library/Application Support/com.apple.TCC` or `/Library/Application Support/com.apple.TCC/TCC.db`.

I could not empirically differentiate the denial errno on this host: my verification shell already holds Full Disk Access, so `opendir` on `~/Library/Safari`, `~/Library/Messages`, and `/Library/Application Support/com.apple.TCC` all returned success. **This finding is reasoned from code and macOS semantics, not from an observed denial** — flagged explicitly as unverified-by-execution.

#### MEDIUM-5 — `Unsupported` is coerced to "granted", making `all_granted` misleading off-macOS

**File:** `src-tauri/src/permissions/mod.rs:172-176`

```rust
let fda_granted = matches!(fda_status, PermissionStatus::Granted | PermissionStatus::Unsupported);
let ax_granted  = matches!(ax_status,  PermissionStatus::Granted | PermissionStatus::Unsupported);
```

Treating "this concept does not exist on this OS" as "granted" is defensible for the toast's purpose, but it silently discards information and the DTO exposes both `status: "unsupported"` and `granted: true` for the same item — a self-contradictory payload the frontend must not misread. The UI at `PermissionsSection.tsx:96-104` renders `status?.fullDiskAccess.granted ? "Granted" : "Required"`, so a Windows/Linux user sees a green **"Granted"** badge for Full Disk Access, a macOS-only concept, alongside a header that reads "Configure macOS permissions…". This is confusing on non-macOS.

Because the panel is unconditionally reachable on all platforms (`SettingsDialog.tsx:125` renders the nav button with no platform guard) and every string in `PermissionsSection.tsx` is macOS-specific ("macOS System Settings", "Photo Library", "click the '+' button and select Ferryx from Applications"), this is a genuine cross-platform robustness gap.

**Recommendation:** render `status === "unsupported"` as a distinct neutral badge ("Not applicable"), and hide or platform-gate the Permissions nav entry when `platform !== "macos"`.

#### MEDIUM-6 — Notification status is served from a 10s cache, so "Refresh Status" can lie

**Files:** `src-tauri/src/permissions/mod.rs:154`, `src-tauri/src/notification/permission.rs:73,209-222`

`get_system_permissions_status` calls `platform_permission_provider().status()`, which on macOS returns a value cached for `STATUS_CACHE_TTL = 10s`. The Settings panel's explicit **"Refresh Status"** button (`PermissionsSection.tsx:73-81`) therefore may return a stale notification authorization for up to 10 seconds after the user toggles it in System Settings — precisely the moment the user presses Refresh. The panel's own step-4 instruction ("Return to Ferryx and click 'Refresh Status' to confirm") walks the user straight into this.

FDA and Accessibility are uncached and correct. Only notifications are affected.

**Recommendation:** expose a cache-bypassing path for user-initiated refresh, or reduce the perceived-correctness gap by noting the delay in the UI.

#### MEDIUM-7 — `getSystemPermissionsStatus` omits the `isTauri()` guard used by its neighbors

**File:** `ui/src/lib/tauri.ts:678-680`

Sibling bridges in this file consistently guard (`tauri.ts:145,161,172,230,240,292,320`). The three new bridges do not. In the browser/remote build the invoke rejects, which `checkAndPromptPermissions`'s bare `catch { return false; }` swallows — so the observable outcome is currently benign. But it relies on an unrelated try/catch rather than the module's own convention, and `PermissionsSection` would silently render an all-`null` panel if it ever became reachable from the remote app. Today it is not: `RemoteApp.tsx` renders no `SettingsDialog` and does not call `initPermissionsToast` (verified by grep).

Low blast radius, but it is an inconsistency in a boundary module where consistency is the whole point.

---

### LOW

#### LOW-1 — New Rust files are not rustfmt-clean

`rustfmt --check --edition 2021 src/permissions/mod.rs src/ipc/permissions.rs tests/permissions_contract.rs` reports diffs in `permissions/mod.rs` (lines ~172, ~186) and `permissions_contract.rs` (lines ~25, ~52), including a **trailing-whitespace-only line** at `permissions_contract.rs:25`. The repo has pre-existing fmt drift elsewhere, so this is not a new policy break, but new files should land formatted.

#### LOW-2 — `void 0;` used as an empty-catch placeholder

`ui/src/lib/permissionsToast.ts:34,42` and `ui/src/components/settings/PermissionsSection.tsx:51,57` use `void 0;` inside `catch`. The adjacent, pre-existing `updateToast.ts:36` uses a comment (`// ignore quota/disabled storage`) for the identical situation, which is clearer about intent. Cosmetic, but it makes swallowed errors look accidental rather than deliberate.

#### LOW-3 — `PermissionsSection` duplicates the backend's description strings as fallbacks

`PermissionsSection.tsx:112-113`, `160-161`, `196-197` each hardcode a `??` fallback that is a verbatim copy of the backend `description` in `permissions/mod.rs:200-217`. Two sources of truth for the same copy; they will drift. The fallback only renders while `status === null` (initial load or fetch failure), where a skeleton or a short generic line would serve better.

#### LOW-4 — `initPermissionsToast`'s 1800 ms delay is an unexplained magic number

`ui/src/lib/permissionsToast.ts:108`. A one-line comment explaining what the delay is waiting for (window paint? avoid competing with the update toast?) would prevent someone "cleaning it up" later. Note this is startup-scheduling code, not a test, so it does not violate the no-fixed-sleeps-in-tests rule.

#### LOW-5 — Out-of-scope change bundled into a reviewed file

`src-tauri/src/ipc/mod.rs` adds **both** `permissions` and `native_menu` module declarations/re-exports. `native_menu` belongs to a separate in-flight change (`src-tauri/src/ipc/native_menu.rs` is untracked; its commands are registered at `lib.rs:796-799`, outside the permissions block). It is not a defect, but it muddies the reviewable boundary of this change and should be split before commit.

---

## 3. Section-by-Section Assessment

### 3.1 Architecture & Design Quality — adequate with a duplication problem

The layering is right and idiomatic for this codebase: pure logic in `permissions/mod.rs`, thin `#[tauri::command]` wrappers in `ipc/permissions.rs`, `cfg`-gated platform branches, serde DTOs with `camelCase` for the wire and `snake_case` for the enum. It mirrors the existing `notification` module closely enough to be immediately legible.

The failure is at the seams: a duplicated `OpenSystemSettingsResult` type (HIGH-3), duplicated URL literals within one file (MEDIUM-3), duplicated description copy across the FFI boundary (LOW-3), and a duplicated-but-contradictory "open notification settings" behavior (HIGH-2). None is individually fatal; together they mean a future maintainer has to know which of two copies is authoritative in four separate places.

### 3.2 macOS TCC & Security Correctness — URL schemes verified correct; canary is the weak point

**URL schemes: verified working on macOS Sonoma/Sequoia/26 — this is the strongest part of the change.** I did not take this on faith. All three URLs use legacy `com.apple.preference.*` pane identifiers, which Ventura+ System Settings routes through an explicit compatibility map declared in each settings extension's `Info.plist`:

```
/System/Library/ExtensionKit/Extensions/SecurityPrivacyExtension.appex/Contents/Info.plist
  EXAppExtensionAttributes.SettingsExtensionAttributes:
    allowsXAppleSystemPreferencesURLScheme = true
    legacyBundleIdentifier = "com.apple.preference.security"

/System/Library/ExtensionKit/Extensions/NotificationsSettings.appex/Contents/Info.plist
    allowsXAppleSystemPreferencesURLScheme = true
    legacyBundleIdentifier = "com.apple.preference.notifications"
```

The `Privacy_AllFiles` anchor is also present in `SecurityPrivacyExtension.appex/Contents/Resources/*.lproj/PrivacySecurity.searchTerms`. So `com.apple.preference.security?Privacy_AllFiles`, `...?Privacy_Accessibility`, and `com.apple.preference.notifications` all resolve. No change needed here.

**FDA canary:** the `~/Library/Safari` → `~/Library/Suggestions` fallback is the conventional approach and is reasonable. Its weaknesses are MEDIUM-4 (dead `EPERM` arm; all errors mapped to `Denied`) and the no-canary-exists case. **Safety-wise the canary is sound**: it only ever calls `read_dir` on two fixed, non-user-controlled paths under `$HOME`, discards the results without reading file contents, and never enumerates or touches Photo Library / Documents / Downloads — so the canary itself cannot trigger the TCC prompts the feature exists to prevent. There is no path traversal or injection surface.

**Injection surface of the opener:** `open_system_settings_for_target` matches `target` against a closed allowlist and returns an error for anything else *before* constructing a command — the `target` string never reaches the shell. `permissions_contract.rs:66-70` covers this. Correct by construction; `no_window_command("open").arg(url)` uses no shell.

**On the stated goal:** granting FDA does suppress per-folder TCC prompts, so the mechanism is right. But note the change ships **no** `NSPhotoLibraryUsageDescription` / folder usage strings and no signing/notarization config in `tauri.conf.json` — the feature is pure user guidance, not a code-level fix, and the underlying root cause (documented in `docs/MACOS_TCC_DOCUMENTS_ACCESS_ROOT_CAUSE_AND_FIX_2026-09-04.md` as `find /` from an agent subprocess plus a `repoRoot: "/"` watcher) was already addressed separately. That framing should be explicit somewhere, because "Grant FDA and the prompts stop" is only true for prompts caused by legitimate traversal.

### 3.3 FFI & Memory Safety — correct; no leaks or UB found

`request_accessibility` (`permissions/mod.rs:92-146`) is sound. Detailed check:

- `kCFStringEncodingUTF8 = 0x08000100` — **verified against the SDK header** (`CFString.h:113`). The literal `0x0800_0100` is correct.
- `CFStringCreateWithCString` returns +1; the null check at line 126 is present; `CFRelease(key)` at line 144 balances it.
- `CFDictionaryCreate` returns +1 and is released at 141-143 (null-guarded). No leak on the success path.
- `kCFTypeDictionaryKeyCallBacks` / `kCFTypeDictionaryValueCallBacks` are passed by address as `*const c_void` — correct; CF retains the key/value via those callbacks, so releasing `key` after dictionary creation is right.
- `keys`/`values` are stack arrays that outlive the `CFDictionaryCreate` call. Fine.
- `numValues: isize` correctly models `CFIndex` on 64-bit.
- `AXIsProcessTrusted`/`AXIsProcessTrustedWithOptions` declared as returning `bool` — matches `Boolean` (unsigned char) in the Apple ABI on arm64/x86_64 for this purpose; consistent with widespread practice.
- Creating a *new* `CFString` equal to `kAXTrustedCheckOptionPrompt` rather than linking the exported constant works, because `kCFTypeDictionaryKeyCallBacks` hashes/compares with `CFHash`/`CFEqual`, not pointer identity. Slightly unusual but correct.
- One early-return path (`CString::new` failure at line 120) is unreachable in practice since the literal has no interior NUL; harmless.

**No leak, no double-free, no use-after-free.** One design note: this function shows a **system modal prompt** and is invoked from `handleRequestAccessibility` (`PermissionsSection.tsx:55-63`) — intentional and user-initiated, so appropriate.

### 3.4 IPC, Concurrency & Thread Safety — correct

All three commands route through `run_blocking` (`ipc/mod.rs:24-32` → `tokio::task::spawn_blocking`), which is the right discipline and matters materially here: `get_system_permissions_status` transitively calls the macOS notification provider, which blocks on an `mpsc::sync_channel` with a **5-second** `CALLBACK_TIMEOUT` (`notification/permission.rs:72,196-203`), and `open_system_settings_for_target` calls blocking `Command::status()`. Neither may run on the async runtime; neither does.

`request_accessibility` on a background thread showing the AX prompt is acceptable — `AXIsProcessTrustedWithOptions` dispatches the prompt itself and does not require the main thread.

No shared mutable state is introduced. The one shared piece, `STATUS_CACHE`, is pre-existing and `Mutex`-guarded with `if let Ok(...)` (poison-tolerant). No new lock ordering. The commands are stateless and safe under concurrent invocation. No `unwrap()` on lock acquisition anywhere in the new code.

### 3.5 Cross-Platform Robustness — the weakest area

Compiles cleanly on all three targets (`cfg` coverage is complete: macOS / Windows / else). But:

- Windows supports only `notifications`; `full_disk_access` and `accessibility` fall to `"target unsupported on Windows"`, yet the UI still renders both cards with clickable "Open System Settings" buttons that do nothing visible — `handleOpenSettings` (`PermissionsSection.tsx:47-53`) ignores the returned `OpenSystemSettingsResult` entirely and shows no feedback on `opened: false`. A silent no-op button is worse than a disabled one.
- Linux supports nothing; every button is a silent no-op.
- Combined with MEDIUM-5, non-macOS users see "Granted" badges for macOS-only concepts.
- Every visible string in the panel is macOS-specific, but the panel is unconditionally reachable on all platforms.
- The `cfg(windows)` and `cfg(not(macos/windows))` branches of `open_system_settings_for_target` are **entirely untested** — `permissions_contract.rs` has no non-macOS assertions.

This is a MEDIUM cluster rather than HIGH only because macOS is the stated target of the feature.

### 3.6 React Lifecycle, State Management & UX

**Positives.** `PermissionsSection` is clean: `fetchStatus` is a stable `useCallback` with `[]` deps, the `useEffect` dep array is honest, and `finally` reliably clears both `loading` and `refreshing`. `Toaster` is confirmed `position="bottom-right"` (`ui/src/components/ui/sonner.tsx:54`), matching the update-toast pattern the user asked to mirror. `initPermissionsToast` returns a real cleanup that clears the timer, and `App.tsx`'s effect returns it directly — correct under StrictMode double-mount (the app runs in StrictMode per `main.tsx:36,46`), so the worst case is one extra scheduled-then-cancelled timer, not a duplicate toast (and `id: PERMISSIONS_TOAST_ID` would dedupe anyway).

**Problems.**
- CRITICAL-1 (blank Settings pane from the sidebar) and HIGH-1 (action click permanently suppresses the toast) are both in this area.
- `PermissionsSection` has **no unmount guard**: if the dialog closes while `getSystemPermissionsStatus()` is in flight (plausible — the macOS notification query can take up to 5s), `setStatus`/`setLoading`/`setRefreshing` fire on an unmounted component. React 18 no longer warns, so this is benign today, but the 5s window makes it likely rather than theoretical. An `ignore` flag or `AbortController`-style guard is warranted.
- `handleOpenSettings` discards `OpenSystemSettingsResult.opened`/`.reason` (`PermissionsSection.tsx:47-53`). The backend went to the trouble of returning structured failure information and the UI throws it away — on any platform or failure mode, the button is a silent no-op.
- The panel does not auto-refresh on window focus. The documented flow is "leave the app → toggle in System Settings → come back", which is exactly a focus event; requiring a manual Refresh click (which is itself subject to MEDIUM-6's 10s cache) is a missed, cheap UX win.
- The toast only ever fires once per app launch, ~1.8s after mount. If permissions are revoked mid-session there is no re-check. Acceptable scope-wise, worth noting.

---

## 4. Ranked Recommendations

### P0 — Blockers (must fix before approval)

1. **CRITICAL-1** — Fix the sidebar Settings regression. Wrap the call site (`App.tsx:1761`, audit `1820`) *and* validate `initialSection` against the known `SectionId` set in `SettingsDialogBody` (`SettingsDialog.tsx:46`) so a bad value cannot blank the pane. Add a regression test that clicks the sidebar gear and asserts the General section renders.
2. **HIGH-1** — Stop persisting the dismissed flag when the user clicks "Open Settings". Move persistence out of `onDismiss` into the explicit "Later"/close paths, and `event.preventDefault()` in `action.onClick`. Add a test asserting the action path leaves `ferryx.permissions.toast-dismissed` unset.
3. **HIGH-2** — Resolve the contradiction with `notification/mod.rs:24-32`. Either relax that guard (and delete its now-false comment) or gate the new opener behind `has_bundle_identity()`. Record the decision in code.
4. **HIGH-3** — Rename `permissions::OpenSystemSettingsResult` (or drop it from the `ipc` glob re-export) so the two same-named, different-shaped DTOs cannot collide.

### P1 — Important

5. **MEDIUM-3** — Delete the dead `system_settings_url` field from the DTO and `ui/src/lib/types.ts`; retarget the contract test at the single source of truth in `open_system_settings_for_target`.
6. **MEDIUM-5 / §3.5** — Platform-gate the Permissions nav entry (or render `unsupported` as a neutral "Not applicable" badge and disable dead buttons) so Windows/Linux users are not shown macOS-only "Granted" claims and silent no-op buttons.
7. **MEDIUM-4** — Remove the unreachable `EPERM` arm or make it meaningful; map non-`EPERM` errors to `NotDetermined` rather than `Denied`; add a third canary or a comment for the "no canary exists" case.
8. **MEDIUM-2 / MEDIUM-1** — Delete the tautological enum-membership assertions and the prose-pinning assertions; add the tests that would actually catch regressions (non-macOS `open_system_settings_for_target` behavior; toast action path not persisting dismissal).
9. **§3.6** — Surface `OpenSystemSettingsResult.opened === false` in the UI instead of discarding it; add an unmount guard to `PermissionsSection`'s async `setState`.

### P2 — Minor / Nitpick

10. **MEDIUM-6** — Bypass the 10s notification status cache for user-initiated "Refresh Status".
11. **MEDIUM-7** — Add `isTauri()` guards to the three new bridges for consistency with the rest of `tauri.ts`.
12. **§3.6** — Refresh permission status on window focus.
13. **LOW-1** — Run `rustfmt` on the three new Rust files (includes a trailing-whitespace line).
14. **LOW-3** — Drop the duplicated description fallbacks in `PermissionsSection`; use a skeleton while `status === null`.
15. **LOW-2 / LOW-4** — Replace `void 0;` with intent comments; document the 1800 ms delay.
16. **LOW-5** — Split the unrelated `native_menu` module registration out of `ipc/mod.rs` before committing this change.

---

## 5. What This Change Gets Right

Worth stating plainly, because the blockers are narrow and the foundation is not:

- The CoreFoundation FFI is genuinely correct — balanced retain/release, correct encoding constant verified against the SDK header, correct callback vtables. This is the part most likely to be wrong in a change like this, and it is right.
- The macOS Settings URL schemes are correct and I verified them against the actual `Info.plist` compatibility declarations on this machine rather than assuming.
- `run_blocking` discipline is correct on all three commands, which matters because one of them can block for 5 seconds.
- The settings-target allowlist makes command injection structurally impossible, and it is the one thing the Rust tests actually verify.
- The FDA canary cannot itself trigger the TCC prompts the feature exists to suppress.
- `cfg` coverage compiles cleanly on all three platforms.
- The toast genuinely lands bottom-right, mirroring the update-toast pattern the user asked for, with a correct cleanup function that is StrictMode-safe.
- No pre-existing tests were deleted, skipped, or weakened; the 12 `App.test.tsx` failures are pre-existing and I proved it by reverting and re-running.

The change is close. Four fixes — two of which are one-liners — take it from BLOCK to a reasonable merge.

---

## 6. Verdict

- **codeQualityStatus:** `BLOCK`
- **recommendation:** `REQUEST_CHANGES`

One CRITICAL and three HIGH findings remain, of which CRITICAL-1 and HIGH-1 are empirically reproduced user-visible defects, not theoretical concerns. Per the review policy, any remaining CRITICAL or HIGH mandates `REQUEST_CHANGES`.
