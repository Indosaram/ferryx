# Windows Black Screen Root Cause — 2026-08-30

## Verdict

Two independent defects invalidated the earlier Windows completion claim.

1. `bun tauri dev` launched over `maho-win` OpenSSH runs in Windows Session 0. The resulting `ferryx.exe` process had `MainWindowHandle=0`, no visible top-level HWND, no Ferryx-owned `msedgewebview2.exe`, no request to Vite, and no Tauri page-load event. `GUI=1` was only a process-count proxy and never proved that an interactive desktop window existed.
2. In a real interactive launch, mounting any native terminal activated an unconditional CSS block that forced `html`, `body`, `#root`, the application root, `main`, and all terminal layout ancestors to `transparent !important` on every platform. That transparency is intentional for macOS native composition, but on opaque Windows WebView2 it leaves the application chrome unpainted and exposes the WebView background as a uniform black client area.

## Runtime evidence

The instrumented SSH reproduction used the required command from the actual checkout:

```powershell
cd C:\Users\sook\ferryx-winbuild\orca-lite
bun tauri dev
```

Observed after the Rust build completed:

- `ferryx.exe` GUI process existed in Session 0.
- `Get-Process ferryx` reported `MainWindowHandle=0` and an empty `MainWindowTitle`.
- Enumerating visible top-level windows found no HWND owned by the Ferryx GUI PID.
- The Ferryx process tree contained only the GUI process, daemon, and daemon console; it contained no WebView2 process.
- No system `msedgewebview2.exe` command line referenced Ferryx.
- Vite received no `/` request from Ferryx.
- Tauri `on_page_load` emitted no event.
- Frontend boot tracing emitted no `index`, `main`, import, render, or paint event.

Therefore SSH process counts cannot be used as Ferryx desktop visual evidence.

## Hypothesis matrix

| Hypothesis | Evidence | Verdict |
| --- | --- | --- |
| React import or mount failed | No Vite navigation, page-load event, or WebView2 process existed in the SSH launch, so React was never reached there | Not the SSH-layer failure |
| Native terminal child HWND covered the complete WebView | No Ferryx window/WebView/native child HWND was created in Session 0; source geometry also confines the child to frontend-provided pane bounds | Rejected for SSH reproduction; still subordinate to the CSS defect for interactive runs |
| Global native-terminal transparency blanked Windows app chrome | `ui/src/index.css` forced every application root surface transparent whenever `native-terminal-pane` existed; the native pane mounts unconditionally; symptom changed from desktop-through transparency to opaque black after the window opacity override | Confirmed source-level interactive rendering defect |
| Vite orphan/stale server caused the black window | A correct owned Vite lifecycle was separately verified, while the visible black symptom persisted | Insufficient and not the root fix |

## Fix

Commit `75c4d36 fix(windows): keep application chrome opaque`:

- `ui/src/main.tsx` marks only macOS documents with `html.platform-macos` using the existing platform detector.
- `ui/src/index.css` scopes the native-terminal transparency block under `html.platform-macos`.
- Windows and Linux retain their normal opaque `body`, `#root`, application chrome, and `main` backgrounds.
- `ui/src/nativeTerminalPlatformTransparency.test.ts` prevents root-level native-terminal transparency from becoming platform-global again.

## RED to GREEN

RED:

```text
native terminal platform transparency > keeps root application surfaces opaque outside macOS
expected index.css not to contain an unscoped html:has([data-testid="native-terminal-pane"])
```

GREEN on the actual Windows checkout:

```text
Test Files  2 passed (2)
Tests       5 passed (5)
```

The full frontend type-check and build also passed:

```text
tsc && vite build
1840 modules transformed
built in 8.91s
```

An unrelated pre-existing Windows test-harness issue remains in `appearanceThemeContract.test.ts`: its `readFileSync(new URL(..., import.meta.url))` path is not a filesystem URL under Windows Vitest. The new regression test and native terminal tests pass independently, and the full TypeScript/Vite build is green.

## Dirty-work preservation

- Actual checkout before this fix: 59 dirty entries.
- Actual checkout after applying the three production fix files: 62 dirty entries.
- Temporary instrumentation in `ui/index.html`, `ui/vite.config.ts`, and `src-tauri/src/lib.rs` was removed.
- No destructive Git operation was used.

## Required visual QA

OpenSSH cannot satisfy the visual gate because it launches in Session 0. The user must run the debug app from an interactive Windows PowerShell window:

```powershell
cd C:\Users\sook\ferryx-winbuild\orca-lite
bun tauri dev
```

The user must confirm that the interactive window:

1. is opaque;
2. displays Ferryx sidebar, tabs, and terminal content instead of a uniform black client;
3. has no duplicated native menu;
4. exposes the terminal shell selector under the tab-bar `+` menu.
