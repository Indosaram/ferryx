#[cfg(test)]
mod tests {
    use crate::daemon::protocol::{
        clip_stage_budget, DaemonRequest, DaemonResponse, DaemonSessionDetails,
        LocalSplitEnvelope, PaneAttachTuple, PanePresentationReceipt, PreparedLocalSplit,
        SplitAttachAttempt, SplitDelivery, SplitErrorDetails, SplitIdentity, SplitNoChild,
        SplitOperationRequest, SplitOperationResponse, SplitOperationResult, SplitOwnership,
        SplitUnknownReason, ATTEMPT_TOTAL_BUDGET_MS, CANCEL_ACK_MAX_MS,
        DAEMON_CANCEL_CLEANUP_MAX_MS, LOCAL_SPLIT_LIFECYCLE_CAPABILITY, LOCAL_SPLIT_VALIDITY_MS,
        STAGE_ATTACH_OR_LISTENER_MAX_MS, STAGE_CREATE_OR_STATUS_MAX_MS, STAGE_CWD_PROBE_MAX_MS,
        STAGE_PRESENTATION_MAX_MS,
    };
    use crate::ipc::error::{IpcError, IpcErrorCode};
    use serde_json::json;

    #[test]
    fn pane_liveness_contract_constants_and_budget_clipping() {
        assert_eq!(ATTEMPT_TOTAL_BUDGET_MS, 15_000);
        assert_eq!(STAGE_CREATE_OR_STATUS_MAX_MS, 9_000);
        assert_eq!(STAGE_ATTACH_OR_LISTENER_MAX_MS, 4_000);
        assert_eq!(STAGE_PRESENTATION_MAX_MS, 2_000);
        assert_eq!(STAGE_CWD_PROBE_MAX_MS, 500);
        assert_eq!(CANCEL_ACK_MAX_MS, 3_000);
        assert_eq!(DAEMON_CANCEL_CLEANUP_MAX_MS, 2_500);
        assert_eq!(LOCAL_SPLIT_VALIDITY_MS, 600_000);
        assert_eq!(LOCAL_SPLIT_LIFECYCLE_CAPABILITY, "localSplitLifecycleV1");

        assert_eq!(clip_stage_budget(15_000, STAGE_CREATE_OR_STATUS_MAX_MS), 9_000);
        assert_eq!(clip_stage_budget(15_000, STAGE_ATTACH_OR_LISTENER_MAX_MS), 4_000);
        assert_eq!(clip_stage_budget(15_000, STAGE_PRESENTATION_MAX_MS), 2_000);

        assert_eq!(clip_stage_budget(5_000, STAGE_CREATE_OR_STATUS_MAX_MS), 5_000);
        assert_eq!(clip_stage_budget(2_500, STAGE_ATTACH_OR_LISTENER_MAX_MS), 2_500);
        assert_eq!(clip_stage_budget(800, STAGE_PRESENTATION_MAX_MS), 800);
        assert_eq!(clip_stage_budget(0, STAGE_PRESENTATION_MAX_MS), 0);
    }

    #[test]
    fn pane_liveness_contract_split_identity_and_prepared_payload() {
        let identity = SplitIdentity {
            request_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            origin_epoch: "42".to_string(),
            expires_at_unix_ms: 1_700_000_600_000,
        };
        let val = serde_json::to_value(&identity).expect("serialize identity");
        assert_eq!(val["requestId"], "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(val["originEpoch"], "42");
        assert_eq!(val["expiresAtUnixMs"], 1_700_000_600_000_u64);

        let prepared = PreparedLocalSplit {
            identity: identity.clone(),
            workspace_id: "ws-test".to_string(),
            worktree: None,
            cwd: "/Volumes/repo".to_string(),
            shell: Some("/bin/zsh".to_string()),
            cols: 120,
            rows: 40,
        };
        let prep_val = serde_json::to_value(&prepared).expect("serialize prepared");
        assert!(prep_val.get("remainingMs").is_none());
        assert!(prep_val["identity"].get("remainingMs").is_none());
        let decoded: PreparedLocalSplit = serde_json::from_value(prep_val).expect("decode prepared");
        assert_eq!(decoded, prepared);
    }

    #[test]
    fn pane_liveness_contract_local_split_envelope_roundtrip() {
        let env = LocalSplitEnvelope {
            origin_epoch: 42,
            expires_at_unix_ms: 1_700_000_600_000,
            remaining_ms: 9_000,
        };
        let val = serde_json::to_value(&env).expect("serialize envelope");
        assert_eq!(val["originEpoch"], 42);
        assert_eq!(val["expiresAtUnixMs"], 1_700_000_600_000_u64);
        assert_eq!(val["remainingMs"], 9_000);
        let decoded: LocalSplitEnvelope = serde_json::from_value(val).expect("decode envelope");
        assert_eq!(decoded, env);
    }

    #[test]
    fn pane_liveness_contract_split_operation_result_all_states() {
        let details = DaemonSessionDetails {
            session_id: "pty-1".to_string(),
            workspace_id: Some("ws-test".to_string()),
            worktree: None,
            cwd: Some("/repo".to_string()),
            cols: 80,
            rows: 24,
            running: true,
            start_sequence: Some(1),
            end_sequence: Some(10),
            last_output_age_ms: Some(15),
            suspended: false,
            reader_paused: None,
            kernel_stopped: None,
            registry_suspended: None,
            suspension_source: None,
            incarnation: Some("uuid-inc-1".to_string()),
        };

        let cases: Vec<SplitOperationResult<String>> = vec![
            SplitOperationResult::Absent { can_create: true },
            SplitOperationResult::Pending { cancel_requested: false },
            SplitOperationResult::Created {
                session_id: "pty-1".to_string(),
                daemon_epoch: "42".to_string(),
                session: details,
                ownership: SplitOwnership::Created,
            },
            SplitOperationResult::Cancelled,
            SplitOperationResult::Exited,
            SplitOperationResult::Failed {
                error: IpcError::spawn_request_expired("request timed out"),
                no_child: SplitNoChild,
            },
            SplitOperationResult::Unknown {
                reason: SplitUnknownReason::EpochChanged,
            },
            SplitOperationResult::Unknown {
                reason: SplitUnknownReason::PublicationUncertain,
            },
        ];

        for result in cases {
            let encoded = serde_json::to_value(&result).expect("encode operation result");
            let decoded: SplitOperationResult<String> =
                serde_json::from_value(encoded.clone()).expect("decode operation result");
            assert_eq!(serde_json::to_value(&decoded).unwrap(), encoded);
        }
    }

    #[test]
    fn pane_liveness_contract_failed_requires_no_child_true() {
        let legal = json!({
            "state": "failed",
            "error": { "code": "SPAWN_CANCELLED", "message": "cancelled" },
            "noChild": true
        });
        let decoded: Result<SplitOperationResult, _> = serde_json::from_value(legal);
        assert!(decoded.is_ok());

        let illegal_false = json!({
            "state": "failed",
            "error": { "code": "SPAWN_CANCELLED", "message": "cancelled" },
            "noChild": false
        });
        assert!(serde_json::from_value::<SplitOperationResult>(illegal_false).is_err());

        let illegal_missing = json!({
            "state": "failed",
            "error": { "code": "SPAWN_CANCELLED", "message": "cancelled" }
        });
        assert!(serde_json::from_value::<SplitOperationResult>(illegal_missing).is_err());
    }

    #[test]
    fn pane_liveness_contract_attach_tuple_7_fields_and_matching() {
        let tuple_a = PaneAttachTuple {
            backend_session_id: "backend-uuid-1".to_string(),
            incarnation: Some("inc-uuid-1".to_string()),
            daemon_epoch: "42".to_string(),
            frontend_session_id: "fe-sess-1".to_string(),
            pane_identity: "leaf-1".to_string(),
            binding_key: "bind-abc".to_string(),
            attempt_generation: 3,
        };

        let val = serde_json::to_value(&tuple_a).expect("serialize attach tuple");
        assert_eq!(val["backendSessionId"], "backend-uuid-1");
        assert_eq!(val["incarnation"], "inc-uuid-1");
        assert_eq!(val["daemonEpoch"], "42");
        assert_eq!(val["frontendSessionId"], "fe-sess-1");
        assert_eq!(val["paneIdentity"], "leaf-1");
        assert_eq!(val["bindingKey"], "bind-abc");
        assert_eq!(val["attemptGeneration"], 3);

        let decoded: PaneAttachTuple = serde_json::from_value(val).expect("decode attach tuple");
        assert_eq!(decoded, tuple_a);
        assert!(tuple_a.matches_presentation(&decoded));

        let mut mismatch_backend = tuple_a.clone();
        mismatch_backend.backend_session_id = "backend-uuid-2".to_string();
        assert!(!tuple_a.matches_presentation(&mismatch_backend));

        let mut mismatch_inc = tuple_a.clone();
        mismatch_inc.incarnation = Some("inc-uuid-other".to_string());
        assert!(!tuple_a.matches_presentation(&mismatch_inc));

        let mut mismatch_epoch = tuple_a.clone();
        mismatch_epoch.daemon_epoch = "43".to_string();
        assert!(!tuple_a.matches_presentation(&mismatch_epoch));

        let mut mismatch_fe = tuple_a.clone();
        mismatch_fe.frontend_session_id = "fe-sess-other".to_string();
        assert!(!tuple_a.matches_presentation(&mismatch_fe));

        let mut mismatch_pane = tuple_a.clone();
        mismatch_pane.pane_identity = "leaf-other".to_string();
        assert!(!tuple_a.matches_presentation(&mismatch_pane));

        let mut mismatch_binding = tuple_a.clone();
        mismatch_binding.binding_key = "bind-stale".to_string();
        assert!(!tuple_a.matches_presentation(&mismatch_binding));

        let mut mismatch_gen = tuple_a.clone();
        mismatch_gen.attempt_generation = 4;
        assert!(!tuple_a.matches_presentation(&mismatch_gen));

        let receipt = PanePresentationReceipt {
            attach_tuple: tuple_a.clone(),
            presented: true,
            presentation_time_unix_ms: Some(1_700_000_123_456),
        };
        let receipt_val = serde_json::to_value(&receipt).expect("serialize receipt");
        assert_eq!(receipt_val["presented"], true);
        assert_eq!(receipt_val["attachTuple"]["backendSessionId"], "backend-uuid-1");
    }

    #[test]
    fn pane_liveness_contract_incarnation_vs_owner_epoch() {
        let legacy_details = DaemonSessionDetails {
            session_id: "pty-legacy".to_string(),
            workspace_id: None,
            worktree: None,
            cwd: None,
            cols: 80,
            rows: 24,
            running: true,
            start_sequence: None,
            end_sequence: None,
            last_output_age_ms: None,
            suspended: false,
            reader_paused: None,
            kernel_stopped: None,
            registry_suspended: None,
            suspension_source: None,
            incarnation: None,
        };
        assert!(legacy_details.is_recoverable_legacy());
        assert!(!legacy_details.matches_incarnation("any-incarnation"));

        let proven_details = DaemonSessionDetails {
            incarnation: Some("uuid-pty-stable".to_string()),
            ..legacy_details
        };
        assert!(!proven_details.is_recoverable_legacy());
        assert!(proven_details.matches_incarnation("uuid-pty-stable"));
        assert!(!proven_details.matches_incarnation("uuid-different"));
    }

    #[test]
    fn pane_liveness_contract_explicit_capability_negotiation() {
        let legacy_hs = json!({
            "type": "handshakeOk",
            "version": 5,
            "pid": 1234,
            "epoch": 100
        });
        let decoded: DaemonResponse = serde_json::from_value(legacy_hs).expect("decode legacy handshake");
        match decoded {
            DaemonResponse::HandshakeOk { capabilities, .. } => {
                assert!(capabilities.is_empty());
                assert!(!capabilities.contains(&LOCAL_SPLIT_LIFECYCLE_CAPABILITY.to_string()));
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let modern_hs = json!({
            "type": "handshakeOk",
            "version": 5,
            "pid": 1234,
            "epoch": 100,
            "capabilities": [LOCAL_SPLIT_LIFECYCLE_CAPABILITY],
            "admissionTimeUnixMs": 1_700_000_000_000_u64
        });
        let decoded: DaemonResponse = serde_json::from_value(modern_hs).expect("decode modern handshake");
        match decoded {
            DaemonResponse::HandshakeOk { capabilities, admission_time_unix_ms, .. } => {
                assert_eq!(capabilities, vec![LOCAL_SPLIT_LIFECYCLE_CAPABILITY]);
                assert_eq!(admission_time_unix_ms, Some(1_700_000_000_000));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn pane_liveness_contract_typed_ipc_error_codes_roundtrip() {
        let codes = [
            (IpcErrorCode::UnsupportedCapability, "UNSUPPORTED_CAPABILITY"),
            (IpcErrorCode::SpawnRequestConflict, "SPAWN_REQUEST_CONFLICT"),
            (IpcErrorCode::SpawnRequestExpired, "SPAWN_REQUEST_EXPIRED"),
            (IpcErrorCode::SpawnEpochChanged, "SPAWN_EPOCH_CHANGED"),
            (IpcErrorCode::SpawnAttemptTimeout, "SPAWN_ATTEMPT_TIMEOUT"),
            (IpcErrorCode::SpawnCancelled, "SPAWN_CANCELLED"),
        ];

        for (code, wire) in codes {
            let val = serde_json::to_value(&code).expect("serialize error code");
            assert_eq!(val, json!(wire));
            let decoded: IpcErrorCode = serde_json::from_value(val).expect("deserialize error code");
            assert_eq!(decoded, code);
            assert_eq!(IpcErrorCode::from_code_str(wire), code);
        }

        let err = IpcError::spawn_request_conflict("duplicate key with changed payload");
        assert_eq!(err.code, IpcErrorCode::SpawnRequestConflict);
    }

    #[test]
    fn pane_liveness_contract_error_details_and_delivery_roundtrip() {
        let details = SplitErrorDetails {
            request_id: "req-123".to_string(),
            origin_epoch: "42".to_string(),
            stage: "create".to_string(),
            delivery: SplitDelivery::Ambiguous,
            operation_state: "pending".to_string(),
            prepared_local_split: None,
        };
        let val = serde_json::to_value(&details).expect("serialize details");
        assert_eq!(val["delivery"], "ambiguous");
        let decoded: SplitErrorDetails = serde_json::from_value(val).expect("decode details");
        assert_eq!(decoded, details);
    }

    #[cfg(feature = "local-split-qa")]
    #[test]
    fn pane_liveness_contract_qa_barrier_reservation_selector_validation() {
        use crate::ipc::qa_barrier::QaReservationSelector;

        let valid_pred = QaReservationSelector {
            run_id: "run-1".to_string(),
            operation_id: "op-1".to_string(),
            client_request_id: Some("req-1".to_string()),
            source_backend_session_id: Some("pty-src".to_string()),
            workspace_id: Some("ws-1".to_string()),
            worktree_path: None,
            target_role: Some("predecessor".to_string()),
        };
        assert!(valid_pred.validate_target_role().is_ok());

        let valid_succ = QaReservationSelector {
            target_role: Some("successor".to_string()),
            ..valid_pred.clone()
        };
        assert!(valid_succ.validate_target_role().is_ok());

        let invalid_session_id = QaReservationSelector {
            target_role: Some("pty-123".to_string()),
            ..valid_pred.clone()
        };
        assert!(invalid_session_id.validate_target_role().is_err());

        let invalid_wildcard = QaReservationSelector {
            target_role: Some("*".to_string()),
            ..valid_pred.clone()
        };
        assert!(invalid_wildcard.validate_target_role().is_err());
    }
}
