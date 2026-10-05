# Mac supplemental candidate file runs
These10 commands recover assertions after the full-suite timeout; they do not establish full-suite acceptance.
| Host | Exact command | Native exit | Selected | Verdict | Asserted line |
| --- | --- | --- | --- | --- | --- |
| mac | `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx` | 1 | 197 | RAN_FAILED |       Tests  186 failed / 11 passed (197) |
| mac | `bun run --cwd ui test src/App.test.tsx` | 1 | 154 | RAN_FAILED |       Tests  3 failed / 151 passed (154) |
| mac | `bun run --cwd ui test src/App.remote.test.tsx` | 1 | 20 | RAN_FAILED |       Tests  1 failed / 19 passed (20) |
| mac | `bun run --cwd ui test src/App.pairedDaemon.test.tsx` | 1 | 8 | RAN_FAILED |       Tests  1 failed / 7 passed (8) |
| mac | `bun run --cwd ui test src/components/NativeTerminalPane.presentation.test.tsx` | 1 | 14 | RAN_FAILED |       Tests  3 failed / 11 passed (14) |
| mac | `bun run --cwd ui test src/components/TerminalSearchOverlay.test.tsx` | 1 | 11 | RAN_FAILED |       Tests  1 failed / 10 passed (11) |
| mac | `bun run --cwd ui test src/lib/updater.test.ts` | 1 | 23 | RAN_FAILED |       Tests  1 failed / 22 passed (23) |
| mac | `bun run --cwd ui test src/components/SettingsDialog.cli.test.tsx` | 1 | 6 | RAN_FAILED |       Tests  2 failed / 4 passed (6) |
| mac | `bun run --cwd ui test src/components/TerminalSplitView.paneHandleReach.test.tsx` | 1 | 1 | RAN_FAILED |       Tests  1 failed (1) |
| mac | `bun run --cwd ui test src/lib/pairedDaemonRollout.test.ts` | 1 | 5 | RAN_FAILED |       Tests  1 failed / 4 passed (5) |
