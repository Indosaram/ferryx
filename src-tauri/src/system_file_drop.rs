//! Replays a file drop inside a system webview tab.
//!
//! # Why this is not Linux-only
//!
//! The platform webview cannot be handed an OS file drop as a `File` without native glue, and on
//! Windows and Linux the shell consumes the drop at app level, so a browser pane never sees it.
//! Delivering the drop as a synthetic `DragEvent` sequence with a `DataTransfer` the page can read
//! is what Chrome itself does for a real drop, and it is the one path every engine shares.
//! The Linux window destination (`crate::linux_file_drop`) and the Windows app-level drop both
//! call this.

use base64::Engine as _;

/// A JS string literal for `value`, escaped so a path can never end the literal early.
pub(crate) fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Builds the script that replays a file drop inside a system-webview tab.
///
/// The page receives the same sequence a Chromium drop produces (`dragenter`, `dragover`,
/// `drop`) with a `DataTransfer` whose `files` holds one entry per dropped path, named after the
/// file, so a handler that reads `dataTransfer.files` behaves identically on both engines.
pub(crate) fn file_drop_script(paths: &[String], point: (f64, f64)) -> String {
    // The page is given the file's real bytes, not an empty placeholder: a handler that reads
    // `files[0].size` or the contents must see what the file manager actually dropped.
    let files: Vec<String> = paths
        .iter()
        .map(|path| {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let bytes = base64::engine::general_purpose::STANDARD
                .encode(std::fs::read(path).unwrap_or_default());
            format!("{{ name: {}, bytes: {} }}", json_string(&name), json_string(&bytes))
        })
        .collect();
    let files = files.join(",");
    let paths = paths.iter().map(|path| json_string(path)).collect::<Vec<_>>().join(",");
    format!(
        r#"(() => {{
  const paths = [{paths}];
  const files = [{files}];
  const data = new DataTransfer();
  for (const file of files) {{
    const raw = atob(file.bytes);
    const buffer = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i += 1) buffer[i] = raw.charCodeAt(i);
    data.items.add(new File([buffer], file.name));
  }}
  data.setData('text/uri-list', paths.map((p) => 'file://' + p).join('\r\n'));
  const target = document.elementFromPoint({x}, {y}) || document.body;
  for (const type of ['dragenter', 'dragover', 'drop']) {{
    target.dispatchEvent(new DragEvent(type, {{ bubbles: true, cancelable: true, dataTransfer: data }}));
  }}
}})();"#,
        x = point.0,
        y = point.1
    )
}


#[cfg(test)]
mod tests {
    use super::{file_drop_script, json_string};

    #[test]
    fn the_drop_script_carries_the_file_bytes_and_its_name() {
        let dir = std::env::temp_dir().join("ferryx-drop-script-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("upload-fixture.txt");
        std::fs::write(&path, b"0123456789").unwrap();
        let script = file_drop_script(&[path.to_string_lossy().into_owned()], (845.0, 817.0));
        assert!(script.contains("name: \"upload-fixture.txt\""));
        // 10 bytes of '0123456789' as base64, so files[0].size is the real 10.
        assert!(script.contains("bytes: \"MDEyMzQ1Njc4OQ==\""));
        assert!(script.contains("new DataTransfer()"));
        assert!(script.contains("'drop'"));
        assert!(script.contains("elementFromPoint(845, 817)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_with_quotes_or_backslashes_is_escaped_not_broken() {
        // A Windows-style or quoted name must not terminate the JSON string early.
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("a\u{1}b"), "\"a\\u0001b\"");
    }
}
