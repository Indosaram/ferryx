pub mod billing;
pub mod deletion;
pub mod enroll_client;
pub mod install_keys;
pub mod machine_requests;
pub mod mailer;
pub mod offer_sink;
pub mod origin;
pub mod peer_record;
pub mod service;
pub mod store;
pub mod supervisor_lease;

#[cfg(test)]
mod onboarding_tests;
