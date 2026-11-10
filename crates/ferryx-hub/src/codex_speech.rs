//! Speech to text through a Codex sign-in the machine already has, so dictation works before the
//! owner buys a speech key. The endpoint is ChatGPT's own transcription service, the same one the
//! Codex desktop app records into; it is authenticated with the OAuth token `codex login` saved,
//! never with an API key.
//!
//! This is a private, undocumented endpoint. It can change without notice, so every failure here
//! is reported as a fallback that did not apply rather than as a broken voice feature: the caller
//! still has the configured key path and the typed composer.

use std::path::PathBuf;

/// The audio field and the account header are what the Codex desktop app sends.
const TRANSCRIBE_URL: &str = "https://chatgpt.com/backend-api/transcribe";

/// What `codex login` leaves on disk, minus the refresh token this never needs to send.
pub struct CodexLogin {
    access_token: String,
    account_id: Option<String>,
}

fn auth_path() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::hub_env::home_dir().map(|home| home.join(".codex")))?;
    Some(home.join("auth.json"))
}

/// The Codex sign-in on this machine, or `None` when there is none or it is an API-key login.
///
/// An API-key login is deliberately not accepted: that mode has no OAuth token, and the endpoint
/// answers 401 for it even though the same account works through the desktop app's own sign-in.
pub fn login() -> Option<CodexLogin> {
    login_at(&auth_path()?)
}

/// The same read against an explicit file, so the parse rules are testable without touching the
/// process environment that every other test shares.
fn login_at(path: &std::path::Path) -> Option<CodexLogin> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    if value.get("auth_mode").and_then(|mode| mode.as_str()) == Some("apikey") {
        return None;
    }
    let tokens = value.get("tokens")?;
    let access_token = tokens.get("access_token").and_then(|token| token.as_str())?.trim();
    if access_token.is_empty() {
        return None;
    }
    let account_id = tokens
        .get("account_id")
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    Some(CodexLogin { access_token: access_token.to_owned(), account_id })
}

/// Whether dictation can run on the Codex sign-in alone, without reading the token out.
pub fn available() -> bool {
    login().is_some()
}

/// Transcribes one WAV recording. Errors are for the caller's log, never for a key prompt.
pub async fn transcribe(wav: &[u8]) -> Result<String, String> {
    let login = login().ok_or("no Codex sign-in on this machine")?;
    let part = reqwest::multipart::Part::bytes(wav.to_vec())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|error| error.to_string())?;
    // A file-only body: the endpoint rejects unknown or duplicate multipart fields.
    let form = reqwest::multipart::Form::new().part("file", part);
    let mut request = reqwest::Client::new()
        .post(TRANSCRIBE_URL)
        .bearer_auth(&login.access_token)
        .multipart(form);
    if let Some(account_id) = login.account_id {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("the Codex sign-in was rejected; run `codex login` again".into());
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("transcription answered {status}: {error}"))?;
    body.get("text")
        .and_then(|text| text.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("transcription answered {status} without text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_file(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("codex-speech-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("auth.json");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn an_api_key_login_is_not_mistaken_for_a_usable_sign_in() {
        let path = auth_file("apikey", r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#);
        assert!(login_at(&path).is_none());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn an_oauth_login_carries_the_token_and_account_without_the_refresh_token() {
        let path = auth_file(
            "oauth",
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"tok","refresh_token":"refresh","account_id":"acc"}}"#,
        );
        let login = login_at(&path).expect("an OAuth login is usable");
        assert_eq!(login.access_token, "tok");
        assert_eq!(login.account_id.as_deref(), Some("acc"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_login_without_a_token_is_not_usable() {
        let path = auth_file("empty", r#"{"auth_mode":"chatgpt","tokens":{}}"#);
        assert!(login_at(&path).is_none());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_missing_file_is_not_usable() {
        assert!(login_at(std::path::Path::new("/nonexistent/codex/auth.json")).is_none());
    }
}
