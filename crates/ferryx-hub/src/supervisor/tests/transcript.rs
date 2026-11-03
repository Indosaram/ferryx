//! The app transcript (todo 7): append and cursor-paged reads, idempotent request ids, and a
//! converse turn that stores the owner text and the reply with empty evidence. The engine is a
//! fake that counts its turns; nothing waits on time.
use std::sync::Mutex;

use async_trait::async_trait;

use crate::meta::mhc_fault::SupervisorFault;
use crate::meta::mhc_profile::ConversationScope;
use crate::supervisor::panel_contract::EvidenceRef;
use crate::supervisor::transcript::{
    append, app_scope, by_request, converse, page, AppConversation, ConverseInput, Role,
};

struct FakeEngine {
    reply: Result<String, SupervisorFault>,
    turns: Mutex<Vec<(ConversationScope, String)>>,
}

impl FakeEngine {
    fn replying(text: &str) -> Self {
        Self {
            reply: Ok(text.to_owned()),
            turns: Mutex::new(Vec::new()),
        }
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

    let done = converse(&pool, &engine, &input("  restart the build  ", "req-1"), &|| 42)
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
    let first = converse(&pool, &engine, &input("hello", "req-1"), &|| 1).await.unwrap();

    let again = converse(&pool, &engine, &input("hello", "req-1"), &|| 2).await.unwrap();

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
        turns: Mutex::new(Vec::new()),
    };
    let failure = converse(&pool, &broken, &input("hello", "req-1"), &|| 1)
        .await
        .unwrap_err();
    assert_eq!(failure.code, "MHC_NOT_RUNNING");
    assert_eq!(failure.status, 503);
    assert_eq!(by_request(&pool, "req-1").await.unwrap().unwrap().text, "hello");
    assert!(by_request(&pool, "reply:req-1").await.unwrap().is_none());

    let engine = FakeEngine::replying("back");
    let done = converse(&pool, &engine, &input("hello", "req-1"), &|| 2).await.unwrap();
    assert_eq!(done["reply"]["text"], "back");
    assert_eq!(engine.turns().len(), 1);
}

#[tokio::test]
async fn mentions_reach_the_turn_and_bad_input_is_refused_before_any_write() {
    let pool = crate::storage::memory().await.unwrap();
    let engine = FakeEngine::replying("ok");
    let mut mentioned = input("stop it", "req-1");
    mentioned.mentions = vec!["host-a:pty-1".into()];
    converse(&pool, &engine, &mentioned, &|| 1).await.unwrap();
    assert!(engine.turns()[0].1.contains("host-a:pty-1"));

    let mut bad_ref = input("hi", "req-2");
    bad_ref.mentions = vec!["no-colon".into()];
    for bad in [bad_ref, input("   ", "req-3"), input("hi", ""), input("hi", "reply:x")] {
        let failure = converse(&pool, &engine, &bad, &|| 1).await.unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST");
    }
    assert_eq!(page(&pool, 0, 10).await.unwrap()["messages"].as_array().unwrap().len(), 2);
    assert_eq!(engine.turns().len(), 1);
}
