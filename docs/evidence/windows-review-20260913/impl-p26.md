# P26 Permission capability contract repair

Task st_01a09a0c; HIST-03 / GT-06. Scoped implementation delivered; Cargo and native acceptance remain pending, not whole-objective completion.

## Scope and mechanism

Read src-tauri/AGENTS.md, programming skill and Rust reference, packet addendum, remaining register, original goals/ledger and HIST-03/GT-06 source evidence. Initial scoped git diff was empty; foreign dirty work was not edited. Source ownership remained permissions/mod.rs and permissions_contract.rs. Read-only caller trace: registered lib.rs command -> ipc/permissions.rs run_blocking wrapper -> permission provider/status/request. Existing IPC bool/result DTO signatures remain unchanged; P21 UI is untouched.

- Non-macOS request_accessibility now returns false instead of fabricated success.
- Accessibility can_request is true only on macOS, consistent with unsupported checks off macOS. Aggregate/notification policy is unchanged.
- Production settings entry delegates through an injectable launcher, retaining existing command/argv/routing and failure semantics. Contract tests inject a recording closure, never launch Settings, and assert exact target, opened, reason presence and command arguments rather than human reason text.
- Removed two existing notification-description prose assertions while preserving their machine-consumed capability assertions.
- Unsupported/malformed settings targets must make zero launcher calls. Supported launch success and failure are both exercised. macOS FDA alias and accessibility/notifications routes remain in the compiled contract matrix.

## Registration and RED/GREEN

Official executeAgentToolkit bound to repo cwd and root session 01a0983f-c995-753d-afa9-593f6d118788 accepted revise_criterion C002 before edits. Full pre-existing scenario was preserved and appended; receipt: [p26/registration.json](p26/registration.json).

Exact independent invocation:

```sh
python3 docs/evidence/windows-review-20260913/p26/run-contract.py
```

Runner compiles actual permissions module and actual notification DTO/model using std-only rustc --test, removing serde annotations and renaming target_os cfg to an injected p26_os so Darwin can execute Windows/Linux policy. Provider supplies non-authoritative status; default OS launcher panics if called. Real check/request/capability/settings mapping and integration assertions are not substituted. This is policy execution, not native ABI, serde, Tauri registration or Windows runtime proof.

1. Behavior-preserving launch seam and safe tests staged before product fix.
2. [RED](p26/red.log): Windows 8 pass / 2 fail; Linux 8 pass / 2 fail; each binary exit101, runner exit1. Intended assertions fail on !item.can_request and !request_accessibility(), not compilation or zero discovery.
3. Two-line capability/request repair; identical assertions [GREEN](p26/green.log): both platforms 10 pass, binary/runner exit0.
4. Formatting and removal of the two pre-existing prose-only assertions; [final GREEN](p26/final-green.log): both platforms 10 pass, zero ignored/filtered, exit0. All behavioral RED assertions remain unchanged.

Final hashes from runner:
- permissions/mod.rs: fbf52d3ece645c1f3d901bbb193d46d95a84e610cca47ada0cb7e0ecc7b254de
- permissions_contract.rs: d75a0f47692b978e36a5385529a7f6f4229173729cb0b2430ed73a99a13cdcc1

[Scoped diff](p26/scoped.diff). [Diagnostics](p26/diagnostics.log): final LSP only inactive-code hints; one cancelled source request was repeated successfully. Diff whitespace check exit0. Existing off-macOS home_dir dead-code compiler warning retained, not suppressed.

## Cleanup and narrowly outstanding execution

Every independent run used TemporaryDirectory and reports cleanup_owned_root_absent=True after both binaries exit. No Cargo invocation, global environment mutation, dependency/cache install/copy, daemon, socket, OS permission request, settings operation, branch/worktree mutation, commit or release occurred.

Lead relay required because this child has no send/monitor tool: grant shared Darwin Cargo exclusive slot and execute the staged safe runner:

```sh
bash docs/evidence/windows-review-20260913/p26/run-cargo.sh
```

It contains the registered exact commands:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test permissions_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib permissions:: -- --nocapture
```

macOS integration queries status but never calls accessibility request or default settings launcher. Expected discovery: 3 contract tests on macOS (4 on Windows/Linux), 6 permission unit tests. Full crate build and real registered IPC have not been verified by this child; those are foreign execution prerequisites, not passing claims.

Native handoff only to runtime owner st_01a099f8: run both exact commands on current-source Windows binary, requiring 4+6 passing tests and no Settings launch; use registered IPC to verify accessibility status unsupported, granted=false, canRequest=false, canOpenSettings=false and request result=false. P21 hidden-card behavior remains separate UI evidence. Actual notifications Settings URI opening requires separately authorized runtime action and is explicitly outside this no-OS-settings task; do not invoke it as a contract-test prerequisite. Original aggregate/native acceptance remains open.
