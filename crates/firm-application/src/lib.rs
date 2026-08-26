//! Provider-aware application composition for Star Harness.
//!
//! Provider packages implement native transports; this crate describes which
//! reviewed binding may serve each current product surface. It owns no provider
//! process and writes no coordination state.

mod current_work_delivery;
mod host_runtime_binding;
mod message_authoring;
mod projection_fold;
mod provider_catalog;
mod provider_outcome;
mod runtime_recovery_action;
mod team_runtime_policy;
mod viewer_context;
mod work_service;

pub use current_work_delivery::*;
pub use host_runtime_binding::*;
pub use message_authoring::*;
pub use projection_fold::*;
pub use provider_catalog::*;
pub use provider_outcome::*;
pub use runtime_recovery_action::*;
pub use team_runtime_policy::*;
pub use viewer_context::*;
pub use work_service::*;
