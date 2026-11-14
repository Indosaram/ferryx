pub mod backend;
pub mod mhc_fault;
pub mod mhc_profile;
pub mod mhc_stdio;
pub mod mhc_supervisor;
// `workspace_root_from` is a plain path helper with no Slack/Discord dependency, and
// `hub_runtime::seed` calls it in every build. Gating the module on the connector features
// made `--no-default-features` (the headless CLI build) fail with E0433.
pub mod omo_client;
pub mod pane;
pub mod protocol;
pub mod supervisor;
#[cfg(all(test, unix, feature = "hub-slack"))]
pub(crate) mod rpc_mock;
