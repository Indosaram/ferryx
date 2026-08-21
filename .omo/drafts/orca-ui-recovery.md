---
slug: orca-ui-recovery
status: drafting
intent: clear
review_required: true
plan_path: .omo/plans/orca-ui-recovery.md
plan_sha256: null
review_round_id: null
review_round_limit: 5
pending-action: write and review .omo/plans/orca-ui-recovery.md
review:
  momus:
    status: pending
    workspace_root: null
    runtime_home: null
    target: .omo/plans/orca-ui-recovery.md
    round_id: null
    plan_sha256: null
    launch_id: null
    session: null
    result: null
approach: Restore the debug workspace bootstrap first, then rebuild the supported Tauri surface against an extracted Orca design contract and complete only the UI flows the existing backend can truthfully support.
---

# Draft: orca-ui-recovery

## Components (topology ledger)
<!-- Lock the SHAPE before depth. One row per top-level component that can succeed or fail independently. -->
<!-- id | outcome (one line) | status: active|deferred | evidence path -->
| bootstrap | Debug Tauri launches from `src-tauri/` yet registers and renders the repository-root worktree. | active | `src-tauri/src/lib.rs`, `src-tauri/src/worktree/manager.rs`, live process PID 282 cwd evidence |
| design-system | All user-visible shell primitives compile from a documented Orca-derived token system rather than undefined Tailwind classes. | active | `ui/tailwind.config.js`, `ui/src/index.css`, `ui/original-dist/assets/I18nProvider-TFirEJhb.css` |
| workspace-surface | Sidebar, worktree cards, terminal tabs/panes, empty/error/loading states, and supported commands present real backend state and interactions. | active | `ui/src/App.tsx`, `ui/src/state/*`, `ui/src/lib/tauri.ts`, `src-tauri/src/ipc/*` |
| supported-safety-flows | Worktree create/status/delete-preview/delete and terminal close/signal/list are surfaced with safety-first confirmations and structured errors. | active | `src-tauri/src/ipc/worktree.rs`, `src-tauri/src/ipc/terminal.rs` |
| Orca fidelity | The implementation adopts portable Orca shell, typography, component, and pane-resize conventions without importing Electron-only application logic. | active | `ui/original-dist/index.html`, `ui/original-dist/assets/{I18nProvider-TFirEJhb.css,document-theme-66WaD9Gm.js,useSidebarResize-BhlGhEjK.js}` |

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
<!-- assumption | adopted default | rationale | reversible? -->
| Canonical reference | Treat the locally extracted renderer at `ui/original-dist/` as the visual/component contract, not as code to copy wholesale. | Its Electron `window.api` contracts and daemon model are incompatible with the current Tauri backend; its CSS, layout, font, primitive, and interaction evidence are portable. | yes |
| Scope boundary | Implement the supported workspace and terminal surface completely, but do not fake browser, editor, Monaco, PR, remote-host, automation, or true agent-orchestration features absent from the Rust IPC. | Presently `src-tauri/src/ipc/*` exposes only worktree and terminal capabilities. | yes, when backend support exists |
| Rendering stack | Keep React/Vite/Tailwind v3; create an Orca token layer and small accessible primitives rather than migrate the application to Tailwind v4 or import the original renderer bundle. | Current UI is Tailwind v3; original uses Tailwind v4 plus Radix wrappers. A migration adds unrelated risk. | yes |
| Workspace selection | Resolve and register the canonical Git root discovered from the launch directory; expose a deliberate no-repository empty state if discovery fails. | The debug process actually starts in `src-tauri/`, while Git reports its top-level as the project root. | yes |
| Validation | Use TDD for bootstrap/IPC/store behavior and independent visual QA at 1280, 768, and 375 px against fresh debug-app captures. | The current tests do not exercise startup registration and prior preview-only validation missed the shipped styling failure. | yes |

## Findings (cited - path:lines)
1. The observed debug app is nonfunctional because its process cwd is `/Users/indo/code/project/orca-lite/src-tauri`, while `WorktreeManager::try_new` rejects any path whose canonical form differs from `git rev-parse --show-toplevel`. `create_app` silently discards that registration failure, so the UI's initial `listWorktrees("default")` receives `WORKSPACE_NOT_FOUND`. Evidence: `src-tauri/src/lib.rs`, `src-tauri/src/worktree/manager.rs`, `ui/src/state/workspaceRuntime.ts`, live debug process PID 282.
2. Existing backend tests manually register temporary repository roots and do not invoke the startup path from a nested directory. Evidence: `src-tauri/tests/ipc_hardening_contract.rs`, `src-tauri/src/ipc/tests.rs`.
3. The current frontend is not using a complete design system. `ui/tailwind.config.js` declares only `background` and `foreground`; `ui/src/index.css` declares only the color scheme and body defaults; components rely on undeclared semantic colors, dimensions, utilities, and animations. Evidence: `ui/src/{App.tsx,components/{Sidebar,WorkspaceHeader,TabBar,TerminalSplitView,TerminalPane}.tsx,components/ui/*}`, `ui/tailwind.config.js`, `ui/src/index.css`.
4. The generated surface therefore collapses hierarchy and falls back to default border styles. Fresh visual review found white dividers, missing active-state treatment, unthemed panels, and split-pane gaps. Evidence: `/tmp/orca-lite-final-{split-horizontal,split-vertical,unsplit}.png`; independent visual reviews `st_01a02195`, `st_01a02196`.
5. Current state fakes agents by projecting every terminal tab into `ActiveAgent`; buttons for Workspace, Agents, Search, Settings, details, and card overflow actions are nonfunctional. Evidence: `ui/src/state/workspaceStore.ts`, `ui/src/components/{Sidebar,WorktreeList,WorkspaceHeader}.tsx`.
6. The frontend omits wrappers and visual flows for several backend-supported operations: worktree status, deletion preview, safe/destructive delete, terminal signal, and terminal session list. Evidence: `ui/src/lib/tauri.ts` vs `src-tauri/src/ipc/{worktree,terminal}.rs`.
7. The canonical reference is an extracted Electron renderer, not a raw clone. Its renderer architecture uses React, Tailwind CSS v4, CSS variables, `clsx` + `tailwind-merge`, Radix primitives, Sonner, Geist, and xterm. Evidence: `ui/original-dist/index.html`, `ui/original-dist/assets/{I18nProvider-TFirEJhb.css,button-DszXJEV6.js,dropdown-menu-Dth6LPK-.js,dialog-BbelfMSB.js,app-font-family-CyNkxn1D.js}`.
8. The reference dark system is zinc-based and deliberately compact: `--background #0a0a0a`, `--card #171717`, `--muted #262626`, `--accent #404040`, translucent border/input tokens, sidebar/worktree-sidebar variants, 10px base radius, Geist UI typography, and a mono terminal stack. Its menus/dialogs are translucent 11px-radius surfaces with blur, inset highlights, and Radix state transitions. Evidence: `ui/original-dist/assets/I18nProvider-TFirEJhb.css`, `ui/original-dist/assets/{dropdown-menu-Dth6LPK-.js,dialog-BbelfMSB.js}`.
9. The reference app models real agent rows separately from terminal tabs, has a resizable sidebar, draggable split dividers, and a broad desktop runtime. Only the visual and interaction patterns that map to current Rust IPC are candidates for this plan. Evidence: `ui/original-dist/assets/{AgentDashboardSidebarEntry-DbNJTT-G.js,useSidebarResize-BhlGhEjK.js,terminal-appearance-D3oO-Ew5.js,WorktreeCard-DHnjoc37.js}`.

## Decisions (with rationale)
1. Repair the bootstrap bug before touching visual fidelity: a beautiful shell without a workspace source cannot prove any terminal or worktree behavior.
2. Create `ui/DESIGN.md` as an extracted, Orca-derived contract before creating/replacing surface primitives; it will define tokens, typography, dimensions, primitives, state/motion rules, responsiveness, and accepted scope boundary.
3. Rebuild the shell around supported domain entities (`Worktree`, terminal session, terminal tab, worktree status) and remove the false terminal-tab-to-agent projection. The app may show a terminal activity list, but must not label it as an autonomous agent system.
4. Use portable reference patterns: CSS variables mapped to Tailwind v3 semantic colors, Geist and symbol-font assets copied/served locally, reusable button/menu/dialog/empty/error/status primitives, pane resize affordances, and quiet zinc dividers. Do not run or embed Electron renderer code.
5. A feature is shipped only when its control invokes a supported Tauri IPC method, its success/error state is visible, its optimistic state reconciles with an event or refresh, and its behavior has a regression test plus debug-app QA.

## Scope IN
1. Startup root discovery and registration, including deterministic nested-launch and no-repository test coverage.
2. Orca-derived design contract and portable global token/font/utility system.
3. Functional workspace shell: real worktree list/selection/create/status/delete flows; terminal tabs, close, signal, split/resize; truthful terminal activity presentation; loading/empty/error states.
4. Accessible primitive layer (button, menu, dialog, toast/status styling) needed by the supported flows.
5. Resizable sidebar and correctly sized/draggable split terminal panes where each operation is backed by state and tests.
6. Fresh desktop debug-app/manual and multi-viewport visual QA, plus backend/frontend unit/integration/build gates.

## Scope OUT (Must NOT have)
1. Copying/importing the original Electron renderer or its `window.api` calls.
2. Pretending the present backend supports editors, browser tabs, source control, remote hosts, Automations, PR dashboards, real AI agents, or persistent settings.
3. A Tailwind v3-to-v4 migration solely for visual parity.
4. Any destructive worktree deletion without backend preview/status checks, explicit confirmation, and structured error handling.
5. Committing unrelated existing changes, `.omo/` artifacts, or `ORCA_LITE_FIX_PLAN.md`.

## Open questions
1. Fidelity target: should the implementation reproduce **the full visible Orca workspace shell only** (recommended; supported controls work and unavailable original-app domains are absent), or should it also render nonfunctional visual placeholders for source control/editor/browser/agent-dashboard regions? The latter conflicts with your instruction that the functions must work, so I recommend the first.

## Approval gate
status: awaiting-approval
<!-- When exploration is exhausted and unknowns are answered, set status: awaiting-approval. -->
<!-- That durable record is the loop guard: on a later turn read it and resume at the gate instead of re-running exploration. -->
Approach: first fix startup root registration and prove the debug app loads the real root worktree. Then extract the local Orca renderer into `ui/DESIGN.md`, build a token-backed portable primitive layer, replace the current mock shell with only backend-supported workspace/terminal lifecycle flows, and verify the actual debug Tauri application and fresh visual captures. After approval I will write the detailed plan and run its required Momus review; I will not implement in plan mode.
