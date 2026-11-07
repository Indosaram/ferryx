//! Engine-neutral autofill: the capture envelope the isolated-world script posts, the save
//! prompts it turns into, and the fill script. Both engines feed the same functions: the system
//! webview through the trusted bridge, the Chromium engine through `Event::FormSubmitted`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::origin::{credential_origin_matches, origin_of_page, CredentialOrigin};
use super::store::{SaveKind, VaultCredential, VaultError, VaultItemSummary, VaultStore};

/// Envelope kind the capture script posts.
pub const FORM_SUBMITTED_KIND: &str = "formSubmitted";
/// Tauri event carrying a [`VaultSavePrompt`].
pub const BROWSER_VAULT_SAVE_PROMPT_EVENT: &str = "browser_vault_save_prompt";
const MAX_ENVELOPE_BYTES: usize = 8 * 1024;
const PROMPT_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PENDING: usize = 16;

/// What the capture script posted. Every field is re-validated here.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaptureEnvelope {
    v: u32,
    kind: String,
    origin: String,
    #[serde(default)]
    username: String,
    password: String,
}

/// A submitted login, after validation.
pub struct SubmittedLogin {
    pub origin: CredentialOrigin,
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

/// Whether `raw` is a capture envelope at all (cheap pre-check before the full parse).
pub fn is_capture_envelope(raw: &str) -> bool {
    raw.len() <= MAX_ENVELOPE_BYTES && raw.contains("\"formSubmitted\"")
}

/// Parses a capture envelope and binds it to the tab: the origin the script saw must be the
/// origin of the page the host believes the tab shows. Anything else is dropped.
pub fn parse_submission(raw: &str, page_url: &str) -> Option<SubmittedLogin> {
    if raw.len() > MAX_ENVELOPE_BYTES {
        return None;
    }
    let envelope: CaptureEnvelope = serde_json::from_str(raw).ok()?;
    if envelope.v != 1 || envelope.kind != FORM_SUBMITTED_KIND || envelope.password.is_empty() {
        return None;
    }
    let page = origin_of_page(page_url).ok()?;
    let claimed = origin_of_page(&envelope.origin).ok()?;
    if page != claimed {
        return None;
    }
    Some(SubmittedLogin {
        origin: page,
        username: Zeroizing::new(envelope.username.trim().to_string()),
        password: Zeroizing::new(envelope.password),
    })
}

/// Shown to the user: never the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSavePrompt {
    pub browser_id: String,
    pub prompt_id: String,
    pub origin: String,
    pub username: String,
    pub update: bool,
}

struct Pending {
    browser_id: String,
    login: SubmittedLogin,
    created: Instant,
}

/// Submitted logins waiting for the user's Save / Not now.
#[derive(Default)]
pub struct PendingPrompts {
    entries: HashMap<String, Pending>,
}

impl PendingPrompts {
    /// Turns a submission into a prompt, or `None` when the vault already has exactly this login.
    pub fn offer(
        &mut self,
        store: &VaultStore,
        browser_id: &str,
        login: SubmittedLogin,
        now: Instant,
    ) -> Result<Option<VaultSavePrompt>, VaultError> {
        let update = match store.classify(&login.origin, &login.username, &login.password)? {
            SaveKind::Unchanged => return Ok(None),
            SaveKind::New => false,
            SaveKind::Update { .. } => true,
        };
        self.entries.retain(|_, pending| now.duration_since(pending.created) < PROMPT_TTL);
        // One open prompt per tab: a newer submission replaces the older one.
        self.entries.retain(|_, pending| pending.browser_id != browser_id);
        while self.entries.len() >= MAX_PENDING {
            let Some(oldest) = self.entries.iter().min_by_key(|(_, pending)| pending.created).map(|(id, _)| id.clone()) else {
                break;
            };
            self.entries.remove(&oldest);
        }
        let prompt = VaultSavePrompt {
            browser_id: browser_id.to_string(),
            prompt_id: uuid::Uuid::new_v4().to_string(),
            origin: login.origin.as_str().to_string(),
            username: login.username.to_string(),
            update,
        };
        self.entries.insert(
            prompt.prompt_id.clone(),
            Pending { browser_id: browser_id.to_string(), login, created: now },
        );
        Ok(Some(prompt))
    }

    /// Saves a prompt's login. A prompt is used once; an unknown or expired id is an error.
    pub fn accept(&mut self, store: &mut VaultStore, prompt_id: &str, now: Instant) -> Result<VaultItemSummary, VaultError> {
        let pending = self.take(prompt_id, now)?;
        store.save(pending.login.origin.as_str(), &pending.login.username, &pending.login.password)
    }

    pub fn dismiss(&mut self, prompt_id: &str, now: Instant) -> Result<(), VaultError> {
        self.take(prompt_id, now).map(|_| ())
    }

    fn take(&mut self, prompt_id: &str, now: Instant) -> Result<Pending, VaultError> {
        match self.entries.remove(prompt_id) {
            Some(pending) if now.duration_since(pending.created) < PROMPT_TTL => Ok(pending),
            _ => Err(VaultError::PromptExpired),
        }
    }

    /// Drops a closed tab's prompt.
    pub fn forget_browser(&mut self, browser_id: &str) {
        self.entries.retain(|_, pending| pending.browser_id != browser_id);
    }
}

const FILL_PLACEHOLDER: &str = "__FERRYX_FILL__";

/// Fills a login form in the top frame of the page it runs in (an isolated world): nothing
/// unless the frame is the top frame and its origin is exactly `origin`, and only fields the user
/// can see (`fill.js`).
const FILL_SCRIPT: &str = include_str!("fill.js");

/// The fill script for one login, with its values embedded as escaped JSON.
pub fn fill_script(origin: &CredentialOrigin, username: &str, password: &str) -> Zeroizing<String> {
    let value = serde_json::json!({ "origin": origin.as_str(), "username": username, "password": password });
    let literal = Zeroizing::new(crate::browser::popup::embed_value(&value));
    Zeroizing::new(FILL_SCRIPT.replace(FILL_PLACEHOLDER, &literal))
}

/// The fill script for a saved credential on the tab showing `page_url`, or an error when the
/// page is not exactly the credential's origin (another scheme, host or port). An https login
/// is never sent to an http page. The page re-checks its own origin before it fills.
pub fn fill_for_page(credential: &VaultCredential, page_url: &str) -> Result<Zeroizing<String>, VaultError> {
    if !credential_origin_matches(credential.origin.as_str(), page_url) {
        return Err(VaultError::InvalidArgument("this login is for another site".into()));
    }
    Ok(fill_script(&credential.origin, &credential.username, &credential.password))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::vault::keystore::MemoryKeyStore;

    fn envelope(origin: &str, username: &str, password: &str) -> String {
        serde_json::json!({ "v": 1, "kind": "formSubmitted", "origin": origin, "username": username, "password": password }).to_string()
    }

    #[test]
    fn a_submission_is_bound_to_the_tabs_exact_origin() {
        let raw = envelope("https://example.com", " alice ", "pw");
        let login = parse_submission(&raw, "https://example.com/login?next=1").unwrap();
        assert_eq!(login.origin.as_str(), "https://example.com");
        assert_eq!(login.username.as_str(), "alice");
        assert!(parse_submission(&raw, "http://example.com/login").is_none());
        assert!(parse_submission(&raw, "https://sub.example.com/").is_none());
        assert!(parse_submission(&envelope("https://evil.test", "a", "pw"), "https://example.com/").is_none());
        assert!(parse_submission(&envelope("https://example.com", "a", ""), "https://example.com/").is_none());
        assert!(parse_submission(r#"{"v":2,"kind":"formSubmitted","origin":"https://example.com","password":"x"}"#, "https://example.com/").is_none());
        assert!(parse_submission(r#"{"v":1,"kind":"formSubmitted","origin":"https://example.com","password":"x","extra":1}"#, "https://example.com/").is_none());
        assert!(parse_submission(&"x".repeat(MAX_ENVELOPE_BYTES + 1), "https://example.com/").is_none());
        assert!(is_capture_envelope(&raw));
        assert!(!is_capture_envelope(r#"{"v":1,"kind":"link","url":"https://a.test/"}"#));
    }

    #[test]
    fn prompts_offer_new_and_update_skip_unchanged_and_are_single_use() {
        let dir = tempfile::tempdir().unwrap();
        let keystore = MemoryKeyStore::default();
        let mut store = VaultStore::open(dir.path(), &keystore).unwrap();
        let mut prompts = PendingPrompts::default();
        let now = Instant::now();
        let submit = |password: &str| parse_submission(&envelope("https://example.com", "alice", password), "https://example.com/").unwrap();

        let prompt = prompts.offer(&store, "b1", submit("pw-1"), now).unwrap().unwrap();
        assert!(!prompt.update);
        assert_eq!(prompt.username, "alice");
        let saved = prompts.accept(&mut store, &prompt.prompt_id, now).unwrap();
        assert!(matches!(prompts.accept(&mut store, &prompt.prompt_id, now), Err(VaultError::PromptExpired)));

        assert!(prompts.offer(&store, "b1", submit("pw-1"), now).unwrap().is_none());
        let update = prompts.offer(&store, "b1", submit("pw-2"), now).unwrap().unwrap();
        assert!(update.update);
        assert_eq!(prompts.accept(&mut store, &update.prompt_id, now).unwrap().id, saved.id);

        let dismissed = prompts.offer(&store, "b1", submit("pw-3"), now).unwrap().unwrap();
        prompts.dismiss(&dismissed.prompt_id, now).unwrap();
        assert_eq!(store.credential(&saved.id).unwrap().password.as_str(), "pw-2");
    }

    #[test]
    fn a_newer_prompt_replaces_the_tabs_older_one_and_old_prompts_expire() {
        let dir = tempfile::tempdir().unwrap();
        let keystore = MemoryKeyStore::default();
        let mut store = VaultStore::open(dir.path(), &keystore).unwrap();
        let mut prompts = PendingPrompts::default();
        let now = Instant::now();
        let submit = |user: &str| parse_submission(&envelope("https://example.com", user, "pw"), "https://example.com/").unwrap();
        let first = prompts.offer(&store, "b1", submit("a"), now).unwrap().unwrap();
        let second = prompts.offer(&store, "b1", submit("b"), now).unwrap().unwrap();
        assert!(matches!(prompts.dismiss(&first.prompt_id, now), Err(VaultError::PromptExpired)));
        assert!(matches!(
            prompts.accept(&mut store, &second.prompt_id, now + PROMPT_TTL),
            Err(VaultError::PromptExpired)
        ));
        let third = prompts.offer(&store, "b2", submit("c"), now).unwrap().unwrap();
        prompts.forget_browser("b2");
        assert!(matches!(prompts.dismiss(&third.prompt_id, now), Err(VaultError::PromptExpired)));
    }

    #[test]
    fn the_fill_script_embeds_escaped_values_and_checks_top_frame_and_origin() {
        let origin = origin_of_page("https://example.com/").unwrap();
        let script = fill_script(&origin, "a\"b", "</script><img onerror=x>");
        assert!(!script.contains(FILL_PLACEHOLDER));
        assert!(!script.contains("</script>"));
        assert!(script.contains(r#""origin":"https://example.com""#));
        assert!(script.contains("window.top === window"));
        assert!(script.contains("location.origin !== fill.origin"));
    }

    fn credential(origin: &str) -> VaultCredential {
        VaultCredential {
            origin: super::super::origin::canonicalize_origin(origin).unwrap(),
            username: Zeroizing::new("ada".into()),
            password: Zeroizing::new("pw-7f3c".into()),
        }
    }

    #[test]
    fn a_login_fills_only_on_its_exact_origin() {
        let saved = credential("https://example.com");
        let script = fill_for_page(&saved, "https://example.com/login?next=/").unwrap();
        assert!(script.contains(r#""origin":"https://example.com""#));
        assert!(script.contains("pw-7f3c"));
        for page in [
            "http://example.com/login",
            "https://example.com:8443/login",
            "https://login.example.com/",
            "https://example.com.evil.test/",
            "https://evil.test/?u=https://example.com",
            "about:blank",
            "data:text/html,<form><input type=password></form>",
            "file:///tmp/login.html",
        ] {
            assert!(
                matches!(fill_for_page(&saved, page), Err(VaultError::InvalidArgument(_))),
                "{page} must not receive the https://example.com login"
            );
        }
        let insecure = credential("http://intranet.test");
        assert!(fill_for_page(&insecure, "http://intranet.test/").is_ok());
        assert!(fill_for_page(&insecure, "https://intranet.test/").is_err());
    }

    #[test]
    fn the_fill_script_survives_the_engine_line_protocol() {
        // The Chromium engine joins a command onto one line, so a `//` comment would swallow
        // the rest of the script there.
        assert!(!FILL_SCRIPT.lines().any(|line| line.trim_start().starts_with("//")));
        let origin = origin_of_page("https://example.com/").unwrap();
        let script = fill_script(&origin, "ada", "line\nbreak");
        let fill = ferryx_browser_engine_protocol::Command::Fill { tab: "b1".into(), script: script.to_string() };
        let ferryx_browser_engine_protocol::Command::Fill { script: sent, .. } =
            ferryx_browser_engine_protocol::Command::parse(&fill.to_line()).unwrap()
        else {
            panic!("a fill line parses back as a fill");
        };
        assert_eq!(sent.matches("__FERRYX_FILL__").count(), 0);
        assert!(sent.trim_end().ends_with("})()"));
        assert!(sent.contains(r#""password":"line\nbreak""#));
    }
}
