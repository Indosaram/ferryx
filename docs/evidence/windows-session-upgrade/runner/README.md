# Windows session-upgrade phase1 runner (maho-win)

Task-owned verification scripts. They never touch installed daemons/GUI, `ferryx-releases`, or `ferryx-build*`.
Scope: foundation modules only (`terminal::session_host::{capability,fence,protocol,registry,win_pipe,win_spawn}`,
`terminal::output_hub`). host_main/client are not in the tested tree; no runtime readiness is claimed by this run.

## Frozen paths

| What | Path |
|------|------|
| Checkout | `/Volumes/T9-Mac/project/ferryx-windows-session-upgrade` (HEAD must equal `38276c9c584221122efc2274852e457c080a449b`) |
| Run id | `wsu-<parent8>-<deltaManifestSha256[0:8]>-rNN` |
| Local stage | `/tmp/ferryx-wsu/<RUN>/remote/in/` |
| Local evidence | `docs/evidence/windows-session-upgrade/<RUN>/` (`run.json`, `manifest.tsv`, `in-manifest.tsv`, `frozen-paths.txt`, `local/`, `remote/`) |
| Remote root | `C:\Users\sook\ferryx-wsu\<RUN>\` (`in, src, target, env, ferryx, logs, evidence, cleanup, procs.jsonl, run-start.txt`) |
| Ghostty junction | `src\src-tauri\vendor\ghostty` -> `C:\Users\sook\ferryx-ghostty` (pin `6a508fd5e34c7e222c052a6d00bb3891ff3feace`) |

The evidence directory is excluded from the delta, so the runner is not part of the tested tree.

## Fixtures

| Step | Command (cwd `src`) | Fixture files |
|------|---------------------|---------------|
| build-bin | `cargo build --locked --manifest-path src-tauri\Cargo.toml --bin ferryx` | whole crate; artifact `target\debug\ferryx.exe` |
| lib-session-host | `cargo test --locked ... --lib -- terminal::session_host::` | `src-tauri/src/terminal/session_host/*.rs` |
| lib-output-hub | `cargo test --locked ... --lib -- terminal::output_hub::tests::` | `src-tauri/src/terminal/output_hub.rs` |
| contract | `cargo test --locked ... --test windows_session_host_contract` | `src-tauri/tests/windows_session_host_contract.rs` (absent => EXIT=3 MISSING_UPSTREAM) |
| lib-full | `cargo test --locked ... --lib` | whole lib |

Cargo steps run with `--config <root>\env\cargo-qa.toml` (clears rustc-wrapper) and require `ui-build EXIT=0` plus a real `ui\dist\index.html`.

## Order (run from `docs/evidence/windows-session-upgrade/`)

```
bash runner/stage.sh 01                 # prints RUN=...
bash runner/run.sh push <RUN>
bash runner/run.sh preflight <RUN>
bash runner/run.sh checkout <RUN>
bash runner/run.sh step ui-install <RUN>
bash runner/run.sh step ui-build <RUN>
bash runner/run.sh step build-bin <RUN>
bash runner/run.sh step lib-session-host <RUN>
bash runner/run.sh step lib-output-hub <RUN>
bash runner/run.sh step contract <RUN>
bash runner/run.sh step lib-full <RUN>
bash runner/run.sh cleanup <RUN>        # identity-checked kill + receipts, then fetch
bash runner/run.sh fetch <RUN>          # any time; logs required, others optional
bash runner/run.sh purge <RUN>          # optional; only after cleanup receipt was fetched with EXIT=0
```

Every remote step writes `logs/<step>-command.log`, `logs/<step>.log`, `logs/<step>-exit.log` and prints
`WSU <RUN> <step> EXIT=<n>`; `run.sh` tees into `<RUN>/local/<op>-<utc>.log` and ends with
`WSU <RUN> local:<op> EXIT=<n>`. Monitor that sentinel; do not poll.

`manifest.tsv` columns: `status(P|D) mode blob sha256 bytes path`.

## Exit codes

`0` ok, `1` step error (see log), `2` cleanup SKIP_MISMATCH/SKIP_UNVERIFIED/KILL_FAILED or unknown step,
`3` MISSING_UPSTREAM, `4` expected artifact missing (ui/dist, ferryx.exe), `5` no tests executed,
`9` step already ran (new rNN required), `10/11` push layout refused, `21` fetch required item missing,
`64-67` local usage/stage/gate errors (`66` purge refused), `124` step timeout (tree killed by identity).
