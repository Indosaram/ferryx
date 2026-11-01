//! Opt-in search-engine suggestions for the omnibox. The host fetches them (a page origin could
//! not, CORS) from the engine's OpenSearch suggest endpoint and returns plain strings; any
//! failure is an error the UI answers by showing local suggestions only.

/// The OpenSearch suggest URL for a search engine id the UI settings use.
pub fn suggest_url(engine: &str, query: &str) -> Option<String> {
    let (base, key, extra): (&str, &str, &[(&str, &str)]) = match engine {
        "duckduckgo" => ("https://duckduckgo.com/ac/", "q", &[("type", "list")]),
        "google" => ("https://suggestqueries.google.com/complete/search", "q", &[("client", "firefox")]),
        "bing" => ("https://api.bing.com/osjson.aspx", "query", &[]),
        "brave" => ("https://search.brave.com/api/suggest", "q", &[]),
        _ => return None,
    };
    let mut url = url::Url::parse(base).ok()?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair(key, query.trim());
        for (name, value) in extra {
            pairs.append_pair(name, value);
        }
    }
    Some(url.into())
}

/// Parses an OpenSearch suggestions body: `["query", ["s1", "s2", ...], ...]`.
pub fn parse_open_search(body: &[u8], limit: usize) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let list = value.as_array()?.get(1)?.as_array()?;
    Some(
        list.iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .take(limit)
            .map(str::to_string)
            .collect(),
    )
}
