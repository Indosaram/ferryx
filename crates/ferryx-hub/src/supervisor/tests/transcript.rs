//! The app transcript (todo 7): append and cursor-paged reads, idempotent request ids, and a
//! converse turn that stores the owner text and the reply with empty evidence, concurrent
//! duplicates that share one turn, a dropped request that leaves a failure marker, and the
//! plain-language "retry every 5 min" rule change. The engine is a fake that counts its turns
//! and, when gated, parks inside the turn until the test releases it; nothing waits on time.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::Notify;

use crate::error::Result as HubResult;
use crate::meta::mhc_fault::SupervisorFault;
use crate::meta::mhc_profile::ConversationScope;
use crate::supervisor::panel_contract::{EvidenceRef, RuleKind, RuleScope, RuleStore, StandingRule};
use crate::supervisor::request::RequestFailure;
use crate::supervisor::transcript::{
    append, app_scope, by_request, converse, page, AppConversation, Converse, ConverseInput,
    Flights, Role,
};

struct FakeEngine {
    reply: Result<String, SupervisorFault>,
    turns: Mutex<Vec<(ConversationScope, String)>>,
    /// When set, a turn parks here until the test notifies it.
    gate: Option<Notify>,
    /// Notified each time a turn has started.
    entered: Notify,
}

impl FakeEngine {
    fn replying(text: &str) -> Self {
        Self {
            reply: Ok(text.to_owned()),
            turns: Mutex::new(Vec::new()),
            gate: None,
            entered: Notify::new(),
        }
    }

    fn gated(text: &str) -> Self {
        Self {
            gate: Some(Notify::new()),
            ..Self::replying(text)
        }
    }

    fn release(&self) {
        self.gate.as_ref().expect("gated engine").notify_one();
    }

    fn turns(&self) -> Vec<(ConversationScope, String)> {
        self.turns.lock().unwrap().clone()
    }
}

#[async_trait]
impl AppConversation for FakeEngine {
    async fn converse(
        &self,
        scope: &ConversationScope,
        message: &str,
    ) -> Result<String, SupervisorFault> {
        self.turns
            .lock()
            .unwrap()
            .push((scope.clone(), message.to_owned()));
        self.entered.notify_one();
        if let Some(gate) = &self.gate {
            gate.notified().await;
        }
        self.reply.clone()
    }
}

fn input(text: &str, request_id: &str) -> ConverseInput {
    ConverseInput {
        text: text.into(),
        mentions: Vec::new(),
        request_id: request_id.into(),
    }
}

/// In-memory standing rules with the real store's insert-or-replace contract.
#[derive(Default)]
struct FakeRules(Mutex<Vec<StandingRule>>);

#[async_trait]
impl RuleStore for FakeRules {
    async fn list(&self) -> HubResult<Vec<StandingRule>> {
        Ok(self.0.lock().unwrap().clone())
    }

    async fn upsert(&self, mut rule: StandingRule) -> HubResult<StandingRule> {
        let mut rules = self.0.lock().unwrap();
        if rule.id == 0 {
            rule.id = rules.iter().map(|rule| rule.id).max().unwrap_or(0) + 1;
            rules.push(rule.clone());
        } else if let Some(slot) = rules.iter_mut().find(|slot| slot.id == rule.id) {
            *slot = rule.clone();
        }
        Ok(rule)
    }
}

/// One request on fresh flights, at clock value `at`.
async fn converse_at(
    pool: &SqlitePool,
    engine: &dyn AppConversation,
    input: &ConverseInput,
    at: i64,
) -> Result<Value, RequestFailure> {
    let flights = Flights::default();
    let now = move || at;
    let changed = || {};
    converse(
        &Converse {
            pool,
            engine,
            rules: None,
            flights: &flights,
            now: &now,
            changed: &changed,
        },
        input,
    )
    .await
}

const NOW: &(dyn Fn() -> i64 + Send + Sync) = &|| 7;
const NOTHING: &(dyn Fn() + Send + Sync) = &|| {};

fn ctx<'a>(
    pool: &'a SqlitePool,
    engine: &'a dyn AppConversation,
    rules: Option<&'a dyn RuleStore>,
    flights: &'a Flights,
) -> Converse<'a> {
    Converse {
        pool,
        engine,
        rules,
        flights,
        now: NOW,
        changed: NOTHING,
    }
}

#[tokio::test]
async fn append_then_read_pages_by_cursor_in_order() {
    let pool = crate::storage::memory().await.unwrap();
    for (index, id) in ["a", "b", "c"].into_iter().enumerate() {
        let role = if index % 2 == 0 { Role::Owner } else { Role::Supervisor };
        append(&pool, role, &format!("text {id}"), &[], id, 1_000 + index as i64)
            .await
            .unwrap();
    }

    let first = page(&pool, 0, 2).await.unwrap();
    assert_eq!(first["messages"].as_array().unwrap().len(), 2);
    assert_eq!(first["messages"][0]["text"], "text a");
    assert_eq!(first["messages"][0]["role"], "owner");
    assert_eq!(first["messages"][1]["role"], "supervisor");
    assert_eq!(first["hasMore"], true);

    let cursor = first["nextCursor"].as_i64().unwrap();
    let second = page(&pool, cursor, 2).await.unwrap();
    assert_eq!(second["messages"].as_array().unwrap().len(), 1);
    assert_eq!(second["messages"][0]["text"], "text c");
    assert_eq!(second["messages"][0]["createdAtMs"], 1_002);
    assert_eq!(second["hasMore"], false);

    let end = second["nextCursor"].as_i64().unwrap();
    let empty = page(&pool, end, 2).await.unwrap();
    assert!(empty["messages"].as_array().unwrap().is_empty());
    assert_eq!(empty["nextCursor"], end);
}

#[tokio::test]
async fn a_repeated_request_id_stores_one_row_and_keeps_the_first() {
    let pool = crate::storage::memory().await.unwrap();
    let evidence = [EvidenceRef {
        kind: "receipt".into(),
        id: "r1".into(),
    }];
    let (first, fresh) = append(&pool, Role::Owner, "first", &evidence, "req-1", 5)
        .await
        .unwrap();
    assert!(fresh);
    let (again, fresh) = append(&pool, Role::Owner, "second", &[], "req-1", 9)
        .await
        .unwrap();
    assert!(!fresh);
    assert_eq!(again, first);
    assert_eq!(again.evidence, evidence);
    assert_eq!(page(&pool, 0, 10).await.unwrap()["messages"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn converse_stores_the_owner_text_and_an_unverified_reply_on_channel_app() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("I restarted the build.");

    let done = converse_at(&pool, &engine, &input("  restart the build  ", "req-1"), 42)
        .await
        .unwrap();

    assert_eq!(done["replayed"], false);
    assert_eq!(done["owner"]["text"], "restart the build");
    assert_eq!(done["reply"]["text"], "I restarted the build.");
    assert_eq!(done["reply"]["role"], "supervisor");
    // A claim of action with nothing attesting it carries no evidence: the UI shows Unverified.
    assert_eq!(done["reply"]["evidence"], serde_json::json!([]));
    let turns = engine.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].0, app_scope());
    assert_eq!(turns[0].0.channel_key, "app");
    assert_eq!(turns[0].1, "restart the build");
    let stored = page(&pool, 0, 10).await.unwrap();
    assert_eq!(stored["messages"].as_array().unwrap().len(), 2);
    assert_eq!(stored["messages"][0]["createdAtMs"], 42);
}

#[tokio::test]
async fn a_retried_request_id_replays_the_stored_reply_without_a_second_turn() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("done");
    let first = converse_at(&pool, &engine, &input("hello", "req-1"), 1).await.unwrap();

    let again = converse_at(&pool, &engine, &input("hello", "req-1"), 2).await.unwrap();

    assert_eq!(again["replayed"], true);
    assert_eq!(again["reply"], first["reply"]);
    assert_eq!(engine.turns().len(), 1);
    assert_eq!(page(&pool, 0, 10).await.unwrap()["messages"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_failed_turn_keeps_the_owner_text_and_a_retry_stores_the_reply() {
    let pool = crate::storage::memory().await.unwrap();
    let broken = FakeEngine {
        reply: Err(SupervisorFault::not_running("no engine")),
        ..FakeEngine::replying("")
    };
    let failure = converse_at(&pool, &broken, &input("hello", "req-1"), 1)
        .await
        .unwrap_err();
    assert_eq!(failure.code, "MHC_NOT_RUNNING");
    assert_eq!(failure.status, 503);
    assert_eq!(by_request(&pool, "req-1").await.unwrap().unwrap().text, "hello");
    assert!(by_request(&pool, "reply:req-1").await.unwrap().is_none());

    let engine = FakeEngine::replying("back");
    let done = converse_at(&pool, &engine, &input("hello", "req-1"), 2).await.unwrap();
    assert_eq!(done["reply"]["text"], "back");
    assert_eq!(engine.turns().len(), 1);
}

#[tokio::test]
async fn mentions_reach_the_turn_and_bad_input_is_refused_before_any_write() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("ok");
    let mut mentioned = input("stop it", "req-1");
    mentioned.mentions = vec!["host-a:pty-1".into()];
    converse_at(&pool, &engine, &mentioned, 1).await.unwrap();
    assert!(engine.turns()[0].1.contains("host-a:pty-1"));

    let mut bad_ref = input("hi", "req-2");
    bad_ref.mentions = vec!["no-colon".into()];
    for bad in [bad_ref, input("   ", "req-3"), input("hi", ""), input("hi", "reply:x"), input("hi", "failed:x")] {
        let failure = converse_at(&pool, &engine, &bad, 1).await.unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST");
    }
    assert_eq!(page(&pool, 0, 10).await.unwrap()["messages"].as_array().unwrap().len(), 2);
    assert_eq!(engine.turns().len(), 1);
}

#[tokio::test]
async fn concurrent_duplicates_share_one_turn_and_only_the_follower_is_replayed() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::gated("done once");
    let flights = Flights::default();
    let context = ctx(&pool, &engine, None, &flights);
    let request = input("restart the build", "req-1");

    let (first, second, ()) = tokio::join!(
        converse(&context, &request),
        converse(&context, &request),
        async {
            // The leader is inside its turn; the duplicate is now parked behind it.
            engine.entered.notified().await;
            engine.release();
        },
    );
    let (first, second) = (first.unwrap(), second.unwrap());

    assert_eq!(engine.turns().len(), 1, "a duplicate must not run the engine again");
    assert_eq!(first["replayed"], false);
    assert_eq!(second["replayed"], true);
    assert_eq!(first["reply"], second["reply"]);
    assert_eq!(first["reply"]["text"], "done once");
    assert_eq!(page(&pool, 0, 10).await.unwrap()["messages"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_duplicate_of_a_failed_turn_gets_the_same_failure_and_no_second_turn() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine {
        reply: Err(SupervisorFault::not_running("no engine")),
        ..FakeEngine::gated("")
    };
    let flights = Flights::default();
    let context = ctx(&pool, &engine, None, &flights);
    let request = input("hello", "req-1");

    let (first, second, ()) = tokio::join!(
        converse(&context, &request),
        converse(&context, &request),
        async {
            engine.entered.notified().await;
            engine.release();
        },
    );

    assert_eq!(first.unwrap_err().code, "MHC_NOT_RUNNING");
    assert_eq!(second.unwrap_err().code, "MHC_NOT_RUNNING");
    assert_eq!(engine.turns().len(), 1);
}

#[tokio::test]
async fn the_owner_row_is_announced_while_the_turn_is_still_running() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::gated("later");
    let flights = Flights::default();
    let announced = AtomicUsize::new(0);
    let changed = || {
        announced.fetch_add(1, Ordering::SeqCst);
    };
    let context = Converse {
        changed: &changed,
        ..ctx(&pool, &engine, None, &flights)
    };
    let request = input("hello", "req-1");

    let (done, ()) = tokio::join!(converse(&context, &request), async {
        engine.entered.notified().await;
        // The turn has not answered, yet the owner message is stored and announced.
        assert_eq!(announced.load(Ordering::SeqCst), 1);
        assert_eq!(by_request(&pool, "req-1").await.unwrap().unwrap().text, "hello");
        assert!(by_request(&pool, "reply:req-1").await.unwrap().is_none());
        engine.release();
    });
    done.unwrap();

    assert_eq!(announced.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_dropped_request_leaves_a_failure_marker_and_a_retry_runs_the_turn() {
    let pool = crate::storage::memory().await.unwrap();
    let stuck = FakeEngine::gated("never");
    let flights = Flights::default();
    let request = input("restart the build", "req-1");

    // The HTTP request goes away mid-turn: its future is dropped; a duplicate is waiting on it.
    let context = ctx(&pool, &stuck, None, &flights);
    let mut leader = Box::pin(converse(&context, &request));
    tokio::select! {
        _ = &mut leader => unreachable!("the gated turn cannot finish"),
        () = stuck.entered.notified() => {}
    }
    let mut follower = Box::pin(converse(&context, &request));
    tokio::select! {
        _ = &mut follower => unreachable!("the follower waits for the leader"),
        () = tokio::task::yield_now() => {}
    }
    drop(leader);
    flights.settle().await;

    let refused = follower.await.unwrap_err();
    assert_eq!(refused.code, "SUPERVISOR_UNAVAILABLE");
    assert_eq!(stuck.turns().len(), 1);
    let marker = by_request(&pool, "failed:req-1").await.unwrap().expect("failure marker");
    assert_eq!(marker.role, Role::Supervisor);
    assert!(marker.text.contains("did not finish"), "{}", marker.text);
    assert_eq!(by_request(&pool, "req-1").await.unwrap().unwrap().text, "restart the build");
    assert!(by_request(&pool, "reply:req-1").await.unwrap().is_none());

    // Sending the same request id again runs the turn: the abandoned flight is not a replay.
    let engine = FakeEngine::replying("restarted");
    let done = converse(&ctx(&pool, &engine, None, &flights), &request).await.unwrap();
    assert_eq!(done["replayed"], false);
    assert_eq!(done["reply"]["text"], "restarted");
    assert_eq!(engine.turns().len(), 1);
}

#[tokio::test]
async fn retry_every_5_min_creates_a_300_second_rule_and_replies_with_it_as_evidence() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("the engine must not answer a rule change");
    let rules = FakeRules::default();
    let flights = Flights::default();
    let context = ctx(&pool, &engine, Some(&rules), &flights);

    let done = converse(&context, &input("retry every 5 min", "req-1")).await.unwrap();

    let stored = rules.list().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].kind, RuleKind::UpstreamRetry);
    assert_eq!(stored[0].scope, RuleScope::default());
    assert_eq!(stored[0].params["thenEverySecs"], 300);
    assert_eq!(stored[0].params["backoffSecs"], serde_json::json!([300]));
    assert_eq!(
        stored[0].created_from_message_id.as_deref(),
        Some(done["owner"]["id"].to_string().as_str())
    );
    assert_eq!(done["replayed"], false);
    assert_eq!(done["reply"]["role"], "supervisor");
    assert_eq!(
        done["reply"]["evidence"],
        serde_json::json!([{"kind": "rule", "id": stored[0].id.to_string()}])
    );
    let text = done["reply"]["text"].as_str().unwrap();
    assert!(text.contains("5 min") && text.contains("300 s"), "{text}");
    assert!(engine.turns().is_empty(), "a rule change is not an engine turn");

    // The evidence is stored with the reply, so a replay carries it too.
    let again = converse(&context, &input("retry every 5 min", "req-1")).await.unwrap();
    assert_eq!(again["replayed"], true);
    assert_eq!(again["reply"]["evidence"], done["reply"]["evidence"]);
    assert_eq!(rules.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn rule_changes_replace_in_place_per_scope_and_scope_names_the_repo() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("unused");
    let rules = FakeRules::default();
    let flights = Flights::default();
    let context = ctx(&pool, &engine, Some(&rules), &flights);

    converse(&context, &input("Retry every 5 min.", "a")).await.unwrap();
    converse(&context, &input("please retry every 2 minutes", "b")).await.unwrap();
    converse(&context, &input("retry every 90 seconds in mahoquot projects", "c")).await.unwrap();
    converse(&context, &input("retry every 1 hour", "d")).await.unwrap();

    let stored = rules.list().await.unwrap();
    assert_eq!(stored.len(), 2, "one global rule, one for mahoquot");
    let global = stored.iter().find(|rule| rule.scope == RuleScope::default()).unwrap();
    assert_eq!(global.params["thenEverySecs"], 3_600);
    let scoped = stored.iter().find(|rule| rule.scope.repo.as_deref() == Some("mahoquot")).unwrap();
    assert_eq!(scoped.params["thenEverySecs"], 90);
    assert!(engine.turns().is_empty());
}

#[tokio::test]
async fn a_rule_change_without_a_rule_store_or_with_a_bad_interval_is_refused_not_faked() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("unused");
    let flights = Flights::default();

    let failure = converse(&ctx(&pool, &engine, None, &flights), &input("retry every 5 min", "req-1"))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "SUPERVISOR_UNAVAILABLE");
    assert_eq!(failure.status, 503);
    assert_eq!(by_request(&pool, "req-1").await.unwrap().unwrap().text, "retry every 5 min");
    assert!(by_request(&pool, "reply:req-1").await.unwrap().is_none());

    let rules = FakeRules::default();
    for bad in ["retry every 1 second", "retry every 25 hours", "retry every 99999999999999999999 s"] {
        let failure = converse(&ctx(&pool, &engine, Some(&rules), &flights), &input(bad, "bad"))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST", "{bad}");
    }
    assert!(rules.list().await.unwrap().is_empty());
    assert!(engine.turns().is_empty());
}

#[tokio::test]
async fn text_that_is_not_a_rule_change_or_addresses_sessions_goes_to_the_engine() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("ok");
    let rules = FakeRules::default();
    let flights = Flights::default();
    let context = ctx(&pool, &engine, Some(&rules), &flights);

    converse(&context, &input("can you retry the build every so often?", "a")).await.unwrap();
    let mut mentioned = input("retry every 5 min", "b");
    mentioned.mentions = vec!["host-a:pty-1".into()];
    let done = converse(&context, &mentioned).await.unwrap();

    assert_eq!(engine.turns().len(), 2);
    assert_eq!(done["reply"]["evidence"], serde_json::json!([]));
    assert!(rules.list().await.unwrap().is_empty());
}
