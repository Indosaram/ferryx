use super::*;

#[test]
fn test_machine_limit_free_plan() {
    assert_eq!(machine_limit(PlanKey::Free, 0, 0), FREE_HOSTS);
    assert_eq!(machine_limit(PlanKey::Free, 5, 3), FREE_HOSTS);
}

#[test]
fn test_machine_limit_pro_plan() {
    assert_eq!(machine_limit(PlanKey::ProMonthly, 0, 0), 10);
    assert_eq!(machine_limit(PlanKey::ProAnnual, 1, 0), 10);

    assert_eq!(machine_limit(PlanKey::ProMonthly, 0, 3), 25);
    assert_eq!(machine_limit(PlanKey::ProAnnual, 10, 3), 25);
}

#[test]
fn test_machine_limit_team_plan() {
    assert_eq!(machine_limit(PlanKey::TeamMonthly, 1, 0), 20);
    assert_eq!(machine_limit(PlanKey::TeamAnnual, 0, 0), 20);
    assert_eq!(machine_limit(PlanKey::TeamMonthly, 2, 0), 20);

    assert_eq!(machine_limit(PlanKey::TeamMonthly, 3, 2), 40);
    assert_eq!(machine_limit(PlanKey::TeamAnnual, 3, 2), 40);
}

#[test]
fn test_free_at_limit_ok_one_more_over_limit() {
    let now = 1_000_000;
    let snap_ok = BillingSnapshot {
        plan: PlanKey::Free,
        seats: 0,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used: FREE_HOSTS,
        grace_started_at: None,
    };
    let ent_ok = evaluate(&snap_ok, now);
    assert_eq!(ent_ok.effective_plan, PlanKey::Free);
    assert_eq!(ent_ok.status, EntitlementStatus::Ok);
    assert_eq!(ent_ok.machine_limit, FREE_HOSTS);
    assert!(ent_ok.remote_allowed);
    assert!(!ent_ok.may_enroll_new);

    let snap_over = BillingSnapshot {
        plan: PlanKey::Free,
        seats: 0,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used: FREE_HOSTS + 1,
        grace_started_at: None,
    };
    let ent_over = evaluate(&snap_over, now);
    assert_eq!(ent_over.effective_plan, PlanKey::Free);
    assert_eq!(ent_over.status, EntitlementStatus::OverLimit);
    assert_eq!(ent_over.machine_limit, FREE_HOSTS);
    assert_eq!(ent_over.grace_ends_at, Some(now + GRACE_SECS));
    assert!(ent_over.remote_allowed);
    assert!(!ent_over.may_enroll_new);
}

#[test]
fn a_free_account_can_enroll_three_machines_and_is_refused_on_the_fourth() {
    assert_eq!(FREE_HOSTS, 3, "owner decision 2026-10-24: Free covers 3 machines");
    let now = 1_000_000;
    let free_with = |machines_used: u32| BillingSnapshot {
        plan: PlanKey::Free,
        seats: 0,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used,
        grace_started_at: None,
    };

    for already_enrolled in 0..3 {
        let ent = evaluate(&free_with(already_enrolled), now);
        assert_eq!(ent.status, EntitlementStatus::Ok);
        assert!(
            ent.may_enroll_new,
            "machine {} must be accepted on Free",
            already_enrolled + 1
        );
    }

    let fourth = evaluate(&free_with(3), now);
    assert_eq!(fourth.machine_limit, 3);
    assert_eq!(fourth.status, EntitlementStatus::Ok, "3 machines is within the Free plan");
    assert!(fourth.remote_allowed);
    assert!(!fourth.may_enroll_new, "the fourth machine must be refused");
}

#[test]
fn an_expired_pro_account_over_the_free_limit_keeps_its_machines_and_only_blocks_enrollment() {
    let ends_at = 2_000_000;
    let snap = BillingSnapshot {
        plan: PlanKey::ProMonthly,
        seats: 1,
        host_packs: 0,
        status: SubscriptionStatus::Expired,
        ends_at: Some(ends_at),
        machines_used: FREE_HOSTS + 4,
        grace_started_at: None,
    };

    let now = ends_at + 1;
    let ent = evaluate(&snap, now);
    assert_eq!(ent.effective_plan, PlanKey::Free);
    assert_eq!(ent.machine_limit, FREE_HOSTS);
    assert_eq!(ent.status, EntitlementStatus::OverLimit);
    assert!(
        ent.remote_allowed,
        "the machines already enrolled keep remote access during grace"
    );
    assert!(!ent.may_enroll_new, "only new enrollment is blocked");
    assert_eq!(ent.grace_ends_at, Some(now + GRACE_SECS));
}

#[test]
fn test_downgrade_grace_6_days_23_hours_allowed_7_days_stopped() {
    let grace_start = 1_000_000;
    let snap = BillingSnapshot {
        plan: PlanKey::Free,
        seats: 0,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used: FREE_HOSTS + 1,
        grace_started_at: Some(grace_start),
    };

    let now_before = grace_start + 601_200;
    let ent_before = evaluate(&snap, now_before);
    assert_eq!(ent_before.status, EntitlementStatus::OverLimit);
    assert!(ent_before.remote_allowed);
    assert!(!ent_before.may_enroll_new);
    assert_eq!(ent_before.grace_ends_at, Some(grace_start + GRACE_SECS));

    let now_7days = grace_start + GRACE_SECS;
    let ent_7days = evaluate(&snap, now_7days);
    assert_eq!(ent_7days.status, EntitlementStatus::Stopped);
    assert!(!ent_7days.remote_allowed);
    assert!(!ent_7days.may_enroll_new);
    assert_eq!(ent_7days.grace_ends_at, Some(grace_start + GRACE_SECS));

    let now_after = grace_start + GRACE_SECS + 1;
    let ent_after = evaluate(&snap, now_after);
    assert_eq!(ent_after.status, EntitlementStatus::Stopped);
    assert!(!ent_after.remote_allowed);
    assert_eq!(ent_after.grace_ends_at, Some(grace_start + GRACE_SECS));
}

#[test]
fn test_violation_resolved_clears_grace_immediately() {
    let grace_start = 1_000_000;
    let now = grace_start + 100_000;

    let snap_resolved = BillingSnapshot {
        plan: PlanKey::Free,
        seats: 0,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used: 1,
        grace_started_at: Some(grace_start),
    };
    let ent = evaluate(&snap_resolved, now);
    assert_eq!(ent.status, EntitlementStatus::Ok);
    assert!(ent.remote_allowed);
    assert_eq!(ent.grace_ends_at, None);

    let next_grace = next_grace_started_at(Some(grace_start), false, now);
    assert_eq!(next_grace, None);
}

#[test]
fn test_past_due_7_days_stopped() {
    let grace_start = 1_000_000;
    let snap = BillingSnapshot {
        plan: PlanKey::ProMonthly,
        seats: 1,
        host_packs: 0,
        status: SubscriptionStatus::PastDue,
        ends_at: None,
        machines_used: 5,
        grace_started_at: Some(grace_start),
    };

    let now_before = grace_start + 300_000;
    let ent_before = evaluate(&snap, now_before);
    assert_eq!(ent_before.status, EntitlementStatus::PastDue);
    assert!(ent_before.remote_allowed);
    assert_eq!(ent_before.grace_ends_at, Some(grace_start + GRACE_SECS));

    let now_after = grace_start + GRACE_SECS;
    let ent_after = evaluate(&snap, now_after);
    assert_eq!(ent_after.status, EntitlementStatus::Stopped);
    assert!(!ent_after.remote_allowed);
    assert_eq!(ent_after.grace_ends_at, Some(grace_start + GRACE_SECS));
}

#[test]
fn test_cancelled_valid_before_ends_at_and_free_after_ends_at() {
    let ends_at = 2_000_000;
    let snap = BillingSnapshot {
        plan: PlanKey::ProMonthly,
        seats: 1,
        host_packs: 0,
        status: SubscriptionStatus::Cancelled,
        ends_at: Some(ends_at),
        machines_used: 5,
        grace_started_at: None,
    };

    let now_before = 1_500_000;
    let ent_before = evaluate(&snap, now_before);
    assert_eq!(ent_before.effective_plan, PlanKey::ProMonthly);
    assert_eq!(ent_before.machine_limit, 10);
    assert_eq!(ent_before.status, EntitlementStatus::Ok);
    assert!(ent_before.remote_allowed);
    assert!(ent_before.may_enroll_new);

    let now_after = 2_000_001;
    let ent_after = evaluate(&snap, now_after);
    assert_eq!(ent_after.effective_plan, PlanKey::Free);
    assert_eq!(ent_after.machine_limit, FREE_HOSTS);
    assert_eq!(ent_after.status, EntitlementStatus::OverLimit);
    assert!(ent_after.remote_allowed);
    assert!(!ent_after.may_enroll_new);
    assert_eq!(ent_after.grace_ends_at, Some(now_after + GRACE_SECS));
}

#[test]
fn test_active_and_on_trial_status_aware_ends_at_expiry() {
    let ends_at = 1_500_000;

    let snap_active = BillingSnapshot {
        plan: PlanKey::ProMonthly,
        seats: 1,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: Some(ends_at),
        machines_used: 5,
        grace_started_at: None,
    };

    let ent_active_before = evaluate(&snap_active, 1_499_999);
    assert_eq!(ent_active_before.effective_plan, PlanKey::ProMonthly);
    assert_eq!(ent_active_before.machine_limit, 10);
    assert_eq!(ent_active_before.status, EntitlementStatus::Ok);

    let ent_active_expired = evaluate(&snap_active, 1_500_000);
    assert_eq!(ent_active_expired.effective_plan, PlanKey::Free);
    assert_eq!(ent_active_expired.machine_limit, FREE_HOSTS);
    assert_eq!(ent_active_expired.status, EntitlementStatus::OverLimit);

    let snap_trial = BillingSnapshot {
        plan: PlanKey::TeamMonthly,
        seats: 2,
        host_packs: 0,
        status: SubscriptionStatus::OnTrial,
        ends_at: Some(ends_at),
        machines_used: 15,
        grace_started_at: None,
    };

    let ent_trial_before = evaluate(&snap_trial, 1_400_000);
    assert_eq!(ent_trial_before.effective_plan, PlanKey::TeamMonthly);
    assert_eq!(ent_trial_before.machine_limit, 20);
    assert_eq!(ent_trial_before.status, EntitlementStatus::Ok);

    let ent_trial_expired = evaluate(&snap_trial, 1_500_001);
    assert_eq!(ent_trial_expired.effective_plan, PlanKey::Free);
    assert_eq!(ent_trial_expired.machine_limit, FREE_HOSTS);
    assert_eq!(ent_trial_expired.status, EntitlementStatus::OverLimit);
}

#[test]
fn test_pack_expiration_decreases_limit_entering_over_limit() {
    let now = 1_000_000;
    let snap = BillingSnapshot {
        plan: PlanKey::ProMonthly,
        seats: 1,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used: 12,
        grace_started_at: None,
    };
    let ent = evaluate(&snap, now);
    assert_eq!(ent.effective_plan, PlanKey::ProMonthly);
    assert_eq!(ent.machine_limit, 10);
    assert_eq!(ent.status, EntitlementStatus::OverLimit);
    assert_eq!(ent.grace_ends_at, Some(now + GRACE_SECS));
    assert!(ent.remote_allowed);
    assert!(!ent.may_enroll_new);
}

#[test]
fn test_boundary_machines_used_equals_limit_blocks_enroll_new() {
    let now = 1_000_000;
    let snap = BillingSnapshot {
        plan: PlanKey::ProMonthly,
        seats: 1,
        host_packs: 0,
        status: SubscriptionStatus::Active,
        ends_at: None,
        machines_used: 10,
        grace_started_at: None,
    };
    let ent = evaluate(&snap, now);
    assert_eq!(ent.status, EntitlementStatus::Ok);
    assert!(ent.remote_allowed);
    assert!(!ent.may_enroll_new);
}

#[test]
fn test_next_grace_started_at_transitions() {
    let now1 = 1_000;
    let now2 = 2_000;

    let g1 = next_grace_started_at(None, true, now1);
    assert_eq!(g1, Some(now1));

    let g2 = next_grace_started_at(g1, true, now2);
    assert_eq!(g2, Some(now1));

    let g3 = next_grace_started_at(g2, false, now2);
    assert_eq!(g3, None);
}

#[test]
fn test_serde_roundtrip() {
    let snap = BillingSnapshot {
        plan: PlanKey::TeamMonthly,
        seats: 3,
        host_packs: 2,
        status: SubscriptionStatus::Active,
        ends_at: Some(123456789),
        machines_used: 15,
        grace_started_at: Some(123450000),
    };
    let json = serde_json::to_string(&snap).expect("serialize snapshot");
    let decoded: BillingSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
    assert_eq!(snap, decoded);

    let ent = evaluate(&snap, 123451000);
    let ent_json = serde_json::to_string(&ent).expect("serialize entitlement");
    let ent_decoded: Entitlement =
        serde_json::from_str(&ent_json).expect("deserialize entitlement");
    assert_eq!(ent, ent_decoded);
}
