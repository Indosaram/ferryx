//! Notification dispatch orchestration.
//!
//! The service owns preflight and submission policy. Both the OS permission
//! query and the native submitter sit behind traits so command-level behavior
//! can be tested without ever displaying a real notification.

use super::model::{
    format_notification, DispatchNotificationRequest, DispatchNotificationResult,
    NotificationContent, NotificationDispatchReason, NotificationPermissionRequestDto,
    NotificationPermissionStatusDto, NotificationProbeOutcome, NotificationProbeResult,
    NotificationSource,
};
use super::permission::{platform_permission_provider, NotificationPermissionProvider};

/// Seam over the native notification backend.
///
/// Keeping submission behind a trait allows a fake backend in tests and a
/// future platform-specific silent adapter without changing the IPC contract.
pub trait NativeNotificationBackend: Send + Sync {
    fn submit(&self, content: &NotificationContent) -> Result<(), String>;
}

/// Preflight decision made before any submission is attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightDecision {
    Submit,
    Reject(NotificationDispatchReason),
}

/// Decide whether a dispatch may proceed for a given permission status.
///
/// Only an authoritative query can block: a non-authoritative `unknown` (every
/// current Windows/Linux build) is allowed through so those platforms keep
/// working, while macOS `denied`/`not-determined` short-circuit before the
/// backend can claim a bogus success.
pub fn preflight(status: &NotificationPermissionStatusDto) -> PreflightDecision {
    if !status.supported {
        return PreflightDecision::Reject(NotificationDispatchReason::Unsupported);
    }
    if status.is_blocked() {
        return PreflightDecision::Reject(NotificationDispatchReason::BlockedBySystem);
    }
    if status.needs_permission_request() {
        // Never prompt implicitly: the UI must ask via an explicit user action.
        return PreflightDecision::Reject(NotificationDispatchReason::PermissionRequired);
    }
    PreflightDecision::Submit
}

/// Notification domain service.
pub struct NotificationService {
    permissions: Box<dyn NotificationPermissionProvider>,
    backend: Box<dyn NativeNotificationBackend>,
}

impl NotificationService {
    pub fn new(
        permissions: Box<dyn NotificationPermissionProvider>,
        backend: Box<dyn NativeNotificationBackend>,
    ) -> Self {
        Self {
            permissions,
            backend,
        }
    }

    /// Service wired to the real platform permission provider.
    pub fn with_platform_permissions(backend: Box<dyn NativeNotificationBackend>) -> Self {
        Self::new(platform_permission_provider(), backend)
    }

    pub fn permission_status(&self) -> NotificationPermissionStatusDto {
        self.permissions.status()
    }

    /// Explicit, user-initiated authorization request.
    pub fn request_permission(&self) -> NotificationPermissionRequestDto {
        self.permissions.request()
    }

    /// Format, preflight, and submit one notification.
    pub fn dispatch(&self, request: &DispatchNotificationRequest) -> DispatchNotificationResult {
        let status = self.permissions.status();
        if let PreflightDecision::Reject(reason) = preflight(&status) {
            return DispatchNotificationResult::rejected(reason);
        }

        let content = format_notification(request);
        match self.backend.submit(&content) {
            Ok(()) => DispatchNotificationResult::submitted(),
            Err(error) => {
                tracing::warn!("native notification submission failed: {error}");
                DispatchNotificationResult::rejected(NotificationDispatchReason::BackendError)
            }
        }
    }

    /// Report readiness, optionally submitting one visible test notification.
    ///
    /// Reports `submitted`, never `visibly-delivered`: Focus and
    /// Do-Not-Disturb can suppress a banner with no signal back to rorca.
    pub fn probe_delivery(&self, send_test: bool) -> NotificationProbeResult {
        let status = self.permissions.status();

        let outcome = match preflight(&status) {
            PreflightDecision::Reject(NotificationDispatchReason::BlockedBySystem) => {
                NotificationProbeOutcome::BlockedBySystem
            }
            PreflightDecision::Reject(NotificationDispatchReason::PermissionRequired) => {
                NotificationProbeOutcome::PermissionRequired
            }
            PreflightDecision::Reject(NotificationDispatchReason::Unsupported) => {
                NotificationProbeOutcome::Unsupported
            }
            PreflightDecision::Reject(NotificationDispatchReason::BackendError) => {
                NotificationProbeOutcome::Failed
            }
            PreflightDecision::Submit => NotificationProbeOutcome::Ready,
        };

        if outcome != NotificationProbeOutcome::Ready || !send_test {
            return NotificationProbeResult {
                outcome,
                status,
                test_submitted: false,
            };
        }

        let content = format_notification(&DispatchNotificationRequest {
            source: NotificationSource::Test,
            ..Default::default()
        });

        match self.backend.submit(&content) {
            Ok(()) => NotificationProbeResult {
                outcome: NotificationProbeOutcome::Submitted,
                status,
                test_submitted: true,
            },
            Err(error) => {
                tracing::warn!("notification probe submission failed: {error}");
                NotificationProbeResult {
                    outcome: NotificationProbeOutcome::Failed,
                    status,
                    test_submitted: false,
                }
            }
        }
    }
}

impl std::fmt::Debug for NotificationService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationService")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::notification::model::{NotificationAuthorization, NotificationPlatform};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// Permission provider returning a scripted status.
    pub struct FakePermissionProvider {
        pub status: NotificationPermissionStatusDto,
        pub request_calls: Arc<AtomicUsize>,
    }

    impl FakePermissionProvider {
        pub fn new(status: NotificationPermissionStatusDto) -> Self {
            Self {
                status,
                request_calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl NotificationPermissionProvider for FakePermissionProvider {
        fn status(&self) -> NotificationPermissionStatusDto {
            self.status.clone()
        }

        fn request(&self) -> NotificationPermissionRequestDto {
            self.request_calls.fetch_add(1, Ordering::SeqCst);
            NotificationPermissionRequestDto {
                granted: self.status.authorization == NotificationAuthorization::Authorized,
                status: self.status.clone(),
                error: None,
            }
        }
    }

    /// Backend recording submissions instead of showing notifications.
    #[derive(Default)]
    pub struct FakeBackend {
        pub submissions: Mutex<Vec<NotificationContent>>,
        pub fail_with: Option<String>,
    }

    impl FakeBackend {
        pub fn failing(message: &str) -> Self {
            Self {
                submissions: Mutex::new(Vec::new()),
                fail_with: Some(message.to_string()),
            }
        }

        pub fn count(&self) -> usize {
            self.submissions.lock().expect("submissions lock").len()
        }

        pub fn first_title(&self) -> Option<String> {
            self.submissions
                .lock()
                .expect("submissions lock")
                .first()
                .map(|content| content.title.clone())
        }

        pub fn first_body(&self) -> Option<String> {
            self.submissions
                .lock()
                .expect("submissions lock")
                .first()
                .map(|content| content.body.clone())
        }
    }

    impl NativeNotificationBackend for Arc<FakeBackend> {
        fn submit(&self, content: &NotificationContent) -> Result<(), String> {
            if let Some(error) = &self.fail_with {
                return Err(error.clone());
            }
            self.submissions
                .lock()
                .expect("submissions lock")
                .push(content.clone());
            Ok(())
        }
    }

    /// Authoritative macOS-shaped status for a given authorization value.
    pub fn macos_status(
        authorization: NotificationAuthorization,
    ) -> NotificationPermissionStatusDto {
        NotificationPermissionStatusDto {
            platform: NotificationPlatform::Macos,
            supported: true,
            authorization,
            alerts_enabled: Some(authorization == NotificationAuthorization::Authorized),
            sounds_enabled: Some(authorization == NotificationAuthorization::Authorized),
            requested: authorization != NotificationAuthorization::NotDetermined,
            authoritative: true,
            can_open_settings: true,
        }
    }

    /// Service backed entirely by fakes.
    pub fn service_with(
        status: NotificationPermissionStatusDto,
        backend: Arc<FakeBackend>,
    ) -> NotificationService {
        NotificationService::new(
            Box::new(FakePermissionProvider::new(status)),
            Box::new(backend),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::notification::model::{NotificationAuthorization, NotificationPlatform};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    fn agent_request() -> DispatchNotificationRequest {
        DispatchNotificationRequest {
            source: NotificationSource::AgentTaskComplete,
            agent_label: Some("Claude".into()),
            worktree_label: Some("feat/login".into()),
            ..Default::default()
        }
    }

    #[test]
    fn authorized_status_submits_exactly_one_notification() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );

        let result = service.dispatch(&agent_request());

        assert!(result.submitted);
        assert!(result.reason.is_none());
        assert_eq!(backend.count(), 1);
        assert_eq!(backend.first_title().as_deref(), Some("Claude finished"));
        assert_eq!(backend.first_body().as_deref(), Some("feat/login"));
    }

    #[test]
    fn denied_macos_status_blocks_before_submitting() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Denied),
            Arc::clone(&backend),
        );

        let result = service.dispatch(&agent_request());

        assert!(!result.submitted);
        assert_eq!(
            result.reason,
            Some(NotificationDispatchReason::BlockedBySystem)
        );
        // Critical: the backend must never be reached when the OS denied us.
        assert_eq!(backend.count(), 0);
    }

    #[test]
    fn not_determined_returns_permission_required_without_implicit_prompt() {
        let backend = Arc::new(FakeBackend::default());
        let provider =
            FakePermissionProvider::new(macos_status(NotificationAuthorization::NotDetermined));
        let request_calls = Arc::clone(&provider.request_calls);
        let service = NotificationService::new(Box::new(provider), Box::new(Arc::clone(&backend)));

        let result = service.dispatch(&agent_request());

        assert_eq!(
            result.reason,
            Some(NotificationDispatchReason::PermissionRequired)
        );
        assert_eq!(backend.count(), 0);
        // A background event must never trigger a permission prompt.
        assert_eq!(request_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn backend_failure_is_reported_as_a_structured_reason() {
        let backend = Arc::new(FakeBackend::failing("dbus unavailable"));
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );

        let result = service.dispatch(&agent_request());

        assert!(!result.submitted);
        assert_eq!(
            result.reason,
            Some(NotificationDispatchReason::BackendError)
        );
    }

    #[test]
    fn provisional_authorization_is_allowed_to_dispatch() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Provisional),
            Arc::clone(&backend),
        );

        assert!(service.dispatch(&agent_request()).submitted);
        assert_eq!(backend.count(), 1);
    }

    #[test]
    fn non_authoritative_unknown_status_still_dispatches() {
        // Windows/Linux report unknown; they must keep working rather than
        // being blocked by the absence of an authoritative query.
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            NotificationPermissionStatusDto::non_authoritative(NotificationPlatform::Linux, false),
            Arc::clone(&backend),
        );

        assert!(service.dispatch(&agent_request()).submitted);
        assert_eq!(backend.count(), 1);
    }

    #[test]
    fn unsupported_platform_rejects_dispatch() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            NotificationPermissionStatusDto::unsupported(NotificationPlatform::Other),
            Arc::clone(&backend),
        );

        let result = service.dispatch(&agent_request());

        assert_eq!(result.reason, Some(NotificationDispatchReason::Unsupported));
        assert_eq!(backend.count(), 0);
    }

    #[test]
    fn dispatched_content_is_sanitized_before_reaching_the_backend() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );

        service.dispatch(&DispatchNotificationRequest {
            source: NotificationSource::TerminalBell,
            worktree_label: Some("feat/login".into()),
            terminal_title: Some("run\u{1b}[31m\ntest".into()),
            ..Default::default()
        });

        let body = backend.first_body().expect("submitted body");
        assert!(!body.contains('\n'));
        assert!(!body.contains('\u{1b}'));
    }

    #[test]
    fn probe_without_test_reports_ready_and_sends_nothing() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );

        let probe = service.probe_delivery(false);

        assert_eq!(probe.outcome, NotificationProbeOutcome::Ready);
        assert!(!probe.test_submitted);
        assert_eq!(backend.count(), 0);
    }

    #[test]
    fn probe_with_test_submits_exactly_one_test_notification() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );

        let probe = service.probe_delivery(true);

        assert_eq!(probe.outcome, NotificationProbeOutcome::Submitted);
        assert!(probe.test_submitted);
        assert_eq!(backend.count(), 1);
        assert_eq!(
            backend.first_title().as_deref(),
            Some("rorca notifications are working")
        );
    }

    #[test]
    fn probe_reports_blocked_without_sending_a_test() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Denied),
            Arc::clone(&backend),
        );

        let probe = service.probe_delivery(true);

        assert_eq!(probe.outcome, NotificationProbeOutcome::BlockedBySystem);
        assert!(!probe.test_submitted);
        assert_eq!(backend.count(), 0);
        // The UI needs this to offer "Open System Settings".
        assert!(probe.status.can_open_settings);
    }

    #[test]
    fn probe_reports_permission_required_without_prompting() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::NotDetermined),
            Arc::clone(&backend),
        );

        let probe = service.probe_delivery(true);

        assert_eq!(probe.outcome, NotificationProbeOutcome::PermissionRequired);
        assert!(!probe.test_submitted);
        assert_eq!(backend.count(), 0);
    }

    #[test]
    fn probe_failure_reports_failed_not_submitted() {
        let backend = Arc::new(FakeBackend::failing("backend down"));
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );

        let probe = service.probe_delivery(true);

        assert_eq!(probe.outcome, NotificationProbeOutcome::Failed);
        assert!(!probe.test_submitted);
    }

    #[test]
    fn preflight_decisions_cover_every_authorization_state() {
        assert_eq!(
            preflight(&macos_status(NotificationAuthorization::Authorized)),
            PreflightDecision::Submit
        );
        assert_eq!(
            preflight(&macos_status(NotificationAuthorization::Provisional)),
            PreflightDecision::Submit
        );
        assert_eq!(
            preflight(&macos_status(NotificationAuthorization::Denied)),
            PreflightDecision::Reject(NotificationDispatchReason::BlockedBySystem)
        );
        assert_eq!(
            preflight(&macos_status(NotificationAuthorization::NotDetermined)),
            PreflightDecision::Reject(NotificationDispatchReason::PermissionRequired)
        );
        assert_eq!(
            preflight(&macos_status(NotificationAuthorization::Unknown)),
            PreflightDecision::Submit
        );
    }

    #[test]
    fn non_authoritative_denied_does_not_block() {
        // A denied value without an authoritative query is a guess and must
        // not silently suppress notifications.
        let mut status = macos_status(NotificationAuthorization::Denied);
        status.authoritative = false;

        assert_eq!(preflight(&status), PreflightDecision::Submit);
    }

    #[test]
    fn explicit_permission_request_reaches_the_provider_once() {
        let provider =
            FakePermissionProvider::new(macos_status(NotificationAuthorization::Authorized));
        let request_calls = Arc::clone(&provider.request_calls);
        let service = NotificationService::new(
            Box::new(provider),
            Box::new(Arc::new(FakeBackend::default())),
        );

        let result = service.request_permission();

        assert!(result.granted);
        assert_eq!(request_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn permission_status_passes_through_the_provider_value() {
        let service = service_with(
            macos_status(NotificationAuthorization::Denied),
            Arc::new(FakeBackend::default()),
        );

        let status = service.permission_status();

        assert_eq!(status.authorization, NotificationAuthorization::Denied);
        assert!(status.is_blocked());
    }
}
