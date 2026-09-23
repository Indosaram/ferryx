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
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx").join("account"))
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
