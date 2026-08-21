use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, CanonicalMessageDelivery,
    CanonicalMessageDeliveryStatus, Message, MessageAddressKind, MessageKind, MessageRecipientKind,
    MessageRecipientRef, MutationContext, PermissionCeiling, ResponseIntent, TeamMembership,
    TeamMembershipRole, TeamMembershipStatus,
};
use firm_core::collaboration::{
    ArtifactImport, CancellationDecisionKind, CancellationRequestState,
    CollaborationRetentionAnchor, DelegationCancellationDecision, DelegationCancellationRequest,
    DelegationDecision, DelegationDecisionKind, DelegationInboundMode, DelegationInboundPolicy,
    DelegationState, DelegationTerminalOutcome, FabricEffectCertainty, FabricError,
    FabricErrorCode, ImmutableMessageTransferPayload, RemoteFactKind, RemoteFactPublication,
    RemoteFactSnapshot, RemoteMessageReplica, RemoteMessageTransferState, RemoteWorkRef,
    RoutedBusinessKind, RoutedBusinessOperation, RoutedBusinessReceipt, SourceWorkAttestation,
    TargetPlacementRef, WorkOperationalDecisionRef,
};
use firm_core::{
    AgentTeam, AgentTeamRun, AgentTeamStatus, ExecutionNode, ExecutionNodeStatus, Mission,
    MissionStatus, NodeProjectRegistration, NodeProjectRegistrationStatus, TeamActorKind,
    TeamActorRef, TeamRunStatus, WorkCommandContext,
};
use firm_fabric::{
    json_digest, ActorKind as FabricActorKind, ArtifactCapability, ArtifactCapabilityPurpose,
    ArtifactClassification, AuthenticatedActor, EffectCertainty,
    FabricErrorCode as TransportFabricErrorCode, ReceiptKind, RemoteArtifactManifest, RouteReceipt,
    COLLABORATION_BUSINESS_OPERATION_KIND,
};
use firm_store::{
    apply_collaboration_target_operation, canonical_json_fingerprint,
    collaboration_receipt_from_fabric, persist_verified_remote_message_replica,
    project_cross_node_deliveries, queue_remote_message_transfer,
    route_collaboration_business_operation, validate_message_collaboration_scope,
    CollaborationApplicationService, CollaborationDelegationFilter, CollaborationFabricPort,
    CollaborationFabricRouteContext, CollaborationFabricSource, CollaborationMutationContext,
    CollaborationRouteClient, HarnessStore, ProposeDelegationRequest,
    RemoteFabricCollaborationPort, RemoteMessageReplicaExpectation, RemoteMessageReplicaPort,
    ResolvedCollaborationAuthority,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const TARGET_NODE_UUID: &str = "22222222-2222-4222-8222-222222222222";
const SOURCE_NODE_UUID: &str = "11111111-1111-4111-8111-111111111111";

struct TestStore {
    root: PathBuf,
    store: HarnessStore,
}

impl TestStore {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agentfirm-wave6-{label}-{}-{id}",
            std::process::id()
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("init collaboration test store");
        Self { root, store }
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn actor(kind: ActorKind, id: &str) -> ActorRef {
    ActorRef {
        kind,
        id: id.into(),
    }
}

fn placement(observed_node: u64) -> TargetPlacementRef {
    TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 7,
        node_id: if observed_node == 14 {
            "node-c".into()
        } else {
            "node-b".into()
        },
        placement_generation: 1,
    }
}

fn work_ref(node: &str, team: &str, work: &str, revision: u64) -> RemoteWorkRef {
    RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: format!("space-{node}"),
        node_id: node.into(),
        team_id: team.into(),
        team_revision: if team == "team-a" { 5 } else { 7 },
        placement_generation: 1,
        work_id: work.into(),
        work_revision: revision,
        work_event_id: format!("event-{work}-{revision}"),
        digest: format!("sha256:{:064x}", revision),
    }
}

fn authority() -> ResolvedCollaborationAuthority {
    ResolvedCollaborationAuthority {
        source_host: actor(ActorKind::AgentMember, "host-a"),
        source_work_owner: actor(ActorKind::AgentMember, "member-a"),
        target_host: actor(ActorKind::AgentMember, "host-b"),
        target_placement: placement(13),
        source_work_application_service: actor(ActorKind::Service, "source-work-service-a"),
        source_gateway_generation: 8,
    }
}

fn policy() -> DelegationInboundPolicy {
    DelegationInboundPolicy {
        id: "policy-a-b".into(),
        company_id: "company-1".into(),
        target_team_id: "team-b".into(),
        source_team_id: "team-a".into(),
        mode: DelegationInboundMode::HostApprovalRequired,
        allowed_outcome_classes: vec!["implementation".into()],
        max_active_delegations: 4,
        created_by_target_host: actor(ActorKind::AgentMember, "host-b"),
        revision: 1,
        created_at: "2026-08-11T00:00:00Z".into(),
        revoked_at: None,
    }
}

fn install_policy(store: &HarnessStore) {
    let policy = policy();
    let host = actor(ActorKind::AgentMember, "host-b");
    store
        .put_collaboration_inbound_policy(
            &context(host.clone(), "delegation.policy.put", "policy-put-1", 0),
            &policy,
            &host,
        )
        .expect("target Host canonical inbound policy");
    let attestation = source_attestation();
    let service = attestation.work_application_service_ref.clone();
    store
        .put_source_work_attestation(
            &context(
                service.clone(),
                "source_work.attest",
                "source-work-attestation-1",
                0,
            ),
            &attestation,
            &service,
            8,
        )
        .expect("source WorkApplicationService canonical attestation");
}

fn seed_target_team(store: &HarnessStore) {
    seed_team(
        store,
        TARGET_NODE_UUID,
        "Node B",
        "space-node-b",
        "project-b",
        "mission-b",
        "team-b",
        "Team B",
        "host-b",
        "run-b",
    );
}

#[allow(clippy::too_many_arguments)]
fn seed_team(
    store: &HarnessStore,
    node_id: &str,
    node_name: &str,
    execution_space_id: &str,
    project_binding_id: &str,
    mission_id: &str,
    team_id: &str,
    team_name: &str,
    host_id: &str,
    run_id: &str,
) {
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: node_name.into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: node_id.into(),
                execution_space_id: execution_space_id.into(),
                project_binding_id: project_binding_id.into(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            execution_space_id,
        )
        .unwrap();
    store
        .append_mission(&Mission {
            id: mission_id.into(),
            title: format!("{team_name} Mission"),
            objective: "Execute delegated Work".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            legacy_wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        })
        .unwrap();
    let team_creator = actor(ActorKind::Human, "fixture-operator");
    store
        .create_trust_agent_member(
            &MutationContext {
                execution_space_id: execution_space_id.into(),
                authenticated_actor: team_creator.clone(),
                authority_actor: None,
                command_name: "agent_member.create".into(),
                idempotency_key: format!("member-create-{host_id}"),
                expected_version: 0,
                request_fingerprint: None,
            },
            AgentMember {
                id: host_id.into(),
                name: host_id.into(),
                description: "cross-machine fixture Host".into(),
                role: "host".into(),
                capabilities: Vec::new(),
                skill_refs: Vec::new(),
                provider_profile_ref: None,
                model_preference: None,
                workspace_policy: "test".into(),
                permission_ceiling: PermissionCeiling::WorkspaceWrite,
                organization_status: AgentMemberOrganizationStatus::Active,
                version: 1,
                created_by: team_creator.clone(),
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
        )
        .unwrap();
    let team = AgentTeam {
        id: team_id.into(),
        name: team_name.into(),
        description: "Target Team".into(),
        legacy_mission_id: Some(mission_id.into()),
        mission_id: mission_id.into(),
        host_agent_id: host_id.into(),
        node_id: node_id.into(),
        status: AgentTeamStatus::Active,
        revision: 1,
        trashed_at: None,
        member_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    store
        .create_agent_team(
            &MutationContext {
                execution_space_id: execution_space_id.into(),
                authenticated_actor: team_creator.clone(),
                authority_actor: None,
                command_name: "agent_team.create".into(),
                idempotency_key: format!("team-create-{team_id}"),
                expected_version: 0,
                request_fingerprint: None,
            },
            team,
            vec![TeamMembership {
                id: format!("membership-{team_id}-{host_id}"),
                team_id: team_id.into(),
                agent_member_id: host_id.into(),
                node_id: node_id.into(),
                role: TeamMembershipRole::Host,
                state: TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: team_creator,
                revision: 1,
                joined_at: "unix-ms:1".into(),
                left_at: None,
            }],
        )
        .unwrap();
    store
        .create_team_run_with_member_runs_from_agent_team(
            &AgentTeamRun {
                id: run_id.into(),
                agent_team_id: team_id.into(),
                execution_node_id: node_id.into(),
                project_binding_id: project_binding_id.into(),
                previous_run_id: None,
                host_surface: "test".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: Default::default(),
                objective: "Execute delegated Work".into(),
                execution_root: None,
                status: TeamRunStatus::Running,
                member_run_ids: Vec::new(),
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            },
            execution_space_id,
            &[],
            &[],
        )
        .unwrap();
}

fn context(
    actor: ActorRef,
    command: &str,
    key: &str,
    expected: u64,
) -> CollaborationMutationContext {
    CollaborationMutationContext {
        company_id: "company-1".into(),
        authenticated_actor: actor,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_revision: expected,
        occurred_at: format!("2026-08-11T00:00:{expected:02}Z"),
    }
}

fn proposal() -> ProposeDelegationRequest {
    ProposeDelegationRequest {
        delegation_id: "delegation-1".into(),
        source_work_attestation_id: "source-work-attestation-1".into(),
        target_placement: placement(13),
        requested_outcome: "Implement the remote component".into(),
        outcome_class: "implementation".into(),
        acceptance_contract: "checks and evidence are required".into(),
        operation_id: "route-propose-1".into(),
    }
}

fn source_attestation() -> SourceWorkAttestation {
    let mut attestation = SourceWorkAttestation {
        id: "source-work-attestation-1".into(),
        company_id: "company-1".into(),
        source_work_ref: work_ref("node-a", "team-a", "work-a", 9),
        source_owner_ref: actor(ActorKind::AgentMember, "member-a"),
        source_host_ref: actor(ActorKind::AgentMember, "host-a"),
        work_application_service_ref: actor(ActorKind::Service, "source-work-service-a"),
        source_gateway_generation: 8,
        attestation_digest: String::new(),
        issued_at: "2026-08-11T00:00:00Z".into(),
    };
    attestation.attestation_digest = canonical_json_fingerprint(&serde_json::json!({
        "id": attestation.id,
        "company_id": attestation.company_id,
        "source_work_ref": attestation.source_work_ref,
        "source_owner_ref": attestation.source_owner_ref,
        "source_host_ref": attestation.source_host_ref,
        "work_application_service_ref": attestation.work_application_service_ref,
        "source_gateway_generation": attestation.source_gateway_generation,
        "issued_at": attestation.issued_at,
    }));
    attestation
}

#[derive(Default)]
struct FaithfulFabric {
    effects: Mutex<BTreeMap<String, (String, RoutedBusinessReceipt)>>,
}

impl FaithfulFabric {
    fn effect_count(&self) -> usize {
        self.effects.lock().unwrap().len()
    }
}

impl CollaborationFabricPort for FaithfulFabric {
    fn dispatch(
        &self,
        operation: &RoutedBusinessOperation,
    ) -> Result<RoutedBusinessReceipt, FabricError> {
        let mut effects = self.effects.lock().unwrap();
        if let Some((fingerprint, receipt)) = effects.get(&operation.id) {
            if fingerprint != &operation.payload_digest {
                return Err(FabricError {
                    code: FabricErrorCode::IdempotencyConflict,
                    message: "operation id was reused with a different payload".into(),
                    retryable: false,
                    effect_certainty: FabricEffectCertainty::None,
                    resource_kind: "routed_operation".into(),
                    resource_id: operation.id.clone(),
                    current_revision: Some(operation.expected_revision),
                });
            }
            return Ok(receipt.clone());
        }
        let target = work_ref("node-b", "team-b", "work-b", 1);
        let result = serde_json::json!({"target_work_ref": target});
        let receipt = RoutedBusinessReceipt {
            operation_id: operation.id.clone(),
            kind: operation.kind,
            target_node_id: operation.target_placement.node_id.clone(),
            target_placement_generation: operation.target_placement.placement_generation,
            effect_certainty: FabricEffectCertainty::Applied,
            result_digest: canonical_json_fingerprint(&result),
            result,
            applied_at: "2026-08-11T00:00:02Z".into(),
        };
        effects.insert(
            operation.id.clone(),
            (operation.payload_digest.clone(), receipt.clone()),
        );
        Ok(receipt)
    }
}

struct UnknownFabric;

impl CollaborationFabricPort for UnknownFabric {
    fn dispatch(
        &self,
        operation: &RoutedBusinessOperation,
    ) -> Result<RoutedBusinessReceipt, FabricError> {
        Err(FabricError {
            code: FabricErrorCode::RecoveryRequired,
            message: "transport lost after dispatch".into(),
            retryable: false,
            effect_certainty: FabricEffectCertainty::Unknown,
            resource_kind: "routed_operation".into(),
            resource_id: operation.id.clone(),
            current_revision: Some(operation.expected_revision),
        })
    }
}

#[derive(Default)]
struct TerminalRouteClient {
    effects: Mutex<BTreeMap<String, RouteReceipt>>,
}

impl CollaborationRouteClient for TerminalRouteClient {
    fn route_and_reconcile(
        &self,
        operation: firm_fabric::RoutedOperation,
    ) -> Result<RouteReceipt, firm_fabric::FabricError> {
        let mut effects = self.effects.lock().unwrap();
        if let Some(receipt) = effects.get(&operation.id) {
            return Ok(receipt.clone());
        }
        let result = serde_json::json!({
            "target_work_ref": work_ref("node-b", "team-b", "work-b", 1),
        });
        let receipt = RouteReceipt {
            id: format!("receipt:{}", operation.id),
            company_id: operation.company_id.clone(),
            operation_id: operation.id.clone(),
            target_node_id: operation.target_node_id,
            target_gateway_generation: 9,
            control_plane_generation: operation.control_plane_generation,
            route_seq: 1,
            kind: ReceiptKind::OperationApplied,
            application_effect: Some(EffectCertainty::Applied),
            result_schema: Some("agentfirm.collaboration.target_work.v1".into()),
            result_digest: Some(json_digest(&result).unwrap()),
            result: Some(result),
            error: None,
            created_at_unix_ms: 200,
            schema_version: "agentfirm.remote_fabric.v1".into(),
        };
        effects.insert(operation.id, receipt.clone());
        Ok(receipt)
    }
}

fn active_delegation(store: &HarnessStore) -> RemoteWorkRef {
    install_policy(store);
    let auth = authority();
    let request = proposal();
    let proposed = store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "propose-1",
                0,
            ),
            &request,
            &auth,
            &policy(),
        )
        .expect("propose");
    assert_eq!(
        proposed.projection.state,
        DelegationState::AwaitingTargetDecision
    );

    let decision = DelegationDecision {
        id: "decision-accept-1".into(),
        delegation_id: request.delegation_id.clone(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        reason: "capacity available".into(),
        created_at: "2026-08-11T00:00:01Z".into(),
    };
    let accepted = store
        .decide_collaboration_delegation(
            &context(auth.target_host.clone(), "delegation.decide", "accept-1", 1),
            &request.delegation_id,
            &decision,
            &auth,
            &placement(13),
        )
        .expect("target Host accept");
    assert_eq!(
        accepted.projection.state,
        DelegationState::ProvisioningTargetWork
    );

    let route = store
        .target_work_create_operation("company-1", &request.delegation_id, "2026-08-11T00:00:02Z")
        .expect("build target Work routed operation");
    assert_eq!(route.ordering_key, "delegation:delegation-1");

    let target = work_ref("node-b", "team-b", "work-b", 1);
    let control_plane = actor(ActorKind::Service, "fabric-control-plane");
    let active = store
        .apply_target_work_created(
            &context(
                control_plane.clone(),
                "target_work.applied",
                "target-applied-1",
                2,
            ),
            &request.delegation_id,
            &target,
            &placement(13),
            &route.id,
            &control_plane,
        )
        .expect("fold applied target Work result");
    assert_eq!(active.projection.state, DelegationState::Active);
    assert_eq!(
        active.projection.source_work_ref,
        source_attestation().source_work_ref
    );
    assert_eq!(active.projection.target_work_ref.as_ref(), Some(&target));
    target
}

fn canonical_delivery(
    id: &str,
    recipient: &str,
    status: CanonicalMessageDeliveryStatus,
) -> CanonicalMessageDelivery {
    CanonicalMessageDelivery {
        id: id.into(),
        message_id: "message-1".into(),
        subscription_id: format!("subscription-{recipient}"),
        subscription_revision: 1,
        subscription_policy_digest: "sha256:policy".into(),
        recipient_kind: firm_core::agentfirm_api::MessageSubjectKind::AgentMember,
        recipient_ref: recipient.into(),
        target_team_id: None,
        target_node_id: "node-b".into(),
        resolved_team_membership_id: None,
        recipient_agent_member_id: Some(recipient.into()),
        recipient_session_id: Some(format!("session-{recipient}")),
        recipient_session_generation: Some(4),
        status,
        attempt: 1,
        claim_id: None,
        claimed_node_daemon_generation: None,
        provider_receipt_id: None,
        failure_code: None,
        failure_detail: None,
        version: 1,
        created_at: "2026-08-11T00:00:00Z".into(),
        updated_at: "2026-08-11T00:00:00Z".into(),
    }
}

fn message_fingerprint(message: &Message) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": message.sender_actor_ref,
        "sender_agent_member_id": message.sender_agent_member_id,
        "sender_session_id": message.sender_session_id,
        "address_kind": message.address_kind,
        "target_ref": message.target_ref,
        "recipients": message.recipients,
        "team_id": message.team_id,
        "team_run_id": message.team_run_id,
        "work_id": message.work_id,
        "collaboration_scope": message.collaboration_scope,
        "kind": message.kind,
        "body": message.body,
        "body_digest": message.body_digest,
        "correlation_id": message.correlation_id,
        "causation_id": message.causation_id,
        "response_intent": message.response_intent,
        "evidence_refs": message.evidence_refs,
        "schema_version": message.schema_version,
        "idempotency_key": message.idempotency_key,
    }))
}

fn persisted_replica(message: &Message) -> firm_core::collaboration::RemoteMessageReplica {
    firm_core::collaboration::RemoteMessageReplica {
        source_execution_space_id: message.source_execution_space_id.clone(),
        message_id: message.id.clone(),
        schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint.clone(),
        body_digest: message.body_digest.clone(),
        canonical_message_bytes: serde_json::to_vec(message).unwrap(),
        persisted_at: "2026-08-11T00:00:01Z".into(),
    }
}

#[derive(Default)]
struct FaithfulReplicaStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    replicas: Mutex<BTreeMap<(String, String), RemoteMessageReplica>>,
}

impl RemoteMessageReplicaPort for FaithfulReplicaStore {
    fn fetch_message_object(&self, message_object_ref: &str) -> Result<Vec<u8>, FabricError> {
        self.objects
            .lock()
            .unwrap()
            .get(message_object_ref)
            .cloned()
            .ok_or_else(|| FabricError {
                code: FabricErrorCode::MessageReplicaMismatch,
                message: "message object unavailable".into(),
                retryable: false,
                effect_certainty: FabricEffectCertainty::None,
                resource_kind: "message_object_ref".into(),
                resource_id: message_object_ref.into(),
                current_revision: None,
            })
    }

    fn persist_remote_replica(
        &self,
        replica: &RemoteMessageReplica,
    ) -> Result<RemoteMessageReplica, FabricError> {
        let key = (
            replica.source_execution_space_id.clone(),
            replica.message_id.clone(),
        );
        let mut replicas = self.replicas.lock().unwrap();
        if let Some(current) = replicas.get(&key) {
            if current.content_fingerprint == replica.content_fingerprint
                && current.body_digest == replica.body_digest
                && current.canonical_message_bytes == replica.canonical_message_bytes
            {
                return Ok(current.clone());
            }
            return Err(FabricError {
                code: FabricErrorCode::MessageReplicaMismatch,
                message: "same remote Message identity was reused with different bytes".into(),
                retryable: false,
                effect_certainty: FabricEffectCertainty::None,
                resource_kind: "remote_message_replica".into(),
                resource_id: replica.message_id.clone(),
                current_revision: None,
            });
        }
        replicas.insert(key, replica.clone());
        Ok(replica.clone())
    }
}

#[path = "cross_machine_collaboration/accept_cancel_before_accept_race_has_one_linearized_winner.rs"]
mod accept_cancel_before_accept_race_has_one_linearized_winner;
#[path = "cross_machine_collaboration/active_cancellation_is_only_a_source_request_and_target_host_decision.rs"]
mod active_cancellation_is_only_a_source_request_and_target_host_decision;
#[path = "cross_machine_collaboration/actor_scoped_cursor_bounds_hidden_scans_and_freezes_visible_snapshot.rs"]
mod actor_scoped_cursor_bounds_hidden_scans_and_freezes_visible_snapshot;
#[path = "cross_machine_collaboration/actor_scoped_cursor_skips_hidden_rows_and_rejects_scope_reuse.rs"]
mod actor_scoped_cursor_skips_hidden_rows_and_rejects_scope_reuse;
#[path = "cross_machine_collaboration/all_frozen_business_kinds_use_the_wave5_route_and_terminal_receipt_contract.rs"]
mod all_frozen_business_kinds_use_the_wave5_route_and_terminal_receipt_contract;
#[path = "cross_machine_collaboration/collaboration_authority_fence_holds_writer_lock_through_route_commit.rs"]
mod collaboration_authority_fence_holds_writer_lock_through_route_commit;
#[path = "cross_machine_collaboration/complete_artifact_grant_is_delegation_scoped_and_targets_exact_source_host_node.rs"]
mod complete_artifact_grant_is_delegation_scoped_and_targets_exact_source_host_node;
#[path = "cross_machine_collaboration/concurrent_exact_propose_replay_commits_one_relationship.rs"]
mod concurrent_exact_propose_replay_commits_one_relationship;
#[path = "cross_machine_collaboration/control_plane_folds_exact_artifact_import_without_copying_source_bytes.rs"]
mod control_plane_folds_exact_artifact_import_without_copying_source_bytes;
#[path = "cross_machine_collaboration/delegation_list_cursor_freezes_snapshot_and_filters_exact_scope.rs"]
mod delegation_list_cursor_freezes_snapshot_and_filters_exact_scope;
#[path = "cross_machine_collaboration/delegation_proposal_routes_source_attestation_and_target_resolves_host.rs"]
mod delegation_proposal_routes_source_attestation_and_target_resolves_host;
#[path = "cross_machine_collaboration/delegation_relationship_is_idempotent_placement_fenced_and_source_independent.rs"]
mod delegation_relationship_is_idempotent_placement_fenced_and_source_independent;
#[path = "cross_machine_collaboration/faithful_fabric_replays_exact_effect_and_unknown_never_folds_business_truth.rs"]
mod faithful_fabric_replays_exact_effect_and_unknown_never_folds_business_truth;
#[path = "cross_machine_collaboration/immutable_message_transfer_persists_exact_replica_before_delivery_and_replays.rs"]
mod immutable_message_transfer_persists_exact_replica_before_delivery_and_replays;
#[path = "cross_machine_collaboration/message_projection_preserves_per_recipient_partial_delivery_truth.rs"]
mod message_projection_preserves_per_recipient_partial_delivery_truth;
#[path = "cross_machine_collaboration/remote_fact_is_redacted_digest_bound_and_target_scoped.rs"]
mod remote_fact_is_redacted_digest_bound_and_target_scoped;
#[path = "cross_machine_collaboration/source_artifact_import_is_digest_bound_durable_and_exactly_replayed.rs"]
mod source_artifact_import_is_digest_bound_durable_and_exactly_replayed;
#[path = "cross_machine_collaboration/source_artifact_import_uses_frozen_authority_without_copying_central_delegation.rs"]
mod source_artifact_import_uses_frozen_authority_without_copying_central_delegation;
#[path = "cross_machine_collaboration/source_work_attestation_and_placement_v1_fail_closed.rs"]
mod source_work_attestation_and_placement_v1_fail_closed;
#[path = "cross_machine_collaboration/target_host_decision_routes_under_control_plane_and_validates_local_team.rs"]
mod target_host_decision_routes_under_control_plane_and_validates_local_team;
#[path = "cross_machine_collaboration/target_work_create_applies_once_through_native_work_authority.rs"]
mod target_work_create_applies_once_through_native_work_authority;
#[path = "cross_machine_collaboration/torn_tail_is_ignored_and_exact_replay_repairs_atomic_ledger.rs"]
mod torn_tail_is_ignored_and_exact_replay_repairs_atomic_ledger;
