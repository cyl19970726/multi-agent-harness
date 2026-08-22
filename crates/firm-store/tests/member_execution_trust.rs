//! Black-box regression tests for the Member Execution Trust Kernel.
//!
//! These tests intentionally use only public `firm-core` contracts and public
//! `HarnessStore` methods. They pin the highest-risk persistence boundaries:
//! scoped idempotency/CAS, organization and run lifecycle fences, atomic
//! message fanout, report evidence, exact gates/waivers, and workspace paths.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, CandidateKind, CandidateRef,
    Confidence, DeliveryClaim, FailureAnalysis, GateEvaluation, GateRequirement,
    GateRequirementSource, GateVerdict, GateWaiver, GateWaiverState, MemberCoordinationStatus,
    MemberRun, MemberRuntimeStatus, MemberWorkspaceBinding, MutationContext,
    NativeSessionAvailability, NativeSessionRef, PermissionCeiling, PrimaryCauseStatus,
    RetrySafety, TeamMembership, TeamMembershipRole, TeamMembershipStatus, TrustError,
    TrustErrorCode, WorkFinding, WorkFindingKind, WorkReport, WorkReportKind, WorkspaceLifecycle,
    WorkspaceMode, WorkspaceOwnership, WorkspaceSafetyProof,
};
use firm_core::{
    AgentTeam, AgentTeamRun, AgentTeamStatus, ExecutionNode, ExecutionNodeStatus, MemberRunStatus,
    Mission, MissionStatus, NodeProjectRegistration, NodeProjectRegistrationStatus,
    ProviderRuntimeProjection as RuntimeMemberRun, TeamActorKind, TeamActorRef, TeamRunStatus,
    Work, WorkClaimMode, WorkCommandContext, WorkCondition, WorkDelegation, WorkDelegationState,
    WorkPhase, WorkPriority, WorkRef,
};
use firm_store::{
    canonical_json_fingerprint, CurrentTeamMemberLifecycleTransition, HarnessStore, StoreError,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const SPACE: &str = "space-trust-test";
const NODE: &str = "00000000-0000-4000-8000-0000000000aa";

struct TestStore {
    root: PathBuf,
    store: HarnessStore,
}

impl TestStore {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "firm-store-member-trust-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("initialize test store");
        Self { root, store }
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn human(id: &str) -> ActorRef {
    ActorRef {
        kind: ActorKind::Human,
        id: id.into(),
    }
}

fn member_actor(id: &str) -> ActorRef {
    ActorRef {
        kind: ActorKind::AgentMember,
        id: id.into(),
    }
}

fn service(id: &str) -> ActorRef {
    ActorRef {
        kind: ActorKind::Service,
        id: id.into(),
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after unix epoch")
        .as_millis() as u64
}

fn append_legacy_projection<T: serde::Serialize>(store: &HarnessStore, ledger: &str, value: &T) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join(ledger))
        .expect("open Legacy projection fixture ledger");
    serde_json::to_writer(&mut file, value).expect("serialize Legacy projection fixture");
    file.write_all(b"\n")
        .expect("terminate Legacy projection fixture row");
    file.sync_all()
        .expect("persist Legacy projection fixture row");
}

fn context(actor: ActorRef, command: &str, key: &str, expected_version: u64) -> MutationContext {
    MutationContext {
        execution_space_id: SPACE.into(),
        authenticated_actor: actor,
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version,
        request_fingerprint: None,
    }
}

fn trust_code(error: StoreError) -> TrustErrorCode {
    match error {
        StoreError::Conflict(raw) => {
            serde_json::from_str::<TrustError>(&raw)
                .unwrap_or_else(|_| panic!("expected serialized TrustError, got {raw}"))
                .code
        }
        other => panic!("expected trust conflict, got {other}"),
    }
}

fn member(id: &str, creator: &ActorRef) -> AgentMember {
    AgentMember {
        id: id.into(),
        name: format!("Member {id}"),
        description: "durable organization identity".into(),
        role: "worker".into(),
        capabilities: vec!["code".into()],
        skill_refs: Vec::new(),
        provider_profile_ref: Some("codex-default".into()),
        model_preference: None,
        workspace_policy: "managed-worktree".into(),
        permission_ceiling: PermissionCeiling::WorkspaceWrite,
        organization_status: AgentMemberOrganizationStatus::Active,
        version: 1,
        created_by: creator.clone(),
        created_at: "t1".into(),
        updated_at: "t1".into(),
    }
}

fn native_session(id: &str) -> NativeSessionRef {
    NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "persistent".into(),
        native_session_id: id.into(),
        native_locator_kind: "thread".into(),
        provider_version: Some("test".into()),
        adapter_contract_version: "1".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("t1".into()),
        parent_native_session_id: None,
    }
}

fn member_run(id: &str, member_id: &str, team_run_id: &str, resumable: bool) -> MemberRun {
    MemberRun {
        id: id.into(),
        agent_member_id: member_id.into(),
        team_run_id: team_run_id.into(),
        role_snapshot: "worker".into(),
        provider_profile_snapshot: Some("codex-default".into()),
        requested_controls: serde_json::json!({}),
        effective_controls: serde_json::json!({}),
        coordination_status: MemberCoordinationStatus::Active,
        runtime_status: MemberRuntimeStatus::Idle,
        runtime_generation: 1,
        workspace_binding_id: None,
        native_session: resumable.then(|| native_session(&format!("session-{id}"))),
        version: 1,
        started_at: "t1".into(),
        last_event_at: None,
        finished_at: None,
    }
}

fn seed_team(store: &HarnessStore, label: &str, member_ids: &[&str]) -> AgentTeamRun {
    let mission_id = format!("mission-{label}");
    let team_id = format!("team-{label}");
    let run_id = format!("team-run-{label}");
    store
        .append_mission(&Mission {
            id: mission_id.clone(),
            title: "trust test".into(),
            objective: "exercise the trust kernel".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Planned,
            legacy_wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "t1".into(),
            updated_at: "t1".into(),
            completed_at: None,
        })
        .expect("insert mission");
    if store
        .latest_execution_nodes()
        .expect("read nodes")
        .is_empty()
    {
        store
            .insert_execution_node(&ExecutionNode {
                id: NODE.into(),
                display_name: "test node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "t1".into(),
                updated_at: "t1".into(),
            })
            .expect("insert node");
    }
    if !store
        .latest_node_project_registrations()
        .expect("read project registrations")
        .iter()
        .any(|registration| {
            registration.node_id == NODE
                && registration.execution_space_id == SPACE
                && registration.project_binding_id == "project-test"
                && registration.status == NodeProjectRegistrationStatus::Active
        })
    {
        store
            .register_node_project(
                &NodeProjectRegistration {
                    node_id: NODE.into(),
                    execution_space_id: SPACE.into(),
                    project_binding_id: "project-test".into(),
                    status: NodeProjectRegistrationStatus::Active,
                    created_at: "t1".into(),
                    updated_at: "t1".into(),
                },
                SPACE,
            )
            .expect("register project on node");
    }
    let team_creator = human("fixture-host");
    for member_id in member_ids {
        if !store
            .trust_agent_members(SPACE)
            .expect("read Team AgentMembers")
            .iter()
            .any(|candidate| candidate.id == *member_id)
        {
            store
                .create_trust_agent_member(
                    &context(
                        team_creator.clone(),
                        "agent_member.create",
                        &format!("team-member-{label}-{member_id}"),
                        0,
                    ),
                    member(member_id, &team_creator),
                )
                .expect("create durable Team AgentMember");
        }
    }
    let host_agent_id = member_ids[0].to_string();
    let team = AgentTeam {
        id: team_id.clone(),
        name: "trust team".into(),
        description: "trust fixture".into(),
        legacy_mission_id: Some(mission_id.clone()),
        mission_id,
        host_agent_id: host_agent_id.clone(),
        node_id: NODE.into(),
        status: AgentTeamStatus::Active,
        revision: 1,
        trashed_at: None,
        member_ids: member_ids.iter().skip(1).map(|id| (*id).into()).collect(),
        created_at: "t1".into(),
        updated_at: "t1".into(),
    };
    let memberships = member_ids
        .iter()
        .enumerate()
        .map(|(index, member_id)| TeamMembership {
            id: format!("membership-{team_id}-{member_id}"),
            team_id: team_id.clone(),
            agent_member_id: (*member_id).into(),
            node_id: NODE.into(),
            role: if index == 0 {
                TeamMembershipRole::Host
            } else {
                TeamMembershipRole::Member
            },
            state: TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: Vec::new(),
            created_by: team_creator.clone(),
            revision: 1,
            joined_at: "t1".into(),
            left_at: None,
        })
        .collect();
    store
        .create_agent_team(
            &context(
                team_creator,
                "agent_team.create",
                &format!("team-create-{label}"),
                0,
            ),
            team,
            memberships,
        )
        .expect("create durable Team and Memberships");
    let run = AgentTeamRun {
        id: run_id,
        agent_team_id: team_id,
        execution_node_id: NODE.into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "test".into(),
        host_thread_id: None,
        host_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: host_agent_id,
            display_name: Some("Trust fixture Host".into()),
            authn_source: Some("test_team_membership:host".into()),
        }),
        host_control_mode: Default::default(),
        objective: "trust test".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: Vec::new(),
        budget_limit_usd: None,
        created_at: "t1".into(),
        updated_at: "t1".into(),
        completed_at: None,
    };
    append_legacy_projection(store, "team_runs.jsonl", &run);
    run
}

fn acquire_supervisor(
    store: &HarnessStore,
    run: &AgentTeamRun,
    supervisor_id: &str,
) -> firm_core::TeamSupervisorLease {
    let now = unix_ms();
    let daemon = store
        .acquire_node_daemon_lease(NODE, "daemon-test", "daemon-instance-test", now, 60_000)
        .expect("acquire node daemon lease");
    store
        .acquire_team_supervisor_under_node_lease(
            &run.id,
            NODE,
            &daemon.daemon_id,
            daemon.generation,
            SPACE,
            &run.project_binding_id,
            supervisor_id,
            std::process::id(),
            "test://member-execution-trust",
            now,
            60_000,
        )
        .expect("acquire team supervisor lease")
}

fn seed_team_work(store: &HarnessStore, label: &str, work_id: &str) -> String {
    let run = seed_team(store, label, &["host"]);
    let actor = store
        .exact_team_run_host_actor(&run.id)
        .expect("resolve exact fixture Host");
    let created = store
        .insert_work(
            Work {
                id: work_id.into(),
                team_run_id: run.id.clone(),
                accountable_team_id: None,
                assignee_membership_id: None,
                created_by_member_id: None,
                parent_work_id: None,
                title: format!("Trust fixture {work_id}"),
                context_markdown: "trust test".into(),
                completion_criteria_markdown: "done".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: actor.clone(),
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            WorkCommandContext {
                event_id: format!("event-{work_id}"),
                performed_by_actor: actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("create-{work_id}"),
                created_at: "t1".into(),
                duplicate_ok: false,
            },
        )
        .expect("seed Team-scoped Work");
    assert_eq!(created.version, 1);
    run.agent_team_id
}

fn seed_active_team_work(store: &HarnessStore, label: &str, work_id: &str) -> String {
    let run = seed_team(store, label, &["worker"]);
    let runtime_id = "runtime-worker";
    create_member_and_run(store, &human("host"), &run.id, "worker", runtime_id, false);
    append_legacy_projection(
        store,
        "member_runs.jsonl",
        &RuntimeMemberRun {
            id: runtime_id.into(),
            team_run_id: run.id.clone(),
            slot_id: None,
            agent_member_id: "worker".into(),
            name: "Worker".into(),
            role: "worker".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            zero_output_streak: 0,
            last_consumed_work_version: None,
            started_at: "t1".into(),
            last_event_at: None,
            finished_at: None,
        },
    );
    let team_id = seed_team_work_from_run(store, &run, work_id);
    let host = store
        .exact_team_run_host_actor(&run.id)
        .expect("resolve exact fixture Host");
    store
        .assign_work(
            work_id,
            1,
            runtime_id,
            WorkCommandContext {
                event_id: format!("assign-{work_id}"),
                performed_by_actor: host,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("assign-{work_id}"),
                created_at: "t2".into(),
                duplicate_ok: false,
            },
        )
        .expect("assign Work");
    store
        .start_work(
            work_id,
            2,
            runtime_id,
            WorkCommandContext {
                event_id: format!("start-{work_id}"),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::ProviderRuntimeProjection,
                    id: runtime_id.into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("start-{work_id}"),
                created_at: "t3".into(),
                duplicate_ok: false,
            },
        )
        .expect("start Work");
    team_id
}

fn seed_team_work_from_run(store: &HarnessStore, run: &AgentTeamRun, work_id: &str) -> String {
    let actor = store
        .exact_team_run_host_actor(&run.id)
        .expect("resolve exact fixture Host");
    store
        .insert_work(
            Work {
                id: work_id.into(),
                team_run_id: run.id.clone(),
                accountable_team_id: None,
                assignee_membership_id: None,
                created_by_member_id: None,
                parent_work_id: None,
                title: format!("Trust fixture {work_id}"),
                context_markdown: "trust test".into(),
                completion_criteria_markdown: "done".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: actor.clone(),
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            WorkCommandContext {
                event_id: format!("event-{work_id}"),
                performed_by_actor: actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("create-{work_id}"),
                created_at: "t1".into(),
                duplicate_ok: false,
            },
        )
        .expect("seed active Team-scoped Work");
    run.agent_team_id.clone()
}

fn create_member_and_run(
    store: &HarnessStore,
    creator: &ActorRef,
    team_run_id: &str,
    member_id: &str,
    run_id: &str,
    resumable: bool,
) -> MemberRun {
    if !store
        .trust_agent_members(SPACE)
        .expect("read AgentMembers")
        .iter()
        .any(|candidate| candidate.id == member_id)
    {
        store
            .create_trust_agent_member(
                &context(
                    creator.clone(),
                    "member.create",
                    &format!("create-{member_id}"),
                    0,
                ),
                member(member_id, creator),
            )
            .expect("create member");
    }
    let run = member_run(run_id, member_id, team_run_id, resumable);
    let runtime = runtime_member_run(&run, &format!("Member {member_id}"));
    admit_existing_member_run(store, creator, run.clone(), runtime)
        .expect("admit current Team Member atomically");
    run
}

fn admit_existing_member_run(
    store: &HarnessStore,
    creator: &ActorRef,
    run: MemberRun,
    runtime: RuntimeMemberRun,
) -> Result<(), StoreError> {
    let expected = store
        .team_runs()
        .expect("read TeamRun")
        .into_iter()
        .rev()
        .find(|candidate| candidate.id == run.team_run_id)
        .expect("TeamRun");
    let mut next = expected.clone();
    next.member_run_ids.push(run.id.clone());
    next.updated_at = format!("t-admit-{}", next.member_run_ids.len());
    store.admit_member_run_with_canonical(
        &expected,
        &next,
        &runtime,
        SPACE,
        &firm_store::CanonicalMemberRunAdmission {
            context: context(
                creator.clone(),
                "member_run.create",
                &format!("create-{}", run.id),
                0,
            ),
            run: run.clone(),
        },
    )
}

fn runtime_member_run(run: &MemberRun, name: &str) -> RuntimeMemberRun {
    RuntimeMemberRun {
        id: run.id.clone(),
        team_run_id: run.team_run_id.clone(),
        slot_id: None,
        agent_member_id: run.agent_member_id.clone(),
        name: name.to_string(),
        role: run.role_snapshot.clone(),
        provider: "codex".into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: firm_core::MemberCoordinationStatus::Active,
        runtime_generation: run.runtime_generation,
        status: MemberRunStatus::Idle,
        native_session: run.native_session.as_ref().map(|session| {
            serde_json::from_value(serde_json::to_value(session).expect("serialize native session"))
                .expect("map native session")
        }),
        provider_cwd_hint: None,
        provider_environment_observation: None,
        owned_paths: Vec::new(),
        zero_output_streak: 0,
        last_consumed_work_version: None,
        started_at: run.started_at.clone(),
        last_event_at: run.last_event_at.clone(),
        finished_at: run.finished_at.clone(),
    }
}

#[cfg(any())]
fn message(id: &str, team_run_id: &str, sender: &ActorRef, recipients: &[&str]) -> TeamMessage {
    TeamMessage {
        id: id.into(),
        team_run_id: team_run_id.into(),
        work_id: None,
        sender: sender.clone(),
        recipients: recipients.iter().map(|id| member_actor(id)).collect(),
        kind: TeamMessageKind::Message,
        body: "perform the assigned work".into(),
        correlation_id: format!("corr-{id}"),
        causation_id: None,
        response_intent: ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        created_at: "t2".into(),
    }
}

fn delivery_claim(id: &str, supervisor_generation: u64, member_generation: u64) -> DeliveryClaim {
    DeliveryClaim {
        claim_id: id.into(),
        supervisor_generation,
        member_generation,
        claim_expires_at: "t99".into(),
    }
}

fn report(id: &str, kind: WorkReportKind, author: &ActorRef) -> WorkReport {
    WorkReport {
        id: id.into(),
        work_id: "work-1".into(),
        work_revision: 3,
        report_revision: 1,
        kind,
        authored_by: author.clone(),
        summary: "report".into(),
        base_revision: None,
        candidate: None,
        candidate_fingerprint: None,
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        evidence_refs: Vec::new(),
        known_risks: Vec::new(),
        confidence: Some(Confidence::High),
        recommended_next_action: None,
        created_at: "t1".into(),
    }
}

fn requirement(id: &str) -> GateRequirement {
    let evaluator_ref = member_actor("critic");
    let evaluator_version = "v1".to_string();
    GateRequirement {
        id: id.into(),
        work_id: "work-gate".into(),
        work_revision: 1,
        work_report_id: "report-gate".into(),
        candidate_fingerprint: "sha256:candidate".into(),
        source: GateRequirementSource::Direct,
        source_binding_id: None,
        gate_type: "test".into(),
        gate_contract_version: "1".into(),
        evaluator_fingerprint: canonical_json_fingerprint(&serde_json::json!({
            "actor": evaluator_ref,
            "version": evaluator_version,
        })),
        evaluator_ref,
        evaluator_version,
        resolved_config: serde_json::json!({"strict": true}),
        config_fingerprint: "sha256:config".into(),
        required: true,
        dependency_requirement_ids: Vec::new(),
        requirement_set_fingerprint: canonical_json_fingerprint(&serde_json::json!([id])),
        created_at: "t1".into(),
        version: 1,
    }
}

fn evaluation(id: &str, requirement_id: &str, evaluator: &ActorRef) -> GateEvaluation {
    let evaluator_version = "v1".to_string();
    GateEvaluation {
        id: id.into(),
        requirement_id: requirement_id.into(),
        work_id: "work-gate".into(),
        work_revision: 1,
        work_report_id: "report-gate".into(),
        candidate_fingerprint: "sha256:candidate".into(),
        config_fingerprint: "sha256:config".into(),
        evaluator_fingerprint: canonical_json_fingerprint(&serde_json::json!({
            "actor": evaluator,
            "version": evaluator_version,
        })),
        evaluator_version,
        dependency_fingerprint: canonical_json_fingerprint(&serde_json::json!([])),
        verdict: GateVerdict::Passed,
        summary: "passed".into(),
        evidence_refs: vec!["evidence://gate".into()],
        performed_by: evaluator.clone(),
        evaluated_at: "t2".into(),
        version: 1,
    }
}

fn workspace_binding(id: &str, root: &str, creator: &ActorRef) -> MemberWorkspaceBinding {
    MemberWorkspaceBinding {
        id: id.into(),
        project_binding_id: "project-test".into(),
        team_run_id: "team-run".into(),
        member_run_id: "member-run".into(),
        work_id: Some("work-1".into()),
        mode: WorkspaceMode::Worktree,
        ownership: WorkspaceOwnership::Managed,
        canonical_root: root.into(),
        git_common_dir: None,
        base_ref: Some("base".into()),
        git_head: None,
        git_branch: Some("codex/test".into()),
        dirty_fingerprint: None,
        instruction_roots: Vec::new(),
        skill_roots: Vec::new(),
        lifecycle: WorkspaceLifecycle::Requested,
        blocked_reason: None,
        attached_member_generation: Some(1),
        version: 1,
        created_by: creator.clone(),
        created_at: "t1".into(),
        updated_at: "t1".into(),
    }
}

#[path = "member_execution_trust/canonical_acceptance_rolls_up_delegation_in_the_same_operation.rs"]
mod canonical_acceptance_rolls_up_delegation_in_the_same_operation;
#[path = "member_execution_trust/canonical_ledger_recovers_old_torn_tail_and_ignores_uncommitted_next_file.rs"]
mod canonical_ledger_recovers_old_torn_tail_and_ignores_uncommitted_next_file;
#[cfg(any())]
#[path = "member_execution_trust/close_reopen_and_retire_fence_queued_delivery_by_generation.rs"]
mod close_reopen_and_retire_fence_queued_delivery_by_generation;
#[path = "member_execution_trust/current_team_member_lifecycle_updates_both_projections_and_fences_foreign_space.rs"]
mod current_team_member_lifecycle_updates_both_projections_and_fences_foreign_space;
#[cfg(any())]
#[path = "member_execution_trust/delivery_claim_and_receipt_are_generation_fenced_and_reconcile_is_explicit.rs"]
mod delivery_claim_and_receipt_are_generation_fenced_and_reconcile_is_explicit;
#[path = "member_execution_trust/exact_result_report_submits_and_accepts_work_in_canonical_operations.rs"]
mod exact_result_report_submits_and_accepts_work_in_canonical_operations;
#[cfg(any())]
#[path = "member_execution_trust/fanout_is_atomic_and_creates_exactly_one_delivery_per_recipient.rs"]
mod fanout_is_atomic_and_creates_exactly_one_delivery_per_recipient;
#[path = "member_execution_trust/gates_require_exact_evaluation_or_authorized_waiver_and_reject_self_cycles.rs"]
mod gates_require_exact_evaluation_or_authorized_waiver_and_reject_self_cycles;
#[path = "member_execution_trust/idempotency_is_scoped_payload_exact_and_cas_protected.rs"]
mod idempotency_is_scoped_payload_exact_and_cas_protected;
#[cfg(any())]
#[path = "member_execution_trust/linked_team_messages_reject_unknown_and_cross_team_work_without_side_effects.rs"]
mod linked_team_messages_reject_unknown_and_cross_team_work_without_side_effects;
#[path = "member_execution_trust/member_owned_work_records_require_the_exact_active_execution_binding.rs"]
mod member_owned_work_records_require_the_exact_active_execution_binding;
#[path = "member_execution_trust/paused_and_retired_members_cannot_start_runs.rs"]
mod paused_and_retired_members_cannot_start_runs;
#[path = "member_execution_trust/pre_cutover_member_run_materialization_tolerance_is_field_generic.rs"]
mod pre_cutover_member_run_materialization_tolerance_is_field_generic;
#[path = "member_execution_trust/pre_cutover_member_run_without_canonical_last_event_at_still_materializes.rs"]
mod pre_cutover_member_run_without_canonical_last_event_at_still_materializes;
#[path = "member_execution_trust/result_and_failure_reports_require_their_risk_evidence.rs"]
mod result_and_failure_reports_require_their_risk_evidence;
#[path = "member_execution_trust/stale_work_revision_requires_current_supervisor_before_invalidation.rs"]
mod stale_work_revision_requires_current_supervisor_before_invalidation;
#[cfg(any())]
#[path = "member_execution_trust/successor_supervisor_fences_stale_claim_before_any_canonical_side_effect.rs"]
mod successor_supervisor_fences_stale_claim_before_any_canonical_side_effect;
#[path = "member_execution_trust/workspace_binding_rejects_relative_and_parent_traversal_without_side_effects.rs"]
mod workspace_binding_rejects_relative_and_parent_traversal_without_side_effects;
#[path = "member_execution_trust/workspace_binding_requires_exact_member_run_and_project_placement.rs"]
mod workspace_binding_requires_exact_member_run_and_project_placement;
#[path = "member_execution_trust/workspace_transitions_reobserve_git_links_dirty_state_and_cleanup_safety.rs"]
mod workspace_transitions_reobserve_git_links_dirty_state_and_cleanup_safety;
