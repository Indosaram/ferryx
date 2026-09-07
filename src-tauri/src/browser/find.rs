use crate::browser::{BrowserError, BrowserFindResult};

/// Upper bound on the counting walk. A pathological page can hold an unbounded
/// number of matches and each `window.find` step is a live DOM traversal, so the
/// count saturates here rather than freezing the webview.
const BROWSER_FIND_MATCH_LIMIT: usize = 1000;

pub fn browser_find_script(query: &str, backwards: bool) -> Result<String, BrowserError> {
    let query = serde_json::to_string(query).map_err(|error| {
        BrowserError::FindFailed(format!("failed to encode find query: {error}"))
    })?;
    Ok(format!(
        r#"(() => {{
  const query = {query};
  if (!query || typeof window.find !== 'function') {{
    return JSON.stringify({{ matchCount: 0, found: false }});
  }}

  const selection = window.getSelection ? window.getSelection() : null;
  const saved = selection && selection.rangeCount > 0 ? selection.getRangeAt(0).cloneRange() : null;
  const scrollX = window.scrollX || 0;
  const scrollY = window.scrollY || 0;

  // Count with the same engine that performs the highlight. A text scan over
  // `innerText` disagrees with `window.find` routinely -- input values, generated
  // ::before/::after content, shadow DOM and collapsed whitespace all differ --
  // and the UI presents this number as the authoritative match count.
  // Clearing the selection makes the walk start at the top of the document, and
  // wrapAround stays off so it terminates instead of cycling forever.
  if (selection) selection.removeAllRanges();
  let matchCount = 0;
  while (window.find(query, false, false, false, false, false, false)) {{
    matchCount += 1;
    if (matchCount >= {limit}) break;
  }}

  // Counting consumed the selection and may have scrolled the page, so restore
  // both before the real search: the user's cursor decides where it resumes.
  if (selection) {{
    selection.removeAllRanges();
    if (saved) selection.addRange(saved);
  }}
  if (typeof window.scrollTo === 'function') window.scrollTo(scrollX, scrollY);

  const found = window.find(query, false, {backwards}, true, false, false, false);
  return JSON.stringify({{ matchCount, found }});
}})()"#,
        backwards = if backwards { "true" } else { "false" },
        limit = BROWSER_FIND_MATCH_LIMIT
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
    fn find_script_counts_with_the_same_engine_that_highlights() {
        let script = browser_find_script("needle", false).unwrap();
        assert!(
            !script.contains("document.body.innerText") && !script.contains("indexOf(needle"),
            "match count must not fall back to a text scan that disagrees with window.find"
        );
        assert!(
            script.contains("window.find(query, false, false, false, false, false, false)"),
            "counting walk must run forwards without wrapAround so it terminates"
        );
        assert!(script.contains(&BROWSER_FIND_MATCH_LIMIT.to_string()));
    }

    #[test]
    fn find_script_restores_selection_and_scroll_before_searching() {
        let script = browser_find_script("needle", false).unwrap();
        assert!(script.contains("cloneRange()"));
        assert!(script.contains("addRange(saved)"));
        assert!(script.contains("window.scrollTo(scrollX, scrollY)"));
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
