//! Secret redaction for what an agent observes (snapshot URL, title and element names).
//!
//! Observation never carries form values, and passwords are never read; this pass removes
//! credentials that pages put in visible text or in the address: bearer tokens, JWTs, API keys
//! of well-known shapes, long opaque tokens, URL userinfo and secret-named query parameters.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

/// Replacement for a redacted value.
pub const REDACTED: &str = "[redacted]";

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // Authorization header values.
        r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{16,}",
        // JSON Web Tokens.
        r"\beyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}",
        // Well-known API key shapes.
        r"\b(?:sk|pk|rk)-[A-Za-z0-9_-]{16,}",
        r"\bgh[pousr]_[A-Za-z0-9]{20,}",
        r"\bgithub_pat_[A-Za-z0-9_]{20,}",
        r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
        r"\bAKIA[0-9A-Z]{16}\b",
        r"\bAIza[0-9A-Za-z_-]{35}\b",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid secret pattern"))
    .collect()
});

/// A run of 32+ token characters; redacted when it looks random (see [`looks_opaque`]).
static OPAQUE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_-]{32,}").expect("valid token pattern"));

/// An opaque token rather than a slug or a long word: digit-heavy (hex ids), or mixed case
/// with digits and no word separators (base64-style keys).
fn looks_opaque(candidate: &str) -> bool {
    let digits = candidate.chars().filter(char::is_ascii_digit).count();
    let letters = candidate.chars().filter(char::is_ascii_alphabetic).count();
    if letters < 6 || digits == 0 {
        return false;
    }
    let mixed_case = candidate.chars().any(|c| c.is_ascii_uppercase())
        && candidate.chars().any(|c| c.is_ascii_lowercase())
        && !candidate.contains('-');
    digits * 5 >= candidate.len() || mixed_case
}

/// Redacts secrets in page text.
pub fn redact_text(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for pattern in SECRET_PATTERNS.iter() {
        if pattern.is_match(&out) {
            out = Cow::Owned(pattern.replace_all(&out, REDACTED).into_owned());
        }
    }
    if OPAQUE_TOKEN.find_iter(&out).any(|found| looks_opaque(found.as_str())) {
        let replaced = OPAQUE_TOKEN.replace_all(&out, |caps: &regex::Captures<'_>| {
            let found = &caps[0];
            if looks_opaque(found) { REDACTED.to_string() } else { found.to_string() }
        });
        out = Cow::Owned(replaced.into_owned());
    }
    out
}

fn secret_parameter(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "token", "secret", "password", "passwd", "pwd", "apikey", "api_key", "key", "auth", "code",
        "session", "sig", "signature", "credential", "otp", "ticket",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

/// Redacts URL userinfo, secret-named query/fragment parameters and token-shaped text.
pub fn redact_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return redact_text(raw).into_owned();
    };
    if !url.username().is_empty() {
        let _ = url.set_username("redacted");
    }
    if url.password().is_some() {
        let _ = url.set_password(None);
    }
    let redact_pairs = |query: &str| -> Option<String> {
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
        if !pairs.iter().any(|(name, _)| secret_parameter(name)) {
            return None;
        }
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in &pairs {
            serializer.append_pair(name, if secret_parameter(name) { REDACTED } else { value });
        }
        Some(serializer.finish())
    };
    let query = url.query().and_then(redact_pairs);
    if let Some(query) = query {
        url.set_query(Some(&query));
    }
    // OAuth implicit flows put the access token in the fragment.
    let fragment = url.fragment().filter(|fragment| fragment.contains('=')).and_then(redact_pairs);
    if let Some(fragment) = fragment {
        url.set_fragment(Some(&fragment));
    }
    redact_text(url.as_str()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_shaped_text_is_redacted_and_prose_is_kept() {
        assert_eq!(redact_text("Save changes"), "Save changes");
        assert!(matches!(redact_text("Save changes"), Cow::Borrowed(_)));
        assert_eq!(redact_text("Authorization: Bearer abc.def-ghi_12345678"), "Authorization: [redacted]");
        assert_eq!(redact_text("Basic settings and token options"), "Basic settings and token options");
        assert_eq!(redact_text("key sk-ABCDEFGHIJKLMNOPqrst done"), "key [redacted] done");
        assert_eq!(redact_text("ghp_ABCDEFGHIJKLMNOPQRSTuvwx"), REDACTED);
        assert_eq!(redact_text("AKIAABCDEFGHIJKLMNOP"), REDACTED);
        assert_eq!(
            redact_text("jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJlX3g"),
            "jwt [redacted]"
        );
        assert_eq!(redact_text("id 0f3c9a7b2e1d4c5b6a7980f1e2d3c4b5a6"), "id [redacted]");
        assert_eq!(redact_text("k Zx81QmT0pL2vN7rB9sW4yH6jK3fD5gA1"), "k [redacted]");
        // A long word without digits is not a token.
        let word = "pneumonoultramicroscopicsilicovolcanoconiosis";
        assert_eq!(redact_text(word), word);
        let slug = "getting-started-with-rust-2024-edition-guide";
        assert_eq!(redact_text(slug), slug);
    }

    #[test]
    fn urls_lose_userinfo_and_secret_parameters() {
        assert_eq!(
            redact_url("https://user:pw@a.test/cb?code=xyz&state=s1#access_token=tok&x=1"),
            "https://redacted@a.test/cb?code=%5Bredacted%5D&state=s1#access_token=%5Bredacted%5D&x=1"
        );
        assert_eq!(redact_url("https://a.test/search?q=rust"), "https://a.test/search?q=rust");
        assert_eq!(redact_url("about:blank"), "about:blank");
        assert_eq!(redact_url("not a url Bearer abcdefghij0123456"), "not a url [redacted]");
    }
}
