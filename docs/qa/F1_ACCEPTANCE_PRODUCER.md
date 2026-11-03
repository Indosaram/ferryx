# F1 acceptance producer (supervisor autonomy panel plan, F1)

Plan clause: `docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md`, Final verification wave, F1.
Scenario: `f1-acceptance` in `scripts/qa/unified-supervisor.mjs`, which is opt-in through `config.f1Fleet`.
Files: `scripts/qa/unified-supervisor/f1-acceptance.mjs` (runner), `f1-host-agent.mjs` (one per computer), `f1-pane.mjs` (fixture session).

Status: this harness has not been run. It does not prove F1 until a real three-computer run writes `assertions.json` with status PASS.

## Invocation

Run it on the hub computer, which must be a remote host and never the owner's MacBook:

```
bun scripts/qa/unified-supervisor.mjs --scenario f1-acceptance --config <f1.json> --evidence <empty dir>
```

Exit codes: 0 PASS, 1 FAIL, 2 config/usage refusal, 4 harness error or deadline, 5 cleanup incomplete, 130 interrupted.
The deadline is 75 min. The outage alone takes 20 min of real time.
Clean up after a crashed run with `--cleanup --config <f1.json> --evidence <dir>`.
That only covers processes recorded on the hub computer: the relay, the hub daemon, the hub agent and the two ssh client processes.
It cannot reach a target computer. If a remote host agent dies without tearing down (for example `kill -9`, or the target reboots mid-run), that target's isolated daemon and its fixture panes keep running. Stop them by the pid recorded in `f1/rpc.jsonl` (the `daemonStart` reply) on that computer, after checking that the argv still names `<target dataRoot>/runs/<runId>`.
If ssh merely drops, the remote agent's stdin closes and it tears down its own daemon.

### Config (`f1.json`)

These are the standard unified-supervisor fields (see `config.schema.json`):

- `ferryxCli`, `ferryxRelay`: debug builds from this revision. The loopback relay and the loopback push endpoint are honoured only in debug builds.
- `ferryxApp`, `mhcBin`: required by the schema.
- `dataRoot`, `memoryRoot`, `projectRoot`: task-owned roots on the hub computer.
- `ownerPrincipal`.

The F1 fleet:

```json
"f1Fleet": {
  "hub": { "label": "hub", "agentDataRoot": "/abs/task-owned/f1-hub-agent" },
  "relayHost": "<hub address reachable from both targets>",
  "sshBin": "/usr/bin/ssh",
  "targets": [
    { "label": "target-a", "ssh": "<ssh dest>", "bun": "/abs/bun", "repoDir": "/abs/checkout-of-this-rev",
      "ferryxCli": "/abs/debug/ferryx-cli", "dataRoot": "/abs/task-owned/f1-target" },
    { "label": "target-b", "ssh": "<ssh dest>", "bun": "/abs/bun", "repoDir": "/abs/checkout-of-this-rev",
      "ferryxCli": "/abs/debug/ferryx-cli", "dataRoot": "/abs/task-owned/f1-target" }
  ]
}
```

Prerequisites:

- Each computer (hub and targets) must have a clean checkout of the same HEAD. The run checks `git status --porcelain` across the whole repository, not only the QA directory; any dirty file fails `fleet-same-revision`.
- For each computer, the run records the sha256, size and mtime of the `ferryxCli` that computer runs. These go in `f1-evidence.json` under `hosts.<agent>.ferryxCli`, next to `hosts.<agent>.repo` (root, head, dirty). `fleet-same-revision` also fails when a binary cannot be hashed.
- `f1Fleet.hub.agentDataRoot` must be disjoint from `dataRoot`, `memoryRoot` and `projectRoot`; the config check enforces this.
- A target's `bun` and `repoDir` are passed through the remote shell. They may contain only `A-Za-z0-9._/-`.
- Port 43821, the product's fixed gateway port, must be free on all three computers. Nothing is stopped to free it.
- ssh must work non-interactively (BatchMode).
- `outageDurationMs` and `quotaResetMs` default to the real 20 min and 3 min. Any other value fails its clause; it never passes as a shortened run.

## What runs where

| Computer | Product | Fixtures |
|---|---|---|
| hub (runner) | hub daemon (hub on, gateway localNetwork, agent control on), ferryx-relay | host agent: fixture proxy; stalled session; later the approval session |
| targets[0] | target daemon (hub off, relay mode, agent control on), paired by machine grant | 4 sessions behind a proxy that answers 502 for 20 min |
| targets[1] | same | 1 session behind a proxy that answers 429 with a 3 min reset |

Fixture sessions report `agentState` as `claude` on the daemon's agent-state socket, so the daemon records them as provider-sourced. Every turn makes a real HTTP call to the proxy on its own computer.
The supervisor, health classifier, recovery scheduler, action log, panel snapshot, owner event socket, push policy and push sender are all the product's own code.

## Verdict sources

- The owner's paired machine-Control device:
  - `GET /api/v1/supervisor/panel` and `GET /api/v1/fleet/sessions`, snapshotted at every phase.
  - The owner `/api/v1/events` socket. `panel.decision` and `panel.action` frames are stored raw.
- `supervisor_actions` rows, read from the hub database opened read-only.
- Pushes: the owner device subscribes a loopback Web Push endpoint through `POST /api/v1/push/subscribe`. Each encrypted POST that the product sender makes to it counts as one push.
- Recovery actions: what each fixture session received (Escape from a Stop, `continue` from a Send) and each turn's proxy status, with timestamps.

## Clauses (each is a required assertion)

| Clause | Assertion |
|---|---|
| 3 computers | `three-computers` (3 distinct hostnames, non-null hub host id, 3 distinct host ids), `targets-paired-by-machine-grant` |
| 4 sessions, real 20 min proxy outage | `outage-injected-real-duration`, `four-outage-sessions-on-one-computer`, `outage-sessions-recovered-after-outage` |
| 429 with 3 min reset | `quota-session-reset-three-minutes`, `quota-resumed-at-reset` (the first 200 turn falls inside [reset, reset + 60 s]; the 10 min fallback fails it) |
| provider-Working session stops drawing | `stall-session-provider-working`, `stall-cancelled-and-resumed` |
| all sessions finish | `all-recovery-sessions-finished` |
| exactly one log row per incident | `one-log-row-per-incident`, `recovery-rows-closed` |
| zero cards and zero pushes during recovery | `zero-cards-during-recovery` (also fails if the owner socket closed, errored or got a `panel.resync` during recovery, or was not open at the end of recovery), `zero-pushes-during-recovery` |
| non-local `sqlx migrate run`: one card with a recommendation | `approval-one-card-with-recommendation`, `approval-not-answered-by-supervisor` (requires the approval session ref to be resolved) |
| exactly one push | `approval-exactly-one-push` (observed across the full 30 s decision batch window) |
| owned teardown | `fleet-teardown-clean` |

## Evidence

The full record is in `<evidence>/f1/f1-evidence.json`. It contains:

- Per-scenario injection, spawn and done timestamps, with host labels, host ids and session refs.
- Recovery actions per session.
- Proxy logs.
- Every panel and fleet snapshot.
- Owner frames.
- Action rows (ids, incident keys, kinds, live flags).
- Decision-card and push counts.
- `clauses[]`, which gives pass/fail per clause.

Raw streams go in `f1/rpc.jsonl`, `f1/http.jsonl`, `f1/owner-frames.jsonl`, `f1/push-received.jsonl` and `f1/hub-socket.jsonl`.
Harness files: `assertions.json`, `cleanup.json`, `owned.jsonl`.

## Ownership and teardown

- The runner records each process it spawns: the relay, the hub daemon, the hub agent and the two ssh agents. The harness tears them down by recorded handle.
- Each host agent stops only the daemons it started, by recorded pid and process group.
- A pane process is signalled only when its pid comes from its own `pane-<pid>.jsonl` and its argv names this run's pane script and log directory.
- If an agent's stdin closes (the runner exits or ssh drops), that agent tears down. If the agent itself is killed, nothing on its computer tears down (see `--cleanup` above).
- All state lives under `<root>/runs/<runId>`. A root that holds anything else is refused.
