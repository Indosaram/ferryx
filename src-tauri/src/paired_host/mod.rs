//! Local daemon credential authority. Run filesystem methods on a blocking worker.
pub mod attach;
pub mod client;
pub mod direct;
pub mod direct_autotrust;
pub mod direct_client;
pub mod direct_connect;
pub mod direct_route;
pub mod direct_trust;
pub mod direct_wire;
pub mod file_read;
pub mod path_select;
pub mod inventory;
pub mod projects;
pub mod reachability;
pub mod route_view;
pub mod service;
pub mod upload;

#[cfg(test)]
mod proxy_tests;

#[cfg(test)]
mod inventory_tests;

#[cfg(all(test, unix))]
mod native_ambiguity_tests;
