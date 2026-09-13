# Windows filesystem / SSH review - 2026-09-13

Status: COMPLETE (source audit; fixes and Windows runtime verification deferred).
Lane: filesystem-ssh. Task: st_01a09848. Reviewed HEAD: b7ad45163e6d90f2c6ae4410a821e57f7198f5e0.
Working tree contains foreign edits, including worktree rescan integration; those are explicitly separated below.
No production, test, configuration, existing-document, branch, worktree, or live-process changes were made.

## Method and evidence boundary

- Read project, backend, IPC, worktree, terminal, daemon and relevant UI AGENTS. Used Rust LSP document symbols and ast-grep structural rename search.
- Requested programming/debugging/ast-grep SKILL files were not found in the installed skill roots searched. Read the installed pi-ast-grep README instead; this is a tooling limitation, not evidence of product failure.
- Re-read finding source spans and callers before writing. Read the prior September 7 domain audit and September 12 FINAL-AUDIT.
- Ran only read-only searches and Bun in-memory models. No build, test suite, desktop launch, SSH operation or Windows process probe was run.
- “Confirmed” means a reachable source mechanism, not a reproduced Windows binary failure. The proposed RED tests and binary observables below remain unexecuted.
- The installed Windows executable is stale; the prior terminal report's fresh debug proof does not establish filesystem/SSH correctness in that installed executable. Wheel behavior belongs to the input/rendering lanes, not this report.

## Complete domain coverage / disposition

| Paths (under src-tauri/src unless otherwise stated) | Reviewed behavior / disposition |
| --- | --- |
| ssh/mod.rs, config.rs, exec.rs, direct.rs | Host DTO compatibility, import, key-path validation, target argv, POSIX quoting, SSH trust flags, bounded output, Windows bridge launch. FSSH-01/03; test defects FSSH-T01. |
| ssh/runtime.rs, operations.rs, browse.rs | Remote-platform rather than local path parsing; drive/UNC acceptance, verbatim/device rejection, PowerShell UTF-16 encoded commands, base64 data, UTF-8 framing, Git CRT quoting, directory browse, upload ACL/create-new, extension replacement. No general Windows execution stub; alias/canonical identity caveats below. |
| ssh/worktree.rs, ipc/ssh.rs, ipc/project_remote.rs | Remote registration/list/create/delete, Windows separators/case guards, branch validation, host revalidation and persistence. FSSH-02/03/04/06; remote create-return lookup case sensitivity remains an unknown. |
| ssh/helper_setup.rs, helper_assets.rs | Home-derived Windows .exe/root, explicit local artifact install, private staged binary, startup marker, CIM-independent helper startup; manifest target/schema/hash/length/filename validation. FSSH-01; live-binary replacement caveat below. |
| ssh/bridge.rs, state_bridge.rs | Dual framed control/read connections, host/owner/epoch checks, bounded/cancel-poisoned transport, authenticated Windows loopback state relay, EOF lifecycle. No local filesystem interpretation of remote paths in transport. |
| ferryx_scope/ssh/{mod,config,process,helper,standalone}.rs, process/process_windows.rs | Private ACL helper, strict endpoint trust/reparse checks, lock-before-stale-publication, loopback token auth, canonical project/cwd jail, helper Git/PTY operations, COMSPEC default, quoted CIM command line. Historical unconditional containment/rename blockers refuted; helper Git verbatim boundary unknown. |
| worktree/{manager,registry,git,model,mod}.rs | Root construction, ordinary/verbatim/UNC Git boundary, target namespace, writer leases, dirty/unmerged deletion guard, branch operations, porcelain parser, canonical-root uniqueness. FSSH-04/05; no blanket local canonicalization failure. |
| ipc/worktree.rs (foreign dirty), ipc/project.rs | All local CRUD/status/preview/list routes, blocking offload and events; remote list branch, canonical registration and Explorer argv. FSSH-05; Updated event addition belongs to foreign rescan. |
| worktree/rescan.rs (foreign untracked), worktree/mod.rs | Full fingerprint, linked/common admin paths, diff/cache, timeout and reconciliation sweep reviewed read-only. FSSH-X01/X02 are foreign-owner findings, not changes to main. |
| session/mod.rs, ipc/session.rs, daemon/server.rs persistence seams | Save/load/clear/corrupt backup, temp lifecycle, GUI data-dir resolver, daemon APPDATA fallback, remote request/descriptor persistence. Rename blocker refuted; concurrency/durability/ACL limits below. |
| terminal/preferences.rs config I/O | Ghostty CLI-first import; HOME/XDG file discovery, explicit-path load, theme resolution and .exe discovery. Missing native profile fallback is real source behavior; supported Windows config location contract remains unknown. |
| Adjacent tests | direct_tests, worktree script tests, bridge fixture/lifecycle tests, helper setup/service/core test seams, projects persistence tests, scoped_ssh, ssh_windows_live, session persistence and worktree_safety coverage examined. Platform gaps below; live Windows tests are opt-in, not pass evidence. |

## Confirmed findings, severity order

### FSSH-01 - HIGH: Windows helper bridge loses a root containing spaces

- Source: `src-tauri/src/ssh/direct.rs:249-263`, specifically raw `StartInfo.Arguments` concatenation at 255. Base64 decoding protects PowerShell syntax, not the child's argv boundaries.
- Reachable chain: `terminal/remote.rs:265` RemoteRuntime::create (also reconnect at 191) -> `ssh/bridge.rs:792-805` connect_with_target -> `BridgeConnection::spawn:373-381` -> bridge_plan -> bridge_command. `helper_setup.rs:56-61` derives the root from the Windows user profile.
- For `C:\Users\QA Person\.ferryx\helper\h`, the helper gets `--root C:\Users\QA` plus an extra argument; `ferryx_scope/ssh/process.rs:373-380` takes only the next argument. Startup can succeed but bridge handshake cannot find the correct endpoint.
- RED: extend the Windows bridge test to execute the generated command against an argv-recording fixture with a spaced, non-ASCII root and assert one exact root argument; then perform the real framed handshake against an owned helper in that root. Bound on response/EOF, not a sleep.
- Binary observable: new SSH terminal fails after successful helper startup; fixed binary returns handshake protocol=1 and correct host/owner from the spaced-root endpoint.
- Smallest write scope: direct.rs plus direct_tests.rs / owned Windows bridge integration. Apply proper Windows argv quoting compatible with Windows PowerShell 5.1; do not rely solely on modern ArgumentList.

### FSSH-02 - HIGH: accepted Windows cwd is silently replaced with repository root

- Source: `src-tauri/src/daemon/server.rs:1230-1245` uses case-sensitive string strip_prefix and `unwrap_or("")` after `ssh/worktree.rs:169-175,184-204` accepted a case/separator-equivalent descendant and returned it unchanged.
- Reachable chain: SpawnCoordinator::spawn_remote at `daemon/server.rs:1187` -> resolve_remote_spawn_root -> RemoteSessionConfig.worktree=None -> `terminal/remote.rs:260-276` -> helper `pty.spawn`, `ferryx_scope/ssh/helper.rs:278-286`, whose default is `.`.
- Concrete values: stored root `C:\Repo`, requested cwd `c:\repo\src` or `C:/Repo/.orca-worktrees/wt-feature`. Both validate but the relative string becomes empty. The Bun model reproduced both transformations; it did not execute Rust or Windows.
- RED: test the production config-construction seam with those two inputs plus UNC host/share case aliases; assert worktree=`src` / `.orca-worktrees/wt-feature`, never None. Owned helper integration must report the requested cwd from pty.describe.
- Binary observable: terminal opens in main checkout instead of selected worktree, so commands operate on the wrong checkout. Fixed binary reports the selected directory and branch.
- Smallest write scope: daemon/server.rs conversion seam and adjacent daemon remote SSH tests, coordinated with daemon owner; retain remote-platform semantics and reject conversion failure instead of silently choosing root.

### FSSH-03 - MEDIUM: quoted OpenSSH IdentityFile imports are unusable

- Source: `src-tauri/src/ssh/config.rs:72-76,94-100,124` preserves enclosing configuration quotes. `ssh/direct.rs:32-38` then rejects the quoted string as neither a local absolute path nor `~/...`.
- Reachable chain: `ipc/ssh.rs:222-245` cmd_ssh_import_config -> parser/import -> stored host -> cmd_ssh_test_connection/runtime::detect -> ssh_plan/validate_host (also enabled_host during project registration).
- Valid config example: `IdentityFile "C:/Users/QA Person/.ssh/id_ed25519"`. Import succeeds but subsequent connection fails before ssh.exe starts. Quoted Unix keys are affected too; Windows profile spaces make this especially relevant.
- RED: parse/import that config and assert the semantic path without quote delimiters; on Windows assert ssh_plan accepts it and produces exactly one `-i` value. Include apostrophe/space and escaped-quote grammar cases without pinning prose.
- Binary observable: imported trusted host connects with the same key that works in OpenSSH, rather than returning InvalidPath.
- Smallest write scope: ssh/config.rs parser and adjacent config/direct tests; parse OpenSSH quoting rather than blindly trimming every quote.

### FSSH-04 - MEDIUM: managed worktree names pass validation but are invalid Windows path/ref components

- Source: `src-tauri/src/worktree/manager.rs:385-420` permits `< > " |` and DOS device basenames (including extension forms); `299-306` puts workspace ID and slug segments directly into the filesystem path.
- Reachable chain: `ipc/worktree.rs:112-133` -> worktree_path_for/format_branch_name -> create_worktree -> `manager.rs:442-451` mkdir/Git. Remote create reuses format_branch_name via `ssh/worktree.rs:328-345` and `ipc/ssh.rs:384-411`.
- RED: table-drive `bad|name`, `CON`, `aux.txt`, `nested/LPT1.log` as ws-id/slug components; assert typed rejection before filesystem/Git side effects on Windows. Keep legal Unicode/ordinary nested slugs accepted.
- Binary observable: invalid creation is rejected as InvalidNamespace before orphan directories/refs, not generic Git/IO failure. No claim that every device alias causes data loss.
- Smallest write scope: manager namespace/path validation plus worktree tests; remote Windows validation must key off remote platform, not the desktop OS alone.

### FSSH-05 - MEDIUM: deletion does not coordinate active interactive PTYs on Windows

- Source: `src-tauri/src/worktree/manager.rs:558-575` checks exclusive writers only and immediately removes; `worktree/git.rs:379-391` runs once. Interactive PTYs deliberately have no exclusive lease (`terminal/pty.rs:80-104`).
- Reachable chain: `ui/src/components/WorktreeDeleteDialog.tsx:27-40` -> `ipc/worktree.rs:147-171` -> manager deletion -> git remove. There is no daemon session-close/reap step in that route.
- A live shell with cwd in the checkout, or an external handle opened without delete sharing, can prevent Windows removal even under --force. Missing retry alone is not the defect; active process ownership is not consulted.
- RED: owned Windows helper opens a checkout directory without delete sharing and signals readiness via pipe; trigger deletion while it is held, assert a typed busy result without mutation. Release on an explicit pipe message, await process exit, then assert deletion succeeds. Add actual PTY-cwd case.
- Binary observable: deletion while a checkout terminal remains open currently surfaces Git/permission failure; fixed flow explicitly reports in-use or closes only user-authorized owned sessions and waits for their exit.
- Smallest write scope: worktree deletion IPC/service coordination and worktree_safety tests. Do not indiscriminately kill agents or “fix” this with blind retries.

### FSSH-06 - MEDIUM: unreadable/corrupt host store is treated as empty and overwritten

- Source: `src-tauri/src/ipc/ssh.rs:179-183` defaults on every read/parse error; update/import/delete subsequently publish a new store (`231-245`, `254-274`, `186-205`).
- Reachable chain: cmd_ssh_update_host -> load_store -> default empty inventory -> save_store. Windows non-read-sharing handles can cause the read to fail; corrupted JSON reproduces the same mechanism on all platforms without timing luck.
- RED: seed invalid JSON, invoke the real update/import core, assert an error and byte-for-byte unchanged inventory. Windows second case uses a controlled deny-read/share-delete handle and barriers to show no empty replacement after read failure.
- Binary observable: editing one host after a load failure must not erase other saved hosts; current implementation can replace the whole inventory with the edited host.
- Smallest write scope: ipc/ssh.rs load/mutation error propagation and adjacent store tests. Keep explicitly supported per-entry resilient parsing separate from whole-store corruption.

### FSSH-T01 - MEDIUM, tests: Windows SSH coverage fails before reaching the behavior under test

- `src-tauri/src/ssh/direct_tests.rs:4-16,19-37` supplies `/keys/...`; production `direct.rs:32-38` uses local Path::is_absolute, which requires a drive/UNC prefix on Windows. Multiple unconditionally enabled positive plan tests fail validation.
- `src-tauri/src/ssh/bridge_tests.rs:23-31` asserts a helper path without `.exe`; TestFixture::new at 88-89 uses it before every helper-backed case. Native Windows Cargo produces `.exe`; Path::is_file does not perform executable suffix search.
- `src-tauri/src/ssh/worktree.rs:531-590` unconditionally executes a POSIX script in bash against locally canonicalized Windows paths. A stock Windows host need not have bash; WSL bash is not a transparent Git-for-Windows executor.
- The bridge lifecycle case at `bridge_tests.rs:584-586,607-650` uses cmd.exe startup output with fixed cursor=1 and timer-based cancellation; output arrival can complete the supposedly blocked read. This is nondeterministic even though timeout itself is tested in Part A.
- RED/verification after fixes: run the exact direct plan and bridge handshake filters once on native Windows, with a built helper and a local absolute key fixture; run platform-specific worktree execution tests. Replace cancellation timing with a request-received barrier and a deliberately blocked responder. Do not skip the Windows behavior to turn green.
- Binary observable: none; these are validator defects. They invalidate claims of portable test coverage, not independently prove a shipped runtime regression.
- Smallest write scope: those three adjacent test files; keep remote POSIX fixtures when testing remote POSIX semantics, but local key paths and executables must be native.

## Foreign rescan findings - report to owner, do not edit

- **FSSH-X01, MEDIUM (all platforms):** `src-tauri/src/worktree/rescan.rs:412-418,439-446` resets last_scan on every unchanged fingerprint. Caller `sweep_registry_once` at 383-388 / background task at 499-549 invokes it every 30s, so the five-minute forced reconciliation never becomes due during ordinary operation. Bun model: 600 seconds at 30-second increments produces only the initial real scan. RED: injected Instant and constant fingerprint, advance 30s per sweep, assert a real scan at 300s; no sleeps. Observable: a change missed by fingerprint can remain invisible indefinitely. Scope: foreign rescan cache timestamp + owner tests only.
- **FSSH-X02, MEDIUM (Windows, malformed metadata):** `src-tauri/src/worktree/rescan.rs:266-278` rejects traversal by splitting only `/`, then joins an unchecked reference. `refs/..\..\outside` passes that check but resolves outside the common directory on Windows. Caller read_worktree_admin_fingerprint at 148-150/165-176 consumes HEAD metadata. This is a read-only jail violation, not a demonstrated remote exploit or exfiltration. RED: Windows temp common dir, malicious HEAD and sentinel outside; assert ref fingerprint returns an unfollowed marker without reading sentinel. Observable: fingerprint must not encode outside-file content. Scope: foreign ref-component validation/tests; reject native separators/traversal before joining.

## Prior audit dispositions / refutations

- **L2-FS-PATHS-1/2: refuted as unconditional blockers, not fixed by this audit.** projects.rs:135-138 and scoped config.rs:46-50 retain handles, but neither changes share_mode. Installed Rust std source `library/std/src/sys/fs/windows.rs:203` defaults to FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE; rename at 1271-1272 uses MOVEFILE_REPLACE_EXISTING. Cleanup runs after closure unwinds/drops the handle. An external deny-delete handle remains a separate conditional risk. Scoped HostStore's concrete caller is tests/scoped_ssh.rs:16-21, not the desktop host inventory.
- **L2-FS-PATHS-3: current source refutes it.** Helper project.register canonicalizes at helper.rs:174-186; cwd and worktree-base checks at 202-204/280-284 compare canonical paths. There is no unconditional raw-versus-verbatim mismatch.
- **L8-SHELL-AGENT-10: refuted for normal local IPC.** manager.rs:154-170 stores canonical root, 261-266 canonicalizes candidates, and 299-306 derives new targets from that root. git.rs:39-60 strips drive/UNC verbatim prefixes only at the subprocess boundary. Do not strip one side of the jail comparison as the historical recommendation suggests.
- **L2-FS-PATHS-5: retained as FSSH-04. L2-FS-PATHS-6: ADS claim refuted.** Registry IDs can contain a colon, but target creation calls format_branch_name first, which rejects colon at manager.rs:411. Other illegal-name behavior is covered by FSSH-04, not an ADS bypass.
- **L2-FS-PATHS-11 / L8-SHELL-AGENT-6: retained with narrower FSSH-05 mechanism.** Not every open Windows handle blocks deletion; sharing mode matters, and retry cannot resolve a shell that remains alive.
- **L2-FS-PATHS-8: conditional ACL risk, not “all users can read” proof.** Project store uses inherited Windows ACLs; its normal parent comes from per-user Tauri app_data_dir (`ipc/ssh.rs:73-85`). Broad FERRYX_DATA_DIR ACLs need a separate probe. The project store contains host IDs/paths, not private key bytes.
- **L2-FS-PATHS-16: best-effort durability gap, not save failure.** session/mod.rs:225-229 ignores directory-open/fsync failure; file sync and rename already completed. A power-loss durability claim needs native storage evidence; simply cfg-gating the call does not add durability.
- **L10-TESTS-TOOLING-1: refuted.** Session test writes under tempfile; `/tmp/repo` fields at session/mod.rs:295-319 are serialized values, never directory opens. **L10-TESTS-TOOLING-3:** `/tmp` socket assertion describes a POSIX remote command, not a Windows named pipe contract. Its current failure risk is the local key fixture in FSSH-T01. **L10-TESTS-TOOLING-4:** direct /bin/sh execution tests now have cfg(unix); remaining ungated local-path assumptions are FSSH-T01.
- **L2-FS-PATHS-13:** HOME/XDG-only fallback remains in preferences.rs:638-678 (theme path at 494-553); ghostty CLI is tried first at 681-688 and explicit-path loading exists at 616. The old assertion that a canonical native Windows Ghostty location must exist was not established; record as configuration-contract unknown, not a universal terminal failure.
- Prior L2 process discovery/CWD/debug logging/browser opener/auth/manifest items (4/7/9/10/12/14/15) are assigned to daemon, input, or platform-integration lanes; no independent fixed/open claim is made here. September 12 FINAL-AUDIT contains no filesystem/SSH repair proof to adopt.

## Unknowns and exact later probes

- Session save and desktop host-store save use a fixed sibling temp name without a file-scoped mutation lock (session/mod.rs:192-217; ipc/ssh.rs:197-199). Concurrent IPC writers can overlap; deterministically instrument after create/write with barriers and assert both outcomes plus parseable final bytes. Cross-process serialization policy and observed user-level incidence remain unverified.
- Session/project/host replacement may fail under antivirus or explicit deny-delete readers. Test two sequential saves while a Rust default-shared reader stays open (expected success), then controlled share_mode excluding DELETE (expected explicit error and preserved prior state). Do not label ordinary Rust readers a blocker.
- Windows remote canonical identity uses Get-Item.FullName (`operations.rs:69-81`) and exact-root hashing (`projects.rs:87-93`); drive/share case aliases, junctions, short names and case-sensitive directories need native same-file identity tests. Remote create's return lookup (`ipc/ssh.rs:414-426`) normalizes separators but not case; probe lower-case drive input versus Git porcelain before confirming a duplicate/create failure.
- Helper `worktree.create` passes canonical verbatim repo/destination directly to Git (`helper.rs:208-215`), unlike local git.rs normalization. Test actual supported Git-for-Windows with drive, UNC, long and non-ASCII roots; this framed operation has no production UI caller found beyond its exposed RPC/client methods.
- Helper replacement stages/renames/removes the existing .exe (`helper_setup.rs:118-143`) while a helper may be running. Windows image mapping/delete sharing behavior and rollback need a versioned owned-helper install probe; never terminate the user helper/daemon to test it.
- private_file uses icacls inheritance removal plus grant replacement for USERNAME (`ferryx_scope/ssh/mod.rs:7-15`), not removal of every explicit foreign ACE. Endpoint validation separately rejects foreign allow ACEs/reparse points (`process.rs:26-71`). Test domain/local username ambiguity and explicit-ACE leftovers; inherited profile privacy alone is not a verified ACL guarantee for overrides.
- Ghostty preferences: settle the supported Windows user config location, then probe GUI-launch environment with HOME/XDG unset and that one file present. CLI-first success and absent optional import are distinct from broken terminal rendering.

## Verification receipt

- Reopened every confirmed finding span and at least one caller, plus historical rename/canonicalization spans and Rust Windows defaults. LSP returned manager symbols; ast-grep found the three scoped/project rename sites.
- Bun in-memory probes confirmed the cwd-to-empty conversion and reconciliation timestamp arithmetic; bridge whitespace argv example was a model only, not Windows execution.
- `git diff --stat` before the report still showed 23 foreign tracked files. Counts changed from 120 to 178 insertions because a concurrent renderer owner added work; no source write command was issued by this lane. Only this previously absent report was added with apply_patch.
- No tests/builds/runtime QA were run by design. All failing-first proposals above are pending, and no installed-binary repair or validation is claimed.
