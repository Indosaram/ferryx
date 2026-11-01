# Unified supervisor cutover and rollback

This runbook moves one owner installation from the legacy supervision intake to the unified
supervisor, and back. It is separate from `docs/hub-cutover.md`, which moves omo-gateway to the
ferryx hub. Run that cutover first. This one only decides which intake inside the hub takes the
owner's work.

## What the two intakes are

The legacy intake is three things. The omo app-server answers the owner's Slack turns. The
legacy lane goal loop continues a lane's goal after a settled turn. The kibitzer wakes the owner
lead.

The unified intake is also three things. Supervisor admission (`submit`, `adopt`, `message`)
takes the request. The dispatcher starts the worker. The wake scheduler resumes, grades and
closes goals.

The hub stores which intake is open in `supervisor_cutover`, one row per supervisor owner
(migration 0025). A `CHECK` constraint refuses a row with both intakes open, so the two can
never take the same work at once.

| installation | legacy intake | unified intake |
| --- | --- | --- |
| upgraded hub that already had lanes, chat sessions or an omo import | open | closed until `ferryx hub cutover` |
| fresh hub with no legacy history | idle (nothing to take) | open |
| after `ferryx hub cutover` | stopped | open |
| after `ferryx hub rollback` | open | closed |

An upgrade does nothing by itself. No legacy lane is revived and no goal is admitted or
dispatched. No wake is delivered. Owner work sent to the supervisor is refused with
`SUPERVISOR_INTAKE_CLOSED` (HTTP 503), and Slack owner messages keep the legacy turn.

Non-supervisor cron jobs (`scheduled_jobs`) keep their payload, schedule and backend through
every step below. Neither cutover nor rollback reads or writes them.

## Preflight

Run each command on the designated supervisor host. Every one only reads.

```bash
# 1. The hub is up and healthy, and you can see which intake is open now.
ferryx hub status --check
ferryx hub intake

# 2. The mhc supervisor engine is turned on in the hub env file.
grep -E '^FERRYX_HUB_MHC_(SUPERVISOR|BIN|PROVIDER|MODEL)=' ~/.ferryx/hub/env

# 3. The engine is ready and advertises every required capability.
ferryx-cli supervisor engine status --json
```

Step 3 must show `"state":"ready"`, and `capabilities` must contain all of `multi_session`,
`session_context`, `durable_session_id`, `memory_write_deny`, `memory_write_deny_effects`,
`tools_exclude`, `memory_receipts`, `memory_forget` and `supervisor_work_tools`. A cutover against
any other engine state is refused with the engine's own code. `MHC_NOT_RUNNING` means no engine
is installed. `MHC_CAPABILITY_MISSING` means a capability is absent. Other `MHC_*` codes come
from a launch or provider fault. A refused cutover changes nothing.

```bash
# 4. Nothing legacy is mid-turn. Adoption refuses a lane whose legacy turn is pending/sent.
sqlite3 -readonly ~/.ferryx/hub/hub.sqlite \
  "SELECT lane_key, proposal_state FROM lane_goal WHERE proposal_state IN ('pending','sent');"

# 5. Record what will not change: cron jobs and live panes.
sqlite3 -readonly ~/.ferryx/hub/hub.sqlite \
  "SELECT id, expression, enabled, next_run_at FROM scheduled_jobs ORDER BY id;" > ~/ferryx-cutover-cron-before.txt
ferryx-cli agents list > ~/ferryx-cutover-panes-before.json
```

If `FERRYX_HUB_DB` points somewhere else, use that path instead of `~/.ferryx/hub/hub.sqlite`
in every command here.

## Backup

Take a consistent copy of the hub database while the daemon runs. Do not stop
`com.ferryx.daemon`, because that ends every PTY session.

```bash
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p ~/.ferryx/hub/backups
sqlite3 ~/.ferryx/hub/hub.sqlite ".backup '$HOME/.ferryx/hub/backups/hub-$STAMP.sqlite'"
sqlite3 -readonly ~/.ferryx/hub/backups/hub-$STAMP.sqlite "PRAGMA integrity_check;"   # must print: ok
cp -p ~/.ferryx/hub/env ~/.ferryx/hub/backups/env-$STAMP
chmod 600 ~/.ferryx/hub/backups/hub-$STAMP.sqlite ~/.ferryx/hub/backups/env-$STAMP
```

The backup is for disaster recovery only. A normal rollback (below) does not need it, because
rollback deletes nothing.

## Cutover

```bash
ferryx hub cutover
```

The command exits 0 only on `"status":"ok"`. The reply lists the steps in commit order:

```json
{"status":"ok","op":"cutover","steps":["legacy_intake_stopped","unified_intake_enabled"],
 "intake":{"legacy":"stopped","unifiedOpen":true,"origin":"cutover",...},"engine":{...}}
```

The legacy stop is committed before the unified intake opens. From that commit on, the legacy
goal loop takes no new turn, a legacy proposal already committed is superseded instead of sent,
and the kibitzer stops waking the owner lead. Owner messages in the DM and in bound channels go
to the supervisor conversation. If such a message arrives on a daemon whose engine is not
installed, it gets an explicit `MHC_NOT_RUNNING` reply. It is never sent silently to the legacy
turn. Running `ferryx hub cutover` again is a no-op with an empty `steps` list.

Verify:

```bash
ferryx hub intake                     # legacy "stopped", unifiedOpen true
sqlite3 -readonly ~/.ferryx/hub/hub.sqlite \
  "SELECT id, step, created_at FROM supervisor_cutover_log ORDER BY id;"
diff <(sqlite3 -readonly ~/.ferryx/hub/hub.sqlite \
  "SELECT id, expression, enabled, next_run_at FROM scheduled_jobs ORDER BY id;") ~/ferryx-cutover-cron-before.txt
```

## Adopting old work

Nothing old is revived without an owner action. Each action maps one old lane, or one old channel
session, to one new goal contract. A retry, from any adapter, returns the same goal with
`"duplicate": true`. The schema allows one adoption row per lane or session.

```bash
# One legacy lane (its contract, mode and remaining budget are kept; supervised stays on hold).
cat > /tmp/adopt-lane.json <<'JSON'
{"requestId":"adopt-ledger-1","action":"adopt","payload":{"laneKey":"ledger"}}
JSON
ferryx-cli supervisor request --json /tmp/adopt-lane.json

# One legacy Slack/Discord session. The session has no contract, so you state the work.
cat > /tmp/adopt-session.json <<'JSON'
{"requestId":"adopt-session-1","action":"adopt","payload":{
  "sessionKey":"slack:C0123:1700000000.000100",
  "title":"Reconcile the nightly ledger",
  "instruction":"Reconcile the nightly ledger and run its verifier.",
  "requirements":[{"id":"r1","text":"the ledger reconciles","verifier":"cargo test reconcile"}],
  "operations":["edit","test"]}}
JSON
ferryx-cli supervisor request --json /tmp/adopt-session.json
```

The old transcript stays read-only and is not imported into owner learning. Only the most recent
messages of a session the owner may read reach the new worker, after the owner's own
instruction, inside an `untrusted` fence. They are context, not instructions.

## Rollback

```bash
ferryx hub rollback
```

The reply lists `["unified_intake_closed","legacy_intake_reopened"]` in that order. The unified
intake is closed first. After that, no admission is accepted (`SUPERVISOR_INTAKE_CLOSED`, 503),
no ready intent is claimed or dispatched, and no wake is delivered. Then the legacy intake
reopens: the legacy owner turn, the legacy goal loop and the kibitzer take work again.

Rollback deletes nothing and touches no pane. Goals, dispatch receipts, execution bindings,
pending wakes, the state log, memory provenance and every PTY session stay as they are. A run
that is already live keeps running in its pane. Its settle is recorded when the next cutover lets
the scheduler run again. Running `ferryx hub rollback` again is a no-op.

Verify:

```bash
ferryx hub intake                     # legacy "open", unifiedOpen false
ferryx-cli agents list > ~/ferryx-rollback-panes-after.json   # every pane from the preflight is still listed
sqlite3 -readonly ~/.ferryx/hub/hub.sqlite \
  "SELECT request_id, state, run_id, provider_session_id FROM supervisor_dispatches ORDER BY id;"
```

To cut over again later, repeat the preflight and run `ferryx hub cutover`. Kept ready intents
are dispatched, kept wakes are delivered, and the adopted goals continue.

## One active supervisor per account

With an account relay that serves the supervisor lease (`docs/relay-failover.md`, "Supervisor
lease on the relay"), exactly one machine of the account runs the supervisor: admission,
dispatch, wakes, cron, Slack and Discord, chat replies and the backfill sweeper. That machine
holds the lease. Every other machine whose hub is on is a standby: healthy, visible, and doing
no supervisor work of its own.

### Lease and fence

The holder renews every 20 s and a lease lasts 120 s. The holder stops 15 s before its local
deadline, counted from when it sent the last successful renew; after sleep or a wall clock
jump it drops to standby at once until the next renew succeeds. Another machine can take a
lease only 30 s after it expired.

Every supervisor send to a pane carries the lease fence. An executing daemon that has seen a
higher fence for the account rejects the send with `STALE_SUPERVISOR_FENCE`, and the goal is
held instead of settled. Manual input and requests without a fence are never fenced. A holder
that is told its fence is stale stands down and releases the lease.

A rolling daemon upgrade on the holder keeps the lease: the old daemon freezes its lease
client at the handover commit and hands `{machine_id, fence, deadline}` to the new one, whose
first request is a renew with the same machine id and fence. The new daemon starts the hub only
once that renew is granted. No other machine sees a free lease during the handover.

### Standby behaviour

- Slack Socket Mode is never opened on a standby, and no Slack or Discord post or chat reply
  leaves it; queued replies stay queued for the holder.
- `ferryx hub start` on a standby answers `{"status":"standby","holder":...}` and starts
  nothing.
- Phone routes (`POST /api/v1/supervisor/start`, supervisor requests, conversation, memory,
  pause) answer HTTP 409 `SUPERVISOR_STANDBY` with details `{holderMachineId, holderLabel}`, and
  the phone switches to the holder.
- `supervisor.request` from the owner's CLI or MCP is forwarded to the holder, and so are
  observations of this machine's own panes (keystrokes, relayed input, worker reports;
  `POST /api/v1/supervisor/observe` on the holder). The holder supervises the standby's panes.
- Forwarding needs an agent-control grant on the holder. A standby without one pairs itself:
  it redeems a grant through the account using the account session in
  `FERRYX_ACCOUNT_SESSION_TOKEN`. Without a session, or if redeeming fails (retried after
  5 minutes), the standby is "not paired to holder": requests are refused with
  `SUPERVISOR_STANDBY` and observations are dropped and counted in `observeDropped`.

`hub_stopped` still means "the owner turned the supervisor off" and is independent of the
lease. Losing the lease stops the hub tasks without setting it, so the machine starts again by
itself when it becomes holder.

### Moving the supervisor

On the machine that should run the supervisor:

```bash
ferryx hub move-here
```

The phone does the same with `POST /api/v1/supervisor/move` on the target machine, body
`{"force": false}`, answer `{"moved": true}` (`false`: it already ran there); the desktop panel
has a "Move supervisor here" button. The target asks the holder, over the paired end-to-end
channel, to stop (`POST /api/v1/supervisor/release-to`). The holder publishes a final
checkpoint and releases the lease reserved for the target. The target pulls that checkpoint
(`GET /api/v1/supervisor/checkpoint`), imports it and acquires the lease with the next fence.

What moves: goals, the supervisor identity, hub settings and secrets (the hub env file), the mhc
supervisor profile and the supervisor memory. Provider sessions do not move; the new holder
starts fresh ones. Before the import replaces anything, the target keeps its previous database
as `hub.sqlite.pre-move-<ts>` (with its `-wal`/`-shm`), and the previous env file and memory
repo as `*.pre-move-<ts>`. Hosts are remapped to the target's paired hosts. A host the exporter
could not scope is written as `unmapped:<id>`; its goals are held with `REMOTE_HOST_UNMAPPED`
and never sent to another pane. A target hub that already has a different supervisor and was
never archived refuses the import with `SUPERVISOR_IDENTITY_MISMATCH`.

If the holder cannot be reached, the move succeeds only when the lease expired past the grace
period and this machine has the checkpoint the relay recorded last. Otherwise it fails with
HTTP 409 `SUPERVISOR_FORCE_REQUIRED`, details `{holderLabel, checkpointAt}`, and the message
names what will be lost ("changes since <checkpointAt> on <holder>"). To take over anyway:

```bash
ferryx hub move-here --force --confirm
```

`--force` alone is sent unforced and only prints the warning; `--confirm` without `--force` is
a usage error. On the phone, send `{"force": true}`. A holder whose lease is still live cannot
be forced out; the move answers `SUPERVISOR_STANDBY`.

### Checkpoints and automatic takeover

The holder publishes a checkpoint every 300 s and on every release. Standbys pull the newest
one over the paired channel and keep the two newest, readable by the owner only. A checkpoint
contains the Slack and provider tokens and the hub secrets, so a standby holds them too. A
checkpoint is at most 64 MiB compressed (`CHECKPOINT_TOO_LARGE` otherwise), and reading one
stops once it unpacks past 512 MiB, so a small archive cannot fill the disk.

When the holder's lease expired past the grace period, a standby takes over automatically only
if its hub is not stopped and its newest checkpoint is the one the relay recorded. It imports
that checkpoint (keeping `hub.sqlite.pre-move-<ts>`) before its hub reopens. The relay decides
between several candidates. A standby that is blocked says why in `promotionBlockReason`:
`HUB_STOPPED`, `CHECKPOINT_MISSING`, `CHECKPOINT_STALE`, `LEASE_HELD` or the relay's refusal
code. Nothing forces a takeover automatically.

An old holder that comes back (after sleep or a network cut) finds another machine holding the
lease and becomes standby; every send it had queued is rejected with `STALE_SUPERVISOR_FENCE`.
While the relay runs on its standby, the holder keeps renewing and nothing changes hands; a
move or a takeover waits for the primary relay or its promotion.

### Status fields

The supervisor snapshot's `hub` object reports:

| field | shape | meaning |
| --- | --- | --- |
| `role` | `holder` \| `standby` \| `unleased` | `unleased`: no lease can be confirmed (for example `RELAY_LEASE_UNSUPPORTED`); treated like standby |
| `holderMachineId`, `holderLabel` | string | the machine running the supervisor |
| `holderPaired` | bool | this standby can forward to the holder |
| `fence` | int | the lease fence this machine last saw |
| `leaseExpiresIn` | int seconds | until the holder's lease expires unless renewed |
| `checkpointId`, `checkpointAge` | string, int seconds; absent if none | the newest checkpoint stored here |
| `observeDropped` | int | pane observations this standby could not forward |
| `promotionBlockReason` | `{code, message}`; absent if nothing blocks | why this standby would not take over |
| `unreachableHosts` | `[{host, reason}]`, `host` null for this machine | machines the holder could not push its fence to; their sessions wait |
| `unmappedHosts` | `[string]` | host ids from an imported checkpoint this machine cannot map |
| `adoptable` | list of goals | open goals of this machine's archived pre-lease supervisor |

The fields marked "absent" are omitted, not null. A daemon without lease support sends none of
these fields and runs its own supervisor as before.

### First rollout

Order matters, because a daemon without lease support still runs its own supervisor and its own
Slack socket:

1. Upgrade both relays: the standby relay, then the primary (`docs/relay-failover.md`, "Deploy
   order for the lease").
2. Upgrade and start the machine that should keep the supervisor first: the one that has the
   Slack tokens today. The first machine of the account that gets the lease keeps its
   supervisor identity, goals and memory.
3. Upgrade every other daemon of the account before relying on the lease.

Every other machine whose hub ran its own supervisor before the lease archives it the first
time it becomes standby: it copies its database to `hub.sqlite.pre-lease-<ts>` (never deleted)
and lists its open goals in `adoptable` and in the standby UI. Nothing is adopted
automatically; adopting an archived goal is an owner action that submits it to the holder once.
(The CLI command for it is not merged yet.) An archived hub counts as fresh for a later move
into this machine.

If the relay is ever downgraded on purpose to a version without the lease, daemons that saw the
lease stay on standby (`RELAY_LEASE_UNSUPPORTED`). Clear that with
`ferryx hub lease reset-capability` on each machine.

### Self-hosted and single-machine installs

A daemon with no account enrollment, or whose relay never advertised the lease, is its own
permanent holder with fence `0` and behaves as before. If the relay is unreachable at boot, the
last holder of a one-machine account starts without waiting.

## Disaster recovery from the backup

Use this only if the hub database itself is damaged. It replaces every hub row written since the
backup, so it is not a rollback.

```bash
ferryx hub stop                       # hub tasks only; the daemon and its PTYs stay up
cp -p ~/.ferryx/hub/hub.sqlite ~/.ferryx/hub/backups/hub-damaged-$(date -u +%Y%m%dT%H%M%SZ).sqlite
rm -f ~/.ferryx/hub/hub.sqlite-wal ~/.ferryx/hub/hub.sqlite-shm
cp -p ~/.ferryx/hub/backups/hub-$STAMP.sqlite ~/.ferryx/hub/hub.sqlite
ferryx hub start
ferryx hub status --check
ferryx hub intake
```

## Checks in the repository

- Hub tests: `cargo test --manifest-path crates/ferryx-hub/Cargo.toml supervisor::tests::cutover`
- Isolated end-to-end run: `bun scripts/qa/unified-supervisor.mjs --scenario legacy --config .qa/unified-supervisor.json --evidence ../.evidence/unified-supervisor-20261027/legacy`
  (evidence in `legacy/` and `task-16-cutover.log`)
