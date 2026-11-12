//! The pull-request lane: the owner's open PRs ordered by who acts next.
//!
//! It reuses the owner's existing `gh` sign-in through [`crate::inbox_watch::run_json`] and is
//! independent of the inbox's `OMON_GITHUB_SOURCE_ENABLED` switch. Two searches run (PRs the
//! owner authored and PRs that request the owner's review); a PR in both is listed once.
//! A missing or signed-out `gh` yields an empty lane and one `outages` row, never an error.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};

/// How often the lane is refreshed.
pub const REFRESH_EVERY: Duration = Duration::from_secs(60);

/// The `outages` subject a failing PR fetch is recorded under.
pub const OUTAGE_SUBJECT: &str = "GitHub pull requests";

const PR_FIELDS: &str = "url title number isDraft reviewDecision headRefName updatedAt repository{nameWithOwner} commits(last:1){nodes{commit{statusCheckRollup{state}}}}";

/// The two searches: PRs the owner authored, and PRs waiting for the owner's review.
pub fn search_queries() -> [(Source, String); 2] {
    let query = |filter: &str| {
        format!(
            "query{{search(query:\"is:pr is:open archived:false {filter} sort:updated-desc\",type:ISSUE,first:50){{nodes{{... on PullRequest{{{PR_FIELDS}}}}}}}}}"
        )
    };
    [
        (Source::Authored, query("author:@me")),
        (Source::ReviewRequested, query("review-requested:@me")),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Authored,
    ReviewRequested,
}

/// Who acts next on a PR. The declaration order is the lane order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrLaneKind {
    /// The owner: a review is requested, or the PR is ready (not a draft, checks green or none).
    WaitsOnOwner,
    /// A live agent session is on the PR's head branch.
    AgentWorking,
    /// Checks fail and no agent is on it.
    Failing,
    /// Checks still running, or a draft nobody is on: nobody needs to act yet.
    Pending,
}

/// The combined check state of the PR's last commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Success,
    Failure,
    Pending,
    None,
}

impl CheckState {
    fn from_rollup(raw: Option<&str>) -> Self {
        match raw {
            Some("SUCCESS") => Self::Success,
            Some("FAILURE" | "ERROR") => Self::Failure,
            Some(_) => Self::Pending,
            None => Self::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub url: String,
    pub title: String,
    pub number: u64,
    pub repo: String,
    pub head_ref_name: String,
    pub is_draft: bool,
    pub review_decision: Option<String>,
    pub checks: CheckState,
    /// The owner's review is requested on it.
    pub review_requested: bool,
    pub updated_at: String,
    pub lane: PrLaneKind,
}

/// The lane as clients read it (`GET supervisor/prs`, `panel.prs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrLane {
    pub prs: Vec<PullRequest>,
    /// Why the lane is empty because `gh` failed (`gh_missing`, `gh_unauthenticated`,
    /// `gh_failed`); `None` when the fetch succeeded.
    pub outage: Option<String>,
    pub refreshed_at_ms: i64,
}

impl PrLane {
    /// Same PRs and outage, ignoring when it was refreshed.
    pub fn same_content(&self, other: &Self) -> bool {
        self.prs == other.prs && self.outage == other.outage
    }
}

fn text(node: &Value, key: &str) -> String {
    node.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn nodes(response: &Value) -> Result<&Vec<Value>> {
    response
        .pointer("/data/search/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            let detail = response
                .pointer("/errors/0/message")
                .and_then(Value::as_str)
                .unwrap_or("missing data.search.nodes");
            HubError::message(format!("PR lane query failed: {detail}"))
        })
}

/// Who acts next, checked in lane order: the owner (a requested review, or a ready PR: not a
/// draft with green or no checks), then a live agent on the head branch, then failing checks,
/// else pending.
pub fn classify(
    is_draft: bool,
    checks: CheckState,
    review_requested: bool,
    agent_on_branch: bool,
) -> PrLaneKind {
    let ready = !is_draft && matches!(checks, CheckState::Success | CheckState::None);
    if review_requested || ready {
        return PrLaneKind::WaitsOnOwner;
    }
    if agent_on_branch {
        return PrLaneKind::AgentWorking;
    }
    if checks == CheckState::Failure {
        return PrLaneKind::Failing;
    }
    PrLaneKind::Pending
}

/// Builds the ordered lane from the two search responses. `live_branches` are the head branch
/// names live agent sessions are on.
pub fn build(
    responses: &[(Source, Value)],
    live_branches: &HashSet<String>,
) -> Result<Vec<PullRequest>> {
    let mut by_url: HashMap<String, PullRequest> = HashMap::new();
    for (source, response) in responses {
        for node in nodes(response)? {
            let url = text(node, "url");
            if url.is_empty() {
                continue;
            }
            let requested = *source == Source::ReviewRequested;
            if let Some(existing) = by_url.get_mut(&url) {
                existing.review_requested |= requested;
                continue;
            }
            let checks = CheckState::from_rollup(
                node.pointer("/commits/nodes/0/commit/statusCheckRollup/state")
                    .and_then(Value::as_str),
            );
            by_url.insert(
                url.clone(),
                PullRequest {
                    url,
                    title: text(node, "title"),
                    number: node.get("number").and_then(Value::as_u64).unwrap_or(0),
                    repo: node
                        .pointer("/repository/nameWithOwner")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    head_ref_name: text(node, "headRefName"),
                    is_draft: node.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
                    review_decision: node
                        .get("reviewDecision")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    checks,
                    review_requested: requested,
                    updated_at: text(node, "updatedAt"),
                    lane: PrLaneKind::Pending,
                },
            );
        }
    }
    let mut prs: Vec<PullRequest> = by_url
        .into_values()
        .map(|mut pr| {
            let on_branch =
                !pr.head_ref_name.is_empty() && live_branches.contains(&pr.head_ref_name);
            pr.lane = classify(pr.is_draft, pr.checks, pr.review_requested, on_branch);
            pr
        })
        .collect();
    sort(&mut prs);
    Ok(prs)
}

/// The head branches live agent sessions are on: a session with an agent, a known branch and
/// a fresh (not stale) reading. These mark a PR on that branch `agent_working`.
pub fn live_branches(sessions: &[crate::supervisor::panel_contract::FleetSession]) -> HashSet<String> {
    sessions
        .iter()
        .filter(|session| !session.stale && !session.agent.is_empty())
        .filter_map(|session| session.branch.clone())
        .filter(|branch| !branch.is_empty())
        .collect()
}

/// Lane order, then most recently updated first, then URL for a stable tie.
pub fn sort(prs: &mut [PullRequest]) {
    prs.sort_by(|a, b| {
        a.lane
            .cmp(&b.lane)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.url.cmp(&b.url))
    });
}

/// Runs both searches with `program` (normally `gh`).
pub async fn fetch_with(program: &str, live_branches: &HashSet<String>) -> Result<Vec<PullRequest>> {
    let mut responses = Vec::new();
    for (source, query) in search_queries() {
        let args = [
            "api".to_owned(),
            "graphql".to_owned(),
            "-f".to_owned(),
            format!("query={query}"),
        ];
        responses.push((source, crate::inbox_watch::run_json(program, &args).await?));
    }
    build(&responses, live_branches)
}

/// The outage cause a failed fetch is recorded under.
pub fn outage_cause(error: &HubError) -> &'static str {
    let message = error.to_string();
    if message.contains("could not start") {
        "gh_missing"
    } else if crate::inbox_watch::is_auth_failure(&message)
        || message.to_ascii_lowercase().contains("gh auth login")
    {
        "gh_unauthenticated"
    } else {
        "gh_failed"
    }
}

/// One refresh: the lane, with a failure recorded as one `outages` row (a repeat only moves its
/// `last_seen`) and a success recovering it.
pub async fn refresh(
    pool: &SqlitePool,
    program: &str,
    live_branches: &HashSet<String>,
    now: DateTime<Utc>,
) -> PrLane {
    let refreshed_at_ms = now.timestamp_millis();
    match fetch_with(program, live_branches).await {
        Ok(prs) => {
            if let Err(error) = crate::outages::observe(pool, OUTAGE_SUBJECT, "", false, now).await {
                tracing::warn!(%error, "PR lane outage recovery was not recorded");
            }
            PrLane { prs, outage: None, refreshed_at_ms }
        }
        Err(error) => {
            let cause = outage_cause(&error);
            tracing::warn!(%error, cause, "PR lane fetch failed");
            if let Err(error) = crate::outages::observe(pool, OUTAGE_SUBJECT, cause, true, now).await {
                tracing::warn!(%error, "PR lane outage was not recorded");
            }
            PrLane { prs: Vec::new(), outage: Some(cause.to_owned()), refreshed_at_ms }
        }
    }
}

/// Refreshes the lane every [`REFRESH_EVERY`] with `gh` and hands each result to `publish`.
/// `live_branches` is asked again on every refresh.
pub async fn serve<B, BFut, P>(pool: SqlitePool, live_branches: B, publish: P)
where
    B: Fn() -> BFut + Send,
    BFut: std::future::Future<Output = HashSet<String>> + Send,
    P: Fn(&PrLane) + Send,
{
    let mut ticks = tokio::time::interval(REFRESH_EVERY);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticks.tick().await;
        let branches = live_branches().await;
        let lane = refresh(&pool, "gh", &branches, Utc::now()).await;
        publish(&lane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHORED: &str = include_str!("pr_lane_fixtures/authored.json");
    const REVIEW_REQUESTED: &str = include_str!("pr_lane_fixtures/review_requested.json");

    fn responses() -> Vec<(Source, Value)> {
        vec![
            (Source::Authored, serde_json::from_str(AUTHORED).unwrap()),
            (Source::ReviewRequested, serde_json::from_str(REVIEW_REQUESTED).unwrap()),
        ]
    }

    fn branches(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn lanes(prs: &[PullRequest]) -> Vec<(u64, PrLaneKind)> {
        prs.iter().map(|pr| (pr.number, pr.lane)).collect()
    }

    #[test]
    fn pr_lane_lists_waits_on_owner_first_then_agent_working_failing_pending() {
        let prs = build(&responses(), &branches(&["ferryx/ws1/refactor"])).unwrap();
        assert_eq!(
            lanes(&prs),
            vec![
                // Review requested (both queries), newest first, then ready authored PRs.
                (3, PrLaneKind::WaitsOnOwner),
                (12, PrLaneKind::WaitsOnOwner),
                (14, PrLaneKind::WaitsOnOwner),
                (77, PrLaneKind::WaitsOnOwner),
                (13, PrLaneKind::AgentWorking),
                (11, PrLaneKind::Failing),
                (15, PrLaneKind::Pending),
            ]
        );
        let first = &prs[0];
        assert_eq!(first.repo, "acme/lib");
        assert_eq!(first.head_ref_name, "chore/bump");
        assert_eq!(first.checks, CheckState::Pending);
        let docs = prs.iter().find(|pr| pr.number == 14).unwrap();
        assert_eq!(docs.checks, CheckState::None);
        let failing = prs.iter().find(|pr| pr.number == 11).unwrap();
        assert_eq!(failing.review_decision.as_deref(), Some("REVIEW_REQUIRED"));
        assert!(!failing.review_requested);
    }

    fn fleet_session(id: &str, agent: &str, branch: Option<&str>, stale: bool) -> crate::supervisor::panel_contract::FleetSession {
        let mut session: crate::supervisor::panel_contract::FleetSession = serde_json::from_value(serde_json::json!({
            "sessionRef": format!("mac:{id}"), "hostId": "mac", "hostLabel": "Mac", "title": id,
            "cwd": "/repo", "agent": agent, "state": "working", "stateSource": "provider",
            "stateSinceMs": null, "screenChangedAtMs": null, "pendingPrompt": null,
            "lastAssistantLine": null, "updatedAtMs": 1, "stale": stale
        }))
        .unwrap();
        session.branch = branch.map(str::to_owned);
        session
    }

    #[test]
    fn pr_lane_a_session_on_the_head_branch_makes_that_pr_agent_working() {
        let fleet = vec![
            fleet_session("agent", "claude", Some("ferryx/ws1/refactor"), false),
            // A plain shell, a stale reading and a session with no branch mark nothing.
            fleet_session("shell", "", Some("wip/cache"), false),
            fleet_session("gone", "codex", Some("chore/bump"), true),
            fleet_session("detached", "codex", None, false),
        ];
        let branches = live_branches(&fleet);
        assert_eq!(branches, ["ferryx/ws1/refactor".to_owned()].into_iter().collect());
        let prs = build(&responses(), &branches).unwrap();
        let refactor = prs.iter().find(|pr| pr.head_ref_name == "ferryx/ws1/refactor").unwrap();
        assert_eq!(refactor.lane, PrLaneKind::AgentWorking);
        let cache = prs.iter().find(|pr| pr.head_ref_name == "wip/cache").unwrap();
        assert_eq!(cache.lane, PrLaneKind::Pending);
        assert_eq!(
            prs.iter().filter(|pr| pr.lane == PrLaneKind::AgentWorking).count(),
            1
        );
    }

    #[test]
    fn pr_lane_dedupes_the_two_queries_by_url() {
        let prs = build(&responses(), &HashSet::new()).unwrap();
        assert_eq!(prs.len(), 7);
        let shared: Vec<_> = prs.iter().filter(|pr| pr.number == 3).collect();
        assert_eq!(shared.len(), 1);
        assert!(shared[0].review_requested);
    }

    #[test]
    fn pr_lane_without_a_live_branch_has_no_agent_working() {
        let prs = build(&responses(), &HashSet::new()).unwrap();
        let refactor = prs.iter().find(|pr| pr.number == 13).unwrap();
        assert_eq!(refactor.lane, PrLaneKind::Failing);
        assert!(prs.iter().all(|pr| pr.lane != PrLaneKind::AgentWorking));
    }

    #[test]
    fn pr_lane_the_owner_is_checked_before_an_agent_on_the_branch() {
        assert_eq!(
            classify(false, CheckState::Failure, true, true),
            PrLaneKind::WaitsOnOwner
        );
        assert_eq!(
            classify(false, CheckState::Success, false, true),
            PrLaneKind::WaitsOnOwner
        );
        assert_eq!(classify(true, CheckState::Success, false, true), PrLaneKind::AgentWorking);
        assert_eq!(classify(true, CheckState::Success, false, false), PrLaneKind::Pending);
        assert_eq!(classify(false, CheckState::Pending, false, false), PrLaneKind::Pending);
    }

    #[test]
    fn pr_lane_serializes_the_wire_names() {
        let prs = build(&responses(), &HashSet::new()).unwrap();
        let lane = PrLane { prs, outage: None, refreshed_at_ms: 1 };
        let value = serde_json::to_value(&lane).unwrap();
        assert_eq!(value["prs"][0]["lane"], "waits_on_owner");
        assert_eq!(value["prs"][0]["headRefName"], "chore/bump");
        assert_eq!(value["prs"][0]["reviewRequested"], true);
        assert_eq!(value["refreshedAtMs"], 1);
        assert!(value["outage"].is_null());
    }

    #[test]
    fn pr_lane_a_graphql_error_is_a_failure_not_an_empty_lane() {
        let error = serde_json::json!({"errors": [{"message": "Bad credentials"}]});
        let failure = build(&[(Source::Authored, error)], &HashSet::new()).unwrap_err();
        assert!(failure.to_string().contains("Bad credentials"));
    }

    async fn outage_rows(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as("SELECT cause, state FROM outages WHERE subject = ?")
            .bind(OUTAGE_SUBJECT)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
    }

    #[tokio::test]
    async fn pr_lane_a_missing_gh_is_an_empty_lane_and_one_outage() {
        let pool = crate::storage::memory().await.unwrap();
        let missing = std::env::temp_dir()
            .join(format!("ferryx-no-gh-{}", std::process::id()))
            .join("gh");
        let program = missing.to_string_lossy().into_owned();
        for minute in 0..3 {
            let lane = refresh(
                &pool,
                &program,
                &HashSet::new(),
                at(&format!("2026-11-11T00:0{minute}:00Z")),
            )
            .await;
            assert!(lane.prs.is_empty());
            assert_eq!(lane.outage.as_deref(), Some("gh_missing"));
        }
        assert_eq!(
            outage_rows(&pool).await,
            vec![("gh_missing".to_owned(), "active".to_owned())]
        );
    }

    fn fake_gh(name: &str, unix_body: &str, windows_body: &str) -> String {
        let root = std::env::temp_dir().join(format!("ferryx-pr-lane-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join(if cfg!(windows) { "gh.cmd" } else { "gh" });
        if cfg!(windows) {
            std::fs::write(&script, windows_body.replace('\n', "\r\n")).unwrap();
        } else {
            std::fs::write(&script, unix_body).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        script.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn pr_lane_an_unauthenticated_gh_is_an_empty_lane_and_one_outage_then_recovers() {
        let pool = crate::storage::memory().await.unwrap();
        let signed_out = fake_gh(
            "signed-out",
            "#!/bin/sh\necho 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4\n",
            "@echo off\necho To get started with GitHub CLI, please run:  gh auth login 1>&2\nexit /b 4\n",
        );
        for minute in 0..2 {
            let lane = refresh(
                &pool,
                &signed_out,
                &HashSet::new(),
                at(&format!("2026-11-11T00:0{minute}:00Z")),
            )
            .await;
            assert!(lane.prs.is_empty());
            assert_eq!(lane.outage.as_deref(), Some("gh_unauthenticated"));
        }
        assert_eq!(
            outage_rows(&pool).await,
            vec![("gh_unauthenticated".to_owned(), "active".to_owned())]
        );

        // Signed in again: the recorded fixtures come back and the outage recovers.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/supervisor/pr_lane_fixtures");
        let authored = dir.join("authored.json");
        let requested = dir.join("review_requested.json");
        let signed_in = fake_gh(
            "signed-in",
            &format!(
                "#!/bin/sh\ncase \"$4\" in *review-requested*) cat '{}' ;; *) cat '{}' ;; esac\n",
                requested.display(),
                authored.display()
            ),
            // The lane asks authored first, then review-requested; a marker file tells them apart
            // without parsing the quoted query in cmd.
            &format!(
                "@echo off\nif exist \"%~dp0asked\" (type \"{}\" & del \"%~dp0asked\" & exit /b 0)\ntype \"{}\"\ntype nul > \"%~dp0asked\"\n",
                requested.display(),
                authored.display()
            ),
        );
        let lane = refresh(&pool, &signed_in, &HashSet::new(), at("2026-11-11T00:05:00Z")).await;
        assert_eq!(lane.outage, None);
        assert_eq!(lane.prs.len(), 7);
        assert_eq!(lane.prs[0].lane, PrLaneKind::WaitsOnOwner);
        assert!(lane.prs[0].review_requested);
        assert_eq!(
            outage_rows(&pool).await,
            vec![("gh_unauthenticated".to_owned(), "recovered".to_owned())]
        );
    }
}
