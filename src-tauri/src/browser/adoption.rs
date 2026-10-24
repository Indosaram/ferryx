//! Adoption acknowledgements for CLI-opened browser sessions.
//!
//! `ferryx browser open` creates a backend session, but only the GUI knows whether that
//! session reached a visible tab: an active remote host, a duplicate `browserId`, or a failed
//! pane adoption all leave the panel hidden. The CLI must be able to say which one happened
//! instead of reporting a tab nobody can see.
//!
//! The GUI answers through [`cmd_browser_session_adoption`], resolving the waiter that the CLI
//! registered with [`expect`] BEFORE it emitted `browser_session_created`. Registration is
//! deliberately first: a fast GUI could otherwise answer before anyone was listening.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::oneshot;

#[cfg(feature = "desktop")]
use crate::ipc::error::IpcError;

/// How long the CLI waits for the GUI to report whether the tab was shown.
pub const ADOPTION_ACK_TIMEOUT: Duration = Duration::from_secs(3);

/// The GUI skipped adoption because a remote host owns the desktop session.
pub const ADOPTION_REASON_REMOTE_HOST_ACTIVE: &str = "remote-host-active";
/// The GUI could not put the session into a tab (duplicate id, workspace switch, ...).
pub const ADOPTION_REASON_ADOPT_FAILED: &str = "adopt-failed";
/// No GUI answered within [`ADOPTION_ACK_TIMEOUT`]; the tab state is unknown.
pub const ADOPTION_REASON_NO_GUI_ACK: &str = "no-gui-ack";

/// Whether the GUI actually showed the tab for a browser session.
///
/// `adopted: None` means unknown: the CLI must not claim the tab is visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAdoption {
    pub adopted: Option<bool>,
    pub reason: Option<String>,
}

fn no_gui_ack() -> BrowserAdoption {
    BrowserAdoption {
        adopted: None,
        reason: Some(ADOPTION_REASON_NO_GUI_ACK.to_string()),
    }
}

type Waiter = oneshot::Sender<BrowserAdoption>;

fn pending_waiters() -> &'static Mutex<HashMap<String, Waiter>> {
    static PENDING: OnceLock<Mutex<HashMap<String, Waiter>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registers a waiter for `browser_id` and returns its receiver.
///
/// Call this BEFORE emitting the event that lets the GUI answer; otherwise a fast GUI can
/// resolve before the waiter exists.
pub fn expect(browser_id: &str) -> oneshot::Receiver<BrowserAdoption> {
    let (sender, receiver) = oneshot::channel();
    pending_waiters().lock().insert(browser_id.to_string(), sender);
    receiver
}

/// Delivers `adoption` to the waiter registered for `browser_id`.
///
/// Returns `false` when nobody was waiting: no GUI ever registered a waiter for that id, the
/// answer arrived after the timeout removed the entry, or the waiting task was dropped. A late
/// answer is therefore never mistaken for a delivered one.
pub fn resolve(browser_id: &str, adoption: BrowserAdoption) -> bool {
    match pending_waiters().lock().remove(browser_id) {
        Some(sender) => sender.send(adoption).is_ok(),
        None => false,
    }
}

/// Drops a pending waiter without answering it, so a later GUI answer is not recorded as
/// delivered.
pub fn forget(browser_id: &str) -> bool {
    pending_waiters().lock().remove(browser_id).is_some()
}

/// Awaits the GUI verdict for `browser_id`, bounded by [`ADOPTION_ACK_TIMEOUT`].
///
/// A delivered answer, a dropped sender, and an elapsed deadline are three distinct outcomes;
/// only the first carries GUI state, and the deadline path also forgets the waiter.
pub async fn await_ack(
    browser_id: &str,
    receiver: oneshot::Receiver<BrowserAdoption>,
) -> BrowserAdoption {
    match tokio::time::timeout(ADOPTION_ACK_TIMEOUT, receiver).await {
        Ok(Ok(adoption)) => adoption,
        Ok(Err(_dropped)) => no_gui_ack(),
        Err(_elapsed) => {
            forget(browser_id);
            no_gui_ack()
        }
    }
}

#[cfg(feature = "desktop")]
/// Reports whether the GUI showed the tab for a CLI-opened browser session.
///
/// Returns `false` when no waiter was registered, so the GUI cannot claim credit for a
/// session the CLI never opened.
#[tauri::command]
pub async fn cmd_browser_session_adoption(
    browser_id: String,
    adopted: bool,
    reason: Option<String>,
) -> Result<bool, IpcError> {
    Ok(resolve(
        &browser_id,
        BrowserAdoption {
            adopted: Some(adopted),
            reason,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(adopted: bool) -> BrowserAdoption {
        BrowserAdoption {
            adopted: Some(adopted),
            reason: None,
        }
    }

    #[tokio::test]
    async fn adoption_ack_reaches_the_registered_waiter() {
        // Given a CLI that registered its waiter before emitting, when the GUI resolves it,
        // then the waiting caller receives the GUI's verdict.
        let receiver = expect("adoption-ack-delivers");

        assert!(resolve("adoption-ack-delivers", verdict(true)));

        assert_eq!(
            receiver.await.expect("waiter receives the GUI verdict"),
            verdict(true)
        );
    }

    #[test]
    fn resolve_reports_when_nobody_was_waiting() {
        // Given no waiter for this browser id, when a GUI answer arrives, then it is refused
        // rather than silently accepted.
        assert!(!resolve("adoption-nobody-waits", verdict(false)));
    }

    #[tokio::test]
    async fn adoption_ack_times_out_before_any_answer_and_drops_the_waiter() {
        // Given a registered waiter, when no GUI answers, then the bounded wait elapses and the
        // pending entry is gone so a late answer cannot be delivered.
        let receiver = expect("adoption-ack-times-out");

        assert!(
            tokio::time::timeout(Duration::ZERO, receiver).await.is_err(),
            "an unanswered wait must surface as a timeout, not as a verdict"
        );
        assert!(
            forget("adoption-ack-times-out"),
            "the timed-out waiter is removed"
        );
        assert!(
            !resolve("adoption-ack-times-out", verdict(true)),
            "an answer arriving after the timeout is not delivered"
        );
    }

    #[tokio::test]
    async fn await_ack_returns_the_delivered_verdict() {
        // Given a GUI answer that arrives in time, when the CLI waits, then it returns the GUI's
        // verdict instead of a timeout placeholder.
        let receiver = expect("adoption-await-ack-delivers");
        assert!(resolve(
            "adoption-await-ack-delivers",
            BrowserAdoption {
                adopted: Some(false),
                reason: Some(ADOPTION_REASON_REMOTE_HOST_ACTIVE.to_string()),
            }
        ));

        assert_eq!(
            await_ack("adoption-await-ack-delivers", receiver).await,
            BrowserAdoption {
                adopted: Some(false),
                reason: Some(ADOPTION_REASON_REMOTE_HOST_ACTIVE.to_string()),
            }
        );
    }

    #[test]
    fn browser_adoption_serializes_camel_case_and_keeps_unknown_as_null() {
        // Given an unknown verdict, when it is serialized, then `adopted` is null and the reason
        // code is preserved verbatim for the CLI's JSON output.
        assert_eq!(
            serde_json::to_value(BrowserAdoption {
                adopted: None,
                reason: Some(ADOPTION_REASON_NO_GUI_ACK.to_string()),
            })
            .expect("serialize adoption"),
            serde_json::json!({ "adopted": null, "reason": "no-gui-ack" })
        );

        assert_eq!(
            serde_json::to_value(BrowserAdoption {
                adopted: Some(false),
                reason: Some(ADOPTION_REASON_REMOTE_HOST_ACTIVE.to_string()),
            })
            .expect("serialize adoption"),
            serde_json::json!({ "adopted": false, "reason": "remote-host-active" })
        );
    }

    #[test]
    fn opened_response_carries_the_adoption_verdict_on_the_wire() {
        // Given the CLI's opened response, when it is serialized, then the adoption verdict is
        // part of the wire shape the CLI prints (not dropped on the floor).
        let response = crate::ipc::browser_cli::BrowserCliResponse::Opened {
            browser: crate::browser::model::BrowserSessionSummary {
                browser_id: "browser-adoption-wire".into(),
                webview_label: "browser-view-adoption-wire".into(),
                workspace_id: Some("ws-adoption".into()),
                profile_id: crate::browser::model::BrowserProfileId::Default,
                url: "https://example.com/".into(),
                title: None,
                visible: true,
            },
            adoption: BrowserAdoption {
                adopted: Some(false),
                reason: Some(ADOPTION_REASON_REMOTE_HOST_ACTIVE.to_string()),
            },
        };

        let json = serde_json::to_value(&response).expect("serialize opened response");

        assert_eq!(json["type"], "opened");
        assert_eq!(json["adoption"]["adopted"], false);
        assert_eq!(json["adoption"]["reason"], "remote-host-active");
        assert_eq!(json["browser"]["browserId"], "browser-adoption-wire");
    }
}
