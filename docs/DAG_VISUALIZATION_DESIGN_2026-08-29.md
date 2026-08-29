# Ferryx DAG Run Visualization — Design Proposal

**Date:** 2026-08-29
**Status:** Proposal (awaiting approval)

## Goal

Visualize omo (`mass-ulw` / native `dag` tool) DAG runs inside Ferryx as a live, pretty
graph: wave-by-wave node states, critical path, bottlenecks — the same data the omo TUI
shows in its `omo-dag` widget and `/dag` command, rendered as a first-class Ferryx panel.

## Data source: omo's on-disk DAG journal (recommended)

omo journals every DAG run under the agent cwd:

```
<project-cwd>/.omo/senpi-task/dag/
├── runs/<runId>.json        # authoritative checkpoint: definition + node states + amendHistory
├── events/<runId>.jsonl     # append-only, seq-strictly-increasing, fsynced
├── results/<runId>/<nodeId>.txt
├── keys/<hash(key)>.json    # idempotency-key index
└── locks/                   # flock files, not data
```

Facts verified from the omo-task bundle (`omo-ai/plugin/extensions/omo-task.js`):

- Checkpoints are rewritten atomically (`write` + rename-style via `ox()` helper) on every
  state transition; events JSONL is strictly seq-ordered and fsynced — a tail-and-replay
  consumer is safe.
- Bounded data: ≤ 64 nodes/run, ≤ 16 runs/session, retention 7 days. Full-snapshot
  broadcast per change is cheap (a few KB).
- Terminal run states: `completed | failed | cancelled | paused`; node states:
  `running ▶ / pending ◌ / scheduled ◔ / blocked ⊟ / completed ✓ / failed ✗ /
   skipped ⊘ / cancelled ⊘ / paused ⏸`.
- `waves`, `criticalPath`, `bottlenecks` are **derived** data (omo computes them from
  `dependsOn` for `/dag` output) — Ferryx can recompute them deterministically from the
  checkpoint's `nodes[].dependsOn` (≤ 64 nodes, trivial layered assignment). No need to
  parse omo's derivation.

Why not the alternatives:

- **Extending `ferryx-agent-state.ts`** (the existing `~/.omo/agent/extensions/`
  lifecycle extension) to also report DAG state: possible, but omo does not expose DAG
  runtime events to extensions, so the extension would poll the *same journal files* and
  add a hop (extension → socket → daemon) plus coupling to omo session IDs. The Rust-side
  watcher reads the same truth with fewer moving parts.
- **Agent-rendered Mermaid in the terminal:** not "pretty UI", no interactivity, tied to
  scrollback. Rejected as the primary path; fine as an export format.

### Risk: journal format is omo-internal

`.omo/senpi-task` is not a public contract. Mitigations: tolerant parsing (ignore unknown
fields), skip-and-log on parse failure (degrade to "run unreadable" state, never crash),
and a smoke test pinned against the current omo version.

## Rust side (`src-tauri/src/dag/`)

1. `journal.rs` — parse `runs/*.json` + replay `events/<runId>.jsonl` (skip seq ≤
   last-seen; this gives crash-free resume). Derive waves (Kahn layering), critical path
   (longest path by elapsed on completed nodes / topological otherwise), bottlenecks
   (out-degree of `dependsOn` edges among unfinished nodes).
2. `watcher.rs` — per open project cwd: initial scan via `run_blocking` (project rule: no
   sync disk IO on async threads), then a `notify` crate watcher (new dev-dep) on
   `dag/runs` + `dag/events` with 250 ms debounce; fall back to 1 s polling if watch
   registration fails (e.g. network mounts).
3. Events — emit `dag://run-updated` Tauri events carrying the full merged snapshot
   (name, runId, status, nodes, waves, criticalPath, bottlenecks, counts, amendHistory).
   Initial state on subscribe via a `dag_list_runs` / `dag_get_run` IPC command pair
   (structured `{ code, message, details }` errors — no regex matching).
4. Remote — mirror the `SubscribeRemoteEvents` bridge pattern (`daemon/protocol.rs` /
   `start_remote_event_bridge`) so the phone client receives the same snapshots.

## UI side

**Placement:** a new typed pane leaf (`dag`) in the existing binary pane tree —
splittable/attachable like terminal and browser panes — plus an optional compact live
"wave strip" (1-line progress like the TUI widget) in the tab area next to agent
activity dots. Storage keys under `ferryx.*` (`ferryx.dag.*`).

**Rendering (recommended): hand-rolled layered SVG, no new dep.**

omo hands us the layout for free: waves are already topological layers. Layout =
columns = wave index, rows = order within wave; edges as cubic beziers between column
gaps. That is the entire hard part of DAG layout, solved by data. React Flow
(`@xyflow/react`) buys free pan/zoom/minimap at the cost of a new dep and its own
styling opinions; for ≤ 64 nodes a fit-to-view SVG with wheel-zoom is small and matches
the metallic dark aesthetic precisely. If free-form pan/drag is wanted later, React Flow
can replace the viewport layer without touching the data model.

Visual contract:

- Node card: state glyph (omo's own ✓/✗/▶ set), label, route (`category:quick` /
  `agent:…` + model), attempt `x2`, elapsed, `after` deps truncated.
- State → color: running = accent pulse, completed = success green, failed = red with
  error code on hover, blocked/pending = neutral, paused = amber.
- Critical path edges tinted accent; bottleneck nodes get a badge (`blocks 4`).
- Run header: name, `wave 2/4`, `3/8 done, 2 running`, amend count; header doubles as
  the click target for per-run detail (first failure, result file link → opens in
  browser pane via existing `results/<runId>/<nodeId>.txt` path).
- Node click → popover: full status, model, attempts, timing, error message, "reveal
  node output" (open result file) and, when the node is a live child task, the owning
  terminal via the existing identity-triad mapping if available.

**State layer:** `ui/src/state/dagStore.ts` (Zustand) keyed by project cwd → runs;
event subscription follows the same lifecycle rules as terminal listeners (no teardown
during HMR/pane DnD — `workspaceStore` retention contract).

## Delivery slices

1. `journal.rs` parser + derivation + unit tests against a fixture journal captured
   from a real 3-node `mass-ulw` run (RED→GREEN).
2. Watcher + Tauri events + IPC commands; manual QA with `bun tauri dev` (debug build
   only) running a live `mass-ulw` graph in a terminal pane.
3. `dagStore` + pane leaf type + SVG graph view with static rendering (snapshot only).
4. Live transitions (pulse on running, edge tint on critical path), wave strip, remote
   event relay.
5. Polish pass: node popover, result-file reveal, empty/unreadable-run states.

## Out of scope

- Writing to the journal or driving runs from Ferryx (omo owns the engine; control stays
  in the terminal).
- Cross-project DAG aggregation (later, if wanted).
- Historical analytics over past runs beyond the 7-day retention window.
