# Xterm Removal Verification

Date: 2026-08-25

## Delivered behavior

- The desktop terminal route renders `NativeTerminalPane` exclusively through
  `ui/src/components/TerminalPane.tsx`.
- The remote terminal is a dependency-free WebSocket stream in
  `ui/src/remote/RemoteTerminal.tsx`. It renders terminal output in a focused
  `<pre>`, applies imported terminal preferences, preserves paste and regular
  input, encodes navigation keys, sends Ctrl-C as the existing interrupt
  message, and retains `MobileKeyDock` controls.
- The former xterm renderer modules were removed. No `@xterm/*` package,
  lockfile, source import, or legacy renderer/settings helper reference remains
  in the UI or site frontend surfaces.

## Automated evidence

The following commands passed against the final live tree:

```sh
bun run --cwd ui test src/remote/RemoteTerminal.contract.test.tsx
# 1 file, 2 tests passed

bun run --cwd ui build
# tsc && vite build passed

bun run --cwd ui test
# 78 files, 616 tests passed
```

The final zero-reference audit passed:

```sh
test ! -e ui/src/lib/terminalRenderer.ts
test ! -e ui/src/lib/terminalRendererMetrics.ts
! git grep -in -E '(@xterm|xterm\.js|xterm|terminalHostManager|terminalInstanceFactory|terminalOutputScheduler|terminalRenderer|applyTerminalSettings)' -- ui/src ui/package.json ui/bun.lock site/package.json site/bun.lock
```

`lsp_diagnostics` could not run because the workspace LSP daemon socket was
unreachable. The production build's `tsc` phase passed, providing the available
TypeScript diagnostic coverage for this environment.

## Manual QA required

Desktop UI automation is intentionally not performed. In the rebuilt desktop
app, open a workspace terminal and verify normal typing, arrows, Enter, and
Ctrl-C on the native terminal surface. From a paired remote browser client,
verify streamed output, regular typing, paste, arrows, Ctrl-C, and the mobile
key dock.
