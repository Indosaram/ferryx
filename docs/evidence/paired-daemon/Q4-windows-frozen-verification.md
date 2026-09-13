# Q4 Windows frozen verification

## Disposition: NOT ACCEPTED

Native Windows compilation of the frozen Wave1 backend failed with two E0433
errors. This is independent platform evidence expansion, not implementation
approval, behavioral RED, A06/A07/A08 packet acceptance, or full-plan acceptance.
No production source was changed. No later Q1/Q3 or active A10 source was copied.

Task: st_01a0984d; parent 01a097f8-4568-7573-897e-d61f0fe6d692.

## Snapshot identity

Source: `/Users/indo/code/project/orca-lite-wt/herdr-wave1`, HEAD
`e1a00339a5339ed4d9c9634e26b86f41111d49f6` **plus actual inherited dirty and
untracked implementation**. HEAD alone was not built. All 34 expected hashes
from A08-typed-owner-verifier-source-after.json matched before/after capture,
in the private snapshot, and in a further frozen-root check after transfer.
The native Windows transfer and post-build checks matched all 969 source/asset
manifest entries, which include those 34 and the pinned Cargo.lock.

- Source archive SHA-256: `c35fdc9be7cc5b7ef20bb25d341b26b7a28818a4571be9162aef1f98beab399f`.
- Ghostty pinned commit: `6a508fd5e34c7e222c052a6d00bb3891ff3feace`.
- Ghostty bundle SHA-256: `6032eb88ddc9bc4e18c439ccd79729bab92535fa19d22408aa66dd6e829b5ae3`.
- `Q4-windows-manifest.json` records every regular source/fixture/private UI
  dist input. The dependency was bundled from the pinned local Git commit and
  cloned into the private Windows root, not linked into shared writable trees.
  Targets, node_modules, .env files and canonical configuration/secrets were
  excluded. No dependency symlink was followed.

The initial snapshot attempt exposed ignored Cargo.lock omission and failed
before archive/transfer. The corrected collector explicitly includes all 34
frozen inputs. Its failed log and corrected script are retained.

## Platform, isolation, and fixture audit

Host `maho-win`, native Microsoft Windows NT 10.0.26200.0, Rust/Cargo 1.97.0,
`x86_64-pc-windows-msvc`, Git 2.55.0.windows.2; C: free at preflight
194577104896 bytes. SSH Session 0 was used only for headless compilation.
The exact new remote root was checked absent before creation:
`C:\Users\sook\ferryx-herdr-q4-windows-01a097f8`.
Local staging was likewise checked absent:
`/tmp/ferryx-herdr-q4-windows-01a097f8`.

Before invoking Cargo, the script isolated HOME, USERPROFILE, HOMEDRIVE/PATH,
APPDATA, LOCALAPPDATA, FERRYX_RUNTIME_DIR/DATA_DIR/SESSION_DIR, XDG config/data/
cache/state/runtime and TMP/TEMP/TMPDIR. Native GetTempPath returned the QA temp
root outside a Git checkout. Cargo/Rustup homes were explicitly retained for
the toolchain. Target was private, jobs=3, dev/test debug=0, incremental=0.
Global/system Git configuration was disabled or redirected to private HOME.

Static fixture audit found tempfile-based roots, Unix-only transport tests,
and relay fixtures using tempdir_in("."). Remote security fixtures also create
temporary children under private UI dist. **No test fixture was executed**:
the compiler-first gate failed before test isolation could be fully exercised.
Future execution must account for Cargo test's package working directory and
the source-relative asset fixtures; environment variables alone do not move
explicit tempdir_in(".") calls. No fixture privacy pass is claimed.

Local LSP diagnostics on the private Windows runner file returned no
diagnostics before the actual compilation attempt. This Darwin LSP observation
does not validate Windows cfg branches. Native Windows LSP was not connected;
the retained where-rust-analyzer output is availability only, not diagnostics.

## Command and result ledger

All command bodies are retained in Q4-windows-verify.ps1 and logs. A bounded
background Process monitor waited on the owned build process (30 minute limit);
no fixed sleep or timing-luck test retry was used.

1. Initial requested build selected host-configured sccache despite a PowerShell
   empty environment assignment. sccache panicked `Unable to get config
   directory`, native diagnostic exit 101. The first monitor's ExitCode was
   blank, explicitly retained as a receipt limitation, not a successful exit.
2. Wrapper selection was corrected using a private Cargo config with
   `rustc-wrapper = ""` and an explicit empty child environment value. The
   next launch failed with exit 101 because ProcessStartInfo needed an explicit
   WorkingDirectory. Both setup failures are retained; neither compiled Ferryx.
3. With explicit private WorkingDirectory and wrapper override, ran:

   ```text
   cargo --config C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\cargo-qa.toml build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay
   ```

   Result: **exit 101, WAITED=True**. Native dependencies and Ferryx compilation
   executed. `Q4-windows-remote-build.log` preserves full output, including all
   four warnings. The added --config is only a toolchain isolation override.

Exact compiler blocker:

```text
error[E0433]: cannot find `native_terminal` in `ipc`
src\clipboard_image.rs:127:34: crate::ipc::native_terminal::CF_DIB_ID
src\clipboard_image.rs:128:38: crate::ipc::native_terminal::CF_DIBV5_ID
src\ipc\mod.rs:10-11: module gated behind feature = "native-terminal"
error: could not compile `ferryx` (lib) due to 2 previous errors; 4 warnings emitted
```

The Windows clipboard function is OS-gated, not native-terminal-feature-gated;
the referenced constants (8 and 17) live in the feature-gated IPC module. The
coordinated repair batch needs to remove that headless dependency without
enabling the native renderer merely to make this lane compile. No repair was
attempted here. A clean repaired build may reveal additional defects.

## Coverage boundaries

| Required surface | Actual result |
| --- | --- |
| Native Windows compiler | Executed, blocked as above; not accepted |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture` | Not run, shared library compiler blocker |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence --test machine_worktrees --test worktree_safety --test machine_worktree_legacy_bounds --test machine_worktree_transports --test relay_pairing_generation_regression -- --nocapture` | Not run, shared library compiler blocker |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib worktree:: -- --nocapture` | Not run, shared library compiler blocker |
| HTTP / relay / WS / PTY / runtime Git cases | None executed; no pass or zero-match coverage claim |
| Native drive root / UNC refusal / hidden attribute / ACL directory behavior | Unavailable due to compiler blocker; no UNC share invented or created |
| Suspended-create / Job assignment / resume / descendant drain | Source inspected, not runtime-proven |
| machine_worktree_transports | Entire fixture is `#![cfg(unix)]`; would supply no Windows coverage |
| Unix symlink, mode bits, process-group and Linux byte-name branches | cfg-excluded on Windows, not passing Windows evidence |
| Native desktop / installed app / actual Tauri invocation | Not launched; separate manual acceptance |

## Cleanup and retention

`Q4-windows-remote-cleanup.log`: cleanup command exit 0, post-build 969 hashes
unchanged, zero processes with executable paths under the private QA root,
build child exit 101 and waited. All nine private runtime/home/data/session/
config/cache/temp/appdata/localappdata directories were removed with explicit
absence receipts. No daemon, PTY, HTTP/WS listener or runtime Git fixture was
launched. Only the private build/dependency subprocesses ran; no canonical
daemon discovery/calls, existing app, remote checkout or credentials were used.

The local shell monitor PIDs 96207, 1097 and 4733 were absent on final inspection;
the final SSH invocation exited 101 as recorded. Staging, private Windows source
and target, archives/bundle, scripts and logs are intentionally retained for
the composed-platform repair run. They are not runtime fixtures. No source
delta exists; authored scripts/config are verification tooling only. Evidence
is uncommitted. No release build, deployment, privileged change or commit occurred.
