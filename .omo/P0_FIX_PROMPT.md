# Task: Implement P0 fixes F1, F6, F3, F4 from FERRYX_PARITY_FULL_SWEEP.md

Repo: /Users/indo/code/project/orca-lite (Tauri + React + xterm app, brand: Ferryx)

Implement exactly these four fixes. No other refactoring. Do NOT create git commits.

## F1 [BLOCKER] Restored terminal tabs attach to dead backend sessions after app restart

Context:
- ui/src/App.tsx (~lines 78-125): restoreWorkspace(restoredState) restores tabs first; each restored tab has alreadyOpen=true so ensureTabForWorktree (spawn) is skipped.
- ui/src/lib/sessionPersistence.ts (~108-116): restored tabs carry backendSessionId from the PREVIOUS process. The backend PTYs live inside the Tauri process PtyManager (src-tauri/src/ipc/terminal.rs) and all die on restart.
- App.tsx (~line 91-92) calls listTerminalSessions() but discards the result (.catch(() => []) then unused).

Fix:
- After restoring the workspace, compare each restored tab/pane's backendSessionId against the live session list from listTerminalSessions().
- For any backendSessionId that is not live: null it out in the restored state (so persistence no longer claims it) and re-spawn the terminal for that tab/pane (reuse the existing spawn path used when alreadyOpen is false).
- Keep behavior identical when the session IS live (dev/HMR case: backend survives, tab should reattach as today).
- If ui/src/lib/tauriTransport.ts has a listSessions()/attach contract, add a liveness check there only if it is a small, natural fit; do not redesign the transport layer.

## F6 [BLOCKER web/remote] Deleted rorca-icon.svg references break SW install + violate branding

Context:
- ui/public/sw.js line 7: cache.addAll includes '/src/assets/rorca-icon.svg' (deleted file -> 404 -> SW install fails entirely). SW registers only in non-Tauri environments (ui/src/main.tsx ~15-17), i.e. web/remote client mode.
- ui/public/manifest.webmanifest: name "rorca Remote", short_name "rorca", icons referencing the same deleted asset.

Fix:
- Replace the rorca-icon.svg reference with the existing ui/src/assets/ferryx-icon.svg (use the correct URL for what sw.js can actually cache at runtime; if '/src/assets/...' was never a valid runtime URL, point it to a file that exists under ui/public/ or the built asset path — verify what actually resolves in the built output).
- manifest.webmanifest: name -> "Ferryx Remote", short_name -> "Ferryx", icons -> ferryx icon asset(s). Do not downgrade to legacy names or colored assets.

## F3 [MAJOR] terminalHostManager duplicate listeners + stale closures

Context: ui/src/lib/terminalHostManager.ts ~lines 150-168: delegated onBell/onTitleChange (150-159) are ALSO registered a second time as direct closures (160-168) -> callbacks fire twice and stale initial closures get pinned.

Fix: Remove the duplicate direct-closure registrations; keep only the delegated ones.

## F4 [MAJOR] xterm instance leak — destroy() never called

Context: terminalHostManager.ts ~233-242 destroy() cleans resizeObserver/disposables/WebGL/terminal.dispose(), but neither tab close (CLOSE_TAB in ui/src/state/workspaceStore.ts ~295-345) nor pane close (~712+) ever calls it, so instances stay in this.instances for the whole session.

Fix: Wire destroy() into the tab-close and pane-close paths in workspaceStore.ts (or wherever the store notifies the host manager). Make sure closing one pane of a multi-pane tab destroys only that pane's instance, and closing a tab destroys all its panes' instances.

## Verification (must run, must pass)

From ui/:
1. `bun run build` (tsc + vite build) — exit 0, no new type errors.
2. `bun test` — all tests pass; if a test fails for a pre-existing reason, state that clearly with the failure output.
3. Re-read each edited file after editing to confirm the change landed as intended.

## Report

End with a concise summary: files changed, how each of F1/F6/F3/F4 was fixed, and verification results.
