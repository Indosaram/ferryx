use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MailerError {
    #[error("MAIL_FAILED: {0}")]
    MailFailed(String),
}

impl MailerError {
    pub const CODE: &'static str = "MAIL_FAILED";

    pub fn code(&self) -> &'static str {
        match self {
            MailerError::MailFailed(_) => Self::CODE,
        }
    }

    pub fn mail_failed(message: impl Into<String>) -> Self {
        Self::MailFailed(message.into())
    }
}

impl From<std::io::Error> for MailerError {
    fn from(err: std::io::Error) -> Self {
        Self::mail_failed(err.to_string())
    }
}

pub trait Mailer: Send + Sync {
    fn send_magic_link(&self, to: &str, url: &str) -> Result<(), MailerError>;
}

#[derive(Debug, Clone, Default)]
pub struct FileMailer {
    mail_dir: Option<PathBuf>,
}

impl FileMailer {
    pub fn new() -> Self {
        Self { mail_dir: None }
    }

    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            mail_dir: Some(dir.into()),
        }
    }

    fn resolve_dir(&self) -> Result<PathBuf, MailerError> {
        if let Some(dir) = &self.mail_dir {
            return Ok(dir.clone());
        }
        match std::env::var("FERRYX_MAIL_DIR") {
            Ok(val) if !val.trim().is_empty() => Ok(PathBuf::from(val.trim())),
            _ => Err(MailerError::mail_failed("FERRYX_MAIL_DIR is not set or empty")),
        }
    }
}

impl Mailer for FileMailer {
    fn send_magic_link(&self, _to: &str, url: &str) -> Result<(), MailerError> {
        let dir = self.resolve_dir()?;

        if !dir.exists() {
            if let Some(parent) = dir.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    return Err(MailerError::mail_failed(format!(
                        "Parent directory does not exist: {}",
                        parent.display()
                    )));
                }
            }
            std::fs::create_dir_all(&dir)
                .map_err(|e| MailerError::mail_failed(format!("Failed to create mail directory: {e}")))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            }
        }

        if !dir.is_dir() {
            return Err(MailerError::mail_failed(format!(
                "Path is not a directory: {}",
                dir.display()
            )));
        }

        let file_name = format!(
            "magic-link-{}-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            uuid::Uuid::new_v4()
        );
        let file_path = dir.join(file_name);

        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        use std::io::Write as _;
        let mut file = options
            .open(&file_path)
            .map_err(|e| MailerError::mail_failed(format!("Failed to create file: {e}")))?;

        file.write_all(url.as_bytes())
            .map_err(|e| MailerError::mail_failed(format!("Failed to write content: {e}")))?;

        file.flush()
            .map_err(|e| MailerError::mail_failed(format!("Failed to flush file: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| MailerError::mail_failed(format!("Failed to set permissions: {e}")))?;
        }

        Ok(())
    }
}

/// Runs the async HTTP send future from the synchronous `Mailer` trait method.
/// Uses the active Tokio runtime when present (`block_in_place`), otherwise
/// builds a minimal current-thread runtime for the single request.
fn block_on_send(
    fut: impl std::future::Future<Output = Result<(), MailerError>>,
) -> Result<(), MailerError> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| MailerError::mail_failed(e.to_string()))?
            .block_on(fut)
    }
}

fn resend_payload(from_address: &str, to: &str, url: &str) -> serde_json::Value {
    serde_json::json!({
        "from": from_address,
        "to": [to],
        "subject": "Sign in to Ferryx",
        "html": format!("<div style=\"font-family:sans-serif;max-width:500px;margin:0 auto;padding:20px;\"><h2>Sign in to Ferryx</h2><p>Click the button below to authorize this machine:</p><p><a href=\"{url}\" style=\"display:inline-block;padding:12px 24px;background:#10b981;color:#ffffff;text-decoration:none;border-radius:6px;font-weight:bold;\">Authorize Machine</a></p><p style=\"color:#666;font-size:12px;\">Or copy and paste this link into your browser:<br/><a href=\"{url}\">{url}</a></p></div>")
    })
}

fn webhook_payload(to: &str, url: &str) -> serde_json::Value {
    serde_json::json!({ "to": to, "url": url })
}

#[derive(Debug, Clone)]
pub struct ResendMailer {
    api_key: String,
    from_address: String,
    client: reqwest::Client,
}

impl ResendMailer {
    pub fn new(api_key: impl Into<String>, from_address: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            from_address: from_address.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl Mailer for ResendMailer {
    fn send_magic_link(&self, to: &str, url: &str) -> Result<(), MailerError> {
        let payload = resend_payload(&self.from_address, to, url);
        let fut = async {
            let resp = self
                .client
                .post("https://api.resend.com/emails")
                .bearer_auth(&self.api_key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| MailerError::mail_failed(e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(MailerError::mail_failed(format!(
                    "Resend API error {status}: {body}"
                )));
            }
            Ok(())
        };
        block_on_send(fut)
    }
}

#[derive(Debug, Clone)]
pub struct WebhookMailer {
    url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl WebhookMailer {
    pub fn new(url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            url: url.into(),
            token: token.filter(|t| !t.trim().is_empty()),
            client: reqwest::Client::new(),
        }
    }
}

impl Mailer for WebhookMailer {
    fn send_magic_link(&self, to: &str, url: &str) -> Result<(), MailerError> {
        let payload = webhook_payload(to, url);
        let fut = async {
            let mut req = self.client.post(&self.url).json(&payload);
            if let Some(token) = &self.token {
                req = req.bearer_auth(token);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| MailerError::mail_failed(e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(MailerError::mail_failed(format!(
                    "Webhook API error {status}: {body}"
                )));
            }
            Ok(())
        };
        block_on_send(fut)
    }
}

/// Picks the mail transport from the environment:
/// `RESEND_API_KEY` -> Resend, else `FERRYX_MAIL_WEBHOOK_URL` -> webhook,
/// else a `FileMailer` rooted at `fallback_dir` (default `mail/`).
pub fn create_production_mailer(fallback_dir: Option<PathBuf>) -> Arc<dyn Mailer> {
    if let Ok(key) = std::env::var("RESEND_API_KEY") {
        if !key.is_empty() {
            let from_address = std::env::var("FERRYX_MAIL_FROM")
                .unwrap_or_else(|_| "Ferryx <login@checka.cc>".to_string());
            return Arc::new(ResendMailer::new(key, from_address));
        }
    }
    if let Ok(webhook) = std::env::var("FERRYX_MAIL_WEBHOOK_URL") {
        if !webhook.is_empty() {
            let token = std::env::var("FERRYX_MAIL_WEBHOOK_TOKEN").ok();
            return Arc::new(WebhookMailer::new(webhook, token));
        }
    }
    Arc::new(FileMailer::with_dir(
        fallback_dir.unwrap_or_else(|| PathBuf::from("mail")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};

    static MAIL_DIR_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_mail_dir_env() -> MutexGuard<'static, ()> {
        MAIL_DIR_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn file_mailer_writes_link() {
        let temp_dir = std::env::temp_dir().join(format!("ferryx-test-mail-{}", uuid::Uuid::new_v4()));
        let mailer = FileMailer::with_dir(&temp_dir);

        let test_url = "https://ferryx.dev/auth/magic?token=deadbeef12345678";
        let res = mailer.send_magic_link("alice@example.com", test_url);
        assert!(res.is_ok(), "send_magic_link should succeed");

        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .collect();

        assert_eq!(entries.len(), 1, "Exactly one file should be created");
        let file_path = entries[0].path();
        let content = std::fs::read_to_string(&file_path).expect("read file");
        assert_eq!(content, test_url, "One file contains the exact URL");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&file_path).expect("metadata");
            assert_eq!(
                meta.permissions().mode() & 0o777,
                0o600,
                "File permissions must be 0600"
            );
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn file_mailer_writes_link_env_dir() {
        let _guard = lock_mail_dir_env();
        let temp_dir = std::env::temp_dir().join(format!("ferryx-test-mail-env-{}", uuid::Uuid::new_v4()));
        std::env::set_var("FERRYX_MAIL_DIR", &temp_dir);

        let mailer = FileMailer::new();
        let test_url = "https://ferryx.dev/auth/magic?token=envtest987654";
        let res = mailer.send_magic_link("bob@example.com", test_url);
        assert!(res.is_ok(), "send_magic_link with env var should succeed");

        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .collect();

        assert_eq!(entries.len(), 1, "Exactly one file should be created");
        let content = std::fs::read_to_string(entries[0].path()).expect("read file");
        assert_eq!(content, test_url);

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::env::remove_var("FERRYX_MAIL_DIR");
    }

    #[test]
    fn file_mailer_writes_link_missing_parent_returns_mail_failed() {
        let _guard = lock_mail_dir_env();
        let missing_parent = std::env::temp_dir()
            .join(format!("nonexistent-parent-{}", uuid::Uuid::new_v4()))
            .join("sub")
            .join("mail");
        std::env::set_var("FERRYX_MAIL_DIR", &missing_parent);

        let mailer = FileMailer::new();
        let res = mailer.send_magic_link("charlie@example.com", "https://ferryx.dev/auth/magic?token=fail");
        assert!(res.is_err(), "Must fail when parent directory is missing");
        let err = res.unwrap_err();
        assert_eq!(err.code(), "MAIL_FAILED", "Error code must be MAIL_FAILED");
        std::env::remove_var("FERRYX_MAIL_DIR");
    }

    #[test]
    fn file_mailer_writes_link_failing_double_creates_no_session() {
        struct FailingMailerDouble;
        impl Mailer for FailingMailerDouble {
            fn send_magic_link(&self, _to: &str, _url: &str) -> Result<(), MailerError> {
                Err(MailerError::mail_failed("Simulated mail transport failure"))
            }
        }

        struct LoginRequestService<M: Mailer> {
            mailer: M,
            sessions_created: AtomicUsize,
        }

        impl<M: Mailer> LoginRequestService<M> {
            fn request_login(&self, email: &str, url: &str) -> Result<(), MailerError> {
                self.mailer.send_magic_link(email, url)?;
                self.sessions_created.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let service = LoginRequestService {
            mailer: FailingMailerDouble,
            sessions_created: AtomicUsize::new(0),
        };

        let result = service.request_login("dave@example.com", "https://ferryx.dev/auth/magic?token=xyz");
        assert!(result.is_err(), "Service request_login should fail on failing mailer");
        let err = result.unwrap_err();
        assert_eq!(err.code(), "MAIL_FAILED", "Error code must be MAIL_FAILED");
        assert_eq!(
            service.sessions_created.load(Ordering::SeqCst),
            0,
            "A failing mailer double returns MAIL_FAILED and creates no session"
        );
    }

    #[test]
    fn resend_payload_generation() {
        let url = "https://relay.checka.cc/auth/magic?token=deadbeef12345678";
        let payload = resend_payload("Ferryx <login@checka.cc>", "alice@example.com", url);

        assert_eq!(payload["from"], "Ferryx <login@checka.cc>");
        assert_eq!(payload["to"], serde_json::json!(["alice@example.com"]));
        assert_eq!(payload["subject"], "Sign in to Ferryx");

        let html = payload["html"].as_str().expect("html must be a string");
        assert!(html.contains(url), "html must embed the magic link URL");
        assert!(
            html.contains("Authorize Machine"),
            "html must contain the authorize button label"
        );
        assert!(
            html.contains("background:#10b981"),
            "html must contain the button styling"
        );
    }

    #[test]
    fn webhook_payload_generation() {
        let payload = webhook_payload("bob@example.com", "https://relay.checka.cc/auth/magic?token=xyz");
        assert_eq!(payload["to"], "bob@example.com");
        assert_eq!(payload["url"], "https://relay.checka.cc/auth/magic?token=xyz");
    }

    #[test]
    fn create_production_mailer_falls_back_to_file_mailer() {
        const KEYS: [&str; 3] = [
            "RESEND_API_KEY",
            "FERRYX_MAIL_WEBHOOK_URL",
            "FERRYX_MAIL_WEBHOOK_TOKEN",
        ];
        let saved: Vec<(&'static str, Option<String>)> =
            KEYS.iter().map(|&k| (k, std::env::var(k).ok())).collect();
        for (k, v) in &saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }

        let temp_dir =
            std::env::temp_dir().join(format!("ferryx-test-prod-mail-{}", uuid::Uuid::new_v4()));
        let mailer = create_production_mailer(Some(temp_dir.clone()));
        let url = "https://ferryx.dev/auth/magic?token=prodtest987654";
        let res = mailer.send_magic_link("erin@example.com", url);
        assert!(
            res.is_ok(),
            "fallback FileMailer should write the link: {res:?}"
        );

        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "Exactly one file should be created");
        let content = std::fs::read_to_string(entries[0].path()).expect("read file");
        assert_eq!(content, url);

        let _ = std::fs::remove_dir_all(&temp_dir);
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}
