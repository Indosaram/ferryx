# Rust core review: crate root, CLI, agent detection, DAG watcher

Scope reviewed: `src-tauri/src/cli.rs`, `src-tauri/src/main.rs`, `src-tauri/src/clipboard_image.rs`,
`src-tauri/src/macos_file_drop.rs`, `src-tauri/src/agent_detect/*`, `src-tauri/src/dag/watcher.rs`.
No source file was modified. No cargo/bun command was run; every claim below comes from reading the
cited lines.

### [P1] DAG watcher parks forever after a native watch is lost; it never falls back to polling
- Location: src-tauri/src/dag/watcher.rs:198 (and the select arms at :208 and :232)
- Observed: when the native watch is lost, the recovery arm tears the watch down but leaves the
  polling flag untouched: `lose_watch = None;` / `drop(_watcher_guard.take());` /
  `notify_rx.close();` followed by a single `scan_and_emit` and `hooks.event("lost-scanned")`.
  `polling_mode` was bound once at :167 (`let (polling_mode, mut _watcher_guard) = match watcher_res`)
  and is `false` whenever the watch armed successfully. After the teardown, the notify arm is gated
  off by `recv_res = notify_rx.recv(), if !notify_rx.is_closed()` (:208) and the reconciliation arm
  is gated off by `_ = poll_interval.tick(), if polling_mode` (:232). The `lose_watch` arm is now
  `None` (`std::future::pending()`) and `debounce_sleep` is `None`. Every arm that can make progress
  is disabled, so `select!` parks the task permanently.
- Why it is wrong: this is a production defect, not an environment artifact. In production
  `lose_watch` is `None`, but the identical state is reached whenever the OS backend stops
  delivering events for the watched inode — the common case being `.omo/senpi-task/dag` being
  deleted and recreated (journal cleanup, worktree reset, `git clean`), or an inotify/FSEvents
  backend dropping the handle. From that moment the watcher emits nothing: DAG run progress in the
  UI freezes at the last snapshot for the rest of the app's lifetime, with no error and no log. Only
  an app restart recovers it. This is exactly what
  `dag::watcher::tests::test_dag_watcher_recovers_after_silent_watch_loss` asserts: the test's first
  `rx.recv()` times out and `.expect("recreated journal must emit after silent watch loss")` panics.
  Verdict: **production defect**.
- Minimal fix: make `polling_mode` a mutable local and set `polling_mode = true;` inside the
  watch-loss branch (next to `lose_watch = None;`), so the 1 s `poll_interval` arm at :232 becomes
  the reconciliation path once the native watch is gone. (Re-arming a fresh `RecommendedWatcher` on
  the recreated directory is a valid superset, but flipping to polling is the smallest change that
  restores liveness.)

### [P1] `--handover-from` with a missing value silently degrades a handover into a cold daemon start
- Location: src-tauri/src/cli.rs:468
- Observed: `args.get(pos + 1).map(std::path::PathBuf::from)` — the token after `--handover-from` is
  taken verbatim with no validation, and a missing token yields `None`. Contrast the browser parser's
  `required_option`, which both rejects flag-shaped values and errors on a missing one
  (cli.rs:46: `.filter(|value| !value.starts_with("--"))`).
- Why it is wrong: `ferryx --daemon --handover-from` (value dropped by a wrapper script, a shell
  glob that expanded to nothing, a truncated argv) returns `None`, and `run_daemon_headless` then
  takes the non-handover branch: `let announce_readiness = handover_from.is_none();` (cli.rs:470) is
  `true`, so the process prints `FERRYX_DAEMON_READY` and
  `run_server_with_handover_and_readiness(None, ...)` never adopts the legacy socket
  (daemon/server.rs:1455 `if let Some(ref legacy_path) = handover_from`). The operator asked for a
  handover and silently got a fresh daemon that leaves the old socket unmigrated — no error, no
  non-zero exit. The flag-shaped-value case (`--handover-from --daemon`) is the benign half: it
  fails loudly later in `validate_runtime_socket_path`.
- Minimal fix: reuse the existing validator instead of hand-rolling the lookup — have
  `parse_handover_from` return `Result` built on `required_option(args, "--handover-from")` when the
  flag is present, and have `main`/`bin/cli.rs` exit non-zero on `Err` rather than silently
  continuing.

### [P2] `scan_and_emit` performs blocking filesystem I/O directly on the async runtime
- Location: src-tauri/src/dag/watcher.rs:71 and :81
- Observed: the scan runs inline in an `async fn` with no `spawn_blocking`:
  `let entries = match std::fs::read_dir(target_dir) {` (:71) and, per candidate file,
  `if let Ok(content) = std::fs::read_to_string(&path) {` (:81), preceded by `path.is_file()` /
  `is_dir()` metadata syscalls and followed by JSON parsing.
- Why it is wrong: every scan occupies a Tokio worker for the duration of a directory walk plus N
  synchronous file reads and JSON parses. On a slow or network-backed journal directory, or with a
  large `runs/` directory, this stalls other tasks scheduled on that worker; on a current-thread
  runtime it stalls the entire runtime. `dag::watcher::tests::test_dag_scan_runs_off_async_worker`
  demonstrates it deterministically: it joins the scan with a sentinel task
  (`tokio::join!(biased; scan, sentinel)`) and the sentinel cannot run until the scan's barrier is
  released, so `sentinel.expect("async sentinel must execute before scan release")` fails.
  Verdict: **production defect** (the blocking calls are production code; only the observation hook
  is test-only).
- Minimal fix: move the directory walk, file reads and `parse_run_checkpoint` into
  `tokio::task::spawn_blocking`, returning the collected snapshots, and keep only the `sink.send`
  loop on the async side.

### [P2] `required_option` rejects any legitimate option value that begins with `--`
- Location: src-tauri/src/cli.rs:46
- Observed: `.filter(|value| !value.starts_with("--"))` — a present value that happens to start with
  two dashes is discarded and reported as `missing value for {name}`.
- Why it is wrong: the browser CLI passes free-form user text through the same helper.
  `ferryx browser fill --browser-id b1 --generation 3 --ref e2 --value "--no-reply"` and
  `ferryx browser keypress ... --key "--"` fail with a misleading "missing value for --value"
  even though the value was supplied. There is no escape hatch (no `--` terminator handling), so
  those values are simply unreachable from the CLI.
- Minimal fix: only apply the flag-shaped-value guard to options with constrained value grammars
  (`--browser-id`, `--generation`), or honour a `--` separator so `--value -- --no-reply` passes the
  literal through.

### [P2] macOS file drop silently collapses a multi-file drop to a single file on the fallback path
- Location: src-tauri/src/macos_file_drop.rs:126
- Observed: after the legacy `NSFilenamesPboardType` array path fails, the fallback reads one string
  and returns a one-element vector: `return vec![path.to_string()];` (reached from
  `pasteboard.stringForType(file_url_type)` at :122). `NSPasteboard::stringForType` returns the
  representation of the *first* pasteboard item only.
- Why it is wrong: on the very macOS releases this fallback exists for (the comment at :120-121 says
  "Finder stops advertising the legacy type in some macOS releases"), dragging five files onto the
  window attaches exactly one, chosen arbitrarily by pasteboard ordering, with no warning. The user
  believes all five were attached.
- Minimal fix: iterate `pasteboard.pasteboardItems()` and read `NSPasteboardTypeFileURL` from each
  item, collecting every resolved path, instead of the single `stringForType` read.

### [P3] `parse_launch_mode` scans the entire argv, including option values
- Location: src-tauri/src/cli.rs:454
- Observed: `for arg in args { if arg.as_ref() == "--daemon" { return LaunchMode::Daemon; } }` — no
  positional awareness, and `args` includes `argv[0]` and every option value.
- Why it is wrong: any option value equal to `--daemon` flips the app into headless daemon mode.
  Today the `browser`/`pair`/`remote` subcommands return before this call (main.rs:8-34), so the
  reachable cases are narrow; conversely an unrecognised flag such as `--deamon` silently launches
  the GUI instead of reporting an unknown argument. It is a latent parsing sharp edge rather than a
  live defect.
- Minimal fix: match `--daemon` only among leading flags (stop at the first non-flag token), and
  reject unknown `--`-prefixed arguments with a usage error.

## Notes on things checked and found sound
- `agent_detect::engine::detect` — the `RuleState::Unknown => unreachable!()` at engine.rs:81 is
  genuinely unreachable: the guard at engine.rs:68 returns early for both `skip_state_update` and
  `RuleState::Unknown`, and the `previous == None` case yields `None` rather than panicking. No
  screen content can panic the detector; unmatched screens hold the previous state (engine.rs:88-94).
- `clipboard_image::dib_to_png` — all header reads are bounds-checked (`read_u32`/`read_u16` via
  `bytes.get(..)?`), the stride uses `checked_*` arithmetic, and the pixel buffer length is validated
  (`if pixels.len() < stride.checked_mul(height)?`) before the per-row slicing, so the indexing at
  clipboard_image.rs:277 cannot go out of bounds.

Summary: P0=0, P1=2, P2=3, P3=1
