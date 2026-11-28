//! One five-part report per settled goal, posted in the conversation the goal came from (todo 13).
//!
//! A goal that settles asks the owner something: it completed the work, it failed, or it needs
//! an answer only the owner can give. Whichever it is, the answer belongs where the request was
//! made, so [`report_settled`] reads the goal's run state and the origin of the request that
//! admitted it (adapter, channel binding, source message: recorded at admission in
//! `supervisor_requests`) and writes one report:
//!
//! - `desktop`, `phone`, `cli` and `mcp` origins (and a chat origin whose binding is gone) get
//!   the report in the app transcript, keyed [`report_key`] so a replay writes nothing. A
//!   `phone` origin also raises a [`ReportNotice`]; the daemon turns it into a push through
//!   [`super::push_policy::report_push`].
//! - `slack` and `discord` origins with a channel binding get the report queued into the same
//!   channel and thread (`report_gate::queue_delivery_in`) and posted through the owner alert
//!   outbox. The queue and its `supervisor_goal_reports` row commit together, so a restart never
//!   queues it twice. A crash between Slack accepting the post and the local receipt leaves the
//!   outbox row uncertain; [`reconcile_uncertain`] reads the thread (`conversations.replies`)
//!   and records the receipt if the report is there. It never posts the report again.
//!
//! The report has five parts: outcome, what changed, checks run, checks not run, next step.
//! `completed` is the outcome the owner asked for, `degraded` is the failure, `awaiting_owner`
//! is the question. Any other run state is not settled and reports nothing.
//!
//! Every line that looks like a credential ([`looks_like_secret`]) is dropped: a settle detail
//! can quote a log line, and a credential never reaches the owner's chat.
use std::str::FromStr;
use std::sync::OnceLock;

use sqlx::SqlitePool;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::memory_import::looks_like_secret;
use super::transcript::{self, Role};
use super::{ChannelProvider, RunState, SupervisorError};
use crate::report_gate::{Report, Salience};
use crate::storage::{now_rfc3339, WriteTx};

/// A report newly written for a goal that came from the phone: the daemon pushes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportNotice {
    pub goal_id: String,
    pub state: RunState,
    pub title: String,
}

fn notices() -> &'static broadcast::Sender<ReportNotice> {
    static NOTICES: OnceLock<broadcast::Sender<ReportNotice>> = OnceLock::new();
    NOTICES.get_or_init(|| broadcast::channel(64).0)
}

/// Receives the phone-origin reports written from now on.
pub fn subscribe() -> broadcast::Receiver<ReportNotice> {
    notices().subscribe()
}

/// The request id the report of `goal_id` at `revision` in `state` is stored under, and the
/// topic key of its chat delivery: one goal revision settling one way is reported once, however
/// many times it is called. An owner hold bumps the revision, so a later hold is a new report.
pub fn report_key(goal_id: &str, revision: i64, state: RunState) -> String {
    format!("goal-report:{goal_id}:r{revision}:{}", state.as_str())
}

/// Whether `state` is a settle the owner must hear about.
pub fn is_settled(state: RunState) -> bool {
    matches!(
        state,
        RunState::Completed | RunState::Degraded | RunState::AwaitingOwner
    )
}

/// The outcome word of a settle, as the owner reads it.
fn outcome(state: RunState) -> Option<&'static str> {
    match state {
        RunState::Completed => Some("completed"),
        RunState::Degraded => Some("failed"),
        RunState::AwaitingOwner => Some("needs owner"),
        _ => None,
    }
}

fn next_step(state: RunState) -> &'static str {
    match state {
        RunState::Completed => "Review the result; nothing else is pending.",
        RunState::Degraded => "Tell me to retry, or give a correction.",
        _ => "Answer the question above so the goal can continue.",
    }
}

/// The requirements of the goal's lane split into the checks that ran and those that did not.
async fn checks_of(
    pool: &SqlitePool,
    lane_key: &str,
) -> Result<(Vec<String>, Vec<String>), SupervisorError> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT req_id, text, verifier, status FROM lane_goal_requirements
         WHERE lane_key = ? ORDER BY rowid",
    )
    .bind(lane_key)
    .fetch_all(pool)
    .await?;
    let mut run = Vec::new();
    let mut not_run = Vec::new();
    for (id, text, verifier, status) in rows {
        let line = format!("{id} {status}: {text} (verifier: {verifier})");
        if status == "pending" {
            not_run.push(line);
        } else {
            run.push(line);
        }
    }
    Ok((run, not_run))
}

fn list_or_none(lines: &[String]) -> String {
    if lines.is_empty() {
        "none".to_owned()
    } else {
        lines.join("; ")
    }
}

/// Everything the five-part body is made of.
struct Facts<'a> {
    state: RunState,
    title: &'a str,
    detail: Option<&'a str>,
    change: Option<String>,
    run: Vec<String>,
    not_run: Vec<String>,
}

/// The report body: outcome headline, then the five parts, without any line that looks like a
/// credential. Empty when the state is not a settle.
fn body_of(facts: &Facts<'_>) -> String {
    let Some(word) = outcome(facts.state) else {
        return String::new();
    };
    let detail = facts
        .detail
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .unwrap_or("no detail was recorded");
    let change = facts
        .change
        .as_deref()
        .unwrap_or("Ferryx recorded no change for this goal");
    let text = format!(
        "{word}: {}\nOutcome: {detail}\nWhat changed: {change}\nChecks run: {}\nChecks not run: {}\nNext step: {}",
        facts.title,
        list_or_none(&facts.run),
        list_or_none(&facts.not_run),
        next_step(facts.state),
    );
    text.lines()
        .filter(|line| !looks_like_secret(line))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// Salience of the report: a question is the owner's to act on, an outcome is the result of what
/// he asked for.
fn salience_of(state: RunState) -> Salience {
    if state == RunState::AwaitingOwner {
        Salience::Action
    } else {
        Salience::Result
    }
}

/// The conversation lane of a bound chat origin. The sender component is the owner's provider
/// id (the last component of the principal), so the report lands in the same lane as the
/// inbound message it answers.
fn session_of(
    provider: ChannelProvider,
    channel_id: &str,
    thread_id: &str,
    owner_principal: &str,
) -> crate::chat::session::SessionKey {
    use crate::chat::session::SessionKey;
    let thread = (!thread_id.is_empty()).then(|| thread_id.to_owned());
    match provider {
        ChannelProvider::Discord => {
            SessionKey::new("discord", Some(channel_id), channel_id, thread, "")
        }
        _ => SessionKey::new(
            "slack",
            None::<String>,
            channel_id,
            thread,
            owner_principal.rsplit(':').next().unwrap_or(owner_principal),
        ),
    }
}

/// Report the settle of `goal_id` in the conversation it came from, once. Returns whether
/// something new was written; `false` when the goal is not settled, when it is already reported,
/// or when the report would be empty.
pub async fn report_settled(pool: &SqlitePool, goal_id: &str) -> Result<bool, SupervisorError> {
    let Some(goal) = super::goal_of(pool, goal_id).await? else {
        return Err(SupervisorError::NotFound {
            kind: "goal",
            id: goal_id.to_owned(),
        });
    };
    if !is_settled(goal.run_state) {
        return Ok(false);
    }
    let title: Option<String> = sqlx::query_scalar("SELECT title FROM lanes WHERE key = ?")
        .bind(&goal.lane_key)
        .fetch_optional(pool)
        .await?;
    let title = title.unwrap_or_else(|| goal.lane_key.clone());
    let pr: Option<(String, String)> =
        sqlx::query_as("SELECT url, state FROM lane_pull_requests WHERE lane_key = ?")
            .bind(&goal.lane_key)
            .fetch_optional(pool)
            .await?;
    let (run, not_run) = checks_of(pool, &goal.lane_key).await?;
    let body = body_of(&Facts {
        state: goal.run_state,
        title: &title,
        detail: goal.state_detail.as_deref(),
        change: pr.map(|(url, state)| format!("pull request {url} ({state})")),
        run,
        not_run,
    });
    if body.trim().is_empty() {
        return Ok(false);
    }
    let key = report_key(goal_id, goal.revision, goal.run_state);
    let origin: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT source_adapter, source_message_id, channel_binding_id
         FROM supervisor_requests WHERE goal_id = ? ORDER BY id LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(pool)
    .await?;
    let notice = ReportNotice {
        goal_id: goal_id.to_owned(),
        state: goal.run_state,
        title: title.clone(),
    };
    let Some((adapter, source_message_id, binding)) = origin else {
        // A goal with no request is not reachable from a conversation: report it in the app
        // transcript, which is always the owner's.
        return append_transcript(pool, &key, &body, None).await;
    };
    let binding = match binding {
        None => None,
        Some(binding_id) => sqlx::query_as::<_, (String, String, String)>(
            "SELECT provider, channel_id, thread_id FROM supervisor_channel_bindings
             WHERE binding_id = ?",
        )
        .bind(&binding_id)
        .fetch_optional(pool)
        .await?,
    };
    let provider = ChannelProvider::from_str(&adapter)?;
    let chat_origin = binding.filter(|_| {
        !matches!(
            provider,
            ChannelProvider::Desktop
                | ChannelProvider::Phone
                | ChannelProvider::Cli
                | ChannelProvider::Mcp
                | ChannelProvider::Import
        )
    });
    let Some((provider_text, channel_id, thread_id)) = chat_origin else {
        let push = (provider == ChannelProvider::Phone).then_some(notice);
        return append_transcript(pool, &key, &body, push).await;
    };
    let provider = ChannelProvider::from_str(&provider_text)?;
    let session = session_of(provider, &channel_id, &thread_id, &goal.owner_principal);
    let already: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_goal_reports
                       WHERE goal_id = ? AND revision = ? AND state = ?)",
    )
    .bind(goal_id)
    .bind(goal.revision)
    .bind(goal.run_state.as_str())
    .fetch_one(pool)
    .await?;
    if already {
        return Ok(false);
    }
    let mut report = Report::new(
        Uuid::new_v4().to_string(),
        "supervisor-goal",
        &key,
        salience_of(goal.run_state),
        &body,
    )
    .with_state(&format!(
        "{}:{}",
        goal.run_state.as_str(),
        goal.state_detail.as_deref().unwrap_or_default()
    ))
    .with_origin(&source_message_id);
    // A settle answers the request the owner made, so the gate must read it as a reply to him and
    // never as the hub's own unsolicited narration (which is withheld when it is not in the
    // owner's language).
    report.reply_to_owner = true;
    let mut tx = pool.begin_write().await?;
    let queued = crate::report_gate::queue_delivery_in(
        &mut tx,
        &session.storage_key(),
        &report,
        false,
        &body,
        None,
    )
    .await
    .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO supervisor_goal_reports(goal_id, revision, state, provider,
                channel_id, thread_id, origin_message_id, outbox_id, created_at)
         VALUES(?,?,?,?,?,?,?,?,?)",
    )
    .bind(goal_id)
    .bind(goal.revision)
    .bind(goal.run_state.as_str())
    .bind(provider.as_str())
    .bind(&channel_id)
    .bind(&thread_id)
    .bind(&source_message_id)
    .bind(queued.1)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    let Some(outbox_id) = queued.1.filter(|_| inserted == 1) else {
        return Ok(false);
    };
    crate::owner_alert::post_queued(pool, outbox_id, &session.storage_key(), &body)
        .await
        .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    Ok(true)
}

/// The report in the app transcript, idempotent on its request id. A report newly written for a
/// phone origin also raises its push notice.
async fn append_transcript(
    pool: &SqlitePool,
    key: &str,
    body: &str,
    push: Option<ReportNotice>,
) -> Result<bool, SupervisorError> {
    let (_, created) = transcript::append(pool, Role::Supervisor, body, &[], key, transcript::now_ms())
        .await
        .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    if let Some(notice) = push.filter(|_| created) {
        // No receiver is not an error: the push is best effort, the transcript row is the report.
        if notices().send(notice).is_err() {
            tracing::debug!("no push forwarder is listening for goal reports");
        }
    }
    Ok(created)
}

/// The messages of the Slack thread rooted at `thread`, as `(ts, text)`.
#[cfg(feature = "hub-slack")]
async fn thread_messages(channel: &str, thread: &str) -> crate::error::Result<Vec<(String, String)>> {
    let value = crate::chat::slack_web::post_form(
        "conversations.replies",
        &[("channel", channel), ("ts", thread), ("limit", "200")],
    )
    .await?;
    Ok(value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| {
                    let ts = message.get("ts")?.as_str()?.to_owned();
                    let text = message.get("text")?.as_str()?.to_owned();
                    Some((ts, text))
                })
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(not(feature = "hub-slack"))]
async fn thread_messages(_: &str, _: &str) -> crate::error::Result<Vec<(String, String)>> {
    Ok(Vec::new())
}

/// Settles the Slack reports whose post may have reached Slack without a recorded receipt (the
/// outbox row is uncertain, or its sender died mid-post) by reading the thread: when the report's
/// headline is in it, the receipt is recorded and the row is delivered. A report not found is
/// left uncertain; it is never posted again. Returns how many reports were found.
pub async fn reconcile_uncertain(pool: &SqlitePool) -> Result<usize, SupervisorError> {
    type Row = (String, i64, String, String, String, i64, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.goal_id, r.revision, r.state, r.channel_id, r.thread_id, o.id, o.text
           FROM supervisor_goal_reports r
           JOIN chat_outbox o ON o.id = r.outbox_id
          WHERE r.reconciled_at IS NULL AND r.provider = 'slack' AND r.thread_id != ''
            AND o.delivered_at IS NULL AND o.cancelled_at IS NULL
            AND (o.send_uncertain_at IS NOT NULL
                 OR (o.send_inflight IS NOT NULL
                     AND julianday(o.send_claim_until) <= julianday(?)))
          ORDER BY o.id",
    )
    .bind(now_rfc3339())
    .fetch_all(pool)
    .await?;
    let mut found = 0;
    for (goal_id, revision, state, channel, thread, outbox_id, text) in rows {
        let headline = text.lines().next().unwrap_or_default().trim().to_owned();
        let messages = match thread_messages(&channel, &thread).await {
            Ok(messages) => messages,
            Err(error) => {
                tracing::warn!(goal_id, %error, "the thread of an uncertain goal report was not read");
                continue;
            }
        };
        let Some((ts, _)) = messages
            .into_iter()
            .find(|(_, message)| !headline.is_empty() && message.replace('*', "").contains(&headline))
        else {
            continue;
        };
        let now = now_rfc3339();
        let mut tx = pool.begin_write().await?;
        sqlx::query(
            "UPDATE chat_outbox SET delivered_at = ?, posted_channel = ?, posted_ts = ?,
                    send_claim = NULL, send_claim_until = NULL, send_inflight = NULL
              WHERE id = ? AND delivered_at IS NULL",
        )
        .bind(&now)
        .bind(&channel)
        .bind(&ts)
        .bind(outbox_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE supervisor_goal_reports SET reconciled_at = ?, posted_ts = ?
              WHERE goal_id = ? AND revision = ? AND state = ?",
        )
        .bind(&now)
        .bind(&ts)
        .bind(&goal_id)
        .bind(revision)
        .bind(&state)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        found += 1;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::goals::contract::RequirementSpec;
    use crate::goals::types::GoalMode;
    use crate::supervisor::admission::OperationClass;
    use crate::supervisor::{
        admit_request_bound, bind_channel, bind_project_host, create_project, ensure_identity,
        transition_goal, AdmitRequest, AdmitTerms, ChannelAddress, ExecutionKind, ExecutionTarget,
    };

    const OWNER: &str = "slack:T1:U-owner";
    const HOST: &str = "host-a";
    const REPO: &str = "/home/owner/code/ferryx";

    struct Fixture {
        pool: SqlitePool,
        supervisor_id: String,
        project_id: String,
    }

    async fn fixture() -> Fixture {
        let pool = crate::storage::memory().await.unwrap();
        let identity = ensure_identity(&pool, OWNER).await.unwrap();
        let project = create_project(
            &pool,
            &identity.supervisor_id,
            "ferryx",
            "github.com/example/ferryx",
        )
        .await
        .unwrap();
        bind_project_host(
            &pool,
            &project.project_id,
            HOST,
            &format!("{REPO}/.git"),
        )
        .await
        .unwrap();
        Fixture {
            pool,
            supervisor_id: identity.supervisor_id,
            project_id: project.project_id,
        }
    }

    impl Fixture {
        fn request(&self, adapter: ChannelProvider, binding: Option<String>) -> AdmitRequest {
            AdmitRequest {
                supervisor_id: self.supervisor_id.clone(),
                owner_principal: OWNER.into(),
                source_adapter: adapter,
                source_message_id: format!("{}:goal-report-1", adapter.as_str()),
                body: "Finish the fetch cache".into(),
                project_id: self.project_id.clone(),
                channel_binding_id: binding,
                target: ExecutionTarget {
                    kind: ExecutionKind::Pty,
                    host_id: HOST.into(),
                },
                title: "Finish the fetch cache".into(),
                cwd: REPO.into(),
                instruction: "Finish the fetch cache and run the verifier.".into(),
                requirements: vec![RequirementSpec {
                    id: "r1".into(),
                    text: "the fetch cache is wired".into(),
                    verifier: "cargo test -p ferryx-hub passes".into(),
                }],
                mode: GoalMode::Supervised,
                turn_limit: 5,
                operation_grant: vec![OperationClass::Edit],
            }
        }

        async fn goal(&self, request: AdmitRequest) -> String {
            let admission = admit_request_bound(&self.pool, &request, &AdmitTerms::default(), None)
                .await
                .unwrap();
            admission.receipt.goal_id
        }
    }

    /// Moves the goal in the table the way a settle from the run would, without a live worker.
    async fn settle(pool: &SqlitePool, goal_id: &str, state: RunState, detail: Option<&str>) {
        sqlx::query("UPDATE supervisor_goals SET run_state = ?, state_detail = ? WHERE goal_id = ?")
            .bind(state.as_str())
            .bind(detail)
            .bind(goal_id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn transcripts(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_app_transcript")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn transcript_text(pool: &SqlitePool, request_id: &str) -> String {
        sqlx::query_scalar("SELECT text FROM supervisor_app_transcript WHERE request_id = ?")
            .bind(request_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_completed_desktop_goal_is_reported_in_the_app_transcript_once() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Desktop, None)).await;
        settle(
            &fx.pool,
            &goal_id,
            RunState::Completed,
            Some("the fetch cache is wired"),
        )
        .await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        assert_eq!(transcripts(&fx.pool).await, 1);
        let text = transcript_text(&fx.pool, &report_key(&goal_id, 1, RunState::Completed)).await;
        assert!(text.starts_with("completed: Finish the fetch cache\n"), "{text}");
        assert!(text.contains("the fetch cache is wired"), "{text}");
        // The same settle again writes nothing.
        assert!(!report_settled(&fx.pool, &goal_id).await.unwrap());
        assert_eq!(transcripts(&fx.pool).await, 1);
    }

    #[tokio::test]
    async fn a_degraded_phone_goal_is_reported_in_the_app_transcript() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Phone, None)).await;
        settle(&fx.pool, &goal_id, RunState::Degraded, Some("the verifier failed")).await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        assert_eq!(transcripts(&fx.pool).await, 1);
        let text = transcript_text(&fx.pool, &report_key(&goal_id, 1, RunState::Degraded)).await;
        assert!(text.starts_with("failed: Finish the fetch cache\n"), "{text}");
    }

    #[tokio::test]
    async fn an_awaiting_owner_goal_is_queued_into_its_slack_thread() {
        let fx = fixture().await;
        let address = ChannelAddress {
            provider: ChannelProvider::Slack,
            workspace_id: "T1".into(),
            account_id: "B1".into(),
            channel_id: "C1".into(),
            thread_id: "111.222".into(),
            owner_principal: OWNER.into(),
        };
        let binding = bind_channel(&fx.pool, &fx.supervisor_id, &address, None, None)
            .await
            .unwrap();
        let goal_id = fx
            .goal(fx.request(ChannelProvider::Slack, Some(binding.binding_id.clone())))
            .await;
        let goal = super::super::goal_of(&fx.pool, &goal_id).await.unwrap().unwrap();
        // The transition hook is what reports a settle, so the goal moves the real way.
        transition_goal(
            &fx.pool,
            &goal_id,
            goal.revision,
            goal.run_state,
            RunState::AwaitingOwner,
            Some("which project should this run in?"),
        )
        .await
        .unwrap();
        let session = session_of(ChannelProvider::Slack, "C1", "111.222", OWNER)
            .storage_key();
        let queued: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox WHERE session_key = ?")
                .bind(&session)
                .fetch_one(&fx.pool)
                .await
                .unwrap();
        assert_eq!(queued, 1);
        let text: String = sqlx::query_scalar("SELECT text FROM chat_outbox WHERE session_key = ?")
            .bind(&session)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert!(text.starts_with("needs owner: Finish the fetch cache\n"), "{text}");
        assert_eq!(transcripts(&fx.pool).await, 0);
        // Reporting it again finds the same topic in the same state and queues nothing.
        assert!(!report_settled(&fx.pool, &goal_id).await.unwrap());
    }

    #[tokio::test]
    async fn a_running_goal_reports_nothing() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Desktop, None)).await;
        settle(&fx.pool, &goal_id, RunState::Running, Some("working")).await;
        assert!(!report_settled(&fx.pool, &goal_id).await.unwrap());
        assert_eq!(transcripts(&fx.pool).await, 0);
    }

    #[tokio::test]
    async fn a_credential_line_never_reaches_the_report() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Desktop, None)).await;
        settle(
            &fx.pool,
            &goal_id,
            RunState::Degraded,
            Some("the verifier failed\n-----BEGIN PRIVATE KEY-----\nabc123"),
        )
        .await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        let text = transcript_text(&fx.pool, &report_key(&goal_id, 1, RunState::Degraded)).await;
        assert!(text.contains("the verifier failed"), "{text}");
        assert!(!text.contains("BEGIN"), "{text}");
    }

    async fn slack_goal(fx: &Fixture) -> (String, String) {
        let address = ChannelAddress {
            provider: ChannelProvider::Slack,
            workspace_id: "T1".into(),
            account_id: "B1".into(),
            channel_id: "C1".into(),
            thread_id: "111.222".into(),
            owner_principal: OWNER.into(),
        };
        let binding = bind_channel(&fx.pool, &fx.supervisor_id, &address, None, None)
            .await
            .unwrap();
        let goal_id = fx
            .goal(fx.request(ChannelProvider::Slack, Some(binding.binding_id)))
            .await;
        let session = session_of(ChannelProvider::Slack, "C1", "111.222", OWNER).storage_key();
        (goal_id, session)
    }

    async fn outbox_texts(pool: &SqlitePool, session: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT text FROM chat_outbox WHERE session_key = ? ORDER BY id")
            .bind(session)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_report_has_five_parts_and_unmet_requirements_are_checks_not_run() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Desktop, None)).await;
        settle(&fx.pool, &goal_id, RunState::Completed, Some("wired")).await;
        report_settled(&fx.pool, &goal_id).await.unwrap();
        let text = transcript_text(&fx.pool, &report_key(&goal_id, 1, RunState::Completed)).await;
        for part in ["Outcome: wired", "What changed:", "Checks run: none", "Checks not run: r1 pending", "Next step:"] {
            assert!(text.contains(part), "{part} missing from {text}");
        }
    }

    #[tokio::test]
    async fn a_phone_goal_reports_in_the_app_transcript_and_raises_one_push_notice() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Phone, None)).await;
        let mut notices = subscribe();
        settle(&fx.pool, &goal_id, RunState::Completed, Some("wired")).await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        assert!(!report_settled(&fx.pool, &goal_id).await.unwrap());
        let mut mine = Vec::new();
        while let Ok(notice) = notices.try_recv() {
            if notice.goal_id == goal_id {
                mine.push(notice);
            }
        }
        assert_eq!(mine.len(), 1, "one notice, none on replay: {mine:?}");
        assert_eq!(mine[0].state, RunState::Completed);
        assert_eq!(transcripts(&fx.pool).await, 1);
    }

    #[tokio::test]
    async fn a_desktop_goal_raises_no_push_notice() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Desktop, None)).await;
        let mut notices = subscribe();
        settle(&fx.pool, &goal_id, RunState::Degraded, Some("failed")).await;
        report_settled(&fx.pool, &goal_id).await.unwrap();
        while let Ok(notice) = notices.try_recv() {
            assert_ne!(notice.goal_id, goal_id);
        }
    }

    #[tokio::test]
    async fn each_settle_kind_of_a_slack_goal_is_queued_in_its_thread_once() {
        for (state, headline) in [
            (RunState::Completed, "completed: Finish the fetch cache"),
            (RunState::Degraded, "failed: Finish the fetch cache"),
        ] {
            let fx = fixture().await;
            let (goal_id, session) = slack_goal(&fx).await;
            settle(&fx.pool, &goal_id, state, Some("detail")).await;
            assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
            assert!(!report_settled(&fx.pool, &goal_id).await.unwrap(), "replay after a restart");
            let texts = outbox_texts(&fx.pool, &session).await;
            assert_eq!(texts.len(), 1, "{texts:?}");
            assert!(texts[0].starts_with(headline), "{}", texts[0]);
            assert_eq!(transcripts(&fx.pool).await, 0, "the report goes to the thread, not the app");
        }
    }

    #[tokio::test]
    async fn a_second_owner_hold_of_a_new_revision_is_a_new_report() {
        let fx = fixture().await;
        let (goal_id, session) = slack_goal(&fx).await;
        settle(&fx.pool, &goal_id, RunState::AwaitingOwner, Some("which project?")).await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        sqlx::query("UPDATE supervisor_goals SET revision = 3 WHERE goal_id = ?")
            .bind(&goal_id)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        assert_eq!(outbox_texts(&fx.pool, &session).await.len(), 2);
    }

    #[cfg(feature = "hub-slack")]
    async fn uncertain_report(fx: &Fixture) -> (String, i64) {
        let (goal_id, session) = slack_goal(fx).await;
        settle(&fx.pool, &goal_id, RunState::Completed, Some("wired")).await;
        report_settled(&fx.pool, &goal_id).await.unwrap();
        let outbox: i64 = sqlx::query_scalar("SELECT id FROM chat_outbox WHERE session_key = ?")
            .bind(&session)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        // Slack accepted the post, the hub died before it wrote the receipt.
        sqlx::query("UPDATE chat_outbox SET send_uncertain_at = ?, send_inflight = 0 WHERE id = ?")
            .bind(now_rfc3339())
            .bind(outbox)
            .execute(&fx.pool)
            .await
            .unwrap();
        (goal_id, outbox)
    }

    #[cfg(feature = "hub-slack")]
    fn posted(calls: &crate::chat::slack_web::mock::Calls) -> usize {
        calls.lock().unwrap().iter().filter(|(method, _)| method == "chat.postMessage").count()
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn an_uncertain_report_found_in_the_thread_is_recorded_and_never_reposted() {
        let fx = fixture().await;
        let (_, outbox) = uncertain_report(&fx).await;
        let replies = serde_json::json!({"ok": true, "messages": [
            {"ts": "111.222", "text": "Finish the fetch cache"},
            {"ts": "111.333", "text": "completed: Finish the fetch cache\nOutcome: wired"}]});
        let (base, calls) = crate::chat::slack_web::mock::serve_scripted("B1", vec![("conversations.replies", vec![replies])]).await;
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(reconcile_uncertain(&fx.pool).await.unwrap(), 1);
            assert_eq!(reconcile_uncertain(&fx.pool).await.unwrap(), 0);
        })
        .await;
        let (delivered, ts): (bool, Option<String>) =
            sqlx::query_as("SELECT delivered_at IS NOT NULL, posted_ts FROM chat_outbox WHERE id = ?")
                .bind(outbox)
                .fetch_one(&fx.pool)
                .await
                .unwrap();
        assert_eq!((delivered, ts.as_deref()), (true, Some("111.333")));
        assert_eq!(posted(&calls), 0, "reconciliation reads the thread and never posts");
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn an_uncertain_report_missing_from_the_thread_stays_uncertain_and_is_not_reposted() {
        let fx = fixture().await;
        let (_, outbox) = uncertain_report(&fx).await;
        let replies = serde_json::json!({"ok": true, "messages": [{"ts": "111.222", "text": "Finish the fetch cache"}]});
        let (base, calls) = crate::chat::slack_web::mock::serve_scripted("B1", vec![("conversations.replies", vec![replies])]).await;
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(reconcile_uncertain(&fx.pool).await.unwrap(), 0);
            let sent = crate::chat::reply::flush_stale(&fx.pool, chrono::Duration::zero(), chrono::Duration::days(1)).await.unwrap();
            assert_eq!(sent, 0, "an uncertain row is excluded from the retry");
        })
        .await;
        let status = crate::control::report_status_reply(&fx.pool, outbox).await;
        assert_eq!(status["delivery"], "uncertain");
        assert_eq!(posted(&calls), 0);
    }

    const FIVE_PARTS: [&str; 5] =
        ["Outcome: which project?", "What changed:", "Checks run:", "Checks not run:", "Next step:"];

    #[tokio::test]
    async fn an_app_needs_owner_settle_posts_one_five_part_report_and_a_replay_adds_none() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Desktop, None)).await;
        settle(&fx.pool, &goal_id, RunState::AwaitingOwner, Some("which project?")).await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        let text = transcript_text(&fx.pool, &report_key(&goal_id, 1, RunState::AwaitingOwner)).await;
        assert!(text.starts_with("needs owner: Finish the fetch cache\n"), "{text}");
        for part in FIVE_PARTS {
            assert!(text.contains(part), "{part} missing from {text}");
        }
        // The same settle called again, as after a restart, writes no second row.
        assert!(!report_settled(&fx.pool, &goal_id).await.unwrap());
        assert_eq!(transcripts(&fx.pool).await, 1);
    }

    #[tokio::test]
    async fn a_phone_needs_owner_settle_posts_one_report_and_one_push_notice_and_a_replay_adds_none() {
        let fx = fixture().await;
        let goal_id = fx.goal(fx.request(ChannelProvider::Phone, None)).await;
        let mut notices = subscribe();
        settle(&fx.pool, &goal_id, RunState::AwaitingOwner, Some("which project?")).await;
        assert!(report_settled(&fx.pool, &goal_id).await.unwrap());
        assert!(!report_settled(&fx.pool, &goal_id).await.unwrap());
        let text = transcript_text(&fx.pool, &report_key(&goal_id, 1, RunState::AwaitingOwner)).await;
        for part in FIVE_PARTS {
            assert!(text.contains(part), "{part} missing from {text}");
        }
        assert_eq!(transcripts(&fx.pool).await, 1);
        let mut mine = Vec::new();
        while let Ok(notice) = notices.try_recv() {
            if notice.goal_id == goal_id {
                mine.push(notice.state);
            }
        }
        assert_eq!(mine, vec![RunState::AwaitingOwner], "one push notice, none on replay");
    }
}
