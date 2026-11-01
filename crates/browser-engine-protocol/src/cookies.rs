//! Cookies handed to the engine for its profile (an import from another browser).
//!
//! Ferryx writes one [`StagedCookie`] per line to [`STAGED_COOKIES_FILE`] in the profile's
//! cache directory. The engine applies the file when it starts and whenever Ferryx sends
//! [`crate::Command::ImportCookies`], then deletes it. Fields are tab separated; a cookie whose
//! fields contain a tab or a line break is not representable and is skipped by the writer.

/// File name of the staged cookies inside a profile's cache directory.
pub const STAGED_COOKIES_FILE: &str = "ferryx-import-cookies.tsv";

/// Chromium's SameSite choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    Unspecified,
    NoRestriction,
    Lax,
    Strict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedCookie {
    /// `.example.com` for a domain cookie, `example.com` for a host-only cookie.
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    /// Seconds since the Unix epoch; `None` for a session cookie.
    pub expires_unix: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

fn representable(field: &str) -> bool {
    !field.contains(['\t', '\r', '\n'])
}

impl SameSite {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::NoRestriction => "none",
            Self::Lax => "lax",
            Self::Strict => "strict",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "unspecified" => Self::Unspecified,
            "none" => Self::NoRestriction,
            "lax" => Self::Lax,
            "strict" => Self::Strict,
            _ => return None,
        })
    }
}

impl StagedCookie {
    /// The cookie as one line, or `None` when a field cannot be written.
    pub fn to_line(&self) -> Option<String> {
        let fields = [&self.domain, &self.name, &self.value, &self.path];
        if !fields.iter().all(|field| representable(field)) || self.domain.is_empty() || self.name.is_empty() {
            return None;
        }
        Some(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.domain,
            self.name,
            self.value,
            self.path,
            self.expires_unix.map(|t| t.to_string()).unwrap_or_default(),
            u8::from(self.secure),
            u8::from(self.http_only),
            self.same_site.as_str(),
        ))
    }

    pub fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
        let [domain, name, value, path, expires, secure, http_only, same_site] = parts.as_slice() else {
            return None;
        };
        if domain.is_empty() || name.is_empty() {
            return None;
        }
        let flag = |v: &str| match v {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        };
        Some(Self {
            domain: domain.to_string(),
            name: name.to_string(),
            value: value.to_string(),
            path: if path.starts_with('/') { path.to_string() } else { "/".to_string() },
            expires_unix: if expires.is_empty() { None } else { Some(expires.parse().ok()?) },
            secure: flag(secure)?,
            http_only: flag(http_only)?,
            same_site: SameSite::parse(same_site)?,
        })
    }

    /// The URL the engine sets the cookie for: its host over https when secure.
    pub fn url(&self) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        format!("{scheme}://{}{}", self.domain.trim_start_matches('.'), self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie() -> StagedCookie {
        StagedCookie {
            domain: ".example.com".into(),
            name: "sid".into(),
            value: "a=b; c".into(),
            path: "/app".into(),
            expires_unix: Some(1_900_000_000),
            secure: true,
            http_only: true,
            same_site: SameSite::Lax,
        }
    }

    #[test]
    fn a_cookie_survives_a_round_trip() {
        let session = StagedCookie { expires_unix: None, same_site: SameSite::Unspecified, ..cookie() };
        for c in [cookie(), session] {
            assert_eq!(StagedCookie::parse(&c.to_line().unwrap()), Some(c));
        }
    }

    #[test]
    fn a_tab_or_line_break_in_a_field_is_not_written() {
        assert_eq!(StagedCookie { value: "a\tb".into(), ..cookie() }.to_line(), None);
        assert_eq!(StagedCookie { name: "a\nb".into(), ..cookie() }.to_line(), None);
    }

    #[test]
    fn url_uses_the_host_and_the_scheme_the_cookie_needs() {
        assert_eq!(cookie().url(), "https://example.com/app");
        let plain = StagedCookie { secure: false, domain: "a.test".into(), path: "/".into(), ..cookie() };
        assert_eq!(plain.url(), "http://a.test/");
    }
}
