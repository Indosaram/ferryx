# Rust Tauri IPC command layer review — 2026-09-14

Scope: `src-tauri/src/ipc/` excluding `browser_cli.rs` and `worktree.rs`.
Only findings whose decisive lines were opened and confirmed are listed. The 20-byte terminal
output framing (`src-tauri/src/ipc/terminal.rs:441-466`) was checked and is *not* truncating:
`1 + 1 + 2 + 8 + 8 == 20`, and the only narrowing cast is a checked
`u16::try_from(session_id.len()).ok()?` that falls back to the JSON path, so it is not reported.

### [P1] SSH host store mutations are an unguarded read-modify-write onto a shared fixed temp file

- Location: src-tauri/src/ipc/ssh.rs:206 (mutating commands at src-tauri/src/ipc/ssh.rs:235-236, 262-274, 285-297)
- Observed: every mutating SSH command does load → mutate → save with no lock, e.g. `cmd_ssh_update_host` runs `let mut store = load_store(&path)?;` (ssh.rs:264) … `save_store(&path, &store)?;` (ssh.rs:274) inside `run_blocking`, and `save_store` stages through one process-wide constant filename: `let tmp = path.with_extension("json.tmp");` (ssh.rs:206) followed by `std::fs::write(&tmp, serialized.as_bytes()).and_then(|()| std::fs::rename(&tmp, path))` (ssh.rs:207-208). Each `run_blocking` call lands on a different `spawn_blocking` thread (`src-tauri/src/ipc/mod.rs:59-64`), so two in-flight invokes execute this sequence concurrently. The sibling remote-project store in the same directory does it correctly: `static MUTATION: Mutex<()>` (src-tauri/src/ssh/projects.rs:10) held across read+write (projects.rs:156-158) plus a per-write unique temp `path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()))` (projects.rs:125).
- Why it is wrong: two overlapping invokes (the settings UI can trivially fire delete-host and update-host, or import-config and update-host, back to back) produce a lost update — the second writer serializes a snapshot taken before the first writer's change, so a deleted host reappears or a just-saved edit silently vanishes. Worse, both writers open and truncate the same `ssh_hosts.json.tmp`; interleaved `write` calls of different lengths leave a spliced buffer that is renamed over the real store, after which `load_store` fails with `Failed to parse ssh store` (ssh.rs:180-185) for every subsequent call and the user's entire saved SSH host inventory — and with it every remote project that resolves through it — is unusable, with no backup on disk.
- Minimal fix: mirror `ssh/projects.rs`: add a `static SSH_STORE_MUTATION: Mutex<()>` in `ipc/ssh.rs`, take it at the top of each `run_blocking` closure that does load+save (`cmd_ssh_import_config`/`import_config_into_store`, `cmd_ssh_update_host`, `cmd_ssh_delete_host`), and change line 206 to a unique temp: `let tmp = path.with_extension(format!("{}.json.tmp", uuid::Uuid::new_v4()));`.

### [P1] `dag_watch_project` marks a project permanently watched even when the watcher task dies, so run updates never resume

- Location: src-tauri/src/ipc/dag.rs:84-92
- Observed: the command registers the root by set insertion and only spawns a watcher on the first insert: `watched.insert(canonical.clone())` (dag.rs:88) gating `if is_new_root { … crate::dag::watcher::spawn_dag_watcher(PathBuf::from(&canonical), tx); … }` (dag.rs:90-92). Nothing anywhere removes an entry from `dag_watched_roots()` — the only other reference is a read inside the test at dag.rs:346. The watcher task it spawns is not permanent: `run_watcher_loop_observed` returns early when the initial hydrate fails (`if !scan_and_emit(...) { return; }`, src-tauri/src/dag/watcher.rs:141-142 and 179-181) and `break`s out of its select loop on sink failure or a closed notify channel (watcher.rs:204, 215, 228, 234).
- Why it is wrong: once that task exits — receiver dropped on the UI side, or the journal directory not yet present at first hydrate — the canonical path is still in the set, so every later `dag_watch_project` call for that project takes the `is_new_root == false` branch and never re-arms a watcher. The user sees a DAG panel that hydrates once from `load_current_snapshots` and then goes permanently stale: no `dag-run-updated` events for the rest of the app session, with no error surfaced. Reopening the project does not help; only an app restart does.
- Minimal fix: make the registration reflect liveness — keep the spawned `JoinHandle` (or a liveness flag) in the map instead of a bare `String`, and remove the entry when the watcher task returns, e.g. wrap the spawn in a task that awaits the watcher then does `dag_watched_roots().lock()…remove(&canonical)`, so the next invoke re-arms.

### [P2] `dag_get_run` joins the caller-supplied `run_id` into a filesystem path with no validation

- Location: src-tauri/src/ipc/dag.rs:148-153
- Observed: the run id arrives straight from the IPC boundary and is used to build read candidates with no character or component check: `let clean_id = run_id.strip_prefix("dag_").unwrap_or(&run_id);` then `runs_dir.join(format!("dag_{clean_id}.json")), runs_dir.join(format!("{run_id}.json")), runs_dir.join(&run_id),` (dag.rs:148-153); the third candidate takes the raw string, so `run_id = "../../../../etc/hosts"` escapes `runs_dir` entirely before `std::fs::read_to_string(candidate)` (dag.rs:157). `project_path` (dag.rs:142) is likewise unvalidated, so the runs directory root is caller-chosen too.
- Why it is wrong: the command reads any file on disk the app user can read, outside the project's `runs` directory. The desktop capability grants this to the main webview only (`src-tauri/capabilities/default.json`), so the practical blast radius today is a compromised or injected renderer script rather than a remote attacker — but it is an unvalidated path crossing a trust boundary in a shipped command, and any future exposure of this command over the remote gateway turns it into arbitrary file disclosure. Content only escapes through successful checkpoint parsing, so this is a boundary defect, not confirmed disclosure of arbitrary bytes.
- Minimal fix: reject ids that are not plain file-stem tokens before building candidates — e.g. at dag.rs:148 add `if run_id.is_empty() || run_id.contains(['/', '\\']) || run_id.contains("..") { return Ok(None); }` — and drop the bare `runs_dir.join(&run_id)` candidate at dag.rs:152.

### [P3] `cmd_terminal_spawn` writes unconditional `eprintln!` diagnostics on a hot shipped path

- Location: src-tauri/src/ipc/terminal.rs:583 (repeated at 632, 653, 690, 703, 733, 754, 773)
- Observed: every spawn logs to raw stderr regardless of build profile or log level: `eprintln!("[cmd_terminal_spawn] request received has_worktree={has_worktree} has_cwd={has_cwd} has_client_request_id={has_client_request_id}");` (terminal.rs:583-585), while the rest of the module already uses `tracing::debug!`/`tracing::error!` (e.g. terminal.rs:494, worktree_disk.rs:251).
- Why it is wrong: release builds spam stderr on every terminal spawn, split, and restore, bypassing the tracing filter operators use to control diagnostics, and these lines cannot be silenced or redirected with the rest of the app's logging.
- Minimal fix: convert each `eprintln!` in `cmd_terminal_spawn` to the matching `tracing::debug!`/`tracing::warn!` call.

Summary: P0=0, P1=2, P2=1, P3=1
