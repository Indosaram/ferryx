//! Handover of an agent session the hub did not start: the owner hands a running pane back
//! to the supervisor so it becomes a goal, without the supervisor ever typing into it.
//!
//! The goal is admitted as `observed` ([`AdmitTerms::observed`]): it commits as `running`,
//! bound to the handed-over pane, and its initial dispatch intent is superseded in the same
//! transaction, so the dispatcher has nothing to transmit for it. Handing over the same
//! session twice is the same admission (the source message id is
//! [`handover_source_id`]), not a second goal; the same session id on two hosts is two
//! sessions, because a provider session id only names a session on its host.
use sqlx::SqlitePool;

use super::{
    admit_request_bound, Admission, AdmitRequest, AdmitTerms, BoundSession, RunState,
    SupervisorError,
};

/// Prefix of every handover source message id; reserved for handover, like the adoption
/// prefixes.
pub const HANDOVER_PREFIX: &str = "handover:";

/// The source message id of the handover of one session: one session on one host is handed
/// over once.
pub fn handover_source_id(host_id: &str, backend_session_id: &str) -> String {
    format!("{HANDOVER_PREFIX}{host_id}:{backend_session_id}")
}

/// Admit `request` as the goal observing the already-running `session`.
///
/// A shell is not an agent session and a session the hub dispatched is already the
/// supervisor's, so both are refused. A handover the owner cancelled is never recreated by a
/// later handover of the same session: the cancelled goal is the answer.
pub async fn handover(
    pool: &SqlitePool,
    request: AdmitRequest,
    session: BoundSession,
    is_shell: bool,
    hub_dispatched: bool,
) -> Result<Admission, SupervisorError> {
    if is_shell {
        return Err(SupervisorError::Invalid(
            "a shell is not an agent session".into(),
        ));
    }
    if hub_dispatched {
        return Err(SupervisorError::Invalid(
            "the supervisor already owns this session".into(),
        ));
    }
    let cancelled: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM supervisor_requests r
            JOIN supervisor_goals g ON g.goal_id = r.goal_id
            WHERE r.source_message_id = ? AND g.run_state = ?)",
    )
    .bind(&request.source_message_id)
    .bind(RunState::Cancelled.as_str())
    .fetch_one(pool)
    .await?;
    if cancelled {
        return Err(SupervisorError::Invalid(format!(
            "the handover {} was cancelled",
            request.source_message_id
        )));
    }
    admit_request_bound(
        pool,
        &request,
        &AdmitTerms {
            observed: true,
            ..Default::default()
        },
        Some(&session),
    )
    .await
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::goals::contract::RequirementSpec;
    use crate::goals::types::GoalMode;
    use crate::supervisor::admission::OperationClass;
    use crate::supervisor::{
        bind_project_host, create_project, ensure_identity, execution_binding_of, goal_of,
        transition_goal, ChannelProvider, ExecutionKind, ExecutionTarget,
    };

    const OWNER: &str = "slack:T1:U-owner";
    const HOST: &str = "host-a";
    const HOST_B: &str = "host-b";
    const REPO: &str = "/home/owner/code/ferryx";
    const REPO_B: &str = "/home/owner/code/ferryx-b";
    const PANE: &str = "pane-handed-over-1";
    const BODY: &str = "The fetch cache is half wired; finish it and run the verifier.";

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
        bind_project_host(&pool, &project.project_id, HOST, &format!("{REPO}/.git"))
            .await
            .unwrap();
        bind_project_host(&pool, &project.project_id, HOST_B, &format!("{REPO_B}/.git"))
            .await
            .unwrap();
        Fixture {
            pool,
            supervisor_id: identity.supervisor_id,
            project_id: project.project_id,
        }
    }

    impl Fixture {
        fn request(&self, host_id: &str, session_id: &str) -> AdmitRequest {
            AdmitRequest {
                supervisor_id: self.supervisor_id.clone(),
                owner_principal: OWNER.into(),
                source_adapter: ChannelProvider::Slack,
                source_message_id: handover_source_id(host_id, session_id),
                body: BODY.into(),
                project_id: self.project_id.clone(),
                channel_binding_id: None,
                target: ExecutionTarget {
                    kind: ExecutionKind::Pty,
                    host_id: host_id.into(),
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
    }

    fn session(host_id: &str, session_id: &str) -> BoundSession {
        BoundSession {
            host_id: host_id.into(),
            session_id: session_id.into(),
        }
    }

    async fn count(pool: &SqlitePool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn handover_creates_one_running_goal_and_no_ready_dispatch() {
        let fx = fixture().await;
        let admission = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            false,
        )
        .await
        .unwrap();
        assert!(!admission.duplicate);
        let goal = goal_of(&fx.pool, &admission.receipt.goal_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((goal.run_state, goal.revision), (RunState::Running, 2));
        let binding = execution_binding_of(&fx.pool, &goal.goal_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (binding.host_id.as_str(), binding.provider_session_id.as_deref(), binding.generation),
            (HOST, Some(PANE), 1)
        );
        let states: Vec<String> =
            sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE goal_id = ?")
                .bind(&goal.goal_id)
                .fetch_all(&fx.pool)
                .await
                .unwrap();
        assert_eq!(states, vec!["superseded".to_owned()]);
        assert_eq!(
            count(
                &fx.pool,
                "SELECT COUNT(*) FROM supervisor_dispatches WHERE state = 'ready'"
            )
            .await,
            0,
            "nothing is ever typed into the handed-over pane"
        );
        assert_eq!(count(&fx.pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    }

    #[tokio::test]
    async fn a_second_handover_is_a_replay() {
        let fx = fixture().await;
        let first = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            false,
        )
        .await
        .unwrap();
        let again = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            false,
        )
        .await
        .unwrap();
        assert!(again.duplicate);
        assert_eq!(again.receipt.goal_id, first.receipt.goal_id);
        assert_eq!(count(&fx.pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    }

    #[tokio::test]
    async fn a_shell_is_refused() {
        let fx = fixture().await;
        let message = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            true,
            false,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(message.contains("shell"), "{message}");
        assert_eq!(count(&fx.pool, "SELECT COUNT(*) FROM supervisor_goals").await, 0);
    }

    #[tokio::test]
    async fn a_hub_dispatched_session_is_refused() {
        let fx = fixture().await;
        let message = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            true,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(message.contains("already owns"), "{message}");
        assert_eq!(count(&fx.pool, "SELECT COUNT(*) FROM supervisor_goals").await, 0);
    }

    #[tokio::test]
    async fn a_cancelled_handover_is_not_recreated() {
        let fx = fixture().await;
        let admission = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            false,
        )
        .await
        .unwrap();
        let goal = goal_of(&fx.pool, &admission.receipt.goal_id)
            .await
            .unwrap()
            .unwrap();
        transition_goal(
            &fx.pool,
            &goal.goal_id,
            goal.revision,
            RunState::Running,
            RunState::Cancelled,
            Some("owner cancel"),
        )
        .await
        .unwrap();
        let message = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            false,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(message.contains("cancelled"), "{message}");
        assert_eq!(count(&fx.pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    }

    #[tokio::test]
    async fn the_same_session_id_on_two_hosts_is_two_goals() {
        let fx = fixture().await;
        let first = handover(
            &fx.pool,
            fx.request(HOST, PANE),
            session(HOST, PANE),
            false,
            false,
        )
        .await
        .unwrap();
        let second = handover(
            &fx.pool,
            fx.request(HOST_B, PANE),
            session(HOST_B, PANE),
            false,
            false,
        )
        .await
        .unwrap();
        assert!(!second.duplicate);
        assert_ne!(second.receipt.goal_id, first.receipt.goal_id);
        assert_eq!(count(&fx.pool, "SELECT COUNT(*) FROM supervisor_goals").await, 2);
        let other = execution_binding_of(&fx.pool, &second.receipt.goal_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (other.host_id.as_str(), other.provider_session_id.as_deref()),
            (HOST_B, Some(PANE))
        );
    }
}
