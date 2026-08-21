#![allow(clippy::result_large_err)]

use ed25519_dalek::{Signer, SigningKey};
use firm_fabric::transport::{
    decode_frame, encode_frame, FabricSessionFence, NodeFabricConfig, MAX_FABRIC_FRAME_BYTES,
};
use firm_fabric::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const COMPANY: &str = "company-test";
const SCHEMA_DIGEST: &str = "schema-bundle-v1";
const TOKEN_A: &str = "enrollment-token-node-a-0000000000000001";
const TOKEN_B: &str = "enrollment-token-node-b-0000000000000002";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "agentfirm-fabric-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).expect("create isolated test root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        if let Ok(target) = std::fs::canonicalize(&self.0) {
            assert!(target.starts_with(&temp));
            assert!(target
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("agentfirm-fabric-")));
            std::fs::remove_dir_all(target).expect("remove isolated test root");
        }
    }
}

#[cfg(unix)]
fn secure_private_key(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("restrict private key fixture");
}

fn actor(id: &str, roles: &[&str]) -> AuthenticatedActor {
    AuthenticatedActor {
        company_id: COMPANY.into(),
        actor_id: id.into(),
        actor_kind: ActorKind::Human,
        role_bindings: roles.iter().map(|role| (*role).to_string()).collect(),
        session_id: format!("session-{id}"),
        issued_at_unix_ms: 1,
        expires_at_unix_ms: 1_000_000,
    }
}

fn hello(node: &str, instance: &str, cert: &str, fingerprint: &str) -> NodeHello {
    NodeHello {
        company_id: COMPANY.into(),
        node_id: node.into(),
        instance_id: instance.into(),
        node_daemon_id: format!("node-daemon:{node}"),
        node_daemon_generation: 1,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: SCHEMA_DIGEST.into(),
        features: BTreeSet::from(["durable-routing".into()]),
        build_sha: "build-test".into(),
        last_persisted_route_seq: 0,
        unresolved_operation_ids: BTreeSet::new(),
        certificate_serial: cert.into(),
        public_key_fingerprint: fingerprint.into(),
    }
}

fn signing_key(node: &str) -> SigningKey {
    SigningKey::from_bytes(&match node {
        "node-a" => [1; 32],
        "node-b" => [2; 32],
        _ => [3; 32],
    })
}

fn fingerprint(node: &str) -> String {
    sha256_hex(signing_key(node).verifying_key().to_bytes())
}

fn enrollment_proof(enrollment_id: &str, node: &str, cert: &str) -> EnrollmentProof {
    let challenge = firm_fabric::enrollment::enrollment_challenge(
        COMPANY,
        enrollment_id,
        node,
        cert,
        SCHEMA_DIGEST,
    );
    let key = signing_key(node);
    EnrollmentProof {
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    }
}

fn hello_proof(
    hello: &NodeHello,
    control_plane_generation: u64,
    key: &SigningKey,
) -> NodeHelloProof {
    let challenge =
        firm_fabric::node_gateway::node_hello_challenge(COMPANY, control_plane_generation, hello)
            .expect("hello challenge");
    NodeHelloProof {
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    }
}

fn verified_peer(hello: &NodeHello) -> firm_fabric::transport::VerifiedMtlsPeer {
    firm_fabric::transport::VerifiedMtlsPeer {
        company_id: hello.company_id.clone(),
        node_id: hello.node_id.clone(),
        certificate_serial: hello.certificate_serial.clone(),
        public_key_fingerprint: hello.public_key_fingerprint.clone(),
        tls_version: "TLS1.3".into(),
        websocket_subprotocol: firm_fabric::transport::FABRIC_WEBSOCKET_SUBPROTOCOL.into(),
    }
}

fn connect_node<K: ArtifactKeyBackend>(
    control: &ControlPlane<'_, K>,
    generation: u64,
    hello: &NodeHello,
    key: &SigningKey,
    now_unix_ms: u64,
) -> Result<NodeWelcome, FabricError> {
    control.connect_gateway(
        generation,
        &verified_peer(hello),
        hello,
        &hello_proof(hello, generation, key),
        now_unix_ms,
    )
}

fn enroll_nodes<K: ArtifactKeyBackend>(control: &ControlPlane<'_, K>, generation: u64) {
    let host = actor("host", &["company_host"]);
    for (enrollment, token, node, cert) in [
        ("enroll-a", TOKEN_A, "node-a", "cert-a"),
        ("enroll-b", TOKEN_B, "node-b", "cert-b"),
    ] {
        control
            .create_enrollment(
                &host,
                generation,
                enrollment,
                token,
                node,
                BTreeSet::from(["durable-routing".into(), "artifact-transfer".into()]),
                500_000,
                10,
            )
            .expect("create enrollment");
        control
            .consume_enrollment(
                generation,
                token,
                node,
                node,
                &enrollment_proof(enrollment, node, cert),
                cert,
                900_000,
                SCHEMA_DIGEST,
                20,
            )
            .expect("consume enrollment");
    }
}

fn operation(source_generation: u64, control_generation: u64) -> RoutedOperation {
    let body = json!({"probe": "reachable"});
    RoutedOperation {
        id: "operation-1".into(),
        company_id: COMPANY.into(),
        kind: "fabric.probe.v1".into(),
        source_authority: OperationSourceAuthority::Node,
        source_node_id: Some("node-a".into()),
        target_node_id: "node-b".into(),
        source_gateway_generation: Some(source_generation),
        source_node_daemon_id: Some("node-daemon:node-a".into()),
        source_node_daemon_generation: Some(1),
        control_plane_generation: control_generation,
        source_execution_space_id: Some("space-a".into()),
        target_execution_space_id: Some("space-b".into()),
        actor: actor("fabric-client", &["fabric_submit"]),
        actor_runtime_generation: Some(1),
        authorization_context: BTreeMap::from([("scope".into(), "probe".into())]),
        idempotency_key: "idempotency-1".into(),
        ordering_key: "probe:node-a:node-b".into(),
        correlation_id: "correlation-1".into(),
        causation_id: None,
        expected_target_revision: None,
        body_schema: "agentfirm.remote_fabric.probe.v1".into(),
        body_digest: json_digest(&body).expect("digest body"),
        body,
        priority: OperationPriority::Normal,
        created_at_unix_ms: 100,
        expires_at_unix_ms: 50_000,
        protocol_version: FABRIC_PROTOCOL_VERSION,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: FABRIC_CANONICALIZATION_VERSION.into(),
    }
}

fn fabric_session(
    node_id: &str,
    gateway_generation: u64,
    control_plane_generation: u64,
) -> FabricSessionFence {
    FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: node_id.into(),
        gateway_generation,
        node_daemon_id: format!("node-daemon:{node_id}"),
        node_daemon_generation: 1,
        control_plane_generation,
    }
}

#[allow(clippy::too_many_arguments)]
fn rotate_gateway_certificate(
    control: &ControlPlane<'_, InMemoryArtifactKeyBackend>,
    store: &FabricStore,
    control_generation: u64,
    welcome: &NodeWelcome,
    node_id: &str,
    current_serial: &str,
    next_serial: &str,
    now_unix_ms: u64,
) -> NodeHello {
    let node = store.snapshot().expect("snapshot").nodes[node_id].clone();
    let challenge = firm_fabric::enrollment::certificate_rotation_challenge(
        COMPANY,
        node_id,
        current_serial,
        next_serial,
        node.node_revision,
        SCHEMA_DIGEST,
    );
    let key = signing_key(node_id);
    let proof = EnrollmentProof {
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    };
    let (_, certificate) = control
        .rotate_node_certificate(
            control_generation,
            node_id,
            welcome.gateway_generation,
            &welcome.node_daemon_id,
            welcome.node_daemon_generation,
            current_serial,
            next_serial,
            node.node_revision,
            &proof,
            now_unix_ms + 600_000,
            now_unix_ms,
        )
        .expect("rotate gateway certificate under current NodeDaemon authority");
    let mut hello = hello(
        node_id,
        &format!("gateway-{node_id}-successor"),
        next_serial,
        &certificate.public_key_fingerprint,
    );
    hello.node_daemon_id = certificate.node_daemon_id;
    hello.node_daemon_generation = certificate.node_daemon_generation;
    hello
}

fn accept_fabric_operation<K: ArtifactKeyBackend>(
    control: &ControlPlane<'_, K>,
    control_plane_generation: u64,
    source_gateway_generation: u64,
    operation: RoutedOperation,
    now_unix_ms: u64,
) -> Result<(RoutedOperation, RouteAttempt, RouteReceipt, bool), FabricError> {
    control.accept_operation(
        control_plane_generation,
        &fabric_session(
            "node-a",
            source_gateway_generation,
            control_plane_generation,
        ),
        &actor("fabric-client", &["fabric_submit"]),
        operation,
        now_unix_ms,
    )
}

#[path = "fabric_contract/artifact_digest_scope_encryption_and_one_use_capability_fail_closed.rs"]
mod artifact_digest_scope_encryption_and_one_use_capability_fail_closed;
#[path = "fabric_contract/canonical_json_v1_is_key_order_independent_and_rejects_floats.rs"]
mod canonical_json_v1_is_key_order_independent_and_rejects_floats;
#[path = "fabric_contract/checked_in_valid_schema_fixtures_match_rust_wire_types.rs"]
mod checked_in_valid_schema_fixtures_match_rust_wire_types;
#[path = "fabric_contract/commit_failure_and_torn_final_frame_recover_without_partial_state.rs"]
mod commit_failure_and_torn_final_frame_recover_without_partial_state;
#[path = "fabric_contract/concurrent_one_use_enrollment_has_exactly_one_winner.rs"]
mod concurrent_one_use_enrollment_has_exactly_one_winner;
#[path = "fabric_contract/control_plane_backup_restores_exact_transaction_and_rejects_tamper_or_overwrite.rs"]
mod control_plane_backup_restores_exact_transaction_and_rejects_tamper_or_overwrite;
#[path = "fabric_contract/control_plane_ca_issues_exact_company_execution_node_client_identity.rs"]
mod control_plane_ca_issues_exact_company_execution_node_client_identity;
#[path = "fabric_contract/control_plane_source_is_closed_and_node_daemon_parent_fences_successors.rs"]
mod control_plane_source_is_closed_and_node_daemon_parent_fences_successors;
#[path = "fabric_contract/control_plane_store_is_durably_bound_to_one_company.rs"]
mod control_plane_store_is_durably_bound_to_one_company;
#[path = "fabric_contract/control_plane_successor_immediately_fences_prior_live_gateway_generation.rs"]
mod control_plane_successor_immediately_fences_prior_live_gateway_generation;
#[path = "fabric_contract/diagnostics_derive_connection_and_recovery_truth_without_mutation.rs"]
mod diagnostics_derive_connection_and_recovery_truth_without_mutation;
#[path = "fabric_contract/draining_rejects_new_target_work_but_preserves_inflight_completion.rs"]
mod draining_rejects_new_target_work_but_preserves_inflight_completion;
#[path = "fabric_contract/durable_checkpoint_recovers_stale_or_torn_cache_and_never_hides_journal_tamper.rs"]
mod durable_checkpoint_recovers_stale_or_torn_cache_and_never_hides_journal_tamper;
#[path = "fabric_contract/durable_rate_limit_rejects_new_work_but_preserves_exact_replay.rs"]
mod durable_rate_limit_rejects_new_work_but_preserves_exact_replay;
#[path = "fabric_contract/durable_route_replays_exactly_and_fences_stale_source_generation.rs"]
mod durable_route_replays_exactly_and_fences_stale_source_generation;
#[path = "fabric_contract/enrollment_proof_and_certificate_rotation_are_cryptographic_and_generation_fenced.rs"]
mod enrollment_proof_and_certificate_rotation_are_cryptographic_and_generation_fenced;
#[path = "fabric_contract/enrollment_revocation_is_exact_cas_and_prevents_later_consumption.rs"]
mod enrollment_revocation_is_exact_cas_and_prevents_later_consumption;
#[path = "fabric_contract/expired_offline_operation_cannot_persist_or_cross_native_effect_boundary.rs"]
mod expired_offline_operation_cannot_persist_or_cross_native_effect_boundary;
#[path = "fabric_contract/expired_ordering_tombstone_survives_replay_and_successor_before_valid_next.rs"]
mod expired_ordering_tombstone_survives_replay_and_successor_before_valid_next;
#[path = "fabric_contract/expired_unaccepted_source_outbox_settles_locally_without_route_or_native_effect.rs"]
mod expired_unaccepted_source_outbox_settles_locally_without_route_or_native_effect;
#[path = "fabric_contract/message_route_requires_verified_immutable_payload_not_identity_only.rs"]
mod message_route_requires_verified_immutable_payload_not_identity_only;
#[path = "fabric_contract/node_local_gateway_session_queues_exact_recoverable_operation_and_fences_predecessor.rs"]
mod node_local_gateway_session_queues_exact_recoverable_operation_and_fences_predecessor;
#[path = "fabric_contract/node_local_journal_recovers_lost_ack_without_duplicate_native_effect.rs"]
mod node_local_journal_recovers_lost_ack_without_duplicate_native_effect;
#[path = "fabric_contract/node_local_journal_verifies_committed_wire_digest_across_defaulted_field_upgrade.rs"]
mod node_local_journal_verifies_committed_wire_digest_across_defaulted_field_upgrade;
#[path = "fabric_contract/node_local_store_rejects_foreign_and_stale_sessions_with_zero_delta.rs"]
mod node_local_store_rejects_foreign_and_stale_sessions_with_zero_delta;
#[path = "fabric_contract/offline_queue_capacity_rejects_at_count_and_over_bytes_with_zero_delta.rs"]
mod offline_queue_capacity_rejects_at_count_and_over_bytes_with_zero_delta;
#[path = "fabric_contract/offline_source_outbox_rebinds_only_after_empty_current_generation_reconcile.rs"]
mod offline_source_outbox_rebinds_only_after_empty_current_generation_reconcile;
#[path = "fabric_contract/one_use_enrollment_and_stale_control_plane_have_zero_side_effects.rs"]
mod one_use_enrollment_and_stale_control_plane_have_zero_side_effects;
#[path = "fabric_contract/operation_registry_requires_closed_kind_schema_and_body_scope.rs"]
mod operation_registry_requires_closed_kind_schema_and_body_scope;
#[path = "fabric_contract/outbound_mtls_identity_rejects_missing_and_symlinked_key_material_before_network.rs"]
mod outbound_mtls_identity_rejects_missing_and_symlinked_key_material_before_network;
#[path = "fabric_contract/real_loopback_wss_requires_mtls_hostname_and_frozen_subprotocol.rs"]
mod real_loopback_wss_requires_mtls_hostname_and_frozen_subprotocol;
#[path = "fabric_contract/retry_requires_a_new_target_generation_and_reconcile_never_blind_replays.rs"]
mod retry_requires_a_new_target_generation_and_reconcile_never_blind_replays;
#[path = "fabric_contract/successor_control_plane_routes_immutable_prior_generation_operation.rs"]
mod successor_control_plane_routes_immutable_prior_generation_operation;
#[path = "fabric_contract/successor_reconnect_settles_expired_offline_operation_as_not_applied.rs"]
mod successor_reconnect_settles_expired_offline_operation_as_not_applied;
#[path = "fabric_contract/target_persistence_rejects_unresolved_route_sequence_gaps.rs"]
mod target_persistence_rejects_unresolved_route_sequence_gaps;
#[path = "fabric_contract/target_successor_session_fences_predecessor_before_claim_or_result_side_effects.rs"]
mod target_successor_session_fences_predecessor_before_claim_or_result_side_effects;
#[path = "fabric_contract/three_independent_store_roots_preserve_control_plane_and_node_authority.rs"]
mod three_independent_store_roots_preserve_control_plane_and_node_authority;
#[path = "fabric_contract/two_outbound_gateways_route_one_operation_through_durable_target_apply.rs"]
mod two_outbound_gateways_route_one_operation_through_durable_target_apply;
#[path = "fabric_contract/two_process_style_control_plane_stores_have_one_exclusive_generation_winner.rs"]
mod two_process_style_control_plane_stores_have_one_exclusive_generation_winner;
#[path = "fabric_contract/two_process_style_node_outbox_handles_share_one_atomic_journal.rs"]
mod two_process_style_node_outbox_handles_share_one_atomic_journal;
#[path = "fabric_contract/unknown_application_effect_remains_durable_recovery_required.rs"]
mod unknown_application_effect_remains_durable_recovery_required;
#[path = "fabric_contract/wire_config_and_frame_codec_are_closed_and_generation_fenced.rs"]
mod wire_config_and_frame_codec_are_closed_and_generation_fenced;
