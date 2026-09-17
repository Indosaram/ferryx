//! Remote Browser Screencast Admission & Concurrency Controller
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §5, §6.2, §8.1)

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;

// §8.1 Binding overrides
pub const MAX_VIEWERS_PER_BROWSER: usize = 2;
pub const MAX_CAPTURED_BROWSERS: usize = 1;
pub const MAX_GLOBAL_DRIVERS: usize = 1;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
pub const SUBSCRIPTION_TTL: Duration = Duration::from_secs(15);
pub const SWEEPER_INTERVAL: Duration = Duration::from_secs(2);
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);
pub const ACK_PROGRESS_DEADLINE: Duration = Duration::from_secs(10);

pub const COMMAND_RATE_LIMIT_PER_SEC: f64 = 20.0;
pub const COMMAND_BURST_MAX: f64 = 40.0;
pub const CLAIM_RATE_LIMIT_PER_SEC: f64 = 2.0;
pub const CLAIM_BURST_MAX: f64 = 2.0;
pub const EVAL_RATE_LIMIT_PER_SEC: f64 = 1.0;
pub const EVAL_BURST_MAX: f64 = 1.0;

pub const CONTROL_QUEUE_MAX_COUNT: usize = 16;
pub const CONTROL_QUEUE_MAX_BYTES: usize = 1024 * 1024; // 1 MiB

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionOutcome {
    Admit,
    PermanentDrop(String),
    Backpressured(String),
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverLease {
    pub device_id: String,
    pub connection_id: String,
    pub subscription_id: String,
    pub browser_id: String,
    pub lease_epoch: u64,
    pub expires_at: Instant,
}

pub struct DriverBroker {
    current_lease: Mutex<Option<DriverLease>>,
    epoch_counter: AtomicU64,
    reclaim_tx: tokio::sync::broadcast::Sender<String>,
}

impl DriverBroker {
    pub fn new() -> Self {
        let (reclaim_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            current_lease: Mutex::new(None),
            epoch_counter: AtomicU64::new(1),
            reclaim_tx,
        }
    }

    pub fn subscribe_reclaim(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.reclaim_tx.subscribe()
    }

    pub fn claim_driver(
        &self,
        device_id: &str,
        connection_id: &str,
        subscription_id: &str,
        browser_id: &str,
        now: Instant,
    ) -> Result<DriverLease, String> {
        let mut guard = self.current_lease.lock();
        if let Some(existing) = guard.as_ref() {
            if now < existing.expires_at {
                if existing.device_id == device_id && existing.subscription_id == subscription_id {
                    let renewed = DriverLease {
                        device_id: device_id.to_string(),
                        connection_id: connection_id.to_string(),
                        subscription_id: subscription_id.to_string(),
                        browser_id: browser_id.to_string(),
                        lease_epoch: existing.lease_epoch,
                        expires_at: now + SUBSCRIPTION_TTL,
                    };
                    *guard = Some(renewed.clone());
                    return Ok(renewed);
                }
                return Err("BROWSER_DRIVER_BUSY".to_string());
            }
        }

        let next_epoch = self.epoch_counter.fetch_add(1, Ordering::SeqCst);
        let new_lease = DriverLease {
            device_id: device_id.to_string(),
            connection_id: connection_id.to_string(),
            subscription_id: subscription_id.to_string(),
            browser_id: browser_id.to_string(),
            lease_epoch: next_epoch,
            expires_at: now + SUBSCRIPTION_TTL,
        };
        *guard = Some(new_lease.clone());
        Ok(new_lease)
    }

    pub fn refresh_lease(
        &self,
        device_id: &str,
        connection_id: &str,
        subscription_id: &str,
        browser_id: &str,
        lease_epoch: u64,
        now: Instant,
    ) -> Result<Instant, String> {
        let mut guard = self.current_lease.lock();
        if let Some(existing) = guard.as_mut() {
            if existing.device_id == device_id
                && existing.connection_id == connection_id
                && existing.subscription_id == subscription_id
                && existing.browser_id == browser_id
                && existing.lease_epoch == lease_epoch
                && now < existing.expires_at
            {
                let new_expiry = now + SUBSCRIPTION_TTL;
                existing.expires_at = new_expiry;
                return Ok(new_expiry);
            }
        }
        Err("Driver lease expired or invalid".to_string())
    }

    pub fn release_driver(
        &self,
        device_id: &str,
        connection_id: &str,
        subscription_id: &str,
        browser_id: &str,
        lease_epoch: u64,
    ) -> bool {
        let mut guard = self.current_lease.lock();
        if let Some(existing) = guard.as_ref() {
            if existing.device_id == device_id
                && existing.connection_id == connection_id
                && existing.subscription_id == subscription_id
                && existing.browser_id == browser_id
                && existing.lease_epoch == lease_epoch
            {
                *guard = None;
                return true;
            }
        }
        false
    }

    pub fn is_active_driver(
        &self,
        device_id: &str,
        connection_id: &str,
        subscription_id: &str,
        browser_id: &str,
        lease_epoch: u64,
        now: Instant,
    ) -> bool {
        let guard = self.current_lease.lock();
        if let Some(existing) = guard.as_ref() {
            existing.device_id == device_id
                && existing.connection_id == connection_id
                && existing.subscription_id == subscription_id
                && existing.browser_id == browser_id
                && existing.lease_epoch == lease_epoch
                && now < existing.expires_at
        } else {
            false
        }
    }

    pub fn reclaim_desktop(&self) -> Option<DriverLease> {
        let mut guard = self.current_lease.lock();
        let lease = guard.take();
        if let Some(ref l) = lease {
            let _ = self.reclaim_tx.send(l.browser_id.clone());
        }
        lease
    }
}

pub struct SubscriberQueue {
    pub subscription_id: String,
    pub stream_id: u32,
    pub unacked_seq: Option<u32>,
    pub unacked_sent_at: Option<Instant>,
    pub pending_frame: Option<(u32, Vec<u8>)>,
    pub is_closed: bool,
}

impl SubscriberQueue {
    pub fn new(subscription_id: String, stream_id: u32) -> Self {
        Self {
            subscription_id,
            stream_id,
            unacked_seq: None,
            unacked_sent_at: None,
            pending_frame: None,
            is_closed: false,
        }
    }

    pub fn enqueue_frame(&mut self, seq: u32, frame_bytes: Vec<u8>, now: Instant) -> AdmissionOutcome {
        if self.is_closed {
            return AdmissionOutcome::Closed;
        }

        if self.unacked_seq.is_none() {
            self.unacked_seq = Some(seq);
            self.unacked_sent_at = Some(now);
            AdmissionOutcome::Admit
        } else {
            // Latest-only pending slot: overwrites previous pending frame
            self.pending_frame = Some((seq, frame_bytes));
            AdmissionOutcome::Backpressured("Pending slot replaced with latest frame".to_string())
        }
    }

    pub fn acknowledge_frame(&mut self, stream_id: u32, seq: u32, now: Instant) -> Option<(u32, Vec<u8>)> {
        if self.stream_id != stream_id {
            return None;
        }
        if self.unacked_seq == Some(seq) {
            self.unacked_seq = None;
            self.unacked_sent_at = None;
            if let Some((next_seq, next_bytes)) = self.pending_frame.take() {
                self.unacked_seq = Some(next_seq);
                self.unacked_sent_at = Some(now);
                return Some((next_seq, next_bytes));
            }
        }
        None
    }

    pub fn is_ack_stalled(&self, now: Instant) -> bool {
        match self.unacked_sent_at {
            Some(sent_at) => now.saturating_duration_since(sent_at) > ACK_PROGRESS_DEADLINE,
            None => false, // Idle is not stalled
        }
    }

    pub fn close(&mut self) {
        self.is_closed = true;
        self.pending_frame = None;
    }
}

pub struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    rate_per_sec: f64,
    capacity: f64,
}

impl TokenBucket {
    pub fn new(rate_per_sec: f64, capacity: f64, now: Instant) -> Self {
        Self {
            tokens: capacity,
            last_refill: now,
            rate_per_sec,
            capacity,
        }
    }

    pub fn try_consume(&mut self, count: f64, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate_per_sec).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= count {
            self.tokens -= count;
            true
        } else {
            false
        }
    }
}

pub struct DeviceRateLimiter {
    commands: Mutex<TokenBucket>,
    claims: Mutex<TokenBucket>,
    evals: Mutex<TokenBucket>,
}

impl DeviceRateLimiter {
    pub fn new(now: Instant) -> Self {
        Self {
            commands: Mutex::new(TokenBucket::new(
                COMMAND_RATE_LIMIT_PER_SEC,
                COMMAND_BURST_MAX,
                now,
            )),
            claims: Mutex::new(TokenBucket::new(
                CLAIM_RATE_LIMIT_PER_SEC,
                CLAIM_BURST_MAX,
                now,
            )),
            evals: Mutex::new(TokenBucket::new(
                EVAL_RATE_LIMIT_PER_SEC,
                EVAL_BURST_MAX,
                now,
            )),
        }
    }

    pub fn check_command(&self, now: Instant) -> bool {
        self.commands.lock().try_consume(1.0, now)
    }

    pub fn check_claim(&self, now: Instant) -> bool {
        self.claims.lock().try_consume(1.0, now)
    }

    pub fn check_eval(&self, now: Instant) -> bool {
        self.evals.lock().try_consume(1.0, now)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ViewerKey {
    device_id: String,
    viewer_instance_id: String,
}

pub struct AdmissionController {
    pub broker: Arc<DriverBroker>,
    viewers: Mutex<HashMap<String, HashMap<String, ViewerKey>>>, // browser_id -> (subscription_id -> ViewerKey)
    captured_browsers: Mutex<HashSet<String>>,
    limiters: Mutex<HashMap<String, Arc<DeviceRateLimiter>>>,
}

impl AdmissionController {
    pub fn new() -> Self {
        Self {
            broker: Arc::new(DriverBroker::new()),
            viewers: Mutex::new(HashMap::new()),
            captured_browsers: Mutex::new(HashSet::new()),
            limiters: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_device_limiter(&self, device_id: &str, now: Instant) -> Arc<DeviceRateLimiter> {
        let mut map = self.limiters.lock();
        map.entry(device_id.to_string())
            .or_insert_with(|| Arc::new(DeviceRateLimiter::new(now)))
            .clone()
    }

    pub fn try_subscribe(
        &self,
        browser_id: &str,
        subscription_id: &str,
        device_id: &str,
        viewer_instance_id: &str,
    ) -> Result<(), String> {
        let mut captured = self.captured_browsers.lock();
        let mut viewers_map = self.viewers.lock();

        if !captured.contains(browser_id) {
            if captured.len() >= MAX_CAPTURED_BROWSERS {
                return Err(format!(
                    "Capturing browser limit ({}) exceeded",
                    MAX_CAPTURED_BROWSERS
                ));
            }
            captured.insert(browser_id.to_string());
        }

        let browser_viewers = viewers_map.entry(browser_id.to_string()).or_default();
        let key = ViewerKey {
            device_id: device_id.to_string(),
            viewer_instance_id: viewer_instance_id.to_string(),
        };

        // Same (device_id, viewer_instance_id) replaces previous viewer on this browser
        let mut to_replace = None;
        for (sub_id, existing_key) in browser_viewers.iter() {
            if existing_key == &key {
                to_replace = Some(sub_id.clone());
                break;
            }
        }
        if let Some(old_sub) = to_replace {
            browser_viewers.remove(&old_sub);
        } else if browser_viewers.len() >= MAX_VIEWERS_PER_BROWSER {
            return Err(format!(
                "Max viewers per browser ({}) exceeded",
                MAX_VIEWERS_PER_BROWSER
            ));
        }

        browser_viewers.insert(subscription_id.to_string(), key);
        Ok(())
    }

    pub fn unsubscribe(&self, browser_id: &str, subscription_id: &str) {
        let mut captured = self.captured_browsers.lock();
        let mut viewers_map = self.viewers.lock();

        if let Some(browser_viewers) = viewers_map.get_mut(browser_id) {
            browser_viewers.remove(subscription_id);
            if browser_viewers.is_empty() {
                viewers_map.remove(browser_id);
                captured.remove(browser_id);
            }
        }
    }

    pub fn should_capture(&self, browser_id: &str) -> bool {
        let viewers_map = self.viewers.lock();
        if let Some(browser_viewers) = viewers_map.get(browser_id) {
            !browser_viewers.is_empty()
        } else {
            false
        }
    }

    pub fn reclaim_desktop(&self) -> Option<DriverLease> {
        self.broker.reclaim_desktop()
    }

    pub fn subscribe_reclaim(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.broker.subscribe_reclaim()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_viewer_and_driver_budget_concurrency() {
        let broker = DriverBroker::new();
        let now = Instant::now();

        let lease1 = broker.claim_driver("dev1", "conn1", "sub1", "b1", now).unwrap();
        assert_eq!(lease1.device_id, "dev1");
        assert_eq!(lease1.lease_epoch, 1);

        // Second driver claim from another device must fail with busy
        assert!(broker.claim_driver("dev2", "conn2", "sub2", "b1", now).is_err());

        // Same device and subscription renews the lease
        let lease_renewed = broker.claim_driver("dev1", "conn1", "sub1", "b1", now).unwrap();
        assert_eq!(lease_renewed.lease_epoch, 1);

        // Controller concurrency checks (max 2 viewers per browser, max 1 browser)
        let ctrl = AdmissionController::new();
        assert!(ctrl.try_subscribe("b1", "sub1", "dev1", "v1").is_ok());
        assert!(ctrl.try_subscribe("b1", "sub2", "dev2", "v2").is_ok());

        // 3rd viewer on b1 must fail
        assert!(ctrl.try_subscribe("b1", "sub3", "dev3", "v3").is_err());

        // Attempting to capture a 2nd browser must fail (MAX_CAPTURED_BROWSERS = 1)
        assert!(ctrl.try_subscribe("b2", "sub4", "dev4", "v4").is_err());

        // Same viewer reconnects -> succeeds and replaces old
        assert!(ctrl.try_subscribe("b1", "sub5", "dev1", "v1").is_ok());
    }

    #[test]
    fn test_latest_only_subscriber_queue_slow_viewer_isolation() {
        let mut queue = SubscriberQueue::new("sub1".into(), 1);
        let now = Instant::now();

        // 1st frame enqueued: immediately admitted to in-flight
        let res1 = queue.enqueue_frame(1, vec![1, 2, 3], now);
        assert_eq!(res1, AdmissionOutcome::Admit);
        assert_eq!(queue.unacked_seq, Some(1));

        // 2nd frame enqueued while 1st is unacked: placed in pending slot (backpressured)
        let res2 = queue.enqueue_frame(2, vec![4, 5, 6], now);
        assert!(matches!(res2, AdmissionOutcome::Backpressured(_)));
        assert_eq!(queue.pending_frame.as_ref().map(|(s, _)| *s), Some(2));

        // 3rd frame enqueued: replaces pending slot (latest-only!), sequence 2 is discarded
        let res3 = queue.enqueue_frame(3, vec![7, 8, 9], now);
        assert!(matches!(res3, AdmissionOutcome::Backpressured(_)));
        assert_eq!(queue.pending_frame.as_ref().map(|(s, _)| *s), Some(3));

        // ACK arrives for frame 1: pending frame 3 moves to unacked and is returned for transmission!
        let next = queue.acknowledge_frame(1, 1, now).unwrap();
        assert_eq!(next.0, 3);
        assert_eq!(queue.unacked_seq, Some(3));
        assert!(queue.pending_frame.is_none());
    }

    #[test]
    fn test_ack_progress_deadline_only_when_unacked_frame_exists() {
        let mut queue = SubscriberQueue::new("sub1".into(), 1);
        let now = Instant::now();

        // Idle queue: NO unacked frame exists. Deadline must NEVER report stall!
        assert!(!queue.is_ack_stalled(now + Duration::from_secs(100)));

        // Frame sent at now
        queue.enqueue_frame(1, vec![1], now);
        // At 5 seconds: not stalled yet (deadline is 10s)
        assert!(!queue.is_ack_stalled(now + Duration::from_secs(5)));
        // At 11 seconds: unacked frame exists, stalled!
        assert!(queue.is_ack_stalled(now + Duration::from_secs(11)));

        // Once ACK arrives, back to idle: not stalled even in distant future
        queue.acknowledge_frame(1, 1, now + Duration::from_secs(5));
        assert!(!queue.is_ack_stalled(now + Duration::from_secs(100)));
    }

    #[test]
    fn test_rate_limiter_burst_and_recovery() {
        let now = Instant::now();
        let limiter = DeviceRateLimiter::new(now);

        // Command burst max is 40
        for _ in 0..40 {
            assert!(limiter.check_command(now));
        }
        // 41st command immediately fails
        assert!(!limiter.check_command(now));

        // After 1 second, 20 tokens replenished
        let later = now + Duration::from_secs(1);
        assert!(limiter.check_command(later));

        // Eval rate limit is 1/sec
        assert!(limiter.check_eval(now));
        assert!(!limiter.check_eval(now));
        assert!(limiter.check_eval(now + Duration::from_secs(1)));
    }

    #[tokio::test]
    async fn test_driver_tuple_binding_and_reclaim_broadcast() {
        let broker = DriverBroker::new();
        let mut reclaim_rx = broker.subscribe_reclaim();
        let now = Instant::now();

        let lease = broker.claim_driver("dev1", "conn1", "sub1", "b1", now).unwrap();
        assert_eq!(lease.lease_epoch, 1);

        // Exact tuple matches
        assert!(broker.is_active_driver("dev1", "conn1", "sub1", "b1", 1, now));

        // Mismatched device_id
        assert!(!broker.is_active_driver("dev2", "conn1", "sub1", "b1", 1, now));
        // Mismatched connection_id
        assert!(!broker.is_active_driver("dev1", "conn2", "sub1", "b1", 1, now));
        // Mismatched subscription_id
        assert!(!broker.is_active_driver("dev1", "conn1", "sub2", "b1", 1, now));
        // Mismatched browser_id
        assert!(!broker.is_active_driver("dev1", "conn1", "sub1", "b2", 1, now));
        // Mismatched epoch
        assert!(!broker.is_active_driver("dev1", "conn1", "sub1", "b1", 2, now));

        // Reclaim broadcasts the browser_id
        let reclaimed = broker.reclaim_desktop();
        assert!(reclaimed.is_some());
        assert_eq!(reclaimed.unwrap().browser_id, "b1");

        let broadcast_id = reclaim_rx.recv().await.unwrap();
        assert_eq!(broadcast_id, "b1");

        // After reclaim, driver is no longer active
        assert!(!broker.is_active_driver("dev1", "conn1", "sub1", "b1", 1, now));
    }
}
