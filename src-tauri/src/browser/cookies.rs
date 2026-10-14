use crate::browser::BrowserError;
use cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires_unix: Option<i64>,
    pub same_site: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonCookie {
    name: String,
    #[serde(default)]
    value: String,
    #[serde(default, alias = "host")]
    domain: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    secure: bool,
    #[serde(default, alias = "http_only")]
    http_only: bool,
    #[serde(
        default,
        alias = "expirationDate",
        alias = "expiration_date",
        alias = "expires"
    )]
    expiration_date: Option<f64>,
    #[serde(default, alias = "same_site")]
    same_site: Option<String>,
}

fn from_json_cookie(cookie: JsonCookie) -> Result<ImportedCookie, BrowserError> {
    let name = cookie.name.trim().to_string();
    if name.is_empty() {
        return Err(BrowserError::CookieImport(
            "cookie name cannot be empty".into(),
        ));
    }
    Ok(ImportedCookie {
        name,
        value: cookie.value,
        domain: cookie
            .domain
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        path: cookie
            .path
            .filter(|value| value.starts_with('/'))
            .unwrap_or_else(|| "/".into()),
        secure: cookie.secure,
        http_only: cookie.http_only,
        expires_unix: cookie.expiration_date.and_then(|value| {
            if value.is_finite() && value > 0.0 {
                Some(value.floor() as i64)
            } else {
                None
            }
        }),
        same_site: cookie.same_site.map(|value| value.to_ascii_lowercase()),
    })
}

fn parse_json(input: &str) -> Result<Vec<ImportedCookie>, BrowserError> {
    let value: serde_json::Value = serde_json::from_str(input).map_err(|error| {
        BrowserError::CookieImport(format!("invalid JSON cookie file: {error}"))
    })?;
    let cookie_values = match value {
        serde_json::Value::Array(values) => values,
        serde_json::Value::Object(mut object) => object
            .remove("cookies")
            .and_then(|value| value.as_array().cloned())
            .ok_or_else(|| {
                BrowserError::CookieImport(
                    "JSON must be an array or contain a cookies array".into(),
                )
            })?,
        _ => {
            return Err(BrowserError::CookieImport(
                "JSON cookie file must contain an array".into(),
            ))
        }
    };
    cookie_values
        .into_iter()
        .map(|value| {
            serde_json::from_value::<JsonCookie>(value)
                .map_err(|error| {
                    BrowserError::CookieImport(format!("invalid cookie entry: {error}"))
                })
                .and_then(from_json_cookie)
        })
        .collect()
}

fn parse_netscape(input: &str) -> Result<Vec<ImportedCookie>, BrowserError> {
    let mut cookies = Vec::new();
    for (line_number, raw_line) in input.lines().enumerate() {
        let mut line = raw_line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        let mut http_only = false;
        if let Some(rest) = line.strip_prefix("#HttpOnly_") {
            http_only = true;
            line = rest;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            return Err(BrowserError::CookieImport(format!(
                "invalid Netscape cookie line {}: expected 7 tab-separated fields",
                line_number + 1
            )));
        }
        let name = fields[5].trim();
        if name.is_empty() {
            return Err(BrowserError::CookieImport(format!(
                "empty cookie name on line {}",
                line_number + 1
            )));
        }
        let expires_unix = fields[4]
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0);
        cookies.push(ImportedCookie {
            name: name.to_string(),
            value: fields[6].to_string(),
            domain: Some(fields[0].trim().to_string()).filter(|value| !value.is_empty()),
            path: if fields[2].starts_with('/') {
                fields[2].to_string()
            } else {
                "/".into()
            },
            secure: fields[3].eq_ignore_ascii_case("TRUE"),
            http_only,
            expires_unix,
            same_site: None,
        });
    }
    Ok(cookies)
}

pub fn parse_cookie_file(input: &str) -> Result<Vec<ImportedCookie>, BrowserError> {
    let trimmed = input.trim_start();
    let cookies = if trimmed.starts_with('[') || trimmed.starts_with('{') {
        parse_json(input)?
    } else {
        parse_netscape(input)?
    };
    if cookies.is_empty() {
        return Err(BrowserError::CookieImport(
            "cookie file did not contain any cookies".into(),
        ));
    }
    Ok(cookies)
}

pub fn cookie_from_imported(cookie: ImportedCookie) -> Result<Cookie<'static>, BrowserError> {
    let mut builder = Cookie::build((cookie.name, cookie.value))
        .path(cookie.path)
        .secure(cookie.secure)
        .http_only(cookie.http_only);

    if let Some(domain) = cookie.domain {
        builder = builder.domain(domain);
    }
    if let Some(expires_unix) = cookie.expires_unix {
        let expires = OffsetDateTime::from_unix_timestamp(expires_unix).map_err(|error| {
            BrowserError::CookieImport(format!(
                "invalid cookie expiration timestamp {expires_unix}: {error}"
            ))
        })?;
        builder = builder.expires(expires);
    }
    if let Some(same_site) = cookie.same_site.as_deref() {
        let parsed = match same_site {
            "lax" => Some(SameSite::Lax),
            "strict" => Some(SameSite::Strict),
            "none" | "no_restriction" | "no-restriction" => Some(SameSite::None),
            "unspecified" | "" => None,
            other => {
                return Err(BrowserError::CookieImport(format!(
                    "unsupported SameSite value: {other}"
                )))
            }
        };
        if let Some(parsed) = parsed {
            builder = builder.same_site(parsed);
        }
    }

    Ok(builder.build())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstalledBrowserKind {
    Chrome,
    Edge,
}

pub fn is_google_cookie(domain: Option<&str>, _name: &str) -> bool {
    let Some(domain) = domain else {
        return false;
    };
    let clean = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    clean == "google.com"
        || clean.ends_with(".google.com")
        || clean == "googleusercontent.com"
        || clean.ends_with(".googleusercontent.com")
        || clean == "gstatic.com"
        || clean.ends_with(".gstatic.com")
        || clean == "youtube.com"
        || clean.ends_with(".youtube.com")
        || clean == "doubleclick.net"
        || clean.ends_with(".doubleclick.net")
        || clean.starts_with("google.")
        || clean.contains(".google.")
}

pub fn filter_google_cookies(cookies: Vec<ImportedCookie>) -> Vec<ImportedCookie> {
    cookies
        .into_iter()
        .filter(|c| !is_google_cookie(c.domain.as_deref(), &c.name))
        .collect()
}

pub fn resolve_installed_browser_profile_dir(
    kind: InstalledBrowserKind,
    profile_name: Option<&str>,
    home_dir: Option<&std::path::Path>,
) -> Result<std::path::PathBuf, BrowserError> {
    let profile = profile_name.unwrap_or("Default");
    let mut base = match home_dir {
        Some(path) => path.to_path_buf(),
        None => {
            return Err(BrowserError::CookieImport(
                "home directory unknown".into(),
            ))
        }
    };

    #[cfg(target_os = "macos")]
    {
        base.push("Library");
        base.push("Application Support");
        match kind {
            InstalledBrowserKind::Chrome => {
                base.push("Google");
                base.push("Chrome");
            }
            InstalledBrowserKind::Edge => {
                base.push("Microsoft Edge");
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        base.push("AppData");
        base.push("Local");
        match kind {
            InstalledBrowserKind::Chrome => {
                base.push("Google");
                base.push("Chrome");
                base.push("User Data");
            }
            InstalledBrowserKind::Edge => {
                base.push("Microsoft");
                base.push("Edge");
                base.push("User Data");
            }
        }
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        base.push(".config");
        match kind {
            InstalledBrowserKind::Chrome => {
                base.push("google-chrome");
            }
            InstalledBrowserKind::Edge => {
                base.push("microsoft-edge");
            }
        }
    }

    base.push(profile);
    Ok(base)
}

pub fn parse_installed_profile_cookie_content(
    input: &str,
) -> Result<Vec<ImportedCookie>, BrowserError> {
    let all_cookies = parse_cookie_file(input)?;
    let filtered: Vec<ImportedCookie> = filter_google_cookies(all_cookies);
    if filtered.is_empty() {
        return Err(BrowserError::CookieImport(
            "no valid non-Google cookies found in profile".into(),
        ));
    }
    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_google_cookie_exclusion() {
        assert!(is_google_cookie(Some(".google.com"), "SID"));
        assert!(is_google_cookie(Some("accounts.google.com"), "SSID"));
        assert!(is_google_cookie(Some(".youtube.com"), "LOGIN_INFO"));
        assert!(is_google_cookie(Some("google.co.kr"), "NID"));
        assert!(is_google_cookie(Some(".gstatic.com"), "1P_JAR"));
        assert!(!is_google_cookie(Some(".github.com"), "user_session"));
        assert!(!is_google_cookie(Some("example.org"), "session"));
        assert!(!is_google_cookie(None, "session"));
    }

    #[test]
    fn test_filter_google_cookies() {
        let list = vec![
            ImportedCookie {
                name: "sid".into(),
                value: "123".into(),
                domain: Some(".google.com".into()),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires_unix: None,
                same_site: None,
            },
            ImportedCookie {
                name: "gh_session".into(),
                value: "abc".into(),
                domain: Some(".github.com".into()),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires_unix: None,
                same_site: None,
            },
        ];
        let filtered = filter_google_cookies(list);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "gh_session");
    }

    #[test]
    fn test_parse_installed_profile_cookie_content_excludes_google() {
        let json = r##"[
            {"name": "token", "value": "val1", "domain": "app.internal.test"},
            {"name": "google_auth", "value": "val2", "domain": "accounts.google.com"}
        ]"##;
        let cookies = parse_installed_profile_cookie_content(json).expect("should succeed");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "token");
    }

    #[test]
    fn test_parse_installed_profile_cookie_content_rejects_only_google() {
        let json = r##"[
            {"name": "google_auth", "value": "val2", "domain": "accounts.google.com"}
        ]"##;
        let err = parse_installed_profile_cookie_content(json).expect_err("should reject");
        assert!(matches!(err, BrowserError::CookieImport(_)));
    }
}
