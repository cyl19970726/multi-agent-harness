//! Process entrypoints for the Company Control Plane and outbound NodeGateway.

#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_fabric::gateway_runtime::{
    now_unix_ms, serve_control_plane_session_with_application, ControlPlaneReceiptApplication,
    NodeApplication, NodeGatewayConnection, ProbeApplication,
};
use harness_fabric::transport::{
    accept_control_plane_mtls, ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles,
    NodeTlsIdentityMaterial,
};
use harness_fabric::{
    ArtifactClassification, AuthenticatedActor, ControlPlane, EffectCertainty, FabricError,
    FabricErrorCode, InMemoryArtifactKeyBackend, NodeAdministrativeStatus, NodeHello, ReceiptKind,
    RouteReceipt, RoutedOperation, TargetApplicationResult, COLLABORATION_BUSINESS_OPERATION_KIND,
    FABRIC_PROTOCOL_VERSION,
};
use harness_store::remote_fabric_store::RemoteFabricStoreLayout;
use harness_store::HarnessStore;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{CliError, CliResult, ResolvedStore};

#[path = "fabric_runtime/control_plane.rs"]
mod control_plane;
use control_plane::*;
#[path = "fabric_runtime/queue_authority.rs"]
mod queue_authority;
use queue_authority::*;
#[path = "fabric_runtime/collaboration_outbound.rs"]
mod collaboration_outbound;
#[cfg(test)]
use collaboration_outbound::source_work_attestation_id;
#[path = "fabric_runtime/cli_routing.rs"]
mod cli_routing;
use cli_routing::*;
#[path = "fabric_runtime/node_application.rs"]
mod node_application;
use node_application::*;
#[path = "fabric_runtime/http_transport.rs"]
mod http_transport;
use http_transport::*;
#[path = "fabric_runtime/artifact_control.rs"]
mod artifact_control;
use artifact_control::*;
#[path = "fabric_runtime/control_plane_http.rs"]
mod control_plane_http;
use control_plane_http::*;
#[path = "fabric_runtime/host_http_routes.rs"]
mod host_http_routes;
use host_http_routes::*;
#[path = "fabric_runtime/credentials.rs"]
mod credentials;
use credentials::*;

pub(crate) use artifact_control::{decode_collaboration_cursor, encode_collaboration_cursor};
pub(crate) use collaboration_outbound::{
    queue_collaboration_message, queue_collaboration_proposal, queue_remote_fact_publication,
};
pub(crate) use credentials::firm_home;
pub(crate) use queue_authority::{
    fabric_command, resolve_collaboration_message_authority, QueueCollaborationMessageRequest,
    QueueCollaborationProposalRequest, QueueRemoteFactPublicationRequest,
};

#[cfg(test)]
#[path = "fabric_runtime_tests.rs"]
mod tests;
