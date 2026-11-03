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
