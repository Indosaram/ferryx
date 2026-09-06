# Final backend verification receipt

Task: `st_01a077ff`. Executed 2026-09-06 UTC on Darwin arm64, from
`/Users/indo/code/project/orca-lite`.

**Backend code verdict: APPROVE. Native desktop gate: UNVERIFIED / NOT APPROVED.**
No source changes, new harness, VM, desktop automation, commits, all-targets run,
or `daemon_persistence_contract` execution were performed by this reviewer.

## Fresh checks

Before Cargo execution, `lsp_diagnostics(src-tauri/src, severity=all)` returned
zero diagnostics/errors for **50 scanned Rust files, capped at 50**. This is not
a claim that LSP checked every Rust file. Cargo is the compiler result below.

Each required command ran **once**, sequentially, with stdout/stderr captured by
Python `subprocess.run`; exit status and the final 150 output lines were emitted
to this task's tool transcript. No log files were added. Relevant tails:

| Command | UTC start | Result | Exit |
| --- | --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh -- --nocapture` | 18:34:59.894 | 33 passed, 0 failed, 0 ignored; 599 filtered; 5.04s | 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib project_remote` | 18:35:05.506 | 4 passed, 0 failed, 0 ignored; 628 filtered; 0.04s | 0 |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 18:35:05.960 | Finished dev profile in 0.98s | 0 |

```text
FERRYX_SSH_QA_REGISTERED {"workspaceId":"ssh:04f96df075438d50125e1936186fe342ef11558d282328a5eba28a9651c90b2d","repoRoot":"/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHXaluY/remote project's space","gitRoot":"/private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHXaluY/remote project's space","hostId":"qa-loopback","hostLabel":"QA seam regression"}
FERRYX_SSH_QA_PTY_OK qa-new-tab /private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHXaluY/remote project's space
FERRYX_SSH_QA_PTY_OK qa-split-restore /private/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmpHXaluY/remote project's space

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 631 filtered out; finished in 2.01s
[above: child execution nested inside the 33-test command, not an extra top-level run]
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 599 filtered out; finished in 5.04s

running 4 tests
test ipc::project_remote::tests::remote_startup_cannot_use_local_shell_resolver ... ok
test ipc::project_remote::tests::remote_registration_wire_request_rejects_extra_execution_fields ... ok
test ipc::project_remote::tests::all_local_git_boundaries_reject_remote_namespace_even_without_loaded_metadata ... ok
test ipc::project_remote::tests::local_registration_never_canonicalizes_a_remote_id_into_a_local_project ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 628 filtered out; finished in 0.04s

Checking ferryx v2026.902.2 (/Users/indo/code/project/orca-lite/src-tauri)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.98s
```

Warnings remain: 50 vendored `wgpu-hal` cfg warnings and 7 Ferryx warnings per
command, covering notification/native-terminal imports/fields/mutability and
existing terminal/worktree lifecycle helpers. No warning was suppressed or fixed.

## Boundaries actually checked

The passing SSH target includes real generated-key loopback `/usr/sbin/sshd`, the
production registration core, canonical symlink/quoted-directory probing,
re-registration by returned ID, a fresh daemon without local workspace entries,
and two real SSH PTYs. It also exercises the existing configurable QA seam
against its own loopback fixture; no external configuration or VM was supplied.
Output subscription precedes terminal input; the octal-escaped marker cannot be
satisfied by input echo. Sessions are closed and the fixture listener is aborted
and awaited by the existing test. This is not native GUI or an OS-process restart
observation.

Passing tests and inspected callers cover host-qualified/fixed-width identity,
local collision rejection, disk reload, corrupt-store failure, atomic-save temp
cleanup, disabled/deleted host rejection, attach validation, no local startup
fallback, safe argv/quoting, deadline termination/reaping, local Git rejection,
and explicit workspace-context Reveal. A legacy local path-only Reveal remains
local even when the same path is saved remotely. Full review: `code-review.md`.

## Earlier failure retained, not retried away

`backend-public-core.log:1499-1518` records a prior optional QA-phase failure:

```text
src/daemon/remote_ssh_qa.rs:95:6:
actual backend registration/probe against QA host: IpcError { code: InvalidPath, message: "SSH directory probe timed out after its bounded deadline", details: None }
src/daemon/remote_ssh_qa.rs:67:9:
SSH QA child failed: exit status: 101
child: 0 passed; 1 failed; finished in 8.01s
parent: 0 passed; 1 failed; finished in 14.14s
REAL SSH EXIT: 101
```

The test invokes the production 8-second probe deadline, not a separate mocked
registration. The historical capture does not establish the timeout's cause.
Today's mandatory single execution passed, including that loopback QA phase;
it does not erase the earlier failure or establish repeated-run reliability.
Minimal recommendation if that failure is pursued: capture which connection/probe
stage stalls in the existing fixture before changing the deadline; do not add
retries, skip the assertion, or weaken SSH trust. No implementation or retry was
performed here. The abandoned external-VM attempt separately failed on its absent
pinned trust file (`backend.md`); it is not passing product evidence.

Native Accessibility, screenshots, OS picker behavior and WGPU visibility remain
unverified. Neither these backend results nor source inspection approves that gate.
