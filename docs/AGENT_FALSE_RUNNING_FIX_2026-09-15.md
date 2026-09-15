# Agent false-running state repair

## Confirmed causes and changes

- `ui/src/state/workspaceStore.ts`: terminal exit events now reach parked workspace snapshots, not just the mounted project. Remote `missing`, `expired`, and `legacyLost` states and confirmed-missing backend failures settle activity. Local failed terminals settle activity as exited terminals already did.
- Late screen or title reports cannot restore working/waiting activity on an exited, failed, or unbound session. A new backend binding permits new activity again.
- `selectAgents` no longer equates a live PTY with observed agent work. Without activity evidence it reports `starting`, not `working`.
- `src-tauri/src/terminal/foreground.rs`: an intervening unrelated process no longer consumes the observed agent-to-shell transition. Unknown process observations still preserve the current state.
- `src-tauri/src/daemon/{agent_state,protocol,proxy,server}.rs`: state frames distinguish agent reports, process releases, process observations, and manual resets. The origin survives snapshots and legacy forwarding.
- `src-tauri/src/native_terminal/surface_host.rs`: an agent's own idle report and a confirmed process release suppress stale screen inference. A subsequent agent-process observation re-enables inference; a manual reset retains its existing re-enable behavior. This prevents a leftover spinner from turning idle back into working.

No elapsed-time downgrade was added. A quiet live agent and a disconnected SSH transport retain their activity; neither is evidence of process death.

## Frontend evidence

- New lifecycle regression suite covers remote loss versus disconnection, failed terminals, late screen/title events, rebind recovery, and bare PTY classification.
- New parked-workspace integration test drives the real workspace hook, terminal event bus, and snapshot cache. Subscription readiness is awaited directly, without polling delays.
- Lead verification: `bun run --cwd ui test src/state/agentLifecycleTruth.test.ts src/state/parkedLifecycleRouting.test.tsx src/state/workspaceStore.test.tsx`: 3 files, 80 tests passed, exit 0.
- Implementation worker verification: state/lib suites passed 3,504 tests in 112 files; TypeScript and Vite build passed. Full suite had 4,913 passes and 13 failures: six reproduced at clean HEAD, seven belonged to concurrent settings changes. This was not a clean full-suite result.
- Existing extension test uses a real loopback TCP listener: `bun run --cwd ui test src/lib/sshAgentState.test.ts`: 1 passed, exit 0.
- Real TabBar/WorktreeList/reducer browser page at desktop 1280x900 and mobile 390x844: working produced two spinner elements; exit and stale post-exit reports produced zero; a new backend's genuine work restored two. Failed lifecycle and late title scenarios produced `done` and zero spinners at both sizes.
- Browser verification is DOM/state evidence, not desktop application or pixel approval. The available image reader could not inspect the generated screenshot.

## Backend verification

The production native stream pump regression `process_release_hands_the_session_back_to_screen_inference` passed: one test, exit 0. It feeds the actual stale `Working (esc to interrupt)` text after release, then a process-observed frame and a new working screen to prove recovery.

Final checks, all exit 0 (commands prefixed with `RUSTC_WRAPPER=` after an earlier compiler process received external SIGTERM):

- `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_state -- --nocapture`: 26 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::foreground -- --nocapture`: 6 passed, including a real PTY process-exit test.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib an_agents_own_idle -- --nocapture`: 2 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib process_release_hands -- --nocapture`: 1 passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --tests`: passed.

The Rust language server timed out obtaining fresh diagnostics after the final edit. Cargo compiled the test targets successfully; existing unrelated unused-variable/dead-code/unsafe warnings remain. The changed paths passed `git diff --check`. Full Rust tests and Windows/Linux runtime checks were not run.

## Deployment and compatibility boundary

The running app and user daemons were not restarted, replaced, or killed. Source changes are not proof that an already-running process has loaded this fix.

Old daemon frames omit the new origin and cannot distinguish a self-reported idle from a process release. They decode conservatively as agent-owned state. Mixed-version sessions need a new origin-carrying report before that distinction can be recovered; no destructive upgrade is required or performed here.

Changes remain uncommitted in the shared working tree. Concurrent settings and notification changes were left untouched.
