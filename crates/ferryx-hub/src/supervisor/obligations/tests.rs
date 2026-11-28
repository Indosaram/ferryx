//! Behavioral tests for the owner answer obligations: the 60s holding line of a message queued
//! behind a running turn, the app transcript line, a restart re-arming pending rows, and the
//! delivery outcomes that close (or must not close) an obligation.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use super::*;

#[derive(Default)]
struct Recorder {
    count: AtomicUsize,
    last_text: std::sync::Mutex<Option<String>>,
    waits: AtomicUsize,
    last_wait: std::sync::Mutex<Option<Duration>>,
    waiting: tokio::sync::Notify,
    release: tokio::sync::Notify,
    holding: tokio::sync::Notify,
    inner: Option<AppTranscriptNotifier>,
}

#[async_trait]
impl ObligationNotifier for Recorder {
    async fn send_holding(&self, ob: &Obligation, text: &str) -> crate::error::Result<bool> {
        let res = match &self.inner {
            Some(inner) => inner.send_holding(ob, text).await,
            None => Ok(true),
        };
        self.count.fetch_add(1, Ordering::SeqCst);
        *self.last_text.lock().unwrap() = Some(text.to_owned());
        self.holding.notify_one();
        res
    }

    async fn wait_for(&self, duration: Duration) {
        *self.last_wait.lock().unwrap() = Some(duration);
        self.waits.fetch_add(1, Ordering::SeqCst);
        self.waiting.notify_one();
        self.release.notified().await;
    }
}

impl Recorder {
    /// Blocks until `n` timers are parked in `wait_for`, checks the 60s schedule, then lets
    /// them all wake.
    async fn release_after_waits(&self, n: usize) {
        while self.waits.load(Ordering::SeqCst) < n {
            self.waiting.notified().await;
        }
        let wait = self.last_wait.lock().unwrap().expect("a scheduled wait");
        assert!(
            wait > Duration::from_secs(50) && wait <= Duration::from_secs(61),
            "scheduled wait {wait:?}"
        );
        self.release.notify_waiters();
    }
}

/// An in-memory pool whose acquire timeout cannot trip under load.
async fn memory_pool_long_timeout() -> SqlitePool {
    let options = <SqliteConnectOptions as std::str::FromStr>::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .acquire_timeout(Duration::from_secs(3600))
        .connect_with(options)
        .await
        .unwrap();
    crate::storage::migrate(&pool).await.unwrap();
    pool
}

async fn setup() -> (SqlitePool, i64) {
    let pool = memory_pool_long_timeout().await;
    let id = open_obligation(&pool, "slack", "C1", "M1", Some("C1"), Some("T1"))
        .await
        .unwrap();
    (pool, id)
}

#[tokio::test]
async fn an_unanswered_owner_message_gets_a_holding_line_at_60s() {
    let (pool, id) = setup().await;
    let recorder = Arc::new(Recorder::default());
    let mut task = tokio::spawn(run_obligation_timer(pool.clone(), id, recorder.clone()));
    recorder.release_after_waits(1).await;
    tokio::select! {
        () = recorder.holding.notified() => {}
        res = &mut task => panic!("obligation timer exited before sending holding line: {res:?}"),
    }
    assert_eq!(recorder.count.load(Ordering::SeqCst), 1);
    task.abort();
}

/// The regression the obligation exists for: a second owner message queued behind a turn that
/// still holds the session lock. Its holding line must fire on its own clock, because the turn
/// that would answer it has not even started.
#[tokio::test]
async fn a_queued_message_behind_a_running_turn_still_gets_its_60s_line() {
    let pool = memory_pool_long_timeout().await;
    let key = "slack|C_OBL_QUEUED|T_OBL_QUEUED";
    let lock = crate::chat::slack_turn::session_lock(key);
    let holding_turn = lock.lock().await;
    // The queued message opens its obligation before it waits on the lock, exactly as
    // `try_handle` does.
    let id = open_obligation(&pool, "slack", "C1", "C1/900.2", Some("C1"), Some("T1"))
        .await
        .unwrap();
    let queued_ran = Arc::new(AtomicBool::new(false));
    let recorder = Arc::new(Recorder::default());
    let mut timer = tokio::spawn(run_obligation_timer(pool.clone(), id, recorder.clone()));
    let ran = Arc::clone(&queued_ran);
    let waiting_lock = Arc::clone(&lock);
    let waiting = tokio::spawn(async move {
        let _turn = waiting_lock.lock().await;
        ran.store(true, Ordering::SeqCst);
    });
    recorder.release_after_waits(1).await;
    tokio::select! {
        () = recorder.holding.notified() => {}
        res = &mut timer => panic!("obligation timer exited before sending holding line: {res:?}"),
    }
    assert_eq!(
        recorder.last_text.lock().unwrap().as_deref(),
        Some("작업 중: queued"),
        "the queued message's own holding line"
    );
    assert!(
        !queued_ran.load(Ordering::SeqCst),
        "the holding line fired while the queued turn was still waiting for the lock"
    );
    drop(holding_turn);
    waiting.await.unwrap();
    timer.abort();
}

#[tokio::test]
async fn an_app_message_gets_its_60s_line_in_the_app_transcript() {
    let pool = memory_pool_long_timeout().await;
    let id = open_obligation(&pool, "desktop", "app", "req_app_1", None, None)
        .await
        .unwrap();
    let recorder = Arc::new(Recorder {
        inner: Some(AppTranscriptNotifier(pool.clone())),
        ..Default::default()
    });
    let mut task = tokio::spawn(run_obligation_timer(pool.clone(), id, recorder.clone()));
    recorder.release_after_waits(1).await;
    tokio::select! {
        () = recorder.holding.notified() => {}
        res = &mut task => panic!("obligation timer exited before sending holding line: {res:?}"),
    }
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT role, text FROM supervisor_app_transcript
         WHERE request_id LIKE 'holding:desktop:req_app_1:1:%'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    let (role, text) = row.expect("a holding line in the app transcript");
    assert_eq!(role, "supervisor");
    assert!(text.starts_with("작업 중:"), "{text}");
    task.abort();
}

#[tokio::test]
async fn pending_obligations_survive_a_hub_restart() {
    let (pool, _id) = setup().await;
    let recorder = Arc::new(Recorder::default());
    let timers = reconstruct_pending_obligations(&pool, recorder).await.unwrap();
    assert_eq!(timers.len(), 1, "the open row is re-armed");
    // An answered row is not: the owner was already told.
    mark_answered(&pool, "slack", "M1").await.unwrap();
    let recorder = Arc::new(Recorder::default());
    assert_eq!(reconstruct_pending_obligations(&pool, recorder).await.unwrap().len(), 0);
}

/// A stand-down aborts the task that owns the recovered timers; none may fire afterwards.
#[tokio::test]
async fn aborting_the_recovery_task_aborts_its_recovered_timers() {
    let (pool, _id) = setup().await;
    let recorder = Arc::new(Recorder::default());
    let mut timers = reconstruct_pending_obligations(&pool, recorder.clone()).await.unwrap();
    let serving = tokio::spawn(async move { while timers.join_next().await.is_some() {} });
    while recorder.waits.load(Ordering::SeqCst) < 1 {
        recorder.waiting.notified().await;
    }
    serving.abort();
    assert!(serving.await.unwrap_err().is_cancelled());
    recorder.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(recorder.count.load(Ordering::SeqCst), 0);
}

/// Two overlapping timers for one obligation must claim the reminder slot once.
#[tokio::test]
async fn overlapping_timers_post_one_holding_line() {
    let (pool, id) = setup().await;
    let recorder = Arc::new(Recorder::default());
    let first = tokio::spawn(run_obligation_timer(pool.clone(), id, recorder.clone()));
    let second = tokio::spawn(run_obligation_timer(pool.clone(), id, recorder.clone()));
    recorder.release_after_waits(2).await;
    recorder.holding.notified().await;
    // The timer that lost the claim exits without posting; the winner sleeps for its next line.
    let mut first = first;
    let mut second = second;
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! { _ = &mut first => {}, _ = &mut second => {} }
    })
    .await
    .unwrap();
    assert_eq!(recorder.count.load(Ordering::SeqCst), 1);
    first.abort();
    second.abort();
}

/// A Slack post that Slack rejects: the real retry-then-uncertain path, not a direct call to
/// `mark_delivery_uncertain`. The obligation must stay open so health can say the owner may not
/// have been told.
#[cfg(feature = "hub-slack")]
#[tokio::test]
async fn a_rejected_slack_post_keeps_the_obligation_open() {
    use crate::chat::slack::with_test_api;
    use crate::chat::slack_supervisor::{
        settle_owner_delivery, OwnerAnswer, OwnerDelivery,
    };
    use crate::chat::slack_web::mock::serve_scripted;
    use serde_json::json;

    let (pool, id) = setup().await;
    let rejected = json!({"ok": false, "error": "channel_not_found"});
    let (base, calls) = serve_scripted("UBOT", vec![("chat.postMessage", vec![rejected])]).await;
    let outcome = with_test_api(base, "xoxb-test".into(), async {
        settle_owner_delivery(
            &pool,
            "M1",
            OwnerAnswer { channel: "C1", thread: Some("T1"), text: "the answer" },
        )
        .await
    })
    .await;
    assert_eq!(outcome, OwnerDelivery::Unanswered);
    let ob = load(&pool, id).await.unwrap().unwrap();
    assert!(ob.answered_at.is_none(), "a rejected post never answers the owner");
    assert_eq!(ob.delivery_state, "uncertain");
    assert!(
        ob.last_error.as_deref().unwrap_or("").contains("channel_not_found"),
        "{:?}",
        ob.last_error
    );
    assert_eq!(uncertain_deliveries(&pool).await.unwrap().len(), 1);
    let attempts = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _)| method == "chat.postMessage")
        .count();
    assert_eq!(attempts, 2, "one retry, then the uncertain record");
}

/// An explicit-silence reply is the owner's answer: nothing is posted and the obligation closes,
/// so health never reports `OWNER_ANSWER_UNCERTAIN` for it.
#[cfg(feature = "hub-slack")]
#[tokio::test]
async fn an_explicit_silence_reply_closes_the_obligation_and_leaves_health_clean() {
    use crate::chat::slack::with_test_api;
    use crate::chat::slack_supervisor::{
        settle_owner_delivery, OwnerAnswer, OwnerDelivery,
    };
    use crate::chat::slack_turn::PostOutcome;
    use crate::chat::slack_web::mock::serve_scripted;
    use crate::supervisor::setup::{setup_status, EngineView};

    let (pool, id) = setup().await;
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let outcome = with_test_api(base, "xoxb-test".into(), async {
        settle_owner_delivery(
            &pool,
            "M1",
            OwnerAnswer { channel: "C1", thread: Some("T1"), text: "SILENT" },
        )
        .await
    })
    .await;
    assert_eq!(
        outcome,
        OwnerDelivery::Answered(PostOutcome::SuppressedSilence),
        "an explicit silence is an answer, not a failed delivery"
    );
    let ob = load(&pool, id).await.unwrap().unwrap();
    assert!(ob.answered_at.is_some(), "the silence answered the owner");
    assert_eq!(ob.delivery_state, "delivered");
    assert!(calls.lock().unwrap().is_empty(), "nothing was posted for a silence");
    let status = setup_status(&pool, EngineView::default()).await.unwrap();
    assert!(status.supervisor_healthy);
    assert_ne!(
        status.engine_fault.map(|fault| fault.code).as_deref(),
        Some("OWNER_ANSWER_UNCERTAIN")
    );
}
