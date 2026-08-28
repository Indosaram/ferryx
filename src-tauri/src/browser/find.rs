use crate::browser::{BrowserError, BrowserFindResult};

pub fn browser_find_script(query: &str, backwards: bool) -> Result<String, BrowserError> {
    let query = serde_json::to_string(query).map_err(|error| {
        BrowserError::FindFailed(format!("failed to encode find query: {error}"))
    })?;
    Ok(format!(
        r#"(() => {{
  const query = {query};
  if (!query) return JSON.stringify({{ matchCount: 0, found: false }});
  const text = (document.body && document.body.innerText) || '';
  const haystack = text.toLocaleLowerCase();
  const needle = query.toLocaleLowerCase();
  let matchCount = 0;
  let offset = 0;
  while (needle && (offset = haystack.indexOf(needle, offset)) !== -1) {{
    matchCount += 1;
    offset += Math.max(needle.length, 1);
  }}
  const found = typeof window.find === 'function'
    ? window.find(query, false, {backwards}, true, false, false, false)
    : false;
  return JSON.stringify({{ matchCount, found }});
}})()"#,
        backwards = if backwards { "true" } else { "false" }
    ))
}

pub fn parse_browser_find_callback(result: &str) -> Result<BrowserFindResult, BrowserError> {
    let payload: String = serde_json::from_str(result).map_err(|error| {
        BrowserError::FindFailed(format!("invalid find callback result: {error}"))
    })?;
    serde_json::from_str(&payload)
        .map_err(|error| BrowserError::FindFailed(format!("invalid find response: {error}")))
}

pub const BROWSER_CLEAR_FIND_SCRIPT: &str = r#"(() => {
  const selection = window.getSelection && window.getSelection();
  if (selection) selection.removeAllRanges();
  return true;
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_script_json_encodes_query_and_direction() {
        let script = browser_find_script("a'\"</script>", true).unwrap();
        assert!(script.contains("a'\\\"</script>"));
        assert!(script.contains("window.find"));
        assert!(script.contains("true, true, false"));
    }

    #[test]
    fn parses_tauri_find_callback_payload() {
        let result = r#""{\"matchCount\":3,\"found\":true}""#;
        assert_eq!(
            parse_browser_find_callback(result).unwrap(),
            BrowserFindResult {
                match_count: 3,
                found: true,
            }
        );
    }
}
