# omo-gateway → ferryx-hub parity (audited 2026-10-23 KST)

Spec: `Indosaram/omo-gateway` (code + tests). Target: `ferryx-hub` on main `e50bb6d0` (1022.8, PR #19) plus branch `parity/omo-gateway`.
Live evidence: Mac ferryx hub (cutover at 09:36 KST), read-only checks. Counts only; no private message content.
Status: **ok** = matches omo · **fixed** = was missing or broken, fixed on this branch · **missing** / **partial** = still open (reason given) · **other** = owned by the 1022.8 / 1022.7 work.

| feature | omo location | ferryx status | evidence |
|---|---|---|---|
| Slack message → omo turn | slack/ingress.rs, agent/omo_backend.rs | other (1022.8, merged #19) | chat/slack_turn.rs + meta/omo_client.rs |
| `/omo-help`, `/omo-yolo`, `/omo-cron`, `/omo-huddle`, `/omo-sessions`, `/omo-reset` | slack/commands.rs | ok | chat/slash.rs tests `yolo_cron_stop_and_huddle_follow_the_gateway`, `a_slash_command_frame_runs_omo_yolo` |
| `/omo-thread` (create a named thread + starter turn) | slack/commands.rs (`THREAD_ROOT_PREFIX`, `ensure_session`) | **fixed** (was: echoed text only) | posts the root, stores the session, sends the permalink, runs the starter turn. e2e `e2e_omo_thread_creates_a_thread_and_omo_title_renames_it` (mock Slack) |
| `/omo-title` (rename a thread made by `/omo-thread`) | slack/commands.rs | **fixed** (was: echoed text only) | checks the root's author and prefix via conversations.replies, then chat.update. Same e2e test + `thread_and_title_refuse_dms_nested_threads_and_unnamed_threads` |
| `/omo-stop` | slack/commands.rs → multiplexer stop → `turn/interrupt` | **fixed** (was: sent Stop to a meta pane that no longer exists after 1022.8) | new chat/turns.rs registry; omo_client cancels on stop and sends `turn/interrupt`. With no thread given, picks the single live thread in the channel, as omo does. e2e `e2e_a_live_omo_turn_is_steered_then_stopped` (mock app-server) + `a_top_level_stop_picks_the_one_live_thread_in_the_channel` |
| `/omo-steer` | slack/commands.rs → `turn/steer` (`expectedTurnId`) | **fixed** (was: typed into the meta pane) | `omo_client::steer` sends the same frame as omo_protocol::turn_steer_request. With no live turn, queues a `[Steering]` turn. Same e2e test |
| Approval buttons (once / session / always / deny) and app-server approval policy | slack/interactive.rs, agent/omo_protocol.rs | ok / other | chat/approval.rs, chat/buttons.rs; omo_client policy (full access, APPROVAL_MODE, 5-denial limit) from 1022.8 |
| `requestUserInput` questions | agent/omo_protocol.rs `user_input_cancellation` | ok (same as omo: cancelled mid-turn) | meta/omo_client.rs header |
| Inbox sources: mail, iMessage, Kakao, GitHub, Slack, Discord | inbox/sources/* | ok / other (decoders: 1022.8 task) | inbox_sources.rs, inbox_watch.rs |
| Triage → owner items → action loop | inbox/triage.rs, autonomy/loop.rs | ok | autonomy.rs; live: action loop running |
| Owner alerts (📬 DM for owner items) | inbox/owner_alert.rs, slack/home.rs owner DM | **fixed** (was broken) | live: 1 alert stuck under key `meta` (not a SessionKey), 2 obligations marked delivered without a send. Now: obligation pending → outbox → Slack post → delivered; `repair_misrouted` re-addresses old rows; retried every 60s. e2e `e2e_an_owner_alert_is_posted_to_slack_and_then_delivered`, `alerts_queued_under_the_lead_lane_are_readdressed_and_reopened` |
| Outbox retry (undelivered replies) | slack/egress.rs | **fixed** (was: flushed only at boot) | `reply::flush_stale` every 60s from `owner_alert::serve_delivery`; one bad row no longer stops the rest |
| Lanes & threads (dispatch, guard, kibitzer, settle, signals) | lanes/* | ok | lanes/* with tests (18) |
| Cron scheduling, delivery, ack_command | cron/scheduler.rs, executor.rs, ack.rs | ok (claim/finish/status: 1022.7) | schedule.rs `DueDelivery.ack`; live: 60 runs since cutover, all succeeded |
| Dashboard-created or resumed cron jobs run | dashboard.rs cron CRUD → store.rs `next_run_at` | **fixed** (was: NULL `next_run_at`, never ran) | dashboard_api cron tests |
| Cron failure → owner (`needs_owner`) | cron/health.rs + remediate.rs `needs_owner` | **fixed** (was: incident row written, nobody told) | incident → open owner item `cron:<id>` → owner alert DM `📬 [크론] job — job: needs you - …`. Test in `a_due_job_is_claimed_once_and_a_failure_opens_an_incident` |
| Cron auto-remediation lane (judge classifies cause, retries once, opens a fix lane, owner-token approve-rerun) | cron/remediate.rs | partial | the owner is now told; the judge-driven lane is not ported (judge.rs belongs to the other task). Rerun endpoint still refuses publishing jobs |
| Dashboard / tracker API: every :9119 route used by the web bundle | dashboard.rs, tracker/api.rs | **fixed** (was: 404) | sessions/{id}, messages, ws, cron/jobs/{id} GET/PUT/DELETE, bots CRUD, logs/ws, status. 8 dashboard_api tests |
| :9119 Host / Origin guard | dashboard.rs `validate_host_and_origin` | **fixed** (was missing) | live before: `Host: evil.example` → 200, cross-origin POST → 200 (omo: 403). Test `the_guard_refuses_foreign_hosts_and_cross_site_writes` |
| Web UI static root (`DASHBOARD_WEB_ROOT`, ~/.omon/web) | dashboard_runtime.rs | **fixed** | `the_web_root_honors_the_omo_plist_name` |
| Dashboard stop (POST /stop, ws `stop`) | dashboard.rs `stop_session` | **fixed** (was: "chat runtime is not configured") | interrupts the session's live omo turn. Test `stop_interrupts_the_sessions_live_turn` |
| Dashboard-native chat (POST /chat, ws `message`) | dashboard.rs + multiplexer | missing (returns omo's own "chat runtime is not configured" 503) | needs a dashboard chat runtime; Slack turns are unaffected |
| Voice / huddle | voice/* | ok | voice.rs, voice_session.rs, discord_voice.rs (8 tests); `/omo-huddle` |
| Storage retention | storage/retention.rs | **fixed** (was missing) | retention.rs (`OMON_STORAGE_RETENTION_DAYS`=30, `CRON_RUNS_RETENTION_DAYS`=14), at boot then daily. 2 tests |
| Health / status / watchdog | readiness.rs, work_heartbeat.rs | ok (status/check/watchdog: PR #18) | `/api/health`, `ferryx hub status --check` |
| Full Disk Access / readiness probes | readiness.rs | partial | basic readiness only |
| Session & agent control (list, reset, stop, steer) | sessions/control.rs | ok after the stop/steer fix | agent_control.rs, slash.rs |
| Slack backfill (messages missed while the hub was down) | slack/backfill.rs | missing | messages sent during a hub restart are not replayed |
| Tracker requests sync | tracker/requests.rs | partial | tracker routes ok; requests sync not ported |
| Judge circuit breaker | judge/mod.rs | other (judge.rs owned by the 1022.8 task) | — |
| Env vars | plist / runtime_config.rs | partial | not read: `OMON_OMO_CRON_TURN_TOTAL_TIMEOUT_SECS`, `OMON_HOME_PLATFORM`, `DISCORD_AUTO_THREAD`, `DISCORD_BOT_TOKENS` (hub reading omo env config: 1022.7 scope) |
| Secrets | ~/.omon/secrets, plist | ok | read from the same places; never logged. Bot profile API keeps secrets (`bot_profiles_round_trip_without_leaking_or_losing_secrets`) |
