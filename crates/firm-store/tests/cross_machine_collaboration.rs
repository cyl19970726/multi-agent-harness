use firm_core::agentfirm_api::{
    ActorKind, ActorRef, CanonicalMessageDelivery, CanonicalMessageDeliveryStatus, Message,
    MessageAddressKind, MessageKind, MessageRecipientKind, MessageRecipientRef, ResponseIntent,
};
use firm_core::collaboration::{
    CancellationDecisionKind, CancellationRequestState, CollaborationRetentionAnchor,
    DelegationCancellationDecision, DelegationCancellationRequest, DelegationDecision,
    DelegationDecisionKind, DelegationInboundMode, DelegationInboundPolicy, DelegationState,
    DelegationTerminalOutcome, FabricEffectCertainty, FabricError, FabricErrorCode,
    ImmutableMessageTransferPayload, RemoteFactKind, RemoteFactPublication, RemoteFactSnapshot,
    RemoteMessageReplica, RemoteMessageTransferState, RemoteWorkRef, RoutedBusinessKind,
    RoutedBusinessOperation, RoutedBusinessReceipt, SourceWorkAttestation, TargetPlacementRef,
    WorkOperationalDecisionRef,
};
use firm_core::{
    AgentTeam, AgentTeamRun, AgentTeamStatus, ExecutionNode, ExecutionNodeStatus, Mission,
    MissionStatus, NodeProjectRegistration, NodeProjectRegistrationStatus, TeamRunStatus,
};
use firm_fabric::{
    json_digest, ActorKind as FabricActorKind, AuthenticatedActor, EffectCertainty, ReceiptKind,
    RouteReceipt, COLLABORATION_BUSINESS_OPERATION_KIND,
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
    store
        .insert_execution_node(&ExecutionNode {
            id: TARGET_NODE_UUID.into(),
            display_name: "Node B".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: TARGET_NODE_UUID.into(),
                execution_space_id: "space-node-b".into(),
                project_binding_id: "project-b".into(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            "space-node-b",
        )
        .unwrap();
    store
        .insert_mission(&Mission {
            id: "mission-b".into(),
            title: "Mission B".into(),
            objective: "Execute delegated Work".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        })
        .unwrap();
    store
        .insert_agent_team_with_unique_mission(&AgentTeam {
            id: "team-b".into(),
            name: "Team B".into(),
            description: "Target Team".into(),
            mission_id: "mission-b".into(),
            host_agent_id: "host-b".into(),
            node_id: TARGET_NODE_UUID.into(),
            status: AgentTeamStatus::Active,
            member_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();
    store
        .create_team_run_from_agent_team(
            &AgentTeamRun {
                id: "run-b".into(),
                agent_team_id: "team-b".into(),
                execution_node_id: TARGET_NODE_UUID.into(),
                project_binding_id: "project-b".into(),
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
            "space-node-b",
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

#[test]
fn faithful_fabric_replays_exact_effect_and_unknown_never_folds_business_truth() {
    let test = TestStore::new("faithful-fabric");
    install_policy(&test.store);
    let auth = authority();
    test.store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "fabric-propose",
                0,
            ),
            &proposal(),
            &auth,
            &policy(),
        )
        .expect("fabric proposal");
    let decision = DelegationDecision {
        id: "fabric-accept".into(),
        delegation_id: "delegation-1".into(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        reason: "accepted".into(),
        created_at: "2026-08-11T00:00:01Z".into(),
    };
    test.store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "fabric-accept",
                1,
            ),
            "delegation-1",
            &decision,
            &auth,
            &placement(13),
        )
        .expect("fabric accepted");

    let route = test
        .store
        .target_work_create_operation("company-1", "delegation-1", "2026-08-11T00:00:02Z")
        .unwrap();
    assert_eq!(route.authenticated_actor, auth.target_host);
    let route_client = TerminalRouteClient::default();
    let remote_port = RemoteFabricCollaborationPort::new(
        &route_client,
        CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-a".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "session-host-b".into(),
                issued_at_unix_ms: 1,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-b"),
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
        "2026-08-11T00:00:03Z",
    );
    let remote_first = remote_port
        .dispatch(&route)
        .expect("real Wave5 route adapter");
    let remote_replay = remote_port.dispatch(&route).expect("exact route replay");
    assert_eq!(remote_first, remote_replay);
    assert_eq!(route_client.effects.lock().unwrap().len(), 1);
    let fabric = FaithfulFabric::default();
    let first_receipt = fabric.dispatch(&route).unwrap();
    let replay_receipt = fabric.dispatch(&route).unwrap();
    assert_eq!(first_receipt, replay_receipt);
    assert_eq!(fabric.effect_count(), 1);

    let control_plane = actor(ActorKind::Service, "fabric-control-plane");
    let before_hostile_fold = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .apply_target_work_created(
            &context(
                actor(ActorKind::Service, "forged-service"),
                "target_work.applied",
                "forged-fold-1",
                2,
            ),
            "delegation-1",
            &work_ref("node-b", "team-b", "work-b", 1),
            &placement(13),
            &route.id,
            &control_plane,
        )
        .is_err());
    assert_eq!(
        test.store.collaboration_operations().unwrap(),
        before_hostile_fold
    );
    let service = CollaborationApplicationService::new(&test.store, &fabric, &control_plane);
    let applied = service
        .provision_target_work(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "fabric-fold-1",
                2,
            ),
            "delegation-1",
            &placement(13),
        )
        .expect("fold faithful applied receipt");
    assert_eq!(applied.projection.state, DelegationState::Active);
    assert_eq!(fabric.effect_count(), 1);

    let second = TestStore::new("unknown-fabric");
    install_policy(&second.store);
    second
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "unknown-propose",
                0,
            ),
            &proposal(),
            &auth,
            &policy(),
        )
        .unwrap();
    second
        .store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "unknown-accept",
                1,
            ),
            "delegation-1",
            &decision,
            &auth,
            &placement(13),
        )
        .unwrap();
    let before = second.store.collaboration_operations().unwrap();
    let unknown_fabric = UnknownFabric;
    let unknown =
        CollaborationApplicationService::new(&second.store, &unknown_fabric, &control_plane);
    assert!(unknown
        .provision_target_work(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "unknown-fold-1",
                2,
            ),
            "delegation-1",
            &placement(13),
        )
        .is_err());
    assert_eq!(second.store.collaboration_operations().unwrap(), before);
    assert_eq!(
        second.store.collaboration_delegations("company-1").unwrap()[0].state,
        DelegationState::ProvisioningTargetWork
    );
}

#[test]
fn delegation_relationship_is_idempotent_placement_fenced_and_source_independent() {
    let test = TestStore::new("delegation");
    install_policy(&test.store);
    let auth = authority();
    let request = proposal();
    let propose_context = context(
        auth.source_host.clone(),
        "delegation.propose",
        "propose-1",
        0,
    );

    let first = test
        .store
        .propose_collaboration_delegation(&propose_context, &request, &auth, &policy())
        .expect("first proposal");
    let replay = test
        .store
        .propose_collaboration_delegation(&propose_context, &request, &auth, &policy())
        .expect("exact proposal replay");
    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(test.store.collaboration_operations().unwrap().len(), 3);

    let hostile_context = context(
        actor(ActorKind::AgentMember, "sibling-member"),
        "delegation.decide",
        "hostile-decide",
        1,
    );
    let decision = DelegationDecision {
        id: "decision-1".into(),
        delegation_id: request.delegation_id.clone(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: actor(ActorKind::AgentMember, "sibling-member"),
        reason: "spoof".into(),
        created_at: "2026-08-11T00:00:01Z".into(),
    };
    let before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .decide_collaboration_delegation(
            &hostile_context,
            &request.delegation_id,
            &decision,
            &auth,
            &placement(13),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let proper_decision = DelegationDecision {
        decided_by_target_host: auth.target_host.clone(),
        ..decision
    };
    let stale_before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "stale-placement",
                1
            ),
            &request.delegation_id,
            &proper_decision,
            &auth,
            &placement(14),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), stale_before);
}

#[test]
fn concurrent_exact_propose_replay_commits_one_relationship() {
    let test = TestStore::new("concurrent-propose");
    install_policy(&test.store);
    let store = Arc::new(test.store.clone());
    let barrier = Arc::new(Barrier::new(8));
    let mut threads = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let auth = authority();
            barrier.wait();
            store.propose_collaboration_delegation(
                &context(
                    auth.source_host.clone(),
                    "delegation.propose",
                    "concurrent-propose-1",
                    0,
                ),
                &proposal(),
                &auth,
                &policy(),
            )
        }));
    }
    let results = threads
        .into_iter()
        .map(|thread| {
            thread
                .join()
                .expect("proposal thread")
                .expect("exact replay")
        })
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| !result.replayed).count(), 1);
    assert_eq!(results.iter().filter(|result| result.replayed).count(), 7);
    assert_eq!(
        store
            .collaboration_operations()
            .unwrap()
            .iter()
            .filter(|operation| operation.aggregate_kind == "work_delegation_v1")
            .count(),
        1
    );
}

#[test]
fn accept_cancel_before_accept_race_has_one_linearized_winner() {
    let test = TestStore::new("accept-cancel-race");
    install_policy(&test.store);
    let auth = authority();
    test.store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "race-propose",
                0,
            ),
            &proposal(),
            &auth,
            &policy(),
        )
        .expect("race proposal");
    let before = test.store.collaboration_operations().unwrap().len();
    let store = Arc::new(test.store.clone());
    let barrier = Arc::new(Barrier::new(2));
    let accept_store = Arc::clone(&store);
    let accept_barrier = Arc::clone(&barrier);
    let accept = std::thread::spawn(move || {
        let auth = authority();
        let decision = DelegationDecision {
            id: "race-accept".into(),
            delegation_id: "delegation-1".into(),
            expected_delegation_revision: 1,
            decision: DelegationDecisionKind::Accept,
            decided_by_target_host: auth.target_host.clone(),
            reason: "accept".into(),
            created_at: "2026-08-11T00:00:01Z".into(),
        };
        accept_barrier.wait();
        accept_store.decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "race-accept",
                1,
            ),
            "delegation-1",
            &decision,
            &auth,
            &placement(13),
        )
    });
    let cancel_store = Arc::clone(&store);
    let cancel_barrier = Arc::clone(&barrier);
    let cancel = std::thread::spawn(move || {
        let auth = authority();
        cancel_barrier.wait();
        cancel_store.cancel_delegation_before_accept(
            &context(
                auth.source_host.clone(),
                "delegation.cancel_before_accept",
                "race-cancel",
                1,
            ),
            "delegation-1",
            "withdraw before acceptance",
            &auth,
        )
    });
    let accepted = accept.join().expect("accept thread");
    let cancelled = cancel.join().expect("cancel thread");
    assert_ne!(accepted.is_ok(), cancelled.is_ok());
    assert_eq!(store.collaboration_operations().unwrap().len(), before + 1);
    let current = store
        .collaboration_delegations("company-1")
        .unwrap()
        .pop()
        .unwrap();
    assert!(matches!(
        current.state,
        DelegationState::ProvisioningTargetWork | DelegationState::Terminal
    ));
}

#[test]
fn torn_tail_is_ignored_and_exact_replay_repairs_atomic_ledger() {
    let test = TestStore::new("torn-tail");
    install_policy(&test.store);
    let auth = authority();
    let ctx = context(
        auth.source_host.clone(),
        "delegation.propose",
        "torn-propose",
        0,
    );
    test.store
        .propose_collaboration_delegation(&ctx, &proposal(), &auth, &policy())
        .expect("durable proposal");
    let ledger = test.root.join("agentfirm_collaboration_operations.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .unwrap();
    file.write_all(b"{\"torn\":").unwrap();
    file.sync_all().unwrap();

    let replay = test
        .store
        .propose_collaboration_delegation(&ctx, &proposal(), &auth, &policy())
        .expect("complete durable rows survive torn tail");
    assert!(replay.replayed);
    assert_eq!(
        test.store
            .collaboration_delegations("company-1")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn delegation_list_cursor_freezes_snapshot_and_filters_exact_scope() {
    let test = TestStore::new("cursor");
    install_policy(&test.store);
    let auth = authority();
    for ordinal in 1..=3 {
        let mut request = proposal();
        request.delegation_id = format!("delegation-{ordinal}");
        request.operation_id = format!("route-propose-{ordinal}");
        test.store
            .propose_collaboration_delegation(
                &context(
                    auth.source_host.clone(),
                    "delegation.propose",
                    &format!("cursor-propose-{ordinal}"),
                    0,
                ),
                &request,
                &auth,
                &policy(),
            )
            .expect("cursor proposal");
    }
    let filter = CollaborationDelegationFilter {
        source_team_id: Some("team-a".into()),
        target_team_id: Some("team-b".into()),
        node_id: Some("node-b".into()),
        state: Some(DelegationState::AwaitingTargetDecision),
    };
    let first = test
        .store
        .list_collaboration_delegations("company-1", &filter, None, 2)
        .expect("first frozen page");
    assert_eq!(first.items.len(), 2);
    let cursor = first.next_cursor.expect("third item remains");

    let mut fourth = proposal();
    fourth.delegation_id = "delegation-4".into();
    fourth.operation_id = "route-propose-4".into();
    test.store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "cursor-propose-4",
                0,
            ),
            &fourth,
            &auth,
            &policy(),
        )
        .expect("fourth proposal after first page");

    let second = test
        .store
        .list_collaboration_delegations("company-1", &filter, Some(cursor), 2)
        .expect("second page from frozen sequence");
    assert_eq!(second.items.len(), 1);
    assert!(second.next_cursor.is_none());
    assert_eq!(second.as_of_store_sequence, first.as_of_store_sequence);

    let fresh = test
        .store
        .list_collaboration_delegations("company-1", &filter, None, 10)
        .expect("fresh view includes later proposal");
    assert_eq!(fresh.items.len(), 4);
    assert!(fresh.as_of_store_sequence > first.as_of_store_sequence);
}

#[test]
fn active_cancellation_is_only_a_source_request_and_target_host_decision() {
    let test = TestStore::new("cancel");
    let _target = active_delegation(&test.store);
    let auth = authority();
    let request = DelegationCancellationRequest {
        id: "cancel-request-1".into(),
        delegation_id: "delegation-1".into(),
        expected_delegation_revision: 3,
        requested_by: auth.source_host.clone(),
        reason: "source priorities changed".into(),
        state: CancellationRequestState::Pending,
        target_host_decision_ref: None,
        revision: 1,
        created_at: "2026-08-11T00:00:03Z".into(),
        updated_at: "2026-08-11T00:00:03Z".into(),
    };
    let requested = test
        .store
        .request_delegation_cancellation(
            &context(
                auth.source_host.clone(),
                "delegation.cancel.request",
                "cancel-1",
                3,
            ),
            &request,
            &auth,
        )
        .expect("source Host cancellation request");
    assert_eq!(
        requested.projection.state,
        DelegationState::CancellationRequested
    );
    assert_eq!(requested.projection.terminal_outcome, None);

    let decision = DelegationCancellationDecision {
        id: "cancel-decision-1".into(),
        cancellation_request_id: request.id.clone(),
        expected_request_revision: 1,
        decision: CancellationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        native_work_event_ref: "work-event-b-cancelled".into(),
        reason: "target Work quiesced".into(),
        created_at: "2026-08-11T00:00:04Z".into(),
    };
    let cancelled = test
        .store
        .decide_delegation_cancellation(
            &context(
                auth.target_host.clone(),
                "delegation.cancel.decide",
                "cancel-decision-1",
                4,
            ),
            "delegation-1",
            &request.id,
            &decision,
            &auth,
            &placement(13),
        )
        .expect("target Host cancellation decision");
    assert_eq!(cancelled.projection.state, DelegationState::Terminal);
    assert_eq!(
        cancelled.projection.terminal_outcome,
        Some(DelegationTerminalOutcome::Cancelled)
    );
    assert_eq!(cancelled.projection.source_work_ref.work_revision, 9);
    let request_projection = test
        .store
        .collaboration_cancellation_requests("company-1", "delegation-1")
        .unwrap()
        .pop()
        .expect("cancellation request projection");
    assert_eq!(request_projection.state, CancellationRequestState::Accepted);
    assert_eq!(request_projection.revision, 2);
    assert_eq!(
        request_projection.target_host_decision_ref.as_deref(),
        Some("cancel-decision-1")
    );
}

#[test]
fn remote_fact_is_redacted_digest_bound_and_target_scoped() {
    let test = TestStore::new("publication");
    let target = active_delegation(&test.store);
    let fact = serde_json::json!({
        "submitted_work_revision": 1,
        "outcome": "implemented",
        "checks": ["check:unit"],
        "evidence": ["artifact:diff"],
        "target_host_decision": "accepted"
    });
    let digest = canonical_json_fingerprint(&fact);
    let publication = RemoteFactPublication {
        id: "publication-1".into(),
        company_id: "company-1".into(),
        delegation_id: "delegation-1".into(),
        origin_node_id: "node-b".into(),
        origin_team_id: "team-b".into(),
        fact_work_ref: target,
        delegation_source_work_ref: source_attestation().source_work_ref,
        fact_kind: RemoteFactKind::Report,
        fact_id: "report-b-1".into(),
        fact_revision: 1,
        fact_digest: digest.clone(),
        summary: "target result is ready for source integration".into(),
        classification: "team-visible".into(),
        snapshot: RemoteFactSnapshot {
            publication_id: "publication-1".into(),
            fact_schema: "agentfirm.work-report.v1".into(),
            canonical_redacted_fact: fact,
            canonical_digest: digest,
        },
        artifact_refs: vec!["artifact:diff".into()],
        evidence_refs: vec!["check:unit".into()],
        operational_decision_ref: None,
        created_by: actor(ActorKind::AgentMember, "member-b"),
        created_at: "2026-08-11T00:00:05Z".into(),
        retain_until: "2026-09-10T00:00:05Z".into(),
    };
    let publish_context = context(
        publication.created_by.clone(),
        "remote_fact.publish",
        "publish-1",
        0,
    );
    let mut forged = publication.clone();
    forged.fact_digest = format!("sha256:{:064x}", 999);
    let before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .publish_remote_fact(
            &publish_context,
            &forged,
            std::slice::from_ref(&publication.created_by),
            &placement(13),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let published = test
        .store
        .publish_remote_fact(
            &publish_context,
            &publication,
            std::slice::from_ref(&publication.created_by),
            &placement(13),
        )
        .expect("publish exact redacted snapshot");
    assert!(!published.replayed);
    let replay = test
        .store
        .publish_remote_fact(
            &publish_context,
            &publication,
            std::slice::from_ref(&publication.created_by),
            &placement(13),
        )
        .expect("publication replay");
    assert!(replay.replayed);

    let operational_decision = WorkOperationalDecisionRef {
        decision_id: "work-decision-b-1".into(),
        work_ref: publication.fact_work_ref.clone(),
        decision_revision: 1,
        digest: format!("sha256:{:064x}", 77),
    };
    let available = test
        .store
        .mark_delegation_result_available(
            &context(
                authority().target_host.clone(),
                "delegation.result_available",
                "result-available-1",
                3,
            ),
            "delegation-1",
            &publication.id,
            &operational_decision,
            &authority(),
            &placement(13),
        )
        .expect("target Host exposes accepted target result");
    assert_eq!(available.projection.state, DelegationState::ResultAvailable);
    assert_eq!(available.projection.source_work_ref.work_revision, 9);

    let integrated_source = work_ref("node-a", "team-a", "work-a", 10);
    let completed = test
        .store
        .complete_delegation_after_source_integration(
            &context(
                authority().source_host.clone(),
                "delegation.complete_after_source_integration",
                "source-integrated-1",
                4,
            ),
            "delegation-1",
            &integrated_source,
            "source-work-event-accepted-10",
            &authority(),
        )
        .expect("source Host independently integrates and closes relationship");
    assert_eq!(completed.projection.state, DelegationState::Terminal);
    assert_eq!(
        completed.projection.terminal_outcome,
        Some(DelegationTerminalOutcome::Completed)
    );
    // The relationship stores integration evidence but never rewrites source
    // Work authority into its own projection.
    assert_eq!(completed.projection.source_work_ref.work_revision, 9);
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
        recipient_identity_id: recipient.into(),
        target_node_id: "node-b".into(),
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
        "sender_agent_id": message.sender_agent_id,
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

#[test]
fn message_projection_preserves_per_recipient_partial_delivery_truth() {
    let recipients = vec![
        MessageRecipientRef {
            kind: MessageRecipientKind::AgentIdentity,
            id: "member-b1".into(),
        },
        MessageRecipientRef {
            kind: MessageRecipientKind::AgentIdentity,
            id: "member-b2".into(),
        },
    ];
    let mut message = Message {
        id: "message-1".into(),
        source_execution_space_id: "space-node-a".into(),
        source_node_id: "node-a".into(),
        source_node_daemon_id: "daemon-a".into(),
        source_authority_generation: 8,
        sender_actor_ref: actor(ActorKind::AgentMember, "host-a"),
        sender_agent_id: Some("host-a".into()),
        sender_session_id: Some("session-host-a".into()),
        address_kind: MessageAddressKind::DirectAgent,
        target_ref: recipients[0].clone(),
        recipients,
        team_id: Some("team-a".into()),
        team_run_id: None,
        work_id: Some("work-a".into()),
        collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
            source_team_id: "team-a".into(),
            target_team_id: "team-b".into(),
            delegation_id: Some("delegation-1".into()),
            expected_delegation_revision: Some(3),
            source_work_ref: Some(work_ref("node-a", "team-a", "work-a", 9)),
            target_work_ref: Some(work_ref("node-b", "team-b", "work-b", 1)),
        }),
        kind: MessageKind::Message,
        body: "Please review the delegated result".into(),
        body_digest: canonical_json_fingerprint(&serde_json::json!({
            "body": "Please review the delegated result"
        })),
        correlation_id: "correlation-1".into(),
        causation_id: None,
        response_intent: ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "message-1".into(),
        created_at: "2026-08-11T00:00:00Z".into(),
    };
    message.content_fingerprint = message_fingerprint(&message);
    validate_message_collaboration_scope(&message).expect("exact source/target scope");
    let mut forged_scope = message.clone();
    forged_scope
        .collaboration_scope
        .as_mut()
        .unwrap()
        .target_team_id = "team-a".into();
    assert!(validate_message_collaboration_scope(&forged_scope).is_err());
    let replica = persisted_replica(&message);
    let deliveries = vec![
        canonical_delivery(
            "delivery-1",
            "member-b1",
            CanonicalMessageDeliveryStatus::ProviderReceived,
        ),
        canonical_delivery(
            "delivery-2",
            "member-b2",
            CanonicalMessageDeliveryStatus::Queued,
        ),
    ];
    let projections = project_cross_node_deliveries(
        &message,
        &replica,
        &deliveries,
        "route-1",
        Some(9),
        44,
        "2026-08-11T00:00:01Z",
    )
    .expect("project independent recipient states");
    assert_eq!(projections.len(), 2);
    assert_eq!(
        projections[0].state,
        CanonicalMessageDeliveryStatus::ProviderReceived
    );
    assert_eq!(projections[1].state, CanonicalMessageDeliveryStatus::Queued);

    let mut duplicate = deliveries.clone();
    duplicate[1].recipient_identity_id = "member-b1".into();
    assert!(project_cross_node_deliveries(
        &message,
        &replica,
        &duplicate,
        "route-1",
        Some(9),
        44,
        "2026-08-11T00:00:01Z",
    )
    .is_err());

    let team_recipient = MessageRecipientRef {
        kind: MessageRecipientKind::Team,
        id: "team-b".into(),
    };
    let team_message = Message {
        target_ref: team_recipient.clone(),
        recipients: vec![team_recipient],
        ..message.clone()
    };
    let team_replica = persisted_replica(&team_message);
    assert_eq!(
        project_cross_node_deliveries(
            &team_message,
            &team_replica,
            &deliveries,
            "route-team-1",
            Some(9),
            45,
            "2026-08-11T00:00:02Z",
        )
        .expect("target Node subscription expansion remains per-recipient")
        .len(),
        2
    );

    let mut mixed_nodes = deliveries;
    mixed_nodes[1].target_node_id = "node-c".into();
    assert!(project_cross_node_deliveries(
        &team_message,
        &team_replica,
        &mixed_nodes,
        "route-team-1",
        Some(9),
        45,
        "2026-08-11T00:00:02Z",
    )
    .is_err());
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

#[test]
fn immutable_message_transfer_persists_exact_replica_before_delivery_and_replays() {
    let recipients = vec![MessageRecipientRef {
        kind: MessageRecipientKind::AgentIdentity,
        id: "member-b1".into(),
    }];
    let mut message = Message {
        id: "remote-message-1".into(),
        source_execution_space_id: "space-node-a".into(),
        source_node_id: "node-a".into(),
        source_node_daemon_id: "daemon-a".into(),
        source_authority_generation: 8,
        sender_actor_ref: actor(ActorKind::AgentMember, "host-a"),
        sender_agent_id: Some("host-a".into()),
        sender_session_id: Some("session-host-a".into()),
        address_kind: MessageAddressKind::DirectAgent,
        target_ref: recipients[0].clone(),
        recipients,
        team_id: Some("team-a".into()),
        team_run_id: None,
        work_id: Some("work-a".into()),
        collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
            source_team_id: "team-a".into(),
            target_team_id: "team-b".into(),
            delegation_id: Some("delegation-1".into()),
            expected_delegation_revision: Some(3),
            source_work_ref: Some(work_ref("node-a", "team-a", "work-a", 9)),
            target_work_ref: Some(work_ref("node-b", "team-b", "work-b", 1)),
        }),
        kind: MessageKind::Message,
        body: "immutable remote body".into(),
        body_digest: canonical_json_fingerprint(
            &serde_json::json!({"body": "immutable remote body"}),
        ),
        correlation_id: "remote-correlation-1".into(),
        causation_id: None,
        response_intent: ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "remote-message-1".into(),
        created_at: "2026-08-11T00:00:00Z".into(),
    };
    message.content_fingerprint = message_fingerprint(&message);
    let bytes = serde_json::to_vec(&message).unwrap();
    let port = FaithfulReplicaStore::default();
    let inline = ImmutableMessageTransferPayload::CanonicalBytes {
        canonical_message_bytes: bytes.clone(),
    };
    let queued = queue_remote_message_transfer(
        &message,
        &placement(13),
        inline.clone(),
        "2026-08-11T00:00:00Z",
    )
    .expect("source Node queues the already-authored Message while Control Plane is offline");
    assert_eq!(
        queued.state,
        RemoteMessageTransferState::QueuedForControlPlane
    );
    assert_eq!(queued.message_id, message.id);
    let expectation = |persisted_at: &str| RemoteMessageReplicaExpectation {
        source_execution_space_id: message.source_execution_space_id.clone(),
        message_id: message.id.clone(),
        schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint.clone(),
        body_digest: message.body_digest.clone(),
        persisted_at: persisted_at.into(),
    };
    let first = persist_verified_remote_message_replica(
        &port,
        &inline,
        &expectation("2026-08-11T00:00:01Z"),
    )
    .expect("target persists exact inline remote replica");
    let replay = persist_verified_remote_message_replica(
        &port,
        &inline,
        &expectation("2026-08-11T00:00:02Z"),
    )
    .expect("same Message bytes replay the original target replica");
    assert_eq!(first, replay);
    assert_eq!(port.replicas.lock().unwrap().len(), 1);

    let object_ref = "message-object:remote-message-1";
    let object_digest =
        canonical_json_fingerprint(&serde_json::from_slice::<serde_json::Value>(&bytes).unwrap());
    port.objects
        .lock()
        .unwrap()
        .insert(object_ref.into(), bytes.clone());
    let referenced = ImmutableMessageTransferPayload::MessageObjectRef {
        message_object_ref: object_ref.into(),
        authenticated_content_digest: object_digest,
    };
    assert!(persist_verified_remote_message_replica(
        &port,
        &referenced,
        &expectation("2026-08-11T00:00:03Z"),
    )
    .is_ok());

    let before = port.replicas.lock().unwrap().clone();
    let mut forged = message.clone();
    forged.body = "forged body".into();
    let forged_payload = ImmutableMessageTransferPayload::CanonicalBytes {
        canonical_message_bytes: serde_json::to_vec(&forged).unwrap(),
    };
    assert!(persist_verified_remote_message_replica(
        &port,
        &forged_payload,
        &expectation("2026-08-11T00:00:04Z"),
    )
    .is_err());
    assert_eq!(*port.replicas.lock().unwrap(), before);

    let deliveries = vec![CanonicalMessageDelivery {
        message_id: message.id.clone(),
        ..canonical_delivery(
            "remote-delivery-1",
            "member-b1",
            CanonicalMessageDeliveryStatus::Queued,
        )
    }];
    assert_eq!(
        project_cross_node_deliveries(
            &message,
            &first,
            &deliveries,
            "route-remote-1",
            Some(9),
            45,
            "2026-08-11T00:00:05Z",
        )
        .expect("delivery projection is derived only after replica persistence")
        .len(),
        1
    );
}

#[test]
fn source_work_attestation_and_placement_v1_fail_closed() {
    let test = TestStore::new("attestation");
    install_policy(&test.store);
    let auth = authority();
    let before = test.store.collaboration_operations().unwrap();

    let mut caller_authored = source_attestation();
    caller_authored.id = "caller-authored-attestation".into();
    caller_authored.work_application_service_ref = auth.source_host.clone();
    assert!(test
        .store
        .put_source_work_attestation(
            &context(
                auth.source_host.clone(),
                "source_work.attest",
                "caller-authored-attestation",
                0,
            ),
            &caller_authored,
            &auth.source_work_application_service,
            auth.source_gateway_generation,
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let mut stale_authority = auth.clone();
    stale_authority.source_gateway_generation = 9;
    assert!(test
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "stale-attestation-propose",
                0,
            ),
            &proposal(),
            &stale_authority,
            &policy(),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let mut non_v1 = proposal();
    non_v1.target_placement.placement_generation = 2;
    assert!(test
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "non-v1-placement",
                0,
            ),
            &non_v1,
            &auth,
            &policy(),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    assert_eq!(
        CollaborationRetentionAnchor {
            terminal_transport_at_unix_ms: Some(100),
            terminal_delegation_at_unix_ms: Some(300),
            source_import_completed_at_unix_ms: Some(200),
        }
        .safe_retention_start_unix_ms(),
        Some(300)
    );
    assert_eq!(
        CollaborationRetentionAnchor {
            terminal_transport_at_unix_ms: Some(100),
            terminal_delegation_at_unix_ms: Some(300),
            source_import_completed_at_unix_ms: None,
        }
        .retain_until_unix_ms(30 * 24 * 60 * 60 * 1_000),
        None
    );
}

#[test]
fn all_frozen_business_kinds_use_the_wave5_route_and_terminal_receipt_contract() {
    let kinds = [
        RoutedBusinessKind::DelegationPropose,
        RoutedBusinessKind::DelegationDecide,
        RoutedBusinessKind::TargetWorkCreate,
        RoutedBusinessKind::DelegationCancelRequest,
        RoutedBusinessKind::DelegationCancelDecide,
        RoutedBusinessKind::TeamMessageDeliver,
        RoutedBusinessKind::RemoteFactPublish,
        RoutedBusinessKind::ArtifactGrant,
    ];
    let context = CollaborationFabricRouteContext {
        authenticated_actor: AuthenticatedActor {
            company_id: "company-1".into(),
            actor_id: "node-a".into(),
            actor_kind: FabricActorKind::Service,
            role_bindings: BTreeSet::from(["fabric_submit".into()]),
            session_id: "session-host-a".into(),
            issued_at_unix_ms: 1,
            expires_at_unix_ms: 10_000,
        },
        resolved_business_actor: actor(ActorKind::AgentMember, "host-a"),
        source: CollaborationFabricSource::Node {
            source_execution_space_id: "space-node-a".into(),
            source_gateway_generation: 8,
            source_node_daemon_id: "daemon-a".into(),
            source_node_daemon_generation: 4,
        },
        control_plane_generation: 3,
        target_execution_space_id: Some("space-node-b".into()),
        created_at_unix_ms: 100,
        expires_at_unix_ms: 5_000,
    };

    for kind in kinds {
        let payload = serde_json::json!({"kind": kind.wire_name(), "delegation_id": "d-1"});
        let operation = RoutedBusinessOperation {
            id: format!("route-{}", kind.wire_name()),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: "company-1".into(),
            kind,
            authenticated_actor: actor(ActorKind::AgentMember, "host-a"),
            source_node_id: "node-a".into(),
            target_placement: placement(13),
            expected_revision: 7,
            idempotency_key: format!("idem-{}", kind.wire_name()),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: kind.required_capability(),
            ordering_key: "delegation:d-1".into(),
            created_at: "2026-08-13T00:00:00Z".into(),
        };
        let mut route_context = context.clone();
        if matches!(
            kind,
            RoutedBusinessKind::DelegationDecide
                | RoutedBusinessKind::TargetWorkCreate
                | RoutedBusinessKind::DelegationCancelDecide
                | RoutedBusinessKind::ArtifactGrant
        ) {
            route_context.source = CollaborationFabricSource::ControlPlane;
            route_context.authenticated_actor.actor_id = "control-plane-1".into();
        }
        let routed = route_collaboration_business_operation(&operation, &route_context)
            .expect("frozen collaboration kind must use the Wave5 envelope");
        assert_eq!(routed.kind, COLLABORATION_BUSINESS_OPERATION_KIND);
        routed.closed_body().expect("closed transport registry");

        let result = serde_json::json!({"operation": operation.id, "applied": true});
        let receipt = RouteReceipt {
            id: format!("receipt:{}", operation.id),
            company_id: operation.company_id.clone(),
            operation_id: operation.id.clone(),
            target_node_id: operation.target_placement.node_id.clone(),
            target_gateway_generation: 9,
            control_plane_generation: 3,
            route_seq: 11,
            kind: ReceiptKind::OperationApplied,
            application_effect: Some(EffectCertainty::Applied),
            result_schema: Some("agentfirm.collaboration.result.v1".into()),
            result_digest: Some(json_digest(&result).unwrap()),
            result: Some(result.clone()),
            error: None,
            created_at_unix_ms: 150,
            schema_version: "agentfirm.remote_fabric.v1".into(),
        };
        let business =
            collaboration_receipt_from_fabric(&operation, &receipt, "2026-08-13T00:00:01Z")
                .expect("only terminal applied is business success");
        assert_eq!(business.result, result);

        let mut accepted_only = receipt.clone();
        accepted_only.kind = ReceiptKind::ControlPlaneAccepted;
        accepted_only.application_effect = None;
        assert!(collaboration_receipt_from_fabric(
            &operation,
            &accepted_only,
            "2026-08-13T00:00:01Z"
        )
        .is_err());

        let mut unknown = receipt;
        unknown.kind = ReceiptKind::RecoveryRequired;
        unknown.application_effect = Some(EffectCertainty::Unknown);
        let error = collaboration_receipt_from_fabric(&operation, &unknown, "2026-08-13T00:00:01Z")
            .unwrap_err();
        assert_eq!(error.code, FabricErrorCode::RecoveryRequired);
        assert_eq!(error.effect_certainty, FabricEffectCertainty::Unknown);
    }
}

#[test]
fn target_work_create_applies_once_through_native_work_authority() {
    let target = TestStore::new("target-work-application");
    seed_target_team(&target.store);
    let target_placement = TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 1,
        node_id: TARGET_NODE_UUID.into(),
        placement_generation: 1,
    };
    let payload = serde_json::json!({
        "delegation_id": "delegation-native-1",
        "requested_outcome": "Implement the target component",
        "acceptance_contract": "checks and evidence are required",
        "source_work_ref": work_ref("node-a", "team-a", "work-a", 9),
        "target_placement": target_placement,
    });
    let business = RoutedBusinessOperation {
        id: "route-target-native-1".into(),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: "company-1".into(),
        kind: RoutedBusinessKind::TargetWorkCreate,
        authenticated_actor: actor(ActorKind::AgentMember, "host-b"),
        source_node_id: "node-a".into(),
        target_placement,
        expected_revision: 2,
        idempotency_key: "target-native-1".into(),
        payload_digest: canonical_json_fingerprint(&payload),
        payload,
        required_capability: "collaboration.target_work_create".into(),
        ordering_key: "delegation:delegation-native-1".into(),
        created_at: "2026-08-13T00:00:00Z".into(),
    };
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-a".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "daemon-a:1".into(),
                issued_at_unix_ms: 1,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-b"),
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
    )
    .unwrap();
    let first = apply_collaboration_target_operation(&target.store, &route, "unix-ms:200")
        .expect("native target Work creation");
    let replay = apply_collaboration_target_operation(&target.store, &route, "unix-ms:201")
        .expect("native target Work exact replay");
    assert_eq!(first.1, replay.1);
    let works = target.store.latest_works().unwrap();
    assert_eq!(works.len(), 1);
    assert_eq!(works[0].id, "remote-work:delegation-native-1");
    assert_eq!(works[0].team_id.as_deref(), Some("team-b"));

    let before = target.store.latest_works().unwrap();
    let mut stale = route;
    stale.body["target_team_revision"] = serde_json::json!(2);
    stale.body_digest = json_digest(&stale.body).unwrap();
    assert!(apply_collaboration_target_operation(&target.store, &stale, "unix-ms:202").is_err());
    assert_eq!(target.store.latest_works().unwrap(), before);
}

#[test]
fn delegation_proposal_routes_source_attestation_and_target_resolves_host() {
    let source = TestStore::new("proposal-source");
    let target = TestStore::new("proposal-target");
    install_policy(&source.store);
    seed_target_team(&target.store);
    let target_placement = TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 1,
        node_id: TARGET_NODE_UUID.into(),
        placement_generation: 1,
    };
    let request = ProposeDelegationRequest {
        delegation_id: "delegation-routed-1".into(),
        source_work_attestation_id: "source-work-attestation-1".into(),
        target_placement: target_placement.clone(),
        requested_outcome: "Implement target component".into(),
        outcome_class: "implementation".into(),
        acceptance_contract: "checks and evidence".into(),
        operation_id: "route-proposal-1".into(),
    };
    let business = source
        .store
        .delegation_propose_operation(
            &context(
                actor(ActorKind::AgentMember, "host-a"),
                "delegation_propose",
                "proposal-route-1",
                0,
            ),
            &request,
            "policy-a-b",
        )
        .expect("source Node builds attested proposal");
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-a".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "daemon-a:8".into(),
                issued_at_unix_ms: 1,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-a"),
            source: CollaborationFabricSource::Node {
                source_execution_space_id: "space-node-a".into(),
                source_gateway_generation: 8,
                source_node_daemon_id: "daemon-a".into(),
                source_node_daemon_generation: 4,
            },
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("Wave5 route");
    let applied = apply_collaboration_target_operation(&target.store, &route, "unix-ms:200")
        .expect("target validates current Team placement and Host");
    assert_eq!(
        applied.0,
        "agentfirm.collaboration.delegation_proposal_validated.v1"
    );
    assert_eq!(applied.1["target_host_ref"]["id"], "host-b");
    assert!(target.store.latest_works().unwrap().is_empty());

    let before = target.store.collaboration_operations().unwrap();
    let mut stale = route;
    stale.body["target_team_revision"] = serde_json::json!(2);
    stale.body_digest = json_digest(&stale.body).unwrap();
    assert!(apply_collaboration_target_operation(&target.store, &stale, "unix-ms:201").is_err());
    assert_eq!(target.store.collaboration_operations().unwrap(), before);
}

#[test]
fn target_host_decision_routes_under_control_plane_and_validates_local_team() {
    let central = TestStore::new("decision-central");
    let target = TestStore::new("decision-target");
    install_policy(&central.store);
    seed_target_team(&target.store);
    let target_placement = TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 1,
        node_id: TARGET_NODE_UUID.into(),
        placement_generation: 1,
    };
    let mut auth = authority();
    auth.target_placement = target_placement.clone();
    let mut request = proposal();
    request.target_placement = target_placement.clone();
    central
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "decision-propose-1",
                0,
            ),
            &request,
            &auth,
            &policy(),
        )
        .expect("central proposal");
    let decision = DelegationDecision {
        id: "decision-route-1".into(),
        delegation_id: request.delegation_id.clone(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        reason: "capacity available".into(),
        created_at: "2026-08-13T00:00:00Z".into(),
    };
    let business = central
        .store
        .delegation_decide_operation(
            &context(
                auth.target_host.clone(),
                "delegation_decide",
                "decision-route-1",
                1,
            ),
            &request.delegation_id,
            &decision,
        )
        .expect("Control Plane builds exact target Host decision");
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "control-plane:3".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["company_control_plane".into()]),
                session_id: "control-plane:3".into(),
                issued_at_unix_ms: 100,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: auth.target_host,
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("decision uses accepted Control Plane route authority");
    let applied = apply_collaboration_target_operation(&target.store, &route, "unix-ms:200")
        .expect("current target Host decision validates");
    assert_eq!(
        applied.0,
        "agentfirm.collaboration.delegation_decision_validated.v1"
    );
    assert_eq!(applied.1["decision"]["id"], "decision-route-1");
    assert!(target.store.latest_works().unwrap().is_empty());

    let before = target.store.latest_works().unwrap();
    let mut hostile = route;
    hostile.body["business_actor_id"] = serde_json::json!("member-b");
    hostile.body_digest = json_digest(&hostile.body).unwrap();
    assert!(apply_collaboration_target_operation(&target.store, &hostile, "unix-ms:201").is_err());
    assert_eq!(target.store.latest_works().unwrap(), before);
}
