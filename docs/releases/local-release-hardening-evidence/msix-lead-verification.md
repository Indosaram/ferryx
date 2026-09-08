# Lead Windows MSIX verification

Date: 2026-09-08. This real-surface run supersedes any earlier worker-only completion claim.

## Regression discovered and fixed

The first lead run hung on missing mandatory ExePath because test-build-msix.ps1 launched a child powershell.exe without -NonInteractive. Lead captured the hang at test 1, stopped only processes whose command line contained the unique test directory stem, and observed original monitor exit 255 with MSIX_LEAD_CLEANUP_OK.

All ten nested PowerShell invocations were corrected to include -NonInteractive. No production packager behavior or test assertion was weakened.

## Actual invocation

Copied only scripts/build-msix.ps1 and scripts/test-build-msix.ps1 to the fresh owned directory C:/Users/sook/AppData/Local/Temp/ferryx-msix-lead-1788848764749. Executed:

```sh
ssh -o BatchMode=yes -o ConnectTimeout=10 maho-win powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/sook/AppData/Local/Temp/ferryx-msix-lead-1788848764749/test-build-msix.ps1
```

Copy and cleanup were part of the same bounded monitor command. Monitor mon_K63PKQ39V0TMGTZB / bash_13.

## Captured result

```text
status: completed exit_code: 0
RESULT: PASS
RESULT: PASS
RESULT: PASS
RESULT: PASS
RESULT: PASS
RESULT: PASS
RESULT: PASS
RESULT: PASS
RESULT: PASS
MSIX successfully validated: ProjectMaho.Ferryx 2026.908.1.0
RESULT: PASS
Test Run Summary: Total=10, Passed=10, Failed=0
MSIX_LEAD_CLEANUP_OK
```

The valid package scenario used the real Windows SDK MakeAppx and read back ProjectMaho.Ferryx 2026.908.1.0 from the package manifest. Fault scenarios covered absent input, invalid calendar/quad versions, binary version mismatch, stale output, native tool failure and explicit signing mode. This proves packaging script behavior with small executable fixtures, not a full Ferryx build or Store submission.

## Cleanup

- Test process completed exit 0; harness removed its fixture scratch in finally.
- Outer transport removed C:/Users/sook/AppData/Local/Temp/ferryx-msix-lead-1788848764749; emitted MSIX_LEAD_CLEANUP_OK.
- Interrupted first run left one empty scratch directory, identified from its creation time and contents: C:/Users/sook/AppData/Local/Temp/ferryx-test-scratch-bfda6974fc8f42a5ab0813730154d8e6. Removed only that empty directory; monitor mon_9MXNNAAC9V3C9N8K / bash_15 exited 0 with INTERRUPTED_MSIX_SCRATCH_CLEANUP_OK.
- No user checkout, app, daemon, signing key or global configuration changed.

## Integrated foundation check

Before this harness-only correction, lead ran node --test scripts/sync-version.test.mjs scripts/release-workflow.test.mjs scripts/minisign-verify.test.mjs scripts/build-msix.test.mjs under mon_XVE5T63FVMBREPEC / bash_10: 65 tests, 65 passed, zero failed/cancelled/skipped, exit 0. Native PowerShell behavior above is the authority for the Windows version/exit rules; JavaScript contract checks alone do not prove those rules.
