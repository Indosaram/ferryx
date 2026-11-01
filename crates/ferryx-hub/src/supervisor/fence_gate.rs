//! The holder's fence push and the dispatch gate on it (plan task 6, GAP-4).
//!
//! On every Standby->Holder transition the hub starts a new generation, and with it a new
//! [`FenceGate`]. Before its first dispatch the new holder pushes its lease fence to its own
//! daemon and to every paired host ([`FenceGate::push_all`]), so a machine it reaches later
//! cannot accept the previous holder's lower fence. A host whose push fails is listed
//! ([`FenceGate::unreachable_hosts`]), and its bindings are not dispatched until a push to it
//! succeeds ([`FenceGate::ensure`]); the intent stays `ready`.
//!
//! When an executing daemon refuses a dispatch as `STALE_SUPERVISOR_FENCE`, the dispatcher
//! holds the goal and calls [`FenceGate::stand_down`]: this hub dispatches nothing more and the
//! push side gives the lease up.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::agent_control::STALE_SUPERVISOR_FENCE;

/// Skip code of a dispatch whose host has not taken this holder's fence yet.
pub const SUPERVISOR_FENCE_PENDING: &str = "SUPERVISOR_FENCE_PENDING";

/// Delivers this holder's current fence to one daemon.
#[async_trait]
pub trait FencePush: Send + Sync {
    /// Paired hosts this holder pushes its fence to when its term starts.
    async fn hosts(&self) -> Vec<String> {
        Vec::new()
    }

    /// Pushes the fence to `host_id`'s daemon; `None` is this machine's own daemon. `Err`
    /// carries a code-first reason for status.
    async fn push_fence(&self, host_id: Option<&str>) -> Result<(), String>;

    /// An executing daemon refused this holder's fence: give the lease up.
    fn stand_down(&self) {}
}

/// One holder term's fence pushes. `None` keys this machine's own daemon.
pub struct FenceGate {
    push: Arc<dyn FencePush>,
    pushed: Mutex<HashSet<Option<String>>>,
    unreachable: Mutex<BTreeMap<Option<String>, String>>,
    stale: AtomicBool,
}

impl FenceGate {
    pub fn new(push: Arc<dyn FencePush>) -> Self {
        Self {
            push,
            pushed: Mutex::new(HashSet::new()),
            unreachable: Mutex::new(BTreeMap::new()),
            stale: AtomicBool::new(false),
        }
    }

    /// `Ok` once `host_id` (`None`: this machine) has taken this holder's fence; pushes it
    /// when it has not. A failed push is listed and refuses the dispatch.
    pub async fn ensure(&self, host_id: Option<&str>) -> Result<(), String> {
        if self.stood_down() {
            return Err(format!(
                "{STALE_SUPERVISOR_FENCE}: this hub stood down after a stale fence refusal"
            ));
        }
        let key = host_id.map(str::to_owned);
        if self.pushed.lock().expect("pushed").contains(&key) {
            return Ok(());
        }
        match self.push.push_fence(host_id).await {
            Ok(()) => {
                self.unreachable.lock().expect("unreachable").remove(&key);
                self.pushed.lock().expect("pushed").insert(key);
                Ok(())
            }
            Err(why) => {
                if why.starts_with(STALE_SUPERVISOR_FENCE) {
                    // A daemon already took a newer holder's fence: this holder is stale.
                    self.stand_down();
                }
                self.unreachable
                    .lock()
                    .expect("unreachable")
                    .insert(key, why.clone());
                Err(why)
            }
        }
    }

    /// The start of a holder term, before its first dispatch: pushes the fence to this
    /// machine's daemon and to every paired host the push side names.
    pub async fn push_on_start(&self) {
        let hosts = self.push.hosts().await;
        self.push_all(&hosts).await;
    }

    /// Pushes the fence to this machine's daemon, then to each of `hosts`. Failures are
    /// listed and retried by [`Self::ensure`] before that host's next dispatch.
    pub async fn push_all(&self, hosts: &[String]) {
        if let Err(why) = self.ensure(None).await {
            tracing::warn!(%why, "the supervisor fence was not pushed to this machine's daemon");
        }
        for host in hosts {
            if let Err(why) = self.ensure(Some(host)).await {
                tracing::warn!(host = %host, %why, "the supervisor fence was not pushed to a paired host; its bindings wait");
            }
        }
    }

    /// Hosts whose fence push failed and why (`None`: this machine's daemon).
    pub fn unreachable_hosts(&self) -> Vec<(Option<String>, String)> {
        self.unreachable
            .lock()
            .expect("unreachable")
            .iter()
            .map(|(host, why)| (host.clone(), why.clone()))
            .collect()
    }

    /// Stops every further dispatch of this term and asks the push side to give the lease up,
    /// once.
    pub fn stand_down(&self) {
        if !self.stale.swap(true, Ordering::SeqCst) {
            self.push.stand_down();
        }
    }

    pub fn stood_down(&self) -> bool {
        self.stale.load(Ordering::SeqCst)
    }
}
