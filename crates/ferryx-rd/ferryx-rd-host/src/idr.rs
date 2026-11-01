//! One session-wide budget for reactive IDRs. Initial/restart IDRs bypass it.
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) const IDR_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(super) struct ForcedIdrLimiter(Mutex<Option<Instant>>);

impl ForcedIdrLimiter {
    pub(super) fn request<E>(
        &self,
        now: Instant,
        force: impl FnOnce() -> Result<bool, E>,
    ) -> Result<bool, E> {
        let mut last = self.0.lock().expect("IDR limiter poisoned");
        if last.is_some_and(|last| now.saturating_duration_since(last) < IDR_INTERVAL) {
            return Ok(false);
        }
        let requested = force()?;
        if requested {
            *last = Some(now);
        }
        Ok(requested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn client_and_transport_share_exact_boundary_budget() {
        let client = Arc::new(ForcedIdrLimiter::default());
        let transport = Arc::clone(&client);
        let now = Instant::now();
        let allow = || Ok::<_, ()>(true);
        assert_eq!(client.request(now, allow), Ok(true));
        assert_eq!(
            transport.request(now + IDR_INTERVAL - Duration::from_nanos(1), allow),
            Ok(false)
        );
        assert_eq!(transport.request(now + IDR_INTERVAL, allow), Ok(true));
        assert_eq!(client.request(now + IDR_INTERVAL, allow), Ok(false));
        assert_eq!(client.request(now, allow), Ok(false));
    }

    #[test]
    fn absent_pipeline_and_failed_requests_do_not_consume_budget() {
        let limiter = ForcedIdrLimiter::default();
        let now = Instant::now();
        assert_eq!(limiter.request(now, || Ok::<_, ()>(false)), Ok(false));
        assert_eq!(
            limiter.request(now, || Err::<bool, _>("encoder failed")),
            Err("encoder failed")
        );
        assert_eq!(limiter.request(now, || Ok::<_, ()>(true)), Ok(true));
        assert_eq!(
            limiter.request(now, || -> Result<bool, ()> {
                panic!("limited request reached encoder")
            }),
            Ok(false)
        );
    }
}
