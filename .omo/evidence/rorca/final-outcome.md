# rorca final outcome ledger

This is the evidence-based completion ledger for `.omo/plans/rorca-ui-parity.md` and `.omo/evidence/rorca/completion-checklist.md`.

---

## 1. Final Regression Gates

All automated build and regression gates pass cleanly:

| Gate | Target / Scope | Result | Status |
| --- | --- | --- | --- |
| `bun run --cwd ui test` | Full frontend test suite (22 files, 96 tests) | 22 files passed, 96 tests passed | **PASS** |
| `bun run --cwd ui build` | Frontend Vite production bundle build | Built clean with no type/bundling errors | **PASS** |
| `cargo test` | Native Rust Tauri backend unit/integration tests (`src-tauri`) | All suites passed | **PASS** |
| `clippy -D warnings` | Native Rust strict linter (`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`) | Clean, exited 0 | **PASS** |

---

## 2. Validated Capabilities & Automated Evidence

| Component / Subsystem | Evidence & Test Coverage | Verdict |
| --- | --- | --- |
| **Nested Split Layout** | 13 focused tests in `ui/src/components/TerminalSplitView.test.tsx` verifying pane splitting (horizontal/vertical), pane-local controls, focus transitions, pane closing, and layout tree immutability. | **PASS** |
| **Numeric Shortcuts** | 24 focused tests in `ui/src/lib/shortcuts.test.tsx` validating modifier matching, numeric chord routing (`CmdOrCtrl+1..9`), capture phase handling, and terminal input preservation. | **PASS** |
| **Visual Chrome Remediation** | 8 focused tests across `ui/src/components/Sidebar.test.tsx` (4 tests) and `ui/src/components/TabBar.test.tsx` (4 tests) verifying single 1px rail divider and complete `bg-card` tab strip coverage. | **PASS** |
| **Titlebar Capability Contract** | Native Tauri window dragging integration with strict `data-tauri-drag-region` on noninteractive chrome and `no-drag` exclusions across all interactive controls. | **PASS** |
| **Ghostty / Project / Worktree** | Prior evidence in `.omo/evidence/rorca/native-worker-receipt.md` confirming native Ghostty config parser (duplicate font last-wins precedence, option-as-alt, fallback safety) and project/worktree branch selector flows. | **PASS** |
| **Visual Parity Comparison** | Programmatic PNG comparison against reference in `.omo/evidence/rorca/final-visual-comparison.md` confirming color palette, 236px rail width, 36px header, 32px tab strip, 896px centered settings detail, and full-bleed terminal canvas. | **PASS** |

---

## 3. Pending User-Run Desktop Manual Checks

The following items cannot be fully proven in headless CI/automation and are reserved for user-run desktop manual validation:

1. **Nested three-pane behavior:** Interactive split resizing, focus movement, and terminal session lifecycle across a 3+ pane layout.
2. **Cmd/Ctrl numeric selections:** Direct keyboard navigation (`Cmd+1..9` on macOS / `Ctrl+1..9` on Linux/Windows) switching active tabs/worktrees with focused terminal canvases.
3. **Cmd-Option-D:** Keyboard chord trigger for horizontal/secondary split behavior in running desktop app.
4. **Titlebar window movement:** Physical mouse drag on titlebar chrome moving the native OS window without interfering with window control clicks or sidebar resize handles.

---

## 4. User Acceptance Checklist

Please complete the following verification steps on the live desktop build:

- [ ] **Nested Three-Pane Behavior**  
  *Action:* Create two horizontal splits and one vertical split (3 panes). Type in each pane, resize dividers, and close each pane individually.  
  *Result:* ________________________________________  
  *Sign-off:* ________________________________________  

- [ ] **Cmd/Ctrl Numeric Selections (`Cmd/Ctrl+1..9`)**  
  *Action:* Open multiple tabs and worktrees; press `Cmd+1` through `Cmd+N` (or `Ctrl+1..N`) while xterm canvas has focus. Ensure tab switches immediately without character leakage into the terminal prompt.  
  *Result:* ________________________________________  
  *Sign-off:* ________________________________________  

- [ ] **Cmd-Option-D Split Shortcut**  
  *Action:* Focus an active terminal pane and trigger `Cmd+Option+D` (or `Ctrl+Alt+D`). Verify expected split is created.  
  *Result:* ________________________________________  
  *Sign-off:* ________________________________________  

- [ ] **Titlebar Window Dragging & Movement**  
  *Action:* Click and drag the empty titlebar area to move the desktop window across monitors/spaces. Click titlebar action buttons to confirm clickability.  
  *Result:* ________________________________________  
  *Sign-off:* ________________________________________  
