//! Supervisor storage tests. `schema` covers migration 0020 and the store's dedupe, revision,
//! rollback and concurrency guarantees.
//! `admission` covers work admission and the initial dispatcher.
//! `receipts` covers causal worker receipts: accepted, started and settled per transport.
mod admission;
mod receipts;
mod schema;
