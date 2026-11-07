//! Safari `Cookies.binarycookies` parser (Apple Binary Cookies format).
//!
//! Safari does not store cookies in SQLite. Instead it uses a proprietary
//! big-/little-endian mixed binary container. This module parses that format
//! into the crate's [`CookieEntry`] type.
//!
//! ## File layout
//!
//! ```text
//! magic        : 4 bytes  = b"cook"
//! num_pages    : u32 BE
//! page_sizes[] : u32 BE   x num_pages
//! pages[]      : raw page blobs (each of its declared size)
//! (trailing checksum + 8-byte footer follow the pages; ignored here)
//! ```
//!
//! ## Page layout (little-endian within the page)
//!
//! ```text
//! header       : u32 = 0x0000_0100 (bytes 00 00 01 00)
//! num_cookies  : u32 LE
//! offsets[]    : u32 LE   x num_cookies (relative to page start)
//! footer       : u32 = 0
//! cookies[]    : raw cookie blobs
//! ```
//!
//! ## Cookie layout (offsets relative to the cookie start, little-endian)
//!
//! ```text
//! 0  : cookie_size   u32 LE
//! 4  : version       u32 (unused)
//! 8  : flags         u32 LE   (0x1 = Secure, 0x4 = HttpOnly)
//! 12 : (unused)      u32
//! 16 : domain_offset u32 LE
//! 20 : name_offset   u32 LE
//! 24 : path_offset   u32 LE
//! 28 : value_offset  u32 LE
//! 32 : end_marker    u64 (unused)
//! 40 : expiry        f64 LE  (Mac absolute time — seconds since 2001-01-01)
//! 48 : creation      f64 LE  (Mac absolute time — unused)
//! .. : NUL-terminated strings for domain / name / path / value
//! ```

use std::path::{Path, PathBuf};

use crate::{CookieEntry, ImportError, ImportResult};

/// Seconds between the Unix epoch (1970-01-01) and the Mac/Core Data epoch
/// (2001-01-01). Safari cookie dates are stored relative to the Mac epoch.
const MAC_EPOCH_OFFSET: f64 = 978_307_200.0;

const FLAG_SECURE: u32 = 0x1;
const FLAG_HTTPONLY: u32 = 0x4;

/// The fixed-size cookie header, before the variable-length strings.
const COOKIE_HEADER_LEN: usize = 56;

/// Guards against absurd counts in corrupt files (prevents huge allocations).
const MAX_REASONABLE_COUNT: usize = 10_000_000;

/// Locates Safari's binary cookies file relative to `safari_dir`
/// (`~/Library/Safari`) and parses it.
///
/// Cookies do not live inside the Safari profile directory. Modern
/// (sandboxed) Safari stores them under the app container; older versions use
/// the shared per-user `Cookies` directory. Both candidates are probed.
pub fn parse_safari_cookies(safari_dir: &Path) -> ImportResult<Vec<CookieEntry>> {
    let path = locate_cookies_file(safari_dir).ok_or_else(|| {
        ImportError::FileNotFound(
            safari_dir
                .join("../Cookies/Cookies.binarycookies")
                .display()
                .to_string(),
        )
    })?;

    let data = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ImportError::FileNotFound(path.display().to_string()),
        std::io::ErrorKind::PermissionDenied => {
            ImportError::PermissionDenied(path.display().to_string())
        }
        _ => ImportError::Io(format!("{}: {}", path.display(), e)),
    })?;

    parse_binary_cookies(&data)
}

/// Returns the first existing binary cookies file for a Safari install.
fn locate_cookies_file(safari_dir: &Path) -> Option<PathBuf> {
    // `safari_dir` is `~/Library/Safari`; its parent is `~/Library`.
    let library_dir = safari_dir.parent()?;

    let candidates = [
        // macOS 14+ sandboxed container location.
        library_dir.join("Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies"),
        // Legacy shared per-user location.
        library_dir.join("Cookies/Cookies.binarycookies"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

/// Parses an in-memory `Cookies.binarycookies` blob.
///
/// Never panics: truncated/corrupt structures cause the affected page or
/// cookie to be skipped, and a bad magic yields a [`ImportError::Parse`].
pub fn parse_binary_cookies(data: &[u8]) -> ImportResult<Vec<CookieEntry>> {
    if data.len() < 8 || &data[0..4] != b"cook" {
        return Err(ImportError::Parse(
            "invalid Cookies.binarycookies magic".into(),
        ));
    }

    let num_pages = read_u32_be(data, 4)
        .ok_or_else(|| ImportError::Parse("truncated cookies header".into()))?
        as usize;
    if num_pages > MAX_REASONABLE_COUNT {
        return Err(ImportError::Parse("implausible cookie page count".into()));
    }

    // Read the page-size table (u32 BE x num_pages) starting at offset 8.
    let mut page_sizes = Vec::with_capacity(num_pages.min(1024));
    let mut off = 8usize;
    for _ in 0..num_pages {
        match read_u32_be(data, off) {
            Some(size) => page_sizes.push(size as usize),
            None => return Err(ImportError::Parse("truncated page-size table".into())),
        }
        off += 4;
    }

    // Pages follow immediately after the size table.
    let mut page_start = off;
    let mut results = Vec::new();
    for &page_size in &page_sizes {
        let page_end = page_start.saturating_add(page_size);
        if page_size == 0 || page_end > data.len() {
            // Corrupt/truncated — stop gracefully with what we have.
            break;
        }
        parse_page(&data[page_start..page_end], &mut results);
        page_start = page_end;
    }

    Ok(results)
}

/// Parses a single page and appends any recovered cookies.
fn parse_page(page: &[u8], out: &mut Vec<CookieEntry>) {
    // header (u32) + num_cookies (u32) minimum.
    if page.len() < 8 {
        return;
    }

    let num_cookies = match read_u32_le(page, 4) {
        Some(n) => n as usize,
        None => return,
    };
    if num_cookies > MAX_REASONABLE_COUNT {
        return;
    }

    let mut off = 8usize;
    for _ in 0..num_cookies {
        let cookie_off = match read_u32_le(page, off) {
            Some(o) => o as usize,
            None => return,
        };
        off += 4;
        if let Some(entry) = parse_cookie(page, cookie_off) {
            out.push(entry);
        }
    }
}

/// Parses one cookie record located at `base` within its page.
fn parse_cookie(page: &[u8], base: usize) -> Option<CookieEntry> {
    // Ensure the fixed header fits within the page.
    if base.checked_add(COOKIE_HEADER_LEN)? > page.len() {
        return None;
    }

    let flags = read_u32_le(page, base + 8)?;
    let domain_off = read_u32_le(page, base + 16)? as usize;
    let name_off = read_u32_le(page, base + 20)? as usize;
    let path_off = read_u32_le(page, base + 24)? as usize;
    let value_off = read_u32_le(page, base + 28)? as usize;
    let expiry = read_f64_le(page, base + 40)?;

    let host = read_cstring(page, base.checked_add(domain_off)?);
    if host.is_empty() {
        return None;
    }
    let name = read_cstring(page, base.checked_add(name_off)?);
    let path = read_cstring(page, base.checked_add(path_off)?);
    let value = read_cstring(page, base.checked_add(value_off)?);

    let expires = (expiry + MAC_EPOCH_OFFSET) as i64;

    Some(CookieEntry {
        host,
        name,
        value,
        path,
        expires,
        is_secure: flags & FLAG_SECURE != 0,
        is_httponly: flags & FLAG_HTTPONLY != 0,
        // The binarycookies format predates SameSite; report "unspecified".
        same_site: -1,
    })
}

fn read_u32_be(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_f64_le(data: &[u8], off: usize) -> Option<f64> {
    let b = data.get(off..off.checked_add(8)?)?;
    let mut arr = [0u8; 8];
    arr.copy_from_slice(b);
    Some(f64::from_le_bytes(arr))
}

/// Reads a NUL-terminated UTF-8 string starting at `off`. Out-of-range offsets
/// or missing terminators degrade gracefully to an empty / best-effort string.
fn read_cstring(data: &[u8], off: usize) -> String {
    if off >= data.len() {
        return String::new();
    }
    let slice = &data[off..];
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one cookie record blob.
    fn build_cookie(
        domain: &str,
        name: &str,
        path: &str,
        value: &str,
        flags: u32,
        expiry: f64,
    ) -> Vec<u8> {
        let header = COOKIE_HEADER_LEN as u32;
        let domain_off = header;
        let name_off = domain_off + domain.len() as u32 + 1;
        let path_off = name_off + name.len() as u32 + 1;
        let value_off = path_off + path.len() as u32 + 1;
        let total = value_off + value.len() as u32 + 1;

        let mut c = Vec::new();
        c.extend_from_slice(&total.to_le_bytes()); // size
        c.extend_from_slice(&0u32.to_le_bytes()); // version
        c.extend_from_slice(&flags.to_le_bytes()); // flags
        c.extend_from_slice(&0u32.to_le_bytes()); // unused
        c.extend_from_slice(&domain_off.to_le_bytes());
        c.extend_from_slice(&name_off.to_le_bytes());
        c.extend_from_slice(&path_off.to_le_bytes());
        c.extend_from_slice(&value_off.to_le_bytes());
        c.extend_from_slice(&0u64.to_le_bytes()); // end marker
        c.extend_from_slice(&expiry.to_le_bytes()); // expiry f64
        c.extend_from_slice(&0f64.to_le_bytes()); // creation f64
        c.extend_from_slice(domain.as_bytes());
        c.push(0);
        c.extend_from_slice(name.as_bytes());
        c.push(0);
        c.extend_from_slice(path.as_bytes());
        c.push(0);
        c.extend_from_slice(value.as_bytes());
        c.push(0);
        c
    }

    /// Builds a page containing the given cookie blobs.
    fn build_page(cookies: &[Vec<u8>]) -> Vec<u8> {
        let num = cookies.len() as u32;
        let header_size = 4 + 4 + cookies.len() * 4 + 4;
        let mut offsets = Vec::new();
        let mut running = header_size;
        for c in cookies {
            offsets.push(running as u32);
            running += c.len();
        }

        let mut page = Vec::new();
        page.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // page header
        page.extend_from_slice(&num.to_le_bytes());
        for o in &offsets {
            page.extend_from_slice(&o.to_le_bytes());
        }
        page.extend_from_slice(&0u32.to_le_bytes()); // page footer
        for c in cookies {
            page.extend_from_slice(c);
        }
        page
    }

    /// Builds a full binarycookies file from the given pages.
    fn build_file(pages: &[Vec<u8>]) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"cook");
        f.extend_from_slice(&(pages.len() as u32).to_be_bytes());
        for p in pages {
            f.extend_from_slice(&(p.len() as u32).to_be_bytes());
        }
        for p in pages {
            f.extend_from_slice(p);
        }
        // Real files append a checksum + 8-byte footer here; the parser ignores
        // everything past the declared pages, so we omit them.
        f
    }

    #[test]
    fn invalid_magic_returns_parse_error() {
        let err = parse_binary_cookies(b"not a cookie file").unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn empty_input_returns_parse_error() {
        let err = parse_binary_cookies(&[]).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn zero_pages_returns_empty() {
        let file = build_file(&[]);
        let cookies = parse_binary_cookies(&file).unwrap();
        assert!(cookies.is_empty());
    }

    #[test]
    fn parses_single_cookie() {
        // expiry: 0 Mac-epoch seconds -> Unix 978307200.
        let cookie = build_cookie(
            ".example.com",
            "session",
            "/",
            "abc123",
            FLAG_SECURE | FLAG_HTTPONLY,
            0.0,
        );
        let page = build_page(&[cookie]);
        let file = build_file(&[page]);

        let cookies = parse_binary_cookies(&file).unwrap();
        assert_eq!(cookies.len(), 1);
        let c = &cookies[0];
        assert_eq!(c.host, ".example.com");
        assert_eq!(c.name, "session");
        assert_eq!(c.path, "/");
        assert_eq!(c.value, "abc123");
        assert!(c.is_secure);
        assert!(c.is_httponly);
        assert_eq!(c.same_site, -1);
        assert_eq!(c.expires, 978_307_200);
    }

    #[test]
    fn parses_multiple_cookies_across_pages() {
        let c1 = build_cookie(".a.com", "one", "/", "1", 0, 100.0);
        let c2 = build_cookie(".b.com", "two", "/app", "2", FLAG_SECURE, 200.0);
        let c3 = build_cookie(".c.com", "three", "/", "3", FLAG_HTTPONLY, 300.0);
        let page1 = build_page(&[c1, c2]);
        let page2 = build_page(&[c3]);
        let file = build_file(&[page1, page2]);

        let cookies = parse_binary_cookies(&file).unwrap();
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0].host, ".a.com");
        assert!(!cookies[0].is_secure && !cookies[0].is_httponly);
        assert_eq!(cookies[1].host, ".b.com");
        assert!(cookies[1].is_secure && !cookies[1].is_httponly);
        assert_eq!(cookies[2].host, ".c.com");
        assert!(!cookies[2].is_secure && cookies[2].is_httponly);
        assert_eq!(cookies[2].expires, 978_307_200 + 300);
    }

    #[test]
    fn skips_cookie_with_empty_host() {
        let cookie = build_cookie("", "orphan", "/", "x", 0, 0.0);
        let page = build_page(&[cookie]);
        let file = build_file(&[page]);

        let cookies = parse_binary_cookies(&file).unwrap();
        assert!(cookies.is_empty());
    }

    #[test]
    fn truncated_page_data_is_skipped_gracefully() {
        let cookie = build_cookie(".example.com", "session", "/", "abc123", 0, 0.0);
        let page = build_page(&[cookie]);
        let mut file = build_file(&[page]);
        // Chop off the tail so the declared page overruns the buffer.
        file.truncate(file.len() - 10);

        // Must not panic; returns whatever was safely recoverable (none here).
        let cookies = parse_binary_cookies(&file).unwrap();
        assert!(cookies.is_empty());
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        // Create the Safari dir but no Cookies files anywhere.
        let safari_dir = tmp.path().join("Library/Safari");
        std::fs::create_dir_all(&safari_dir).unwrap();
        let err = parse_safari_cookies(&safari_dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn locates_and_parses_legacy_cookie_path() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("Library");
        let safari_dir = library.join("Safari");
        std::fs::create_dir_all(&safari_dir).unwrap();
        let cookies_dir = library.join("Cookies");
        std::fs::create_dir_all(&cookies_dir).unwrap();

        let cookie = build_cookie(".legacy.com", "k", "/", "v", 0, 0.0);
        let file = build_file(&[build_page(&[cookie])]);
        std::fs::write(cookies_dir.join("Cookies.binarycookies"), &file).unwrap();

        let cookies = parse_safari_cookies(&safari_dir).unwrap();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].host, ".legacy.com");
    }

    #[test]
    fn locates_and_parses_container_cookie_path() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("Library");
        let safari_dir = library.join("Safari");
        std::fs::create_dir_all(&safari_dir).unwrap();
        let container_dir = library.join("Containers/com.apple.Safari/Data/Library/Cookies");
        std::fs::create_dir_all(&container_dir).unwrap();

        let cookie = build_cookie(".container.com", "k", "/", "v", 0, 0.0);
        let file = build_file(&[build_page(&[cookie])]);
        std::fs::write(container_dir.join("Cookies.binarycookies"), &file).unwrap();

        let cookies = parse_safari_cookies(&safari_dir).unwrap();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].host, ".container.com");
    }

    #[test]
    fn corrupt_file_on_disk_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("Library");
        let safari_dir = library.join("Safari");
        std::fs::create_dir_all(&safari_dir).unwrap();
        let cookies_dir = library.join("Cookies");
        std::fs::create_dir_all(&cookies_dir).unwrap();
        std::fs::write(cookies_dir.join("Cookies.binarycookies"), b"garbage").unwrap();

        let err = parse_safari_cookies(&safari_dir).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {err:?}");
    }
}
