use serde::{Deserialize, Serialize};

pub const GRACE_SECS: u64 = 604_800;

pub const FREE_HOSTS: u32 = 1;
pub const PRO_BASE_HOSTS: u32 = 10;
pub const TEAM_HOSTS_PER_SEAT: u32 = 10;
pub const TEAM_MIN_SEATS: u32 = 2;
pub const HOSTS_PER_PACK: u32 = 5;

pub const MAX_TRUSTED_SEATS: u32 = 10_000;
pub const MAX_TRUSTED_HOST_PACKS: u32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKey {
    Free,
    ProMonthly,
    ProAnnual,
    TeamMonthly,
    TeamAnnual,
}

impl Default for PlanKey {
    fn default() -> Self {
        Self::Free
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Active,
    OnTrial,
    PastDue,
    Unpaid,
    Cancelled,
    Expired,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitlementStatus {
    Ok,
    OverLimit,
    PastDue,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingSnapshot {
    pub plan: PlanKey,
    pub seats: u32,
    pub host_packs: u32,
    pub status: SubscriptionStatus,
    pub ends_at: Option<u64>,
    pub machines_used: u32,
    pub grace_started_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entitlement {
    pub effective_plan: PlanKey,
    pub status: EntitlementStatus,
    pub machine_limit: u32,
    pub grace_ends_at: Option<u64>,
    pub remote_allowed: bool,
    pub may_enroll_new: bool,
}

pub fn machine_limit(plan: PlanKey, seats: u32, host_packs: u32) -> u32 {
    match plan {
        PlanKey::Free => FREE_HOSTS,
        PlanKey::ProMonthly | PlanKey::ProAnnual => {
            PRO_BASE_HOSTS.saturating_add(host_packs.saturating_mul(HOSTS_PER_PACK))
        }
        PlanKey::TeamMonthly | PlanKey::TeamAnnual => {
            let effective_seats = seats.max(TEAM_MIN_SEATS);
            effective_seats
                .saturating_mul(TEAM_HOSTS_PER_SEAT)
                .saturating_add(host_packs.saturating_mul(HOSTS_PER_PACK))
        }
    }
}

pub fn is_subscription_active(status: SubscriptionStatus, ends_at: Option<u64>, now: u64) -> bool {
    match status {
        SubscriptionStatus::Active | SubscriptionStatus::OnTrial => match ends_at {
            Some(end) => now < end,
            None => true,
        },
        SubscriptionStatus::Cancelled => match ends_at {
            Some(end) => now < end,
            None => false,
        },
        SubscriptionStatus::PastDue | SubscriptionStatus::Unpaid => true,
        SubscriptionStatus::Expired | SubscriptionStatus::Paused => false,
    }
}

pub fn next_grace_started_at(prev: Option<u64>, violated: bool, now: u64) -> Option<u64> {
    if violated {
        prev.or(Some(now))
    } else {
        None
    }
}

pub fn evaluate(snapshot: &BillingSnapshot, now: u64) -> Entitlement {
    let sub_valid = match snapshot.plan {
        PlanKey::Free => false,
        _ => is_subscription_active(snapshot.status, snapshot.ends_at, now),
    };

    let effective_plan = if sub_valid {
        snapshot.plan
    } else {
        PlanKey::Free
    };

    let limit = machine_limit(effective_plan, snapshot.seats, snapshot.host_packs);

    let is_payment_delinquent = sub_valid
        && matches!(
            snapshot.status,
            SubscriptionStatus::PastDue | SubscriptionStatus::Unpaid
        );

    let is_over_limit = snapshot.machines_used > limit;
    let violated = is_payment_delinquent || is_over_limit;

    let grace_start = if violated {
        snapshot.grace_started_at.or(Some(now))
    } else {
        None
    };

    let status = if !violated {
        EntitlementStatus::Ok
    } else {
        let grace_start_time = grace_start.unwrap_or(now);
        if now >= grace_start_time.saturating_add(GRACE_SECS) {
            EntitlementStatus::Stopped
        } else if is_payment_delinquent {
            EntitlementStatus::PastDue
        } else {
            EntitlementStatus::OverLimit
        }
    };

    let grace_ends_at = if violated {
        let grace_start_time = grace_start.unwrap_or(now);
        Some(grace_start_time.saturating_add(GRACE_SECS))
    } else {
        None
    };

    let remote_allowed = status != EntitlementStatus::Stopped;
    let may_enroll_new = remote_allowed && snapshot.machines_used < limit;

    Entitlement {
        effective_plan,
        status,
        machine_limit: limit,
        grace_ends_at,
        remote_allowed,
        may_enroll_new,
    }
}

#[cfg(test)]
#[path = "entitlement_tests.rs"]
mod entitlement_tests;
