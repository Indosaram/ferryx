# Mass-ULW Plan: Comprehensive Code Review of orca-lite (Ferryx)

## Objective
Perform an in-depth, rigorous multi-agent code review of the entire `orca-lite` (Ferryx) codebase across all layers and persist the comprehensive findings into `docs/code-review-report.md`.

## DAG Architecture & Decomposition
```
Wave 1 (Parallel Domain Specialist Reviews):
├── [review-tauri-backend] (unspecified-high)
│     Rust backend, daemon, pty/terminal, IPC, worktree, remote server
├── [review-frontend-state-terminal] (unspecified-high)
│     Zustand workspaceStore, paneTree, terminalHostManager, transport, HMR
├── [review-ui-ux-components] (visual-engineering)
│     Components, TabBar, Drag & Drop, TerminalSplitView, styling, Ferryx branding
├── [review-security-error-handling] (unspecified-high)
│     Remote auth, IPC boundaries, browser security, process spawning, error types
└── [review-test-build-tooling] (unspecified-high)
      Unit/integration tests, test seams, Vitest, Cargo tests, CI/build configs

Wave 2 (Synthesis & Artifact Generation):
└── [synthesize-and-persist-report] (writing) [dependsOn: all wave 1 nodes]
      Consolidate findings, structure prioritized matrix, write docs/code-review-report.md
```

## Success Criteria
1. Full codebase coverage with concrete file:line references across Rust, TypeScript, React, and Build systems.
2. Clear categorization of findings by severity (Critical / High / Medium / Low / Nit) and domain.
3. Actionable remediation recommendations and architectural improvement roadmap.
4. Comprehensive Markdown report persisted in `docs/code-review-report.md`.
