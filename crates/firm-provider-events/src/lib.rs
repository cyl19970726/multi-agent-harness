//! Closed provider-native event semantics for AgentFirm.
//!
//! Provider transcripts remain provider-owned. This crate accepts one native
//! row at a time and returns exact native observations for disposable,
//! response-local read models. It never persists a transcript or authors
//! Message, Work, Delivery, Decision, or provider-effect truth.

mod access;
mod decoder;
mod fold;
mod model;
mod persisted;
mod persisted_model;
mod reader;
mod service;

pub use access::*;
pub use decoder::{
    adapter_manifest, decode_native_event, decode_native_json_line, AdapterFidelity,
    AdapterManifest, DecodeContext, DecodeError, DecodeOutcome, NativeEvent,
};
pub use fold::{FoldOutcome, ProviderEventFold, ProviderEventFoldError, SessionEpisode};
pub use model::*;
pub use persisted::*;
pub use persisted_model::*;
pub use reader::*;
pub use service::*;
