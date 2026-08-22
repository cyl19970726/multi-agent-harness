//! Provider-aware application composition for Star Harness.
//!
//! Provider packages implement native transports; this crate describes which
//! reviewed binding may serve each current product surface. It owns no provider
//! process and writes no coordination state.

mod provider_catalog;
mod team_runtime_policy;
mod work_service;

pub use provider_catalog::*;
pub use team_runtime_policy::*;
pub use work_service::*;
