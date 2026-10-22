# omo-gateway to ferryx hub cutover

The ferryx daemon starts the hub. Slack, Discord, and voice stay idle until a token or speech key is present. The dashboard listens on `127.0.0.1:9119` and serves `~/.omon/web` plus `/api/health`, `/api/tracker`, cron, approvals, and logs.

Two Socket Mode connections must not overlap. Unload `ai.omon.gateway` and `ai.omon.gateway-watchdog` before the ferryx hub opens Slack.

## Before

1. Build a ferryx binary whose default features include `hub-slack`, `hub-discord`, and `hub-voice`.
2. Confirm `~/.omon/secrets/` still holds the bot tokens. The hub reads `NAME_FILE`, then `NAME`, then those files. Do not print them.
3. Dry-run the copy. It prints row counts and writes nothing:

```bash
ferryx hub migrate --from-omo ~/.omon/gateway.db --dry-run
```

## Cutover

`scripts/hub-cutover.sh` refuses to run without `--yes`. On macOS it:

1. Boots out `ai.omon.gateway-watchdog` and `ai.omon.gateway`.
2. Waits until that gateway process is gone.
3. Runs `ferryx hub migrate --from-omo <db>` (idempotent).
4. Kickstarts `com.ferryx.daemon` when that label is already loaded. It does not install a second daemon.

```bash
scripts/hub-cutover.sh cutover --db ~/.omon/gateway.db --ferryx /path/to/ferryx --yes
```

`--dry-run` stops after the row-count report. Then check `http://127.0.0.1:9119/api/health`.

`scripts/launchd/com.ferryx.hub-watchdog.plist` runs `scripts/hub-watchdog.sh` every 60 seconds. The template has no secrets. A Slack alert is sent only when `WATCHDOG_SLACK_TOKEN_FILE` and `WATCHDOG_SLACK_CHANNEL` are set in the environment.

## Rollback

Rollback restarts the ferryx daemon with `FERRYX_HUB=off` and bootstraps the omo launch agents again. Restarting the daemon drops live PTY sessions.

```bash
scripts/hub-cutover.sh rollback --yes
```

Leave the ferryx hub off until omo's Socket Mode connection is the only one.
