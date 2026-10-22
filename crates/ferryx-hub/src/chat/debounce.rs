//! Slack split-message window. The caller sleeps until `deadline` and then flushes.
//! A new message in the same session replaces the deadline, so this is not a poll.
use std::collections::BTreeMap;

pub const DEFAULT_DEBOUNCE_MS: u64 = 600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Routed {
    Turn(String),
    Steer(String),
}

pub fn route(active_turn: bool, text: &str) -> Routed {
    if active_turn {
        Routed::Steer(text.to_owned())
    } else {
        Routed::Turn(text.to_owned())
    }
}

#[derive(Debug, Default)]
pub struct Debounce {
    batches: BTreeMap<String, (u64, Vec<String>)>,
}

impl Debounce {
    pub fn push(&mut self, now_ms: u64, key: &str, text: &str) {
        let batch = self
            .batches
            .entry(key.to_owned())
            .or_insert_with(|| (0, Vec::new()));
        batch.0 = now_ms.saturating_add(DEFAULT_DEBOUNCE_MS);
        batch.1.push(text.to_owned());
    }

    /// Take every session whose window closed at `now_ms`.
    pub fn due(&mut self, now_ms: u64) -> Vec<(String, String)> {
        let ready: Vec<String> = self
            .batches
            .iter()
            .filter(|(_, (deadline, _))| *deadline <= now_ms)
            .map(|(key, _)| key.clone())
            .collect();
        ready
            .into_iter()
            .filter_map(|key| {
                self.batches
                    .remove(&key)
                    .map(|(_, parts)| (key, parts.join("\n")))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_coalesces_and_an_active_turn_is_steered() {
        let mut debounce = Debounce::default();
        debounce.push(0, "C1", "hello");
        debounce.push(100, "C1", "world");
        assert!(debounce.due(699).is_empty());
        let flushed = debounce.due(700);
        assert_eq!(flushed, vec![("C1".into(), "hello\nworld".into())]);
        assert!(matches!(route(true, "hello\nworld"), Routed::Steer(_)));
        assert!(matches!(route(false, "hello"), Routed::Turn(_)));
    }
}
