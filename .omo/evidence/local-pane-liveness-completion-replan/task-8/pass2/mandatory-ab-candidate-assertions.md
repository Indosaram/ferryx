# Linux candidate assertions requiring base attribution
These are observed failures, not yet classified pre-existing or candidate-caused. Matching-host base runs are subscribed after candidate exit.
## src/components/TerminalSplitView.paneHandleReach.test.tsx
```text
 FAIL  src/components/TerminalSplitView.paneHandleReach.test.tsx > pane handle reachability over a native terminal > overlays the handle only inside the narrow hotspot without shrinking the terminal
Error: expect(element).toHaveClass("h-3")

Expected the element to have class:
  h-3
Received:
  absolute inset-x-0 top-0 z-30 flex h-5 items-center justify-between overflow-visible border-b border-border/30 bg-background/85 px-2 text-[11px] text-muted-foreground transition-opacity duration-150 select-none cursor-grab touch-none active:cursor-grabbing pointer-events-none opacity-0
 ❯ src/components/TerminalSplitView.paneHandleReach.test.tsx:83:20
     81|     const leaf = screen.getByTestId("pane-leaf");
     82| 
     83|     expect(handle).toHaveClass("h-3");
       |                    ^
     84|     expect(handle).toHaveClass("opacity-0", "pointer-events-none");
     85|     expect(terminal.style.marginTop).toBe("");

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[256/1467]⎯

```
## src/lib/pairedDaemonRollout.test.ts
```text
 FAIL  src/lib/pairedDaemonRollout.test.ts > failed renegotiation revokes prior admission: {"capabilities":["directoryBrowseV1","futureV9"]}
AssertionError: promise resolved "{ apiVersion: 1, …(7) }" instead of rejecting

- Expected
+ Received

- Error {
-   "message": "rejected promise",

+ {
+   "accessScope": "machine",
+   "apiVersion": 1,
+   "capabilities": [
+     "directoryBrowseV1",
+     "futureV9",
+   ],
+   "daemonEpoch": "9",
+   "limits": {
+     "directoryEntries": 100,
+     "terminalSessions": 10,
+   },
+   "machineId": "machine-a",
+   "permission": "control",
+   "platform": "linux",
  }

 ❯ src/lib/pairedDaemonRollout.test.ts:35:40
     33|   await expect(f.adapter.directories()).resolves.toMatchObject({ path:…
     34|   f.peer(peer);
     35|   await expect(f.adapter.capabilities()).rejects.toMatchObject({ code …
       |                                        ^
     36|   const count = f.invoke.mock.calls.length;
     37|   await expect(f.adapter.directories()).rejects.toMatchObject({ code: …

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[272/1467]⎯

```
