# Daemon increment: lead verification

Status: backend increment verified on macOS. Full application acceptance remains
incomplete until frontend wiring, remaining safety cases and platform validation.

## Implemented contract

The local daemon stores the remote TargetRef and stable local backend ID before
acknowledging a registered SSH spawn. Startup restores that identity without
spawning a replacement or invoking an agent resume command. A fresh daemon
replays the helper-retained history from cursor zero into a fresh local hub;
remote attachment explicitly resets the client parser before replay.

Reconnect uses independent SSH read/control connections, bounded retry,
generation-checked input and target-specific close. A control failure interrupts
a pending read instead of waiting indefinitely. Duplicate controllers for the
same remote target are rejected. Dropping the local runtime does not stop the
remote PTY.

Remote retention gaps are ordered items in the output stream. They invalidate
stale retained history and reach native clients through the existing Gap frame
before recovered output. Remote cursor metadata is not used as a local sequence.
Structured setup error codes/details survive the bridge; only raw OpenSSH stderr
is classified from diagnostic text.

## Lead-executed evidence

- `ssh_process_survival`: 15 passed, including runtime same-target replay and
  ordered hub gap/late-attachment behavior. `gap-stream-green.log`.
- `ssh_reconnect_safety`: 16 passed, including actual daemon stream and Unix proxy
  gap ordering, native pump-to-desktop state events, setup classification and
  helper auth/target isolation. `gap-safety-green.log`.
- Subsequent control-failure and duplicate-target regressions: each passed in
  the lead's separate run, `controller-safety-lead-green.log`. Their original
  failing assertions are in `controller-producer-red.log`; the producer's full
  242-test terminal GREEN was audited and retained in
  `controller-terminal-green.log`.
- `ssh_daemon_restart`: 3 passed. Local `test_agent_resume`: 13 passed.
  Local `test_server_spawn`: 2 passed.
  `daemon-local-regressions-lead.log`.
- `direct_ssh_real_transport_registration_and_pty`: passed, including registered
  SSH spawn, worktree CWD, gateway routing, actual remote identity restoration and
  revocation checks. `registered-worktree-lead.log`.
- All three exact `bun scripts/qa/ssh-process-survival.mjs --scenario ...`
  invocations passed in the lead's run, `lead-scenarios.log`:
  - Transport loss: PID 79073, nonce
    `926b8667-be8d-4071-a2a1-9854e2599368`, counter 1 to 2.
  - Isolated daemon process restart: PID 79155, nonce
    `68a04bd8-edbb-46dc-965f-298fbee1aae8`, counter 1 to 2.
  - Outage safety: PID 79230, nonce
    `55da6703-ca8b-4233-bb71-a276299f4d54`, counter 1 to 2 to 3.
    The last recovery intentionally uses explicit retry after the bounded
    automatic budget; it is not represented as click-free recovery.

The scenario runner compiles a dedicated unoptimized example executable. It is
not an environment-dependent test that breaks ordinary Cargo test discovery.
Each scenario preserves TargetRef/PID/nonce and records unchanged trust hashes,
zero remaining exact-fixture descendants/sockets, and fixture removal.

## Cleanup correction

The producer's separate registered-worktree test leaked four detached helper
owners after deleting its temporary directories. The lead detected PIDs 65056,
65431, 71754 and 72140, verified their exact QA command lines, subscribed to native
process-exit events and terminated only those helpers. Receipt:
`regression-helper-cleanup.log`.

That fixture now keeps a foreground helper Child guard and waits for its exit
on teardown. The repaired real regression passed and a subsequent process
snapshot found no helper daemon processes. The temporary cleanup program was
removed. No user daemon, terminal session or SSH trust file was modified.

## Scope and remaining gates

The scenario seed uses the production RemoteRuntime create API; production
registered Spawn admission is covered by the separate real worktree regression,
not by the seed. The scenario pane hash is a fixture association, not frontend
layout verification. UI IPC registration/lifecycle, desktop/mobile screenshots,
WebSocket gap handling, changed-helper-epoch and legacy-session error cases,
and final Linux/Windows application checks remain required.

LSP was unavailable at the configured daemon socket. Successful Rust compilation,
example builds and the tests above are the compiler substitute, not a claim of
clean LSP diagnostics. Sixteen existing compiler warnings remain outside this
increment. Full `cargo check --tests` also has a reported pre-existing
`scoped_design` PNG buffer-size type error; no test was weakened to hide it.
