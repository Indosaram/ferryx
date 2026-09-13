# P17 MSIX resource staging implementation

Status: scoped implementation and native fixture tests staged; native RED/GREEN and parser validation blocked on the runtime owner's Windows execution. This is not completion of P17 acceptance or the aggregate objective.

## Delivered

- `scripts/build-msix.ps1` copies repository-relative `ui/dist` and `src-tauri/resources/helpers` to `ui/dist` and `helpers` beside the packaged executable. Source paths derive from the script, not invocation CWD, matching `tauri.conf.json` and the executable-relative runtime lookup.
- Packaging rejects missing index, manifest, or declared helper binaries; validates helper schema/protocol, target/filename and manifest length/SHA256 before MakeAppx. Copies the entire resource trees, including nested UI assets and every declared target.
- `scripts/test-build-msix.ps1` retains the existing 12 cases and adds four missing/corrupt resource cases. Its real MakeAppx fixture now checks archive paths, lengths and SHA256 for index, nested JS, manifest and three target helper fixtures. Helpers are intentionally byte fixtures, not runnable production binaries. All inputs live in a GUID scratch repository; no checkout resource edits. `-BuildScript` selects original versus repaired implementation without overwriting either. Cleanup is checked and reported.

Cause trace: `scripts/lib/release-platforms.mjs` invokes build-msix after a no-bundle Tauri build; MakeAppx consumes only the explicit staging directory. Previously that directory contained executable/icons/manifest only. `remote/server.rs::resolve_dist_dir_from` expects executable-adjacent ui/dist/index.html; `ssh/helper_assets.rs::resolve_helper_asset` consumes helpers/manifest.json and target/filename with manifest hash/length validation. The correction supplies that layout rather than changing runtime fallback behavior.

## Registration and local evidence

Official `executeAgentToolkit`, bound to repo CWD and parent session `01a0983f-c995-753d-afa9-593f6d118788`, accepted C002 `revise_criterion` before edits, preserving its full prior scenario. Receipt: `p17/registration.json`.

- `node --test scripts/build-msix.test.mjs`: exit 0, 8 tests passed, 0 failed (`p17/node-contract.log`). These are existing source contracts, NOT behavioral RED/GREEN proof.
- `git diff --check -- scripts/build-msix.ps1 scripts/test-build-msix.ps1`: exit 0 (`p17/diff-check.log`).
- Both .ps1 LSP requests unavailable: no configured server. Neither powershell.exe nor pwsh exists locally (`p17/diagnostics-cleanup.log`). No native test was attempted or claimed as an intended RED.
- Original product script preserved exactly in `p17/build-msix.original.ps1`; original SHA256 `39ce06318980284b654782218a36ed8bbcc78d11a0cb6736e3dada44c5eb5ff8`. All source hashes in `p17/source-sha256.log`.

## Exact lead relay to runtime owner st_01a099f8

Only the runtime owner may stage/run this on maho-win. Copy the test, repaired product script and preserved original to an owner-controlled scratch folder, preserving the test/product sibling names; no full repository, production executable, certificates or install needed. Host needs Windows PowerShell, .NET Framework csc.exe and Windows SDK MakeAppx.exe. Execute these from that scratch folder, capture stdout/stderr and exit codes:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\test-build-msix.ps1 -BuildScript .\build-msix.original.ps1
# RED: real MakeAppx fixture reaches archive checking and fails Missing packaged resource;
# missing/corrupt-resource tests also fail on original. Tool discovery failure is NOT RED.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\test-build-msix.ps1 -BuildScript .\build-msix.ps1
# GREEN: Total=16, Passed=16, Failed=0, exit 0, with scratch cleanup receipt.
```

Run the original first with the exact same test file; fix is currently staged without the requested prior observed RED because the exclusive native runner is unavailable to this child. Do not represent chronology as observed RED-before-repair. Return real logs to `p17/native-red.log` and `p17/native-green.log`, source hashes, PowerShell/SDK versions, and cleanup receipt. Also parse both changed scripts with `[System.Management.Automation.Language.Parser]::ParseFile` and require zero parse errors before execution. No signing occurs in successful fixture packs (`-SkipSigning`); retained negative signing tests only check missing parameters/certificates. No installation, release, daemon or GUI launch is part of these commands.

The original separate acceptance for a real clean application serving authenticated remote UI and resolving helper assets outside the repository remains unverified and must use an owner-approved actual application/runtime fixture. A synthetic PE used by this suite cannot prove that runtime behavior. No Cargo slot requested because none is needed for archive fixtures.

## Safety and ownership

Scoped scripts were clean at initial and pre-write git checks. Foreign dirty work remained untouched. Only those scripts plus this packet's evidence files were authored. Redundant owned staging text was removed; no native scratch was created locally. Work is uncommitted in the shared tree. No branch/worktree creation/deletion, main switch, commit/push, release/sign/install, global environment change, remote mutation, or daemon action was performed.
