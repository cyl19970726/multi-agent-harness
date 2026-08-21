//! Temporary source-compatibility projection for the extracted runtime contract.
//!
//! New owners import `firm-runtime-contract` directly. This binary-private
//! re-export keeps the provider migration slices behavior-preserving until the
//! last `firm-cli` implementation moves to its owning package.

pub(crate) use harness_runtime_contract::*;
