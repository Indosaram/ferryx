//! Google refuses sign-in inside embedded system webviews ("disallowed_useragent", "This
//! browser or app may not be secure") even with the user-agent spoof in `security.rs`. A tab
//! that lands on that page is offered a handoff: the same sign-in in a Chromium engine tab of
//! the same profile, whose cookies then stay in that profile.

use serde::{Deserialize, Serialize};

const GOOGLE_ACCOUNTS_HOST: &str = "accounts.google.com";
/// Path fragments of Google's rejection pages.
const BLOCK_PATH_MARKERS: &[&str] = &["/signin/rejected", "/v3/signin/rejected"];
/// Query markers Google puts on the OAuth error page for embedded webviews.
const BLOCK_QUERY_MARKERS: &[&str] = &["disallowed_useragent", "disallowed-useragent"];
/// Titles of the rejection page (English UI).
const BLOCK_TITLE_MARKERS: &[&str] = &[
    "this browser or app may not be secure",
    "couldn't sign you in",
    "couldn\u{2019}t sign you in",
];
/// Where a handoff restarts when the blocked page names no `continue` target.
const DEFAULT_SIGN_IN_URL: &str = "https://accounts.google.com/";

/// What the tab's page is, for the handoff bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SignInBlock {
    /// Google blocked sign-in in this webview; `retry_url` restarts it in another engine.
    #[serde(rename_all = "camelCase")]
    GoogleBlocked {
        retry_url: String,
    },
    NotBlocked,
}

/// Classifies a tab's address and title as Google's embedded-webview sign-in block.
pub fn detect_sign_in_block(url: &str, title: Option<&str>) -> SignInBlock {
    let Ok(parsed) = url::Url::parse(url) else {
        return SignInBlock::NotBlocked;
    };
    if parsed.scheme() != "https"
        || !parsed
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(GOOGLE_ACCOUNTS_HOST))
    {
        return SignInBlock::NotBlocked;
    }
    let path = parsed.path().to_ascii_lowercase();
    let query = parsed.query().unwrap_or_default().to_ascii_lowercase();
    let title = title.unwrap_or_default().to_lowercase();
    let blocked = BLOCK_PATH_MARKERS
        .iter()
        .any(|marker| path.starts_with(marker))
        || BLOCK_QUERY_MARKERS
            .iter()
            .any(|marker| query.contains(marker))
        || BLOCK_TITLE_MARKERS
            .iter()
            .any(|marker| title.contains(marker));
    if !blocked {
        return SignInBlock::NotBlocked;
    }
    SignInBlock::GoogleBlocked {
        retry_url: retry_url(&parsed),
    }
}

/// The `continue` target when it is an https URL, else Google's account sign-in page.
fn retry_url(blocked: &url::Url) -> String {
    blocked
        .query_pairs()
        .find(|(key, _)| key == "continue")
        .and_then(|(_, value)| url::Url::parse(&value).ok())
        .filter(|target| target.scheme() == "https")
        .map(|target| target.to_string())
        .unwrap_or_else(|| DEFAULT_SIGN_IN_URL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(url: &str, title: Option<&str>) -> Option<String> {
        match detect_sign_in_block(url, title) {
            SignInBlock::GoogleBlocked { retry_url } => Some(retry_url),
            SignInBlock::NotBlocked => None,
        }
    }

    #[test]
    fn detects_the_rejected_page_and_keeps_its_continue_target() {
        let url = "https://accounts.google.com/v3/signin/rejected?continue=https%3A%2F%2Fmail.google.com%2Fmail%2F&flowName=GlifWebSignIn";
        assert_eq!(
            blocked(url, None).as_deref(),
            Some("https://mail.google.com/mail/")
        );
    }

    #[test]
    fn detects_the_disallowed_useragent_oauth_error() {
        let url = "https://accounts.google.com/signin/oauth/error?authError=Cg9kaXNhbGxvd2VkX3VzZXJhZ2VudA&client_id=x&error=disallowed_useragent";
        assert_eq!(
            blocked(url, None).as_deref(),
            Some("https://accounts.google.com/")
        );
    }

    #[test]
    fn detects_the_block_by_title() {
        let url =
            "https://accounts.google.com/v3/signin/identifier?continue=https://drive.google.com/";
        assert_eq!(
            blocked(
                url,
                Some("Couldn\u{2019}t sign you in - This browser or app may not be secure")
            )
            .as_deref(),
            Some("https://drive.google.com/")
        );
    }

    #[test]
    fn ignores_normal_sign_in_other_hosts_and_other_oauth_errors() {
        assert_eq!(
            blocked(
                "https://accounts.google.com/v3/signin/identifier",
                Some("Sign in - Google Accounts")
            ),
            None
        );
        assert_eq!(
            blocked(
                "https://evil.example/signin/rejected?error=disallowed_useragent",
                None
            ),
            None
        );
        assert_eq!(
            blocked("http://accounts.google.com/signin/rejected", None),
            None
        );
        assert_eq!(
            blocked(
                "https://accounts.google.com/signin/oauth/error?error=invalid_client",
                None
            ),
            None
        );
        assert_eq!(blocked("not a url", None), None);
    }

    #[test]
    fn a_non_https_continue_target_falls_back_to_the_sign_in_page() {
        let url = "https://accounts.google.com/signin/rejected?continue=javascript:alert(1)";
        assert_eq!(
            blocked(url, None).as_deref(),
            Some("https://accounts.google.com/")
        );
    }

    #[test]
    fn serializes_for_the_handoff_bar() {
        assert_eq!(
            serde_json::to_value(SignInBlock::GoogleBlocked {
                retry_url: "https://a/".into()
            })
            .unwrap(),
            serde_json::json!({ "kind": "googleBlocked", "retryUrl": "https://a/" })
        );
    }
}
