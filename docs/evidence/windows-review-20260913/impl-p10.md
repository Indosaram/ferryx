# P10 implementation checkpoint - st_01a09a03

Status: regression staging implemented; product repairs and execution blocked on the explicitly required lead Cargo/native slots. NOT packet complete, NOT RED/GREEN, NOT Windows acceptance.

## Ownership and registration

Read root/src-tauri/worktree AGENTS.md, applicable Orca CLI skill (no runtime CLI invoked), repair-packets.md, gap-packet-addendum.md, dag-remaining-register.md, audit-filesystem-ssh.md and remaining-contracts.md. Initial and scoped pre-edit git diffs showed P10 files clean; foreign ipc/ssh.rs changes remain untouched. No branch, worktree, commit, push, install, daemon or native mutation occurred.

Official executeAgentToolkit imported from `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`, bound to this repo and parent session `01a0983f-c995-753d-afa9-593f6d118788`, accepted revise_criterion for G001/C002. The full prior scenario was retained with P10 exact commands/binary conditions appended. Receipt: `p10/registration.json` (`ok: true`, `accepted: true`).

## Staged executable assertions

- `ssh/config.rs`: `p10_identity_file_import_preserves_semantic_argv` follows real parse -> import -> ssh_plan, with owned native absolute temp-root key data and exact one `-i` value. `p10_identity_file_config_quoting_grammar` covers double quotes, Windows backslashes, equals syntax, escaped quote, inline comment and quoted literal hash. Product parser remains unfixed for intended RED.
- `ssh/direct_tests.rs`: Windows-only `p10_windows_bridge_preserves_native_argv` compiles an owned C# argv-recorder executable via Windows PowerShell Add-Type, then invokes actual production bridge_command through bounded_output. Base64 output decodes to exactly four argv values, including spaced Unicode root and trailing backslash. No SSH connection or helper daemon. Product Arguments concatenation remains unfixed for native RED.
- `tests/worktree_safety.rs`: Windows-only `p10_windows_namespace_rejects_invalid_path_components` checks typed InvalidNamespace for both workspace and slug: pipe/angle/quote, DOS names/extensions/nesting, console device aliases and superscript port digit. Portable positive control retains nested Unicode and non-device lookalikes. These new tests call only format_branch_name: no Git commit/worktree creation, PTY or daemon.

## Exact lead relay action required

Grant an exclusive Darwin Cargo slot and supply the absolute Cargo executable, existing provisioned Cargo home, Rustup home and lead-selected shared target. Invoke:

`node docs/evidence/windows-review-20260913/p10/run.mjs red ABS_CARGO ABS_CARGO_HOME ABS_RUSTUP_HOME ABS_LEAD_TARGET`

The uppercase arguments above describe required lead values, not executable placeholders. The runner refuses absent/nonabsolute values. It launches only the registered config regression prefix and exact pure namespace positive control, serially, with child-only owned profile/runtime/temp settings, jobs=2, logs/exit receipts/source hash, and owned-root cleanup. It never copies caches, starts daemons or runs the unsafe full bridge/worktree targets. A build failure or zero tests is not behavioral RED. Inspect process ownership before releasing a slot if Cargo hits its failure deadline. No Cargo command has been run by this child.

After actual config assertion RED, return slot/results to this owner for minimal parser fix and identical GREEN. Production changes are deliberately not applied before the required intended RED.

Native runtime owner st_01a099f8 must run these exact staged native cases against unfixed source before Windows repair:

- `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::direct::tests::p10_windows_bridge_preserves_native_argv -- --exact --nocapture`
- `cargo test --manifest-path src-tauri/Cargo.toml --test worktree_safety p10_windows_namespace_rejects_invalid_path_components -- --exact --nocapture`

Both require nonzero assertion discovery. PowerShell/Add-Type prerequisite failure is not argv RED. Afterwards retain identical assertions for GREEN. No native command was run here.

## Remaining implementation inside this packet

1. After RED, parse OpenSSH IdentityFile semantics and quote full Windows bridge argv with Windows PowerShell 5.1-compatible CRT escaping (including doubled trailing backslashes); ArgumentList alone is not available on that runtime.
2. Windows namespace validation must apply by *target platform*, not just desktop OS. Local manager and remote Windows create share ownership; preserve legal POSIX names when the actual target allows them. Test before Git/filesystem mutation, not a generic Git error after mkdir.
3. Repair existing direct test `/keys` fixture to local native absolute data, bridge helper path to explicitly verified native artifact, and Windows worktree execution fixture without accidental WSL bash.
4. Replace bridge lifecycle startup-output/timer cancellation with a deliberate responder gate and exact request-received signal; do not execute existing broad `ssh::bridge::` filter. It includes unconditionally invoked live loopback SSH plus helper/PTY cases. Real framed handshake needs separately provenance-verified helper and owned daemon permission/runtime orchestration.
5. CONTRACT-RC-01 missing local helper test still assumes unowned `/nonexistent/...` and needs owned absent child plus zero-transport recorder; CONTRACT-RC-05 still pins message prose and needs full structured fields. Neither unsafe test was executed.
6. Retain original native Git Unicode/no-orphan and real SSH helper handshake acceptance, diagnostics/build/related packet tests once seams are safely staged. Current no-worktree/no-commit instruction prevents invoking existing Git-creating worktree_safety cases here.

These are exact unfinished increments, not claims of repaired behavior or a new whole-domain audit proposal.

## Actual verification and cleanup receipts

- LSP diagnostics `ssh/config.rs`: no diagnostics.
- LSP diagnostics `ssh/direct_tests.rs`: only inactive Windows cfg hint; native new case not typechecked on Darwin.
- LSP diagnostics `tests/worktree_safety.rs`: only inactive Windows cfg hint; native negative case not typechecked on Darwin.
- `node --check docs/evidence/windows-review-20260913/p10/run.mjs`: exit 0.
- `git diff --check -- src-tauri/src/ssh/config.rs src-tauri/src/ssh/direct_tests.rs src-tauri/tests/worktree_safety.rs`: exit 0.
- Scoped diff: 3 files, 90 inserted test lines; no production behavior changed.
- RED: not run. GREEN: not run. Build: not run. Native: not run.
- No child process/test fixture was launched, so no runtime-owned resource needed cleanup. Evidence runner remains intentionally staged, uncommitted alongside tests. Foreign changes were not reverted or overwritten.
