# Remaining PowerShell helper wrappers

2026-09-13. Source review only; neither wrapper was executed.

## Complete named source coverage

- `scripts/qa/check-remote-helper.ps1`: all 28 lines read.
  Mandatory caller-owned archive -> random TEMP fixture -> tar extraction ->
  locked helper cargo check -> finally removes fixture and input archive.
- `scripts/qa/verify-remote-helper.ps1`: all 43 lines read.
  Mandatory archive -> random TEMP fixture -> locked cargo test with optional
  filter -> ContractsOnly success, or debug cargo build -> guessed helper
  executable -> `ssh-helper-survival.mjs`. Finally restores the helper-binary
  environment variable and removes fixture and input archive.
- `remote-helper/Cargo.toml`: standalone binary targets
  `../src-tauri/src/ferryx_scope/ssh/standalone.rs`. These wrappers do not launch
  the desktop application or build a release.
- `scripts/qa/ssh-helper-survival.mjs:292-335`: child cleanup catches errors,
  removes its fixture before the remaining-live-PID assertion, and sets
  process.exitCode on failed cleanup. This is existing GT-01 evidence.

## Confirmed boundary defects

**GT-01 extension: input ownership and failure cleanup.** Both wrappers delete
the caller-supplied Archive in finally without an ownership or consume-input
contract. Their unique fixture name proves ownership of that new fixture,
not the input archive. The verification wrapper also removes the extracted
source/build tree after a failed child harness, without confirming that
harness-owned processes have exited. The existing lower-level GT-01 cleanup
problem is not repaired by this outer finally. No arbitrary archive or live
harness was used to demonstrate deletion.

**GT-05 related wrapper artifact lookup.** The verification wrapper hardcodes
`remote-helper\target\debug\ferryx-remote-helper.exe` after Cargo succeeds.
Inherited CARGO_TARGET_DIR or target configuration may put the actual compiler
artifact elsewhere. This is conditional, not a claim that the normal local
default path fails. Capture Cargo's compiler-artifact executable, or enforce
and verify an owned build-output contract; do not accept a stale file.

**Validator coverage gap.** ContractsOnly treats cargo exit 0 as success even
when TestFilter matches zero tests. A zero-match invocation is not evidence of
Windows contract execution. Keep invocation success distinct from a nonzero
executed-test receipt.

## Non-defects and unresolved observations

TEMP plus GUID is a real isolation measure; these wrappers do not explicitly
target installed user daemons. Cleanup exceptions are not explicitly
suppressed by these PowerShell wrappers: ErrorActionPreference is Stop.
Nevertheless, printed cleanup text alone is not a process/port receipt.
Tar receives Archive as one native argument; no shell-string interpolation
defect is established here. Untrusted archive traversal was not tested and
is not asserted. PowerShell-version-specific native stderr behavior and
actual helper test selection remain execution prerequisites, not observed
failures.

## Regression registration proposals, not executed commands

Before changes, allocate an import-safe/injected native-command seam in an
owned fixture. The following is a proposed new test harness, not an existing
safe command:

`powershell.exe -NoProfile -File scripts/qa/helper-wrapper-contracts.ps1`

Binary conditions:

1. Given a caller-owned archive and simulated successful or failed Cargo,
   the archive remains byte-identical; only the owned extracted fixture can
   be removed. The old unconditional Remove-Item must fail this assertion.
2. After child harness failure with an explicitly signaled live owned child,
   the wrapper returns failure and retains required cleanup material until
   exact child termination is observed. Never use unrelated PIDs.
3. An owned nondefault Cargo target directory resolves exactly the emitted
   debug executable; a stale default-path file is not selected.
4. A filter matching zero tests is reported as incomplete verification, not
   a Windows acceptance success.

Actual native Windows helper contracts and survival execution follow only
after the lower-level GT-01 safety repairs, with verified owned processes,
ports, roots, executable identity and cleanup receipts. These cases extend
existing packet families rather than creating duplicate product findings.

## Integrity receipt

Pre-write git diff still showed the same 30 foreign tracked files with
391 insertions and 40 deletions. No source, test, config, archive, process,
daemon, branch or worktree was modified by this review. Only this report was
added. The active loop remains incomplete and isolation approval is pending.
