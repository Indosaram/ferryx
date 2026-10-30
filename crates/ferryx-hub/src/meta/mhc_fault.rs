//! Typed failures of the retained mhc supervisor engine.
//!
//! Every fault names one structured code, and its free text is secret-redacted when the fault is
//! built, so a fault can be logged, stored or shown without leaking credentials. The supervisor
//! never answers a fault by switching to another model: the caller sees the fault instead.
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use crate::error::HubError;

/// Longest free-text detail a fault keeps, in characters.
pub const FAULT_DETAIL_MAX_CHARS: usize = 512;

/// Why the supervisor engine could not produce a judgment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupervisorFault {
    /// The configured mhc executable was not found on the hub child PATH.
    MissingExecutable { program: String },
    /// The engine does not advertise every capability the supervisor requires.
    Capability { missing: Vec<String> },
    /// The provider refused the credentials (or none are configured).
    Auth { detail: String },
    /// The provider refused for quota, rate limit or billing reasons.
    Quota { detail: String },
    /// Any other provider-side turn failure, including a refused model substitution.
    Provider { detail: String },
    /// The engine broke the wire contract (bad reply shape, refused command, wrong mode).
    Protocol { detail: String },
    /// The engine process could not be started.
    Launch { detail: String },
    /// The engine transport ended. An in-flight turn is never retransmitted.
    EngineExited { detail: String },
    /// The engine ended the session mid-turn. The next use resumes it.
    SessionClosed { reason: String },
    /// A resumed session returned another identity than the persisted one; it was refused.
    SessionMismatch {
        role: String,
        expected: String,
        actual: String,
    },
    /// A bounded deadline passed.
    Timeout { operation: String },
    /// The engine is not running in this process: the hub stopped it, or this process (the
    /// MCP server, a CLI) never owned one. Nothing is launched to answer the call.
    NotRunning { detail: String },
    /// An explicitly configured supervisor turn budget is spent. There is no budget unless the
    /// operator configured one; exhaustion is a visible hold, never another model.
    Budget { detail: String },
    /// Another supervisor process still owns the engine profile (`<data>/mhc-supervisor.lock`)
    /// after the bounded wait. Nothing was launched or sent.
    ProfileBusy { detail: String },
    /// The owner cancelled the decision, or its caller went away.
    Cancelled,
}

impl SupervisorFault {
    /// Stable machine code of the fault.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingExecutable { .. } => "MHC_MISSING_EXECUTABLE",
            Self::Capability { .. } => "MHC_CAPABILITY_MISSING",
            Self::Auth { .. } => "MHC_AUTH",
            Self::Quota { .. } => "MHC_QUOTA",
            Self::Provider { .. } => "MHC_PROVIDER",
            Self::Protocol { .. } => "MHC_PROTOCOL",
            Self::Launch { .. } => "MHC_LAUNCH_FAILED",
            Self::EngineExited { .. } => "MHC_ENGINE_EXITED",
            Self::SessionClosed { .. } => "MHC_SESSION_CLOSED",
            Self::SessionMismatch { .. } => "MHC_SESSION_MISMATCH",
            Self::Timeout { .. } => "MHC_TIMEOUT",
            Self::NotRunning { .. } => "MHC_NOT_RUNNING",
            Self::Budget { .. } => "MHC_BUDGET",
            Self::ProfileBusy { .. } => "MHC_PROFILE_BUSY",
            Self::Cancelled => "MHC_CANCELLED",
        }
    }

    pub fn missing_executable(program: &str) -> Self {
        Self::MissingExecutable {
            program: redact(program),
        }
    }

    pub fn auth(detail: &str) -> Self {
        Self::Auth {
            detail: redact(detail),
        }
    }

    pub fn quota(detail: &str) -> Self {
        Self::Quota {
            detail: redact(detail),
        }
    }

    pub fn provider(detail: &str) -> Self {
        Self::Provider {
            detail: redact(detail),
        }
    }

    pub fn protocol(detail: &str) -> Self {
        Self::Protocol {
            detail: redact(detail),
        }
    }

    pub fn launch(detail: &str) -> Self {
        Self::Launch {
            detail: redact(detail),
        }
    }

    pub fn engine_exited(detail: &str) -> Self {
        Self::EngineExited {
            detail: redact(detail),
        }
    }

    pub fn session_closed(reason: &str) -> Self {
        Self::SessionClosed {
            reason: redact(reason),
        }
    }

    pub fn timeout(operation: &str) -> Self {
        Self::Timeout {
            operation: redact(operation),
        }
    }

    pub fn not_running(detail: &str) -> Self {
        Self::NotRunning {
            detail: redact(detail),
        }
    }

    pub fn budget(detail: &str) -> Self {
        Self::Budget {
            detail: redact(detail),
        }
    }

    pub fn profile_busy(detail: &str) -> Self {
        Self::ProfileBusy {
            detail: redact(detail),
        }
    }

    /// The fault for a turn the engine reported as failed: auth, quota, or a provider failure.
    pub fn from_turn_error(text: &str) -> Self {
        match classify_error_text(text) {
            ErrorClass::Quota => Self::quota(text),
            ErrorClass::Auth => Self::auth(text),
            ErrorClass::Other => Self::provider(text),
        }
    }

    /// The fault for a command the engine refused: auth or quota when the text says so,
    /// otherwise a protocol fault naming the command.
    pub fn from_refusal(command: &str, error: &str, code: Option<&str>) -> Self {
        let text = match code {
            Some(code) if !code.is_empty() => format!("{command} refused ({code}): {error}"),
            _ => format!("{command} refused: {error}"),
        };
        match classify_error_text(error) {
            ErrorClass::Quota => Self::quota(&text),
            ErrorClass::Auth => Self::auth(&text),
            ErrorClass::Other => Self::protocol(&text),
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::MissingExecutable { program } => format!("{program} was not found"),
            Self::Capability { missing } => format!("missing capabilities: {}", missing.join(", ")),
            Self::Auth { detail }
            | Self::Quota { detail }
            | Self::Provider { detail }
            | Self::Protocol { detail }
            | Self::Launch { detail }
            | Self::EngineExited { detail }
            | Self::NotRunning { detail }
            | Self::Budget { detail }
            | Self::ProfileBusy { detail } => detail.clone(),
            Self::SessionClosed { reason } => format!("session closed: {reason}"),
            Self::SessionMismatch {
                role,
                expected,
                actual,
            } => format!("{role} resumed as {actual}, expected {expected}"),
            Self::Timeout { operation } => format!("{operation} timed out"),
            Self::Cancelled => "cancelled".to_owned(),
        }
    }
}

impl std::fmt::Display for SupervisorFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for SupervisorFault {}

impl From<SupervisorFault> for HubError {
    fn from(fault: SupervisorFault) -> Self {
        HubError::message(fault.to_string())
    }
}

/// Which context a typed model turn runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnContext {
    /// A retained decision context.
    Retained,
    /// A fresh context that sees only this prompt (the convergence check).
    Fresh,
}

/// Failure of a typed model turn: a typed supervisor fault, or a legacy backend error.
#[derive(Debug)]
pub enum TurnFault {
    Supervisor(SupervisorFault),
    Other(HubError),
}

impl std::fmt::Display for TurnFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Supervisor(fault) => fault.fmt(formatter),
            Self::Other(error) => error.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorClass {
    Auth,
    Quota,
    Other,
}

fn classify_error_text(text: &str) -> ErrorClass {
    let lower = text.to_ascii_lowercase();
    const QUOTA: [&str; 10] = [
        "429",
        "quota",
        "rate limit",
        "rate_limit",
        "ratelimit",
        "too many requests",
        "usage limit",
        "billing",
        "credit balance",
        "resource_exhausted",
    ];
    const AUTH: [&str; 13] = [
        "401",
        "403",
        "unauthorized",
        "unauthenticated",
        "authentication",
        "invalid api key",
        "invalid x-api-key",
        "api key",
        "no api key",
        "not logged in",
        "login required",
        "credential",
        "oauth token",
    ];
    if QUOTA.iter().any(|needle| lower.contains(needle)) {
        ErrorClass::Quota
    } else if AUTH.iter().any(|needle| lower.contains(needle)) {
        ErrorClass::Auth
    } else {
        ErrorClass::Other
    }
}

fn redaction_rules() -> &'static [(Regex, &'static str)] {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            (
                r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=\-]{6,}",
                "$1 [redacted]",
            ),
            (
                r#"(?i)\b(api[_-]?key|x-api-key|access[_-]?token|refresh[_-]?token|token|secret|password|authorization)(["']?\s*[:=]\s*["']?)[^\s"',;]{4,}"#,
                "$1$2[redacted]",
            ),
            (r"\b(sk|pk|rk)-[A-Za-z0-9_\-]{8,}", "[redacted]"),
            (r"\b(ghp|gho|ghs|xox[abprs]|AIza)[A-Za-z0-9_\-]{8,}", "[redacted]"),
            (r"[A-Za-z0-9+=_\-]{40,}", "[redacted]"),
        ]
        .into_iter()
        .map(|(pattern, replacement)| {
            (
                Regex::new(pattern).expect("fault redaction pattern compiles"),
                replacement,
            )
        })
        .collect()
    })
}

/// Masks credential-like material and caps the text at [`FAULT_DETAIL_MAX_CHARS`].
pub fn redact(text: &str) -> String {
    let mut redacted = text.to_owned();
    for (pattern, replacement) in redaction_rules() {
        redacted = pattern.replace_all(&redacted, *replacement).into_owned();
    }
    if redacted.chars().count() > FAULT_DETAIL_MAX_CHARS {
        let mut capped: String = redacted.chars().take(FAULT_DETAIL_MAX_CHARS).collect();
        capped.push_str("...");
        capped
    } else {
        redacted
    }
}
