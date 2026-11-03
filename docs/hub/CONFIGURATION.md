# Hub configuration

The hub (`crates/ferryx-hub`, started by the Ferryx daemon) has no built-in owner, language, time
zone, model or personal URL. This page lists what a new owner sets, the default for each setting,
and what happens when it is unset.

## Where settings come from

Every setting is read through `hub_env::var`, in this order:

1. The daemon's own environment.
2. The hub env file `~/.ferryx/hub/env` (or the file `FERRYX_HUB_ENV_FILE` names): `KEY=VALUE`
   lines, mode 0600. `ferryx hub env import --from-plist <plist>` fills it from an omo-gateway
   launchd plist.
3. For the few settings that have one, the JSON config file at `FERRYX_HUB_CONFIG` (camelCase
   keys). The environment and the env file win over the file.

Secrets (`SLACK_BOT_TOKEN`, `SLACK_APP_TOKEN`, `OPENAI_API_KEY`) are also read from `NAME_FILE`
(a path; it wins over `NAME`) and then from `~/.ferryx/hub/secrets/<name>`.

## Minimal setup

```sh
# ~/.ferryx/hub/env
SLACK_BOT_TOKEN=xoxb-...
SLACK_APP_TOKEN=xapp-...
FERRYX_HUB_SLACK_OWNER=U0123456789
SLACK_HOME_CHANNEL=C0123456789
FERRYX_HUB_OWNER_LANGUAGE=en
FERRYX_HUB_OWNER_TIMEZONE=America/New_York
```

## Owner

| Setting | Config file key | Default | When unset |
|---|---|---|---|
| `FERRYX_HUB_SLACK_OWNER` | `ownerSlackId` | none | Falls back to the first `SLACK_ALLOWED_USERS` entry. With neither set, the hub runs with **no owner**. At load it logs the error `no hub owner is configured`. There is no owner DM, so the lead is `OMON_LANE_DEFAULT_LEAD` or the meta session. Nobody can press approval buttons. The hub never falls back to a hardcoded person. |
| `SLACK_ALLOWED_USERS` | - | empty | Comma or space separated Slack user ids allowed to talk to the bot and press buttons, in addition to the owner. Empty means only the owner. |
| `SLACK_HOME_CHANNEL` | `slackHomeChannel` | none | Informational notices (results, recovered cron jobs) go to the owner DM. The first id in a comma list is used. A leading `#` is dropped. The env value wins over the file. |
| `SLACK_FREE_RESPONSE_CHANNELS` | - | empty | Channels where the bot answers without a mention, in addition to the home channel. |
| `OMON_LANE_DEFAULT_LEAD` | - | the owner DM session | Overrides the lead session key that lane events, the action loop and owner alerts address. |
| `OMON_SLACK_PERSONA_FROM` | - | none | The omo Slack persona that the owner-DM lead runs as. Its memory repo is `bot-<persona>`. Unset, owner directives are read from the only `bot-*` memory repo, if there is exactly one. There is no built-in persona. |
| `OMON_OWNER_MEMORY_ROOT` | - | the lead bot's omo memory repo | Owner directives (standing preferences) are read from here. Without it the hub looks for `bot-<OMON_SLACK_PERSONA_FROM>` and then for the only `bot-*` repo under `~/.omo/memory/agents`. If neither is found, prompts carry no owner directives and a warning is logged. |

## Language and time zone

| Setting | Config file key | Default | When unset |
|---|---|---|---|
| `FERRYX_HUB_OWNER_LANGUAGE` | `ownerLanguage` | `LC_ALL`/`LANG`, then `en` | A BCP 47 tag (`ko`, `en-US`, `ja`). Built-in owner-facing texts exist in Korean and English. Any other language gets the English text. Model-written text (kibitzer nudges, briefs) is requested in the owner's language by name. |
| `FERRYX_HUB_OWNER_TIMEZONE` | `ownerTimezone` | `TZ`, then `/etc/localtime`, then UTC | An IANA zone name. Day boundaries follow it: daily judge caps, the usage digest's "yesterday", kibitzer wake caps, inbox filter stats and the breaker's one-alert-per-day rule. Quiet hours follow it too. Time labels (`14:02 KST`, `2026-10-21 19:00 CET`) use the zone's abbreviation. |
| `FERRYX_HUB_QUIET_HOURS` (or `OMON_SESSION_QUIET_HOURS`) | - | `0-8` | Owner-local hours during which session notices are held until the end hour. `off` disables them. The kibitzer uses the same 00:00-08:00 owner-local window. |

The locale is resolved once, the first time it is used. `HubConfig::load` runs at daemon
startup, before any owner-facing text is built. It copies `ownerLanguage`/`ownerTimezone` from the
config file into `FERRYX_HUB_OWNER_LANGUAGE`/`FERRYX_HUB_OWNER_TIMEZONE`, but only when neither the
environment nor the env file sets them. Edit a setting and restart the daemon to apply it.

## Models

| Setting | Default | When unset |
|---|---|---|
| `OMON_BACKGROUND_MODEL` | none | `provider/model` for every turn the hub starts by itself: cron agent jobs, lane events outside the owner DM, judge fallback and kibitzer turns, and the retry after a failed turn. Unset (or blank) means those turns run on the session's or engine's default model. No model is built in. |
| `FERRYX_HUB_MHC_DAILY_TURN_BUDGET` | none | An optional positive daily budget for supervisor engine turns. Unset means no budget. |

## Judge (typesafe-judge / jev)

| Setting | Default | When unset |
|---|---|---|
| `TYPESAFE_API_KEY`, `TYPESAFE_URL`, `TYPESAFE_MODEL` | none | Passed to the `typesafe-judge` binary (on `PATH` or `~/.cargo/bin`). Without them the judge fails and the hub falls back to one small-model JSON turn, up to `OMON_JUDGE_FALLBACK_DAILY_CAP` per day. After that it gives the conservative answer. The values come from the environment or env file, never from `~/.zshenv`. |
| `OMON_JEV_ENABLED` | on | Only `0`/`false`/`no` turn the judge off. |
| `OMON_JEV_DAILY_CAP` | `300` | Judge calls per owner-local day. |
| `OMON_JUDGE_FALLBACK_DAILY_CAP` | `50` | Fallback model turns per owner-local day. |

## Inbox and integrations

| Setting | Default | When unset |
|---|---|---|
| `OMON_INBOX_ENABLED` | on | `0`/`false`/`no` stops the inbox watchers. |
| `OMON_PIRATETALK_URL` | optional | Base URL of a PirateTalk (KakaoTalk bridge) server for the Kakao inbox source. Set it only if you run one. **Known gap:** `inbox_watch::fetch_kakao` still falls back to the original owner's server when this is unset. Until that fallback is removed, set the URL explicitly or disable the Kakao source. |
| `OMON_INBOX_KAKAO_ROOMS` | all rooms | Restricts the Kakao source to these rooms. |
| `FERRYX_HUB_QUOTA_URL` | `http://127.0.0.1:18801` (planned) | The model gateway's quota endpoint. Cron remediation checks it directly before it claims a quota problem. **Known gap:** `cron_remediate::HubRuntime` still hardcodes `http://127.0.0.1:18801` and does not read this variable yet. |
| `TRACKER_REQUESTERS` | none | Comma separated inbox author names whose requests are mirrored into labelled tracker issues. Unset means nothing is mirrored. |

## Notices and cron

| Setting | Default | When unset |
|---|---|---|
| `FERRYX_HUB_NOTIFY` (or `OMON_SESSION_NOTIFY`) | `actionable` | `all` also posts plain turn-complete and progress notices. |
| `FERRYX_HUB_NOTIFY_AFTER_SECS` (or `OMON_SESSION_NOTIFY_AFTER_SECS`) | `1800` | How long a session notice waits before it is posted. |
| `FERRYX_CRON_RUN_ID` | set by the hub | Not a setting. The hub exports it to every cron job script. A script that reports a failure to the hub should pass it as the report's `incident`. The hub's own remediation of that run is then recognised as the same incident and reported once. |

## Storage

| Setting | Config file key | Default |
|---|---|---|
| `FERRYX_HUB_DB` | `databasePath` | `~/.ferryx/hub/hub.sqlite` |
| `FERRYX_HUB_CONFIG` | - | none: the config file is optional |
| `FERRYX_HUB_ENV_FILE` | - | `~/.ferryx/hub/env` |
