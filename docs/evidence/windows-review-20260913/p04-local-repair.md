# P04 local repair - RED/GREEN complete

## Resumed repair receipt (supersedes the historical blocked receipt below)

The lead allocated ShortcutHints.tsx and ShortcutHints.test.tsx, registered the
combined command in C002 before edits, and explicitly prohibited unrelated
LOC-driven splits. All four code paths were clean on resumed ownership.

Complete owned files and delivered changes:

- ui/src/lib/shortcuts.ts: non-Mac override on workspace digits only; shared
  resolveBinding used by matching and labels. Ctrl selects tabs; Alt selects
  workspaces, consistent with the existing browser guest bridge. Mac unchanged.
- ui/src/lib/shortcuts.test.tsx: all nine digits exercise the actual hook with
  both families enabled, intended handler once, opposite never, default
  prevented, on both platforms. Updated non-Mac workspace label expectation.
- ui/src/components/ShortcutHints.tsx: resolve candidates before modifier
  filtering and construction of the winner-check event.
- ui/src/components/ShortcutHints.test.tsx: real component with both families,
  non-Mac Ctrl/Alt and Mac Control/Command hints, labels and exclusivity.
- docs/evidence/windows-review-20260913/p04-local-repair.md: this receipt.

Exact registered command, identical on RED and GREEN:

```sh
bun run --cwd ui test src/lib/shortcuts.test.tsx src/components/ShortcutHints.test.tsx
```

Vitest 3.2.7, maxWorkers=1, working directory
/Users/indo/code/project/orca-lite/ui, macOS arm64, 2026-09-13.
The shell captured `$?` immediately and printed the receipt handles below;
these identify session output, not external log-file paths.

RED handle `P04_RED_EXIT=1`: start 13:08:33, duration 1.38s, product unchanged.

```text
src/lib/shortcuts.test.tsx (72 tests | 10 failed)
src/components/ShortcutHints.test.tsx (18 tests | 1 failed)
Test Files  2 failed (2)
Tests       11 failed | 79 passed (90)
error: script "test" exited with code 1
P04_RED_EXIT=1
```

Intended assertion at shortcuts.test.tsx:82, for each digit 1..9:
`expect(workspace ? selectWorkspace : tab).toHaveBeenCalledOnce()` failed with
`expected "spy" to be called once, but got 0 times`. Each case passed non-Mac
Ctrl tab dispatch then failed non-Mac Alt workspace dispatch. New Mac loop
iterations were reached on GREEN; existing Mac dispatch tests passed on RED.

Independent real-hint assertion at ShortcutHints.test.tsx:70:
`expect(hints().find((hint) => hint.dataset.shortcutHint === action)?.textContent).toBe(label)`
failed with `expected undefined to be 'Alt+2'`. Non-Mac Control and both Mac
hint controls passed. Registry label assertion at shortcuts.test.tsx:155 also
failed: `expected 'Ctrl+1' to be 'Alt+1'`.

GREEN handle `P04_GREEN_EXIT=0`: same assertions, no intervening test changes.

```text
src/lib/shortcuts.test.tsx (72 tests) 134ms
src/components/ShortcutHints.test.tsx (18 tests) 141ms
Test Files  2 passed (2)
Tests       90 passed (90)
Start at    13:09:37
Duration    1.18s
P04_GREEN_EXIT=0
```

One RED run and one GREEN run, no retries or skipped tests. Synchronous hook
dispatch; hints advance the existing intentional 300ms delay using fake time,
not sleep/polling. Hook unmount is in finally; hints retain existing fixture
cleanup and timer restoration.

LSP on all four changed files before GREEN: both tests clean; shortcuts.ts
has only existing deprecation hints for keyCode (429/469) and platform (551);
ShortcutHints.tsx has only existing keyCode deprecation hint (122). No errors
or warnings. `git diff --check` passed. Product diff inspected: only workspace
digit definitions and shared resolution/caller adoption changed. Four-file
diff: 65 insertions, 14 deletions. Build deferred to lead as instructed.

Pure LOC: shortcuts.ts 518, its test 442, ShortcutHints.tsx 179, its test 142.
Existing long modules retained under explicit lead decision. Architectural
self-review: registry/router and hint rendering retain their responsibilities;
helper has three real consumers; no new untrusted boundary, tagged variant,
escape hatch, defensive check, parameter bloat, negative flag, logging or
destructive-operation re-query. Regression distinguishes missing routing and
missing hints before repair. All existing tests retained and passing.

Non-digit bindings, Ctrl+W policy, paste/link code and IME/AltGr guards remain
unchanged. Existing Mac, IME, AltGr and other shortcut tests passed. Native
paste/link behavior was not exercised. Foreign changes were preserved.
No branches/worktrees/commits/pushes, GUI, daemon, SSH, installation, release
or external fixtures; test processes exited and no runtime cleanup remains.

Local repair is complete, not native Windows acceptance. Lead still owes
combined batch verification and actual Windows `bun tauri dev` verification:
physical Ctrl digits select intended tabs, Alt digits intended workspaces,
held hints/labels agree, main webview and browser guest agree. Preserve and
exercise Ctrl+W close, Ctrl+V paste, Ctrl+click links, IME/AltGr, record actual
selection and owned-process cleanup. Parent loop criteria remain pending.

## Historical blocked receipt (before expanded ownership)

Date: 2026-09-13. Task: st_01a098ef. Host: macOS arm64.

## Outcome

No source or test changes were made. No RED/GREEN command was run. The local
repair is **not complete**: caller ownership must be resolved before changing
the platform-specific registry contract. This is not Windows runtime evidence.

## Ownership and inspection receipt

Before ownership, `git status --short --untracked-files=all --` for the three
assigned paths returned no entries; their scoped `git diff` was empty, and
the report path did not exist. Foreign modifications were present elsewhere
and were not edited, restored, staged, or removed.

Complete owned-file disposition:

- `ui/src/lib/shortcuts.ts`: read completely, unchanged.
- `ui/src/lib/shortcuts.test.tsx`: read completely, unchanged.
- `docs/evidence/windows-review-20260913/p04-local-repair.md`: this new report.

Read the original loop brief and repair-packets.md, including P04's policy
constraints. Traced the registry, matcher, capture-phase dispatcher, App's
simultaneously enabled tab/workspace handlers and their index-selection
callbacks, labels, modifier-held shortcut hints, and browser guest bridge.

## Confirmed mechanism and scope conflict

1. `shortcuts.ts` defines tab digits with `control: true` and workspace digits
   with `mod: true`. `matchesBinding(..., false)` resolves both to Ctrl.
   `useShortcuts` returns after the first enabled matching action; tab actions
   precede workspace actions. App registers both sets together.
2. `src-tauri/src/browser/guest.rs:140-152` already implements Cmd+digits for
   Mac workspaces, Alt+digits for non-Mac workspaces, and Ctrl+digits for tabs.
3. `ui/src/components/ShortcutHints.tsx:90-106` independently interprets
   `binding.mod`, `control`, `alt`, and `shift` to filter held modifiers and
   synthesize the candidate event before calling `matchesBinding`. Changing
   only matching and labels, or adding platform overrides that this caller
   does not resolve, leaves Alt-held workspace hints missing or incorrect.
   A platform-aware binding contract therefore needs coordinated changes in
   this caller, outside the assigned write scope. Hardwiring registry values
   to the host at module initialization would also undermine the existing
   explicit `isMac` contract used by matching, labels, hooks, and tests.

Required lead decision: allocate `ui/src/components/ShortcutHints.tsx` and its
test to this repair or coordinate their owner to consume the same resolved
binding contract. Register the additional test invocation before execution.
The existing hint test at `ShortcutHints.test.tsx:132` verifies that Ctrl-held
hints do not advertise shadowed workspaces; it does not cover Alt-held
workspace hints. No extra fixture or invocation was introduced here.

An additional supplied-skill constraint needs reconciliation: the requested
pure-LOC measurement reports 511 lines for shortcuts.ts and 418 for its test,
both already above the skill's 250-line ceiling. The source includes a large
registry data table, but the test is not a pure data table. A required split
would need additional file ownership; no unrelated refactor was attempted.

## RED/GREEN receipt

Registered command, not executed:

```sh
bun run --cwd ui test src/lib/shortcuts.test.tsx
```

- RED: not captured; no test registered in the source and no assertion failure
  claimed. Counts and exit status: unavailable, because execution did not occur.
- GREEN: not captured; counts and exit status unavailable.
- Intended behavioral regression: enable tab and workspace handlers together;
  dispatch each Ctrl+1..9 and Alt+1..9 through the real hook with `isMac=false`;
  assert only the intended handler runs exactly once. Retain Mac, non-digit,
  IME and AltGr coverage. The RED must demonstrate the workspace handler was
  not invoked before repair, not merely compare duplicate binding strings.
- LSP/build/runtime: not run; no code changed and scope resolution stopped
  implementation before RED. No validator pass is claimed.

## Cleanup and remaining Windows obligations

Only this report was created. No branches, worktrees, commits, pushes, GUI,
daemon, SSH, installations, release operations, or temporary fixtures were
created or invoked. No child runtime resources require cleanup.

After local RED/GREEN and coordinated hint coverage, the lead still owes real
Windows debug-GUI verification from the authorized isolated source tree using
`bun tauri dev`: Ctrl+digits selects the intended tabs; Alt+digits selects the
intended workspaces; labels/hints agree; browser guest and main webview routing
agree. Preserve Ctrl+W close, Ctrl+V paste, Ctrl+click links, IME and AltGr.
Capture actual target selection and owned-process cleanup; this report closes
neither Windows runtime acceptance nor the parent loop criteria.
