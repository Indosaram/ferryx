# Windows UI Missing Coverage Gap Report

**Audit Date**: 2026-09-13 | **Baseline HEAD**: `b7ad4516` | **Deliverable**: `docs/evidence/windows-review-20260913/gap-ui.md`
**Scope**: Missing-source-coverage audit for `coverage.md` Section G UI gaps:
1. `ui/src/components/settings/SshSection.tsx` and `ui/src/components/settings/SshSection.test.tsx`
2. `ui/src/components/RemoteDirectoryPicker.tsx` and `ui/src/components/RemoteDirectoryPicker.test.tsx` (with `ui/src/lib/remoteDirectories.ts`)
3. Remote UI/input shared Windows-reachable callers: `ui/src/remote/RemoteTerminal.tsx`, `ui/src/remote/RemoteTerminal.contract.test.tsx`, `ui/src/remote/RemoteTerminalGestures.test.tsx`, `ui/src/remote/RemoteApp.tsx`, `ui/src/lib/remoteClient.ts`
4. `site/src/components/FeatureVisuals.tsx`
5. `site/src/components/ui/PlatformIcons.tsx`
6. `site/src/lib/downloads.ts` and `site/src/lib/downloads.test.ts` (with `site/src/components/DownloadMenu.tsx`)

---

## 1. Executive Summary & Audit Scope

- **Objective**: Complete missing source review across the six UI and public web components identified as explicit coverage gaps in `docs/evidence/windows-review-20260913/coverage.md`.
- **Operating Constraints**:
  - Audit only; no implementation, no source edits, no commits, and no test suite or daemon execution performed in this node.
  - Foreign uncommitted work (native-image/worktree additions and `App.tsx`/`App.test.tsx` close confirmation) preserved completely.
  - Strict policy preservation: existing Ctrl+W close, Ctrl+V paste, and Ctrl+click link policies are preserved. Speculative clipboard activation failure claims, 140px titlebar spacing claims, and prose-pinning test proposals are excluded.
- **Sole Confirmed Repair Candidate**:
  - `GAP-UI-06` (`RemoteTerminal.tsx:490-498`): Wheel `deltaMode` normalization. Handler divides `event.deltaY / 20` unconditionally, omitting line mode (`DOM_DELTA_LINE`), page mode (`DOM_DELTA_PAGE`), pixel mode (`DOM_DELTA_PIXEL`), and fractional delta accumulation contracts.
- **Stable Finding Classification & Reconciled Boundary Summary**:
  - `GAP-UI-01` (UX Affordance / Validation Limitation): `SshSection.tsx:727-739` provides a free-text input without a file picker for `identityFile`. Call-chain tracing proves manual form submission bypasses the OpenSSH config parser (`src-tauri/src/ssh/config.rs:72-76`, `FSSH-03`) and saves directly to the JSON store via `cmd_ssh_update_host`. This is a UX limitation, not a parser defect.
  - `GAP-UI-02` (Test Gap): `SshSection.test.tsx:784-848` exercises custom SSH config path overrides exclusively with POSIX paths (`/tmp/...`), omitting Windows drive-letter paths (`C:\...`).
  - `GAP-UI-03` (Autocomplete Policy / UX Enhancement): In `RemoteDirectoryPicker.tsx:104-111`, entering bare `C:` yields `lastSlash = -1` and `parent = null`, dispatching `load(null, "C:")` which lists home directory `~` with prefix `"C:"`. In Win32 semantics, `C:` denotes a drive-relative path, not unconditionally drive root (`C:\`). Autocomplete expansion to `C:\` is a convenience heuristic, not a defect fix.
  - `GAP-UI-04` (Design Styling Consideration): In `RemoteDirectoryPicker.tsx:193-212`, option items display `entry.name` plus `<span aria-hidden="true" className="ml-auto text-muted-foreground">/</span>`. This is a decorative directory indicator, not path text corruption or a functional defect.
  - `GAP-UI-05` (Test Gap): `RemoteDirectoryPicker.test.tsx:60-67` lacks tests for bare drives (`C:`), drive roots (`C:\`), drive transitions (`D:\`), and UNC paths.
  - `GAP-UI-06` (Confirmed Source Branch Defect): `RemoteTerminal.tsx:490-498` wheel calculation omits `deltaMode` normalization. Sole confirmed repair candidate.
  - `GAP-UI-07` (Runtime Unknown / Unproved): In `RemoteTerminal.tsx:708-713, 738-747`, when the surface container is focused via keyboard navigation, pressing AltGr or non-ASCII printable keys calls `inputSinkRef.current?.focus()` without calling `event.preventDefault()`. Source comments intentionally rely on the browser routing subsequent `beforeinput`/`input` events to the newly focused textarea. Source handler (`onPointerDown`) focuses the sink on pointer events, but actual browser default sequencing across various pointer and keyboard events is unknown at runtime without live receipts.
  - `GAP-UI-08` (Marketing Illustration / Not Defective): `FeatureVisuals.tsx:27-48` terminal demo displays `"renderer: wgpu / Metal"` inside macOS window chrome. It illustrates a macOS terminal session; no prose-pinning tests are authorized.
  - `GAP-UI-09` & `GAP-UI-10` (Test Gaps): Zero unit tests exist for `FeatureVisuals.tsx` or `PlatformIcons.tsx`.
  - `GAP-UI-11` (Site Navigation Policy / Not Shown Defective): `downloads.ts:29` and `DownloadMenu.tsx:74-85` route to Microsoft Store search in the same tab. Current site behavior is not shown defective; direct installer exclusion is confirmed project release policy.
  - `GAP-UI-12` (Test Gap): `downloads.test.ts:75-96` tests `detectUserPlatform` only against legacy `navigator.platform = 'Win32'`, omitting modern `userAgentData.platform = 'Windows'` Client Hints.

---

## 2. Stable Finding ID & Disposition Mapping

To prevent ledger confusion, finding IDs remain strictly stable across reviews:

| Stable Finding ID | Subject & Location | Initial Review Finding | Corrected Classification | Disposition |
| :--- | :--- | :--- | :--- | :--- |
| **GAP-UI-01** | `SshSection.tsx:727-739`, `288` | Quoted Windows path / missing picker | UX Affordance Limitation | Bypasses `FSSH-03` parser; saves directly to JSON store via `cmd_ssh_update_host`. Not a parser bug. |
| **GAP-UI-02** | `SshSection.test.tsx:784-848` | POSIX config path mock tests | Test Inventory Gap | Missing Windows drive-letter config path test cases. |
| **GAP-UI-03** | `RemoteDirectoryPicker.tsx:104-111` | Bare drive `C:` parsing | Autocomplete Policy / UX | Win32 drive-relative semantics preserved (`C:` is drive-relative, not unconditionally root). |
| **GAP-UI-04** | `RemoteDirectoryPicker.tsx:193-212` | Trailing `/` separator | Design Styling Consideration | Decorative `aria-hidden` marker alongside `entry.name`; not path corruption or functional defect. |
| **GAP-UI-05** | `RemoteDirectoryPicker.test.tsx:60-67` | PathAutocomplete test coverage | Test Inventory Gap | Missing tests for bare drives, drive roots (`C:\`), and UNC paths. |
| **GAP-UI-06** | `RemoteTerminal.tsx:490-498` | Wheel `deltaMode` division | **CONFIRMED REPAIR CANDIDATE** | Source branch omission; lacks page mode, line mode, pixel mode, and fractional delta accumulation. |
| **GAP-UI-07** | `RemoteTerminal.tsx:708-713, 738-747` | AltGr / non-ASCII sink focus | Runtime Unknown / Unproved | Omission of `preventDefault()` relies on subsequent `beforeinput`/`input` targeting. Unknown without live receipts. |
| **GAP-UI-08** | `FeatureVisuals.tsx:27-48` | Metal renderer demo copy | Marketing Illustration Policy | Visual depicts macOS terminal; not an application defect. Prose-pinning tests forbidden. |
| **GAP-UI-09** | `FeatureVisuals.tsx:1-147` | Component test coverage | Test Inventory Gap | Zero automated unit tests. |
| **GAP-UI-10** | `PlatformIcons.tsx:1-47` | Component test coverage | Test Inventory Gap | Zero automated unit tests. SVG geometry verified clean. |
| **GAP-UI-11** | `downloads.ts:29`, `DownloadMenu.tsx:74` | Store search URL / same-tab | Site Navigation Policy | Current site behavior is not shown defective; Store-only distribution is intentional policy. |
| **GAP-UI-12** | `downloads.test.ts:75-96` | Client Hints test coverage | Test Inventory Gap | Omits `userAgentData.platform = 'Windows'`. |

---

## 3. Inventory of G UI Scope Items

| Scope Item | File Path | Line Count | Key Windows Branch Points & Seams | Current Coverage Disposition |
| :--- | :--- | :--- | :--- | :--- |
| **1. SSH Settings UI** | `ui/src/components/settings/SshSection.tsx` | 1070 lines | Lines 162-177 (`open` file dialog for config file), 288 (`identityFile` packaging), 485 & 584 (`systemConfig.path` display), 727-739 (identity input field), 988-1004 (`environment.platform === "windows"` branch). | Reviewed; GAP-UI-01 UX limitation noted. |
| **1a. SSH Settings Tests** | `ui/src/components/settings/SshSection.test.tsx` | 857 lines | Lines 66-84 (Windows environment mock), 704-848 (system config and custom file override tests). | Reviewed; GAP-UI-02 test gap identified. |
| **2. Remote Directory Picker** | `ui/src/components/RemoteDirectoryPicker.tsx` | 259 lines | Lines 31 (`windows` regex `/^[A-Za-z]:\|^\\\\/`), 32 (`pathKey` backslash/lowercase normalization), 81-84 (`navigate` suffix), 104-111 (`updatePath` drive/slash parsing), 193-212 (`entry.name` and decorative `/`). | Reviewed; GAP-UI-03 & GAP-UI-04 categorized. |
| **2a. Directory Picker Tests** | `ui/src/components/RemoteDirectoryPicker.test.tsx` | 144 lines | Lines 60-67 (Windows separator tests for `C:/.../` and `C:\...\`), 109-119 (initial error retry with Windows home). | Reviewed; GAP-UI-05 test gap identified. |
| **3. Remote Terminal UI/Input** | `ui/src/remote/RemoteTerminal.tsx` | 903 lines | Lines 490-498 (`handleWheel` delta division), 667-669 (`onPointerDown` focus sink), 684-749 (`onKeyDown` AltGr, Ctrl chords, non-ASCII keys), 750-766 (`onPaste` CRLF normalization and bracketed paste). | Reviewed; GAP-UI-06 confirmed repair candidate; GAP-UI-07 runtime unknown. |
| **3a. Remote Terminal Contracts** | `ui/src/remote/RemoteTerminal.contract.test.tsx` | 963 lines | Lines 500-523 (keyboard & paste), 694-719 (wheel scroll clamping), 750-800 (focus sink lifecycle). | Reviewed; GAP-UI-06 test harness seam identified. |
| **3b. Remote Gestures Tests** | `ui/src/remote/RemoteTerminalGestures.test.tsx` | 412 lines | Lines 97-412 (touch swipe tab switching and pinch font resizing). | Reviewed; touch-only gesture suite; unaffected by Win32 pointer hooks. |
| **4. Site Feature Visuals** | `site/src/components/FeatureVisuals.tsx` | 147 lines | Lines 13-25 (`Chrome` 3-dot macOS header), 27-48 (`GhosttyVisual` macOS terminal demo), 111-127 (`ZeroElectronVisual` platform switcher). | Reviewed; GAP-UI-08 marketing copy; GAP-UI-09 test gap. |
| **5. Site Platform Icons** | `site/src/components/ui/PlatformIcons.tsx` | 47 lines | Lines 11-17 (`WindowsIcon` SVG), 27-33 (`MicrosoftStoreIcon` SVG), 43-54 (`PlatformIcon` switcher). | Reviewed; clean SVG geometry; GAP-UI-10 test gap. |
| **6. Site Downloads Config** | `site/src/lib/downloads.ts` | 135 lines | Lines 29 (`MICROSOFT_STORE_URL`), 55-75 (`PLATFORMS.windows` Store-only asset), 120-135 (`detectUserPlatform` Client Hints/platform fallback). | Reviewed; GAP-UI-11 navigation policy; GAP-UI-12 test gap. |
| **6a. Site Downloads Tests** | `site/src/lib/downloads.test.ts` | 97 lines | Lines 35-51 (Store-only asset contract), 75-96 (`detectUserPlatform` mock tests). | Reviewed; GAP-UI-12 test gap identified. |

---

## 4. Detailed Component Audits

### 4.1 `ui/src/components/settings/SshSection.tsx` & Test Suite

#### Source Audit & Call-Chain Tracing
- **Config File Picker Integration** (`SshSection.tsx:162-177`):
  Invokes `@tauri-apps/plugin-dialog` `open({ multiple: false, directory: false, title: "Select an SSH config file" })`. On Windows desktop, this opens the native Common Item Dialog. When selected, the path is stored via `setConfigPathOverride(chosen)` and `setSshConfigPathOverride(chosen)` (`localStorage` key `ferryx.ssh.configPath`), and passed to `cmd_ssh_read_system_config`.
- **Identity File Form Path & Validation** (`SshSection.tsx:727-739`, `288`):
  `identityFile` is configured through a text `<Input>`. When the user submits the form, `handleSubmitForm` packages `identityFile: formData.identityFile.trim() || undefined` and invokes `updateSshHost` (`src/lib/sshHosts.ts:221`).
  - `updateSshHost` invokes `cmd_ssh_update_host` (`src-tauri/src/ipc/ssh.rs:249`).
  - In `ipc/ssh.rs:249-268`, `cmd_ssh_update_host` loads the host JSON store (`store.hosts`), updates or inserts the `SshHost` struct, and saves the JSON store directly via `save_store`.
  - **Call Chain Proof**: The manual form does **not** call the OpenSSH config parser (`src-tauri/src/ssh/config.rs:72-76`, `FSSH-03`). `FSSH-03` affects only config file import (`cmd_ssh_import_config` / `cmd_ssh_read_system_config`). The manual form persists the user's typed string directly to the JSON store.
  - The absence of a file picker button for `identityFile` is a UI affordance limitation (`GAP-UI-01`), not a parser bug.
- **Environment Detection** (`SshSection.tsx:988-1004`):
  Properly inspects `test.environment.platform === "windows"` and conditionally renders `"Windows" · <executor> <version> · <Git status>`, enabling `cmd_ssh_prepare_integration`.
- **Test Inventory** (`SshSection.test.tsx`):
  - Lines 66-84 verify `platform: "windows"` rendering.
  - Lines 784-848 test custom config path override and reset, but mock paths are exclusively POSIX (`/tmp/work-ssh-config`, `/Users/test/.ssh/config`). Windows drive-letter paths are unrepresented in the test suite (`GAP-UI-02`).

### 4.2 `ui/src/components/RemoteDirectoryPicker.tsx` & Test Suite

#### Source Audit & Semantics Analysis
- **Platform Detection** (`RemoteDirectoryPicker.tsx:31`):
  ```typescript
  const windows = listing ? /^[A-Za-z]:|^\\\\/.test(listing.homePath) : /^[A-Za-z]:|^\\\\/.test(pathInput);
  ```
  Evaluates `listing.homePath` or `pathInput`. Correctly recognizes Windows drive letters (`C:`) and backslash UNC paths (`\\server\share`).
- **Path Input Parsing & Win32 Drive-Relative Semantics** (`RemoteDirectoryPicker.tsx:104-111`):
  ```typescript
  const normalized = windows ? value.replace(/\\/g, "/") : value;
  const lastSlash = normalized.lastIndexOf("/");
  const parent = lastSlash < 0 ? null
    : lastSlash === 0 ? "/"
    : windows && lastSlash === 2 && normalized[1] === ":" ? normalized.slice(0, 3)
    : normalized.slice(0, lastSlash);
  const fragment = value.slice(lastSlash + 1);
  const directory = parent && listing && pathKey(parent) === pathKey(listing.path) ? listing.path : parent;
  void load(directory, fragment);
  ```
  - When a user types `C:\`: `lastSlash === 2`, `normalized[1] === ":"`. `parent` evaluates to `"C:/"`, `fragment` is `""`. Correctly browses the root of drive C.
  - When a user types bare `C:`: `lastSlash === -1`. `parent` evaluates to `null`, and `fragment` is `"C:"`. `load(null, "C:")` is dispatched, which backend `resolve_path` (`browse.rs:88`) maps to `environment.home` with prefix `"C:"`.
  - **Win32 Semantics Note (`GAP-UI-03`)**: In Windows and DOS, `C:` denotes a drive-relative path pointing to the current directory on drive C, whereas `C:\` explicitly denotes the root directory. Because `RemoteDirectoryPicker` resolves `null` to the user's remote home directory, entering `C:` queries `~` rather than drive root. While an autocomplete heuristic could optionally expand `C:` to `C:\`, maintaining distinction between drive-relative and root paths is consistent with Windows filesystem semantics, not a correctness defect.
- **Option Item Separator** (`RemoteDirectoryPicker.tsx:193-212`):
  ```tsx
  {entries.map((entry, index) => (
    <li key={entry.path} role="option" id={`${listId}-${index}`} ...>
      <Folder className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="truncate">{entry.name}</span>
      <span aria-hidden="true" className="ml-auto text-muted-foreground">/</span>
    </li>
  ))}
  ```
  Each suggestion displays `entry.name` (e.g. `code` or `Documents`) accompanied by `<span aria-hidden="true" className="ml-auto text-muted-foreground">/</span>`. This is a decorative directory indicator (`GAP-UI-04`), not full path corruption or a functional defect.
- **Test Inventory** (`RemoteDirectoryPicker.test.tsx`):
  Lines 60-67 test `C:/Users/dev/code/` and `C:\Users\dev\code\`. Lacks tests for drive transitions, drive roots (`C:\`), bare drives (`C:`), and UNC paths (`GAP-UI-05`).

### 4.3 Remote UI/Input Shared Windows-Reachable Callers

#### Sole Confirmed Repair Candidate: Wheel `deltaMode` Normalization (`GAP-UI-06`)
- **Source Inspection** (`RemoteTerminal.tsx:490-498`):
  ```typescript
  const handleWheel = (event: React.WheelEvent<HTMLDivElement>) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    if (event.deltaY === 0) return;
    const rawRows = Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1);
    const rows = Math.min(10, Math.max(-10, rawRows));
    if (rows !== 0) {
      socket.send(JSON.stringify({ type: "scroll", rows }));
    }
  };
  ```
- **Reopened Mechanism**:
  The handler assumes pixel deltas by unconditionally dividing `event.deltaY / 20`. It contains zero handling for `event.deltaMode`:
  - `DOM_DELTA_PIXEL` (`0`): Pixel deltas from trackpads or high-precision mice.
  - `DOM_DELTA_LINE` (`1`): Line deltas commonly emitted by notched wheel mice.
  - `DOM_DELTA_PAGE` (`2`): Page deltas emitted by page-scrolling gestures.
  A complete normalization contract requires:
  1. Unit conversion: If `deltaMode === 1`, convert lines directly using cell metrics; if `deltaMode === 2`, convert pages using visible viewport rows (`geometry.rows`); if `deltaMode === 0`, divide by `cellMetrics.height` (falling back to 20px).
  2. Fractional delta accumulation: Maintain a fractional row accumulator across wheel bursts to ensure low-velocity trackpad movements are not dropped, while clamping integer rows to `[-10, 10]`.
- **Literal Existing Contract Test Command (PROPOSAL ONLY - NO EXECUTION CLAIM)**:
  `bun run --cwd ui test src/remote/RemoteTerminal.contract.test.tsx`
- **Observable Contract**:
  - Exact WebSocket message: `socket.send(JSON.stringify({ type: "scroll", rows }))`.
  - Cell context: respects measured `cellMetrics.height` from `getBoundingClientRect` when present.
  - No-sibling-state: modifies only the scroll message payload without altering active tab, selection, or input sink state.

#### AltGr & Non-ASCII Focus Delegation (`RemoteTerminal.tsx:708-713, 738-747`, `GAP-UI-07`)
- **Source Architecture**:
  - `RemoteTerminal` renders a hidden `<textarea ref={inputSinkRef}>` to capture text input and IME compositions.
  - The source handler `onPointerDown` (`lines 667-669`) explicitly calls `inputSinkRef.current?.focus()`.
  - When the surface container is focused via keyboard navigation and an AltGr key or non-ASCII printable key is pressed, lines 709 and 746 call `inputSinkRef.current?.focus()` without calling `event.preventDefault()`.
  - The source comment explicitly documents this design choice:
    `// AltGraph produces a printable glyph via the OS layout. Hand the keystroke to the input sink (no preventDefault) so its InputEvent emits the glyph exactly once, matching native behavior, instead of synthesizing a meta chord or double-sending the character.`
  - Because `event.preventDefault()` is omitted, the browser continues its default input processing. In modern browsers, subsequent `beforeinput` and `input` events can target the newly active element (`inputSinkRef`).
  - **Verdict**: Actual browser default event sequencing across various pointer and keyboard events is unknown at runtime. Without empirical receipts from physical Windows browser runs, the claim that keystrokes are lost is unproved. Classified as runtime unknown `GAP-UI-UNK-01`.

#### Clipboard & Paste Policy Compliance (`RemoteTerminal.tsx:717`, `750-766`)
- Line 717: `if (ctrlChordChar === "v") return;` intentionally permits native paste to fire `onPaste`, preserving existing Ctrl+V paste policy.
- Lines 750-766: `onPaste` normalizes Windows CRLF (`\r\n` -> `\n`) and wraps multiline paste in bracketed paste mode (`\x1b[200~...\x1b[201~`).

### 4.4 `site/src/components/FeatureVisuals.tsx` & `site/src/components/ui/PlatformIcons.tsx`

#### Source Audit
- **Feature Visuals Analysis** (`FeatureVisuals.tsx:13-48`):
  - `Chrome` (`lines 13-25`) renders three circular dots (`<i className="h-2 w-2 rounded-full bg-white/25" />`), styling the demo window as a macOS terminal.
  - `GhosttyVisual` (`lines 27-48`) renders a static terminal session mockup displaying `"renderer: wgpu / Metal"`.
  - **Verdict (`GAP-UI-08`)**: The feature visual illustrates a macOS terminal session (matching the macOS window chrome). This is illustrative marketing copy depicting macOS, not an application code defect. Per senior test discipline mandates, prose and doc copy are not pinned with tests.
- **Platform Icons Analysis** (`PlatformIcons.tsx:1-47`):
  - `WindowsIcon` (`lines 11-17`): Well-formed 4-pane Win32/UWP SVG path in a 24x24 viewport (`viewBox="0 0 24 24"`, `fill="currentColor"`).
  - `MicrosoftStoreIcon` (`lines 27-33`): Well-formed Store bag SVG path.
  - `PlatformIcon` (`lines 43-54`): Strict TypeScript discriminator mapping `'macos' | 'windows' | 'linux' | 'store'`.
  - **Verdict (`GAP-UI-10`)**: SVG geometry and TypeScript contracts are clean and verified. Test coverage absence is noted as a test gap.

### 4.5 `site/src/lib/downloads.ts` & `site/src/lib/downloads.test.ts`

#### Source Audit
- **Microsoft Store Link & Navigation Policy** (`downloads.ts:26-75`, `DownloadMenu.tsx:74-85`):
  - `MICROSOFT_STORE_URL`: Defined as `'https://apps.microsoft.com/search?query=Ferryx'`.
  - In `DownloadMenu.tsx:74-85`, the hero button links directly to `MICROSOFT_STORE_URL` in the same browsing tab (no `target="_blank"`).
  - **Policy Verdict (`GAP-UI-11`)**: Current site behavior is not shown defective. Navigating to the Microsoft Store search query in the same tab is current site behavior. Direct installer exclusion is confirmed project release policy (`downloads.test.ts:35-51`).
- **Platform Detection Contract** (`downloads.ts:120-135`):
  - Accurately checks `userAgentData.platform` (Client Hints) before falling back to `navigator.platform` and `userAgent`.
  - Test suite (`downloads.test.ts:75-96`) mocks only legacy `navigator.platform = 'Win32'`. Modern `userAgentData.platform = 'Windows'` is omitted from the test matrix (`GAP-UI-12`).

---

## 5. Refuted Allegations & Non-Defects

1. **Lost Keystroke Claim on AltGr Focus Delegation (`RemoteTerminal.tsx:710, 746`)**:
   - *Status*: **REFUTED AS A CONFIRMED DEFECT / DOWNGRADED TO RUNTIME UNKNOWN (`GAP-UI-UNK-01`)**.
   - *Reasoning*: DOM `keydown` dispatch not being retroactive does not prove that subsequent `beforeinput` and `input` events fail to target the newly focused `<textarea>`. The source comment explicitly notes that `event.preventDefault()` is omitted so that the OS layout produces the printable glyph in the input sink. Source handler (`onPointerDown`) focuses the sink on pointer events, but actual browser default sequencing across various pointer and keyboard events is unknown at runtime.
2. **`identityFile` Parser Rejection in Manual Form (`SshSection.tsx:288`)**:
   - *Status*: **REFUTED AS A PARSER DEFECT**.
   - *Reasoning*: The manual form in `SshSection.tsx` dispatches `cmd_ssh_update_host`, which saves the host directly into the JSON store without passing it through `src-tauri/src/ssh/config.rs` (`FSSH-03`). The absence of a file picker is a UX affordance limitation (`GAP-UI-01`), not a parser bug.
3. **Bare Drive `C:` as Root Defect (`RemoteDirectoryPicker.tsx:104-111`)**:
   - *Status*: **REFUTED AS A DEFECT**.
   - *Reasoning*: In Win32 and DOS filesystem semantics, `C:` denotes a drive-relative path (current directory on drive C), whereas `C:\` denotes the drive root. In `RemoteDirectoryPicker`, `C:` is parsed as a relative path and queries the remote home directory with prefix `"C:"`. Expanding `C:` to `C:\` would be an autocomplete heuristic (`GAP-UI-03`), not a defect fix.
4. **Option Item Separator as Path Corruption (`RemoteDirectoryPicker.tsx:193-212`)**:
   - *Status*: **REFUTED AS PATH CORRUPTION / RECLASSIFIED AS STYLING CONSIDERATION (`GAP-UI-04`)**.
   - *Reasoning*: The component displays `entry.name` alongside an `aria-hidden` decorative slash `<span aria-hidden="true" ...>/</span>`. It does not corrupt the path or display `C:\Users\dev/`.
5. **Same-Tab Store Search as Source Defect (`downloads.ts:29`, `DownloadMenu.tsx:74`)**:
   - *Status*: **REFUTED AS A DEFECT**.
   - *Reasoning*: Current site behavior is not shown defective. Direct installer exclusion is documented project release policy.
6. **Feature Visual Metal Demo Copy (`FeatureVisuals.tsx:32`)**:
   - *Status*: **REFUTED AS A DEFECT**.
   - *Reasoning*: The visual illustrates a macOS terminal session inside macOS window chrome. Per senior test discipline mandates, prose and doc copy are not pinned with tests.
7. **Speculative Clipboard API Activation Expiration (`WIN-UI-05`)**:
   - *Status*: **REFUTED AS A CONFIRMED DEFECT**.
   - *Reasoning*: The allegation that an `await` IPC call alone unconditionally expires transient user activation in Chromium is unproved. However, live permissions and focus boundaries across IPC remain an empirical question (`GAP-UI-UNK-02`).
8. **140px Window Titlebar Caption Button Spacing Claim (`WIN-UI-07` / `PKG-04`)**:
   - *Status*: **REFUTED**.
   - *Reasoning*: `titleBarStyle: "Overlay"` applies custom titlebar styling on macOS only. On Windows, standard native Win32/DWM non-client caption controls are rendered; adding a 140px spacer would introduce artificial dead space.
9. **Ctrl+W, Ctrl+V, and Ctrl+Click Policies**:
   - *Status*: **POLICY PRESERVED**.
   - *Reasoning*: Existing tab close (`Ctrl+W`), paste (`Ctrl+V`), and link navigation (`Ctrl+Click`) behaviors remain untouched in accordance with review mandates.

---

## 6. Exact Remaining Runtime Uncertainties

*Note: Source review alone cannot certify Windows runtime correctness. The following empirical behaviors require execution on a physical or virtual Windows host:*

1. **Windows PC Mouse Wheel DeltaMode Emission in Browsers (`GAP-UI-06`)**:
   - *Uncertainty*: Does WebView2, Chrome, Edge, and Firefox on Windows emit `WheelEvent.deltaMode === 1` (`DOM_DELTA_LINE`) or `deltaMode === 0` (`DOM_DELTA_PIXEL`) for single-notch PC mouse wheel rolls?
   - *Runtime Probe*: Run an instrumented probe page in Chrome/Edge on Windows logging `(e.deltaMode, e.deltaX, e.deltaY, e.wheelDeltaY)` on notched PC mouse wheel rolls.
2. **AltGr / Non-ASCII Keystroke Dispatch on Unfocused Terminal Surface (`GAP-UI-UNK-01`)**:
   - *Uncertainty*: When `surfaceRef` is focused via keyboard navigation and the user types `AltGr+Q` (`@`) or non-ASCII characters on a German/Polish keyboard layout, does the browser trigger `beforeinput`/`input` on the newly focused textarea, or is the initial glyph discarded?
   - *Runtime Probe*: In Chrome/Edge on Windows with German keyboard layout, tab-focus `surfaceRef`, press `AltGr+Q`, and inspect whether `@` appears in the terminal grid.
3. **ConPTY Multiline Paste Line Ending Processing**:
   - *Uncertainty*: When `RemoteTerminal.tsx:755` normalizes Windows CRLF (`\r\n`) to `\n` in multiline paste, does ConPTY on a Windows daemon host interpret bare `\n` as `\r\n`, or does it cause staircase formatting?
   - *Runtime Probe*: Connect a remote web client to a Ferryx daemon hosting `cmd.exe` or `powershell.exe` on Windows; paste a 3-line block and observe whether commands execute with proper line breaks.
4. **Asynchronous Clipboard Access under Windows Security Contexts (`GAP-UI-UNK-02`)**:
   - *Uncertainty*: Does `navigator.clipboard.writeText` succeed across local Tauri IPC boundaries under standard user permissions, UAC-elevated contexts, and remote desktop sessions?
   - *Runtime Probe*: Execute terminal text copy via `Ctrl+Shift+C` on Windows under standard and elevated desktop sessions.
5. **OpenDialog Initial Directory Behavior for SSH Config**:
   - *Uncertainty*: When `SshSection.tsx:164` calls `open({ multiple: false, directory: false })` without `defaultPath`, does the Windows Common Item Dialog default to `%USERPROFILE%\.ssh` if previously accessed, or does it default to `Documents`?
   - *Runtime Probe*: Click "Choose File…" in SSH Settings on a clean Windows desktop installation and observe initial dialog directory.
6. **Remote Directory Listing Latency on Deep Windows Paths**:
   - *Uncertainty*: Does PowerShell `Get-ChildItem` in `browse.rs:172` exceed the 12-second bounded output timeout when run against large or slow remote Windows directories (e.g. `C:\Windows\System32` or `WinSxS`)?
   - *Runtime Probe*: Execute `cmd_ssh_list_directories` against a live Windows SSH target at `C:\Windows\System32`; record latency and truncation flag behavior.

---

## 7. Verification Receipts

- **File Reference Audit**:
  - `ui/src/components/settings/SshSection.tsx`: Verified lines 162-177, 230-246, 288, 485, 584, 727-739, 988-1004 against current working tree.
  - `ui/src/components/settings/SshSection.test.tsx`: Verified lines 66-84, 784-848 against current working tree.
  - `ui/src/components/RemoteDirectoryPicker.tsx`: Verified lines 31, 32, 81-84, 104-111, 193-212 against current working tree.
  - `ui/src/components/RemoteDirectoryPicker.test.tsx`: Verified lines 60-67, 109-119 against current working tree.
  - `ui/src/remote/RemoteTerminal.tsx`: Verified lines 490-498, 667-669, 684-749, 750-766 against current working tree.
  - `site/src/components/FeatureVisuals.tsx`: Verified lines 13-25, 27-48, 111-127 against current working tree.
  - `site/src/components/ui/PlatformIcons.tsx`: Verified lines 11-17, 27-33, 43-54 against current working tree.
  - `site/src/lib/downloads.ts`: Verified lines 26-29, 55-75, 120-135 against current working tree.
  - `site/src/lib/downloads.test.ts`: Verified lines 35-51, 75-96 against current working tree.
- **Foreign Tree Integrity**:
  - `git status --short` verified before and after report authoring.
  - Foreign modified files in `src-tauri/`, `ui/src/App.tsx`, `ui/src/App.test.tsx`, `ConfirmCloseTabDialog.tsx`, and `.omo/` left completely untouched.
  - No source, test, or configuration files modified or created other than `docs/evidence/windows-review-20260913/gap-ui.md`.
