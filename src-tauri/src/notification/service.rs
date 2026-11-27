//! Notification dispatch orchestration.
//!
//! The service owns preflight and submission policy. Both the OS permission
//! query and the native submitter sit behind traits so command-level behavior
//! can be tested without ever displaying a real notification.

// allow: SIZE_OK — dispatch policy plus the ordering fence and their unit tests;
// the fence is backend state, so it belongs here rather than in a new module.

use super::model::{
    format_notification, DispatchNotificationRequest, DispatchNotificationResult,
    NotificationContent, NotificationDispatchReason, NotificationPermissionRequestDto,
    NotificationPermissionStatusDto, NotificationProbeOutcome, NotificationProbeResult,
    NotificationSound, NotificationSource, NotificationTarget,
};
use super::permission::{platform_permission_provider, NotificationPermissionProvider};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Seam over the native notification backend.
///
/// Keeping submission behind a trait allows a fake backend in tests and a
/// future platform-specific silent adapter without changing the IPC contract.
pub trait NativeNotificationBackend: Send + Sync {
    fn submit(&self, content: &NotificationContent) -> Result<(), String>;

    /// Submit one notification the order fence already claimed for a sequence.
    ///
    /// `superseded` answers whether a withdrawal at or above this dispatch's sequence has settled
    /// since. A backend whose platform can only withdraw a banner it already holds a slot for
    /// (Linux) asks it *after* registering that slot and *before* showing, so a withdrawal that
    /// landed between the fence claim and the registration is honoured instead of leaving a
    /// banner the fence already records as withdrawn. Backends that cannot race this way keep the
    /// default, which is plain [`submit`](Self::submit).
    fn submit_fenced(
        &self,
        content: &NotificationContent,
        _superseded: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), String> {
        self.submit(content)
    }

    /// Remove the delivered notification for `target`, if the platform can.
    ///
    /// Defaults to a no-op: a backend with no withdrawal primitive still has to
    /// satisfy the seam, and withdrawing something never delivered is not an
    /// error.
    fn withdraw(&self, _target: &NotificationTarget) -> Result<(), String> {
        Ok(())
    }
}

/// Maximum number of pane targets the order fence remembers.
const FENCE_CAPACITY: usize = 1024;

/// Fence key for one pane: `(workspace_id, session_id)`.
type TargetKey = (String, String);

fn target_key(target: &NotificationTarget) -> TargetKey {
    (target.workspace_id.clone(), target.session_id.clone())
}

/// Highest sequence this fence has settled for one pane target.
///
/// Both counters only ever move forward, so a late stale operation is always
/// recognised as stale and can never re-open an older window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TargetFence {
    last_submitted_seq: u64,
    last_withdrawn_seq: u64,
}

impl TargetFence {
    /// Highest sequence acted on for this target, dispatch or withdrawal.
    fn watermark(&self) -> u64 {
        self.last_submitted_seq.max(self.last_withdrawn_seq)
    }

    /// True while a submitted prompt has not been withdrawn yet.
    fn is_live(&self) -> bool {
        self.last_submitted_seq > self.last_withdrawn_seq
    }
}

/// Backend-owned sequence allocator and per-target ordering fence.
///
/// Native delivery runs on blocking threads, so a dispatch and the withdrawal
/// that superseded it can finish in either order. Allocating the sequence when
/// the intent is created and comparing it here lets a late stale operation be
/// dropped instead of resurrecting a prompt the user already dismissed or
/// deleting the newer prompt that replaced it.
#[derive(Debug)]
pub struct NotificationOrderFence {
    next_seq: AtomicU64,
    targets: parking_lot::Mutex<HashMap<TargetKey, TargetFence>>,
}

impl Default for NotificationOrderFence {
    fn default() -> Self {
        Self {
            // 1-based: a fresh fence must start above every value a default
            // `TargetFence` holds, or the first dispatch would look stale.
            next_seq: AtomicU64::new(1),
            targets: parking_lot::Mutex::new(HashMap::new()),
        }
    }
}

impl NotificationOrderFence {
    /// Allocate the sequence number for one dispatch or withdrawal.
    pub fn next_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Has a withdrawal at or above `seq` already settled for `key`?
    ///
    /// Asked by a backend after it registers its banner slot and before it shows: a `true` answer
    /// means the withdrawal found no slot to close, so this dispatch has to close its own.
    fn superseded(&self, key: &TargetKey, seq: u64) -> bool {
        self.targets
            .lock()
            .get(key)
            .is_some_and(|entry| entry.last_withdrawn_seq >= seq)
    }

    /// Record a withdrawal's watermark without a backend call, for tests that drive the ordering
    /// between the fence claim and the banner registration.
    #[cfg(test)]
    fn withdraw_for_test(&self, target: &NotificationTarget, seq: u64) {
        let mut targets = self.targets.lock();
        let entry = fence_entry(&mut targets, target_key(target));
        if entry.last_submitted_seq <= seq {
            entry.last_withdrawn_seq = entry.last_withdrawn_seq.max(seq);
        }
    }
}

/// Insert-or-get the fence entry for `key`, keeping the map bounded.
///
/// At capacity, settled entries (submitted and already withdrawn) go first:
/// they can never block a future operation. If every entry is still live, the
/// entry with the smallest watermark is dropped because its window closed
/// longest ago.
fn fence_entry(targets: &mut HashMap<TargetKey, TargetFence>, key: TargetKey) -> &mut TargetFence {
    if !targets.contains_key(&key) && targets.len() >= FENCE_CAPACITY {
        // Withdrawal-only entries (`last_submitted_seq == 0`) stay: dropping one
        // would forget a watermark an older queued dispatch still has to lose to.
        targets.retain(|_, entry| entry.is_live() || entry.last_submitted_seq == 0);
        if targets.len() >= FENCE_CAPACITY {
            let oldest = targets
                .iter()
                .min_by_key(|(_, entry)| entry.watermark())
                .map(|(oldest, _)| oldest.clone());
            if let Some(oldest) = oldest {
                targets.remove(&oldest);
            }
        }
    }
    targets.entry(key).or_default()
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
    pub fn probe_delivery(
        &self,
        send_test: bool,
        sound: NotificationSound,
    ) -> NotificationProbeResult {
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
            sound,
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

    /// Format, preflight, and submit one notification under the order fence.
    ///
    /// `seq` must be allocated by [`NotificationOrderFence::next_seq`] when the
    /// intent is created, not when the blocking work starts. A dispatch older
    /// than anything the fence already settled for the same pane is dropped:
    /// either a newer prompt replaced it or the user already dismissed it, and
    /// resubmitting would resurrect a banner the app has moved past.
    pub fn dispatch_sequenced(
        &self,
        request: &DispatchNotificationRequest,
        fence: &NotificationOrderFence,
        seq: u64,
    ) -> DispatchNotificationResult {
        let key = request.target.as_ref().map(target_key);
        if let Some(key) = &key {
            let targets = fence.targets.lock();
            if targets.get(key).is_some_and(|entry| seq <= entry.watermark()) {
                return DispatchNotificationResult::suppressed();
            }
        }

        let status = self.permissions.status();
        if let PreflightDecision::Reject(reason) = preflight(&status) {
            return DispatchNotificationResult::rejected(reason);
        }

        let content = format_notification(request);
        if let Some(key) = &key {
            // Re-check and claim the sequence under the lock so the submit
            // below can never be ordered behind a withdrawal that already
            // settled, and a later dispatch can never claim a lower sequence.
            let mut targets = fence.targets.lock();
            let entry = fence_entry(&mut targets, key.clone());
            if seq <= entry.watermark() {
                return DispatchNotificationResult::suppressed();
            }
            entry.last_submitted_seq = entry.last_submitted_seq.max(seq);
        }

        // A withdrawal can settle between that claim and the backend's banner registration, and a
        // slot registered after it would never be closed. The backend re-asks this once its slot
        // exists and before the banner is shown.
        let superseded = || match &key {
            Some(key) => fence.superseded(key, seq),
            None => false,
        };

        match self.backend.submit_fenced(&content, &superseded) {
            Ok(()) => DispatchNotificationResult::submitted(),
            Err(error) => {
                tracing::warn!("native notification submission failed: {error}");
                DispatchNotificationResult::rejected(NotificationDispatchReason::BackendError)
            }
        }
    }

    /// Remove the delivered notification for `target` under the order fence.
    ///
    /// Deliberately runs no permission preflight: the banner is already on
    /// screen, and revocation of alerts does not make it disappear. A
    /// withdrawal older than a prompt the fence already accepted is dropped, so
    /// a late stale recall cannot delete the newer prompt that replaced it.
    pub fn withdraw_sequenced(
        &self,
        target: &NotificationTarget,
        fence: &NotificationOrderFence,
        seq: u64,
    ) -> Result<(), String> {
        {
            let mut targets = fence.targets.lock();
            let entry = fence_entry(&mut targets, target_key(target));
            if entry.last_submitted_seq > seq {
                return Ok(());
            }
            entry.last_withdrawn_seq = entry.last_withdrawn_seq.max(seq);
        }
        self.backend.withdraw(target)
    }

    /// Withdraw the delivered notification for `target` without ordering.
    pub fn withdraw(&self, target: &NotificationTarget) -> Result<(), String> {
        self.backend.withdraw(target)
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
        pub withdrawn: Mutex<Vec<NotificationTarget>>,
        pub fail_with: Option<String>,
    }

    impl FakeBackend {
        pub fn failing(message: &str) -> Self {
            Self {
                submissions: Mutex::new(Vec::new()),
                withdrawn: Mutex::new(Vec::new()),
                fail_with: Some(message.to_string()),
            }
        }

        pub fn count(&self) -> usize {
            self.submissions.lock().expect("submissions lock").len()
        }

        pub fn withdrawn_count(&self) -> usize {
            self.withdrawn.lock().expect("withdrawn lock").len()
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

        fn withdraw(&self, target: &NotificationTarget) -> Result<(), String> {
            self.withdrawn
                .lock()
                .expect("withdrawn lock")
                .push(target.clone());
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
            badges_enabled: Some(authorization == NotificationAuthorization::Authorized),
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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

        let probe = service.probe_delivery(false, NotificationSound::System);

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

        let probe = service.probe_delivery(true, NotificationSound::System);

        assert_eq!(probe.outcome, NotificationProbeOutcome::Submitted);
        assert!(probe.test_submitted);
        assert_eq!(backend.count(), 1);
        assert_eq!(
            backend.first_title().as_deref(),
            Some("Ferryx notifications are working")
        );
    }

    #[test]
    fn probe_reports_blocked_without_sending_a_test() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Denied),
            Arc::clone(&backend),
        );

        let probe = service.probe_delivery(true, NotificationSound::System);

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

        let probe = service.probe_delivery(true, NotificationSound::System);

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

        let probe = service.probe_delivery(true, NotificationSound::System);

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

    // -----------------------------------------------------------------------
    // Sequenced dispatch and withdrawal
    // -----------------------------------------------------------------------

    fn targeted_request(workspace_id: &str, session_id: &str) -> DispatchNotificationRequest {
        DispatchNotificationRequest {
            source: NotificationSource::AgentTaskComplete,
            agent_label: Some("Claude".into()),
            target: Some(NotificationTarget {
                workspace_id: workspace_id.into(),
                session_id: session_id.into(),
            }),
            ..Default::default()
        }
    }

    fn target_of(workspace_id: &str, session_id: &str) -> NotificationTarget {
        NotificationTarget {
            workspace_id: workspace_id.into(),
            session_id: session_id.into(),
        }
    }

    #[test]
    fn withdrawal_reaches_the_backend_without_permission_preflight() {
        // The banner is already on screen; revoking alerts later does not make
        // it disappear, so withdrawal must not consult the permission status.
        for status in [
            macos_status(NotificationAuthorization::Denied),
            macos_status(NotificationAuthorization::NotDetermined),
            NotificationPermissionStatusDto::unsupported(NotificationPlatform::Other),
        ] {
            let backend = Arc::new(FakeBackend::default());
            let service = service_with(status.clone(), Arc::clone(&backend));
            let fence = NotificationOrderFence::default();
            let target = target_of("ws-1", "fe-1");

            let outcome = service.withdraw_sequenced(&target, &fence, fence.next_seq());

            assert_eq!(outcome, Ok(()), "status {status:?}");
            assert_eq!(backend.withdrawn_count(), 1, "status {status:?}");
            assert_eq!(backend.count(), 0, "status {status:?}");
        }
    }

    #[test]
    fn late_stale_dispatch_does_not_resurrect_a_settled_pane() {
        // Blocking threads can finish in any order: the newest prompt lands
        // first, then the withdrawal that superseded the middle one, then the
        // oldest dispatch. The oldest must be dropped, and the withdrawal must
        // not delete the prompt that is still live.
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );
        let fence = NotificationOrderFence::default();
        let request = targeted_request("ws-1", "fe-1");
        let target = target_of("ws-1", "fe-1");
        let seq1 = fence.next_seq();
        let seq2 = fence.next_seq();
        let seq3 = fence.next_seq();

        let newest = service.dispatch_sequenced(&request, &fence, seq3);
        let stale_withdraw = service.withdraw_sequenced(&target, &fence, seq2);
        let oldest = service.dispatch_sequenced(&request, &fence, seq1);

        assert!(newest.submitted);
        assert_eq!(stale_withdraw, Ok(()));
        assert_eq!(oldest, DispatchNotificationResult::suppressed());
        assert_eq!(backend.count(), 1);
        assert_eq!(backend.withdrawn_count(), 0);
    }

    #[test]
    fn withdrawal_that_finished_first_suppresses_the_stale_dispatch() {
        // The reverse race: the recall settles before the dispatch it
        // supersedes even starts. Submitting would show a banner the user has
        // already dismissed.
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );
        let fence = NotificationOrderFence::default();
        let request = targeted_request("ws-1", "fe-1");
        let target = target_of("ws-1", "fe-1");
        let seq1 = fence.next_seq();
        let seq2 = fence.next_seq();

        assert_eq!(
            service.withdraw_sequenced(&target, &fence, seq2),
            Ok(())
        );
        let stale = service.dispatch_sequenced(&request, &fence, seq1);

        assert_eq!(stale, DispatchNotificationResult::suppressed());
        assert_eq!(backend.count(), 0);
    }

    #[test]
    fn sequences_allocated_after_a_withdrawal_still_deliver_and_withdraw() {
        // A reloaded frontend keeps the same backend-owned allocator, so new
        // sequences are always above everything the fence already settled.
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );
        let fence = NotificationOrderFence::default();
        let request = targeted_request("ws-1", "fe-1");
        let target = target_of("ws-1", "fe-1");

        assert_eq!(
            service.withdraw_sequenced(&target, &fence, fence.next_seq()),
            Ok(())
        );
        let delivered = service.dispatch_sequenced(&request, &fence, fence.next_seq());
        let withdrawn = service.withdraw_sequenced(&target, &fence, fence.next_seq());

        assert!(delivered.submitted);
        assert_eq!(withdrawn, Ok(()));
        assert_eq!(backend.count(), 1);
        assert_eq!(backend.withdrawn_count(), 2);
    }

    #[test]
    fn withdrawal_keeps_delegating_through_the_unordered_path() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );
        let target = target_of("ws-1", "fe-1");

        assert_eq!(service.withdraw(&target), Ok(()));
        assert_eq!(backend.withdrawn_count(), 1);
    }

    #[test]
    fn a_withdrawal_that_settles_before_the_banner_shows_suppresses_it() {
        // The fence claims the dispatch, then a withdrawal settles, then the backend registers its
        // banner slot. Without the post-register check the banner would outlive the fence.
        struct RacingBackend {
            fence: Arc<NotificationOrderFence>,
            target: NotificationTarget,
            seq: u64,
            shown: Arc<AtomicUsize>,
        }
        impl NativeNotificationBackend for RacingBackend {
            fn submit(&self, _content: &NotificationContent) -> Result<(), String> {
                self.shown.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn submit_fenced(
                &self,
                _content: &NotificationContent,
                superseded: &(dyn Fn() -> bool + Send + Sync),
            ) -> Result<(), String> {
                // The withdrawal lands exactly between the fence claim and this registration.
                self.fence.withdraw_for_test(&self.target, self.seq + 1);
                if superseded() {
                    return Ok(());
                }
                self.submit(_content)
            }
        }

        let shown = Arc::new(AtomicUsize::new(0));
        let fence = Arc::new(NotificationOrderFence::default());
        let target = target_of("ws-1", "fe-1");
        let request = targeted_request("ws-1", "fe-1");
        let seq = fence.next_seq();
        let backend = RacingBackend {
            fence: Arc::clone(&fence),
            target: target.clone(),
            seq,
            shown: Arc::clone(&shown),
        };
        let service = NotificationService::new(
            Box::new(FakePermissionProvider::new(macos_status(
                NotificationAuthorization::Authorized,
            ))),
            Box::new(backend),
        );

        let result = service.dispatch_sequenced(&request, &fence, seq);

        assert!(result.submitted, "a withdrawn dispatch is not a failure");
        assert_eq!(shown.load(Ordering::SeqCst), 0, "the banner was never shown");
    }

    #[test]
    fn an_unwithdrawn_dispatch_still_shows_through_the_fenced_seam() {
        let backend = Arc::new(FakeBackend::default());
        let service = service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(&backend),
        );
        let fence = NotificationOrderFence::default();

        let result = service.dispatch_sequenced(&targeted_request("ws-1", "fe-1"), &fence, fence.next_seq());

        assert!(result.submitted);
        assert_eq!(backend.count(), 1, "the default seam forwards to submit");
    }

    #[test]
    fn fence_prunes_settled_targets_before_live_ones_at_capacity() {
        // An unbounded map would grow with every pane ever notified. Settled
        // entries go first: they can never block a future operation.
        let mut targets: HashMap<TargetKey, TargetFence> = HashMap::new();
        for index in 0..FENCE_CAPACITY {
            let key = (format!("ws-{index}"), "fe".to_string());
            targets.insert(
                key,
                TargetFence {
                    last_submitted_seq: index as u64 + 1,
                    last_withdrawn_seq: index as u64 + 1,
                },
            );
        }
        let live_key = ("ws-live".to_string(), "fe".to_string());
        targets.insert(
            live_key.clone(),
            TargetFence {
                last_submitted_seq: 1,
                last_withdrawn_seq: 0,
            },
        );

        let entry = fence_entry(&mut targets, ("ws-new".to_string(), "fe".to_string()));

        assert_eq!(entry.last_submitted_seq, 0);
        assert!(targets.contains_key(&live_key));
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn fence_keeps_withdrawal_only_watermarks_at_capacity() {
        let mut targets: HashMap<TargetKey, TargetFence> = HashMap::new();
        for index in 0..FENCE_CAPACITY - 1 {
            targets.insert(
                (format!("ws-{index}"), "fe".to_string()),
                TargetFence {
                    last_submitted_seq: index as u64 + 1,
                    last_withdrawn_seq: index as u64 + 1,
                },
            );
        }
        let withdrawn_key = ("ws-withdrawn".to_string(), "fe".to_string());
        targets.insert(
            withdrawn_key.clone(),
            TargetFence {
                last_submitted_seq: 0,
                last_withdrawn_seq: 9_999,
            },
        );

        fence_entry(&mut targets, ("ws-new".to_string(), "fe".to_string()));

        assert_eq!(targets.get(&withdrawn_key).map(|e| e.last_withdrawn_seq), Some(9_999));
        assert_eq!(targets.len(), 2);
    }
}
