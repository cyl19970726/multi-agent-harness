//! Closed provider-native event semantics for AgentFirm.
//!
//! Provider transcripts remain provider-owned. This crate accepts one native
//! row at a time and returns bounded, privacy-safe observations that can be
//! folded into server-owned read models. It never authors Message, Work,
//! Delivery, Decision, or provider-effect truth.

mod access;
mod decoder;
mod fold;
mod model;
mod store;

pub use access::*;
pub use decoder::{
    adapter_manifest, decode_native_event, decode_native_json_line, AdapterFidelity,
    AdapterManifest, DecodeContext, DecodeOutcome, NativeEvent,
};
pub use fold::{FoldOutcome, ProviderEventFold, ProviderEventFoldError, SessionEpisode};
pub use model::*;
pub use store::*;
