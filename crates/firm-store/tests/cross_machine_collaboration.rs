use firm_core::agentfirm_api::{
    ActorKind, ActorRef, CanonicalMessageDelivery, CanonicalMessageDeliveryStatus, Message,
    MessageAddressKind, MessageKind, MessageRecipientKind, MessageRecipientRef, ResponseIntent,
};
use firm_core::collaboration::{
    CancellationDecisionKind, CancellationRequestState, DelegationCancellationDecision,
    DelegationCancellationRequest, DelegationDecision, DelegationDecisionKind,
    DelegationInboundMode, DelegationInboundPolicy, DelegationState, DelegationTerminalOutcome,
    FabricEffectCertainty, FabricError, FabricErrorCode, RemoteFactKind, RemoteFactPublication,
    RemoteFactSnapshot, RemoteWorkRef, RoutedBusinessOperation, RoutedBusinessReceipt,
    TargetPlacementRef, WorkOperationalDecisionRef,
};
use firm_store::{
    canonical_json_fingerprint, project_cross_node_deliveries, CollaborationApplicationService,
    CollaborationDelegationFilter, CollaborationFabricPort, CollaborationMutationContext,
    HarnessStore, ProposeDelegationRequest, ResolvedCollaborationAuthority,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

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

fn placement(generation: u64) -> TargetPlacementRef {
    TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 7,
        node_id: "node-b".into(),
        placement_generation: generation,
    }
}

fn work_ref(node: &str, team: &str, work: &str, revision: u64) -> RemoteWorkRef {
    RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: format!("space-{node}"),
        node_id: node.into(),
        team_id: team.into(),
        team_revision: if team == "team-a" { 5 } else { 7 },
        placement_generation: if team == "team-a" { 11 } else { 13 },
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
        source_work_ref: work_ref("node-a", "team-a", "work-a", 9),
        source_owner_ref: actor(ActorKind::AgentMember, "member-a"),
        target_placement: placement(13),
        requested_outcome: "Implement the remote component".into(),
        outcome_class: "implementation".into(),
        acceptance_contract: "checks and evidence are required".into(),
        operation_id: "route-propose-1".into(),
    }
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
        .target_work_create_operation(
            "company-1",
            &request.delegation_id,
            &auth.target_host,
            "2026-08-11T00:00:02Z",
        )
        .expect("build target Work routed operation");
    assert_eq!(route.ordering_key, "delegation:delegation-1");

    let target = work_ref("node-b", "team-b", "work-b", 1);
    let active = store
        .apply_target_work_created(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "target-applied-1",
                2,
            ),
            &request.delegation_id,
            &target,
            &placement(13),
            &route.id,
        )
        .expect("fold applied target Work result");
    assert_eq!(active.projection.state, DelegationState::Active);
    assert_eq!(active.projection.source_work_ref, request.source_work_ref);
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
        .target_work_create_operation(
            "company-1",
            "delegation-1",
            &auth.target_host,
            "2026-08-11T00:00:02Z",
        )
        .unwrap();
    let fabric = FaithfulFabric::default();
    let first_receipt = fabric.dispatch(&route).unwrap();
    let replay_receipt = fabric.dispatch(&route).unwrap();
    assert_eq!(first_receipt, replay_receipt);
    assert_eq!(fabric.effect_count(), 1);

    let service = CollaborationApplicationService::new(&test.store, &fabric);
    let applied = service
        .provision_target_work(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "fabric-fold-1",
                2,
            ),
            "delegation-1",
            &auth.target_host,
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
    let unknown = CollaborationApplicationService::new(&second.store, &unknown_fabric);
    assert!(unknown
        .provision_target_work(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "unknown-fold-1",
                2,
            ),
            "delegation-1",
            &auth.target_host,
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
    assert_eq!(test.store.collaboration_operations().unwrap().len(), 2);

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
        delegation_source_work_ref: proposal().source_work_ref,
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
    let message = Message {
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
        kind: MessageKind::Message,
        body: "Please review the delegated result".into(),
        body_digest: format!("sha256:{:064x}", 1),
        correlation_id: "correlation-1".into(),
        causation_id: None,
        response_intent: ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        content_fingerprint: format!("sha256:{:064x}", 2),
        schema_version: 1,
        idempotency_key: "message-1".into(),
        created_at: "2026-08-11T00:00:00Z".into(),
    };
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
        &duplicate,
        "route-1",
        Some(9),
        44,
        "2026-08-11T00:00:01Z",
    )
    .is_err());
}
