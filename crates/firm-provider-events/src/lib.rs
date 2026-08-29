//! Persisted provider-native Session readers and projections for AgentFirm.
//!
//! Provider transcripts remain provider-owned. This crate reads complete
//! provider-native rows into response-local v3 records. It never persists a
//! transcript or authors Message, Work, Delivery, Decision, runtime, or
//! provider-effect truth.

mod persisted;
mod persisted_model;
pub use persisted::*;
pub use persisted_model::*;
