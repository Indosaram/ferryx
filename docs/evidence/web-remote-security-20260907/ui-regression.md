# Remote UI Regression Evidence

Source baseline: 37272f5. No UI production changes in the security patch.

## Build

`bun run --cwd ui build` in isolated worktree. Exit 0

```text
vite v6.4.3 building for production...
transforming...
✓ 1868 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                                        1.29 kB │ gzip:   0.60 kB
dist/assets/crush-DHElXcRZ.png                         5.46 kB
dist/assets/droid-BhrKLgQ8.svg                         6.30 kB │ gzip:   2.88 kB
dist/assets/antigravity-DgAQAdn3.svg                   7.63 kB │ gzip:   1.72 kB
dist/assets/gjc-CmlXGg4z.png                          22.73 kB
dist/assets/geist-variable-CrgPqtmy.woff2             69.44 kB
dist/assets/ferryx-icon-OXRkkUvz.png                 805.82 kB
dist/assets/index-zcQS6jqy.css                        76.35 kB │ gzip:  13.87 kB
dist/assets/check-BZv37Nto.js                          0.29 kB │ gzip:   0.24 kB
dist/assets/card-CwS9iJeD.js                           2.08 kB │ gzip:   0.88 kB
dist/assets/PermissionsOnboardingDialog-0YcvDGGc.js    6.21 kB │ gzip:   1.83 kB
dist/assets/browser-6C9JBzaZ.js                       25.78 kB │ gzip:  10.13 kB
dist/assets/RemoteApp-CQkYHWoP.js                     44.77 kB │ gzip:  13.43 kB
dist/assets/sonner-R7Ev958r.js                        63.59 kB │ gzip:  22.08 kB
dist/assets/index-Cd09m_dW.js                        179.28 kB │ gzip:  57.51 kB
dist/assets/SettingsDialog-CG1GdC7k.js               193.23 kB │ gzip:  57.81 kB
dist/assets/App-CP_lbVbw.js                          447.71 kB │ gzip: 130.28 kB
✓ built in 2.95s
$ tsc && vite build

```

## Regression tests

Command: `node /Users/indo/code/project/orca-lite/ui/node_modules/vitest/vitest.mjs run src/remote src/lib/remoteClient.test.ts src/components/settings/RemoteAccessSection.test.tsx --maxWorkers=1` from isolated ui directory. Exit 0

```text

 RUN  v3.2.7 /Users/indo/code/project/ferryx-web-remote-security-20260907/ui

 ✓ src/remote/RemoteUI.test.tsx (39 tests) 1173ms
 ✓ src/remote/RemoteTerminal.contract.test.tsx (42 tests) 743ms
 ✓ src/components/settings/RemoteAccessSection.test.tsx (6 tests) 1565ms
   ✓ RemoteAccessSection > does not probe for or display Tailscale detection  435ms
   ✓ RemoteAccessSection > creates a pairing code when the user explicitly clicks Generate QR Code  327ms
   ✓ RemoteAccessSection > displays an alert when enabling the remote gateway fails  353ms
   ✓ RemoteAccessSection > displays an alert when revoking a device fails  348ms
 ✓ src/remote/RemoteAttention.test.tsx (9 tests) 518ms
 ✓ src/remote/RemoteTerminalGestures.test.tsx (10 tests) 342ms
 ✓ src/remote/terminalGridProtocol.test.ts (6 tests) 6ms
 ✓ src/remote/attentionInventory.test.ts (2 tests) 18ms
 ✓ src/lib/remoteClient.test.ts (3 tests) 7ms

 Test Files  8 passed (8)
      Tests  117 passed (117)
   Start at  07:43:24
   Duration  36.40s (transform 6.29s, setup 2.82s, collect 17.86s, tests 4.37s, environment 7.19s, prepare 916ms)


```
