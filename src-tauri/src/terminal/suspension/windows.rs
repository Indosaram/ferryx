use super::{ActuationReceipt, SuspensionError, SuspensionSource, SuspensionTarget};

fn unsupported() -> SuspensionError {
    SuspensionError::UnsupportedPlatform(
        "declared Windows features provide no identity-bound suspension and stop observation backend",
    )
}

pub(super) fn stop_for_owned_suspension(
    _target: &SuspensionTarget,
) -> Result<ActuationReceipt, SuspensionError> {
    Err(unsupported())
}

pub(super) fn classify_stop_source(
    _target: &SuspensionTarget,
) -> Result<SuspensionSource, SuspensionError> {
    Err(unsupported())
}

pub(super) fn resume_owned(_target: &SuspensionTarget) -> Result<(), SuspensionError> {
    Err(unsupported())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_backend_never_claims_or_resumes_a_stop() {
        let target = SuspensionTarget {
            pid: 42,
            incarnation: "pane-a".into(),
            started_at_unix_ms: Some(100),
        };
        assert!(matches!(
            stop_for_owned_suspension(&target),
            Err(SuspensionError::UnsupportedPlatform(_))
        ));
        assert!(matches!(
            classify_stop_source(&target),
            Err(SuspensionError::UnsupportedPlatform(_))
        ));
        assert!(matches!(
            resume_owned(&target),
            Err(SuspensionError::UnsupportedPlatform(_))
        ));
    }
}
