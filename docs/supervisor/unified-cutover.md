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

## Automatic setup

One owner action sets the supervisor up. It runs from the desktop Supervisor setup step
(onboarding or the setup banner), from a paired phone (`POST /api/v1/supervisor/setup`,
machine Control devices only), or from the CLI:

```bash
# What is still missing, and the next step.
ferryx hub setup --status --json

# Set up, seed owner memory, and map one checkout on this host.
ferryx hub setup --import-memory --project ~/code/myrepo
```

The CLI is refused inside a Ferryx pane (`FERRYX_SESSION_ID` is set). When a running daemon
advertises `supervisorSetupV1`, the CLI goes through it. Without one, the CLI writes the hub
database and env file itself, and the engine starts with the next hub generation.

A run does these steps in order. Each one is idempotent, so a second run changes nothing.

1. It writes `FERRYX_HUB_MHC_SUPERVISOR=1` to the hub env file (`~/.ferryx/hub/env`, or
   `FERRYX_HUB_ENV_FILE`) unless the engine is already on.
2. It records the owner's choices in `hub_meta`: automatic cutover on, and memory import on
   only with `--import-memory` (`importMemory` on the other surfaces).
3. If the running hub generation has no engine, it restarts the generation in-process, as
   `ferryx hub stop` then `ferryx hub start` do, without leaving the stopped flag set. The
   daemon is never restarted, so live PTY sessions stay up. A daemon that handed the hub to its
   successor refuses with `INVALID_STATE`.
4. It creates the one owner identity if none exists. The owner is `slack:<team>:<user>` when
   the hub knows the Slack owner (`owner_slack_id`) and the Slack team (from a stored Slack
   conversation, else Slack `auth.test`). Otherwise it is `local:<OS account>`.
5. It maps each requested project path on this host (`local`) the way
   `ferryx-cli supervisor setup` does. The git common dir is resolved to its canonical path.
   The repository identity is the normalized `origin` URL, else `local:<git common dir>`.
   A path that is not a checkout root or git common dir is refused on its own
   (`NOT_A_GIT_DIR` in `projectResults`), and the other paths still map.
6. It answers with the setup status below, plus `projectResults` and `restarted`.

After setup, each hub generation with the engine watches the engine's health. It does not
poll. On every transition to Ready it runs the automatic cutover. That cutover does nothing
unless automatic cutover is on, an owner exists, and there is legacy work to cut over. It
never runs after an explicit `ferryx hub rollback`, because the owner decided.

After the first Ready, the generation seeds owner memory once, if memory import is on and no
import has finished yet. It reads two sources:

- the owner's typed turns from the most recent Claude (`~/.claude/projects`), Codex
  (`~/.codex/sessions`) and omo (`~/.omo/agent/sessions`) sessions: 20 sessions, the latest
  10 turns of each;
- the owner's global instruction files `~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`,
  `~/.config/opencode/AGENTS.md` and `~/.omo/agent/AGENTS.md`: up to 64 KiB each, one
  paragraph per turn.

Sessions the hub drove, tagged wrappers and anything that looks like a credential are skipped.
A rerun ingests nothing new. Agent-written memory, such as `~/.omo/memory`, is never read.
The log records counts only, never text.

When the engine is missing or not ready, nothing is cut over. `nextAction` names the unmet
engine step, and `engineFault` carries the `MHC_*` code. The Preflight steps below fix it.

The setup status is camelCase JSON:

| field | meaning |
| --- | --- |
| `engineEnabled`, `engineInstalled`, `engineState` | the env flag, an engine in the running generation, and its state |
| `engineFault`, `missingCapabilities` | `{code, message}` or null, and required capabilities the engine lacks |
| `restartRequired` | the flag is on but the running generation has no engine |
| `owner`, `projects` | `{supervisorId, ownerPrincipal}` or null, and the owner's mapped project count |
| `intake` | `{unifiedOpen, legacy, origin, legacyHistory}` |
| `autoCutover`, `memoryImport` | the owner's choices; `memoryImport` is `{enabled, sessions, turns, lastAt}` |
| `adoptable` | legacy lanes that can be adopted |
| `nextAction` | the first unmet step: `enable_engine`, `restart_required`, `wait_engine`, `fix_engine`, `create_owner`, `map_projects`, `cutover`, `import_memory`, `adopt` or `none` |

The same status is in the Supervisor snapshot's `setup` field. It is also served by
`GET /api/v1/supervisor/setup` (any machine device except a mirror) and by the daemon request
`supervisorSetup {action: "status"}`.

Old lanes can be adopted in one action: the desktop's adopt-all, the phone's
`POST /api/v1/supervisor/adopt-all` (Control devices), or `supervisorSetup {action: "adoptAll"}`.
It adopts every adoptable legacy lane as the owner, through the same adopt request as
"Adopting old work" below. A lane is adoptable when it is queued or blocked, is not a supervisor
goal, and was not adopted before. The answer is
`{"adopted": [lane], "refused": [{"lane", "code", "message"}]}`. One refusal does not stop the
others.

## Preflight

The manual runbook below is for an operator who wants to drive each step. Automatic setup
performs the same steps. Run each command on the designated supervisor host. Every one only
reads.

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
- `ferryx-cli mcp` asks the daemon for its role at startup and again every 5 s while it serves.
  On standby it never opens the hub database, and supervisor reads and writes are refused with
  `SUPERVISOR_STANDBY`, naming the holder. It fails closed: if this machine has a lease client and
  the daemon does not answer, it refuses with `SUPERVISOR_UNAVAILABLE`. A process started on
  standby keeps refusing (`SUPERVISOR_UNAVAILABLE`) after this machine takes over. Restart it
  to serve supervisor state.
- Forwarding needs a machine grant on the holder. A standby without one pairs itself: it
  redeems a grant through the account, signed with its enrolled machine key and bound to the
  device key it pairs with, so the account vouches that the device is a machine of the same
  account. The holder honours that only when the account's signature over the grant, which the
  relay forwards with it, verifies against the account key the holder pinned at enrollment (or
  `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY`); a relay cannot mint the mark itself. With an older relay
  or no pinned key the grant still pairs, but follows the agent-control switch like any other
  device. The account session comes from
  `FERRYX_ACCOUNT_SESSION_TOKEN` when set, otherwise from the saved session (see "The saved
  account session" below). Without a session, or if redeeming fails (retried after 5 minutes),
  the standby is "not paired to holder": requests are refused with `SUPERVISOR_STANDBY` and
  observations are dropped and counted in `observeDropped`. Without a session, `ferryx hub
  status` and the snapshot also carry `holderPairingReason` naming the fix (`ferryx account
  login`).
- A standby that already had a grant on the holder from before this release (for example one
  the desktop paired) holds a grant without the account's same-machine marker, which the holder
  refuses with 403 `FORBIDDEN` while agent control is off. That refusal carries
  `details.refusal = "agentControl"`. On the first such refusal from a machine-to-machine
  route (checkpoint pull, `release-to`, forwarded `supervisor/request`, `observe`) the standby
  sets that grant aside, redeems one fresh grant through the account and retries the call once.
  If the holder still refuses, the refusal is reported and no other grant is redeemed for 5
  minutes. Any other `FORBIDDEN` (a view-only device, a device excluded in Settings, or the
  supervisor's own answer such as a goal of another owner) is reported as it is and redeems
  nothing.
- The "allow agent control" switch does not govern these same-account machine-to-machine
  paths: checkpoint pull (`GET /api/v1/supervisor/checkpoint`), `release-to`, `observe`,
  forwarded `supervisor/request` and the fence push work on a default install. They also skip
  the restricted-device allowlist: a same-account machine peer is admitted even when the
  owner limited agent control to listed devices. Scope and permission still apply (a view-only
  device cannot pull a checkpoint), and excluding the device in Settings still refuses it.
  Phones and the web app keep following the switch and the allowlist on every route,
  including these.

#### The saved account session

The daemon keeps the account session in `account-session.json` in the identity directory:
`~/.ferryx/remote/account-session.json` on macOS and Linux, `%LOCALAPPDATA%\Ferryx\remote\account-session.json`
on Windows, or `$FERRYX_DATA_DIR/remote/account-session.json` when `FERRYX_DATA_DIR` is set.
It is owner-only (mode 0600, or an owner-only ACL on Windows) and only sent to the account
origin that issued it. It is written by:

- `ferryx account login`. On a machine already enrolled to the same owner at that account
  origin it only saves the session: the enrollment, and its enrollment epoch, stay as they
  were, so devices already paired to the machine keep working. A machine not enrolled there,
  or enrolled to another owner, is enrolled as before;
- `ferryx-cli up` when the owner approves the machine in the browser (an install key carries
  no session);
- the desktop app when it first links the computer to the account (onboarding). A computer
  that is already linked is not linked again, so this does not refresh a session.

`ferryx account enroll --code` does not save one; run `ferryx account login` on such a machine.
The session lasts 30 days from when it was issued; after that the standby is "not paired to
holder" until `ferryx account login` is run again.
To remove the saved session, delete the file (there is no `ferryx account logout` command);
`FERRYX_ACCOUNT_SESSION_TOKEN` still wins over it when set.

Approving a machine (the browser approval of `ferryx-cli up` or of `ferryx account login`)
lets it act as a machine of the account: with its saved session it can pair to the account's
other machines and pull their supervisor checkpoints, which carry secrets, even while their
"allow agent control" switch is off. Approve only machines you own.

`hub_stopped` still means "the owner turned the supervisor off" and is independent of the
lease. Losing the lease stops the hub tasks without setting it, so the machine starts again by
itself when it becomes holder.

`ferryx hub stop` on the holder sets `hub_stopped`, stops the hub tasks and then hands the
supervisor on: it publishes a final checkpoint first and then releases the lease, and the
release records that checkpoint. The relay treats a released lease as free without waiting
for the grace period, for a machine that presents that checkpoint. If the final checkpoint cannot
be published, the lease is kept rather than released with an older checkpoint recorded. It
then expires normally (120 s plus the 30 s grace). If the relay does not acknowledge the
release, for example while only the standby relay is up, the lease also just expires. A stop
on a standby, or on a self-hosted holder with fence `0`, publishes and releases nothing.

### Moving the supervisor

On the machine that should run the supervisor:

```bash
ferryx hub move-here
```

The phone does the same with `POST /api/v1/supervisor/move` on the target machine, body
`{"force": false, "replaceLocal": false}` (`replaceLocal` is optional), answer
`{"moved": true}` (`false`: it already ran there). The desktop panel has a "Move supervisor
here" button (`cmd_supervisor_move_here` with `force` and an optional `replace_local`). The
target asks the holder, over the paired end-to-end
channel, to stop (`POST /api/v1/supervisor/release-to`). The holder publishes a final
checkpoint and releases the lease reserved for the target. The target pulls that checkpoint
(`GET /api/v1/supervisor/checkpoint`), imports it and acquires the lease with the next fence.

The released lease stays reserved for the target for 120 s after the release. Until then the
relay refuses every other machine with `LEASE_RESERVED`, the old holder and `--force`
included, so a holder that restarts mid-move cannot take the lease back. If the move fails or is
cancelled, the old holder (or any machine) gets the lease again once the window has passed; in
between no machine runs the supervisor.

What moves: goals, the supervisor identity, hub settings and secrets (the hub env file), the mhc
supervisor profile and the supervisor memory. Provider sessions do not move; the new holder
starts fresh ones. Before the import replaces anything, the target keeps its previous database
as `hub.sqlite.pre-move-<ts>` (with its `-wal`/`-shm`), and the previous env file and memory
repo as `*.pre-move-<ts>`. Hosts are remapped to the target's paired hosts. A host the exporter
could not scope is written as `unmapped:<id>`; its goals are held with `REMOTE_HOST_UNMAPPED`
and never sent to another pane. A target with the same supervisor identity, or one whose
supervisor was already archived (see "First rollout"), imports without an archive.

If the target hub holds a different supervisor, the move is refused with
`SUPERVISOR_IDENTITY_MISMATCH` (HTTP 409). Nothing is archived or staged, and the lease does
not move. The CLI prints a hint to add `--replace-local`. Only the owner can replace that
local supervisor, and only explicitly:

```bash
ferryx hub move-here --replace-local                    # holder reachable
ferryx hub move-here --force --confirm --replace-local  # forced takeover (see below)
```

On the phone, send `{"force": ..., "replaceLocal": true}`. The desktop command takes
`replace_local: true`. With it, the target first archives its own supervisor as
`hub.sqlite.pre-lease-<ts>` (with its `-wal`) and then imports again. The archive is written
once and never deleted, and its open goals become adoptable (see "First rollout").
Automatic takeover never replaces a local supervisor.

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

A forced takeover first imports this machine's newest pulled checkpoint, if it has one, and
only then asks the relay for the lease. That import runs the same identity check. A hub that
holds a different supervisor is therefore refused with `SUPERVISOR_IDENTITY_MISMATCH` before
the lease moves, unless `--replace-local` is also given. If the relay then refuses the lease,
the staged import is discarded and this machine keeps its own database.

### Checkpoints and automatic takeover

The holder publishes a checkpoint every 300 s and on every release. Standbys pull the newest
one over the paired channel and keep the two newest, readable by the owner only. A checkpoint
contains the Slack and provider tokens and the hub secrets, so a standby holds them too. A
checkpoint is at most 64 MiB compressed (`CHECKPOINT_TOO_LARGE` otherwise), and reading one
stops once it unpacks past 512 MiB, so a small archive cannot fill the disk.

When the holder's lease expired past the grace period, a standby takes over automatically only
if its hub is not stopped and its newest checkpoint is the one the relay recorded. It imports
that checkpoint (keeping `hub.sqlite.pre-move-<ts>`) before its hub reopens. The relay decides
between several candidates. This import never replaces a different local supervisor. It is
refused with `SUPERVISOR_IDENTITY_MISMATCH`, the refusal is logged, and the hub reopens on its
own database, which the refused import never touched. A standby that is blocked says why in
`promotionBlockReason`:
`HUB_STOPPED`, `CHECKPOINT_MISSING`, `CHECKPOINT_STALE`, `LEASE_HELD` or the relay's refusal
code. Nothing forces a takeover automatically.

An old holder that comes back (after sleep or a network cut) finds another machine holding the
lease and becomes standby; every send it had queued is rejected with `STALE_SUPERVISOR_FENCE`.
While the relay runs on its standby, nothing changes hands, but the standby cannot extend the
lease: the holder keeps the supervisor at most until the expiry the primary last recorded, then
stands down (see "Supervisor lease on the relay" in `docs/relay-failover.md`). A move or a
takeover waits for the primary relay or its promotion.

### Status fields

The supervisor snapshot's `hub` object reports:

| field | shape | meaning |
| --- | --- | --- |
| `role` | `holder` \| `standby` \| `unleased` | `unleased`: no lease can be confirmed (for example `RELAY_LEASE_UNSUPPORTED`); treated like standby |
| `holderMachineId`, `holderLabel` | string, null if unknown | the machine running the supervisor |
| `holderPaired` | bool | this standby can forward to the holder |
| `holderPairing` | `paired` \| `not paired to holder` \| `no holder` | the pairing state behind `holderPaired` |
| `holderPairingReason` | string; absent unless the owner can fix it | why this standby is not paired, e.g. no account session: run `ferryx account login` |
| `fence` | int | the lease fence this machine last saw |
| `leaseExpiresIn` | int seconds | until the holder's lease expires unless renewed |
| `checkpointId`, `checkpointAge` | string, int seconds; absent if none | the newest checkpoint stored here |
| `observeDropped` | int | pane observations this standby could not forward |
| `promotionBlockReason` | `{code, message}`; absent if nothing blocks | why this standby would not take over |
| `unreachableHosts` | `[{host, reason}]`, `host` null for this machine | machines the holder could not push its fence to; their sessions wait |
| `unmappedHosts` | `[string]` | host ids from an imported checkpoint this machine cannot map |
| `adoptable` | `[{goalId, title, supervisorId, fromMachine}]`; absent when empty | open goals of this machine's archived supervisor not adopted yet |

The fields marked "absent" are omitted, not null. A daemon without lease support sends none of
these fields and runs its own supervisor as before.

`ferryx hub status` reports the same lease fields under `supervisor_lease`, including
`supervisor_lease.holderPairing`. On standby it also prints a `status_hint` line naming the
holder and the pairing state.

### First rollout

Order matters, because a daemon without lease support still runs its own supervisor and its own
Slack socket:

1. Upgrade both relays: the standby relay, then the primary (`docs/relay-failover.md`, "Deploy
   order for the lease").
2. Upgrade and start the machine that should keep the supervisor first: the one that has the
   Slack tokens today. The first machine of the account that gets the lease keeps its
   supervisor identity, goals and memory.
3. Upgrade every other daemon of the account before relying on the lease.
4. On every machine enrolled before this release (including those the desktop app linked and
   those set up with an install key or `ferryx account enroll --code`), run
   `ferryx account login` once after the upgrade, approving it as the account's owner. It only
   saves an account session for the daemon to pair with the holder (see "The saved account
   session"): the machine is not re-enrolled and its enrollment epoch does not change, so its
   existing pairings keep working. Machines set up afterwards with `ferryx account login`,
   `ferryx-cli up` (browser approval) or the desktop's first link already have one. A standby
   still paired by an older desktop grant replaces it by itself on the holder's first
   agent-control refusal once it has a session. Upgrade the relay first (step 1): an account
   service older than this release returns no session, so login then enrolls the machine as
   before and saves nothing.

Every other machine whose hub ran its own supervisor before the lease archives it once it is
standby for a holder with a different supervisor id. The standby learns that id from the
holder's checkpoint it pulls; until it has one, nothing is archived. It copies its database
(and `-wal`) to `hub.sqlite.pre-lease-<ts>` once and never writes or deletes that copy. A
former holder that moved away keeps the holder's own identity, so it is never archived and
lists nothing as adoptable. A move onto a machine
that still holds a different supervisor archives it the same way only with the owner's
`--replace-local` (see "Moving the supervisor"); without it the move is refused. An archived
hub counts as fresh for a later move into this machine.

The archive's open goals (neither completed nor cancelled) are listed in `adoptable` and in the
standby UI ("Adoptable from <fromMachine>"). Nothing is adopted automatically. To adopt them,
run this on the machine that holds the archive:

```bash
ferryx hub adopt-archived --goal <goalId>   # one goal
ferryx hub adopt-archived --all             # every goal not adopted yet (also the default)
```

The desktop app does the same with `cmd_supervisor_adopt_archived` (`goalId`, or none for
all). `cmd_supervisor_adoptable` returns `{"adoptable": [...]}`, whose entries also carry
`projectName` and `runState`. Each adopted goal is sent to the holder as a `supervisor.request`
`submit` with its original request text, contract and project identity; a standby forwards it
over the paired channel. The `requestId` is `archived:<supervisor_id>:<goal_id>`. The holder
admits it once by that id, so adopting the same goal again, from any adapter, returns the same
goal. The reply is `{"adopted": [{goalId, sourceId, adopted, answer, error}]}`, and the
command exits 1 if any goal was not adopted. Adopted goals are recorded in
`adopted-goals.json` beside the hub database, not in the archive, and drop out of `adoptable`.
A goal that failed is not recorded, so the next adopt retries it under the same `requestId`.

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
