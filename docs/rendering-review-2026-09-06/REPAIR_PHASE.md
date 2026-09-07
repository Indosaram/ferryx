# Rendering repair phase: frontend lifetime and atlas design

## Verified input

The diagnosis workflow has completed. The lead read `atlas.md`, `surface.md`,
`frontend.md`, `review.md`, baseline reports and raw result evidence. The lead
also inspected the current `sendInput`/`performAttach` and row-cache code.
`git diff --check` passed. Source findings are not native-screen proof.

## Accepted repair queue

- D1: Complete dense visible atlas working sets without stale UVs or omissions.
- D2: Invalidate departed input-recovery ownership before attach/retry/publication.
- D3: Reconcile Wayland child scale and actual buffer/grid geometry.
- D4: Complete a final frame blocked only by historical atlas entries.
- D5: Rearm a directly rendered dropped frame after releasing host locks.
- D6: Destroy Windows child targets on their owning thread.
- D7: Restore truthful bounded atlas accounting; existing RED is captured.

D1 and D4 share one atlas owner; D7 remains a separately tracked criterion under
that same owner. D3/D5/D6 may overlap host/IPC files and must be serialized at
integration. Conditional font and native allocation hypotheses are not accepted
incident defects without their missing trigger evidence.

## Current phase topology

One new workflow, not an extension of the completed diagnosis graph:

1. `frontend-repair` (`deep`): own D2 end to end in the isolated worktree,
   including deterministic RED cases, minimal fix, GREEN, diagnostics, build
   and real-component browser evidence. Async session ownership requires this
   specialist route. Write only the named UI component/lifecycle/test paths.
2. `atlas-design` (`architect`): independent read-only decision on D1/D4/D7
   capacity, texture bindings, frame rebuild and truthful accounting. A planner
   is justified here because growth, pages and batching have materially different
   correctness/resource consequences. Write only `atlas-repair-plan.md`.
3. `phase-verify` (`deep`), dependent on both: independently verify D2's diff and
   artifacts and the atlas plan's explicit invariants/RED scenarios. Produce
   `repair-phase-verification.md`, with remaining native QA clearly pending.

No team: write scopes are disjoint and there is no simultaneous cross-owner edit.
The lead owns criterion registration, acceptance, source provenance and native QA.
The next phase is defined from this phase's checked results, not pre-scheduled.

## Isolated worktree

`/Users/indo/code/project/orca-lite-rendering-20260906`

Branch: `fix/rendering-review-20260906`.
Base: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`.
The worktree is locked. UI dependencies are installed from the frozen lockfile.
Ghostty is a local shared-object clone checked out at the pinned
`6a508fd5e34c7e222c052a6d00bb3891ff3feace`.

The UI repair paths match reviewed HEAD. The atlas paths do not: the foreign
main-tree atlas/row-cache patch is absent from this clean checkout. Atlas design
must state how the reviewed baseline is reproduced without changing or claiming
ownership of the main-tree patch. No atlas production edit or commit is authorized
by this design node. Integration into main requires the recorded approval step.

## D2 RED and GREEN contract

Use the real component and real native lifecycle module; mock only IPC/layout.
Use explicit deferred events registered before actions and React `act`; no sleeps
or polling assertions. A/B IDs and rectangles must differ.

```sh
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not reclaim the outgoing surface when input fails after tab replacement'
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not recover input when its owner becomes hidden'
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not retry input after its owner leaves during recovery'
```

Capture right-reason RED before production edits. Verify no obsolete recovery
attach/retry or stale result publication; outgoing A teardown must survive B's
presentation. Add queued-operation coverage if the minimal fix touches that seam.
Retain successful same-owner recovery and legitimate same-session reparenting.

After GREEN, run the focused three-file UI test command and `bun run --cwd ui build`.
Exercise the real component/lifecycle in a browser at the IPC boundary and capture
the action log and screenshot; label native compositor pixels unverified.
Do not automate or restart the existing desktop app from a worker.

## Evidence and cleanup

Worker reports live in `docs/rendering-review-2026-09-06/` in the main repo.
Detailed per-scenario artifacts live under
`.omo/evidence/ulw/rendering-review-20260906/`.
Every spawned browser, server and command gets an owned-resource cleanup receipt.
Keep application-source writes exclusively in the isolated worktree.

The atlas design is prose: source/plan review, not a phrase-pinning test.
The full aggregate remains incomplete until native macOS screenshots and required
platform-specific evidence are obtained, not merely until this workflow finishes.

## Independent Wayland lane added

D3 geometry is independent of the active UI write scope and the read-only atlas
design. The runtime refused amendment of the active workflow, without launching
the added node. D3 therefore runs as its own concurrent goal phase:
`dag_d86fe898-31b2-4e08-9568-7204078b4860`, with `wayland-repair` followed by
`wayland-verify` (both `deep`, platform/backend geometry).
The original UI/atlas workflow remains unchanged. D3 owns the pure geometry REDs
and the smallest coherent platform-specific repair end to end. D5 and D6 have
not started, so their overlapping host edits remain serialized.

Allowed production writes in the isolated worktree: `child_surface.rs`,
`composition.rs`, `surface_host.rs`, `platform/mod.rs`, `platform/linux.rs`,
`platform/wayland_child.rs`, and the Wayland/composition contract tests, only
where required for D3. No renderer, UI, vendor, manifest or other-platform
implementation edits. Any interface outside this boundary must be reported
before widening. The lead owns its commit after the UI worker's index operation
has finished; the Wayland worker must not stage or commit shared-index changes.

Capture both named tests RED before production changes:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract host_extent_matches_wayland_buffer_extent_at_fractional_scale -- --exact --nocapture
```

GREEN must exercise the real geometry path used by host buffer and grid
configuration, not only the currently unused helper extents. Check shared edges,
odd physical widths, 1.5/2.0 scales, and unchanged macOS/Windows/X11 semantics.
Real Wayland GUI evidence remains separately required; no macOS geometry test or
X11 run can close it. Save results in `wayland-repair.md` and D3 artifacts.
