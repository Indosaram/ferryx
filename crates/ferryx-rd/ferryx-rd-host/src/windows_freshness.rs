//! Content identity is independent of repeat work publication time.
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct RawFrame {
    pub nv12: Arc<Vec<u8>>,
    pub captured_at: Instant,
    pub published_at: Instant,
    pub capture_id: u64,
    pub repeat: bool,
    pub conversion: Duration,
}

impl RawFrame {
    pub fn repeated(&self, published_at: Instant) -> Self {
        Self {
            published_at,
            repeat: true,
            ..self.clone()
        }
    }

    pub fn ages(&self, entry: Instant) -> (Duration, Duration) {
        (
            entry.saturating_duration_since(self.captured_at),
            entry.saturating_duration_since(self.published_at),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_freshness_idle_repeat_preserves_origin_and_resets_work_age() {
        // Given converted content at logical 0 with publication at 4ms.
        let origin = Instant::now();
        let fresh = RawFrame {
            nv12: Arc::new(vec![128; 24]),
            captured_at: origin,
            published_at: origin + Duration::from_millis(4),
            capture_id: 7,
            repeat: false,
            conversion: Duration::from_millis(4),
        };
        // When idle capture publishes cached pixels at 500ms, encoded at 502ms.
        let repeat = fresh.repeated(origin + Duration::from_millis(500));
        // Then content age is preserved, but repeat residence starts anew.
        assert!(Arc::ptr_eq(&fresh.nv12, &repeat.nv12));
        assert_eq!(repeat.capture_id, 7);
        assert_eq!(repeat.captured_at, origin);
        assert_eq!(
            repeat.ages(origin + Duration::from_millis(502)),
            (Duration::from_millis(502), Duration::from_millis(2))
        );
        assert!(repeat.repeat);
        assert!(!fresh.repeat);
        assert_eq!(repeat.conversion, Duration::from_millis(4));
    }
}
