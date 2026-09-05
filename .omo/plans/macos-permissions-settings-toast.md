# Plan: macOS Permissions Management (Full Disk Access, Accessibility, Notifications) in Settings & Toast Guidance

## Summary
Implement a unified macOS system permissions subsystem for Ferryx:
1. Prevents "Ferryx would like to access your Photo Library" and other folder TCC alerts by requesting Full Disk Access (FDA).
2. Adds a comprehensive Permissions section in Settings for Full Disk Access, Accessibility, and Notifications with status badges, explanations, and direct System Settings openers.
3. Provides a bottom-right toast (via Sonner) guiding users to grant required permissions linked directly to the Settings panel.

## Architecture
- **Backend (`src-tauri`)**:
  - `src/permissions/mod.rs`: Cross-platform permission checker (safe macOS FDA canary check via non-prompting `~/Library/Safari` read attempt, Accessibility via `AXIsProcessTrusted`, Notifications via `NotificationPermissionProvider`).
  - `src/permissions/macos.rs`: macOS-specific URL scheme openers (`x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles`, etc.).
  - `src/ipc/permissions.rs`: Tauri commands (`cmd_permissions_get_status`, `cmd_permissions_open_settings`, `cmd_permissions_request_accessibility`).
- **Frontend (`ui`)**:
  - `lib/types.ts` & `lib/tauri.ts`: Typed permission models and IPC invoke bridge.
  - `lib/storageKeys.ts`: Storage key for dismissed permissions toast.
  - `components/settings/PermissionsSection.tsx`: Settings view showing status, descriptions, and action buttons.
  - `components/SettingsDialog.tsx`: Nav button with `Shield` icon, supports `initialSection` prop.
  - `lib/permissionsToast.ts`: Bottom-right toast guiding user to grant permissions with "Open Settings" button.
  - `App.tsx`: Wires permissions toast initialization and initial section navigation.

## Verification Scenarios
- SC1: Backend tests for permission models and commands.
- SC2: Component test for `PermissionsSection.tsx`.
- SC3: Unit test for `permissionsToast.ts`.
- SC4: Full frontend build and backend cargo check.
