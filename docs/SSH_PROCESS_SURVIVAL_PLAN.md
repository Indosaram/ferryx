# SSH process-preserving automatic reattachment

## Contract

Remote PTYs are owned by a headless remote helper, never by an SSH connection or
the desktop daemon. A local transport/daemon restart must reconnect to the same
remote `TargetRef`, PID, in-memory state and buffered output automatically.
Neither a fresh shell nor an agent resume command satisfies recovery.

Reuse `src-tauri/src/ferryx_scope/ssh` rather than introduce a second PTY owner.
The existing helper is not wired into desktop SSH. The previous fresh-shell
button change is incomplete and its regression expectations must change.

## Corrections to the planning child's proposal

- No fixed sleeps in tests. Use explicit output/state events and bounded deadlines.
  Memory proof uses an interactive nonce/counter process and signal-driven input;
  real PID and nonce must remain identical across reconnect.
- Do not expose `host_store_path` to the frontend or accept arbitrary helper paths
  from IPC. Resolve them from the existing SSH project registry.
- Keep remote output sequence distinct from the local daemon output sequence.
  Persist the remote cursor with `TargetRef`; never compare chunk sequence to
  local byte sequence. Bound frames by serialized size, not raw ring size.
- Helper spawn retries need stable request IDs, dedupe, and request-conflict
  detection. Reads/handshakes may retry; uncertain writes/stops must not replay.
- Protect helper bootstrap with an OS-held exclusive lock. A failed socket probe
  alone is not permission to unlink a live endpoint. Validate ownership, private
  IPC location and symlinks before access, using existing platform patterns.
- A private helper state directory is not a project jail. Canonical registered
  project roots define PTY containment, including remote worktrees.
- Remote helper setup must be explicit and actionable if missing; no automatic
  source compilation, unverified downloads, trust changes, or lossy direct-SSH
  fallback. Provision QA binaries only into unique disposable fixture directories.
  Starting an installed helper is automatic and detached from the SSH channel.
- Old direct-SSH sessions cannot retroactively acquire a persistent PTY owner.
  Preserve any still-live legacy session; if lost, report legacy session loss
  rather than claim process restoration or silently spawn a replacement.
- Preserve agent identity on same-remote-session reattach. Remote owner restart
  changes epoch and is terminal loss, distinct from local daemon restart.

## Phase A: independently buildable remote owner

Three disjoint producers, followed by a verifier:

1. `helper-core` (deep): `src-tauri/src/ferryx_scope/ssh/helper.rs` and its own
   tests. RED/GREEN for project/state separation, idempotent spawn, binary-safe
   input/output, scoped target validation, bounded replay and real PTY survival.
2. `helper-service` (deep): `src-tauri/src/ferryx_scope/ssh/process.rs`,
   `mod.rs` and separate service tests. RED/GREEN for locked atomic endpoint
   startup, safe stale handling, detached start/bridge lifecycle, no PTY stop on
   bridge EOF, Windows/POSIX operation and private endpoint validation.
3. `helper-build` (quick): `remote-helper/Cargo.toml`, lockfile, README and
   `src-tauri/src/ferryx_scope/ssh/standalone.rs`; add standalone minimal Cargo
   package reusing existing helper source and scoped DTOs, without Tauri/GPU deps.
   Pin the existing API with a compiling baseline; no behavior changes here.
4. `verify-helper` (deep), depends on all three: actual isolated helper daemon,
   bridge process EOF/reconnect and PID/nonce proof plus Cargo tests. Capture
   evidence and cleanup in `docs/evidence/ssh-process-survival/helper-*`.
5. Lead audits evidence and commits this independently green helper increment.

Boundary refinement after source verification: Phase A supplies the blocking
daemon owner and disposable bridge. Starting an installed helper detached from
its SSH parent is owned by Phase B runtime setup. Phase A bridge-kill proofs
must not be presented as proof of actual SSH transport-loss recovery.

## Phase B: desktop SSH transport

1. `ssh-bridge` (deep): new `src-tauri/src/ssh/bridge.rs`, `ssh/mod.rs`,
   necessary scoped `direct.rs` integration and tests. Framed OpenSSH client,
   safe installed-helper bootstrap, independent read/control connections,
   explicit remote binding/cursor, safe retries and typed failures.
2. `runtime-setup` (deep): `ssh/operations.rs`, runtime preparation IPC/settings
   contract, supporting scripts. Define and verify explicit helper installation
   from a caller-selected matching binary; never download/compile silently.
   Coordinate shared IPC/module entry files under this one owner.
3. `verify-bridge` (deep), depends on producers: run bridge/helper transport
   tests through real loopback SSH; capture RED/GREEN and trust/cleanup proof.
4. Lead audit and verified commit.

## Phase C: daemon and persisted remote identity

1. `daemon-reattach` (deep): daemon protocol/server/client/store, terminal service
   and IPC integration are one serialized owner. Stable desktop session mapping
   to remote TargetRef/cursor survives local restart; detached local transport is
   not remote process exit. Auto reconnect is deduplicated and backoff bounded.
   Input is rejected during outage. Explicit close alone stops the remote PTY.
2. `verify-daemon` (deep), depends on producer: isolated local daemon restart
   with real helper, same PID/nonce/counter, no new spawn. Test native output
   attachment, sequence replay/gap handling and SSH worktree CWD.
3. Lead audit and verified commit.

## Phase D: user-visible restoration

1. `ui-reattach` (visual-engineering): terminal/IPC types, persistence, restore,
   workspace store, TerminalPane and focused tests. RED first; automatic recovery,
   preserved pane/agent identity, retry reattachment, missing/expired state and
   input freeze. Remove the incorrect SSH shellReplacement bypass.
2. `survival-harness` (deep), independent of UI source: implement
   `scripts/qa/ssh-process-survival.mjs` and isolated Rust integration fixtures.
   Expose `--scenario transport-loss|daemon-restart|reconnect-safety`.
3. `verify-ui-and-harness` (deep), depends on both: actual integration tests,
   UI build, real browser/native debug interaction as appropriate; capture all
   output and screenshots. No desktop release build.
4. Lead audit and verified commit.

## Phase E: complete result verification

1. `transport-loss`: real OpenSSH, cut only QA transport; automatic reconnect to
   identical remote PID/TargetRef/nonce with retained mutable memory.
2. `daemon-restart`: restart only isolated local daemon, retain remote process;
   automatic restore through persisted mapping and frontend lifecycle.
3. `reconnect-safety`: duplicate reconnect, input outage rejection, missing session,
   changed remote epoch, host-key failure, legacy sessions and bounded retries.
4. `platform-parity`: actual remote helper/bridge on macOS, Linux `omarchy`,
   Windows `maho-win`; no POSIX-only substitute for Windows behavior.
5. `desktop-ui`: `bun tauri dev` only, isolated fixture if a native debug run is
   needed. Verify automatic state restoration and screenshots at desktop/mobile
   sizes via faithful surfaces. Never disturb production sessions.
6. Cleanup every resource as a paired task and capture a receipt for each scenario.
7. Final regression suites/build, owned-diff self-review, report
   `docs/verification/ssh-process-survival-20260909.md`, verified commits.

Each phase is a separate native workflow run. Production nodes own their own
failing-first proofs; verifier nodes re-run actual checks rather than accept
producer claims. Subsequent phase details may be refined only from observed
contracts, append-only in the durable notepad. No ulw-plan reviewer gate applies.
