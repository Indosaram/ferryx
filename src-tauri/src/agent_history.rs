//! Shim that pulls the provider-history implementation into the crate.
//! The implementation sits outside the crate's module tree and is reachable only through this shim;
//! the sibling test modules are private and exist only under `cfg(test)`.
#[path = "ferryx_scope/history/mod.rs"]
pub mod history;

#[cfg(test)]
#[path = "ferryx_scope/history/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ferryx_scope/history/hardening_tests.rs"]
mod hardening_tests;
