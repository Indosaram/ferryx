use crate::ipc::IpcError;
use crate::native_terminal::surface_host::NativeTerminalSurfaceHostState;
use std::sync::OnceLock;

#[tauri::command]
pub async fn cmd_native_terminal_hyperlink_at(
    state: tauri::State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    col: u16,
    row: u16,
) -> Result<Option<String>, IpcError> {
    let uri = state
        .inner()
        .with_session_terminal(&session_id, |t| t.hyperlink_uri_at(col, row))
        .map_err(|e| IpcError::internal(e.to_string()))?;
    Ok(uri.map(|uri| normalize_local_file_uri(&uri, local_host_names())))
}

/// Names under which this machine may appear as the authority of a
/// `file://` URI (e.g. GNU `ls --hyperlink` emits `file://<hostname>/path`).
fn local_host_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        system_hostname()
            .map(|h| host_aliases(&h))
            .unwrap_or_default()
    })
}

/// The full hostname plus the name without a trailing ".local" and the short
/// name before the first '.'.
fn host_aliases(hostname: &str) -> Vec<String> {
    let hostname = hostname.trim().trim_end_matches('.');
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            names.push(name.to_string());
        }
    };
    push(hostname);
    // `get` returns None instead of panicking when the cut would split a non-ASCII char.
    if let Some(stem) = hostname
        .len()
        .checked_sub(".local".len())
        .filter(|&cut| cut > 0)
        .filter(|&cut| {
            hostname
                .get(cut..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".local"))
        })
        .map(|cut| &hostname[..cut])
    {
        push(stem);
    }
    if let Some((short, _)) = hostname.split_once('.') {
        push(short);
    }
    names
}

#[cfg(unix)]
fn system_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: buf is a valid writable buffer of the given length.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..len].to_vec()).ok()
}

#[cfg(not(unix))]
fn system_hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

/// Rewrite `file://<local-host>/path` to `file:///path`, preserving the path
/// and its percent-encoding byte for byte. Every other URI (other schemes,
/// empty authority, or a `file://` URI naming a different host) is returned
/// unchanged so the UI keeps refusing remote file links.
pub(crate) fn normalize_local_file_uri(uri: &str, local_hosts: &[String]) -> String {
    const PREFIX: &str = "file://";
    if !uri
        .get(..PREFIX.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(PREFIX))
    {
        return uri.to_string();
    }
    let rest = &uri[PREFIX.len()..];
    let Some(slash) = rest.find('/') else {
        return uri.to_string();
    };
    let (authority, path) = rest.split_at(slash);
    if authority.is_empty()
        || !local_hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(authority))
    {
        return uri.to_string();
    }
    format!("file://{path}")
}

#[cfg(test)]
mod tests {
    use super::{host_aliases, normalize_local_file_uri};

    fn hosts() -> Vec<String> {
        host_aliases("Indoui-MacBookPro-3.local")
    }

    #[test]
    fn local_hostname_authority_is_stripped() {
        assert_eq!(
            normalize_local_file_uri(
                "file://Indoui-MacBookPro-3.local/Users/me/foo.png",
                &hosts()
            ),
            "file:///Users/me/foo.png"
        );
    }

    #[test]
    fn hostname_match_is_case_insensitive_including_scheme() {
        assert_eq!(
            normalize_local_file_uri("FILE://indoui-macbookpro-3.LOCAL/tmp/a", &hosts()),
            "file:///tmp/a"
        );
    }

    #[test]
    fn hostname_without_local_suffix_and_short_name_match() {
        let hosts = host_aliases("box.corp.example.com");
        assert_eq!(
            normalize_local_file_uri("file://box/srv/x", &hosts),
            "file:///srv/x"
        );
        assert_eq!(
            normalize_local_file_uri(
                "file://Indoui-MacBookPro-3/tmp/a",
                &super::host_aliases("Indoui-MacBookPro-3.local")
            ),
            "file:///tmp/a"
        );
    }

    #[test]
    fn percent_encoding_and_query_are_preserved_exactly() {
        assert_eq!(
            normalize_local_file_uri(
                "file://Indoui-MacBookPro-3.local/Users/me/My%20Dir/%E2%9C%93.png#L3",
                &hosts()
            ),
            "file:///Users/me/My%20Dir/%E2%9C%93.png#L3"
        );
    }

    #[test]
    fn remote_host_file_uri_is_left_untouched() {
        let uri = "file://other-machine.local/Users/me/foo.png";
        assert_eq!(normalize_local_file_uri(uri, &hosts()), uri);
    }

    #[test]
    fn non_file_and_already_local_uris_are_left_untouched() {
        for uri in [
            "https://Indoui-MacBookPro-3.local/x",
            "file:///tmp/a",
            "file://localhost/tmp/a",
            "file://Indoui-MacBookPro-3.local",
            "file://Indoui-MacBookPro-3.local:22/tmp/a",
            "fil",
        ] {
            assert_eq!(normalize_local_file_uri(uri, &hosts()), uri);
        }
    }

    #[test]
    fn empty_host_list_changes_nothing() {
        let uri = "file://Indoui-MacBookPro-3.local/tmp/a";
        assert_eq!(normalize_local_file_uri(uri, &[]), uri);
    }

    #[test]
    fn host_aliases_dedupes_and_derives_short_names() {
        assert_eq!(
            host_aliases("Indoui-MacBookPro-3.local"),
            vec![
                "Indoui-MacBookPro-3.local".to_string(),
                "Indoui-MacBookPro-3".to_string()
            ]
        );
        assert_eq!(host_aliases("plain"), vec!["plain".to_string()]);
        assert!(host_aliases("").is_empty());
    }

    #[test]
    fn non_ascii_uris_never_panic_and_stay_untouched() {
        // A remote shell controls OSC 8 URIs; byte 7 may fall inside a multibyte char.
        for uri in ["https:éx", "abcdefé", "한글한", "file:/é"] {
            assert_eq!(normalize_local_file_uri(uri, &hosts()), uri);
        }
        assert_eq!(
            normalize_local_file_uri("file://Indoui-MacBookPro-3.local/tmp/한.png", &hosts()),
            "file:///tmp/한.png"
        );
    }

    #[test]
    fn host_aliases_accepts_non_ascii_hostnames_without_panicking() {
        // COMPUTERNAME on Windows can be non-ASCII; byte slicing must not split a char.
        assert_eq!(host_aliases("한글PC"), vec!["한글PC".to_string()]);
        assert_eq!(
            host_aliases("한글한.local"),
            vec!["한글한.local".to_string(), "한글한".to_string()]
        );
    }
}
