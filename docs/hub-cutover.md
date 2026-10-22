# omo-gateway to ferryx hub cutover

The ferryx daemon opens the hub only after `ferryx hub start` (the cutover script sends it after omo is unloaded) or with `FERRYX_HUB=on`. A fresh hub database stays stopped, because the hub reads the same `~/.omon/secrets` Slack tokens as omo and would otherwise open a second Socket Mode connection. Slack, Discord, and voice stay idle until a token or speech key is present. The dashboard listens on `127.0.0.1:9119` and serves `~/.omon/web` plus `/api/health`, `/api/tracker`, cron, approvals, and logs.

Two Socket Mode connections must not overlap. Unload `ai.omon.gateway` and `ai.omon.gateway-watchdog` before the ferryx hub opens Slack.

A Discord `/voice join` binds that voice channel to the text channel's session. Each utterance is sent to the same pane a typed message in that channel uses, and the spoken reply is the assistant text that pane adds once the turn is idle.

The cron scheduler sleeps until the earliest `next_run_at`. Creating or triggering a job wakes it. A due job runs its payload command, posts Slack and Discord deliveries, and records a cron problem. The third consecutive failure opens a remediation lane. A publishing rerun still needs the owner token on the cron socket. The dashboard `POST /api/cron/jobs/{id}/trigger` returns 409 for a publishing job and for a paused job, and 404 for an unknown id. An enabled job only moves `next_run_at` to now. Approving a remediation problem does not run a disabled job. `[SILENT]` and `wakeAgent: false` are stored on the run and are not posted.

The hub control socket is `~/.ferryx/hub/control.sock`. `ferryx hub stop` and `ferryx hub start` write one JSON line to it. Those commands drop or bind the hub's chat, cron, and HTTP tasks. They do not restart `com.ferryx.daemon`, so live PTY sessions stay up. A fresh hub database, a missing `hub_stopped` row, and an unreadable flag all stay stopped. `hub stop` stores `hub_stopped = 1`. `hub start` stores `hub_stopped = 0`, so the next daemon start brings the tasks back. `FERRYX_HUB=on` starts them even when the flag is missing. `FERRYX_HUB=off` still skips the hub. Cron run-now clients keep using `~/.omon/cron-run-now.sock`. The same line protocol is also bound at `~/.ferryx/hub/cron.sock` and `~/.ferryx/hub/cron-run-now.sock`. A disabled job is not run from that socket unless the line is `{"id":"...","force":true}`. A run-now result is posted to the job's deliver target. An origin-less `origin`, `slack`, `all`, or `discord` target uses the Slack home channel copied at import. Job scripts run with their working directory set to the omo `WorkingDirectory` recorded at import.

`ferryx hub` and `ferryx-cli hub` both dispatch the hub command. The GUI binary does not open a window for that subcommand.

`FERRYX_HUB_NOTIFY=actionable` is the default. Slack and Discord then receive approval cards, a question or a pane waiting on input, a failed turn, a crashed worker, a cron failure (and the line that a remediation lane opened), and the final result of a task the owner started from Slack or Discord, replied in that thread. Plain turn-complete and progress notices are not posted. Every notice, including a suppressed turn-complete, stays in `session_watch_notices` and the dashboard activity log (`/api/logs`). `FERRYX_HUB_NOTIFY=all` also posts turn-complete notices to the home channel. `OMON_SESSION_NOTIFY` is the same switch and loses to `FERRYX_HUB_NOTIFY`. When neither variable is set, `hub_meta.notify_mode` is used. omo-gateway has no stored notify mode; import copies `session_notify`, `FERRYX_HUB_NOTIFY`, or `OMON_SESSION_NOTIFY` from `config.yaml`, `config.yml`, `config.json`, or `.env` next to the omo database when one of those keys is present, and leaves the default when they are absent.

## Live database

The gateway's launchd job uses WorkingDirectory `/Users/indo/code/project/omon-gateway`. The database is `omon_gateway.db` in that directory, which is `~/code/project/omon-gateway/omon_gateway.db`. It is not `~/.omon/gateway.db`.

Discovery order, for `ferryx hub migrate` with no `--from-omo` and for `scripts/hub-cutover.sh`:

1. `~/Library/LaunchAgents/ai.omon.gateway.plist` key `WorkingDirectory`, plus `/omon_gateway.db`. The script reads that one key with `plutil -extract`. It does not print the plist.
2. `~/code/project/omon-gateway/omon_gateway.db`
3. `~/.omon/gateway.db`, only if that file is present

A dry-run against the live file prints these rows. The counts grow as the gateway keeps running, so a later snapshot is larger than an earlier one.

| row | what it counts |
| --- | --- |
| cron_jobs_enabled | jobs with `enabled = 1` |
| lanes | lanes |
| lane_threads | `lane_thread_bindings` |
| issues | tracker issues |
| activity | tracker activity |
| inbox_items | inbox items |
| proposals | action proposals |
| bot_profiles | bot profiles |
| allowlist | approval allowlist |

`cron_jobs` is every row, including paused jobs.

## Before

1. Build a ferryx binary whose default features include `hub-slack`, `hub-discord`, and `hub-voice`.
2. Confirm `~/.omon/secrets/` still holds the bot tokens. The hub reads `NAME_FILE`, then `NAME`, then those files. Do not print them.
3. Dry-run the copy. It prints row counts and does not create or modify the hub database:

```bash
ferryx hub migrate --dry-run
```

A real import skips tracker rows whose deleted parent would fail a foreign key and prints `table orphans_skipped.<table> N`. The import still finishes.

## Cutover

`scripts/hub-cutover.sh` refuses to run without `--yes`. On macOS it:

1. Snapshots the live database with `sqlite3 .backup` while omo is still running.
2. Dry-runs `ferryx hub migrate` against that snapshot, then imports it. The script working directory is the live database's directory. `imported_from` records the live database path, not the snapshot. If either step fails, omo is still loaded.
3. Boots out `ai.omon.gateway-watchdog` and `ai.omon.gateway` only after the import succeeds.
4. Waits until that gateway process is gone.
5. Snapshots the live database again and migrates that copy before `hub start`, so rows written between the first snapshot and bootout are carried over. Changed cron `enabled` / `next_run_at`, issue state, and inbox decision status are updated. New rows are inserted.
6. Sends `ferryx hub start` on the control socket. It does not `kickstart -k` the daemon. If bootout, the delta import, or start fails, both launch agents are loaded again.

```bash
scripts/hub-cutover.sh cutover --ferryx /path/to/ferryx --yes
```

`--db PATH` overrides discovery. `--dry-run` prints the bootout it would do and the row counts. It does not unload omo, does not start the hub, and does not create the hub database. Then check `http://127.0.0.1:9119/api/health`.

`scripts/launchd/com.ferryx.hub-watchdog.plist` runs `scripts/hub-watchdog.sh` every 60 seconds. The template has no secrets. A Slack alert is sent only when `WATCHDOG_SLACK_TOKEN_FILE` and `WATCHDOG_SLACK_CHANNEL` are set in the environment.

## Rollback

Rollback stops the hub through the control socket, then bootstraps the omo launch agents. The ferryx daemon is not restarted.

```bash
scripts/hub-cutover.sh rollback --yes
```

If `ferryx hub stop` fails, the script does not start omo and does not restart the daemon. Leave the ferryx hub off until omo's Socket Mode connection is the only one. A daemon that was started with `FERRYX_HUB=off` never bound the control socket; stopping that hub would require a daemon restart, and that restart drops PTY sessions. The normal daemon start binds the socket, so rollback does not need that restart.
