//! Disposable cloud machines on external providers: the provider seam, the machine catalog and
//! the idle lifecycle that keeps free credit from being burned by a forgotten box.
//!
//! Layout (modules land as their todos do; this module holds only what exists):
//!
//! - [`provider`] - the `SandboxProvider` trait, its request/response types and error kinds, and
//!   the in-memory [`provider::fake::FakeProvider`] every unit test runs against.
//! - [`catalog`] - `~/.ferryx/cloud-hosts.json`: one row per machine until the provider confirms
//!   deletion (tombstones stay).
//! - [`lifecycle`] - create/delete under the catalog's rules, and reconcile: idle stop, hard-max
//!   delete, orphan scan.
//! - [`reaper`] - the 5-minute scheduler entry that runs reconcile without the operator.
//! - [`slots`] - `~/.ferryx/cloud-slots/`: the finite pool of account machine records a T2 box
//!   claims, one lease per record.
//! - [`enroll`] - put a leased record on a fresh box: mint a one-use code, enroll, configure the
//!   box's daemon for the relay, and prove the record is reachable before the create reports
//!   success.
//! - `bootstrap` - the pinned, checksummed mhc install script.
//! - `cli` - `ferryx-cli cloud ...`.
//!
//! Two rules hold across every module here: a status check never wakes a stopped machine, and a
//! secret is never put on a command line, in a log, or in the catalog.

pub mod bootstrap;
pub mod catalog;
pub mod cli;
pub mod enroll;
pub mod lifecycle;
pub mod provider;
pub mod reaper;
pub mod secrets;
pub mod slots;

/// The macOS launchd acceptance run for the reaper: an ignored test that drives the real
/// `launchctl` through the public lifecycle API. Run it on request with
/// `cargo test --lib -- --ignored launchd_acceptance`.
#[cfg(all(test, target_os = "macos"))]
mod launchd_acceptance;

pub use provider::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, SshIdentity, SshTarget, Transport,
};
