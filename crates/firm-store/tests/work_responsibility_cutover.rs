//! Black-box tests for the DEV-36 Global Work responsibility cutover (DOC-106).
//!
//! These pin: TeamMembership-bound assignment with expected-version CAS, no
//! runtime dependence of responsibility, WorkExecutionBinding as the transient
//! execution fence, and the append-only responsibility migration with explicit
//! collision/ambiguity reporting.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, AgentSession,
    AgentSessionStatus, MutationContext, PermissionCeiling, TeamMembership, TeamMembershipRole,
    TeamMembershipStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
};
use firm_core::{
    AgentTeam, AgentTeamRun, AgentTeamStatus, ExecutionNode, ExecutionNodeStatus, Mission,
    MissionStatus, NodeProjectRegistration, NodeProjectRegistrationStatus, TeamActorKind,
    TeamActorRef, TeamRunStatus, Work, WorkClaimMode, WorkCommandContext, WorkCondition,
    WorkEventKind, WorkOperation, WorkPhase, WorkPriority, WorkResponsibilityResolution,
};
use firm_store::HarnessStore;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const SPACE: &str = "space-cutover-test";
const NODE: &str = "00000000-0000-4000-8000-0000000000cd";

struct TestStore {
    root: PathBuf,
    store: HarnessStore,
}

impl TestStore {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "firm-store-work-cutover-{label}-{}-{}",
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

fn trust_context(
    actor: ActorRef,
    command: &str,
    key: &str,
    expected_version: u64,
) -> MutationContext {
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

fn host_work_context(host_member_id: &str, key: &str, at: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: format!("event-{key}"),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Host,
            id: host_member_id.into(),
            display_name: None,
            authn_source: Some("test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: format!("command-{key}"),
        created_at: at.into(),
        duplicate_ok: false,
    }
}

fn member(id: &str) -> AgentMember {
    AgentMember {
        id: id.into(),
        name: format!("Member {id}"),
        description: "durable organization identity".into(),
        role: if id.contains("host") {
            "host"
        } else {
            "worker"
        }
        .into(),
        capabilities: vec!["code".into()],
        skill_refs: Vec::new(),
        provider_profile_ref: Some("codex-default".into()),
        model_preference: None,
        workspace_policy: "managed-worktree".into(),
        permission_ceiling: PermissionCeiling::WorkspaceWrite,
        organization_status: AgentMemberOrganizationStatus::Active,
        version: 1,
        created_by: human("fixture-operator"),
        created_at: "t1".into(),
        updated_at: "t1".into(),
    }
}

fn work_fixture(run_id: &str, host_member_id: &str, id: &str) -> Work {
    Work {
        id: id.into(),
        team_run_id: run_id.into(),
        accountable_team_id: None,
        assignee_membership_id: None,
        parent_work_id: None,
        title: format!("Cutover fixture {id}"),
        context_markdown: "responsibility test".into(),
        completion_criteria_markdown: "exact and honest".into(),
        phase: WorkPhase::Open,
        condition: WorkCondition::Normal,
        resolution: None,
        owner_member_id: None,
        active_member_run_id: None,
        claim_mode: WorkClaimMode::HostAssign,
        eligible_member_ids: Vec::new(),
        prerequisite_work_ids: Vec::new(),
        priority: WorkPriority::Normal,
        created_by_actor: TeamActorRef {
            kind: TeamActorKind::Host,
            id: host_member_id.into(),
            display_name: None,
            authn_source: Some("test".into()),
        },
        created_by_member_id: None,
        result_summary: None,
        blocker_reason: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// Create one durable Team with a Host membership plus one membership per
/// extra member id, and its current TeamRun legacy projection row.
fn seed_team(store: &HarnessStore, label: &str, member_ids: &[&str]) -> AgentTeamRun {
    let mission_id = format!("mission-{label}");
    let team_id = format!("team-{label}");
    let run_id = format!("team-run-{label}");
    store
        .append_mission(&Mission {
            id: mission_id.clone(),
            title: "cutover test".into(),
            objective: "exercise responsibility cutover".into(),
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
                display_name: "cutover node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "t1".into(),
                updated_at: "t1".into(),
            })
            .expect("insert node");
    }
    if !store
        .latest_node_project_registrations()
        .expect("read registrations")
        .iter()
        .any(|registration| {
            registration.node_id == NODE
                && registration.execution_space_id == SPACE
                && registration.project_binding_id == "project-cutover"
                && registration.status == NodeProjectRegistrationStatus::Active
        })
    {
        store
            .register_node_project(
                &NodeProjectRegistration {
                    node_id: NODE.into(),
                    execution_space_id: SPACE.into(),
                    project_binding_id: "project-cutover".into(),
                    status: NodeProjectRegistrationStatus::Active,
                    created_at: "t1".into(),
                    updated_at: "t1".into(),
                },
                SPACE,
            )
            .expect("register project on node");
    }
    for member_id in member_ids {
        if !store
            .trust_agent_members(SPACE)
            .expect("read AgentMembers")
            .iter()
            .any(|candidate| candidate.id == *member_id)
        {
            store
                .create_trust_agent_member(
                    &trust_context(
                        human("fixture-operator"),
                        "agent_member.create",
                        &format!("member-{label}-{member_id}"),
                        0,
                    ),
                    member(member_id),
                )
                .expect("create AgentMember");
        }
    }
    let team = AgentTeam {
        id: team_id.clone(),
        name: "cutover team".into(),
        description: "cutover fixture".into(),
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
            created_by: human("fixture-operator"),
            revision: 1,
            joined_at: "t1".into(),
            left_at: None,
        })
        .collect();
    store
        .create_agent_team(
            &trust_context(
                human("fixture-operator"),
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
        project_binding_id: "project-cutover".into(),
        previous_run_id: None,
        host_surface: "test".into(),
        host_thread_id: None,
        host_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: member_ids[0].into(),
            display_name: None,
            authn_source: Some("test".into()),
        }),
        host_control_mode: Default::default(),
        objective: "cutover test".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: Vec::new(),
        budget_limit_usd: None,
        created_at: "t1".into(),
        updated_at: "t1".into(),
        completed_at: None,
    };
    append_raw_row(store, "team_runs.jsonl", &run);
    run
}

fn append_raw_row<T: serde::Serialize>(store: &HarnessStore, ledger: &str, value: &T) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join(ledger))
        .expect("open raw fixture ledger");
    serde_json::to_writer(&mut file, value).expect("serialize raw fixture row");
    file.write_all(b"\n").expect("terminate raw fixture row");
    file.sync_all().expect("persist raw fixture row");
}

/// Append a pre-cutover WorkOperation row whose Work has no
/// `accountable_team_id` (and, unless `legacy_team_key` is set, no legacy
/// `team_id` alias key either), simulating a TeamRun-scoped compatibility row.
fn append_legacy_work_row(
    store: &HarnessStore,
    run_id: &str,
    host_member_id: &str,
    work_id: &str,
    owner_member_id: Option<&str>,
    legacy_team_key: Option<&str>,
) {
    let mut work = work_fixture(run_id, host_member_id, work_id);
    work.version = 1;
    work.created_at = "t0".into();
    work.updated_at = "t0".into();
    work.owner_member_id = owner_member_id.map(str::to_string);
    let operation = WorkOperation {
        event: firm_core::WorkEvent {
            id: format!("legacy-event-{work_id}"),
            team_run_id: run_id.into(),
            work_id: work_id.into(),
            sequence: 1,
            kind: WorkEventKind::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: work.created_by_actor.clone(),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: format!("legacy-create-{work_id}"),
            payload: serde_json::Value::Null,
            created_at: "t0".into(),
        },
        work,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
        decisions: Vec::new(),
        deliveries: Vec::new(),
        delivery_updates: Vec::new(),
        delegation_revisions: Vec::new(),
    };
    let mut row = serde_json::to_value(&operation).expect("operation JSON");
    let projection = row["work"].as_object_mut().expect("Work object");
    projection.remove("accountable_team_id");
    projection.remove("assignee_membership_id");
    if let Some(team_id) = legacy_team_key {
        projection.insert("team_id".to_string(), serde_json::json!(team_id));
    }
    append_raw_row(store, "work_operations.jsonl", &row);
}

fn work_operations_raw(store: &HarnessStore) -> Vec<String> {
    std::fs::read_to_string(store.root().join("work_operations.jsonl"))
        .expect("read work operations ledger")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn membership_assignment_is_cas_fenced_and_needs_no_runtime() {
    let fixture = TestStore::new("membership-assign");
    let store = &fixture.store;
    let run = seed_team(
        store,
        "assign",
        &["host-assign", "worker-assign", "peer-assign"],
    );
    let work = store
        .insert_work(
            work_fixture(&run.id, "host-assign", "work-assign-1"),
            host_work_context("host-assign", "create-assign-1", "t2"),
        )
        .expect("create Work");
    assert_eq!(work.version, 1);
    assert_eq!(
        work.accountable_team_id.as_deref(),
        Some(run.agent_team_id.as_str())
    );

    // CAS fences on the exact expected Work version.
    let stale = store
        .assign_work_to_membership(
            "work-assign-1",
            99,
            "membership-team-assign-worker-assign",
            SPACE,
            host_work_context("host-assign", "assign-stale", "t3"),
        )
        .expect_err("stale expected version must be fenced");
    assert!(stale.to_string().contains("VERSION_CONFLICT"));

    // Assignment binds the TeamMembership; no provider process exists and no
    // delivery or runtime binding is created.
    let assigned = store
        .assign_work_to_membership(
            "work-assign-1",
            1,
            "membership-team-assign-worker-assign",
            SPACE,
            host_work_context("host-assign", "assign-worker", "t3"),
        )
        .expect("assign Work to TeamMembership without any runtime");
    assert_eq!(assigned.version, 2);
    assert_eq!(
        assigned.assignee_membership_id.as_deref(),
        Some("membership-team-assign-worker-assign")
    );
    assert_eq!(assigned.owner_member_id.as_deref(), Some("worker-assign"));
    assert_eq!(assigned.active_member_run_id, None);
    assert_eq!(assigned.phase, WorkPhase::Open);
    let deliveries = store
        .latest_work_deliveries()
        .expect("deliveries")
        .into_iter()
        .filter(|delivery| delivery.work_id == "work-assign-1")
        .count();
    assert_eq!(deliveries, 0, "membership assignment creates no dispatch");

    // Reassignment is the same CAS path and records the previous assignee.
    let stale_reassign = store
        .assign_work_to_membership(
            "work-assign-1",
            1,
            "membership-team-assign-host-assign",
            SPACE,
            host_work_context("host-assign", "reassign-stale", "t4"),
        )
        .expect_err("reassignment with a stale version must be fenced");
    assert!(stale_reassign.to_string().contains("VERSION_CONFLICT"));
    let reassigned = store
        .assign_work_to_membership(
            "work-assign-1",
            2,
            "membership-team-assign-host-assign",
            SPACE,
            host_work_context("host-assign", "reassign-host", "t4"),
        )
        .expect("reassign to the Host membership");
    assert_eq!(
        reassigned.assignee_membership_id.as_deref(),
        Some("membership-team-assign-host-assign")
    );
    let last = store
        .work_operations()
        .expect("operations")
        .last()
        .expect("reassign operation")
        .clone();
    assert_eq!(last.event.kind, WorkEventKind::Assigned);
    assert_eq!(last.event.expected_version, 2);
    assert_eq!(last.event.resulting_version, 3);

    let same = store
        .assign_work_to_membership(
            "work-assign-1",
            3,
            "membership-team-assign-host-assign",
            SPACE,
            host_work_context("host-assign", "reassign-same", "t5"),
        )
        .expect_err("reassigning the current assignee is a conflict, not a no-op");
    assert!(same.to_string().contains("WORK_ALREADY_ASSIGNED"));

    let missing = store
        .assign_work_to_membership(
            "work-assign-1",
            3,
            "membership-team-assign-ghost",
            SPACE,
            host_work_context("host-assign", "assign-ghost", "t5"),
        )
        .expect_err("unknown membership fails closed");
    assert!(missing.to_string().contains("TEAM_MEMBERSHIP_NOT_FOUND"));
}

#[test]
fn membership_assignment_refuses_observer_and_cross_team_targets() {
    let fixture = TestStore::new("assign-targets");
    let store = &fixture.store;
    let run = seed_team(store, "targets", &["host-targets", "worker-targets"]);
    let other_run = seed_team(store, "targets-other", &["host-other", "worker-other"]);
    let _work = store
        .insert_work(
            work_fixture(&run.id, "host-targets", "work-targets-1"),
            host_work_context("host-targets", "create-targets-1", "t2"),
        )
        .expect("create Work");

    store
        .create_trust_agent_member(
            &trust_context(
                human("fixture-operator"),
                "agent_member.create",
                "member-observer-targets",
                0,
            ),
            member("observer-targets"),
        )
        .expect("create Observer AgentMember");
    let observer = store
        .join_team_membership(
            &trust_context(
                human("fixture-operator"),
                "team_membership.join",
                "join-observer",
                0,
            ),
            TeamMembership {
                id: "membership-team-targets-observer-targets".to_string(),
                team_id: run.agent_team_id.clone(),
                agent_member_id: "observer-targets".into(),
                node_id: NODE.into(),
                role: TeamMembershipRole::Observer,
                state: TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: human("fixture-operator"),
                revision: 1,
                joined_at: "t2".into(),
                left_at: None,
            },
        )
        .expect("join Observer membership");
    let refused = store
        .assign_work_to_membership(
            "work-targets-1",
            1,
            &observer.projection.id,
            SPACE,
            host_work_context("host-targets", "assign-observer", "t3"),
        )
        .expect_err("Observer membership cannot hold Work responsibility");
    assert!(refused.to_string().contains("ASSIGNEE_ROLE_INVALID"));

    let cross_team = store
        .assign_work_to_membership(
            "work-targets-1",
            1,
            &format!("membership-{}-worker-other", other_run.agent_team_id),
            SPACE,
            host_work_context("host-targets", "assign-cross-team", "t3"),
        )
        .expect_err("cross-Team membership cannot take this Team's Work");
    assert!(cross_team.to_string().contains("TEAM_SCOPE_MISMATCH"));
}

#[test]
fn dormant_assignee_retains_responsibility_without_active_membership_or_runtime() {
    let fixture = TestStore::new("dormant-assignee");
    let store = &fixture.store;
    let run = seed_team(store, "dormant", &["host-dormant", "worker-dormant"]);
    let work = store
        .insert_work(
            work_fixture(&run.id, "host-dormant", "work-dormant-1"),
            host_work_context("host-dormant", "create-dormant-1", "t2"),
        )
        .expect("create Work");
    let assigned = store
        .assign_work_to_membership(
            &work.id,
            1,
            "membership-team-dormant-worker-dormant",
            SPACE,
            host_work_context("host-dormant", "assign-dormant", "t3"),
        )
        .expect("assign to an Active membership");
    assert_eq!(assigned.version, 2);

    // The membership leaves (no active execution bindings exist), becoming
    // Inactive. Responsibility is unchanged: no Work operation is written.
    store
        .leave_team_membership(
            &trust_context(
                human("fixture-operator"),
                "team_membership.leave",
                "leave-worker",
                1,
            ),
            "membership-team-dormant-worker-dormant",
            "t4",
        )
        .expect("membership leave");
    let retained = store
        .latest_works()
        .expect("latest works")
        .into_iter()
        .find(|work| work.id == "work-dormant-1")
        .expect("work survives");
    assert_eq!(retained.version, 2, "no Work mutation on membership leave");
    assert_eq!(
        retained.assignee_membership_id.as_deref(),
        Some("membership-team-dormant-worker-dormant")
    );
    assert_eq!(retained.owner_member_id.as_deref(), Some("worker-dormant"));

    // Reassignment to an Inactive membership is allowed (dormant
    // responsibility) but grants no automatic execution authority.
    let reassigned = store
        .assign_work_to_membership(
            &work.id,
            2,
            "membership-team-dormant-host-dormant",
            SPACE,
            host_work_context("host-dormant", "assign-dormant-host", "t5"),
        )
        .expect("reassign while the previous assignee is dormant");
    assert_eq!(reassigned.version, 3);
    let last = store
        .work_operations()
        .expect("operations")
        .last()
        .expect("reassign operation")
        .clone();
    assert_eq!(
        last.event.payload["automatic_execution_authority"],
        serde_json::json!(true),
        "Host membership is Active, so automatic authority may follow later"
    );
}

#[test]
fn execution_binding_fences_runtime_without_owning_responsibility() {
    let fixture = TestStore::new("binding-fence");
    let store = &fixture.store;
    let run = seed_team(store, "binding", &["host-binding", "worker-binding"]);
    let work = store
        .insert_work(
            work_fixture(&run.id, "host-binding", "work-binding-1"),
            host_work_context("host-binding", "create-binding-1", "t2"),
        )
        .expect("create Work");
    let assigned = store
        .assign_work_to_membership(
            &work.id,
            1,
            "membership-team-binding-worker-binding",
            SPACE,
            host_work_context("host-binding", "assign-binding", "t3"),
        )
        .expect("assign to membership");
    assert_eq!(assigned.version, 2);

    store
        .acquire_node_daemon_lease(
            NODE,
            "daemon-cutover",
            "instance-cutover",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_millis() as u64,
            60_000,
        )
        .expect("node daemon lease");
    let session = AgentSession {
        id: "session-worker-binding".into(),
        agent_member_id: "worker-binding".into(),
        node_id: NODE.into(),
        execution_space_id: SPACE.into(),
        node_daemon_id: "daemon-cutover".into(),
        node_daemon_generation: 1,
        provider_kind: "codex".into(),
        provider_profile_ref: "codex-default".into(),
        permission_envelope_ref: "permission-default".into(),
        effective_permission_ceiling: PermissionCeiling::WorkspaceWrite,
        lifecycle: AgentSessionStatus::Idle,
        runtime_generation: 1,
        control_state: Default::default(),
        native_session_ref: None,
        current_turn_id: None,
        queued_input_count: 0,
        version: 1,
        opened_at: "t3".into(),
        last_active_at: "t3".into(),
        closed_at: None,
    };
    store
        .create_agent_session(
            &trust_context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-cutover".into(),
                },
                "session.create",
                "create-session",
                0,
            ),
            session,
        )
        .expect("create AgentSession");

    // A stale Work revision cannot bind execution.
    let stale = store
        .bind_work_execution(
            &trust_context(human("fixture-operator"), "work.bind", "bind-stale", 0),
            WorkExecutionBinding {
                id: "binding-stale".into(),
                work_id: work.id.clone(),
                work_revision: 1,
                team_id: run.agent_team_id.clone(),
                team_membership_id: "membership-team-binding-worker-binding".into(),
                agent_member_id: "worker-binding".into(),
                agent_session_id: "session-worker-binding".into(),
                agent_session_generation: 1,
                delivery_id: "delivery-binding-stale".into(),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: human("fixture-operator"),
                bound_at: "t4".into(),
                ended_at: None,
            },
        )
        .expect_err("stale Work revision must not bind execution");
    assert!(stale.to_string().contains("revision"));

    // The exact current revision binds. Responsibility is untouched.
    store
        .bind_work_execution(
            &trust_context(human("fixture-operator"), "work.bind", "bind-exact", 0),
            WorkExecutionBinding {
                id: "binding-exact".into(),
                work_id: work.id.clone(),
                work_revision: assigned.version,
                team_id: run.agent_team_id.clone(),
                team_membership_id: "membership-team-binding-worker-binding".into(),
                agent_member_id: "worker-binding".into(),
                agent_session_id: "session-worker-binding".into(),
                agent_session_generation: 1,
                delivery_id: "delivery-binding-exact".into(),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: human("fixture-operator"),
                bound_at: "t4".into(),
                ended_at: None,
            },
        )
        .expect("bind exact Work revision");
    let after_bind = store
        .latest_works()
        .expect("latest works")
        .into_iter()
        .find(|work| work.id == "work-binding-1")
        .expect("work");
    assert_eq!(
        after_bind.version, 2,
        "execution binding never mutates Work"
    );
    assert_eq!(
        after_bind.assignee_membership_id.as_deref(),
        Some("membership-team-binding-worker-binding")
    );

    // Release fences runtime authority off; the assignee still retains
    // responsibility.
    store
        .release_work_execution_binding(
            &trust_context(
                human("fixture-operator"),
                "work.release",
                "release-exact",
                1,
            ),
            "binding-exact",
            "t5",
        )
        .expect("release binding");
    let after_release = store
        .latest_works()
        .expect("latest works")
        .into_iter()
        .find(|work| work.id == "work-binding-1")
        .expect("work");
    assert_eq!(after_release.version, 2);
    assert_eq!(
        after_release.assignee_membership_id.as_deref(),
        Some("membership-team-binding-worker-binding")
    );
    let bindings = store
        .fabric_work_execution_bindings(SPACE)
        .expect("bindings");
    assert!(bindings.iter().all(|binding| {
        binding.work_id != "work-binding-1"
            || binding.status == WorkExecutionBindingStatus::Released
    }));
}

#[test]
fn responsibility_migration_is_append_only_reported_and_never_guesses() {
    let fixture = TestStore::new("migration");
    let store = &fixture.store;
    let run = seed_team(store, "migrate", &["host-migrate", "worker-migrate"]);

    // Canonical row created by the current binary: nothing to do.
    let _canonical = store
        .insert_work(
            work_fixture(&run.id, "host-migrate", "work-canonical"),
            host_work_context("host-migrate", "create-canonical", "t2"),
        )
        .expect("create canonical Work");

    // Legacy TeamRun-scoped rows appended raw, bypassing current writers.
    append_legacy_work_row(
        store,
        &run.id,
        "host-migrate",
        "work-legacy-owned",
        Some("worker-migrate"),
        None,
    );
    append_legacy_work_row(
        store,
        &run.id,
        "host-migrate",
        "work-legacy-orphan",
        Some("ghost"),
        None,
    );
    append_legacy_work_row(
        store,
        "team-run-missing",
        "host-migrate",
        "work-legacy-lost",
        None,
        None,
    );
    append_legacy_work_row(
        store,
        &run.id,
        "host-migrate",
        "work-legacy-alias",
        None,
        Some(&run.agent_team_id),
    );

    let before_rows = work_operations_raw(store);
    let report = store
        .migrate_work_responsibility(SPACE, host_work_context("host-migrate", "migrate", "t3"))
        .expect("migration runs");
    assert_eq!(report.execution_space_id, SPACE);
    let mut migrated = report.migrated_work_ids.clone();
    migrated.sort();
    assert_eq!(migrated, ["work-legacy-orphan", "work-legacy-owned"]);

    let entry = |id: &str| {
        report
            .entries
            .iter()
            .find(|entry| entry.work_id == id)
            .unwrap_or_else(|| panic!("migration entry for {id}"))
    };
    assert_eq!(
        entry("work-canonical").accountable_team,
        WorkResponsibilityResolution::AlreadyCanonical
    );
    assert_eq!(
        entry("work-canonical").assignee,
        WorkResponsibilityResolution::Unassigned
    );
    assert_eq!(entry("work-canonical").to_version, None);

    let owned = entry("work-legacy-owned");
    assert_eq!(
        owned.accountable_team,
        WorkResponsibilityResolution::Resolved {
            value: run.agent_team_id.clone()
        }
    );
    assert_eq!(
        owned.assignee,
        WorkResponsibilityResolution::Resolved {
            value: format!("membership-{}-worker-migrate", run.agent_team_id)
        }
    );
    assert_eq!(owned.from_version, 1);
    assert_eq!(owned.to_version, Some(2));

    let orphan = entry("work-legacy-orphan");
    assert!(matches!(
        orphan.assignee,
        WorkResponsibilityResolution::Unresolved { .. }
    ));
    assert_eq!(orphan.to_version, Some(2), "team resolution still lands");

    let lost = entry("work-legacy-lost");
    assert!(matches!(
        lost.accountable_team,
        WorkResponsibilityResolution::Unresolved { .. }
    ));
    assert_eq!(lost.to_version, None, "unresolvable rows are never written");

    // The legacy `team_id` alias reads through as canonical already.
    assert_eq!(
        entry("work-legacy-alias").accountable_team,
        WorkResponsibilityResolution::AlreadyCanonical
    );

    // Append-only: every pre-existing row is byte-identical, exactly two new
    // rows were appended, and Work history before migration is preserved.
    let after_rows = work_operations_raw(store);
    assert_eq!(after_rows.len(), before_rows.len() + 2);
    assert_eq!(&after_rows[..before_rows.len()], before_rows.as_slice());

    let works = store.latest_works().expect("latest works");
    let owned_work = works
        .iter()
        .find(|work| work.id == "work-legacy-owned")
        .expect("owned");
    assert_eq!(owned_work.version, 2);
    assert_eq!(
        owned_work.accountable_team_id.as_deref(),
        Some(run.agent_team_id.as_str())
    );
    assert_eq!(
        owned_work.assignee_membership_id.as_deref(),
        Some(format!("membership-{}-worker-migrate", run.agent_team_id).as_str())
    );
    let lost_work = works
        .iter()
        .find(|work| work.id == "work-legacy-lost")
        .expect("lost");
    assert_eq!(lost_work.version, 1, "unresolved Work keeps its version");
    assert_eq!(lost_work.accountable_team_id, None);

    // A migrated Work accepts current mutations at its new version; an
    // unmigrated one fails closed instead of silently writing.
    store
        .assign_work_to_membership(
            "work-legacy-owned",
            2,
            &format!("membership-{}-host-migrate", run.agent_team_id),
            SPACE,
            host_work_context("host-migrate", "assign-after-migration", "t4"),
        )
        .expect("migrated Work accepts CAS assignment at the migrated version");
    let refused = store.assign_work_to_membership(
        "work-legacy-lost",
        1,
        &format!("membership-{}-host-migrate", run.agent_team_id),
        SPACE,
        host_work_context("host-migrate", "assign-unmigrated", "t4"),
    );
    let refused = refused.expect_err("unmigrated Work without a current TeamRun fails closed");
    assert!(
        refused.to_string().contains("team run not found"),
        "unexpected error: {refused}"
    );

    // Re-running the migration is a reported no-op, not a silent rewrite.
    let second = store
        .migrate_work_responsibility(
            SPACE,
            host_work_context("host-migrate", "migrate-again", "t5"),
        )
        .expect("second migration run");
    assert!(second.migrated_work_ids.is_empty());
    let third_rows = work_operations_raw(store);
    assert_eq!(
        third_rows.len(),
        after_rows.len() + 1,
        "only the post-migration assignment appended"
    );
    assert!(second.entries.iter().any(|entry| {
        entry.work_id == "work-legacy-owned"
            && entry.accountable_team == WorkResponsibilityResolution::AlreadyCanonical
            && entry.assignee == WorkResponsibilityResolution::AlreadyCanonical
    }));
    assert!(second.entries.iter().any(|entry| {
        entry.work_id == "work-legacy-orphan"
            && matches!(
                entry.assignee,
                WorkResponsibilityResolution::Unresolved { .. }
            )
    }));
}
