//! Change notification for the supervisor views. Every store write that changes what a goal
//! receipt shows (run state, revision, waits, next trigger, dispatch stage) announces the goal
//! id here after its commit, whoever made it: an owner request, the dispatcher, a worker
//! receipt, the wake scheduler or restart reconciliation. The daemon forwards announcements
//! to the desktop and phone as `supervisor_changed` (see `hub_runtime`), so a view refetches
//! when the autonomous loop moves a goal, not only after the owner's own action.
//!
//! In-process only: a write made in another process (`ferryx-cli mcp`) is not announced here;
//! the owner-request adapters publish for theirs. Without a subscriber an announcement is
//! dropped, which is correct: the views read durable rows when they refetch.
use std::sync::OnceLock;

use tokio::sync::broadcast;

/// Announcements a slow forwarder may fall behind by before it is told it lagged (and then
/// refetches everything once).
const CAPACITY: usize = 256;

fn channel() -> &'static broadcast::Sender<String> {
    static CHANGES: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    CHANGES.get_or_init(|| broadcast::channel(CAPACITY).0)
}

/// Subscribe before reading state, so a change committed after the read is not lost.
pub fn subscribe() -> broadcast::Receiver<String> {
    channel().subscribe()
}

/// Announce that `goal_id`'s receipt may have changed. Call only after the write committed.
pub fn announce(goal_id: &str) {
    // No receiver is not an error: nobody is showing the goal.
    let _ = channel().send(goal_id.to_owned());
}
