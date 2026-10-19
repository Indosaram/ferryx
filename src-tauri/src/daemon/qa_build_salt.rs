//! QA-only build salt (todo 12). Builds B and C of the daemon lifetime QA harness share one command
//! and version and differ only in `FERRYX_QA_BUILD_SALT`, read at compile time here. The `#[used]`
//! static keeps the salt bytes in the executable, so the two builds hash differently and therefore
//! get distinct runtime generation keys (`daemon::launchd::generation`). Only builds with the
//! `local-split-qa` feature carry it; release builds never compile this module.

/// Name of the build-time environment variable carrying the salt.
pub const BUILD_SALT_ENV: &str = "FERRYX_QA_BUILD_SALT";

/// Marker that brackets the salt in the binary, so the harness can find and compare it.
const UNSALTED: &str = "ferryx-qa-build-salt:<none>";

#[used]
static QA_BUILD_SALT: &str = match option_env!("FERRYX_QA_BUILD_SALT") {
    Some(salt) => salt,
    None => UNSALTED,
};

/// The salt compiled into this build, or `None` for an unsalted QA build.
pub fn build_salt() -> Option<&'static str> {
    let salt: &'static str = QA_BUILD_SALT;
    (salt != UNSALTED).then_some(salt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_salt_is_the_compile_time_value() {
        assert_eq!(build_salt(), option_env!("FERRYX_QA_BUILD_SALT"));
        assert_eq!(BUILD_SALT_ENV, "FERRYX_QA_BUILD_SALT");
    }
}
