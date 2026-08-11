//! AgentFirm Remote Node Fabric foundation.
//!
//! This crate deliberately owns transport trust, durable routing, receipts,
//! reconciliation and artifacts only. It does not mutate Team, Work, Message
//! or provider runtime business state.

// FabricError is a stable, structured wire contract. Keeping its typed
// reconciliation and revision fields inline is preferable to leaking boxed
// implementation details through every public boundary.
#![allow(clippy::result_large_err)]

pub mod artifacts;
pub mod control_plane;
pub mod diagnostics;
pub mod enrollment;
pub mod local_store;
pub mod node_gateway;
pub mod protocol;
pub mod reconcile;
pub mod router;
pub mod store;
pub mod transport;

pub use artifacts::{ArtifactKeyBackend, InMemoryArtifactKeyBackend};
pub use control_plane::ControlPlane;
pub use diagnostics::{inspect_fabric, FabricDiagnostics, NodeFabricDiagnostics};
pub use local_store::{LocalApplicationResult, NodeLocalFabricState, NodeLocalFabricStore};
pub use protocol::*;
pub use store::{FabricStore, FabricStoreLimits};

use sha2::{Digest, Sha256};

pub const FABRIC_PROTOCOL_VERSION: u32 = 1;
pub const FABRIC_SCHEMA_VERSION: &str = "agentfirm.remote_fabric.v1";

pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    bytes_to_hex(&Sha256::digest(bytes.as_ref()))
}

pub fn json_digest<T: serde::Serialize>(value: &T) -> Result<String, FabricError> {
    canonical_digest(value)
}

pub(crate) fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("write to String");
    }
    encoded
}

pub(crate) fn canonical_digest<T: serde::Serialize>(value: &T) -> Result<String, FabricError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("canonical JSON serialization failed: {error}"),
        )
    })?;
    Ok(sha256_hex(bytes))
}
