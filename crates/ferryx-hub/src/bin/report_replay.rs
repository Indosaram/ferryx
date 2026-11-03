//! Replays audited messages through production constructors and the durable report gate.
//! Usage: report_replay <rows.jsonl> [provenance.jsonl] [prehistory.jsonl]. Provenance is keyed by Slack ts,
//! cites producer evidence, and contains producer inputs, never expected verdicts.
//! Each report is admitted under the session key its runtime producer uses. Cron salience and
//! same-turn novelty use the runtime judge (`Judge::from_config`); an unavailable judge falls
//! back exactly as the runtime does. Missing producer evidence makes the run fail.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

use ferryx_hub::chat::session::SessionKey;
use ferryx_hub::judge::Judge;
use ferryx_hub::report_gate::{self, Report};
use serde::Deserialize;
use serde_json::{json, Value};

type BoxError = Box<dyn std::error::Error>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    ts: String,
    evidence: String,
    producer: Producer,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prehistory {
    provenance: Provenance,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EarlierMessage {
    id: String,
    author: Option<String>,
    snippet: String,
    received_at: String,
}

/// Inbox item an urgent-lane turn cites; seeded so `delivery_report` resolves its source.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxSource {
    item_id: String,
    source: String,
    author: Option<String>,
}

/// A recorded `cron_runs` row.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedRun {
    run_id: String,
    status: String,
    output: Option<String>,
    error: Option<String>,
}

/// A recorded cron failure (`cron_problems` row and its job) to remediate again.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedProblem {
    owner: SessionKey,
    home: SessionKey,
    problem_id: i64,
    job_id: String,
    /// `cron_runs.id` of the failing run.
    run_id: String,
    cause: String,
    payload: Value,
    /// The job's next recorded run, standing in for the rerun remediation would start.
    rerun: Option<RecordedRun>,
}

/// Remediation's hub-facing effects for a replay: reruns answer with the recorded next run,
/// lanes are not opened, and notices go through the real runtime's gate without a transport.
struct RecordedRuntime {
    hub: ferryx_hub::cron_remediate::HubRuntime,
    rerun: Option<ferryx_hub::schedule::RunReceipt>,
}

#[async_trait::async_trait]
impl ferryx_hub::cron_remediate::Runtime for RecordedRuntime {
    async fn open_lane(&self, _request: ferryx_hub::cron_remediate::LaneRequest) -> ferryx_hub::Result<()> {
        Ok(())
    }
    async fn rerun(&self, job: &str, _approved: bool) -> ferryx_hub::Result<ferryx_hub::schedule::RunReceipt> {
        self.rerun.clone().ok_or_else(|| ferryx_hub::HubError::message(format!("no recorded rerun of {job}")))
    }
    async fn quota_check(&self) -> ferryx_hub::Result<Value> {
        Err(ferryx_hub::HubError::message("quota is not recorded"))
    }
    async fn log_excerpt(&self, _job: &str) -> ferryx_hub::Result<String> {
        Ok("(replay: no gateway log)".into())
    }
    async fn outcome(&self, problem: &ferryx_hub::cron_remediate::Problem, message: &str) -> ferryx_hub::Result<()> {
        self.hub.outcome(problem, message).await
    }
    async fn owner_notice(&self, problem: &ferryx_hub::cron_remediate::Problem, message: &str) -> ferryx_hub::Result<()> {
        self.hub.owner_notice(problem, message).await
    }
}

/// Runs one recorded cron failure through today's remediation and reports the gate decision of
/// the notice it produced, or `withheld` when it told the owner nothing.
async fn remediate(pool: &sqlx::SqlitePool, judge: &Judge, recorded: RecordedProblem)
    -> Result<(String, Option<i64>), BoxError> {
    use ferryx_hub::cron_remediate::{HubRuntime, Remediator};
    let RecordedProblem { owner, home, problem_id, job_id, run_id, cause, payload, rerun } = recorded;
    let job_id = job_id.as_str();
    sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES(?, 'every 20m', ?, 1)
                 ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json")
        .bind(job_id).bind(payload.to_string()).execute(pool).await?;
    let now = chrono::Utc::now().to_rfc3339();
    // A problem the scheduler reopens for a later failing run keeps its id (`record_problem`).
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at)
                 VALUES(?, ?, ?, ?, 'open', 'failed', ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET status = 'open', summary = excluded.summary, signature = excluded.signature,
                   detail = excluded.detail, run_id = excluded.run_id, lane_key = NULL, updated_at = excluded.updated_at")
        .bind(problem_id.to_string()).bind(job_id).bind(&cause).bind(&now)
        .bind(ferryx_hub::schedule::problem_signature("failed", &cause)).bind(&cause).bind(&run_id).bind(&now)
        .execute(pool).await?;
    let mut hub = HubRuntime::new(pool.clone(), Some(owner.storage_key()));
    hub.home_session = Some(home.storage_key());
    hub.transport = false;
    let runtime = RecordedRuntime {
        hub,
        rerun: rerun.map(|run| ferryx_hub::schedule::RunReceipt {
            job_id: job_id.to_owned(), run_id: run.run_id, status: run.status,
            output: run.output, last_delivery_error: None, error: run.error,
        }),
    };
    let cursor: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM report_suppressions")
        .fetch_one(pool).await?;
    let remediator = Remediator { pool: pool.clone(), judge: judge.clone(), runtime: std::sync::Arc::new(runtime) };
    remediator.on_problem(problem_id, &owner.storage_key()).await?;
    let decisions: Vec<(String, String)> = sqlx::query_as(
        "SELECT decision, report_id FROM report_suppressions WHERE id > ? ORDER BY id")
        .bind(cursor).fetch_all(pool).await?;
    match decisions.as_slice() {
        [] => Ok(("withheld".to_owned(), None)),
        [(decision, report)] => {
            let outbox: Option<i64> = sqlx::query_scalar(
                "SELECT outbox_id FROM report_delivery_parts WHERE json_extract(report_json, '$.id') = ?")
                .bind(report).fetch_optional(pool).await?;
            Ok((decision.clone(), outbox))
        }
        many => Err(format!("remediation produced {} owner notices", many.len()).into()),
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Producer {
    ContentAttachment {
        parent_ts: String,
    },
    OwnerAlert {
        session: SessionKey,
        item_id: String,
        source: String,
        author: Option<String>,
        title: Option<String>,
        snippet: String,
        link: Option<String>,
        received_at: String,
        thread_ref: Option<String>,
        earlier: Vec<EarlierMessage>,
    },
    Cron {
        job_id: String,
        payload: Value,
    },
    SlackTurn {
        session: SessionKey,
        trigger: String,
        status_ts: Option<String>,
        background: bool,
        /// Required: whether the audited text was the turn's final answer. Never inferred from text.
        final_answer: bool,
        #[serde(default)]
        inbox_item: Option<InboxSource>,
    },
    TurnStatus {
        session: SessionKey,
    },
    CronRemediation {
        session: SessionKey,
        problem_id: i64,
        job_id: String,
        problem_kind: String,
        detail: String,
        problem_status: String,
        lane_key: Option<String>,
        recovered: bool,
        message: String,
        /// `cron_problems.run_id`: the failing run the remediation reports on.
        #[serde(default)]
        run_id: Option<String>,
    },
    /// A recorded cron failure run through the current remediation (`Remediator::on_problem`):
    /// cause judgment, rerun verification against the recorded next run, and the runtime's own
    /// owner/home routing. What it reports, if anything, is decided by today's code, not by the
    /// historical message.
    CronProblem(RecordedProblem),
    SessionWatch {
        session: SessionKey,
        dedupe_key: String,
        target: String,
        notice_kind: String,
        notice_text: String,
    },
    External {
        session: SessionKey,
        request: ferryx_hub::control::ReportRequest,
    },
}

fn outcome(decision: &str) -> &'static str {
    match decision {
        "sent" => "keep",
        "digest" => "merge",
        "duplicate" => "suppress_duplicate",
        // `withheld`: the producer reported nothing to the owner.
        "internal" | "muted" | "withheld" => "suppress_internal",
        "empty" => "suppress_empty",
        "stale" => "suppress_stale",
        _ => "unclassified",
    }
}

/// Builds the report and its gate session the way each runtime producer does, then admits it.
async fn admit(pool: &sqlx::SqlitePool, judge: &Judge, producer: Producer, text: &str, at: chrono::DateTime<chrono::Utc>) -> Result<(String, Option<i64>), BoxError> {
    let (session, report): (String, Report) = match producer {
        Producer::CronProblem(problem) => return remediate(pool, judge, problem).await,
        Producer::ContentAttachment { .. } => return Err("attachments require an audited parent result".into()),
        Producer::External { session, request } => {
            let reply = ferryx_hub::control::report_reply(pool, &session.storage_key(), &request).await;
            return Ok((reply.get("decision").and_then(Value::as_str)
                .ok_or_else(|| format!("control admission failed: {reply}"))?.to_owned(),
                reply.get("outbox_id").and_then(Value::as_i64)));
        }
        Producer::OwnerAlert { session, item_id, source, author, title, snippet, link, received_at, thread_ref, earlier } => {
            let inbox = ferryx_hub::storage::memory().await?;
            for previous in earlier {
                ferryx_hub::inbox::ingest(&inbox, &previous.id, &source, &previous.id, &previous.snippet, &previous.received_at).await?;
                sqlx::query("UPDATE inbox_items SET author=?,thread_ref=? WHERE id=?")
                    .bind(&previous.author).bind(&thread_ref).bind(&previous.id).execute(&inbox).await?;
                // Context rows are not candidate alerts in this replay.
                ferryx_hub::inbox::decide(&inbox, &previous.id, ferryx_hub::inbox::Class::Ignore, "context only", None).await?;
            }
            ferryx_hub::inbox::ingest(&inbox, &item_id, &source, &item_id, &snippet, &received_at).await?;
            sqlx::query("UPDATE inbox_items SET author=?,title=?,thread_ref=?,link=? WHERE id=?")
                .bind(&author).bind(&title).bind(&thread_ref).bind(&link).bind(&item_id).execute(&inbox).await?;
            ferryx_hub::inbox::classify_pending(&inbox, judge, at, &session.storage_key(), |_, _| async { Ok(None) }).await?;
            let class: String = sqlx::query_scalar("SELECT class FROM inbox_decisions WHERE item_id=?")
                .bind(&item_id).fetch_one(&inbox).await?;
            match class.as_str() {
                "fyi" => return Ok(("digest".to_owned(), None)),
                "ignore" | "bot" => return Ok(("internal".to_owned(), None)),
                "owner" => {},
                _ => return Err(format!("unexpected inbox class: {class}").into()),
            }
            (session.storage_key(), ferryx_hub::owner_alert::alert_report(&ferryx_hub::owner_alert::PendingAlert {
                item_id, source, author, title, snippet, link,
            }))
        }
        // schedule::deliver_run_output
        Producer::Cron { job_id, payload } => {
            let targets = ferryx_hub::schedule::delivery_targets(&payload);
            let [target] = targets.as_slice() else {
                return Err(format!("cron {job_id}: expected one delivery target, got {}", targets.len()).into());
            };
            if target.platform != "slack" {
                return Err(format!("cron {job_id}: non-slack target {}", target.platform).into());
            }
            let mut report = ferryx_hub::schedule::cron_report(&job_id, &payload, text);
            if payload.get("report_salience").is_none()
                && !report_gate::is_empty_body(&report.body)
                && !report_gate::is_leak(&report.body)
            {
                match report_gate::classify_salience(judge, &report).await {
                    Ok(salience) => report.salience = salience,
                    Err(error) => eprintln!("cron salience unavailable for {job_id}; preserving owner report: {error}"),
                }
            }
            report.topic_key = format!("{}:{}:{}:{}", report.topic_key, target.platform,
                target.chat_id, target.thread_id.as_deref().unwrap_or(""));
            let session = SessionKey::new("slack", None::<String>, &target.chat_id, target.thread_id.clone(), "cron");
            (session.storage_key(), report)
        }
        // slack_turn::emit_result
        Producer::SlackTurn { session, trigger, status_ts, background, final_answer, inbox_item } => {
            #[cfg(feature = "hub-slack")]
            {
                if let Some(item) = inbox_item {
                    ferryx_hub::inbox::ingest(pool, &item.item_id, &item.source, &item.item_id, "", &at.to_rfc3339()).await?;
                    sqlx::query("UPDATE inbox_items SET author=? WHERE id=?")
                        .bind(&item.author).bind(&item.item_id).execute(pool).await?;
                }
                let gate_session = SessionKey::new("slack", None::<String>, session.channel_id.clone(),
                    session.thread_id.clone(), "owner").storage_key();
                let turn = ferryx_hub::chat::slack_turn::SessionTurn {
                    reply_channel: Some(session.channel_id.clone()),
                    reply_thread: session.thread_id.clone(),
                    key: session, text: trigger, status_ts, background,
                };
                let owner = turn.key.user_id.clone();
                // Production tracks commitments only on final replies, before the gate.
                let commitments = if final_answer {
                    ferryx_hub::chat::slack_turn::track_reply_commitments(pool, judge, &owner, &turn, text, at).await?
                } else {
                    ferryx_hub::chat::slack_turn::Commitments::default()
                };
                let mut report = ferryx_hub::chat::slack_turn::delivery_report(pool, &turn, text, final_answer).await?;
                ferryx_hub::chat::slack_turn::apply_commitments(&mut report, commitments);
                if let Err(error) = report_gate::consolidate_turn_report(pool, judge, &gate_session, &mut report).await {
                    eprintln!("turn report comparison unavailable; preserving new report: {error}");
                }
                (gate_session, report)
            }
            #[cfg(not(feature = "hub-slack"))]
            {
                let _ = (session, trigger, status_ts, background, final_answer);
                if let Some(InboxSource { item_id, source, author }) = inbox_item {
                    let _ = (item_id, source, author);
                }
                return Err("Slack turn replay requires --features hub-slack".into());
            }
        }
        // turn_status::may_post identity; production final edits bypass the gate.
        Producer::TurnStatus { session } => {
            #[cfg(feature = "hub-slack")]
            {
                let key = session.storage_key();
                let report = ferryx_hub::chat::turn_status::status_report(&key, text);
                (key, report)
            }
            #[cfg(not(feature = "hub-slack"))]
            {
                let _ = session;
                return Err("turn status replay requires --features hub-slack".into());
            }
        }
        // cron_remediate::HubRuntime::post
        Producer::CronRemediation { session, problem_id, job_id, problem_kind, detail, problem_status, lane_key, recovered, message, run_id } => {
            let problem = ferryx_hub::cron_remediate::Problem {
                id: problem_id, job_id, kind: problem_kind, detail, status: problem_status, lane_key, run_id,
            };
            let message = ferryx_hub::cron_remediate::redact_token(&ferryx_hub::judge::scrub_for_external(&message));
            (session.storage_key(), ferryx_hub::cron_remediate::remediation_report(&problem, &message, recovered))
        }
        // hub::Hub::post_notice
        Producer::SessionWatch { session, dedupe_key, target, notice_kind, notice_text } => {
            let notice = ferryx_hub::sessions::watch::Notice {
                dedupe_key, target, kind: notice_kind, text: notice_text, approval: None,
            };
            (session.storage_key(), ferryx_hub::hub::notice_report(&notice))
        }
    };
    let (decision, outbox) = report_gate::queue(pool, &session, &report, false).await?;
    Ok((decision.as_str().to_owned(), outbox))
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: report_replay <rows.jsonl> [provenance.jsonl] [prehistory.jsonl]")?;
    let mut provenance = HashMap::new();
    if let Some(path) = args.next() {
        for line in BufReader::new(std::fs::File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            let entry: Provenance = serde_json::from_str(&line)?;
            if entry.evidence.trim().is_empty() { return Err("producer evidence is required".into()); }
            if provenance.insert(entry.ts.clone(), entry).is_some() {
                return Err("duplicate producer timestamp".into());
            }
        }
    }
    let pool = ferryx_hub::storage::memory().await?;
    let judge = Judge::from_config(ferryx_hub::judge::typesafe_from_env()).with_pool(pool.clone());
    let mut seeded_until = None;
    if let Some(path) = args.next() {
        for line in BufReader::new(std::fs::File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            let seed: Prehistory = serde_json::from_str(&line)?;
            let entry = seed.provenance;
            if entry.evidence.trim().is_empty() { return Err("prehistory evidence is required".into()); }
            let (seconds, _) = entry.ts.split_once('.').ok_or("invalid prehistory timestamp")?;
            let at = chrono::DateTime::from_timestamp(seconds.parse()?, 0).ok_or("invalid prehistory time")?;
            if seeded_until.is_some_and(|previous| at <= previous) {
                return Err("prehistory must be chronological".into());
            }
            let (decision, _) = admit(&pool, &judge, entry.producer, &seed.text, at).await?;
            eprintln!("seeded_report={}", json!({"ts":entry.ts,"decision":decision,"evidence":entry.evidence}));
            seeded_until = Some(at);
        }
    }
    let mut out = std::io::stdout();
    let mut missing = 0usize;
    let mut content_parents: HashMap<String, (Option<String>, String)> = HashMap::new();
    for line in BufReader::new(std::fs::File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let row: Value = serde_json::from_str(&line)?;
        let i = row.get("i").ok_or("audit row has no index")?;
        let ts = row.get("ts").and_then(Value::as_str).ok_or("audit row has no timestamp")?;
        let text = row.get("text").and_then(Value::as_str).ok_or("audit row has no text")?;
        let (seconds, _) = ts.split_once('.').ok_or("invalid Slack timestamp")?;
        let at = chrono::DateTime::from_timestamp(seconds.parse()?, 0).ok_or("invalid audit time")?;
        if seeded_until.is_some_and(|last| at <= last) {
            return Err("prehistory must precede every audit row".into());
        }
        if row.get("sender").and_then(Value::as_str) == Some("일시불") {
            writeln!(out, "{}", json!({"i":i,"outcome":"owner"}))?;
            continue;
        }
        if let Some(sender @ ("GeekNews" | "Instinct")) = row.get("sender").and_then(Value::as_str) {
            writeln!(out, "{}", json!({"i":i,"outcome":"external_bot","sender":sender}))?;
            continue;
        }
        let Some(entry) = provenance.remove(ts) else {
            missing += 1;
            writeln!(out, "{}", json!({"i":i,"outcome":"unclassified","reason":"missing producer evidence"}))?;
            continue;
        };
        let judge_cursor: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(rowid), 0) FROM judge_calls")
            .fetch_one(&pool).await?;
        if let Producer::ContentAttachment { parent_ts } = &entry.producer {
            let channel = row.get("channel").and_then(Value::as_str);
            let parent = content_parents.get(parent_ts);
            let suppressed = parent.filter(|(parent_channel, decision)| {
                channel.is_some() && parent_channel.as_deref() == channel
                    && row.get("thread_ts").and_then(Value::as_str) == Some(parent_ts.as_str())
                    && matches!(decision.as_str(), "duplicate" | "muted" | "internal" | "empty" | "stale")
            });
            if let Some((_, decision)) = suppressed {
                writeln!(out, "{}", json!({"i":i,"outcome":"parent_suppressed",
                    "parent_ts":parent_ts,"parent_decision":decision,"evidence":entry.evidence}))?;
            } else {
                missing += 1;
                writeln!(out, "{}", json!({"i":i,"outcome":"unclassified",
                    "reason":"attachment requires a suppressed content parent in the same channel and thread",
                    "evidence":entry.evidence}))?;
            }
            continue;
        }
        let content_parent = matches!(&entry.producer, Producer::External { request, .. }
            if request.source == "content-intel-publish-ready");
        let admission = admit(&pool, &judge, entry.producer, text, at).await;
        let calls: Vec<(String, String, bool)> = sqlx::query_as(
            "SELECT purpose, outcome, used_fallback FROM judge_calls WHERE rowid > ? ORDER BY rowid",
        ).bind(judge_cursor).fetch_all(&pool).await?;
        for (purpose, outcome, used_fallback) in calls {
            eprintln!("row_judge_call={}", json!({"i":i,"ts":ts,"purpose":purpose,
                "outcome":outcome,"used_fallback":used_fallback}));
        }
        let (decision, outbox) = match admission {
            Ok(decision) => decision,
            Err(error) => {
                missing += 1;
                writeln!(out, "{}", json!({"i":i,"outcome":"unclassified","reason":error.to_string(),"evidence":entry.evidence}))?;
                continue;
            }
        };
        if content_parent {
            content_parents.insert(ts.to_owned(), (row.get("channel").and_then(Value::as_str).map(str::to_owned), decision.clone()));
        }
        let delivery_policy = if let Some(id) = outbox {
            if report_gate::prepare_delivery_at(&pool, id, at).await?.is_some() {
                "ready"
            } else {
                let cancelled: Option<String> = sqlx::query_scalar("SELECT cancelled_at FROM chat_outbox WHERE id=?")
                    .bind(id).fetch_one(&pool).await?;
                if cancelled.is_some() { "cancelled" } else { "deferred" }
            }
        } else { "not_queued" };
        writeln!(out, "{}", json!({"i":i,"outcome":outcome(&decision),"decision":decision,
            "stage":"delivery_policy","delivery_policy":delivery_policy,
            "delivery_checked":outbox.is_some(),"transport_executed":false,"evidence":entry.evidence}))?;
    }
    let decisions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_suppressions").fetch_one(&pool).await?;
    let judge_calls: Vec<(String, String, String, bool, i64)> = sqlx::query_as(
        "SELECT purpose, kind, outcome, used_fallback, COUNT(*) FROM judge_calls GROUP BY purpose, kind, outcome, used_fallback ORDER BY purpose, kind, outcome",
    ).fetch_all(&pool).await?;
    for (purpose, kind, outcome, used_fallback, count) in judge_calls {
        eprintln!("judge_call={}", json!({"purpose":purpose,"kind":kind,"outcome":outcome,"used_fallback":used_fallback,"count":count}));
    }
    eprintln!("persisted_decisions={decisions} missing_provenance={missing} unused_provenance={}", provenance.len());
    if missing != 0 || !provenance.is_empty() {
        return Err("replay coverage incomplete".into());
    }
    Ok(())
}
