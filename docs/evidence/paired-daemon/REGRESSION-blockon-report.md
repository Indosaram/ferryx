# Subprocess runtime regression: fixed; aggregate remains RED

Task st_01a099c9. Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

## Delivered

Only production source edited: `src-tauri/src/remote/workspace_api.rs`.

The synchronous registration API reaches project projection, then git, then the child supervisor. The old supervisor nested `Handle::block_on` inside callers that include current-thread Tokio tests. Runtime presence does not distinguish blocking workers from async tasks; `block_in_place` cannot support current-thread callers.

The child supervisor is now an async function. Its synchronous adapter runs it on a scoped OS worker with a private current-thread IO/time driver. Subprocess creation, pipe reads, timeout selection, and kill/wait cleanup run independently of the caller's runtime. No shared runtime lifecycle, nested runtime entry, or dependence on a parked caller reactor remains. The unnecessary runtime construction in registration_project was removed.

The existing synchronous API still waits for its result. Production async transports must and do retain their enclosing run_blocking boundary. This is compatibility for synchronous domain callers, not a claim that a synchronous API can yield an async task. One short-lived worker/runtime per subprocess is the deliberate overhead tradeoff.

RequestContext checks, CancelWork signals, authorization revocation, output limits, child deadlines and explicit reaping are retained. Four added tests cover no runtime, direct current-thread context, direct multithread context plus run_blocking, and event-synchronized cancellation/revocation of active children with waitpid proving reaping. No existing test/assertion/ignore attribute or rollout flag was changed.

## Verification actually executed

- Full requested library command: **1031 passed; 3 failed; 1 ignored**, 262.25 seconds test time. See `REGRESSION-blockon-RED.log` (copy of `REGRESSION-blockon-full-45790.log`). No GREEN file was produced.
- Requested paired_host command: **36 passed; 0 failed**, `REGRESSION-blockon-paired-host-45790.log`.
- Requested lib check: exit **0**, `REGRESSION-blockon-check-lib-45790.log`.
- Requested CLI/relay check: exit **0**, `REGRESSION-blockon-check-bins-45790.log`.
- Debug lib/CLI/relay build: exit **0**, `REGRESSION-blockon-build-entrypoint-61542.log`.
- Changed Rust file LSP diagnostics: no diagnostics (one intervening refresh timed out; final refresh succeeded).
- CLI `--help` prints headless usage but exits **1**; relay `--help` exits **2**. These are not claimed as successful functional exercises. No daemon or relay was started by these explicit entrypoint invocations.
- Checks/build report 19 existing warnings outside this change; none suppressed. The first compilation caught an unused doc comment introduced here; it was moved onto the function before the paired-host recompile/checks/build.

The full suite includes all four new child tests passing, plus the unchanged security/idempotency tests `test_omo_resume_cwd_cannot_escape_workspace`, `test_server_spawn_cwd_validation`, and `test_server_spawn_idempotency_cache` passing. Aggregate discovery was 1035 tests, not the supplied earlier 1025; this work added only four. The ignored `dag_live_tail` already has #[ignore] in HEAD.

## Remaining blockers / exact failures

1. `daemon::server::tests::test_runtime_paths_are_profile_aware`: expected `/tmp/rorca-501-dev`, got private `/tmp/blockon-st_01a099c9-h9waivts/runtime`. The unmodified test hardcodes the default despite the explicit FERRYX_RUNTIME_DIR override needed for runtime isolation. Removing isolation risks the forbidden canonical daemon. Production get_runtime_dir correctly honors its override; changing it to satisfy the test is not valid.
2. `ipc::tests::tauri_mock_worktree_commands_use_identity_contract`: `state() called before manage() for Arc<DaemonClient>`. The unchanged test manages only WorkspaceRegistry. Current cmd_worktree_create/delete require the managed DaemonClient for authority-owned mutations. This is unrelated to subprocess supervision. A real private daemon fixture would preserve assertions, but changing this pre-existing test's setup requires parent confirmation given the strict no-test-weakening requirement. Adding a registry-only mutation fallback to production would bypass the new authority contract and is not appropriate.
3. `tests::app_remote_state_persists_pairing_but_starts_off`: expected Off, got Relay. RemoteCreatePairingCode calls handle_remote_configure with Relay, which is persisted and subsequently restored. The auto-enable pairing branch also exists in HEAD, independently verified. Deciding whether pairing should persist Relay or reopening should reset Off affects pairing/restart semantics outside this subprocess change.

These failures are preserved, not ignored, weakened, or misrepresented. No more subprocess edits can resolve these contract conflicts. Parent coordination is required to resolve the remaining contract/test-environment decisions before the demanded aggregate GREEN can honestly be delivered.

## Isolation / assumptions

Validator uses private HOME, runtime/data/session/XDG directories, agent socket and git config; Cargo/Rustup installations are used through explicit tool paths. No canonical daemon/GUI/PTY operations, commits, destructive git operations, release builds, or desktop automation were performed. Private QA directories are retained for inspection rather than destructively cleaned while suite-created resource ownership is uncertain. Commands and exit codes are captured directly without status-losing pipelines; logs have per-process names. The canonical runtime override was never removed.
