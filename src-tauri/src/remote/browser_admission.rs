//! Remote Browser Screencast Admission & Concurrency Controller
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §5, §6.2, §8.1)

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

    pub fn enqueue_frame(
        &mut self,
        seq: u32,
        frame_bytes: Vec<u8>,
        now: Instant,
    ) -> AdmissionOutcome {
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

    pub fn acknowledge_frame(
        &mut self,
        stream_id: u32,
        seq: u32,
        now: Instant,
    ) -> Option<(u32, Vec<u8>)> {
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
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewerEntry {
    key: ViewerKey,
    paused: bool,
    stalled: bool,
}

pub struct AdmissionController {
    pub broker: Arc<DriverBroker>,
    viewers: Mutex<HashMap<String, HashMap<String, ViewerEntry>>>, // browser_id -> (subscription_id -> ViewerEntry)
    captured_browsers: Mutex<HashSet<String>>,
    limiters: Mutex<HashMap<String, Arc<DeviceRateLimiter>>>,
    /// Broadcasts `(browser_id, capturing)` whenever the owned-viewer count for a
    /// browser crosses zero, so capture never outlives its last accepted viewer (R4-6).
    capture_tx: tokio::sync::broadcast::Sender<(String, bool)>,
    /// Authoritative sharing publisher shared by every socket on this gateway (R4-5).
    sharing: Arc<crate::remote::browser_ws::SharingRegistry>,
    mapped_service_subs: Mutex<HashMap<String, String>>,
    eligibility_hook: parking_lot::RwLock<Option<ViewerEligibilityHook>>,
}

pub type ViewerEligibilityHook = Arc<dyn Fn(&str, &str, bool, bool) + Send + Sync>;

impl AdmissionController {
    pub fn new() -> Self {
        let (capture_tx, _) = tokio::sync::broadcast::channel(64);
        let ctrl = Self {
            broker: Arc::new(DriverBroker::new()),
            viewers: Mutex::new(HashMap::new()),
            captured_browsers: Mutex::new(HashSet::new()),
            limiters: Mutex::new(HashMap::new()),
            capture_tx,
            sharing: Arc::new(crate::remote::browser_ws::SharingRegistry::new()),
            mapped_service_subs: Mutex::new(HashMap::new()),
            eligibility_hook: parking_lot::RwLock::new(None),
        };
        if let Some(service) = crate::browser::remote_service::get_active_service() {
            ctrl.connect_remote_service(service);
        }
        ctrl
    }

    pub fn register_service_subscription(&self, admission_sub: &str, service_sub: &str) {
        let mut map = self.mapped_service_subs.lock();
        map.insert(admission_sub.to_string(), service_sub.to_string());
        map.insert(service_sub.to_string(), service_sub.to_string());
    }

    pub fn set_eligibility_hook(&self, hook: ViewerEligibilityHook) {
        *self.eligibility_hook.write() = Some(hook);
    }

    pub fn has_eligibility_hook(&self) -> bool {
        self.eligibility_hook.read().is_some()
    }

    pub fn ensure_connected_remote_service(&self) {
        if !self.has_eligibility_hook() {
            if let Some(service) = crate::browser::remote_service::get_active_service() {
                self.connect_remote_service(service);
            }
        }
    }

    pub fn connect_remote_service(
        &self,
        service: Arc<crate::browser::remote_service::BrowserRemoteService>,
    ) {
        self.set_eligibility_hook(Arc::new(
            move |browser_id, service_sub_id, paused, stalled| {
                service.set_viewer_paused(browser_id, service_sub_id, paused);
                service.set_viewer_stalled(browser_id, service_sub_id, stalled);
            },
        ));
    }

    /// The gateway-wide sharing publisher every browser socket reports through (R4-5).
    pub fn sharing_registry(&self) -> Arc<crate::remote::browser_ws::SharingRegistry> {
        Arc::clone(&self.sharing)
    }

    /// Subscribes to capture-lifecycle transitions (R4-6).
    pub fn subscribe_capture(&self) -> tokio::sync::broadcast::Receiver<(String, bool)> {
        self.capture_tx.subscribe()
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
        for (sub_id, existing_entry) in browser_viewers.iter() {
            if existing_entry.key == key {
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

        let was_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
        browser_viewers.insert(
            subscription_id.to_string(),
            ViewerEntry {
                key,
                paused: false,
                stalled: false,
            },
        );
        let now_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
        drop(viewers_map);
        drop(captured);
        if !was_capturing && now_capturing {
            let _ = self.capture_tx.send((browser_id.to_string(), true));
        }
        Ok(())
    }

    pub fn unsubscribe(&self, browser_id: &str, subscription_id: &str) {
        let mut captured = self.captured_browsers.lock();
        let mut viewers_map = self.viewers.lock();

        let mut halted = false;
        if let Some(browser_viewers) = viewers_map.get_mut(browser_id) {
            let was_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
            browser_viewers.remove(subscription_id);
            if browser_viewers.is_empty() {
                viewers_map.remove(browser_id);
                captured.remove(browser_id);
                if was_capturing {
                    halted = true;
                }
            } else {
                let now_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
                if was_capturing && !now_capturing {
                    halted = true;
                }
            }
        }
        drop(viewers_map);
        drop(captured);
        self.mapped_service_subs.lock().remove(subscription_id);
        // The last owned viewer leaving halts capture: a producer must never keep
        // running with zero viewers (R4-6).
        if halted {
            let _ = self.capture_tx.send((browser_id.to_string(), false));
        }
    }

    pub fn should_capture(&self, browser_id: &str) -> bool {
        let viewers_map = self.viewers.lock();
        if let Some(browser_viewers) = viewers_map.get(browser_id) {
            browser_viewers.values().any(|v| !v.paused && !v.stalled)
        } else {
            false
        }
    }

    pub fn set_viewer_paused(&self, browser_id: &str, subscription_id: &str, paused: bool) {
        let mut viewers_map = self.viewers.lock();
        let mut stalled = false;
        let mut found = false;
        if let Some(browser_viewers) = viewers_map.get_mut(browser_id) {
            let was_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
            let target_sub = if browser_viewers.contains_key(subscription_id) {
                Some(subscription_id.to_string())
            } else {
                self.mapped_service_subs
                    .lock()
                    .iter()
                    .find(|(_, svc)| svc.as_str() == subscription_id)
                    .map(|(adm, _)| adm.clone())
            };
            if let Some(ref sub) = target_sub {
                if let Some(entry) = browser_viewers.get_mut(sub) {
                    entry.paused = paused;
                    stalled = entry.stalled;
                    found = true;
                }
            }
            let now_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
            drop(viewers_map);
            if was_capturing != now_capturing {
                let _ = self
                    .capture_tx
                    .send((browser_id.to_string(), now_capturing));
            }
        }
        if found {
            if self.eligibility_hook.read().is_none() {
                if let Some(service) = crate::browser::remote_service::get_active_service() {
                    self.connect_remote_service(service);
                }
            }
            let mapped_sub = self
                .mapped_service_subs
                .lock()
                .get(subscription_id)
                .cloned()
                .unwrap_or_else(|| subscription_id.to_string());
            if let Some(hook) = self.eligibility_hook.read().as_ref() {
                hook(browser_id, &mapped_sub, paused, stalled);
            }
        }
    }

    pub fn set_viewer_stalled(&self, browser_id: &str, subscription_id: &str, stalled: bool) {
        let mut viewers_map = self.viewers.lock();
        let mut paused = false;
        let mut found = false;
        if let Some(browser_viewers) = viewers_map.get_mut(browser_id) {
            let was_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
            let target_sub = if browser_viewers.contains_key(subscription_id) {
                Some(subscription_id.to_string())
            } else {
                self.mapped_service_subs
                    .lock()
                    .iter()
                    .find(|(_, svc)| svc.as_str() == subscription_id)
                    .map(|(adm, _)| adm.clone())
            };
            if let Some(ref sub) = target_sub {
                if let Some(entry) = browser_viewers.get_mut(sub) {
                    entry.stalled = stalled;
                    paused = entry.paused;
                    found = true;
                }
            }
            let now_capturing = browser_viewers.values().any(|v| !v.paused && !v.stalled);
            drop(viewers_map);
            if was_capturing != now_capturing {
                let _ = self
                    .capture_tx
                    .send((browser_id.to_string(), now_capturing));
            }
        }
        if found {
            if self.eligibility_hook.read().is_none() {
                if let Some(service) = crate::browser::remote_service::get_active_service() {
                    self.connect_remote_service(service);
                }
            }
            let mapped_sub = self
                .mapped_service_subs
                .lock()
                .get(subscription_id)
                .cloned()
                .unwrap_or_else(|| subscription_id.to_string());
            if let Some(hook) = self.eligibility_hook.read().as_ref() {
                hook(browser_id, &mapped_sub, paused, stalled);
            }
        }
    }

    pub fn reclaim_desktop(&self) -> Option<DriverLease> {
        let lease = self.broker.reclaim_desktop();
        if let Some(ref l) = lease {
            self.sharing.driver_released(&l.connection_id);
        }
        lease
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

        let lease1 = broker
            .claim_driver("dev1", "conn1", "sub1", "b1", now)
            .unwrap();
        assert_eq!(lease1.device_id, "dev1");
        assert_eq!(lease1.lease_epoch, 1);

        // Second driver claim from another device must fail with busy
        assert!(broker
            .claim_driver("dev2", "conn2", "sub2", "b1", now)
            .is_err());

        // Same device and subscription renews the lease
        let lease_renewed = broker
            .claim_driver("dev1", "conn1", "sub1", "b1", now)
            .unwrap();
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

    /// R4-6: capture ownership follows owned viewer subscriptions exactly. The last
    /// teardown halts capture; a producer must never run with zero viewers.
    #[tokio::test]
    async fn test_r4_6_capture_lifecycle_follows_owned_viewers_only() {
        let ctrl = AdmissionController::new();
        let mut capture_rx = ctrl.subscribe_capture();

        // No subscription yet: nothing is captured, so attaching a receiver alone
        // can never pin a viewer slot.
        assert!(!ctrl.should_capture("b1"));

        ctrl.try_subscribe("b1", "sub1", "dev1", "v1").unwrap();
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), true));
        assert!(ctrl.should_capture("b1"));

        // A second viewer does not re-signal: capture is already running.
        ctrl.try_subscribe("b1", "sub2", "dev2", "v2").unwrap();
        ctrl.unsubscribe("b1", "sub2");
        assert!(
            ctrl.should_capture("b1"),
            "capture continues while an owned viewer remains"
        );

        // The last owned viewer leaving halts capture and frees the browser slot.
        ctrl.unsubscribe("b1", "sub1");
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), false));
        assert!(
            !ctrl.should_capture("b1"),
            "capture must halt when the last viewer tears down"
        );

        // The freed capture slot is reusable: no leaked ownership from the old socket.
        ctrl.try_subscribe("b2", "sub3", "dev3", "v3").unwrap();
        assert_eq!(capture_rx.recv().await.unwrap(), ("b2".to_string(), true));
        assert!(ctrl.should_capture("b2"));
    }

    #[tokio::test]
    async fn test_driver_tuple_binding_and_reclaim_broadcast() {
        let broker = DriverBroker::new();
        let mut reclaim_rx = broker.subscribe_reclaim();
        let now = Instant::now();

        let lease = broker
            .claim_driver("dev1", "conn1", "sub1", "b1", now)
            .unwrap();
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

    #[tokio::test]
    async fn test_r5_12_pause_and_ack_stall_control_capture_and_mapped_eligibility() {
        let ctrl = AdmissionController::new();
        let mut capture_rx = ctrl.subscribe_capture();

        let recorded_hooks = Arc::new(std::sync::Mutex::new(
            Vec::<(String, String, bool, bool)>::new(),
        ));
        let hooks_clone = Arc::clone(&recorded_hooks);
        ctrl.set_eligibility_hook(Arc::new(move |b, s, paused, stalled| {
            hooks_clone
                .lock()
                .unwrap()
                .push((b.to_string(), s.to_string(), paused, stalled));
        }));

        ctrl.try_subscribe("b1", "sub-adm-1", "dev1", "v1").unwrap();
        ctrl.register_service_subscription("sub-adm-1", "sub-svc-1");
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), true));
        assert!(ctrl.should_capture("b1"));

        // 1. Pause viewer -> capture halts, eligibility hook called with mapped service sub ID
        ctrl.set_viewer_paused("b1", "sub-adm-1", true);
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), false));
        assert!(
            !ctrl.should_capture("b1"),
            "paused viewer must halt capture"
        );
        {
            let hooks = recorded_hooks.lock().unwrap();
            assert_eq!(
                hooks.last(),
                Some(&("b1".into(), "sub-svc-1".into(), true, false))
            );
        }

        // 2. Resume viewer -> capture resumes
        ctrl.set_viewer_paused("b1", "sub-adm-1", false);
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), true));
        assert!(
            ctrl.should_capture("b1"),
            "resumed viewer must restore capture"
        );
        {
            let hooks = recorded_hooks.lock().unwrap();
            assert_eq!(
                hooks.last(),
                Some(&("b1".into(), "sub-svc-1".into(), false, false))
            );
        }

        // 3. ACK stall -> capture halts, eligibility hook called with stalled=true
        ctrl.set_viewer_stalled("b1", "sub-adm-1", true);
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), false));
        assert!(
            !ctrl.should_capture("b1"),
            "stalled viewer must halt capture"
        );
        {
            let hooks = recorded_hooks.lock().unwrap();
            assert_eq!(
                hooks.last(),
                Some(&("b1".into(), "sub-svc-1".into(), false, true))
            );
        }

        // 4. ACK arrives -> stall cleared, capture resumes
        ctrl.set_viewer_stalled("b1", "sub-adm-1", false);
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), true));
        assert!(ctrl.should_capture("b1"));
        {
            let hooks = recorded_hooks.lock().unwrap();
            assert_eq!(
                hooks.last(),
                Some(&("b1".into(), "sub-svc-1".into(), false, false))
            );
        }
    }

    #[tokio::test]
    async fn test_r5_5_sharing_registry_forwards_live_changes_with_dto() {
        let registry = crate::remote::browser_ws::SharingRegistry::new();
        let received = Arc::new(std::sync::Mutex::new(Vec::<
            crate::browser::remote_bridge_protocol::BrowserSharingState,
        >::new()));
        let rec_clone = Arc::clone(&received);
        registry.set_listener(Arc::new(
            move |dto: &crate::browser::remote_bridge_protocol::BrowserSharingState| {
                rec_clone.lock().unwrap().push(dto.clone());
            },
        ));

        // 1. Viewer admitted -> Viewing
        registry.viewer_admitted("conn1", "dev1");
        {
            let rec = received.lock().unwrap();
            assert_eq!(rec.len(), 1);
            assert_eq!(rec[0].r#type, "browserSharingState");
            assert!(rec[0].is_sharing);
            assert_eq!(rec[0].active_sessions_count, 1);
            assert_eq!(
                rec[0].driver_status,
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Viewing
            );
        }

        // 2. Driver claimed -> Driving
        registry.driver_claimed("conn1", "dev1");
        {
            let rec = received.lock().unwrap();
            assert_eq!(rec.len(), 2);
            assert_eq!(
                rec[1].driver_status,
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Driving
            );
            assert_eq!(rec[1].driver_device_id.as_deref(), Some("dev1"));
        }

        // 3. Driver released -> Viewing
        registry.driver_released("conn1");
        {
            let rec = received.lock().unwrap();
            assert_eq!(rec.len(), 3);
            assert_eq!(
                rec[2].driver_status,
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Viewing
            );
            assert_eq!(rec[2].driver_device_id, None);
        }

        // 4. Viewer removed -> Idle
        registry.viewer_removed("conn1");
        {
            let rec = received.lock().unwrap();
            assert_eq!(rec.len(), 4);
            assert!(!rec[3].is_sharing);
            assert_eq!(rec[3].active_sessions_count, 0);
            assert_eq!(
                rec[3].driver_status,
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Idle
            );
        }
    }

    #[tokio::test]
    async fn test_r6_9_viewer_pause_and_ack_stall_reaches_remote_service_via_admission() {
        use crate::browser::model::CreateBrowserRequest;
        use crate::browser::remote_driver::RemoteDriverBroker;
        use crate::browser::remote_service::BrowserRemoteService;
        use crate::browser::BrowserManager;

        let manager = BrowserManager::new();
        let broker = Arc::new(RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(manager.clone(), broker));

        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("b-r69".into()),
                workspace_id: Some("ws-1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        let service_sub = service.subscribe("b-r69", "dev1", "view1").unwrap();

        let ctrl = AdmissionController::new();
        ctrl.connect_remote_service(service.clone());
        ctrl.try_subscribe("b-r69", "sub-adm-r69", "dev1", "view1")
            .unwrap();
        ctrl.register_service_subscription("sub-adm-r69", &service_sub);

        // Verify initial state in service
        let initial_viewer = service.get_viewer_info("b-r69", &service_sub).unwrap();
        assert!(!initial_viewer.paused);
        assert!(!initial_viewer.stalled);

        // Pause through admission
        ctrl.set_viewer_paused("b-r69", "sub-adm-r69", true);
        let paused_viewer = service.get_viewer_info("b-r69", &service_sub).unwrap();
        assert!(
            paused_viewer.paused,
            "Viewer pause must reach BrowserRemoteService"
        );

        // Stall through admission
        ctrl.set_viewer_stalled("b-r69", "sub-adm-r69", true);
        let stalled_viewer = service.get_viewer_info("b-r69", &service_sub).unwrap();
        assert!(
            stalled_viewer.stalled,
            "Viewer stall must reach BrowserRemoteService"
        );

        // Resume through admission
        ctrl.set_viewer_paused("b-r69", "sub-adm-r69", false);
        let resumed_viewer = service.get_viewer_info("b-r69", &service_sub).unwrap();
        assert!(
            !resumed_viewer.paused,
            "Viewer resume must reach BrowserRemoteService"
        );

        // Clear stall through admission
        ctrl.set_viewer_stalled("b-r69", "sub-adm-r69", false);
        let unstalled_viewer = service.get_viewer_info("b-r69", &service_sub).unwrap();
        assert!(
            !unstalled_viewer.stalled,
            "Viewer stall clear must reach BrowserRemoteService"
        );
    }
}
