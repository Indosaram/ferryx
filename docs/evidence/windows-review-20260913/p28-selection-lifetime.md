# P28 selection request lifetime: RC-01 / RC-02

2026-09-13; child st_01a0995f, parent 01a0983f-c995-753d-afa9-593f6d118788.
Local macOS arm64 mounted-component evidence only. Actual Windows response-gated
browser proof remains pending; this does not close parent C002 or P28 runtime acceptance.

## Scope and causal chain

Read root/UI AGENTS, programming/TypeScript and debugging skills, the RC-01/02
caller report, and current C002 registration (2026-09-13T06:06:03.895Z increment).
Both acquired source files were clean at survey and before writing. Foreign
changes, including RemoteTerminal and App tests, were read-only and preserved.
Only RemoteApp.tsx, RemoteUI.test.tsx and this report are delivered.

Real RemoteWorkspaceMirror tab/picker controls call RemoteHostConnection's
selectContext. Its pending option disables selection controls and overrides the
rendered terminal optimistically. Previously the 6000ms timer started after POST
headers, so missing headers stranded controls. An unrelated authoritative event
clears A and refreshes the authoritative model, permitting B, but A's unguarded
POST continuation could clear B, accept it, or replace its deadline with A's timer.

Minimal production change: start the existing timer before fetch, give each attempt
a fresh option identity (picker options can be reused), ignore success and failure
continuations that no longer own pending state, and invalidate ownership on unmount.
No new transport protocol, cancellation contract, preferences, wheel, native, or
cross-worktree target changes. A timed-out POST may still settle at the transport
boundary, but it no longer owns UI state. RC-03 remains separately required.

## Deterministic regression surface

The real RemoteHostConnection, workspace normalization, selection controls and event
handler are mounted. Existing RemoteTerminal presentation stub is retained; fetch
and event WebSocket are controlled at their existing boundaries. No socket, daemon,
desktop, browser, SSH, release, branch, worktree or commit operation was performed.

Added names:
- `releases selection when the request never settles`
- `ignores an obsolete selection response while a newer selection is pending`
  (six rows: success, HTTP failure, network rejection; B accepted or awaiting headers).

All POST, initial read, authoritative refresh, confirmation read and socket-created
signals are registered before triggering actions. Mount commits synchronously before
awaiting effect-driven signals inside async act. Vitest's 5000ms test timeout bounds
signal awaits. No new sleeps, waitFor, findBy, polling or readiness-clock advances.
Fake clock is used only for the request deadline: pending at 5999ms, released at
6000ms; stale A settles at B+2000ms. Delayed B headers cannot extend its deadline.
Tests assert optimistic terminal rollback to authoritative state, selected-tab
identity, enabled controls, no false confirmation read, and zero timers on unmount.

The six B cases start B after an explicit different authoritative selection and
refresh. Accepted B survives A success/failure through its original deadline;
unaccepted B is not accepted by A's success (matching B event must not fetch until
B's own headers arrive). Existing confirmation tests remain intact.

## Receipts and diagnostics

Exact RED and GREEN invocation:
`bun run --cwd ui test src/remote/RemoteUI.test.tsx`

- Intended RED: exit 1, seven intended regression failures, all 43 existing tests pass.
- Identical full-target GREEN: exit 0, 50/50 pass; first run after product fix.
- No pre-existing target test failures observed in the valid RED or GREEN run.
- An earlier harness-development run exited 1 (48 failures/2 passes, 83.174s):
  mounting and awaiting effect signals in the same async act deadlocked, leaving
  pending act scopes and cascading failures. This was corrected before product
  changes; it is not claimed as product RED or as a pre-existing failure.
- RemoteApp LSP all severities: `No diagnostics found` after fix.
- RemoteUI LSP all severities before the mount-order correction: `No diagnostics found`.
  Two post-fix requests returned:
  `Timed out waiting for fresh diagnostics for /Users/indo/code/project/orca-lite/ui/src/remote/RemoteUI.test.tsx within 3000ms.`
- `bun run --cwd ui build`: exit 2, TypeScript reports only the foreign
  App.test.tsx:570 TS2322 shown below. No scoped compiler diagnostics; Vite did not run.
  This session did not edit or repair that foreign test.
- Scoped `git diff --check`: exit 0, no output.

SHA256 provenance:
- Original RemoteApp: `dcbb6ab6a8d215b83d699cac47749bfe8d28637573bc1d61a71f5b3a3693a5a4`
- Fixed RemoteApp: `b53c9370d93bf9ef6d7e38edfc28e23f47bc8c908f502bdfd969180686b17cee`
- Identical intended RED/GREEN RemoteUI tests: `6ca111c6752449bb80b02f11bccdcbbdbba8a725c8294248a482b05e8d5a2f2c`

## Cleanup and remaining acceptance

Lead read the complete source/test diff and both complete valid command
outputs. Independent preregistered command:
`bun run --cwd ui test src/remote/RemoteUI.test.tsx --config vitest.selection-mutation.config.ts`.
mon_T5PB8XEZ4RYX5X9G / bash_41 exited 1 at 15:19:30, 1.36s,
seven intended failures / 43 passes. Exact-once
SELECTION_MUTATION_APPLIED:original-response-lifetime was observed.
Loader restored post-response timeout placement and unguarded response
continuations only; all 50 same assertions executed. Full output read:
never-settling and stale-success deadlines remained disabled, stale failures
released B prematurely, and stale success falsely accepted B.
Temporary config error diagnostics were clean before run, then it was
deleted after exit and checked absent. Fixed RemoteApp SHA above unchanged.
P05 browser owner confirmed the transient App fixture type error repaired;
lead combined build is still required after that writer returns.

Runs used owned background Python monitors which waited for child process exit and
recorded exact exit codes. All four monitors finished; no monitor or test process
remains from these runs. Temporary logs were copied into this report before removal.
Tests unmount and restore timers/globals; no debug instrumentation or temporary
configuration was added. The working changes are uncommitted in a shared moving tree.

Real Windows browser response gating is not executed: hold A's response with the
event stream connected, verify six-second release without reload, invalidate A by
an authoritative selection, start B, then release A success/failure and prove B's
original deadline/confirmation and sibling input isolation. Exact current browser
bundle and gateway identities are still required. Build completion is separately
blocked by the foreign TypeScript error; mounted tests are not browser acceptance.

## Exact command outputs

Outputs below omit ANSI color escapes only. The unsuccessful harness-development
run is summarized above rather than misclassified as a product regression receipt.

### Intended RED

```text
$ vitest run --maxWorkers=1 src/remote/RemoteUI.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/remote/RemoteUI.test.tsx (50 tests | 7 failed) 559ms
   × selection request lifetime > releases selection when the request never settles 56ms
     → expect(element).toBeEnabled()

Received element is not enabled:
  <button
  aria-label="dev"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  disabled=""
  role="tab"
/>
   × selection request lifetime > ignores an obsolete selection response while a newer selection is pending 23ms
     → expect(element).toBeEnabled()

Received element is not enabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  disabled=""
  role="tab"
/>
   × selection request lifetime > ignores an obsolete selection response while a newer selection is pending 16ms
     → expect(element).toBeDisabled()

Received element is not disabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  role="tab"
/>
   × selection request lifetime > ignores an obsolete selection response while a newer selection is pending 14ms
     → expect(element).toBeDisabled()

Received element is not disabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  role="tab"
/>
   × selection request lifetime > ignores an obsolete selection response while a newer selection is pending 16ms
     → expected 3 to be 2 // Object.is equality
   × selection request lifetime > ignores an obsolete selection response while a newer selection is pending 12ms
     → expect(element).toBeDisabled()

Received element is not disabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  role="tab"
/>
   × selection request lifetime > ignores an obsolete selection response while a newer selection is pending 11ms
     → expect(element).toBeDisabled()

Received element is not disabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  role="tab"
/>
   ✓ Remote UI Components > creates a terminal in an empty selected worktree and waits for desktop publication 11ms
   ✓ Remote UI Components > does not confirm creation from an unchanged terminal and reports rejected requests 10ms
   ✓ Remote UI Components > shrinks the mobile shell on visual viewport resize without a screen-height minimum 15ms
   ✓ Remote UI Components > keeps long workspace identifiers within the vertical worktree picker 18ms
   ✓ Remote UI Components > PairingPage renders the Ferryx Desktop PIN flow 4ms
   ✓ Remote UI Components > renders the native active-only state as one mirrored terminal without exposing paths 8ms
   ✓ Remote UI Components > renders the current safe server workspace contract without local paths 11ms
   ✓ Remote UI Components > mirrors only the server-declared terminal and safely confirms a context selection 19ms
   ✓ Remote UI Components > waits for the desktop active-selection event when the first confirmation read is stale 17ms
   ✓ Remote UI Components > recovers from a desktop that never confirms so the picker stays usable 19ms
   ✓ Remote UI Components > refreshes the mirrored terminal after an unsolicited desktop focus change 8ms
   ✓ Remote UI Components > keeps the newest desktop focus when focus events arrive during a refresh 7ms
   ✓ Remote UI Components > clears the mirrored terminal when desktop no longer focuses a terminal 7ms
   ✓ Remote UI Components > shows no focused terminal when only undeclared background sessions are present 3ms
   ✓ Remote UI Components > MobileKeyDock dispatches primary key actions and latches modifiers 3ms
   ✓ Remote UI Components > retains legacy authentication without rendering old Orca branding 4ms
   ✓ Remote UI Components > sets browser document.title to active tab or terminal title on initial authenticated load 4ms
   ✓ Remote UI Components > does not treat a worktree row id as a terminal tab id 0ms
   ✓ Remote UI Components > updates document.title on unsolicited desktop focus and active tab change and falls back to Ferryx 9ms
   ✓ Remote UI Components > resets document.title to Ferryx when unpaired or disconnected 2ms
   ✓ Remote UI Components > allows sequential traversal using previous and next terminal tab controls and ordinal indicator 21ms
   ✓ Remote UI Components > renders safe tab items under active worktree and dispatches tab switch request 10ms
   ✓ Remote UI Components > cycles published terminal tabs only after Desktop confirms the selected tab 20ms
   ✓ Remote UI Components > retains authorization across normal page reload when server returns transient error 3ms
   ✓ Remote UI Components > clears authorization and returns to PairingPage when token is revoked (401) 5ms
   ✓ Remote UI Components > normalizeRemoteWorkspaceState parses activityState, agentType, and attention and drops invalid values defensively 0ms
   ✓ Remote UI Components > tab strip renders state indicators for waiting and working tabs discoverable by accessible name 7ms
   ✓ Remote UI Components > lists a published terminal pane even when the desktop has nothing focused 6ms
   ✓ Remote UI Components > selects a pane from another worktree using that pane's own worktree 8ms
   ✓ Remote UI Components > tab strip renders brand logo image for supported agentType and fallback terminal icon for unknown/missing agentType 9ms
   ✓ Remote UI Components > context selector exposes worktree attention in its accessible name 11ms
   ✓ Remote UI Components > immediately remounts RemoteTerminal to session when selecting a tab with sessionId before confirmation 10ms
   ✓ Remote UI Components > remounts the optimistic terminal after confirmation when its socket closed before opening 10ms
   ✓ Remote UI Components > remounts on confirmation when the optimistic socket opened and then closed 10ms
   ✓ Remote UI Components > remounts when the optimistic socket reports its failed handshake after confirmation 13ms
   ✓ Remote UI Components > does not retry again when the confirmed replacement socket also closes 13ms
   ✓ Remote UI Components > does not perform immediate post-POST workspace/state refresh on selection 8ms
   ✓ Remote UI Components > clears optimistic session override and reverts when confirmation times out 7ms
   ✓ Remote UI Components > clears optimistic session override when selection request fails 9ms
   ✓ Remote UI Components > clears optimistic session override when user disconnects 13ms
   ✓ Remote UI Components > requires confirmation before Disconnect removes the pairing 16ms
   ✓ Remote UI Components > Cancel keeps the remote session paired 12ms
   ✓ Remote UI Components > clears optimistic session override when a different authoritative state arrives 11ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 7 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > releases selection when the request never settles
Error: expect(element).toBeEnabled()

Received element is not enabled:
  <button
  aria-label="dev"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  disabled=""
  role="tab"
/>
 ❯ src/remote/RemoteUI.test.tsx:316:22
    314|       expect(target).toBeDisabled();
    315|       await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    316|       expect(target).toBeEnabled();
       |                      ^
    317|       expect(screen.getByRole("button", { name: "Next terminal tab" })…
    318|       expect(screen.getByTestId("remote-terminal")).toHaveAttribute("d…

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/7]⎯

 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > ignores an obsolete selection response while a newer selection is pending
Error: expect(element).toBeEnabled()

Received element is not enabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  disabled=""
  role="tab"
/>
 ❯ src/remote/RemoteUI.test.tsx:387:23
    385|       expect(targetB).toBeDisabled();
    386|       await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    387|       expect(targetB).toBeEnabled();
       |                       ^
    388|       expect(screen.getByTestId("remote-terminal")).toHaveAttribute("d…
    389|       host.unmount();

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/7]⎯

 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > ignores an obsolete selection response while a newer selection is pending
 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > ignores an obsolete selection response while a newer selection is pending
 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > ignores an obsolete selection response while a newer selection is pending
 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > ignores an obsolete selection response while a newer selection is pending
Error: expect(element).toBeDisabled()

Received element is not disabled:
  <button
  aria-label="editor"
  aria-selected="false"
  class="flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 text-muted-foreground hover:bg-accent/50 hover:text-foreground"
  role="tab"
/>
 ❯ src/remote/RemoteUI.test.tsx:367:23
    365|         await host.responseA.promise;
    366|       });
    367|       expect(targetB).toBeDisabled();
       |                       ^
    368|       expect(screen.getByTestId("remote-terminal")).toHaveAttribute("d…
    369|       expect(screen.getByRole("tab", { name: "tests" })).toHaveAttribu…

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[3/7]⎯

 FAIL  src/remote/RemoteUI.test.tsx > selection request lifetime > ignores an obsolete selection response while a newer selection is pending
AssertionError: expected 3 to be 2 // Object.is equality

- Expected
+ Received

- 2
+ 3

 ❯ src/remote/RemoteUI.test.tsx:374:34
    372|         // event-triggered refresh; a stale snapshot still cannot conf…
    373|         await act(async () => { host.publish("editor"); });
    374|         expect(host.readCount()).toBe(2);
       |                                  ^
    375|         await act(async () => {
    376|           host.responseB.resolve(jsonResponse({ accepted: true }));

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[4/7]⎯


 Test Files  1 failed (1)
      Tests  7 failed | 43 passed (50)
   Start at  15:13:54
   Duration  1.83s (transform 141ms, setup 118ms, collect 563ms, tests 559ms, environment 261ms, prepare 30ms)

error: script "test" exited with code 1
Exit: 1
```

### GREEN

```text
$ vitest run --maxWorkers=1 src/remote/RemoteUI.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/remote/RemoteUI.test.tsx (50 tests) 552ms

 Test Files  1 passed (1)
      Tests  50 passed (50)
   Start at  15:15:00
   Duration  1.71s (transform 139ms, setup 103ms, collect 515ms, tests 552ms, environment 237ms, prepare 26ms)

Exit: 0
```

### Build (foreign failure)

```text
$ tsc && vite build
src/App.test.tsx(570,7): error TS2322: Type '{ id: string; kind: "browser"; label: string; browserId: string; url: string; }[] | { id: string; label: string; sessionId: string; }[]' is not assignable to type '({ id: string; kind: string; label: string; sessionId: string; } | { id: string; label: string; sessionId: string; kind?: undefined; })[]'.
  Type '{ id: string; kind: "browser"; label: string; browserId: string; url: string; }[]' is not assignable to type '({ id: string; kind: string; label: string; sessionId: string; } | { id: string; label: string; sessionId: string; kind?: undefined; })[]'.
    Type '{ id: string; kind: "browser"; label: string; browserId: string; url: string; }' is not assignable to type '{ id: string; kind: string; label: string; sessionId: string; } | { id: string; label: string; sessionId: string; kind?: undefined; }'.
      Property 'sessionId' is missing in type '{ id: string; kind: "browser"; label: string; browserId: string; url: string; }' but required in type '{ id: string; kind: string; label: string; sessionId: string; }'.
Exit: 2
```
