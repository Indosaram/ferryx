# Frontend performance mass-ulw

Started: 2026-08-22
Notepad: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ulw-20260822-232950.XXXXXX.md.nObNOPOSSM`
Repo: `/Users/indo/code/project/orca-lite`
Final report: `FRONTEND_PERFORMANCE_SWEEP.md`

## Tier
LIGHT control-plane in this session. Implementation is delegated.

## Topology
Chained dags. Cell synthesizes between runs.

### Run A — `frontend-perf-audit-20260822`
Wave 1 (parallel, `quick`, disjoint write = one markdown each):

| id | write | reason not-quick? |
|---|---|---|
| audit-app-store | `docs/frontend-perf/audit-app-store.md` | stays quick — mechanical scan |
| audit-shell | `docs/frontend-perf/audit-shell.md` | quick |
| audit-terminal | `docs/frontend-perf/audit-terminal.md` | quick |
| audit-settings | `docs/frontend-perf/audit-settings.md` | quick |
| audit-remote | `docs/frontend-perf/audit-remote.md` | quick |
| audit-bundle | `docs/frontend-perf/audit-bundle.md` | quick |

Wave 2:
- `synthesize` (`unspecified-low`: judgment to prioritize + cut disjoint fix packets) → `docs/frontend-perf/PRIORITIZED.md`
- `verify-audit` (`quick`) depends on synthesize — every lane file + prioritized packets have required headings

### Run B — built from PRIORITIZED.md
One node per packet. Disjoint write scopes. `unspecified-low` default; `unspecified-high` only if a packet spans 4+ files / store API. Each node owns RED test + production change + GREEN. Final `verify-tests` depends on all fix nodes.

### Run C — report
`writing` node writes `FRONTEND_PERFORMANCE_SWEEP.md` from the ledger.

## Constraints copied into every node
- No git commit. No desktop/UI automation. No branding/icon changes. No exposing folder name `orca-lite` in UI.
- Do not weaken UX (hover, motion, content) to buy perf.
- Do not "fix" Vite `server.watch.usePolling` unless it ships to the production bundle.
- Pre-existing failing tests stay documented, not silently skipped.
- Implementation model via category routing only (no `model` + `category`).

## Success criteria
1. Report exists; every High FIXED or ACCEPTED.
2. Touched-domain tests + LSP diagnostics introduce no new failures.
