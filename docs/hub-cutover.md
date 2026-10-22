# omo-gateway to ferryx hub cutover

The ferryx daemon starts the hub. Slack, Discord, and voice stay idle until a token or speech key is present. The dashboard listens on `127.0.0.1:9119` and serves `~/.omon/web` plus `/api/health`, `/api/tracker`, cron, approvals, and logs.

Two Socket Mode connections must not overlap. Unload `ai.omon.gateway` and `ai.omon.gateway-watchdog` before the ferryx hub opens Slack.

The cron scheduler sleeps until the earliest `next_run_at`. Creating or triggering a job wakes it. A due job runs its payload command, posts Slack and Discord deliveries, and records a cron problem. The third consecutive failure opens a remediation lane. A publishing rerun still needs the owner token on the cron socket. `[SILENT]` and `wakeAgent: false` are stored on the run and are not posted.

The hub control socket is `~/.ferryx/hub/control.sock`. `ferryx hub stop` and `ferryx hub start` write one JSON line to it. Those commands drop or bind the hub's chat, cron, and HTTP tasks. They do not restart `com.ferryx.daemon`, so live PTY sessions stay up. Cron run-now clients keep using `~/.omon/cron-run-now.sock`. The same line protocol is also bound at `~/.ferryx/hub/cron.sock` and `~/.ferryx/hub/cron-run-now.sock`.

## Live database

The gateway's launchd job uses WorkingDirectory `/Users/indo/code/project/omon-gateway`. The database is `omon_gateway.db` in that directory, which is `~/code/project/omon-gateway/omon_gateway.db`. It is not `~/.omon/gateway.db`.

Discovery order, for `ferryx hub migrate` with no `--from-omo` and for `scripts/hub-cutover.sh`:

1. `~/Library/LaunchAgents/ai.omon.gateway.plist` key `WorkingDirectory`, plus `/omon_gateway.db`. The script reads that one key with `plutil -extract`. It does not print the plist.
2. `~/code/project/omon-gateway/omon_gateway.db`
3. `~/.omon/gateway.db`, only if that file is present

A dry-run against the live file should report:

| row | count |
| --- | --- |
| cron_jobs_enabled | 16 |
| lanes | 28 |
| lane_threads | 4 |
| issues | 83 |
| activity | 467 |
| inbox_items | 3343 |
| proposals | 5781 |
| bot_profiles | 4 |
| allowlist | 5 |

`cron_jobs` is every row. `cron_jobs_enabled` is `enabled = 1`. `lane_threads` is `lane_thread_bindings`.

## Before

1. Build a ferryx binary whose default features include `hub-slack`, `hub-discord`, and `hub-voice`.
2. Confirm `~/.omon/secrets/` still holds the bot tokens. The hub reads `NAME_FILE`, then `NAME`, then those files. Do not print them.
3. Dry-run the copy. It prints row counts and writes nothing:

```bash
ferryx hub migrate --dry-run
```

## Cutover

`scripts/hub-cutover.sh` refuses to run without `--yes`. On macOS it:

1. Boots out `ai.omon.gateway-watchdog` and `ai.omon.gateway`.
2. Waits until that gateway process is gone.
3. Runs `ferryx hub migrate` (idempotent) against the discovered database.
4. Sends `ferryx hub start` on the control socket. It does not `kickstart -k` the daemon.

```bash
scripts/hub-cutover.sh cutover --ferryx /path/to/ferryx --yes
```

`--db PATH` overrides discovery. `--dry-run` stops after the row-count report. Then check `http://127.0.0.1:9119/api/health`.

`scripts/launchd/com.ferryx.hub-watchdog.plist` runs `scripts/hub-watchdog.sh` every 60 seconds. The template has no secrets. A Slack alert is sent only when `WATCHDOG_SLACK_TOKEN_FILE` and `WATCHDOG_SLACK_CHANNEL` are set in the environment.

## Rollback

Rollback stops the hub through the control socket, then bootstraps the omo launch agents. The ferryx daemon is not restarted.

```bash
scripts/hub-cutover.sh rollback --yes
```

If `ferryx hub stop` fails, the script does not start omo and does not restart the daemon. Leave the ferryx hub off until omo's Socket Mode connection is the only one. A daemon that was started with `FERRYX_HUB=off` never bound the control socket; stopping that hub would require a daemon restart, and that restart drops PTY sessions. The normal daemon start binds the socket, so rollback does not need that restart.
