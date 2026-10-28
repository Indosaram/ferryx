//! Where a queued lane runs when this machine refuses it: the paired host with the fewest
//! working agents, below the lane budget. A host that cannot be listed is skipped.
use std::time::Duration;

use serde_json::Value;

use super::signals::{parse_signals, DoneSignal};
use crate::agent_control::{AgentControl, AgentOp};

const LIST_TIMEOUT: Duration = Duration::from_secs(5);

/// Agents in the `working` state in one agent-control list answer.
pub fn working_agents(list: &Value) -> u64 {
    list.get("sessions")
        .and_then(Value::as_array)
        .map_or(0, |sessions| {
            sessions
                .iter()
                .filter(|session| session.get("state").and_then(Value::as_str) == Some("working"))
                .count() as u64
        })
}

/// True when every done signal can be checked without this machine's files: file and
/// command signals read the lane's checkout, which a remote lane does not have here.
pub fn portable(done_signals: &str) -> bool {
    parse_signals(done_signals).is_ok_and(|signals| {
        signals
            .iter()
            .all(|signal| !matches!(signal, DoneSignal::File { .. } | DoneSignal::Command { .. }))
    })
}

/// The least busy host below `lanes_max` working agents. Ties keep the listed order.
pub async fn idle_host(
    control: &dyn AgentControl,
    hosts: &[String],
    lanes_max: u64,
) -> Option<String> {
    let mut best: Option<(String, u64)> = None;
    for host in hosts {
        let listed = tokio::time::timeout(LIST_TIMEOUT, control.execute(Some(host), AgentOp::List)).await;
        let Ok(Ok(list)) = listed else {
            tracing::info!(host = %host, "paired host skipped for lane placement: not listed");
            continue;
        };
        let busy = working_agents(&list);
        if busy < lanes_max && best.as_ref().is_none_or(|(_, least)| busy < *least) {
            best = Some((host.clone(), busy));
        }
    }
    best.map(|(host, _)| host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use serde_json::json;

    #[test]
    fn only_host_independent_signals_are_portable() {
        assert!(portable("merged owner/repo#12"));
        assert!(!portable("file marker"));
        assert!(!portable("merged owner/repo#12, file marker"));
        assert!(!portable("not a signal"));
    }

    #[tokio::test]
    async fn a_failed_host_is_skipped_and_a_full_host_is_never_chosen() {
        let control = RecordingControl::default();
        *control.result.lock().unwrap() = json!({"sessions": [
            {"state": "working"}, {"state": "idle"}, {"state": "working"}
        ]});
        control.fail_hosts.lock().unwrap().insert("down".into());
        let hosts = vec!["down".to_owned(), "busy".to_owned()];
        assert_eq!(idle_host(&control, &hosts, 3).await.as_deref(), Some("busy"));
        assert_eq!(idle_host(&control, &hosts, 2).await, None);
        assert_eq!(working_agents(&json!({})), 0);
    }
}
