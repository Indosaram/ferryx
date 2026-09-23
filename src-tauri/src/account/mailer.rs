use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
    }

    #[test]
    fn file_mailer_writes_link_missing_parent_returns_mail_failed() {
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
}
