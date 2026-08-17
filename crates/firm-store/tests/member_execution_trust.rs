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
        .insert_mission(&Mission {
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
    let team = AgentTeam {
        id: team_id.clone(),
        name: "trust team".into(),
        description: "trust fixture".into(),
        legacy_mission_id: Some(mission_id.clone()),
        mission_id,
        host_agent_id: member_ids[0].into(),
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
        host_actor: None,
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
    let actor = TeamActorRef {
        kind: TeamActorKind::Host,
        id: "host".into(),
        display_name: None,
        authn_source: Some("test".into()),
    };
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
    let host = TeamActorRef {
        kind: TeamActorKind::Host,
        id: "host".into(),
        display_name: None,
        authn_source: Some("test".into()),
    };
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
    let actor = TeamActorRef {
        kind: TeamActorKind::Host,
        id: "host".into(),
        display_name: None,
        authn_source: Some("test".into()),
    };
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

#[test]
fn current_team_member_lifecycle_updates_both_projections_and_fences_foreign_space() {
    let harness = TestStore::new("combined-member-lifecycle");
    let host = human("host");
    let team_run = seed_team(&harness.store, "combined-member-lifecycle", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        true,
    );

    let legacy_before = harness.store.member_runs().expect("read runtime rows");
    let canonical_before = harness
        .store
        .trust_member_runs(SPACE)
        .expect("read canonical rows");
    let operations_before = harness
        .store
        .canonical_operations()
        .expect("read canonical operations");
    let mut foreign = context(host.clone(), "member_run.close", "foreign-close", 1);
    foreign.execution_space_id = "foreign-space".into();
    let error = harness
        .store
        .transition_current_team_member_lifecycle(
            &foreign,
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Close,
            "t2",
        )
        .expect_err("caller-selected foreign Execution Space must fail closed");
    assert!(error.to_string().contains("EXECUTION_SPACE_SCOPE_MISMATCH"));
    assert_eq!(harness.store.member_runs().unwrap(), legacy_before);
    assert_eq!(
        harness.store.trust_member_runs(SPACE).unwrap(),
        canonical_before
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_before
    );

    let close_context = context(host.clone(), "member_run.close", "close-current", 1);
    let closed = harness
        .store
        .transition_current_team_member_lifecycle(
            &close_context,
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Close,
            "t2",
        )
        .expect("close current Team Member");
    assert_eq!(
        closed.runtime_projection.coordination_status,
        firm_core::MemberCoordinationStatus::Closed
    );
    assert_eq!(closed.runtime_projection.status, MemberRunStatus::Stopped);
    assert_eq!(
        closed.canonical.projection.coordination_status,
        MemberCoordinationStatus::Closed
    );
    assert_eq!(
        closed.canonical.projection.runtime_status,
        MemberRuntimeStatus::Stopped
    );

    let legacy_after_close = harness.store.member_runs().unwrap();
    let operations_after_close = harness.store.canonical_operations().unwrap();
    let close_replay = harness
        .store
        .transition_current_team_member_lifecycle(
            &close_context,
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Close,
            "t2",
        )
        .expect("exact close retry replays");
    assert!(close_replay.canonical.replayed);
    assert_eq!(harness.store.member_runs().unwrap(), legacy_after_close);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_close
    );

    let closed_resume_error = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(
                host.clone(),
                "member_run.resume_native_session",
                "resume-closed",
                2,
            ),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession,
            "t3",
        )
        .expect_err("ResumeNativeSession must not impersonate Reopen");
    assert_eq!(
        trust_code(closed_resume_error),
        TrustErrorCode::InvalidStateTransition
    );
    assert_eq!(harness.store.member_runs().unwrap(), legacy_after_close);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_close
    );

    let stale_version_error = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(host.clone(), "member_run.reopen", "reopen-stale-version", 1),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Reopen,
            "t3",
        )
        .expect_err("stale canonical CAS must reject before either ledger changes");
    assert_eq!(
        trust_code(stale_version_error),
        TrustErrorCode::VersionConflict
    );
    assert_eq!(harness.store.member_runs().unwrap(), legacy_after_close);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_close
    );

    let reopened = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(host.clone(), "member_run.reopen", "reopen-current", 2),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Reopen,
            "t3",
        )
        .expect("reopen current Team Member");
    assert_eq!(reopened.runtime_projection.runtime_generation, 2);
    assert_eq!(reopened.runtime_projection.status, MemberRunStatus::Queued);
    assert_eq!(reopened.canonical.projection.runtime_generation, 2);

    let mut disconnected = reopened.runtime_projection.clone();
    disconnected.status = MemberRunStatus::Disconnected;
    disconnected.last_event_at = Some("t4".into());
    harness
        .store
        .compare_and_append_member_run(&reopened.runtime_projection, &disconnected)
        .expect("record active provider transport loss");
    let resumed = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(
                host.clone(),
                "member_run.resume_native_session",
                "resume-current",
                4,
            ),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession,
            "t5",
        )
        .expect("resume the active Team Member native session");
    assert_eq!(resumed.runtime_projection.runtime_generation, 2);
    assert_eq!(resumed.runtime_projection.status, MemberRunStatus::Starting);
    assert_eq!(resumed.canonical.projection.runtime_generation, 2);
    assert_eq!(
        resumed.canonical.projection.runtime_status,
        MemberRuntimeStatus::Starting
    );
    assert_eq!(
        resumed.canonical.projection.native_session,
        resumed
            .runtime_projection
            .native_session
            .as_ref()
            .map(|session| serde_json::from_value(serde_json::to_value(session).unwrap()).unwrap())
    );
    let runtime_after_resume = harness.store.member_runs().unwrap();
    let operations_after_resume = harness.store.canonical_operations().unwrap();
    let resume_replay = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(
                host.clone(),
                "member_run.resume_native_session",
                "resume-current",
                4,
            ),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession,
            "t5",
        )
        .expect("exact resume retry replays");
    assert!(resume_replay.canonical.replayed);
    assert_eq!(harness.store.member_runs().unwrap(), runtime_after_resume);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_resume
    );

    let retired = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(host, "member_run.retire", "retire-current", 5),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Retire,
            "t6",
        )
        .expect("retire current Team Member");
    assert_eq!(
        retired.runtime_projection.coordination_status,
        firm_core::MemberCoordinationStatus::Retired
    );
    assert_eq!(
        retired.canonical.projection.coordination_status,
        MemberCoordinationStatus::Retired
    );
    assert_eq!(
        retired.runtime_projection.finished_at.as_deref(),
        Some("t6")
    );
    assert_eq!(
        retired.canonical.projection.finished_at.as_deref(),
        Some("t6")
    );
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

#[test]
fn idempotency_is_scoped_payload_exact_and_cas_protected() {
    let harness = TestStore::new("idempotency-cas");
    let host = human("host");
    let request = member("member-a", &host);
    let create = context(host.clone(), "member.create", "same-key", 0);

    let first = harness
        .store
        .create_trust_agent_member(&create, request.clone())
        .expect("first create");
    let replay = harness
        .store
        .create_trust_agent_member(&create, request.clone())
        .expect("exact replay");
    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(first.event.id, replay.event.id);
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 1);

    let mut drifted = request.clone();
    drifted.description = "different payload".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_agent_member(&create, drifted)
                .expect_err("payload drift must fail")
        ),
        TrustErrorCode::IdempotencyKeyReused
    );

    let wrong_cas = context(host.clone(), "member.pause", "pause-wrong-cas", 99);
    assert_eq!(
        trust_code(
            harness
                .store
                .transition_trust_agent_member(
                    &wrong_cas,
                    "member-a",
                    AgentMemberOrganizationStatus::Paused,
                    "t2",
                )
                .expect_err("stale CAS must fail")
        ),
        TrustErrorCode::VersionConflict
    );

    let other_actor = human("other-host");
    harness
        .store
        .create_trust_agent_member(
            &context(other_actor.clone(), "member.create", "same-key", 0),
            member("member-b", &other_actor),
        )
        .expect("same key is scoped by actor");
    let mut other_space = context(host.clone(), "member.create", "same-key", 0);
    other_space.execution_space_id = "space-other".into();
    harness
        .store
        .create_trust_agent_member(&other_space, member("member-c", &host))
        .expect("same key is scoped by execution space");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 3);
}

#[test]
fn canonical_ledger_recovers_old_torn_tail_and_ignores_uncommitted_next_file() {
    let harness = TestStore::new("canonical-crash-recovery");
    let host = human("host");
    harness
        .store
        .create_trust_agent_member(
            &context(host.clone(), "member.create", "create-a", 0),
            member("member-a", &host),
        )
        .expect("commit first canonical operation");

    let ledger = harness.root.join("agentfirm_trust_operations.jsonl");
    let next = harness.root.join("agentfirm_trust_operations.jsonl.next");
    std::fs::write(&next, b"{\"uncommitted\":").expect("simulate crash before rename");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 1);

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .expect("open canonical ledger");
    file.write_all(b"{\"torn\":")
        .expect("simulate legacy append tear");
    file.sync_all().expect("persist torn tail");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 1);

    harness
        .store
        .create_trust_agent_member(
            &context(host.clone(), "member.create", "create-b", 0),
            member("member-b", &host),
        )
        .expect("next commit atomically replaces torn ledger");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 2);
    let repaired = std::fs::read(&ledger).expect("read repaired ledger");
    assert!(repaired.ends_with(b"\n"));
    assert!(!next.exists(), "atomic rename consumes the next file");
    for row in repaired.split(|byte| *byte == b'\n') {
        if !row.is_empty() {
            serde_json::from_slice::<serde_json::Value>(row).expect("complete JSON frame");
        }
    }
}

#[test]
fn paused_and_retired_members_cannot_start_runs() {
    let harness = TestStore::new("member-status");
    let host = human("host");
    let team_run = seed_team(&harness.store, "status", &["paused", "retired"]);
    // seed_team already created both durable AgentMembers; pause/retire them.
    harness
        .store
        .transition_trust_agent_member(
            &context(host.clone(), "member.pause", "pause", 1),
            "paused",
            AgentMemberOrganizationStatus::Paused,
            "t2",
        )
        .expect("pause member");
    assert_eq!(
        trust_code(
            admit_existing_member_run(
                &harness.store,
                &host,
                member_run("run-paused", "paused", &team_run.id, false),
                runtime_member_run(
                    &member_run("run-paused", "paused", &team_run.id, false),
                    "Paused",
                ),
            )
            .expect_err("paused member cannot run")
        ),
        TrustErrorCode::AgentMemberPaused
    );
    harness
        .store
        .transition_trust_agent_member(
            &context(host.clone(), "member.retire", "retire", 1),
            "retired",
            AgentMemberOrganizationStatus::Retired,
            "t2",
        )
        .expect("retire member");
    assert_eq!(
        trust_code(
            admit_existing_member_run(
                &harness.store,
                &host,
                member_run("run-retired", "retired", &team_run.id, false),
                runtime_member_run(
                    &member_run("run-retired", "retired", &team_run.id, false),
                    "Retired",
                ),
            )
            .expect_err("retired member cannot run")
        ),
        TrustErrorCode::AgentMemberRetired
    );
}

#[test]
#[cfg(any())]
fn fanout_is_atomic_and_creates_exactly_one_delivery_per_recipient() {
    let harness = TestStore::new("fanout");
    let host = human("host");
    let team_run = seed_team(&harness.store, "fanout", &["member-a", "member-b"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        false,
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-b",
        "run-b",
        false,
    );

    let before = harness.store.canonical_operations().unwrap().len();
    let invalid = message(
        "message-invalid",
        &team_run.id,
        &host,
        &["member-a", "missing-member"],
    );
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_team_message_with_deliveries(
                    &context(host.clone(), "message.create", "invalid-fanout", 0),
                    invalid,
                    "t2",
                )
                .expect_err("unresolvable fanout must fail atomically")
        ),
        TrustErrorCode::InvalidStateTransition
    );
    assert_eq!(harness.store.canonical_operations().unwrap().len(), before);
    assert!(harness
        .store
        .trust_message_deliveries(SPACE)
        .unwrap()
        .is_empty());

    let valid = message(
        "message-valid",
        &team_run.id,
        &host,
        &["member-a", "member-b"],
    );
    let result = harness
        .store
        .create_trust_team_message_with_deliveries(
            &context(host, "message.create", "valid-fanout", 0),
            valid,
            "t3",
        )
        .expect("valid fanout");
    assert_eq!(result.event.aggregate_kind, "team_message");
    let deliveries = harness.store.trust_message_deliveries(SPACE).unwrap();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(
        deliveries
            .iter()
            .map(|delivery| delivery.recipient_member_run_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        ["run-a", "run-b"].into_iter().collect()
    );
    assert_eq!(
        harness
            .store
            .canonical_operations()
            .unwrap()
            .last()
            .unwrap()
            .initial_outbox_records
            .len(),
        2
    );
}

#[test]
#[cfg(any())]
fn linked_team_messages_reject_unknown_and_cross_team_work_without_side_effects() {
    let harness = TestStore::new("linked-message-scope");
    let host = human("host");
    let team_run = seed_team(&harness.store, "linked-source", &["member-a", "member-b"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "run-a",
        false,
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-b",
        "run-b",
        false,
    );
    seed_team_work(&harness.store, "linked-other-team", "other-team-work");
    let before = harness.store.canonical_operations().unwrap().len();
    for (id, work_id) in [
        ("unknown-link", "missing-work"),
        ("cross-team-link", "other-team-work"),
    ] {
        let mut linked = message(id, &team_run.id, &member_actor("member-a"), &["member-b"]);
        linked.work_id = Some(work_id.into());
        assert!(harness
            .store
            .create_trust_team_message_with_deliveries(
                &context(member_actor("member-a"), "message.create", id, 0),
                linked,
                "t4",
            )
            .is_err());
    }
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before,
        "unknown/cross-Team Work linkage must have zero canonical side effects"
    );
}

#[test]
#[cfg(any())]
fn close_reopen_and_retire_fence_queued_delivery_by_generation() {
    let harness = TestStore::new("run-lifecycle");
    let host = human("host");
    let team_run = seed_team(&harness.store, "lifecycle", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        true,
    );
    harness
        .store
        .create_trust_team_message_with_deliveries(
            &context(host.clone(), "message.create", "queue-message", 0),
            message("message-a", &team_run.id, &host, &["member-a"]),
            "t2",
        )
        .expect("queue message");

    let closed = harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.close", "close", 1),
            "run-a",
            MemberCoordinationStatus::Closed,
            "t3",
        )
        .expect("close run")
        .projection;
    assert_eq!(closed.runtime_generation, 1);
    assert_eq!(closed.runtime_status, MemberRuntimeStatus::Stopped);
    assert_eq!(
        harness.store.trust_message_deliveries(SPACE).unwrap()[0].freeze_generation,
        Some(1)
    );

    let reopened = harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.reopen", "reopen", 2),
            "run-a",
            MemberCoordinationStatus::Active,
            "t4",
        )
        .expect("reopen resumable run")
        .projection;
    assert_eq!(reopened.runtime_generation, 2);
    assert_eq!(reopened.runtime_status, MemberRuntimeStatus::Idle);

    let retired = harness
        .store
        .transition_trust_member_run(
            &context(host, "member_run.retire", "retire-run", 3),
            "run-a",
            MemberCoordinationStatus::Retired,
            "t5",
        )
        .expect("retire run")
        .projection;
    assert_eq!(retired.runtime_generation, 2);
    assert_eq!(retired.runtime_status, MemberRuntimeStatus::Stopped);
    assert_eq!(retired.finished_at.as_deref(), Some("t5"));
    let delivery = &harness.store.trust_message_deliveries(SPACE).unwrap()[0];
    assert_eq!(
        delivery.status,
        firm_core::agentfirm_api::MessageDeliveryStatus::Invalidated
    );
    assert_eq!(delivery.version, 3);
}

fn delivery_claim(id: &str, supervisor_generation: u64, member_generation: u64) -> DeliveryClaim {
    DeliveryClaim {
        claim_id: id.into(),
        supervisor_generation,
        member_generation,
        claim_expires_at: "t99".into(),
    }
}

#[test]
#[cfg(any())]
fn delivery_claim_and_receipt_are_generation_fenced_and_reconcile_is_explicit() {
    let harness = TestStore::new("delivery-generation");
    let host = human("host");
    let team_run = seed_team(&harness.store, "delivery-generation", &["member-a"]);
    let supervisor = acquire_supervisor(&harness.store, &team_run, "supervisor-a");
    let supervisor_actor = service(&supervisor.supervisor_id);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "run-a",
        true,
    );
    for id in ["old", "uncertain", "queued"] {
        harness
            .store
            .create_trust_team_message_with_deliveries(
                &context(host.clone(), "message.create", &format!("message-{id}"), 0),
                message(&format!("message-{id}"), &team_run.id, &host, &["member-a"]),
                "t2",
            )
            .expect("queue delivery");
    }

    let stale_before = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_message_delivery(
                    &context(supervisor_actor.clone(), "delivery.claim", "stale-claim", 0),
                    "message-old:run-a",
                    delivery_claim("claim-stale", supervisor.generation, 0),
                    "t3",
                )
                .expect_err("stale generation cannot claim")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        stale_before
    );

    for (delivery, claim_id) in [
        ("message-old:run-a", "claim-old"),
        ("message-uncertain:run-a", "claim-uncertain"),
    ] {
        harness
            .store
            .claim_trust_message_delivery(
                &context(
                    supervisor_actor.clone(),
                    "delivery.claim",
                    &format!("key-{claim_id}"),
                    0,
                ),
                delivery,
                delivery_claim(claim_id, supervisor.generation, 1),
                "t3",
            )
            .expect("claim at generation one");
    }
    harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.close", "delivery-close", 1),
            "run-a",
            MemberCoordinationStatus::Closed,
            "t4",
        )
        .expect("close run");
    harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.reopen", "delivery-reopen", 2),
            "run-a",
            MemberCoordinationStatus::Active,
            "t5",
        )
        .expect("reopen at generation two");

    let receipt = ProviderReceipt {
        claim_id: "claim-old".into(),
        supervisor_generation: supervisor.generation,
        member_generation: 1,
        provider_receipt_id: "provider-old".into(),
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .receive_trust_message_delivery(
                    &context(
                        supervisor_actor.clone(),
                        "delivery.receive",
                        "stale-receipt",
                        1
                    ),
                    "message-old:run-a",
                    receipt,
                    "t6",
                )
                .expect_err("old-generation receipt must be fenced")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );

    let reconciled = harness
        .store
        .reconcile_trust_message_delivery(
            &context(host.clone(), "delivery.reconcile", "explicit-reconcile", 1),
            "message-uncertain:run-a",
            DeliveryReconcileOutcome::RetrySafeFailure,
            "evidence://provider-query",
            "t7",
        )
        .expect("uncertain old claim requires explicit evidence-backed reconciliation")
        .projection;
    assert_eq!(reconciled.status, MessageDeliveryStatus::Failed);
    assert_eq!(
        reconciled.failure_detail.as_deref(),
        Some("evidence://provider-query")
    );

    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_message_delivery(
                    &context(
                        supervisor_actor.clone(),
                        "delivery.claim",
                        "queued-old-generation",
                        0
                    ),
                    "message-queued:run-a",
                    delivery_claim("claim-queued-stale", supervisor.generation, 1),
                    "t8",
                )
                .expect_err("frozen queued delivery cannot use old generation")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    harness
        .store
        .claim_trust_message_delivery(
            &context(
                supervisor_actor.clone(),
                "delivery.claim",
                "queued-new-generation",
                0,
            ),
            "message-queued:run-a",
            delivery_claim("claim-queued", supervisor.generation, 2),
            "t8",
        )
        .expect("new generation may claim frozen delivery");
    harness
        .store
        .receive_trust_message_delivery(
            &context(
                supervisor_actor.clone(),
                "delivery.receive",
                "fresh-receipt",
                1,
            ),
            "message-queued:run-a",
            ProviderReceipt {
                claim_id: "claim-queued".into(),
                supervisor_generation: supervisor.generation,
                member_generation: 2,
                provider_receipt_id: "provider-fresh".into(),
            },
            "t9",
        )
        .expect("matching receipt");
    let acknowledged = harness
        .store
        .acknowledge_trust_message_delivery(
            &context(supervisor_actor, "delivery.ack", "fresh-ack", 2),
            "message-queued:run-a",
            "claim-queued",
            2,
            "t10",
        )
        .expect("matching acknowledgement")
        .projection;
    assert_eq!(acknowledged.status, MessageDeliveryStatus::Acknowledged);
}

#[test]
#[cfg(any())]
fn successor_supervisor_fences_stale_claim_before_any_canonical_side_effect() {
    let harness = TestStore::new("delivery-supervisor-successor");
    let host = human("host");
    let team_run = seed_team(
        &harness.store,
        "delivery-supervisor-successor",
        &["member-a"],
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "run-a",
        true,
    );
    harness
        .store
        .create_trust_team_message_with_deliveries(
            &context(host, "message.create", "message-successor", 0),
            message(
                "message-successor",
                &team_run.id,
                &human("host"),
                &["member-a"],
            ),
            "t2",
        )
        .expect("queue delivery");
    let first = acquire_supervisor(&harness.store, &team_run, "supervisor-old");
    harness
        .store
        .release_team_supervisor_lease(
            &team_run.id,
            &first.supervisor_id,
            first.generation,
            unix_ms(),
        )
        .expect("release old supervisor");
    let successor = acquire_supervisor(&harness.store, &team_run, "supervisor-successor");
    assert!(successor.generation > first.generation);

    let before = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_message_delivery(
                    &context(
                        service(&first.supervisor_id),
                        "delivery.claim",
                        "stale-supervisor-claim",
                        0,
                    ),
                    "message-successor:run-a",
                    delivery_claim("claim-stale-supervisor", first.generation, 1),
                    "t3",
                )
                .expect_err("successor acquisition must fence old supervisor")
        ),
        TrustErrorCode::SupervisorGenerationFenced
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before,
        "stale Supervisor loses at the same Store lock before provider-visible state"
    );
    harness
        .store
        .claim_trust_message_delivery(
            &context(
                service(&successor.supervisor_id),
                "delivery.claim",
                "successor-claim",
                0,
            ),
            "message-successor:run-a",
            delivery_claim("claim-successor", successor.generation, 1),
            "t4",
        )
        .expect("current successor can claim");
}

#[test]
fn stale_work_revision_requires_current_supervisor_before_invalidation() {
    let harness = TestStore::new("work-delivery-stale-supervisor-order");
    let host = human("host");
    let team_run = seed_team(
        &harness.store,
        "work-delivery-stale-supervisor-order",
        &["member-a"],
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        true,
    );
    seed_team_work_from_run(&harness.store, &team_run, "work-stale");
    harness
        .store
        .create_trust_work_deliveries(
            &context(host, "work_delivery.create", "work-stale-delivery", 0),
            "work-event-stale",
            "work-stale",
            1,
            &["runtime-member-a".into()],
            "t2",
        )
        .expect("queue canonical WorkDelivery");

    let old = acquire_supervisor(&harness.store, &team_run, "supervisor-old");
    harness
        .store
        .release_team_supervisor_lease(&team_run.id, &old.supervisor_id, old.generation, unix_ms())
        .expect("release old Supervisor");
    let current = acquire_supervisor(&harness.store, &team_run, "supervisor-current");
    assert!(current.generation > old.generation);

    let delivery_before = harness.store.trust_work_deliveries(SPACE).unwrap();
    let member_before = harness.store.trust_member_runs(SPACE).unwrap();
    let work_before = harness.store.latest_works().unwrap();
    let provider_before = harness.store.latest_work_deliveries().unwrap();
    let operation_count_before = harness.store.canonical_operations().unwrap().len();

    for (actor, generation, claim_id) in [
        (
            service(&old.supervisor_id),
            old.generation,
            "stale-generation",
        ),
        (
            service("unauthorized-supervisor"),
            current.generation,
            "unauthorized-service",
        ),
    ] {
        assert_eq!(
            trust_code(
                harness
                    .store
                    .claim_trust_work_delivery(
                        &context(actor, "work_delivery.claim", claim_id, 0),
                        "work-event-stale:runtime-member-a",
                        delivery_claim(claim_id, generation, 1),
                        2,
                        "t3",
                    )
                    .expect_err("non-current Supervisor must lose before invalidation")
            ),
            TrustErrorCode::SupervisorGenerationFenced
        );
        assert_eq!(
            harness.store.canonical_operations().unwrap().len(),
            operation_count_before,
            "rejected Supervisor must append no CanonicalOperation"
        );
        assert_eq!(
            harness.store.trust_work_deliveries(SPACE).unwrap(),
            delivery_before,
            "rejected Supervisor must not invalidate or otherwise mutate WorkDelivery"
        );
        assert_eq!(
            harness.store.trust_member_runs(SPACE).unwrap(),
            member_before,
            "rejected Supervisor must not mutate MemberRun"
        );
        assert_eq!(
            harness.store.latest_works().unwrap(),
            work_before,
            "rejected Supervisor must not mutate Work"
        );
        assert_eq!(
            harness.store.latest_work_deliveries().unwrap(),
            provider_before,
            "rejected Supervisor must create no provider dispatch side effect"
        );
    }

    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_work_delivery(
                    &context(
                        service(&current.supervisor_id),
                        "work_delivery.claim",
                        "current-invalidates-stale",
                        0,
                    ),
                    "work-event-stale:runtime-member-a",
                    delivery_claim("current-invalidates-stale", current.generation, 1,),
                    2,
                    "t4",
                )
                .expect_err("current Supervisor intentionally invalidates stale Work revision")
        ),
        TrustErrorCode::WorkRevisionStale
    );
    let invalidated = harness.store.trust_work_deliveries(SPACE).unwrap();
    assert_eq!(invalidated.len(), 1);
    assert_eq!(
        invalidated[0].status,
        firm_core::agentfirm_api::WorkDeliveryStatus::Invalidated
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        operation_count_before + 1,
        "only the authorized current Supervisor may persist stale-revision invalidation"
    );
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

#[test]
fn result_and_failure_reports_require_their_risk_evidence() {
    let harness = TestStore::new("reports");
    let team_id = seed_active_team_work(&harness.store, "reports", "work-1");
    let worker = member_actor("worker");
    let before = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "result-missing", 0),
                    &team_id,
                    {
                        let mut report = report("result-missing", WorkReportKind::Result, &worker);
                        report.work_revision = 4;
                        report
                    },
                )
                .expect_err("result without candidate evidence must fail")
        ),
        TrustErrorCode::ReportEvidenceMissing
    );
    assert_eq!(harness.store.canonical_operations().unwrap().len(), before);

    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "failure-missing", 0),
                    &team_id,
                    report("failure-missing", WorkReportKind::Failure, &worker),
                )
                .expect_err("failure without analysis must fail")
        ),
        TrustErrorCode::FailureAnalysisMissing
    );
    let mut missing_reference = report("failure-missing-ref", WorkReportKind::Failure, &worker);
    missing_reference.failure_analysis_ref = Some("analysis-missing".into());
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "failure-missing-ref", 0,),
                    &team_id,
                    missing_reference,
                )
                .expect_err("failure analysis reference must resolve")
        ),
        TrustErrorCode::FailureAnalysisMissing
    );
    harness
        .store
        .create_trust_failure_analysis(
            &context(worker.clone(), "failure_analysis.create", "analysis", 0),
            &team_id,
            FailureAnalysis {
                id: "analysis-1".into(),
                work_id: "work-1".into(),
                work_revision: 3,
                member_run_id: Some("runtime-worker".into()),
                candidate: None,
                observed_failure: "provider exited".into(),
                impact: "work incomplete".into(),
                primary_cause_status: PrimaryCauseStatus::Suspected,
                primary_cause: Some("provider failure".into()),
                contributing_causes: Vec::new(),
                attempts_already_made: vec!["one retry".into()],
                last_safe_checkpoint: Some("base".into()),
                retry_safety: RetrySafety::Safe,
                side_effect_summary: Some("none".into()),
                recovery_options: vec!["resume".into()],
                recommended_host_decision: "retry".into(),
                evidence_refs: vec!["evidence://provider-log".into()],
                confidence: Confidence::Medium,
                reported_by: worker.clone(),
                created_at: "t2".into(),
            },
        )
        .expect("create failure analysis");
    let mut failure = report("failure-ok", WorkReportKind::Failure, &worker);
    failure.failure_analysis_ref = Some("analysis-1".into());
    harness
        .store
        .create_trust_work_report(
            &context(worker.clone(), "report.create", "failure-ok", 0),
            &team_id,
            failure,
        )
        .expect("failure report with analysis reference");

    let mut result = report("result-ok", WorkReportKind::Result, &worker);
    result.work_revision = 4;
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "0123456789abcdef".into(),
    };
    result.candidate_fingerprint = Some(canonical_json_fingerprint(
        &serde_json::to_value(&candidate).expect("serialize candidate"),
    ));
    result.candidate = Some(candidate);
    result.evidence_refs = vec!["evidence://checks".into()];
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "result-ok", 0),
            &team_id,
            result,
        )
        .expect("evidence-backed result atomically submits Work");
    let submitted = harness
        .store
        .latest_works()
        .expect("read Work")
        .into_iter()
        .find(|work| work.id == "work-1")
        .expect("submitted Work");
    assert_eq!(submitted.version, 4);
    assert_eq!(submitted.phase, WorkPhase::Review);
}

#[test]
fn member_owned_work_records_require_the_exact_active_execution_binding() {
    let harness = TestStore::new("exact-work-binding-records");
    let team_id = seed_active_team_work(&harness.store, "exact-binding", "work-1");
    let worker = member_actor("worker");
    harness
        .store
        .transition_current_team_member_lifecycle(
            &context(worker.clone(), "member_run.close", "close-old-run", 1),
            "runtime-worker",
            CurrentTeamMemberLifecycleTransition::Close,
            "t4",
        )
        .expect("close predecessor MemberRun");
    let before = harness.store.canonical_operations().unwrap().len();
    let mut progress = report("closed-progress", WorkReportKind::Progress, &worker);
    progress.work_revision = 3;
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "closed-progress", 0),
                    &team_id,
                    progress,
                )
                .expect_err("closed member must require an exact active Work rebind")
        ),
        TrustErrorCode::UnauthorizedActor
    );
    let finding = WorkFinding {
        id: "closed-finding".into(),
        work_id: "work-1".into(),
        work_revision: 3,
        kind: WorkFindingKind::Discovery,
        summary: "closed member cannot author before rebind".into(),
        detail_markdown: "exact active binding is no longer active".into(),
        affected_work_refs: Vec::new(),
        reusable_asset_refs: Vec::new(),
        invalidated_assumptions: Vec::new(),
        evidence_refs: Vec::new(),
        confidence: Confidence::High,
        reported_by: worker.clone(),
        created_at: "t5".into(),
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_finding(
                    &context(worker.clone(), "finding.create", "closed-finding", 0),
                    &team_id,
                    finding,
                )
                .expect_err("closed member finding must require explicit Work rebind")
        ),
        TrustErrorCode::UnauthorizedActor
    );
    let failure = FailureAnalysis {
        id: "closed-failure".into(),
        work_id: "work-1".into(),
        work_revision: 3,
        member_run_id: Some("runtime-worker".into()),
        candidate: None,
        observed_failure: "closed member attempted stale binding".into(),
        impact: "none".into(),
        primary_cause_status: PrimaryCauseStatus::Confirmed,
        primary_cause: Some("missing explicit rebind".into()),
        contributing_causes: Vec::new(),
        attempts_already_made: Vec::new(),
        last_safe_checkpoint: None,
        retry_safety: RetrySafety::Safe,
        side_effect_summary: Some("none".into()),
        recovery_options: vec!["rebind".into()],
        recommended_host_decision: "rebind explicitly".into(),
        evidence_refs: Vec::new(),
        confidence: Confidence::High,
        reported_by: worker.clone(),
        created_at: "t5".into(),
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_failure_analysis(
                    &context(worker, "failure.create", "closed-failure", 0),
                    &team_id,
                    failure,
                )
                .expect_err("closed member failure must require explicit Work rebind")
        ),
        TrustErrorCode::UnauthorizedActor
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before,
        "stale WorkExecutionBinding rejection must have zero canonical side effects"
    );
}

#[test]
fn exact_result_report_submits_and_accepts_work_in_canonical_operations() {
    let harness = TestStore::new("accept-work");
    let team_id = seed_active_team_work(&harness.store, "accept-work", "work-1");
    let worker = member_actor("worker");
    let host = human("host");
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "abcdef0123456789".into(),
    };
    let candidate_fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(&candidate).expect("serialize candidate"));
    let mut result = report("report-accept", WorkReportKind::Result, &worker);
    result.work_revision = 4;
    result.candidate = Some(candidate);
    result.candidate_fingerprint = Some(candidate_fingerprint.clone());
    result.evidence_refs = vec!["evidence://exact-candidate".into()];
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "report-accept", 0),
            &team_id,
            result,
        )
        .expect("result submission");
    let before_rejected = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .accept_trust_work(
                    &context(host.clone(), "work.accept", "accept-stale", 4),
                    &team_id,
                    "work-1",
                    "report-accept",
                    "sha256:stale",
                    "t5",
                )
                .expect_err("stale Candidate must not accept Work")
        ),
        TrustErrorCode::ReportEvidenceMissing
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_rejected,
        "rejected acceptance has zero side effects"
    );
    let command = context(host, "work.accept", "accept-exact", 4);
    let accepted = harness
        .store
        .accept_trust_work(
            &command,
            &team_id,
            "work-1",
            "report-accept",
            &candidate_fingerprint,
            "t5",
        )
        .expect("exact Candidate acceptance");
    assert_eq!(accepted.projection.phase, WorkPhase::Closed);
    assert_eq!(accepted.projection.version, 5);
    let replay = harness
        .store
        .accept_trust_work(
            &command,
            &team_id,
            "work-1",
            "report-accept",
            &candidate_fingerprint,
            "t5",
        )
        .expect("accept replay");
    assert!(replay.replayed);
    assert_eq!(replay.event.id, accepted.event.id);
}

#[test]
fn canonical_acceptance_rolls_up_delegation_in_the_same_operation() {
    let harness = TestStore::new("canonical-delegation-rollup");
    seed_active_team_work(&harness.store, "delegation-source", "source-rollup");
    let source = harness
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == "source-rollup")
        .expect("source Work");
    let target_run = seed_team(&harness.store, "delegation-target", &["target-worker"]);
    let target_runtime_id = "runtime-target-worker";
    create_member_and_run(
        &harness.store,
        &human("host"),
        &target_run.id,
        "target-worker",
        target_runtime_id,
        false,
    );
    append_legacy_projection(
        &harness.store,
        "member_runs.jsonl",
        &RuntimeMemberRun {
            id: target_runtime_id.into(),
            team_run_id: target_run.id.clone(),
            slot_id: None,
            agent_member_id: "target-worker".into(),
            name: "Target Worker".into(),
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
    let host_actor = TeamActorRef {
        kind: TeamActorKind::Host,
        id: "host".into(),
        display_name: None,
        authn_source: Some("test".into()),
    };
    let (delegation, target) = harness
        .store
        .create_work_delegation_with_target_work(
            WorkDelegation {
                id: "delegation-rollup".into(),
                source_work_ref: WorkRef {
                    team_run_id: source.team_run_id.clone(),
                    work_id: source.id.clone(),
                },
                source_work_version: source.version,
                source_owner_member_id: source.owner_member_id.clone().expect("source owner"),
                created_by_member_run_id: None,
                target_agent_team_id: target_run.agent_team_id.clone(),
                target_work_ref: WorkRef {
                    team_run_id: String::new(),
                    work_id: String::new(),
                },
                delegated_by_actor: host_actor.clone(),
                state: WorkDelegationState::Active,
                resolution_summary: None,
                blocker_reason: None,
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            Work {
                id: "target-rollup".into(),
                team_run_id: target_run.id.clone(),
                accountable_team_id: None,
                assignee_membership_id: None,
                created_by_member_id: None,
                parent_work_id: None,
                title: "Delegated target".into(),
                context_markdown: "execute delegated target".into(),
                completion_criteria_markdown: "exact candidate accepted".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: Some("target-worker".into()),
                active_member_run_id: Some(target_runtime_id.into()),
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: host_actor.clone(),
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
                event_id: "delegation-create".into(),
                performed_by_actor: host_actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "delegation-create".into(),
                created_at: "t2".into(),
                duplicate_ok: false,
            },
        )
        .expect("atomically create Delegation and target Work");
    let started = harness
        .store
        .start_work(
            &target.id,
            target.version,
            target_runtime_id,
            WorkCommandContext {
                event_id: "target-start".into(),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::ProviderRuntimeProjection,
                    id: target_runtime_id.into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "target-start".into(),
                created_at: "t3".into(),
                duplicate_ok: false,
            },
        )
        .expect("start delegated target");
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "delegated-candidate".into(),
    };
    let candidate_fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(&candidate).unwrap());
    let mut result = report(
        "report-delegated-target",
        WorkReportKind::Result,
        &member_actor("target-worker"),
    );
    result.work_id = target.id.clone();
    result.work_revision = started.version + 1;
    result.candidate = Some(candidate);
    result.candidate_fingerprint = Some(candidate_fingerprint.clone());
    result.evidence_refs = vec!["evidence://delegated-candidate".into()];
    harness
        .store
        .create_trust_work_report(
            &context(
                member_actor("target-worker"),
                "report.create",
                "report-delegated-target",
                0,
            ),
            &target_run.agent_team_id,
            result,
        )
        .expect("submit delegated target result");
    let accepted = harness
        .store
        .accept_trust_work(
            &context(
                human("host"),
                "work.accept",
                "accept-delegated-target",
                started.version + 1,
            ),
            &target_run.agent_team_id,
            &target.id,
            "report-delegated-target",
            &candidate_fingerprint,
            "t5",
        )
        .expect("accept delegated target");
    let rolled_up = harness
        .store
        .latest_work_delegations()
        .unwrap()
        .into_iter()
        .find(|row| row.id == delegation.id)
        .expect("rolled-up Delegation");
    assert_eq!(rolled_up.state, WorkDelegationState::Completed);
    assert_eq!(rolled_up.version, delegation.version + 1);
    let operation = harness
        .store
        .canonical_operations()
        .unwrap()
        .into_iter()
        .find(|operation| operation.event.id == accepted.event.id)
        .expect("canonical acceptance operation");
    assert!(operation.immutable_side_records.iter().any(|record| {
        serde_json::from_value::<firm_core::WorkDelegationRevision>(record.clone())
            .is_ok_and(|revision| revision.delegation.id == delegation.id)
    }));
    let source_after = harness
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == source.id)
        .expect("source Work remains visible");
    assert_eq!(source_after, source, "roll-up must not mutate source Work");
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

#[test]
fn gates_require_exact_evaluation_or_authorized_waiver_and_reject_self_cycles() {
    let harness = TestStore::new("gates");
    let team_id = seed_team_work(&harness.store, "gates", "work-gate");
    let host = human("host");
    let critic = member_actor("critic");
    let mut cyclic = requirement("gate-cycle");
    cyclic.dependency_requirement_ids.push(cyclic.id.clone());
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_requirement(
                    &context(host.clone(), "gate.require", "cycle", 0),
                    &team_id,
                    cyclic,
                )
                .expect_err("self-cycle must fail")
        ),
        TrustErrorCode::GateDependencyCycle
    );
    let mut cycle_a = requirement("cycle-a");
    cycle_a.required = false;
    cycle_a.dependency_requirement_ids = vec!["cycle-b".into()];
    harness
        .store
        .create_trust_gate_requirement(
            &context(host.clone(), "gate.require", "cycle-a", 0),
            &team_id,
            cycle_a,
        )
        .expect("forward dependency may be declared");
    let mut cycle_b = requirement("cycle-b");
    cycle_b.required = false;
    cycle_b.dependency_requirement_ids = vec!["cycle-a".into()];
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_requirement(
                    &context(host.clone(), "gate.require", "cycle-b", 0),
                    &team_id,
                    cycle_b,
                )
                .expect_err("transitive cycle must fail")
        ),
        TrustErrorCode::GateDependencyCycle
    );

    harness
        .store
        .create_trust_gate_requirement(
            &context(host.clone(), "gate.require", "gate-a", 0),
            &team_id,
            requirement("gate-a"),
        )
        .expect("create exact requirement");
    assert_eq!(
        trust_code(
            harness
                .store
                .trust_gate_satisfied(SPACE, "work-gate", 1, "report-gate", "sha256:candidate",)
                .expect_err("required gate needs evaluation")
        ),
        TrustErrorCode::GateEvaluationRequired
    );
    let mut stale = evaluation("eval-stale", "gate-a", &critic);
    stale.candidate_fingerprint = "sha256:other".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_evaluation(
                    &context(critic.clone(), "gate.evaluate", "stale", 0),
                    stale,
                )
                .expect_err("stale candidate must fail")
        ),
        TrustErrorCode::GateRequirementStale
    );
    let before_wrong_evaluator = harness.store.canonical_operations().unwrap().len();
    let impostor = member_actor("worker");
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_evaluation(
                    &context(impostor.clone(), "gate.evaluate", "wrong-evaluator", 0),
                    evaluation("eval-impostor", "gate-a", &impostor),
                )
                .expect_err("wrong evaluator identity must fail")
        ),
        TrustErrorCode::UnauthorizedActor
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_wrong_evaluator,
        "wrong evaluator rejection must have zero durable side effects"
    );
    harness
        .store
        .create_trust_gate_evaluation(
            &context(critic, "gate.evaluate", "exact", 0),
            evaluation("eval-exact", "gate-a", &member_actor("critic")),
        )
        .expect("exact evaluation");

    harness
        .store
        .create_trust_gate_requirement(
            &context(host.clone(), "gate.require", "gate-b", 0),
            &team_id,
            requirement("gate-b"),
        )
        .expect("create waiver requirement");
    let authority = human("release-manager");
    let waiver = GateWaiver {
        id: "waiver-b".into(),
        requirement_id: "gate-b".into(),
        work_id: "work-gate".into(),
        work_revision: 1,
        candidate_fingerprint: "sha256:candidate".into(),
        authority_actor: authority.clone(),
        performed_by_actor: host.clone(),
        reason: "documented emergency".into(),
        evidence_refs: vec!["evidence://waiver".into()],
        state: GateWaiverState::Active,
        version: 1,
        created_at: "t3".into(),
        revoked_at: None,
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_waiver(
                    &context(host.clone(), "gate.waive", "unauthorized", 0),
                    waiver.clone(),
                )
                .expect_err("authority must be explicit")
        ),
        TrustErrorCode::GateWaiverUnauthorized
    );
    let mut authorized = context(host, "gate.waive", "authorized", 0);
    authorized.authority_actor = Some(authority);
    let mut missing_requirement = waiver.clone();
    missing_requirement.id = "waiver-missing".into();
    missing_requirement.requirement_id = "gate-missing".into();
    let mut missing_context = authorized.clone();
    missing_context.idempotency_key = "missing-requirement".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_waiver(&missing_context, missing_requirement)
                .expect_err("waiver requirement must resolve")
        ),
        TrustErrorCode::GateRequirementStale
    );
    harness
        .store
        .create_trust_gate_waiver(&authorized, waiver)
        .expect("authorized waiver");
    harness
        .store
        .trust_gate_satisfied(SPACE, "work-gate", 1, "report-gate", "sha256:candidate")
        .expect("exact evaluation plus exact waiver satisfy all required gates");
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

#[test]
fn workspace_binding_rejects_relative_and_parent_traversal_without_side_effects() {
    let harness = TestStore::new("workspace-path");
    let host = human("host");
    for (id, root) in [
        ("relative", "project/worktree"),
        ("parent", "/tmp/project/../escape"),
    ] {
        let before = harness.store.canonical_operations().unwrap().len();
        assert_eq!(
            trust_code(
                harness
                    .store
                    .create_trust_workspace_binding(
                        &context(host.clone(), "workspace.bind", id, 0),
                        workspace_binding(id, root, &host),
                    )
                    .expect_err("unsafe path must fail")
            ),
            TrustErrorCode::WorkspacePathUnsafe
        );
        assert_eq!(harness.store.canonical_operations().unwrap().len(), before);
    }
}

#[test]
fn workspace_binding_requires_exact_member_run_and_project_placement() {
    let harness = TestStore::new("workspace-placement");
    let host = human("host");
    let team_run = seed_team(&harness.store, "workspace-placement", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        false,
    );

    let mut missing_run = workspace_binding("missing-run", "/trust-test/missing", &host);
    missing_run.team_run_id = team_run.id.clone();
    missing_run.member_run_id = "run-missing".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_workspace_binding(
                    &context(host.clone(), "workspace.bind", "missing-run", 0),
                    missing_run,
                )
                .expect_err("workspace MemberRun must resolve")
        ),
        TrustErrorCode::InvalidStateTransition
    );

    let mut wrong_project = workspace_binding("wrong-project", "/trust-test/project", &host);
    wrong_project.team_run_id = team_run.id.clone();
    wrong_project.member_run_id = "runtime-member-a".into();
    wrong_project.project_binding_id = "project-other".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_workspace_binding(
                    &context(host, "workspace.bind", "wrong-project", 0),
                    wrong_project,
                )
                .expect_err("workspace ProjectBinding must match TeamRun placement")
        ),
        TrustErrorCode::WorkspaceRepositoryMismatch
    );
}

#[test]
fn workspace_transitions_reobserve_git_links_dirty_state_and_cleanup_safety() {
    let harness = TestStore::new("workspace-real-safety");
    let host = human("host");
    let team_run = seed_team(&harness.store, "workspace-real-safety", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        false,
    );
    let repo = harness.root.join("workspace");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "trust@example.invalid"],
        vec!["config", "user.name", "Trust Test"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(repo.join("README.md"), "workspace safety\n").unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["commit", "-qm", "seed"])
        .status()
        .unwrap()
        .success());
    let canonical_root = std::fs::canonicalize(&repo).unwrap();
    let mut binding = workspace_binding("workspace-real", canonical_root.to_str().unwrap(), &host);
    binding.team_run_id = team_run.id;
    binding.member_run_id = "runtime-member-a".into();
    let created = harness
        .store
        .create_trust_workspace_binding(
            &context(
                host.clone(),
                "workspace.provision",
                "workspace-real-create",
                0,
            ),
            binding,
        )
        .expect("create real workspace binding")
        .projection;
    let clean_proof = |version_root: &str| WorkspaceSafetyProof {
        canonical_root: version_root.into(),
        project_binding_id: "project-test".into(),
        git_common_dir: created.git_common_dir.clone(),
        link_escape_free: true,
        repository_matches: true,
        is_dirty: false,
        is_conflicted: false,
        observed_member_generation: 1,
    };
    harness
        .store
        .transition_trust_workspace_binding(
            &context(
                host.clone(),
                "workspace.transition",
                "workspace-preparing",
                1,
            ),
            &created.id,
            WorkspaceLifecycle::Preparing,
            &clean_proof(&created.canonical_root),
            "t2",
        )
        .expect("requested to preparing");
    harness
        .store
        .transition_trust_workspace_binding(
            &context(host.clone(), "workspace.transition", "workspace-ready", 2),
            &created.id,
            WorkspaceLifecycle::Ready,
            &clean_proof(&created.canonical_root),
            "t3",
        )
        .expect("preparing to ready");

    let outside = harness.root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, repo.join("escape-link")).unwrap();
    let before_link = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .transition_trust_workspace_binding(
                    &context(host.clone(), "workspace.attach", "workspace-link", 3),
                    &created.id,
                    WorkspaceLifecycle::Attached,
                    &clean_proof(&created.canonical_root),
                    "t4",
                )
                .expect_err("link escape must fail")
        ),
        TrustErrorCode::WorkspaceLinkEscape
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_link
    );
    #[cfg(unix)]
    std::fs::remove_file(repo.join("escape-link")).unwrap();

    std::fs::write(repo.join("dirty.txt"), "dirty\n").unwrap();
    let before_dirty = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .transition_trust_workspace_binding(
                    &context(host.clone(), "workspace.attach", "workspace-dirty-lie", 3),
                    &created.id,
                    WorkspaceLifecycle::Attached,
                    &clean_proof(&created.canonical_root),
                    "t5",
                )
                .expect_err("caller cannot conceal dirty workspace")
        ),
        TrustErrorCode::WorkspaceDirty
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_dirty
    );
    let mut dirty_proof = clean_proof(&created.canonical_root);
    dirty_proof.is_dirty = true;
    let blocked = harness
        .store
        .transition_trust_workspace_binding(
            &context(host, "workspace.transition", "workspace-cleanup-blocked", 3),
            &created.id,
            WorkspaceLifecycle::CleanupBlocked,
            &dirty_proof,
            "t6",
        )
        .expect("dirty workspace becomes cleanup_blocked")
        .projection;
    assert_eq!(blocked.blocked_reason.as_deref(), Some("WORKSPACE_DIRTY"));
    assert!(blocked.dirty_fingerprint.is_some());
}
