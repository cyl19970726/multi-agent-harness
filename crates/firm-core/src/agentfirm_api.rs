//! Canonical Member Execution Trust Kernel wire-contract facade.
//!
//! Domain contracts live in ownership-focused modules while this facade keeps
//! the stable `firm_core::agentfirm_api::*` public path source-compatible.

mod identity_session;
pub use identity_session::*;
mod messaging;
pub use messaging::*;
mod runtime_control;
pub use runtime_control::*;
mod work_trust;
pub use work_trust::*;

#[cfg(test)]
mod runtime_control_contract_tests;
