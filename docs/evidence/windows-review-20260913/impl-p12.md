# P12 Browser CLI authorization - execution handoff

Task st_01a09a04; root 01a0983f-c995-753d-afa9-593f6d118788; 2026-09-13.

Status: regression staging delivered; authorization implementation and RED/GREEN are blocked before repair by the required lead-issued exclusive Cargo execution slot. P12 is NOT fixed or complete. Production behavior is unchanged. This is not an audit-only proposal: two socket regressions and a guarded runner are staged in the shared tree.

## Scope and registration

Read root/backend/IPC directory instructions, programming/Rust/debugging skill instructions, repair-packets.md P12, gap-packet-addendum.md, dag-remaining-register.md, audit-browser-ipc.md and P08's implementation handoff. Rechecked git status and scoped diff before writing: browser_cli.rs and cli.rs were clean. Foreign dirty files were not changed.

Called official executeAgentToolkit from `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js` with resolveCwd `/Users/indo/code/project/orca-lite` and resolveSessionId bound to root. `steer` / `revise_criterion` appended the exact test commands, binary oracle, resource ownership, and native requirements to C002 while preserving its entire current scenario and expectedEvidence. Receipt: `{"ok":true,"operation":"steer","accepted":true}`. Root ledger is the durable registration receipt.

## Delivered code and safe runner

`src-tauri/src/ipc/browser_cli.rs`: 91 added lines within its existing test module. `p12_raw_tcp_request` binds `127.0.0.1:0`, registers an in-memory browser, connects an independent raw TCP peer, and invokes the actual production `handle_connection`. It does not use a replacement authentication parser. The Tauri mock supplies only AppHandle; request parsing, socket framing and BrowserManager are real. No global runtime directory, environment mutation, daemon, settings, audio or desktop is touched.

- `p12_tcp_rejects_unauthenticated_commands`: bare list/snapshot/act must return parsed `BROWSER_CLI_UNAUTHORIZED`, not browser data or a downstream domain error.
- `p12_tcp_rejects_forged_credential`: forged token list must return that same code, not the private fixture URL.

The listener and client are scoped joined futures under a bounded 5-second timeout, not spawned detached tasks. Completion, timeout and panic drop all socket resources. No fixed sleeps or port-probing race.

`docs/evidence/windows-review-20260913/p12-socket-tests.sh`: exact two-test runner; refuses execution without `--lead-issued-exclusive-slot`. That flag is an acknowledgement of a grant, not permission to self-grant. Both exact commands run even if the first fails. Each must discover exactly one test. Syntax checked and refusal exercised, exit 2, with no Cargo execution.

## Exact next action required from lead

Grant the exclusive shared Darwin Cargo slot and resume this worker; only then run:

```sh
sh docs/evidence/windows-review-20260913/p12-socket-tests.sh --lead-issued-exclusive-slot
```

Commands embedded in runner:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli::tests::p12_tcp_rejects_unauthenticated_commands -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli::tests::p12_tcp_rejects_forged_credential -- --exact --nocapture
```

Require one discovered test per invocation. Intended original RED is assertion output containing `List` instead of unauthorized, from the real TCP handler. Compile errors/zero tests are not RED. No behavioral fix should precede that observation. Retain exact log and binary/compiler-artifact hash, then implement auth and run the same assertions GREEN.

Also relay the common auth agreement to P08/P23: P08's current impl-p08.md explicitly says no wire change/API is published. P23 owns remote/auth.rs. Current write_private_json restricts Unix modes only; it does NOT establish a Windows SID/DACL guarantee and cannot be claimed adequate capability publication. Proposed contract for agreement: per-listener random capability, bounded per-request token envelope checked before BrowserCliRequest dispatch, capability publication restricted/verified for the current Windows SID, client reads only the owned credential file, startup fails if secure publication fails. Browser capability must not authorize daemon commands or vice versa. Keep cli.rs public sender signature unchanged if the sender can load credentials internally. No shared auth writer was edited by P12.

Remaining P12 work after prerequisites: implement independent actual-listener rejection with agreed credential API; add authenticated list/snapshot/act assertions and stale/absent/wrong capability cases, preserve request-size limits, secure Windows publication before port advertisement, test credential rotation and second-account isolation. Unix sockets already enforce 0700 parent/0600 socket; preserve working Unix behavior. Authentication must not rely on port secrecy or remote gateway enablement.

## Exact native handoff to st_01a099f8

Only that runtime owner may mutate maho-win. Build source-provenance debug tests and run the two exact commands above, then the packet filter `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli:: -- --nocapture` after reviewing all included fixture ownership. Require nonzero tests. Use owned ephemeral endpoint and disposable profile, not installed executable or user daemon. Inspect effective capability DACL/current SID, deny second-account read, and independently connect without/with forged capability: list/snapshot/act must be unauthorized with no fixture title or click effect. Through actual debug Ferryx CLI with valid capability, list finds fixture, snapshot returns exact title, click changes its marker; subscribe to actual completion before action with bounded timeout. Rotate listener capability and prove old token denied. Record binary hash, source hash, endpoint/PID ownership and owned resource cleanup. P13 owns real action semantics. These native cases remain unrun and are not replaced by the AppHandle mock.

## Verification and cleanup log

- Official registration: ok true, steer accepted true.
- LSP diagnostics for browser_cli.rs: only inactive-code hints for cfg(not(unix)) at lines 44, 186, 194, 423, 430, 895, 941, 990. No errors/warnings reported for active Darwin code; Windows code is NOT type-verified by that result.
- `git diff --check -- src-tauri/src/ipc/browser_cli.rs`: exit 0, no output.
- Scoped diff: one file, 91 additions; cli.rs unchanged.
- `sh -n docs/evidence/windows-review-20260913/p12-socket-tests.sh`: exit 0.
- Runner without grant: `P12 staged only. Requires lead-issued exclusive Cargo slot; no tests executed.` and `runner_without_slot_exit=2`.
- Staged browser_cli.rs SHA256: `9280f03f98db12d7d48cf343db719e6c6fad09cc34ea63a28214047d72c1489b`.
- Runner SHA256: `652e1054e688a2bb4a21efdd4d7e24a9ce8c6a3d4a2ef87517b6c0d366612042`.
- Cargo RED/GREEN/build: NOT RUN, slot not granted. No passing or failing behavioral result asserted.
- Cleanup: no runtime socket/process/profile created; runner refusal starts none. Only scoped tests and evidence/runner files remain, uncommitted. No branch/worktree creation/deletion/switch, commit/push, install/release, remote mutation, daemon manipulation or dependency-cache action.
