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

pub fn normalize_account_origin(value: &str) -> Result<String, AccountOriginError> {
    let url = url::Url::parse(value).map_err(|error| AccountOriginError {
        code: "ACCOUNT_ORIGIN_INVALID",
        message: error.to_string(),
    })?;
    if !url.username().is_empty() || url.password().is_some() || url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(AccountOriginError {
            code: "ACCOUNT_ORIGIN_INVALID",
            message: "account origin must not contain credentials, path, query, or fragment".into(),
        });
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Commercial,
    SelfHost,
}

impl DeploymentMode {
    pub const COMMERCIAL_STR: &'static str = "commercial";
    pub const SELFHOST_STR: &'static str = "selfhost";

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Commercial => Self::COMMERCIAL_STR,
            Self::SelfHost => Self::SELFHOST_STR,
        }
    }

    /// Whether this deployment mode enables billing logic and enforcement.
    /// In SelfHost mode, billing is completely disabled and all entitlement checks are skipped.
    pub fn is_billing_enabled(&self) -> bool {
        match self {
            Self::Commercial => true,
            Self::SelfHost => false,
        }
    }

    /// Validates whether the required LemonSqueezy environment variables are configured.
    /// Commercial mode requires all 8 LS keys to be present and non-empty.
    /// SelfHost mode does not require any LS configuration.
    pub fn validate_ls_config(&self) -> Result<(), Vec<&'static str>> {
        self.validate_ls_config_with(|key| std::env::var(key).ok())
    }

    /// Pure lookup-based validation for Lemon Squeezy environment variables.
    pub fn validate_ls_config_with<F>(&self, lookup: F) -> Result<(), Vec<&'static str>>
    where
        F: Fn(&str) -> Option<String>,
    {
        match self {
            Self::SelfHost => Ok(()),
            Self::Commercial => {
                let missing = missing_lemon_squeezy_env_vars_with(lookup);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(missing)
                }
            }
        }
    }
}

impl std::str::FromStr for DeploymentMode {
    type Err = DeploymentModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "commercial" => Ok(Self::Commercial),
            "selfhost" | "self_host" | "self-host" => Ok(Self::SelfHost),
            other => Err(DeploymentModeError {
                code: "DEPLOYMENT_MODE_INVALID",
                message: format!(
                    "invalid FERRYX_DEPLOYMENT_MODE '{other}': must be 'commercial' or 'selfhost'"
                ),
            }),
        }
    }
}

impl std::fmt::Display for DeploymentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentModeError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for DeploymentModeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DeploymentModeError {}

/// Parse `FERRYX_DEPLOYMENT_MODE` from the environment.
///
/// Missing or typo in `FERRYX_DEPLOYMENT_MODE` returns an error explicitly presenting
/// both allowed values ('commercial' or 'selfhost').
pub fn deployment_mode() -> Result<DeploymentMode, DeploymentModeError> {
    parse_deployment_mode_value(std::env::var("FERRYX_DEPLOYMENT_MODE").ok().as_deref())
}

/// Pure parser for deployment mode value, accepting an optional string slice.
pub fn parse_deployment_mode_value(raw: Option<&str>) -> Result<DeploymentMode, DeploymentModeError> {
    match raw {
        Some(value) if !value.trim().is_empty() => value.parse(),
        _ => Err(DeploymentModeError {
            code: "DEPLOYMENT_MODE_UNSET",
            message: "FERRYX_DEPLOYMENT_MODE is not set: must be 'commercial' or 'selfhost'".into(),
        }),
    }
}

/// Parse deployment mode using a lookup closure, e.g. for testing without mutating env vars.
pub fn deployment_mode_with<F>(lookup: F) -> Result<DeploymentMode, DeploymentModeError>
where
    F: FnOnce(&str) -> Option<String>,
{
    parse_deployment_mode_value(lookup("FERRYX_DEPLOYMENT_MODE").as_deref())
}

/// The 8 Lemon Squeezy environment variable keys required for commercial operation,
/// including the team host pack variant.
pub const REQUIRED_LEMON_SQUEEZY_ENV_VARS: [&'static str; 8] = [
    "FERRYX_LS_API_KEY",
    "FERRYX_LS_STORE_ID",
    "FERRYX_LS_WEBHOOK_SECRET",
    "FERRYX_LS_VARIANT_PRO_MONTHLY",
    "FERRYX_LS_VARIANT_PRO_ANNUAL",
    "FERRYX_LS_VARIANT_TEAM_MONTHLY",
    "FERRYX_LS_VARIANT_TEAM_ANNUAL",
    "FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL",
];

/// Returns the missing Lemon Squeezy environment variables as a Vec<&'static str>
/// by checking process environment variables.
pub fn missing_lemon_squeezy_env_vars() -> Vec<&'static str> {
    missing_lemon_squeezy_env_vars_with(|key| std::env::var(key).ok())
}

/// Pure checker for missing Lemon Squeezy environment variables using a lookup closure.
pub fn missing_lemon_squeezy_env_vars_with<F>(lookup: F) -> Vec<&'static str>
where
    F: Fn(&str) -> Option<String>,
{
    REQUIRED_LEMON_SQUEEZY_ENV_VARS
        .iter()
        .copied()
        .filter(|&key| {
            lookup(key)
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
        })
        .collect()
}

/// Check if commercial Lemon Squeezy configuration is complete.
pub fn is_commercial_ls_configured() -> bool {
    missing_lemon_squeezy_env_vars().is_empty()
}

/// Pure checker for complete Lemon Squeezy configuration using a lookup closure.
pub fn is_commercial_ls_configured_with<F>(lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    missing_lemon_squeezy_env_vars_with(lookup).is_empty()
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

    #[test]
    fn explicit_cli_origin_rejects_insecure_and_embedded_credentials() {
        assert_eq!(normalize_account_origin("http://relay.example.com").unwrap_err().code, "ACCOUNT_ORIGIN_INSECURE");
        assert_eq!(normalize_account_origin("https://user:pass@relay.example.com").unwrap_err().code, "ACCOUNT_ORIGIN_INVALID");
        assert_eq!(normalize_account_origin("https://relay.example.com/path").unwrap_err().code, "ACCOUNT_ORIGIN_INVALID");
        assert_eq!(normalize_account_origin("http://127.0.0.1:18787/").unwrap(), "http://127.0.0.1:18787");
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

    #[test]
    fn missing_mode_is_rejected() {
        let err_none = parse_deployment_mode_value(None).expect_err("None mode must error");
        assert_eq!(err_none.code, "DEPLOYMENT_MODE_UNSET");
        assert!(err_none.message.contains("commercial"));
        assert!(err_none.message.contains("selfhost"));

        let err_empty = parse_deployment_mode_value(Some("")).expect_err("empty mode must error");
        assert_eq!(err_empty.code, "DEPLOYMENT_MODE_UNSET");

        let err_whitespace = parse_deployment_mode_value(Some("   ")).expect_err("whitespace mode must error");
        assert_eq!(err_whitespace.code, "DEPLOYMENT_MODE_UNSET");

        let err_typo = parse_deployment_mode_value(Some("unknown_mode")).expect_err("typo mode must error");
        assert_eq!(err_typo.code, "DEPLOYMENT_MODE_INVALID");
        assert!(err_typo.message.contains("commercial"));
        assert!(err_typo.message.contains("selfhost"));

        // Also test deployment_mode_with closure form
        assert_eq!(
            deployment_mode_with(|_| None).unwrap_err().code,
            "DEPLOYMENT_MODE_UNSET"
        );
        assert_eq!(
            deployment_mode_with(|_| Some("commercial".into())).unwrap(),
            DeploymentMode::Commercial
        );
    }

    #[test]
    fn selfhost_mode_disables_billing() {
        let mode = parse_deployment_mode_value(Some("selfhost")).expect("selfhost mode");
        assert_eq!(mode, DeploymentMode::SelfHost);
        assert_eq!(mode.as_str(), "selfhost");
        assert!(!mode.is_billing_enabled());
        assert!(mode.validate_ls_config_with(|_| None).is_ok());

        // Alternate spellings like self_host or self-host
        assert_eq!(
            parse_deployment_mode_value(Some("self_host")).expect("self_host"),
            DeploymentMode::SelfHost
        );
        assert_eq!(
            parse_deployment_mode_value(Some("self-host")).expect("self-host"),
            DeploymentMode::SelfHost
        );
        assert_eq!(
            parse_deployment_mode_value(Some("SELFHOST")).expect("case insensitive"),
            DeploymentMode::SelfHost
        );
    }

    #[test]
    fn commercial_without_ls_config_fails_closed() {
        let mode = parse_deployment_mode_value(Some("commercial")).expect("commercial mode");
        assert_eq!(mode, DeploymentMode::Commercial);
        assert_eq!(mode.as_str(), "commercial");
        assert!(mode.is_billing_enabled());

        // All LS keys missing
        let missing = mode
            .validate_ls_config_with(|_| None)
            .expect_err("all LS config missing");
        assert_eq!(missing.len(), 8);
        assert_eq!(missing, REQUIRED_LEMON_SQUEEZY_ENV_VARS.to_vec());
        assert!(!is_commercial_ls_configured_with(|_| None));

        // 7 of 8 variables present (leave team hostpack missing)
        let partial_lookup = |key: &str| {
            if key == "FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL" {
                None
            } else {
                Some("test_value".to_string())
            }
        };
        let missing_one = mode
            .validate_ls_config_with(partial_lookup)
            .expect_err("one LS key missing");
        assert_eq!(missing_one, vec!["FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL"]);
        assert!(!is_commercial_ls_configured_with(partial_lookup));

        // Empty string treated as missing
        let empty_val_lookup = |key: &str| {
            if key == "FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL" {
                Some("   ".to_string())
            } else {
                Some("test_value".to_string())
            }
        };
        assert_eq!(
            mode.validate_ls_config_with(empty_val_lookup).unwrap_err(),
            vec!["FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL"]
        );

        // All 8 variables present and non-empty
        let full_lookup = |_key: &str| Some("valid_value".to_string());
        assert!(mode.validate_ls_config_with(full_lookup).is_ok());
        assert!(is_commercial_ls_configured_with(full_lookup));
    }
}
