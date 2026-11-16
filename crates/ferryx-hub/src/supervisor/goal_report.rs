//! One report per settled goal, posted in the conversation the goal came from (todo 13).
//!
//! A goal that settles asks the owner something: it completed the work, it failed, or it needs
//! an answer only the owner can give. Whichever it is, the answer belongs where the request was
//! made, so [`report_settled`] reads the goal's run state and the origin of the request that
//! admitted it and writes one report:
//!
//! - `desktop`, `phone`, `cli` and `mcp` origins (and a chat origin whose binding is gone) get
//!   the report in the app transcript, keyed [`report_key`] so a replay writes nothing. The
//!   phone's push notification rides the existing transcript push path; this module only writes
//!   the row that path delivers.
//! - `slack` and `discord` origins with a channel binding get the report queued into the same
//!   channel and thread (`report_gate::queue_delivery_in`), exactly like a task result
//!   ([`crate::hub`]'s origin reply), and posted through the owner alert outbox.
//!
//! `completed` is the outcome the owner asked for, `degraded` is the failure, `awaiting_owner`
//! is the question. Any other run state is not settled and reports nothing.
//!
//! The body is the outcome, the goal's title and its state detail, minus any line that looks
//! like a credential ([`looks_like_secret`]): a settle detail can quote a log line, and a
//! credential never reaches the owner's chat.
use std::str::FromStr;

use sqlx::SqlitePool;
use uuid::Uuid;

use super::memory_import::looks_like_secret;
use super::transcript::{self, Role};
use super::{ChannelProvider, RunState, SupervisorError};
use crate::report_gate::{Report, Salience};
use crate::storage::WriteTx;

/// The request id the report of `goal_id` in `state` is stored under, and the topic key of its
/// chat delivery: one goal settling one way is reported once, however many times it is called.
pub fn report_key(goal_id: &str, state: RunState) -> String {
    format!("goal-report:{goal_id}:{}", state.as_str())
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

/// The report body: outcome and title, then the state detail, without any line that looks like
/// a credential. Empty when nothing is left to say.
fn body_of(state: RunState, title: &str, detail: Option<&str>) -> String {
    let Some(outcome) = outcome(state) else {
        return String::new();
    };
    let text = match detail.map(str::trim).filter(|detail| !detail.is_empty()) {
        Some(detail) => format!("{outcome}: {title}\n{detail}"),
        None => format!("{outcome}: {title}"),
    };
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
    let body = body_of(goal.run_state, &title, goal.state_detail.as_deref());
    if body.trim().is_empty() {
        return Ok(false);
    }
    let key = report_key(goal_id, goal.run_state);
    let origin: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT source_adapter, source_message_id, channel_binding_id
         FROM supervisor_requests WHERE goal_id = ? ORDER BY id LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(pool)
    .await?;
    let Some((adapter, source_message_id, binding)) = origin else {
        // A goal with no request is not reachable from a conversation: report it in the app
        // transcript, which is always the owner's.
        return append_transcript(pool, &key, &body).await;
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
        return append_transcript(pool, &key, &body).await;
    };
    let provider = ChannelProvider::from_str(&provider_text)?;
    let session = session_of(provider, &channel_id, &thread_id, &goal.owner_principal);
    let report = Report::new(
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
    tx.commit().await?;
    let Some(outbox_id) = queued.1 else {
        return Ok(false);
    };
    crate::owner_alert::post_queued(pool, outbox_id, &session.storage_key(), &body)
        .await
        .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    Ok(true)
}

/// The report in the app transcript, idempotent on its request id.
async fn append_transcript(
    pool: &SqlitePool,
    key: &str,
    body: &str,
) -> Result<bool, SupervisorError> {
    let (_, created) = transcript::append(
        pool,
        Role::Supervisor,
        body,
        &[],
        key,
        transcript::now_ms(),
    )
    .await
    .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    Ok(created)
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
        let text = transcript_text(&fx.pool, &report_key(&goal_id, RunState::Completed)).await;
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
        let text = transcript_text(&fx.pool, &report_key(&goal_id, RunState::Degraded)).await;
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
        let text = transcript_text(&fx.pool, &report_key(&goal_id, RunState::Degraded)).await;
        assert!(text.contains("the verifier failed"), "{text}");
        assert!(!text.contains("BEGIN"), "{text}");
    }
}
