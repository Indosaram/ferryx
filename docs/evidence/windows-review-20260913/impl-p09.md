# P09 implementation receipts - st_01a09a02

Status: partial implementation verified locally; **not packet completion**. DS-04,
USERPROFILE installation portion of DS-06, and HIST-01 have intended RED/GREEN.
DS-03 native shim launch and authoritative provider discovery require the narrowly
identified execution/foreign-owner prerequisites below. No native acceptance claimed.

## Ownership and registration

Initially all three assigned Rust files were clean; unrelated dirty files were left
untouched. Added only `ipc/agents.rs`, `daemon/agent_extension.rs`,
`ui/src/components/settings/TerminalSection.tsx`, its new test, the owned
`script/qa/p09-agent-contract.mjs` runner, and these receipts. `terminal/shell.rs`
is unchanged; shared DTOs/types and P08's server/protocol/PTY files are unchanged.
Read root, src-tauri, ipc, terminal, daemon, UI and component AGENTS instructions,
packet/addendum/register/source evidence, and installed orca-cli skill discovery
stub (no Orca-managed operations were run).

Official executeAgentToolkit bound to repo cwd and root session registered an
additive C002 revise_criterion; `p09-registration.json` records result.accepted=true.
A prior request lacking evidence was rejected; another overly elaborate additive
wording was rejected by invariant checking; no repairs preceded accepted registration.
The accepted revision preserves the full existing scenario.

## Implemented

- Detection uses injected platform/PATHEXT resolution shared with the real
  `resolve_binary` entry point. Windows honors executable extension order and
  case-insensitive names, accepts explicit supported extensions, rejects data
  files even when PATHEXT includes `.TXT`, preserves path-shaped name rejection.
  Returns actual directory-entry spelling on case-insensitive APFS as well as
  case-sensitive hosts. Unix executable-bit checks are unchanged.
- Extension directory selection injects environment lookup and falls back from
  missing/empty HOME to nonempty USERPROFILE. Existing `.omo`, `.pi`, `.omp`
  extension directories receive exact bundled bytes; unchanged installs remain
  no-ops. No absent agent directories are created by installation.
- Actual TerminalSection uses the existing platform helper to omit the Mac-only
  Option control on Windows/Linux; Mac still emits the exact true setting callback.
  Shell selector remains present. No keyboard, clipboard, link or close policy changes.

## Executed evidence

| Command | Result / evidence |
| --- | --- |
| `bun script/qa/p09-agent-contract.mjs` before fixes | exit 101; 2 intended failed assertions, 1 Unix control passed; `p09-contract-red.log` |
| Same command after fixes | exit 0; 3 tests passed; `p09-contract-green.log` |
| `bun run --cwd ui test src/components/settings/TerminalSection.test.tsx` with original unconditional control | exit 1; Windows/Linux failed, Mac passed; `p09-ui-red.log` |
| Same command with platform gate | exit 0; 3 tests passed; `p09-ui-green.log` |
| `bun run --cwd ui build` | exit 0; tsc and Vite completed; existing >500KB chunk warning retained; `p09-ui-build.log` |
| Changed-file LSP | agents.rs only inactive-code hint for non-Unix function; extension, TerminalSection source/test and runner no diagnostics |
| `git diff --check -- src-tauri/src/ipc/agents.rs src-tauri/src/daemon/agent_extension.rs ui/src/components/settings/TerminalSection.tsx` | exit 0 |

The std-only runner extracts exact current production functions and embedded tests,
compiles them with rustc --test, executes the owned binary, and removes its unique
source/binary directory in finally. It does not compile the full application or
prove Windows API behavior. This is a real production-function oracle, not a
JavaScript reimplementation. Original RED runner emitted an unused Path import
warning because original resolver did not use Path; it was not suppressed.
First repair iteration exposed actual-path spelling mismatch on case-insensitive
APFS; fixed production lookup preserves entry spelling. UI staging initially had
a wrong shell-selector ID and missing process.platform injection on Darwin; these
were corrected, then final unchanged assertions reran against unconditional source
for the recorded two-failure RED before restoring the gate. No fixture/setup failure
is counted as RED.

## Exact lead relay needed

1. **Cargo exclusive slot:** no Cargo command was run, because no lead-issued slot
   arrived. Execute the registered `cargo test --manifest-path src-tauri/Cargo.toml
   --lib ipc::agents:: -- --nocapture`, corresponding `terminal::shell::`, and
   `daemon::agent_extension::` commands only after lead checks their ambient tests
   and allocates a safe owned profile/runner. Existing ipc tests detect ambient sh/ls;
   they are not native-Windows-safe without injected staging. Prefer exact new
   `ipc::agents::p09_tests::` and `daemon::agent_extension::p09_tests::` prefixes
   after additive registration. Current std-only tests never modify global env.
2. **P08 server allocation:** `daemon/server.rs` DiscoverAgentSession local branch
   (~1685) currently goes directly from terminal PID to Unix ps/lsof discovery.
   Make authoritative AgentStateHub provider metadata first, matched to requested
   agent/session, before legacy fallback. Test exact ID, wrong-agent rejection,
   provider rotation and absent-provider fallback through the actual request handler.
   DS05 transport must deliver provider metadata before native discovery can pass.
   No shared type change is needed for existing provider metadata. P09 did not edit
   another owner's server or invent invasive Windows process-memory discovery.
3. **P02 metadata:** no shared target-shell DTO was allocated or edited. P02 must
   not infer target shell from host OS; coordinate explicit additive session metadata
   with server/protocol owner if current session contract lacks it.
4. **DS03 / native owner st_01a099f8:** create owned directory with spaces containing
   only a claude.cmd (and separate .bat) fixture and a native executable argv printer.
   Use isolated daemon AgentResume startup and subscribe output/exit before spawn.
   Exact session values: `p09 ordinary`, `p09 & | < > ^ ! %PATH%`, and an apostrophe
   and double-quote case. Require exact argv/session sentinel, no unintended side
   effect file, no CreateProcessW script-format error; native .exe remains direct.
   Capture original RED **before** shell repair, then identical GREEN. Also exercise
   detection IPC and USERPROFILE-only install on native Windows, and actual settings
   control absence in debug GUI. Runtime owner alone may mutate maho-win.

Why DS03 is not guessed: portable-pty 0.9.0 `cmdbuilder.rs:679-695` applies CRT
append_quoted to every argument, while `win/psuedocon.rs` launches the resolved
module with CreateProcessW. Simply prepending cmd /c cannot establish cmd quoting
correctness for quotes, percent expansion and metacharacters. No launch repair is
claimed from PATHEXT availability alone. Native RED and safe interpreter policy
remain mandatory before touching the shell plan.

## Cleanup / integration limits

Runner logs include owned temp-root removal. Each Rust fixture removes its files
before resuming assertion panic, including RED. UI cleanup restores stubbed navigator
and process and unmounts DOM on every test exit. No fixed sleeps, sockets, daemons,
settings writes, audio/dialogs, branch/worktree/commit/push/release/install occurred.
Changes are uncommitted in the shared working tree. No whole-domain audit repetition,
full Rust build, native GUI result, or aggregate completion is claimed.
