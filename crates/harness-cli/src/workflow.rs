//! Re-export of the provider-agnostic workflow runtime.
//!
//! The runtime was extracted into the `harness-workflow` lib crate so it
//! contains NO provider (codex/claude) code. The binary keeps depending on the
//! same `workflow::*` paths it always used; the real delivery driver
//! (`workflow_real_agent_step` in `main.rs`) is injected through the
//! `AgentStepFn` seam re-exported here.
//!
//! See `crates/harness-workflow/src/lib.rs` for the implementation and tests.

pub use harness_workflow::*;
