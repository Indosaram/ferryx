use super::*;

#[test]
fn concurrent_guesses_spend_exactly_five_failure_slots() {
    // Given a live PIN and concurrent threads, not just cooperative tasks.
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::Control);
    let start = std::sync::Barrier::new(16);
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..16)
            .map(|_| {
                let auth = &auth;
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    auth.exchange_pairing_code("000000", "guess")
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    // Then the check/increment was atomic and even a valid code is locked out.
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(AuthError::InvalidPairingCode)))
            .count(),
        5
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(AuthError::PairingRateLimited)))
            .count(),
        11
    );
    assert!(matches!(
        auth.exchange_pairing_code(&code, "valid"),
        Err(AuthError::PairingRateLimited)
    ));
}

#[test]
fn new_window_discards_expired_codes_and_restores_pairing() {
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::Control);
    for _ in 0..5 {
        assert!(auth.exchange_pairing_code("000000", "guess").is_err());
    }
    let rotated = auth.create_pairing_code(DevicePermission::View);
    {
        // Advance the budget clock without sleeping; rotation cannot reset it.
        let mut window = auth.pairing_window.write();
        assert_eq!(window.failures, 5);
        let start = window.started_at.unwrap();
        // Both fixture PINs are expired at rollover; code lifetime is separate
        // from the budget boundary and must be established explicitly.
        for pairing in window.codes.values_mut() {
            pairing.created_at = start;
        }
        window.refresh(start + PAIRING_EXPIRY - Duration::from_nanos(1));
        assert_eq!(window.failures, 5);
        window.refresh(start + PAIRING_EXPIRY);
        assert_eq!(window.failures, 0);
        assert!(window.codes.is_empty());
        // Return the fixture clock to the present before calling the public API.
        window.started_at = Some(Instant::now());
    }
    for old in [code, rotated] {
        assert!(matches!(
            auth.exchange_pairing_code(&old, "stale"),
            Err(AuthError::InvalidPairingCode)
        ));
    }
    let fresh = auth.create_pairing_code(DevicePermission::View);
    let (_, device) = auth.exchange_pairing_code(&fresh, "fresh").unwrap();
    assert_eq!(device.permission, DevicePermission::View);
}

#[test]
fn late_generated_pin_survives_failure_window_rollover() {
    // Given a PIN generated 59 seconds into the preceding budget window.
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::View);
    {
        let now = Instant::now();
        let mut window = auth.pairing_window.write();
        window.started_at = Some(now - PAIRING_EXPIRY);
        window.codes.get_mut(&code).unwrap().created_at = now - Duration::from_secs(1);
        window.failures = 4;
        // When the independent failure window rolls over (no clock sleeps).
        window.refresh(now);
        assert_eq!(window.failures, 0);
    }
    // Then the one-second-old PIN still pairs with its original permission.
    let (token, device) = auth.exchange_pairing_code(&code, "late PIN").expect("PIN retains its full sixty-second lifetime");
    assert_eq!(device.permission, DevicePermission::View);
    assert_eq!(auth.validate_token(&token).unwrap().id, device.id);
    assert!(matches!(auth.exchange_pairing_code(&code, "replay"), Err(AuthError::InvalidPairingCode)));
}

#[test]
fn expired_pin_is_consumed_and_never_issues_a_token() {
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::Control);
    auth.pairing_window
        .write()
        .codes
        .get_mut(&code)
        .unwrap()
        .created_at = Instant::now() - PAIRING_EXPIRY;
    assert!(matches!(
        auth.exchange_pairing_code(&code, "expired"),
        Err(AuthError::ExpiredPairingCode)
    ));
    assert!(matches!(
        auth.exchange_pairing_code(&code, "replay"),
        Err(AuthError::InvalidPairingCode)
    ));
    assert!(auth.list_devices().is_empty());
}

#[test]
fn valid_pairing_is_single_use_under_concurrent_exchange() {
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::View);
    assert_eq!(code.len(), 6);
    assert!(code.bytes().all(|byte| byte.is_ascii_digit()));
    let start = std::sync::Barrier::new(4);
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    auth.exchange_pairing_code(&code, "same PIN")
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(auth.list_devices().len(), 1);
    assert_eq!(auth.list_devices()[0].permission, DevicePermission::View);
}

#[test]
fn successful_pairing_does_not_reset_the_global_failure_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    let auth = AuthManager::with_persistence(Some(path.clone()));
    let code = auth.create_pairing_code(DevicePermission::View);
    for _ in 0..4 {
        assert!(auth.exchange_pairing_code("000000", "guess").is_err());
    }
    let (token, device) = auth.exchange_pairing_code(&code, "valid").unwrap();
    assert!(matches!(
        auth.exchange_pairing_code("000000", "fifth"),
        Err(AuthError::InvalidPairingCode)
    ));
    assert!(matches!(
        auth.exchange_pairing_code("000000", "sixth"),
        Err(AuthError::PairingRateLimited)
    ));
    let reopened = AuthManager::with_persistence(Some(path));
    let restored = reopened.validate_token(&token).unwrap();
    assert_eq!(restored.id, device.id);
    assert_eq!(restored.permission, DevicePermission::View);
}

#[tokio::test]
async fn revocation_is_latched_and_registration_after_validation_cannot_miss_it() {
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::Control);
    let (token, device) = auth.exchange_pairing_code(&code, "victim").unwrap();
    let mut early = auth.device_revocation(&device.id).unwrap();
    let validated = auth.validate_token(&token).unwrap();
    assert!(auth.revoke_device(&device.id));
    // The wait starts AFTER revocation; a notify-only scheme loses this signal.
    tokio::time::timeout(Duration::from_secs(5), early.wait_for(|revoked| *revoked))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        auth.device_revocation(&validated.id),
        Err(AuthError::Unauthorized)
    ));
}
