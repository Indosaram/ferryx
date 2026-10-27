/// GNU ld otherwise auto-exports every linked global when Rust's cdylib
/// export list is empty, exceeding PE's 16-bit export ordinal limit for this
/// application. Retain explicitly declared exports, not implicit Rust globals.
/// GNULLVM uses a distinct linker and is intentionally not covered here.
pub fn cdylib_export_link_arg(target: &str) -> Option<&'static str> {
    target.ends_with("-windows-gnu").then_some("-Wl,--exclude-all-symbols")
}
