# V05 resumed full UI regression result

## Result

Exit 1: 212 test files, 4,399 tests, 4,396 passed, three failed, zero pending.
Only `ui/src/features/ferryx/push/client.test.ts` has failed assertions.
Vitest's nested-suite counters (543 total, two failed) are not file counts.

Failures match the three pre-existing stub limitations recorded in
V05-resume-integration.md:

- `accepts only same origin exact task links`: expected
  `https://ferryx.test/#task=YWJj`, received null.
- `denied permission never subscribes`: expected denied, received enabled.
- `server unsubscribe precedes local removal and failure preserves subscription`:
  expected rejection, received disabled.

No test was removed, skipped, weakened or retried after an assertion failure.
No unrelated push feature was implemented. This run is not labeled PASS.
Native desktop and the not-yet-implemented paired feature gates remain open.

## Commands and evidence

Working directory:
`/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.
Installed Node version: v22.22.3.

The initial command used the existing Vitest script:

```sh
bun run --cwd ui test --no-cache --configLoader runner --reporter=json --outputFile=/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8/docs/evidence/paired-daemon/V05-resume-full.json
```

It exited 1 before test execution: `ReferenceError: __dirname is not defined`.
Monitor mon_NTF5HNR7AT11WD95 recorded V05_FULL_EXIT=1. This is a runner
configuration failure, not behavioral RED. The runner loader was selected to
avoid writing bundled config into read-only linked node_modules.

A temporary `ui/vitest.v05-resume.config.ts` copied all existing Vitest
configuration fields and replaced only the root/alias path resolution with
the explicit absolute UI root. The second command was:

```sh
bun run --cwd ui test --config vitest.v05-resume.config.ts --no-cache --configLoader runner --reporter=json --outputFile=/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8/docs/evidence/paired-daemon/V05-resume-full.json
```

Monitor mon_E4Y3KYBXRN65R64B / bash_11 exited 1 after writing the full report.
Artifacts:

- V05-resume-full.json: full machine-readable Vitest output, all test results
  and failure messages.
- V05-resume-full-before.json: first command and 394 source hashes.
- V05-resume-full-after.json: actual executed second command, counts, exit,
  all 394 after hashes and temporary-config removal receipt.
- V05-resume-full-monitor.log: final monitor status/output and exit sentinel.

All 394 source hashes remained identical. The temporary config was removed
using apply_patch after the run; no production/config source changes remain
from this verification. Parent UI-only git diff --check exited 0.

## Existing diagnostic limitation

LSP reported TS2769 in the temporary config's React plugin entry. Checking
the original `ui/vitest.config.ts` reproduced the same error at line 6:
top-level Vite and Vitest's nested Vite declare incompatible Plugin types.
No cast, suppression or dependency mutation was introduced to conceal it.
This existing config diagnostic is distinct from the test assertions and the
previously successful UI application build.
