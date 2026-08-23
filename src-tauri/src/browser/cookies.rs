use crate::browser::BrowserError;
use serde::Deserialize;

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
    #[serde(default, alias = "expirationDate", alias = "expiration_date", alias = "expires")]
    expiration_date: Option<f64>,
    #[serde(default, alias = "same_site")]
    same_site: Option<String>,
}

fn from_json_cookie(cookie: JsonCookie) -> Result<ImportedCookie, BrowserError> {
    let name = cookie.name.trim().to_string();
    if name.is_empty() {
        return Err(BrowserError::CookieImport("cookie name cannot be empty".into()));
    }
    Ok(ImportedCookie {
        name,
        value: cookie.value,
        domain: cookie.domain.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()),
        path: cookie.path.filter(|value| value.starts_with('/')).unwrap_or_else(|| "/".into()),
        secure: cookie.secure,
        http_only: cookie.http_only,
        expires_unix: cookie.expiration_date.and_then(|value| {
            if value.is_finite() && value > 0.0 { Some(value.floor() as i64) } else { None }
        }),
        same_site: cookie.same_site.map(|value| value.to_ascii_lowercase()),
    })
}

fn parse_json(input: &str) -> Result<Vec<ImportedCookie>, BrowserError> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|error| BrowserError::CookieImport(format!("invalid JSON cookie file: {error}")))?;
    let cookie_values = match value {
        serde_json::Value::Array(values) => values,
        serde_json::Value::Object(mut object) => object
            .remove("cookies")
            .and_then(|value| value.as_array().cloned())
            .ok_or_else(|| BrowserError::CookieImport("JSON must be an array or contain a cookies array".into()))?,
        _ => return Err(BrowserError::CookieImport("JSON cookie file must contain an array".into())),
    };
    cookie_values
        .into_iter()
        .map(|value| {
            serde_json::from_value::<JsonCookie>(value)
                .map_err(|error| BrowserError::CookieImport(format!("invalid cookie entry: {error}")))
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
            return Err(BrowserError::CookieImport(format!("empty cookie name on line {}", line_number + 1)));
        }
        let expires_unix = fields[4].trim().parse::<i64>().ok().filter(|value| *value > 0);
        cookies.push(ImportedCookie {
            name: name.to_string(),
            value: fields[6].to_string(),
            domain: Some(fields[0].trim().to_string()).filter(|value| !value.is_empty()),
            path: if fields[2].starts_with('/') { fields[2].to_string() } else { "/".into() },
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
        return Err(BrowserError::CookieImport("cookie file did not contain any cookies".into()));
    }
    Ok(cookies)
}
