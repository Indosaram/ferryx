//! Local daemon credential authority. Run filesystem methods on a blocking worker.
pub mod attach;
pub mod client;
pub mod inventory;
pub mod projects;
pub mod service;

#[cfg(test)]
mod proxy_tests;

#[cfg(test)]
mod inventory_tests;

#[cfg(all(test, unix))]
mod native_ambiguity_tests;
