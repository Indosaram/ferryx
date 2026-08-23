# Performance Fix: P-bridge (F-settings-01)

## Summary
- **Packet ID**: `P-bridge`
- **Finding ID**: `F-settings-01`
- **Severity**: High
- **Description**: Removed the document-wide subtree `MutationObserver` on `document.documentElement` from `installSettingsRuntimeBridge()`. The bridge now relies on event-driven updates (`change`, `click`, `orca:terminal-settings`, and media query listeners) rather than scraping the full DOM tree and reading `localStorage` synchronously on every DOM mutation.

## Files Changed
- `ui/src/lib/settingsRuntimeBridge.ts`
- `ui/src/lib/settingsRuntime.test.ts`

## Production Change Details
- **File**: `ui/src/lib/settingsRuntimeBridge.ts`
- **Lines**: Removed the `MutationObserver` instantiation and observation block (`observer.observe(document.documentElement, { childList: true, subtree: true })` previously at lines 122–126). Added `_resetSettingsRuntimeBridgeForTest()` to support isolated test runs.

## Test Verification

### RED Phase
Added regression test in `ui/src/lib/settingsRuntime.test.ts` verifying that `installSettingsRuntimeBridge()` does NOT attach a subtree `MutationObserver` on `document.documentElement`.

**Command:**
```bash
bun run test src/lib/settingsRuntime.test.ts
```

**Output Tail (RED failure):**
```
 FAIL  src/lib/settingsRuntime.test.ts > settings runtime contracts > installSettingsRuntimeBridge > does not attach a document-wide subtree MutationObserver on documentElement
AssertionError: expected "observe" to not be called with arguments: [ <html …(5)>…(2)</html>, …(1) ]

Received: 

  1st observe call:

@@ -7,9 +7,10 @@
      style="color-scheme: dark;"
    >
      <head />
      <body />
    </html>,
-   ObjectContaining {
+   {
+     "childList": true,
      "subtree": true,
    },
  ]

Number of calls: 1

 ❯ src/lib/settingsRuntime.test.ts:78:30
     76|       const observeSpy = vi.spyOn(MutationObserver.prototype, "observe");
     77|       installSettingsRuntimeBridge();
     78|       expect(observeSpy).not.toHaveBeenCalledWith(
       |                              ^
     79|         document.documentElement,
     80|         expect.objectContaining({ subtree: true }),

 Test Files  1 failed (1)
      Tests  1 failed | 5 passed (6)
```

### GREEN Phase
Removed the `MutationObserver` block from `ui/src/lib/settingsRuntimeBridge.ts`.

**Command:**
```bash
bun run test src/lib/settingsRuntime.test.ts
```

**Output Tail (GREEN pass):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/lib/settingsRuntime.test.ts (6 tests) 23ms

 Test Files  1 passed (1)
      Tests  6 passed (6)
   Start at  23:41:07
   Duration  548ms (transform 53ms, setup 66ms, collect 69ms, tests 23ms, environment 174ms, prepare 35ms)
```

## Leftover Risk
- **None**: Settings UI controls synchronize appearance and terminal preferences via standard DOM events (`change`, `click`, custom events) and React component state. No other runtime component depends on document-wide DOM mutation polling for appearance settings.
