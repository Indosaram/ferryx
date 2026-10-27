//! Supervisor storage tests. `schema` covers migration 0020 and the store's dedupe, revision,
//! rollback and concurrency guarantees.
//! `admission` covers work admission and the initial dispatcher.
//! `receipts` covers causal worker receipts: accepted, started and settled per transport.
//! `wakes` covers dependency waits, the state-event cursor and batched goal wakes (0021).
//! `setup` covers installation set-up ([`super::provision_owner`]).
//! `bound_session` covers an owner-chosen existing pane bound at admission and the read-only
//! dispatch status reader.
mod admission;
mod bound_session;
mod receipts;
mod schema;
mod setup;
mod wakes;
