# P21 local repair - 2026-09-13

## Scope and outcome

BROWSER-IPC-B12 from repair-packets.md P21 and audit-browser-ipc.md:95-102 is locally repaired. Only PermissionsSection.tsx, PermissionsSection.test.tsx, and this report were written. The two source paths were clean at entry; unrelated existing work was left untouched.

The provider computes Windows allGranted from notifications, which may be OS-managed/non-authoritative. The component previously interpreted every false result as a reason to display macOS Full Disk Access advice. The warning now requires status.platform === "macos", matching the existing card gate. No text or backend behavior changed. Pending status also no longer prematurely displays macOS advice.

Real component tests retain denied/granted macOS controls, settings IPC, notification request/refetch, and onboarding event coverage. Windows unknown notifications retain OS settings while the alert, FDA/accessibility grant controls, notification request, and Mac onboarding action are absent. Assertions do not pin advice prose. Existing advice-prose assertions were replaced with role/control assertions; existing badge/control labels are UI state selectors.

## C002 execution ledger

Every invocation used exactly:

```sh
bun run --cwd ui test src/components/settings/PermissionsSection.test.tsx
```

Package script: vitest run --maxWorkers=1. Host: Darwin arm64, local jsdom, Vitest 3.2.7. Seven invocations total; no hidden retries or excluded tests:

| Local start | Exit | Result | Interpretation |
| --- | --- | --- | --- |
| 13:18:09 | 1 | 6 failed / 6, 30.65s | Harness mistake: every test timed out at 5000ms because mount effect was awaited inside unfinished act. Not behavioral RED. |
| 13:18:59 | 1 | 1 failed, 5 passed, 701ms | Intended alert RED after moving mount before async act. |
| 13:19:17 | 1 | 1 failed, 5 passed, 752ms | Warning fixed; later onboarding absence assertion exposed Mac navigator fixture. Not GREEN. |
| 13:20:04 | 1 | 1 failed, 5 passed, 757ms | Product reverted; intended alert RED with navigator fixture. |
| 13:20:23 | 1 | 1 failed, 5 passed, 711ms | Warning fixed; Darwin process.platform fallback still selected Mac onboarding. Not GREEN. |
| 13:21:06 | 1 | 1 failed, 5 passed, 1.18s | Final faithful RED: original product, aligned navigator/process OS fixtures. |
| 13:21:24 | 0 | 6 passed / 6, 1.20s | Final GREEN: identical test file and command; only two-line product conditional changed. |

Final RED output (relevant receipt):

```text
FAIL PermissionsSection > renders Windows notifications-only surface with OS-managed badge
AssertionError: expected <div role="alert" …(1)>…(2)</div> to be null
PermissionsSection.test.tsx:213:41
expect(screen.queryByRole("alert")).toBeNull();
Test Files  1 failed (1)
Tests       1 failed | 5 passed (6)
error: script "test" exited with code 1
```

The received node was the existing amber alert containing macOS FDA guidance. The assertion consumes the alert role, not that wording.

Final GREEN output:

```text
✓ src/components/settings/PermissionsSection.test.tsx (6 tests) 63ms
Test Files  1 passed (1)
Tests       6 passed (6)
Start at    13:21:24
Duration    1.20s
```

## Determinism and cleanup

Only OS IPC functions are module-mocked. Real React component, Alert, controls, shortcut platform detection and onboarding event behavior execute. Navigator and Node process platform are fixture inputs, restored after every test. Node's Darwin fallback required setting process.platform for the Windows fixture; no production shortcut edit was made.

The IPC readiness promise is installed before render. Render flushes the mount effect, then async act awaits that exact signal and React updates. Notification refetch readiness is installed before clicking. Vitest's 5000ms timeout is only a failure deadline. No sleeps, polling, waitFor, fake component, or skipped test remains. RTL cleanup unmounts components, removing their focus listeners; the one-shot onboarding listener is consumed. No external service, desktop window, daemon, SSH connection, temporary file, dependency installation, commit, ref, worktree or release was created by this task.

## Verification and review

- Final product LSP diagnostics: no diagnostics found. Earlier test-file diagnostics were clean, but fresh final test-file diagnostics timed out repeatedly after the fixture edits; final test-file LSP status is **unverified**, not claimed clean. Lead should include it in the combined type/build gate.
- Scoped git diff --check: exit 0, no output.
- Pure LOC: product 334 (pre-existing oversized module, unchanged line count); tests 213 (warning band). Explicit task scope forbids unrelated splitting; a future growing test edit should separate fixture responsibility if authorized.
- Architectural self-review: product owns permission settings presentation; test file owns that component's regression behavior. Typed existing IPC values remain the boundary contract. No new domain variant dispatch, type escape hatch, defensive layer, error/log boundary, parameter bloat, negative naming or redundant destructive verification was added. Shared renderStatus helper has six callers. Tests distinguish reverting the fix, as the final RED proves. Existing unrelated production error handling remains unchanged.

Real Windows Settings runtime is pending. This is local component/IPC-seam evidence, not Windows desktop E2E or actual settings-launch certification. Combined batch tests/build are assigned to the lead and were not run here.
