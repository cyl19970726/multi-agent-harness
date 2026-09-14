//! Shared fixtures for the Work responsibility test binaries.
//!
//! Each binary compiles its own copy, so items unused by one of them are
//! expected; the allow keeps `-D warnings` honest about real dead code
//! elsewhere.
#![allow(dead_code)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, MutationContext,
    PermissionCeiling, TeamMembership, TeamMembershipRole, TeamMembershipStatus,
};
use firm_core::{
    AgentTeam, AgentTeamRun, AgentTeamStatus, ExecutionNode, ExecutionNodeStatus, Mission,
    MissionStatus, NodeProjectRegistration, NodeProjectRegistrationStatus, TeamActorKind,
    TeamActorRef, TeamRunStatus, Work, WorkClaimMode, WorkCommandContext, WorkCondition,
    WorkEventKind, WorkOperation, WorkPhase, WorkPriority,
};
use firm_store::HarnessStore;

pub static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
pub const SPACE: &str = "space-cutover-test";
pub const NODE: &str = "00000000-0000-4000-8000-0000000000cd";

pub struct TestStore {
    pub root: PathBuf,
    pub store: HarnessStore,
}

impl TestStore {
    pub fn new(label: &str) -> Self {
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

pub fn rewrite_trust_operation(
    root: &std::path::Path,
    aggregate_id: &str,
    mut rewrite: impl FnMut(&mut serde_json::Value),
) {
    let ledger = root.join("agentfirm_trust_operations.jsonl");
    let contents = std::fs::read_to_string(&ledger).expect("read canonical trust ledger");
    let mut found = false;
    let mut rows = Vec::new();
    for line in contents.lines() {
        let mut row: serde_json::Value = serde_json::from_str(line).expect("parse trust row");
        if row["operation"]["event"]["aggregate_id"] == aggregate_id {
            rewrite(&mut row);
            found = true;
        }
        rows.push(serde_json::to_string(&row).expect("serialize trust row"));
    }
    assert!(found, "canonical aggregate {aggregate_id} must exist");
    std::fs::write(&ledger, format!("{}\n", rows.join("\n")))
        .expect("rewrite canonical trust ledger fixture");
}

pub fn human(id: &str) -> ActorRef {
    ActorRef {
        kind: ActorKind::Human,
        id: id.into(),
    }
}

pub fn trust_context(
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

pub fn host_work_context(host_member_id: &str, key: &str, at: &str) -> WorkCommandContext {
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

pub fn member(id: &str) -> AgentMember {
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

pub fn work_fixture(run_id: &str, host_member_id: &str, id: &str) -> Work {
    Work {
        id: id.into(),
        team_run_id: run_id.into(),
        accountable_team_id: None,
        assignee_membership_id: None,
        legacy_containment_ref: None,
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
pub fn seed_team(store: &HarnessStore, label: &str, member_ids: &[&str]) -> AgentTeamRun {
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

pub fn append_raw_row<T: serde::Serialize>(store: &HarnessStore, ledger: &str, value: &T) {
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
pub fn append_legacy_work_row(
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
            executed_by_member_run_id: None,
        },
        work,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
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

/// Append a pre-cutover WorkOperation row whose Work folds but cannot satisfy
/// the current `Work` contract -- here an empty title. Only a raw legacy row
/// can carry one: every current writer validates the projection before it
/// appends, so this is the exact shape a plan-phase validation must catch.
pub fn append_legacy_work_row_with_invalid_projection(
    store: &HarnessStore,
    run_id: &str,
    host_member_id: &str,
    work_id: &str,
) {
    let mut work = work_fixture(run_id, host_member_id, work_id);
    work.version = 1;
    work.created_at = "t0".into();
    work.updated_at = "t0".into();
    work.title = String::new();
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
            executed_by_member_run_id: None,
        },
        work,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
        delegation_revisions: Vec::new(),
    };
    let mut row = serde_json::to_value(&operation).expect("operation JSON");
    let projection = row["work"].as_object_mut().expect("Work object");
    projection.remove("accountable_team_id");
    projection.remove("assignee_membership_id");
    append_raw_row(store, "work_operations.jsonl", &row);
}

/// Append a pre-cutover WorkOperation row whose Work is already terminal. Only
/// a raw legacy row can carry a closed Work in `work_operations.jsonl`: the
/// current cancel and accept writers settle through the canonical trust
/// ledger, so this is the exact shape the ledger-shaped `Updated` writers see.
pub fn append_legacy_terminal_work_row(
    store: &HarnessStore,
    run_id: &str,
    host_member_id: &str,
    work_id: &str,
    owner_member_id: Option<&str>,
) {
    let mut work = work_fixture(run_id, host_member_id, work_id);
    work.version = 1;
    work.created_at = "t0".into();
    work.updated_at = "t0".into();
    work.owner_member_id = owner_member_id.map(str::to_string);
    work.phase = WorkPhase::Closed;
    work.condition = WorkCondition::Normal;
    work.resolution = Some(firm_core::WorkResolution::Cancelled);
    let operation = WorkOperation {
        event: firm_core::WorkEvent {
            id: format!("legacy-event-{work_id}"),
            team_run_id: run_id.into(),
            work_id: work_id.into(),
            sequence: 1,
            kind: WorkEventKind::Cancelled,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: work.created_by_actor.clone(),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: format!("legacy-cancel-{work_id}"),
            payload: serde_json::Value::Null,
            created_at: "t0".into(),
            executed_by_member_run_id: None,
        },
        work,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
        delegation_revisions: Vec::new(),
    };
    let mut row = serde_json::to_value(&operation).expect("operation JSON");
    let projection = row["work"].as_object_mut().expect("Work object");
    projection.remove("accountable_team_id");
    projection.remove("assignee_membership_id");
    append_raw_row(store, "work_operations.jsonl", &row);
}

pub fn work_operations_raw(store: &HarnessStore) -> Vec<String> {
    std::fs::read_to_string(store.root().join("work_operations.jsonl"))
        .expect("read work operations ledger")
        .lines()
        .map(str::to_string)
        .collect()
}
