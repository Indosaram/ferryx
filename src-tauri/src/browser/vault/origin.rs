//! Credential-origin canonicalization and matching.
//!
//! Ported from Maho (Project-Maho/maho-browser@c2830251,
//! maho/crates/maho-core/src/vault_manager/origin.rs). Ferryx keeps only Maho's `Exact` policy:
//! a credential fills on exactly its scheme, host and port, so an https login never fills on
//! http, on a subdomain or on another port. Userinfo, query, fragment and paths are rejected for
//! a stored origin, never silently stripped.

/// A canonical `scheme://host[:port]` for http(s), with the default port removed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CredentialOrigin(String);

impl CredentialOrigin {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<CredentialOrigin> for String {
    fn from(origin: CredentialOrigin) -> Self {
        origin.0
    }
}

impl TryFrom<String> for CredentialOrigin {
    type Error = InvalidOrigin;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        canonicalize_origin(&value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not an http(s) origin: {0}")]
pub struct InvalidOrigin(pub String);

fn from_url(url: &url::Url, raw: &str) -> Result<CredentialOrigin, InvalidOrigin> {
    let invalid = || InvalidOrigin(raw.to_string());
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(invalid());
    }
    let host = url.host_str().filter(|host| !host.is_empty()).ok_or_else(invalid)?;
    Ok(CredentialOrigin(match url.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    }))
}

/// Canonicalizes a stored origin. Anything beyond an origin is an error.
pub fn canonicalize_origin(raw: &str) -> Result<CredentialOrigin, InvalidOrigin> {
    let invalid = || InvalidOrigin(raw.to_string());
    let url = url::Url::parse(raw).map_err(|_| invalid())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    if url.query().is_some() || url.fragment().is_some() || url.path() != "/" {
        return Err(invalid());
    }
    from_url(&url, raw)
}

/// The origin of a page URL (the tab's current address), for matching and saving.
pub fn origin_of_page(raw: &str) -> Result<CredentialOrigin, InvalidOrigin> {
    let url = url::Url::parse(raw).map_err(|_| InvalidOrigin(raw.to_string()))?;
    from_url(&url, raw)
}

/// True only when both canonicalize and are the same scheme, host and port.
pub fn credential_origin_matches(stored: &str, requested: &str) -> bool {
    match (canonicalize_origin(stored), origin_of_page(requested)) {
        (Ok(stored), Ok(requested)) => stored == requested,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalization_lowercases_hosts_punycodes_and_drops_default_ports() {
        assert_eq!(canonicalize_origin("HTTPS://Example.COM:443").unwrap().as_str(), "https://example.com");
        assert_eq!(canonicalize_origin("http://example.com:80/").unwrap().as_str(), "http://example.com");
        assert_eq!(canonicalize_origin("https://example.com:8443").unwrap().as_str(), "https://example.com:8443");
        assert_eq!(canonicalize_origin("https://bücher.de").unwrap().as_str(), "https://xn--bcher-kva.de");
    }

    #[test]
    fn stored_origins_with_extra_parts_or_other_schemes_are_rejected() {
        for raw in [
            "https://user:pw@example.com",
            "https://example.com/login",
            "https://example.com/?q=1",
            "https://example.com/#x",
            "ftp://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "about:blank",
            "not a url",
        ] {
            assert!(canonicalize_origin(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn page_origins_keep_only_scheme_host_and_port() {
        assert_eq!(
            origin_of_page("https://Accounts.Example.com:443/signin?next=/#top").unwrap().as_str(),
            "https://accounts.example.com"
        );
        assert!(origin_of_page("data:text/html,<form>").is_err());
    }

    #[test]
    fn matching_is_exact_on_scheme_host_and_port() {
        let stored = "https://example.com";
        assert!(credential_origin_matches(stored, "https://example.com/login"));
        assert!(credential_origin_matches(stored, "https://EXAMPLE.com:443/a?b#c"));
        // Saved on https never fills on http.
        assert!(!credential_origin_matches(stored, "http://example.com/login"));
        assert!(!credential_origin_matches(stored, "https://example.com:8443/"));
        assert!(!credential_origin_matches(stored, "https://login.example.com/"));
        assert!(!credential_origin_matches(stored, "https://example.com.evil.test/"));
        assert!(!credential_origin_matches(stored, "https://evilexample.com/"));
        assert!(!credential_origin_matches("https://login.example.com", "https://example.com/"));
        assert!(!credential_origin_matches("not an origin", "https://example.com/"));
    }
}
