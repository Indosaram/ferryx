/// Property-test case count: `PROPTEST_CASES` overrides the per-test default.
pub fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
