use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const LEASE_TTL: Duration = Duration::from_secs(15);
pub const MAX_CLAIMS_PER_SECOND: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverLease {
    pub device_id: String,
    pub connection_id: String,
    pub subscription_id: String,
    pub browser_id: String,
    pub lease_epoch: u64,
    pub expires_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteDriverError {
    Unauthorized(String),
    BrowserDriverBusy { current_owner: String, lease_epoch: u64 },
    StaleLease { expected: u64, actual: u64 },
    RateLimited,
    DesktopReclaimed,
    InvalidLease(&'static str),
}

impl std::fmt::Display for RemoteDriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized(msg) => write!(f, "unauthorized driver claim: {}", msg),
            Self::BrowserDriverBusy { current_owner, lease_epoch } => {
                write!(f, "driver lease busy: owned by {} at epoch {}", current_owner, lease_epoch)
            }
            Self::StaleLease { expected, actual } => {
                write!(f, "stale driver lease: expected epoch {}, found {}", expected, actual)
            }
            Self::RateLimited => write!(f, "driver claim rate limit exceeded"),
            Self::DesktopReclaimed => write!(f, "control has been reclaimed by desktop owner"),
            Self::InvalidLease(msg) => write!(f, "invalid driver lease: {}", msg),
        }
    }
}

impl std::error::Error for RemoteDriverError {}

#[derive(Default)]
pub struct RemoteDriverBroker {
    current_lease: Arc<parking_lot::Mutex<Option<DriverLease>>>,
    epoch_counter: Arc<AtomicU64>,
    claim_history: Arc<parking_lot::Mutex<HashMap<String, Vec<Instant>>>>,
    desktop_reclaimed: Arc<AtomicBool>,
}

impl RemoteDriverBroker {
    pub fn new() -> Self {
        Self {
            current_lease: Arc::new(parking_lot::Mutex::new(None)),
            epoch_counter: Arc::new(AtomicU64::new(1)),
            claim_history: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            desktop_reclaimed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn claim(
        &self,
        device_id: &str,
        connection_id: &str,
        subscription_id: &str,
        browser_id: &str,
        has_control: bool,
    ) -> Result<DriverLease, RemoteDriverError> {
        if !has_control {
            return Err(RemoteDriverError::Unauthorized(
                "control permission required to claim driver lease".into(),
            ));
        }

        let now = Instant::now();

        // Rate limiting: max 2 claims/sec per device
        {
            let mut history = self.claim_history.lock();
            let timestamps = history.entry(device_id.to_string()).or_default();
            timestamps.retain(|&t| now.duration_since(t) < Duration::from_secs(1));
            if timestamps.len() >= MAX_CLAIMS_PER_SECOND {
                return Err(RemoteDriverError::RateLimited);
            }
            timestamps.push(now);
        }

        let mut guard = self.current_lease.lock();

        // Check if existing lease is active
        if let Some(ref current) = *guard {
            if now < current.expires_at {
                // If same owner and same browser, refresh lease
                if current.device_id == device_id
                    && current.connection_id == connection_id
                    && current.subscription_id == subscription_id
                    && current.browser_id == browser_id
                {
                    let renewed = DriverLease {
                        device_id: device_id.to_string(),
                        connection_id: connection_id.to_string(),
                        subscription_id: subscription_id.to_string(),
                        browser_id: browser_id.to_string(),
                        lease_epoch: current.lease_epoch,
                        expires_at: now + LEASE_TTL,
                    };
                    *guard = Some(renewed.clone());
                    return Ok(renewed);
                }

                return Err(RemoteDriverError::BrowserDriverBusy {
                    current_owner: current.device_id.clone(),
                    lease_epoch: current.lease_epoch,
                });
            }
        }

        // Previous lease expired or was None. Create fresh lease.
        self.desktop_reclaimed.store(false, Ordering::SeqCst);
        let new_epoch = self.epoch_counter.fetch_add(1, Ordering::SeqCst);
        let lease = DriverLease {
            device_id: device_id.to_string(),
            connection_id: connection_id.to_string(),
            subscription_id: subscription_id.to_string(),
            browser_id: browser_id.to_string(),
            lease_epoch: new_epoch,
            expires_at: now + LEASE_TTL,
        };
        *guard = Some(lease.clone());
        Ok(lease)
    }

    pub fn heartbeat(
        &self,
        device_id: &str,
        connection_id: &str,
        subscription_id: &str,
        lease_epoch: u64,
    ) -> Result<Duration, RemoteDriverError> {
        let now = Instant::now();
        let mut guard = self.current_lease.lock();

        if let Some(ref mut lease) = *guard {
            if now >= lease.expires_at {
                *guard = None;
                return Err(RemoteDriverError::StaleLease {
                    expected: lease_epoch,
                    actual: 0,
                });
            }

            if lease.lease_epoch != lease_epoch
                || lease.device_id != device_id
                || lease.connection_id != connection_id
                || lease.subscription_id != subscription_id
            {
                return Err(RemoteDriverError::StaleLease {
                    expected: lease_epoch,
                    actual: lease.lease_epoch,
                });
            }

            // Authenticated owner heartbeat extends TTL
            lease.expires_at = now + LEASE_TTL;
            return Ok(LEASE_TTL);
        }

        Err(RemoteDriverError::StaleLease {
            expected: lease_epoch,
            actual: 0,
        })
    }

    pub fn release(&self, subscription_id: &str, lease_epoch: u64) -> Result<bool, RemoteDriverError> {
        let mut guard = self.current_lease.lock();
        if let Some(ref lease) = *guard {
            if lease.subscription_id == subscription_id && lease.lease_epoch == lease_epoch {
                *guard = None;
                return Ok(true);
            }
            if Instant::now() >= lease.expires_at {
                *guard = None;
                return Ok(false); // Idempotent release of already expired lease
            }
            return Err(RemoteDriverError::StaleLease {
                expected: lease_epoch,
                actual: lease.lease_epoch,
            });
        }
        Ok(false) // Idempotent release
    }

    pub fn release_by_subscription(&self, subscription_id: &str) -> bool {
        let mut guard = self.current_lease.lock();
        if let Some(ref lease) = *guard {
            if lease.subscription_id == subscription_id {
                *guard = None;
                return true;
            }
        }
        false
    }

    pub fn revoke_device(&self, device_id: &str) -> bool {
        let mut guard = self.current_lease.lock();
        if let Some(ref lease) = *guard {
            if lease.device_id == device_id {
                *guard = None;
                self.epoch_counter.fetch_add(1, Ordering::SeqCst);
                return true;
            }
        }
        false
    }

    pub fn desktop_reclaim(&self) -> u64 {
        let mut guard = self.current_lease.lock();
        *guard = None;
        self.desktop_reclaimed.store(true, Ordering::SeqCst);
        self.epoch_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn validate_lease(
        &self,
        browser_id: &str,
        lease_epoch: u64,
        device_id: &str,
        connection_id: &str,
    ) -> Result<(), RemoteDriverError> {
        if self.desktop_reclaimed.load(Ordering::SeqCst) {
            return Err(RemoteDriverError::DesktopReclaimed);
        }

        let now = Instant::now();
        let guard = self.current_lease.lock();

        if let Some(ref lease) = *guard {
            if now >= lease.expires_at {
                return Err(RemoteDriverError::StaleLease {
                    expected: lease_epoch,
                    actual: 0,
                });
            }

            if lease.browser_id != browser_id
                || lease.lease_epoch != lease_epoch
                || lease.device_id != device_id
                || lease.connection_id != connection_id
            {
                return Err(RemoteDriverError::StaleLease {
                    expected: lease_epoch,
                    actual: lease.lease_epoch,
                });
            }

            return Ok(());
        }

        Err(RemoteDriverError::StaleLease {
            expected: lease_epoch,
            actual: 0,
        })
    }

    pub fn current_lease(&self) -> Option<DriverLease> {
        let now = Instant::now();
        let guard = self.current_lease.lock();
        if let Some(ref lease) = *guard {
            if now < lease.expires_at {
                return Some(lease.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_lease_claim_and_expiry() {
        let broker = RemoteDriverBroker::new();

        // 1. Claim without control permission rejected
        let no_ctrl = broker.claim("dev1", "conn1", "sub1", "b1", false);
        assert!(matches!(no_ctrl, Err(RemoteDriverError::Unauthorized(_))));

        // 2. Claim with control succeeds with 15s TTL
        let lease = broker.claim("dev1", "conn1", "sub1", "b1", true).expect("claim");
        assert_eq!(lease.device_id, "dev1");
        assert_eq!(lease.browser_id, "b1");
        assert_eq!(lease.lease_epoch, 1);
        assert!(lease.expires_at > Instant::now());

        // 3. Busy when another viewer claims before expiry
        let busy = broker.claim("dev2", "conn2", "sub2", "b1", true);
        assert!(matches!(busy, Err(RemoteDriverError::BrowserDriverBusy { .. })));

        // 4. Busy even if targeting a different browser (service-wide single driver lease!)
        let busy_diff_browser = broker.claim("dev2", "conn2", "sub2", "b2", true);
        assert!(matches!(busy_diff_browser, Err(RemoteDriverError::BrowserDriverBusy { .. })));

        // 5. Validation succeeds for valid lease
        assert!(broker.validate_lease("b1", lease.lease_epoch, "dev1", "conn1").is_ok());

        // 6. Validation fails for wrong browser or wrong device
        assert!(broker.validate_lease("b2", lease.lease_epoch, "dev1", "conn1").is_err());
        assert!(broker.validate_lease("b1", lease.lease_epoch, "dev2", "conn1").is_err());
    }

    #[test]
    fn test_driver_lease_heartbeat_renewal() {
        let broker = RemoteDriverBroker::new();
        let lease = broker.claim("dev1", "conn1", "sub1", "b1", true).unwrap();

        // Heartbeat by owner succeeds
        let remaining = broker.heartbeat("dev1", "conn1", "sub1", lease.lease_epoch).unwrap();
        assert_eq!(remaining, LEASE_TTL);

        // Heartbeat by wrong subscriber fails
        let bad_sub = broker.heartbeat("dev1", "conn1", "sub2", lease.lease_epoch);
        assert!(matches!(bad_sub, Err(RemoteDriverError::StaleLease { .. })));

        // Heartbeat with wrong epoch fails
        let bad_epoch = broker.heartbeat("dev1", "conn1", "sub1", lease.lease_epoch + 99);
        assert!(matches!(bad_epoch, Err(RemoteDriverError::StaleLease { .. })));
    }

    #[test]
    fn test_driver_lease_release_and_race() {
        let broker = Arc::new(RemoteDriverBroker::new());
        let lease = broker.claim("dev1", "conn1", "sub1", "b1", true).unwrap();

        // Release by owner succeeds
        assert!(broker.release("sub1", lease.lease_epoch).unwrap());

        // Double release is idempotent
        assert!(!broker.release("sub1", lease.lease_epoch).unwrap());

        // Concurrent racing claims from multiple threads guarantee exactly one winner
        let mut handles = Vec::new();
        for i in 0..10 {
            let b = Arc::clone(&broker);
            handles.push(std::thread::spawn(move || {
                let dev = format!("dev_{}", i);
                let conn = format!("conn_{}", i);
                let sub = format!("sub_{}", i);
                b.claim(&dev, &conn, &sub, "b1", true)
            }));
        }

        let mut successes = 0;
        let mut busy_count = 0;
        for h in handles {
            let res = h.join().unwrap();
            match res {
                Ok(_) => successes += 1,
                Err(RemoteDriverError::BrowserDriverBusy { .. }) => busy_count += 1,
                Err(e) => panic!("unexpected error: {:?}", e),
            }
        }

        assert_eq!(successes, 1, "exactly one winner in race");
        assert_eq!(busy_count, 9, "all other racing claims are rejected as busy");
    }

    #[test]
    fn test_driver_lease_desktop_reclaim() {
        let broker = RemoteDriverBroker::new();
        let lease = broker.claim("dev1", "conn1", "sub1", "b1", true).unwrap();

        // Desktop reclaim immediately clears lease and increments epoch
        let new_epoch = broker.desktop_reclaim();
        assert!(new_epoch > lease.lease_epoch);

        // Validation with old lease epoch fails
        let err = broker.validate_lease("b1", lease.lease_epoch, "dev1", "conn1");
        assert!(matches!(err, Err(RemoteDriverError::DesktopReclaimed)));

        // Lease is now None
        assert_eq!(broker.current_lease(), None);
    }

    #[test]
    fn test_driver_lease_no_auto_takeover() {
        let broker = RemoteDriverBroker::new();
        let _lease = broker.claim("dev1", "conn1", "sub1", "b1", true).unwrap();

        // Unsubscribe releases lease
        assert!(broker.release_by_subscription("sub1"));

        // NO auto-takeover: current lease is None, no fallback to another viewer
        assert_eq!(broker.current_lease(), None);

        // Another viewer must explicitly claim
        let claim2 = broker.claim("dev2", "conn2", "sub2", "b1", true);
        assert!(claim2.is_ok());
    }
}
