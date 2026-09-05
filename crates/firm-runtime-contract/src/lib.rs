//! Provider-neutral operational contract for persistent coding-agent runtimes.
//!
//! Durable identity, capability, command-fence, and continuation shapes live
//! in `firm-core`. This crate adds only process-local runtime operations,
//! admission/fence validation, lifecycle receipts, and conformance contracts.

#![allow(dead_code)]

mod collaboration_capability;
mod conformance;
mod control;
mod cycle;
mod cycle_assertions;
mod provider_capabilities;
mod receipt_and_terminal;

pub use collaboration_capability::*;
pub use conformance::*;
pub use control::*;
pub use cycle::*;
pub use cycle_assertions::*;
pub use provider_capabilities::*;
pub use receipt_and_terminal::*;

#[cfg(test)]
mod cycle_s1_tests;
#[cfg(test)]
mod tests;
