# Bounded UI source receipts - 2026-09-13

Task st_01a0989e. All 11 assigned paths read fully; source coverage closed for
these bytes, not Windows correctness or runtime acceptance. Two additional
reachable shared-source defects below. No implementation, tests, builds,
runtime, desktop, SSH, historical harness execution, or Git mutation performed.
Branch/worktree permission remains pending. This report is the sole write.

## Method and boundary

Read root, ui, components and lib AGENTS; programming and review-work skills.
This is a source-only leaf review, not the skill's post-implementation runtime
orchestrator. No delegation tool is exposed. Read coverage.md, findings.md and
gap-packet-addendum.md fully; inspected the exact 11 inventory-reconciled.json
ledger entries including their prior scopes. The immutable census is not edited.
Used read for source inspection, LSP references for ownership/resume/device
symbols, then text cross-references: LSP returned only local declarations/local
calls and demonstrably omitted imports, so it was not treated as exhaustive.

Initial and pre-write git status/diff-stat showed 20 foreign tracked modified
paths, plus untracked evidence/docs/rescan/mobile fixtures. All remained read-only.
Observed HEAD ab08b94fdeda5039982fc8a37e8bc36885426667 differs from the older
packet baselines. Assigned files' post-read SHA256 values equal the census for
all 11; no assigned-path drift observed. Receipts bind the read ranges and hashes,
not a frozen checkout or future owner edits. Immediate dirty caller hashes:
App.tsx `4429fd8578adb22d9b98841f002290bfc9dd6c62d6c3f4c3c2dbf087668132ca`;
lib/tauri.ts `3ed91ce0fce2821906b886be799c7019f04a0b0043ff3cc76867f31c091e1b2c`;
remote/RemoteApp.tsx `dcbb6ab6a8d215b83d699cac47749bfe8d28637573bc1d61a71f5b3a3693a5a4`.
No moving-source chase or blanket approval of those callers' foreign changes.

## Exact receipts

Paths below are relative to `ui/src/`. Ranges are inclusive and cover every
line, including comments. No assigned implementation body was skipped as Mac-only:
these are shared TypeScript modules, not Rust cfg exclusions.

| Assigned path / full read | Current SHA256 | Previous scope -> full-file disposition and Windows branches |
|---|---|---|
| components/CommandPalette.tsx:1-143 | `71f87484c35500b2c820c4aaf68e1c73c2e6e9f46f57324b4a16743f5d457a59` | Prior 110-134 + hits 4/19. Desktop App:2633-2641 mounts it outside active remote-host mode. Shared filtering, Escape listener/cleanup, selection callbacks and empty state inspected. Windows false-isMac formatter at 19/123; basename splits both separators at 138. No new defect; formatter reflects existing WIN-UI-03, not a separate binding implementation. |
| components/Sidebar.tsx:1-1044 | `5f729fd280a9ef7df76cdd93f52902ad6b6ac337415e8ece076f899dafd6c9f7` | Prior 360-377. Full reads 1-550 and 551-1044. App:2476-2504 supplies real project/worktree state and actions. Windows omits Mac traffic-light spacer at 368; shared resize lifecycle, accordion/storage, project grouping, active ownership, native menus, SSH versus local actions, overlays and ordering all inspected. BOUNDED-UI-01: row context identity mismatch; ordering remains path-only and cannot distinguish same-path member rows. Storage failures deliberately retain in-session operation, not a new Windows defect. |
| components/WorkspaceHeader.tsx:1-70 | `e669e10c0531c847498706cdb90bb3f5fe62d07387e5589033170711ff87cfdb` | Prior 18-52. Shared exported header, false-isMac omits spacer; left-button/no-drag guards precede Tauri startDragging at 25-29. Production-source cross-reference found no caller outside this declaration (unlike Sidebar). Thus not established as a currently mounted Windows surface. No native caption/double-click defect inferred from an unused component. |
| components/WorktreeList.tsx:1-336 | `00535b5736eb9884a121bc5d97f3c5c4781653f7b2a1df641793c153d2349f6b` | Prior 43-48 label only. Windows label selects File Explorer; macOS Finder and default File Manager are display branches, actual reveal uses IPC. Shared local/SSH deletion/reveal gating, activity, clipboard, menu subscription cleanup and row identity inspected. BOUNDED-UI-01 at 249/331-335; no claim that asynchronous clipboard necessarily loses Chromium activation. |
| components/settings/ShortcutsSection.tsx:1-110 | `5b32e2c9f55511f141af803a7e9af870819283b48aa07649eef215b478929f4b` | Prior 65-92. SettingsDialog:70/187 supplies detected isMac. Shared filtering/group selection/rendering; Windows primary and alias labels both use shared formatter at 74/86. No local key interception or new policy defect. |
| components/settings/types.ts:1-31 | `d18567d3b97da6d718ec156515cb4a4eb8985fe5a4e1ec6ebd0e8e19d386356f` | Prior 29-31. Types only: SectionId, terminal preference callback contract and ShortcutsSection isMac boolean. No emitted platform branch. macosOptionAsAlt field alone is not a Windows failure; HIST-01/P09 owns the separate UI applicability issue. |
| lib/agentResume.ts:1-202 | `5b1451a65e40c9f3ca001849c13a2c5a3b631bfed31fac4b02e60b950005fbdc` | Prior 100-129. Shared provider registry, supported/capture lists, ID/path normalization, extraction/equality and argv construction inspected. Slash, drive and UNC transcript paths accepted within size/control/option guards. No navigator/host-shell branch: result is argv data, not POSIX shell quoting. Affordance/reconnect consumers below retain structured startup metadata. Existing DS-03/04/06 remain backend boundaries, not fixed by this receipt. |
| lib/browserSettings.ts:1-372 | `56ef602ff43b59b880bfa3cd284faceab9c6a819152541faec216c5e11fd32c5` | Prior 86-100. Named profiles enabled for Windows/non-Mac and absent navigator; Mac/iOS restrict to default/private. Fully inspected profile/default normalization, URL/search/home/zoom handling, persistence/history override, events/hooks and labels. Windows capability is backed by non-Mac data-directory implementation, not just an optimistic label. No additional Windows-specific reachable defect established; storage/engine/permission runtime not certified. |
| lib/projectGrouping.ts:1-102 | `6fff3230075fca9b6b42af73c1d31a23215dcc7e849c9d9ddbec79339e0bb812` | Prior claimed 1-104 (actual EOF 102). Drive paths rejected as remote URLs; shared scp/URL remote identity, same-host common-directory comparison, drive/backslash UNC/verbatim normalization, merging and local-primary choice read fully. Folder extraction accepts both separators. Grouping produces workspace-qualified secondary rows consumed by Sidebar, establishing BOUNDED-UI-01 reachability; no filesystem-jail guarantee claimed for these display grouping helpers. |
| lib/worktreeOwnership.ts:1-72 | `0f1fbe56e62e2a467e53d889b5dd5a59845f9bd9b2bd2e3cadd1f49ef8d7c6b1` | Prior claimed 1-74 (actual EOF 72). Shared explicit workspace identity first; unique deepest SSH path match before local managed identity/exact-root override/deepest local/fallback. Drive/UNC case and separator folding, component-boundary prefix checked. No native path canonicalization or host-auth claim. App:1451 and Sidebar:783/795 consume it; ambiguous remote ownership is not arbitrarily chosen by this helper. No new proved defect. |
| remote/deviceIdentity.ts:1-52 | `e225df57020c299be3dc4660aa628837451a5a0eafb0ca5eb8e3ab7d2d502abc` | Prior 1-51. Shared UA parser: Windows/Win32/Win64 OS branch, Edge before Chrome, mobile/Mac/Linux/unknown fallbacks. BOUNDED-UI-02: Opera check follows Chrome/Safari, making normal Chromium Opera identification wrong. This is editable pairing display metadata, not authentication or installation identity. |

## Immediate consumer/ancestor evidence

- main.tsx:1-68 routes Tauri desktop to App and web to RemoteApp. App ranges
  1430-1494, 2470-2504, 2628-2647 were read, not the entire moving App.
- shortcuts.ts:345-494,535-548: false-isMac renders Ctrl for mod/control,
  maps matching Ctrl on Windows, rejects IME/AltGraph; detection uses navigator
  then optional Darwin process fallback. SettingsDialog:65-74,180-191 confirms
  shared detection, not a hardcoded Mac prop.
- WorktreeList:138 -> tauri.ts:159-160 -> ipc/project.rs:319-374:
  cmd_path_reveal uses run_blocking, existence check, macOS-only open -R,
  **actual Windows cfg** explorer with one `/select,{path}` argument, and
  non-Mac/non-Windows xdg-open. This is not the browser cmd/start boundary B02.
- browserTauri.ts:126-140 normalizes profile then invokes cmd_browser_create.
  ipc/browser.rs:855-919,1000-1029 prove the ancestor command, Mac-only named
  rejection, **not(macos)** named directory creation and builder.data_directory.
  Windows reaches that fallback. No Mac-only implementation body was substituted
  for a Windows receipt; no whole browser backend audit claimed.
- agentResumeAffordance.ts:95-189 -> buildResumeArgv at 146 checks capability,
  authoritative metadata, missing reference, live/duplicate/in-flight conditions.
  TerminalPane.tsx:85-119 consumes the affordance for exited-session UI;
  agentReconnect.ts:30-123 sends startup kind=agentResume, providerSession,
  worktree and cwd rather than interpolating this argv into a shell string.
- PairingPage.tsx:1-103 uses suggestion as editable default and empty-input
  fallback, then POSTs code/deviceName/installationId and consumes token.
  RemoteApp.tsx:414-448 also POSTs suggestion for URL pairing. These are shipped
  callers, not historical docs snippets.

## New source findings (proposals, NOT executed)

### BOUNDED-UI-01 - qualified worktree rows lose sortable identity (medium)

App -> Sidebar's groupProjects/groupWorktreesByProject -> WorktreeList is
reachable on Windows without a platform gate. Grouping may combine a local
project and SSH checkout by Git remote. Sidebar:789-791/813-820/883-901 retains
or adds member workspaceId to rows. Sidebar:452 supplies SortableContext IDs via
`worktreeSortableId(groupId, path, row.workspaceId)`, but WorktreeList:248-251
registers useSortable using only `(groupId, path)`. For a qualified row these
strings differ deterministically. For two member rows with the same path,
registrations also collide even though the parent context IDs differ.

Dependency source read: ui/node_modules/@dnd-kit/sortable/dist/sortable.esm.js
295-339,447-531. Context indexes active/over IDs with items.indexOf at 310-311;
useSortable computes its index at 462 and requires valid indexes for displacement
at 507-512. Qualified registration is absent from items, giving -1 and disabling
sorting displacement. This proves a contract failure, not total inability to
invoke Sidebar's drop callback. Sidebar:317-326 also indexes only paths and
pruneWorktreeOrder:929-939 deduplicates paths, so same-path member reordering
cannot preserve selected row identity. One identity defect, not separate packets
for each manifestation. Small correction: consistently carry row workspace
identity through context, registration, drag data and persisted ordering while
retaining legacy stored-path reading. No project-group redesign.

Exact proposed regression invocation (new test file, not created):
`bun run --cwd ui test src/components/Sidebar.sortableIdentity.test.tsx`.
Keep real Sidebar/grouping/WorktreeList and real DndContext/SortableContext;
observe useSortable data without replacing it with an unconditional success
mock. Assert registered IDs equal the context's IDs, every index is nonnegative,
and two qualified same-path rows have distinct IDs. Drive keyboard drag/drop
with deterministic geometry; assert exact member order and persisted reload
order. Subscribe to drop/state signals before triggering, no sleeps/polling.
Real-surface condition: owned current Windows debug desktop with grouped
checkouts, including two SSH members on distinct hosts sharing the same absolute
path; drag and keyboard-reorder the intended row, observe sibling displacement,
correct member selection and retained order after reopening. No SSH execution
or desktop manipulation was performed here.

### BOUNDED-UI-02 - Opera is suggested as Chrome (low, display only)

main web route -> PairingPage:12/30 (or RemoteApp:435) -> suggestDeviceName.
deviceIdentity.ts:32 checks Chrome before the Opera OPR branch at 38. A Windows
Opera UA containing both `Chrome/126.0.0.0 ... Safari/537.36 OPR/112.0.0.0`
necessarily selects Chrome; the later Opera branch cannot correct it. The
wrong name appears prefilled and is submitted when unedited. No token/security
effect asserted. Move specific Opera recognition before generic Chromium
recognition; do not introduce UA-based authorization or a new detection service.

Exact proposed invocation (new pure test file, not created):
`bun run --cwd ui test src/remote/deviceIdentity.opera.test.ts`.
Feed Windows UA fixtures containing OPR+Chrome+Safari and retain Edge/Chrome
controls; assert parsed browser identity in the suggestion, not explanatory
prose. Real-surface condition: current Windows Opera opens owned pairing page,
shows Opera identity and submits it unedited; user-edited name remains intact.
No actual installed Opera version or network success is asserted.

## Retained aliases, refutations and limits

- WIN-UI-03/P04 remains the duplicate workspace/tab Ctrl+digits finding;
  NATIVEUI-GAP-03 aliases its validator. Palette/settings are consumers, not
  new defects. WIN-UI-01 and native-input-09/WIN-UI-04 remain policy/capability
  constraints: preserve Ctrl+W close, Ctrl+V paste and Ctrl+click links.
- WIN-UI-05 activation expiry is unproved; no clipboard rewrite follows from
  these menus. WIN-UI-07/PKG-04 caption-overlap mechanism remains refuted;
  WIN-UI-10 contextMenuGuard reachability refutation and WIN-UI-11 double-click
  uncertainty remain. WIN-UI-12 spacing is not a defect.
- DS-03, DS-04 and DS-06 provider launch/detection boundaries remain open;
  argv validation in agentResume is not Windows executable resolution proof.
  HIST-01/P09 is Option-as-Alt UI applicability, not a type declaration defect.
- OTHERUI-GAP-01/P28 remains pairing **test synchronization** debt, not a
  pairing product failure. The new Opera classification finding is independent.
  Existing remote RC identifiers and GAP-UI-06/WIN-UI-09/historical
  L5-UI-FRONTEND-8 wheel aliases are unchanged and outside this receipt's fixes.
- Historical documentation fixtures, old image mocks and installed-binary
  receipts were not executed or repaired and are not shipped behavior evidence.
  No Windows-specific test absence was promoted into a product defect.
- Proposed regression files do not yet exist; no RED/GREEN, unit pass, runtime
  success, package proof or acceptance criterion is claimed. Lead must independently
  verify source chains and allocate ownership before implementation. Only these
  11 full-file receipts and stated immediate ranges are covered; native input,
  browser engine isolation, ACLs, clipboard permission and real DnD await their
  actual surfaces. Report remains uncommitted in a shared, concurrently dirty tree.
