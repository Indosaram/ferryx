# A10 Unix PTY input reactor initialization repair

Task st_01a0989f. All changes are uncommitted in the shared worktree.

## Owned change and interpretation

Added one Unix `Handle::try_current()` admission check in terminal/pty.rs before input registration and child spawn. Missing runtime now returns `PtyError::SpawnError` through the synchronous fallible API, rather than panicking inside AsyncFd. The already-open PTY pair drops on this error; no child, reader task, registry entry or writer lease is created. All fallible input initialization remains before child spawn. Runtime-backed input initialization, cancellation, output and cleanup implementations are unchanged.

Interpretation: synchronous invalid-command callers must receive a spawn error without a panic or writer claim, not necessarily portable-pty's executable-resolution diagnostic. Without a runtime the error now identifies that missing prerequisite; it does not attempt the invalid executable. Valid synchronous callers without a runtime are also rejected, since successful session creation already requires Tokio reader/lifecycle tasks. Added focused coverage for that typed rejection and empty registry. Existing worktree_safety test is byte-for-byte untouched, including its synchronous attribute and assertions.

This repairs absence of an entered runtime, not explicitly constructed Tokio runtimes with I/O disabled. No new runtime, deferred input registration, post-spawn cleanup branch, dependency or unsafe operation was introduced. Other PTY cleanup defects are outside this task.

Only owned edits: eight production lines in terminal/pty.rs, one test in the existing foreign/untracked tests/pty_input_cancellation.rs, and these A10-input-reactor-* evidence files. session.rs and all remote/server/output/daemon/handover/session_service/vendor/Cargo/UI files were not edited. Existing foreign changes were preserved.

## Exact execution

Working directory for all commands:
`/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`

Each run created its supervisor before starting Cargo (therefore before Ferryx library initialization):

```sh
stage=$(mktemp -d /tmp/a10-input-reactor-red.XXXXXX) # GREEN used a10-input-reactor-green.XXXXXX
mkdir -p "$stage"/{home,runtime,data,sessions,config,cache,state,tmp}
export HOME="$stage/home" FERRYX_RUNTIME_DIR="$stage/runtime" FERRYX_DATA_DIR="$stage/data" FERRYX_SESSION_DIR="$stage/sessions" XDG_CONFIG_HOME="$stage/config" XDG_DATA_HOME="$stage/data" XDG_CACHE_HOME="$stage/cache" XDG_STATE_HOME="$stage/state" XDG_RUNTIME_DIR="$stage/runtime" TMPDIR="$stage/tmp" TMP="$stage/tmp" TEMP="$stage/tmp" CARGO_HOME=/Users/indo/.cargo RUSTUP_HOME=/Users/indo/.rustup CARGO_TARGET_DIR="$PWD/src-tauri/target" CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTC_WRAPPER= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$stage/home/.gitconfig"
```

RED supplied this same environment using `env` rather than `export`.

RED, before production edits:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test worktree_safety pty_spawn_failure_does_not_claim_exclusive_writer -- --exact --nocapture > docs/evidence/paired-daemon/A10-input-reactor-RED.log 2>&1
```

Exit 101: one failed test, panic at terminal/pty.rs:202, `there is no reactor running, must be called from the context of a Tokio 1.x runtime`.

GREEN, one related batch (including real PTY input and multi-session lifecycle):

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test worktree_safety --test pty_input_cancellation -- --nocapture --test-threads=1 > docs/evidence/paired-daemon/A10-input-reactor-GREEN.log 2>&1
```

Exit 0: 3 input tests and 9 worktree safety tests passed. Includes the exact existing synchronous regression, five concurrent worktree terminal lifecycles, same-worktree multiple PTYs, close/natural-exit ownership cleanup and actual saturated cancellable input. No sleep/polling test was added. The input fixture uses actual kernel WouldBlock and a private control socket: original PID 4582 accepted 1022 bytes, received exactly 1023 including the sentinel, not the cancelled LATE bytes. It reports original child reaped, output drain joined and fixture root removed. Valid input is exercised through production PtyManager/PtySession against a real child, not a mock.

Both builds reported 19 existing library warnings, retained in logs without suppression. No Windows/Linux execution or daemon/desktop launch is claimed. The related Cargo test batch compiled the production library and affected tests; no separate application build was run for this single-domain change.

LSP diagnostics on both changed Rust files returned no diagnostics. `git diff --ignore-submodules=all --check` exited 0.

## Owned cleanup

RED supervisor `/tmp/a10-input-reactor-red.R0edcy` and GREEN supervisor `/tmp/a10-input-reactor-green.CZk86H` were removed using `rm -r "$stage"` after each Cargo process returned. GREEN private child fixture `/tmp/a10-input-reactor-green.CZk86H/tmp/.tmpk9a33r` was closed by the passing test. No canonical daemon connection, user PTY action, process discovery/kill, desktop launch, commit or release occurred.

## SHA-256 after GREEN

```text
037d147ab61effd5cad67821f1670a447210b245ddb3618cc11dc4a3d30dc522  src-tauri/src/terminal/pty.rs
78652d1e883749c5360dc6a655700ef59b9d3a5612562f96ce0d413de5ae8a2b  src-tauri/src/terminal/session.rs
515f758eb0592c750c586c12ab583c2af372c79677b1c0d49686da612ea16047  src-tauri/tests/pty_input_cancellation.rs
d02e6af3dd3d9072052ac7189fd94a0bdf1b4dbaada135d0b04b7d7d4700a477  src-tauri/tests/worktree_safety.rs
85dd128faf295d51f41b13a76ea655fe74e7da435ff81f2813176db570a236aa  docs/evidence/paired-daemon/A10-input-reactor-RED.log
3e9d6acd2685cba5124a3aced851754e3e5a5c35d644a57150ecf4dea1993dab  docs/evidence/paired-daemon/A10-input-reactor-GREEN.log
```
