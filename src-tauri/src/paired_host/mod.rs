//! Local daemon credential authority. Run filesystem methods on a blocking worker.
pub mod inventory;
pub mod service;
pub mod client;
pub mod projects;

#[cfg(test)]
mod proxy_tests;

#[cfg(test)]
mod inventory_tests;

#[cfg(all(test, unix))]
mod native_ambiguity_tests;
