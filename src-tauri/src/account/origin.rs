use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountOriginError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for AccountOriginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

pub fn account_origin() -> Result<String, AccountOriginError> {
    if let Some(value) = std::env::var("FERRYX_ACCOUNT_ORIGIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return normalize_account_origin(value.trim());
    }
    if let Some(path) = account_data_dir() {
        let file = path.join("account-origin");
        if let Ok(text) = std::fs::read_to_string(&file) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return normalize_account_origin(trimmed);
            }
        }
    }
    Err(AccountOriginError {
        code: "ACCOUNT_ORIGIN_UNSET",
        message: "set FERRYX_ACCOUNT_ORIGIN".into(),
    })
}

pub fn account_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FERRYX_ACCOUNT_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    resolve_account_data_dir(std::env::var_os("HOME"), std::env::var_os("USERPROFILE"))
}

/// Home-based resolution behind [`account_data_dir`], split out so the platform
/// fallback can be asserted without mutating process-wide environment state.
///
/// Windows does not define `HOME`, so `USERPROFILE` has to stand in for it; without
/// that fallback the resolver returns `None`, `account_origin()` reports
/// `ACCOUNT_ORIGIN_UNSET`, and `ferryx-account` exits with
/// `ACCOUNT_DATA_DIR_UNRESOLVED`.
fn resolve_account_data_dir(
    home: Option<std::ffi::OsString>,
    userprofile: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    home.or(userprofile)
        .map(|base| PathBuf::from(base).join(".ferryx").join("account"))
}

fn normalize_account_origin(value: &str) -> Result<String, AccountOriginError> {
    let url = url::Url::parse(value).map_err(|error| AccountOriginError {
        code: "ACCOUNT_ORIGIN_INVALID",
        message: error.to_string(),
    })?;
    let host = url.host_str().unwrap_or("");
    let loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(AccountOriginError {
            code: "ACCOUNT_ORIGIN_INSECURE",
            message: "account origin must be https, or http on loopback".into(),
        });
    }
    let allow_dev = std::env::var("FERRYX_ACCOUNT_ORIGIN_ALLOW_FERRYX_DEV")
        .ok()
        .is_some_and(|value| value == "1");
    if host.eq_ignore_ascii_case("ferryx.dev") && !allow_dev {
        return Err(AccountOriginError {
            code: "ACCOUNT_ORIGIN_CUTOVER_FORBIDDEN",
            message: "ferryx.dev is not configured by this build".into(),
        });
    }
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Ok(origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_origin_unset_fails_closed() {
        let _guard = EnvGuard::new();
        std::env::remove_var("FERRYX_ACCOUNT_ORIGIN");
        std::env::remove_var("FERRYX_ACCOUNT_DATA_DIR");
        std::env::remove_var("FERRYX_ACCOUNT_ORIGIN_ALLOW_FERRYX_DEV");
        let missing = account_origin().expect_err("unset");
        assert_eq!(missing.code, "ACCOUNT_ORIGIN_UNSET");

        std::env::set_var("FERRYX_ACCOUNT_ORIGIN", "https://relay.checka.cc");
        assert_eq!(
            account_origin().expect("explicit relay").as_str(),
            "https://relay.checka.cc"
        );
        std::env::set_var("FERRYX_ACCOUNT_ORIGIN", "https://ferryx.dev");
        assert_eq!(
            account_origin().expect_err("cutover").code,
            "ACCOUNT_ORIGIN_CUTOVER_FORBIDDEN"
        );
        std::env::set_var("FERRYX_ACCOUNT_ORIGIN_ALLOW_FERRYX_DEV", "1");
        assert_eq!(
            account_origin().expect("allowed later").as_str(),
            "https://ferryx.dev"
        );
        std::env::set_var("FERRYX_ACCOUNT_ORIGIN", "http://127.0.0.1:9");
        assert_eq!(
            account_origin().expect("loopback").as_str(),
            "http://127.0.0.1:9",
            "a non-default port must survive normalization, or peers sign a different origin"
        );
    }

    #[test]
    fn account_data_dir_falls_back_to_userprofile_without_home() {
        let userprofile = if cfg!(windows) {
            r"C:\Users\ferryx"
        } else {
            "/home/ferryx"
        };
        let resolved = resolve_account_data_dir(None, Some(userprofile.into()))
            .expect("USERPROFILE stands in for the HOME a Windows host does not define");
        assert!(
            resolved.is_absolute(),
            "the account data dir must never resolve relative to the working directory"
        );
        assert!(
            resolved.ends_with(PathBuf::from(".ferryx").join("account")),
            "unexpected shape: {}",
            resolved.display()
        );
        assert_eq!(
            resolved,
            PathBuf::from(userprofile).join(".ferryx").join("account")
        );
        assert_eq!(
            resolve_account_data_dir(Some("/home/ferryx".into()), Some(userprofile.into())),
            Some(PathBuf::from("/home/ferryx").join(".ferryx").join("account")),
            "HOME must keep winning so existing platforms resolve the same directory"
        );
        assert_eq!(resolve_account_data_dir(None, None), None);
    }

    struct EnvGuard;

    impl EnvGuard {
        fn new() -> Self {
            Self
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var("FERRYX_ACCOUNT_ORIGIN");
            std::env::remove_var("FERRYX_ACCOUNT_ORIGIN_ALLOW_FERRYX_DEV");
        }
    }
}
