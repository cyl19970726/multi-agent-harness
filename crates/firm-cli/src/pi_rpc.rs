//! Pi persistent runtime composition facade.
//!
//! The RPC transport/session owner and the provider-neutral Team runtime
//! binding are separate modules; this facade preserves existing crate paths.

mod client;
pub(crate) use client::{PiRpcClient, PiSpawnOptions};
mod team_runtime;
pub(crate) use team_runtime::PiTeamRuntime;
