# Q4 Windows platform repair candidate

## Outcome

The private candidate resolves the previously observed Windows selector failures.
Remote tests pass 172/172, worktree-filter tests pass 40/40, the native identity
repro passes, and the headless library/CLI/relay build exits 0. The final affected
integration command exits 0: catalog 4, legacy bounds 2, machine worktrees 2,
relay regression 6 and worktree safety 9. The Unix-only transport target executes
zero tests and is **not Windows coverage**.

This is a review candidate, not production composition or full Q4/release
acceptance. No existing worktree source was edited. All changes live in the
retained private snapshot and the attached patch. Clipboard compilation and its
six tests were already parent-verified; that repair is the baseline here, not
part of this candidate patch.

## Deliverables and identity

- `Q4-windows-platform-candidate.patch`: exact 14-file candidate against the
  frozen snapshot plus the approved clipboard delta.
- `Q4-windows-platform-candidate-source-hashes.json`: per-file before/after
  SHA-256 (null before for the new targeted repro).
- `Q4-windows-platform-candidate-manifest.json`: complete final 970-input
  source/assets identity. Original manifests and all prior logs are preserved.
- `Q4-windows-platform-native-candidate-*.log`: complete native stdout/stderr,
  exact commands, process IDs and waited exit receipts.
- `Q4-windows-platform-candidate-*.ps1`: bounded monitor/isolation commands.

All final 970 inputs matched on Windows during cleanup. The frozen source's
original 34 hashes also matched locally at finish. No active A10/Q1/Q3 source
was imported. The separately reviewed Linux readiness patch was imported
verbatim in its three relevant fixture files (catalog, project crash and
worktree crash markers); its newline-before-marker changes are included in
this exact patch and must not be double-composed by the parent.

## Diagnoses and minimal fixes

1. **Test compilation:** `owned_session: Option<String>` removes the Windows
   unsized-str inference failure while retaining cleanup and all test bodies.
2. **Project identity:** the HTTP resolver returned a normal drive path, while
   project availability compared it with `std::fs::canonicalize`'s verbatim
   result. Registration now retains canonical native identity internally;
   revalidation converts its trusted stored path to navigable syntax before
   applying the unchanged request policy. UNC/device input rejection remains.
3. **Worktree identity:** Git porcelain emits `C:/...`, whereas the manager uses
   `\\?\C:\...`. A small native Git-output adapter converts only drive-prefixed
   paths to native verbatim form, including missing/prunable rows without
   inventing filesystem existence. Both machine NUL and legacy porcelain use
   it. The new native integration repro failed with those exact unequal paths
   before the production change, then passed. Existing owner error, project,
   worktree revision and transaction assertions also passed after the change.
4. **Fixture filenames:** Windows uses apostrophes, spaces and Unicode where
   double quotes/trailing spaces cannot be created. POSIX keeps its original
   quote/trailing-space adversaries. Windows directory coverage additionally
   asserts invalid-name creation failure, normal drive-root browsing with no
   parent, and 422 refusal of UNC, verbatim UNC, device and verbatim request
   strings. No UNC share was created or assumed reachable.
5. **PTY fixtures:** use cmd.exe on Windows, retain /bin/sh on POSIX. Actual
   relay output exposed ConPTY's ESC[6n cursor query. The headless test client
   answers it before sending shell input. A two-command variable expansion
   produces a marker absent from the input bytes, so terminal echo alone cannot
   satisfy execution. Natural-exit worktree safety also answers the query.
   No real clipboard or desktop was involved.
6. **SSH POSIX script fixture:** `bash` on this host selected the Windows WSL
   launcher, whose stdout reported no installed distribution. The fixture now
   locates Git for Windows' bundled bash relative to git.exe on PATH and sends
   its private drive path in slash form. The actual POSIX script still executes;
   it is not cfg-skipped or replaced with a string assertion.
7. **Relay close seam:** the strengthened fixture waits for a real browser close
   acknowledgement and rejects ResetWithoutClosingHandshake. It reproduced a
   deterministic failure even when the old send-and-drop test happened to pass.
   The reverse fixture now flushes inner close and closes its data socket on
   TCP EOF rather than abruptly dropping it. The production bridge waits for
   the inner acknowledgement, flushes the browser's queued close, and drains
   the application after tunnel EOF rather than discarding buffered close
   bytes. All waits remain bounded by the existing transfer deadline. Errors
   are not accepted as successful close. Failed intermediate close approaches
   and the diagnostic bridge error log are retained.

## Verification chronology and exact boundaries

All cargo commands use the existing private `--config ...\cargo-qa.toml` empty
wrapper override, `--locked --manifest-path src-tauri/Cargo.toml
--no-default-features`, private target, max three jobs, debug=0 and incremental=0.
Rust/Cargo 1.97.0 ran natively on maho-win (x86_64-pc-windows-msvc). HOME,
USERPROFILE, APPDATA/LOCALAPPDATA, FERRYX runtime/data/session, XDG and temp paths
were isolated before initialization. Cargo/Rustup homes were explicitly retained.
Every command ran with explicit private source cwd and a bounded child-process
wait, not sleep/poll-to-green.

Original failing-first evidence is in Q4-windows-repaired-verification.md.
New native identity RED is `candidate-red-paths.log`: 0 passed/1 failed, exit
101, actual drive versus verbatim mismatch. Targeted rounds preserve every
subsequent failed assertion and command, including ConPTY output and relay close
failures. They are changed-code/diagnostic runs, not unchanged retries.

The first aggregate (`candidate-aggregate-exits.log`) records:

| Command suffix | Exit/result |
| --- | --- |
| `test ... --lib remote:: -- --nocapture` | 0; 172 passed |
| Six requested integration targets | 101; newly reachable relay fixture sent a verbatim browser path and received 422 |
| `test ... --lib worktree:: -- --nocapture` | 0; 40 passed |
| `test ... --test q4_windows_paths -- --nocapture` | 0; 1 passed |
| `build ... --lib --bin ferryx-cli --bin ferryx-relay` | 0 |

The relay fixture was corrected to send navigable drive syntax without changing
request policy. Its next affected integration run reached previously blocked
worktree-safety /bin/sh fixtures, which were repaired portably. The final
affected aggregate (`candidate-safety-suite.log`, `candidate-safety-exits.log`)
passes all six requested targets with exit 0 and the counts above. This is not
one globally green first aggregate: both failed integration aggregates remain
explicitly recorded. Only those two integration fixture files changed after
the green remote/worktree/build aggregate; production bytes are unchanged.

Real runtime receipts include HTTP project registration/replay/unregister,
native directory browsing, direct=relay U+FFFD listing, worktree lost 201 reply
with zero forwarded bytes and identical replay, dirty/locked/unmerged/partial
delete, owner restart/revision, project crash markers/reaping, and relay WS
text/binary/ping plus executed cmd PTY marker. Worktree safety executes two
simultaneous same-worktree PTYs, natural exit and five-worktree lifecycle cleanup.
Unix-only transport/process-group tests remain cfg-excluded. Ordinary Windows
Git runner execution passes; this does not establish an adversarial descendant
Job cleanup/ACL/hidden-attribute acceptance packet. No new Job or ACL fixture
was authored in this repair candidate.

Local LSP reported no errors for relay_server.rs. The directory diagnostic tool
selected JSON and capped its scan at 50 files, so that result is not claimed as
Rust diagnostics for every changed file. Native compiler/test results above are
the actual cross-platform validation. All compiler warnings remain in logs.

## Cleanup and retention

`Q4-windows-platform-native-candidate-cleanup.log` and the SSH receipt record
cleanup exit 0, 970 matching source inputs, zero owned runtime processes after
excluding the exact current cleanup wrapper, and absence after removing all
nine private runtime/home/data/session/config/cache/temp/appdata/localappdata
directories. Immediate build/test children were waited. Individual fixture
logs retain listener joins, child waits and root removal; the earlier temporary
Windows sharing-violation cleanup failure is preserved, not erased.

The private source/target, original archives/bundle, candidate baselines and
logs remain retained under the assigned local/Windows QA roots. No installed
app, canonical daemon/credential, external service, privileged configuration,
release build or production checkout was changed. Evidence and patch are
uncommitted. Parent review and composition remain required.
