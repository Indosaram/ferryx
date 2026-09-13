# P13 implementation receipt - st_01a09a05

Status: partial implementation delivered, native/history prerequisites open. No whole-domain completion claim.

## Ownership and registration

Read root, backend, IPC, UI and UI-lib AGENTS; programming/Rust/TypeScript/unsafe and debugging instructions; repair-packets P13, gap addendum, remaining register, original goals and source audit. Intake `git status` showed extensive foreign work but all four assigned files clean. Foreign browserTauri.ts and Cargo.toml remained read-only.

Official executeAgentToolkit at `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`, with resolveCwd bound to this repository and resolveSessionId bound to `01a0983f-c995-753d-afa9-593f6d118788`, accepted `revise_criterion` for C002 before repair (`ok:true, accepted:true`). The complete existing scenario was preserved and P13 literal seam commands/native binary conditions appended. Registration explicitly permits typed unsupported Windows keypress rather than false synthetic success. The keypress assertion's exact final test name is `ipc::browser::tests::windows_keypress_returns_typed_unsupported` (the registration describes its behavior but did not enumerate that final name).

## Delivered source

- `src-tauri/src/ipc/browser.rs`: Windows external URLs and existing file paths use ShellExecuteW, not cmd/start. URL validation remains before dispatch. UTF-16 target stays one native file/URI parameter with no lpParameters; embedded NUL rejected. ShellExecute failure codes <=32 propagate; Unix opener failures now propagate too. Both synchronous launch paths run through run_blocking. File paths retain OsStr rather than lossy UTF-8 conversion.
- Same module: file-link resolution uses std::env::home_dir(), which resolves Windows USERPROFILE/profile API instead of requiring HOME. Bare `~` returns the home directory, not `home/~`; `~/suffix` strips exactly one prefix.
- Same module: Windows automation keypress checks current generation then returns structured IPC `UNSUPPORTED` before webview evaluation. This intentionally does not implement trusted native key injection. Click/fill and macOS dispatch are unchanged.
- Same module: the old relative-path test no longer launches the user's default application; it checks the actual resolver and existence of the Cargo.toml fixture.
- `browser/manager.rs`: native history dispatch invalidates automation generation/targets even for same-URL entries, retaining the engine-owned URL pending completion.
- `browser/tests.rs`: regression for same-URL native-history snapshot invalidation.
- `ui/src/lib/browserTauri.test.ts`: exact URL/query, focused back/forward command and structured unsupported rejection contracts; lifecycle test uses an invocation signal instead of polling; retry tests use fake timers.

## Actual evidence

| Check | Result / evidence |
| --- | --- |
| Bare home production resolver RED | `p13-home-red.log`: 2 tests, 1 pass/1 fail; actual `C:/Users/P13 fixture/~` vs expected profile root, exit101 |
| Windows opener request seam RED + home GREEN | `p13-opener-red-home-green.log`: 3 tests, 2 pass/1 fail; actual cmd argv vs ShellExecute request, exit101 |
| Key capability RED | `p13-key-red.log`: actual capability returned Ok instead of typed error; 1 fail, exit101 |
| Current manager source RED | `p13-history-red.log`: generation did not advance, 1 fail, exit101 |
| Same assertions GREEN | `python3 docs/evidence/windows-review-20260913/p13-safe-runner.py`: 3 opener/home + 1 status mapping + 1 key + 1 manager test pass, exit0; individual `p13-*-green.log` and aggregate `p13-safe-runner-green.log` |
| UI tests | `bun run --cwd ui test src/lib/browserTauri.test.ts`: 14 pass in one successful invocation, `p13-ui-green.log` |
| Initial UI fixture error | `p13-ui-initial-failure.log`: 13 pass/1 fail. Newly added expression-bodied beforeEach returned invoke mock, which Vitest used as cleanup; changed hook to return void, then reran. Not a production RED. |
| TypeScript | `cd ui; bunx tsc --noEmit --pretty false`: exit0, empty `p13-ui-types.log` |
| Rust LSP | Final individual checks: manager no diagnostics; tests/browser IPC only inactive Windows/Linux cfg hints, no errors. Earlier parallel requests cancelled/timed out. |
| UI LSP | First check no diagnostics; later fresh requests timed out. Full tsc above passed. |
| Full IPC metadata experiment | `p13-ipc-metadata.log`: exit1 on missing CARGO_PKG_NAME/private run_blocking/mixed cached serde and objc dependencies. Harness failure, not production RED and not a crate build. No blind retries. |
| Diff / runner syntax | Scoped `git diff --check` and `bash -n p13-cargo-runner.sh` exit0. Cargo runner without slot rejects before executing Cargo. |

The source-seam runner uses temporary rustc test binaries, extracting current actual resolver/request/key code and including current manager.rs by path. It links pre-existing crate error/model/security types and dependencies read-only. It does not validate the complete crate or Windows cfg. Manager inclusion emits dead-code warnings for methods outside the narrow harness; warnings are retained, not suppressed. Opener result-code coverage was added after the repair, so it is GREEN-only, not a separate earned RED.

## Narrow outstanding actions for lead

1. **Foreign prerequisite / B03 implementation:** Cargo.toml's owner must add direct Windows dependency `webview2-com = "0.38.2"` (already locked transitively; Tauri does not re-export callback handler types), or explicitly allocate a serialized edit. P13 did not write hand-coded COM vtables to bypass the file boundary. Windows HistoryChanged/SourceChanged callbacks and native GoBack/GoForward remain unimplemented; current Windows JS history fallback remains. The manager snapshot fix is not a substitute for these callbacks.
2. **Exclusive Darwin Cargo:** grant slot and execute `P13_EXCLUSIVE_CARGO_SLOT=granted-by-lead bash docs/evidence/windows-review-20260913/p13-cargo-runner.sh`. Seven exact tests, each requires one discovered/passed test. No broad filters, no cache copying/installing, no target multiplication. No slot was received in this child session; Cargo/build not run.
3. **Native runtime owner st_01a099f8 only:** after callback integration, fresh debug `bun tauri dev`, record source/executable identity. Owned child fixture: initial page, two pushState entries without title/load events; subscribe browser_state_changed before each operation and await exact URL/flags with bounded timeout; toolbar Back/Forward must traverse real engine entries. Failed and no-op history must settle loading. Never replace this with jsdom or synthetic listener counts.
4. **Native OS seam:** default opener receives complete `https://example.test/?a=1&b=2` and owned filename containing spaces/ampersand, benign second-command marker never executes. No user default-app association changes. HOME-absent USERPROFILE owned fixture must open `~/file` and bare `~`. ShellExecuteW is Windows-gated FFI and was not executed under Miri/sanitizers or Windows in this child.
5. **P12 authenticated CLI:** current-generation Backspace and Tab now must return serialized `UNSUPPORTED`, never Ok without editing; stale generation must still reject stale first. If native key injection is later implemented, replace capability fallback only with real input abc->ab and actual Tab focus evidence. Existing unsupported outcome is the packet's authorized fallback.

## Cleanup and boundaries

All rustc temporary sources/binaries used TemporaryDirectory and were removed after execution. Retained artifacts are p13-prefixed evidence and safe runners under this directory. No Cargo target writes, build/install/release, Windows remote mutation, daemons, dialogs/settings/audio, user default applications, branches/worktrees, commits or pushes were performed. Work remains uncommitted in the shared tree. P13 cannot be approved until the listed callback implementation, Cargo/native checks and remaining packet gates are resolved.
