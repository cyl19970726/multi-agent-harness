use std::collections::BTreeSet;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use firm_core::{
    DelegationMode, DelegationStatus, HostAttentionKind, LegacyWave, LegacyWaveExecutorKind,
    LegacyWaveGateStatus, LegacyWaveStatus, MemberActionStatus, MemberExecutionDriver,
    MemberRunStatus, MemberWorkspaceSnapshot, Mission, MissionStatus, OrdinaryMessageBoundary,
    ProviderCompatibilityBlockBoundary, ProviderCompatibilityBlockSource, ProviderDispatchAttempt,
    ProviderDispatchIntent, ProviderEventFidelity, ProviderFeatureMode, ProviderInteractionMode,
    ProviderResponseIntent, RegistryMessageIntent, SenderKind, TeamActorKind, TeamActorRef,
    TeamDeliveryPolicy, TeamDeliveryStatus, TeamRunEventSourceKind, TeamRunStatus, WorkPriority,
};

use super::*;

mod fixtures;
use fixtures::*;
mod work_execution_fixture;
use work_execution_fixture::start_claimed_work_for_test;

fn lock_policy_test_store(label: &str) -> HarnessStore {
    let root = std::env::temp_dir().join(format!(
        "firm-store-lock-policy-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let store = HarnessStore::new(root);
    store.init().expect("init lock-policy store");
    store
}

fn hold_store_lock(store: &HarnessStore) -> File {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(store.root().join(".store.lock"))
        .expect("open store lock");
    lock_file_exclusive(&file).expect("hold store lock");
    file
}

fn canonical_member_admission_for_test(
    execution_space_id: &str,
    runtime: &ProviderRuntimeProjection,
) -> CanonicalMemberRunAdmission {
    CanonicalMemberRunAdmission {
        context: firm_core::agentfirm_api::MutationContext {
            execution_space_id: execution_space_id.into(),
            authenticated_actor: firm_core::agentfirm_api::ActorRef {
                kind: firm_core::agentfirm_api::ActorKind::Service,
                id: "test-node-daemon".into(),
            },
            authority_actor: Some(firm_core::agentfirm_api::ActorRef {
                kind: firm_core::agentfirm_api::ActorKind::AgentMember,
                id: runtime.agent_member_id.clone(),
            }),
            command_name: "team_run.materialize_member_run".into(),
            idempotency_key: format!("test-member-run:{}", runtime.id),
            expected_version: 0,
            request_fingerprint: None,
        },
        run: firm_core::agentfirm_api::MemberRun {
            id: runtime.id.clone(),
            agent_member_id: runtime.agent_member_id.clone(),
            team_run_id: runtime.team_run_id.clone(),
            role_snapshot: runtime.role.clone(),
            provider_profile_snapshot: None,
            requested_controls: serde_json::json!({}),
            effective_controls: serde_json::json!({}),
            coordination_status: match runtime.coordination_status {
                firm_core::MemberCoordinationStatus::Active => {
                    firm_core::agentfirm_api::MemberCoordinationStatus::Active
                }
                firm_core::MemberCoordinationStatus::Closed => {
                    firm_core::agentfirm_api::MemberCoordinationStatus::Closed
                }
                firm_core::MemberCoordinationStatus::Retired => {
                    firm_core::agentfirm_api::MemberCoordinationStatus::Retired
                }
            },
            runtime_status: match runtime.status {
                MemberRunStatus::Starting => {
                    firm_core::agentfirm_api::MemberRuntimeStatus::Starting
                }
                MemberRunStatus::Idle => firm_core::agentfirm_api::MemberRuntimeStatus::Idle,
                MemberRunStatus::Queued => firm_core::agentfirm_api::MemberRuntimeStatus::Queued,
                MemberRunStatus::Running => firm_core::agentfirm_api::MemberRuntimeStatus::Running,
                MemberRunStatus::Waiting => firm_core::agentfirm_api::MemberRuntimeStatus::Waiting,
                MemberRunStatus::Disconnected => {
                    firm_core::agentfirm_api::MemberRuntimeStatus::Disconnected
                }
                MemberRunStatus::Reviewing => {
                    firm_core::agentfirm_api::MemberRuntimeStatus::Reviewing
                }
                MemberRunStatus::Blocked => firm_core::agentfirm_api::MemberRuntimeStatus::Blocked,
                MemberRunStatus::Completed => {
                    firm_core::agentfirm_api::MemberRuntimeStatus::Completed
                }
                MemberRunStatus::Failed => firm_core::agentfirm_api::MemberRuntimeStatus::Failed,
                MemberRunStatus::Stopped => firm_core::agentfirm_api::MemberRuntimeStatus::Stopped,
            },
            runtime_generation: runtime.runtime_generation,
            workspace_binding_id: None,
            native_session: runtime.native_session.as_ref().map(|session| {
                serde_json::from_value(
                    serde_json::to_value(session).expect("serialize native session"),
                )
                .expect("map native session")
            }),
            version: 1,
            started_at: runtime.started_at.clone(),
            last_event_at: runtime.last_event_at.clone(),
            finished_at: runtime.finished_at.clone(),
        },
    }
}

fn seed_current_team_run_fixture(
    store: &HarnessStore,
    run: &AgentTeamRun,
    members: &[ProviderRuntimeProjection],
) {
    use firm_core::agentfirm_api::{
        ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, MutationContext,
        PermissionCeiling,
    };

    const SPACE: &str = "unit-test-space";
    let mut run = run.clone();
    let mut members = members.to_vec();
    let host = members
        .first_mut()
        .expect("current TeamRun fixtures must declare an exact Host MemberRun");
    if run
        .host_actor
        .as_ref()
        .is_none_or(|actor| actor.kind != TeamActorKind::Host || actor.id != host.agent_member_id)
    {
        run.host_actor = Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: host.agent_member_id.clone(),
            display_name: Some(host.name.clone()),
            authn_source: Some("test_team_membership:host".into()),
        });
    }
    if run.host_control_mode == firm_core::HostControlMode::ExternalInteractive {
        host.provider_profile = Some(external_interactive_test_profile(&host.provider));
        host.native_session = None;
    } else if host.is_external_interactive() {
        host.provider_profile = None;
    }
    run.member_run_ids = members.iter().map(|member| member.id.clone()).collect();
    store.init().expect("initialize current TeamRun fixture");
    if !store
        .latest_execution_nodes()
        .expect("read Nodes")
        .iter()
        .any(|node| node.id == run.execution_node_id)
    {
        store
            .insert_execution_node(&ExecutionNode {
                id: run.execution_node_id.clone(),
                display_name: "test-node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .expect("insert Node");
    }
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: run.execution_node_id.clone(),
                execution_space_id: SPACE.into(),
                project_binding_id: run.project_binding_id.clone(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            SPACE,
        )
        .expect("register fixture project");
    let mission_id = format!("mission-{}", run.id);
    store
        .append_mission(&Mission {
            id: mission_id.clone(),
            title: mission_id.clone(),
            objective: run.objective.clone(),
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
        .expect("insert fixture Mission");
    let mut identities = members
        .iter()
        .map(|member| member.agent_member_id.clone())
        .collect::<Vec<_>>();
    for member in &members {
        if !store
            .trust_agent_members(SPACE)
            .expect("read AgentMembers")
            .iter()
            .any(|candidate| candidate.id == member.agent_member_id)
        {
            store
                .create_trust_agent_member(
                    &MutationContext {
                        execution_space_id: SPACE.into(),
                        authenticated_actor: ActorRef {
                            kind: ActorKind::Human,
                            id: "fixture-host".into(),
                        },
                        authority_actor: None,
                        command_name: "agent_member.create".into(),
                        idempotency_key: format!("fixture-agent:{}", member.agent_member_id),
                        expected_version: 0,
                        request_fingerprint: None,
                    },
                    AgentMember {
                        id: member.agent_member_id.clone(),
                        name: member.name.clone(),
                        description: "current TeamRun fixture".into(),
                        role: member.role.clone(),
                        capabilities: Vec::new(),
                        skill_refs: Vec::new(),
                        provider_profile_ref: None,
                        model_preference: None,
                        workspace_policy: "test".into(),
                        permission_ceiling: PermissionCeiling::WorkspaceWrite,
                        organization_status: AgentMemberOrganizationStatus::Active,
                        version: 1,
                        created_by: ActorRef {
                            kind: ActorKind::Human,
                            id: "fixture-host".into(),
                        },
                        created_at: "unix-ms:1".into(),
                        updated_at: "unix-ms:1".into(),
                    },
                )
                .expect("create fixture AgentMember");
        }
    }
    if identities.is_empty() {
        identities.push("fixture-host".into());
        store
            .create_trust_agent_member(
                &MutationContext {
                    execution_space_id: SPACE.into(),
                    authenticated_actor: ActorRef {
                        kind: ActorKind::Human,
                        id: "fixture-host".into(),
                    },
                    authority_actor: None,
                    command_name: "agent_member.create".into(),
                    idempotency_key: format!("fixture-agent:{}", run.agent_team_id),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                AgentMember {
                    id: "fixture-host".into(),
                    name: "Fixture Host".into(),
                    description: "current TeamRun fixture".into(),
                    role: "host".into(),
                    capabilities: Vec::new(),
                    skill_refs: Vec::new(),
                    provider_profile_ref: None,
                    model_preference: None,
                    workspace_policy: "test".into(),
                    permission_ceiling: PermissionCeiling::WorkspaceWrite,
                    organization_status: AgentMemberOrganizationStatus::Active,
                    version: 1,
                    created_by: ActorRef {
                        kind: ActorKind::Human,
                        id: "fixture-host".into(),
                    },
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                },
            )
            .expect("create fixture Host AgentMember");
    }
    let team = AgentTeam {
        id: run.agent_team_id.clone(),
        name: run.agent_team_id.clone(),
        description: "current TeamRun fixture".into(),
        legacy_mission_id: Some(mission_id.clone()),
        mission_id,
        host_agent_id: identities.first().cloned().unwrap_or_else(|| "host".into()),
        node_id: run.execution_node_id.clone(),
        status: firm_core::AgentTeamStatus::Active,
        revision: 1,
        trashed_at: None,
        member_ids: identities.clone(),
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    let team_creator = ActorRef {
        kind: ActorKind::Human,
        id: "fixture-host".into(),
    };
    let memberships = identities
        .iter()
        .enumerate()
        .map(
            |(index, member_id)| firm_core::agentfirm_api::TeamMembership {
                id: format!("membership:{}:{}", run.agent_team_id, member_id),
                team_id: run.agent_team_id.clone(),
                agent_member_id: member_id.clone(),
                node_id: run.execution_node_id.clone(),
                role: if index == 0 {
                    firm_core::agentfirm_api::TeamMembershipRole::Host
                } else {
                    firm_core::agentfirm_api::TeamMembershipRole::Member
                },
                state: firm_core::agentfirm_api::TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: team_creator.clone(),
                revision: 1,
                joined_at: "unix-ms:1".into(),
                left_at: None,
            },
        )
        .collect();
    store
        .create_agent_team(
            &MutationContext {
                execution_space_id: SPACE.into(),
                authenticated_actor: team_creator,
                authority_actor: None,
                command_name: "agent_team.create".into(),
                idempotency_key: format!("fixture-team:{}", run.agent_team_id),
                expected_version: 0,
                request_fingerprint: None,
            },
            team,
            memberships,
        )
        .expect("create fixture AgentTeam and Memberships");
    let canonical = members
        .iter()
        .map(|member| canonical_member_admission_for_test(SPACE, member))
        .collect::<Vec<_>>();
    store
        .create_team_run_with_member_runs_from_agent_team(&run, SPACE, &members, &canonical)
        .expect("create coherent current TeamRun fixture");
}

fn seed_host_attention_fixture(
    store: &HarnessStore,
    run_id: &str,
    host_thread_id: Option<&str>,
) -> (AgentTeamRun, ProviderRuntimeProjection, Work) {
    let host_agent_member_id = format!("agent-{run_id}");
    let mut run = AgentTeamRun {
        id: run_id.into(),
        agent_team_id: format!("team-{run_id}"),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "codex-app".into(),
        host_thread_id: host_thread_id.map(str::to_string),
        host_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: host_agent_member_id.clone(),
            display_name: Some("Fixture Host".into()),
            authn_source: Some("test_team_membership:host".into()),
        }),
        host_control_mode: firm_core::HostControlMode::ExternalInteractive,
        objective: "prove exact Host attention".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec![format!("member-{run_id}")],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    let mut member = ProviderRuntimeProjection {
        id: format!("member-{run_id}"),
        team_run_id: run_id.into(),
        slot_id: None,
        agent_member_id: host_agent_member_id.clone(),
        name: "builder".into(),
        role: "builder".into(),
        provider: "kimi".into(),
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
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    };
    member.provider_profile = Some(external_interactive_test_profile(&member.provider));
    run.member_run_ids = vec![member.id.clone()];
    seed_current_team_run_fixture(store, &run, std::slice::from_ref(&member));
    let work = store
        .insert_work(
            Work {
                id: format!("work-{run_id}"),
                team_run_id: run_id.into(),
                accountable_team_id: Some(run.agent_team_id.clone()),
                assignee_membership_id: None,
                legacy_containment_ref: None,
                title: "deliver exact Host attention".into(),
                context_markdown: String::new(),
                completion_criteria_markdown: "Host receives exact durable attention".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_member_id: None,
                created_by_actor: TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: host_agent_member_id.clone(),
                    display_name: None,
                    authn_source: None,
                },
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
                event_id: format!("work-event-{run_id}"),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: host_agent_member_id,
                    display_name: None,
                    authn_source: None,
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("create-work-{run_id}"),
                created_at: "unix-ms:2".into(),
                duplicate_ok: false,
            },
        )
        .expect("seed Work");
    let membership_id = format!(
        "membership:{}:{}",
        run.agent_team_id, member.agent_member_id
    );
    let work = store
        .assign_work_to_membership(
            &work.id,
            work.version,
            &membership_id,
            "unit-test-space",
            WorkCommandContext {
                event_id: format!("work-event-{run_id}-assigned"),
                performed_by_actor: run
                    .host_actor
                    .clone()
                    .expect("fixture has exact Host authority"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("assign-work-{run_id}"),
                created_at: "unix-ms:2".into(),
                duplicate_ok: false,
            },
        )
        .expect("assign seeded Work responsibility");
    (run, member, work)
}

fn seed_test_host_attention(
    store: &HarnessStore,
    run: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
    work: &Work,
    id: &str,
    created_at: &str,
) -> HostAttention {
    let attention = HostAttention {
        id: id.into(),
        team_run_id: run.id.clone(),
        kind: HostAttentionKind::WorkReviewRequested,
        work_id: work.id.clone(),
        work_version: work.version,
        source_event_ref: format!("source-{id}"),
        member_run_id: Some(member.id.clone()),
        status: HostAttentionStatus::Actionable,
        attempt: 0,
        claim_id: None,
        claimed_host_surface: None,
        claimed_host_thread_id: None,
        claimed_host_lease_id: None,
        claimed_host_lease_generation: None,
        claimed_host_lease_owner_id: None,
        claimed_recipient_member_run_id: None,
        claimed_recipient_session_id: None,
        claimed_recipient_session_generation: None,
        claimed_node_daemon_id: None,
        claimed_node_daemon_generation: None,
        provider_receipt_id: None,
        last_failure_reason: None,
        created_at: created_at.into(),
        updated_at: created_at.into(),
    };
    store
        .ensure_host_attention(&attention)
        .expect("seed Host attention");
    attention
}

fn seed_lease_run(store: &HarnessStore, id: &str) {
    let node_id = "00000000-0000-4000-8000-000000000001";
    if !store
        .latest_execution_nodes()
        .expect("read Nodes")
        .iter()
        .any(|node| node.id == node_id)
    {
        store
            .insert_execution_node(&ExecutionNode {
                id: node_id.into(),
                display_name: "test-node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .expect("seed Node");
    }
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: node_id.into(),
                execution_space_id: "space-test".into(),
                project_binding_id: "project-test".into(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            "space-test",
        )
        .expect("seed project registration");
    store
        .legacy_import_append_team_run_projection(&AgentTeamRun {
            id: id.into(),
            agent_team_id: format!("team-{id}"),
            execution_node_id: node_id.into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "lease test".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: Vec::new(),
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        })
        .expect("seed run");
}

trait TestSupervisorLeaseExt {
    fn acquire_test_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        owner_process_id: u32,
        owner_locator: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease>;
}

impl TestSupervisorLeaseExt for HarnessStore {
    fn acquire_test_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        owner_process_id: u32,
        owner_locator: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        let run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|run| run.id == team_run_id)
            .ok_or_else(|| StoreError::Conflict(format!("team run not found: {team_run_id}")))?;
        if !self
            .latest_execution_nodes()?
            .iter()
            .any(|node| node.id == run.execution_node_id)
        {
            self.insert_execution_node(&ExecutionNode {
                id: run.execution_node_id.clone(),
                display_name: "test-node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })?;
        }
        let parent = self.acquire_node_daemon_lease(
            &run.execution_node_id,
            "daemon-test",
            "instance-test",
            now_unix_ms,
            u64::MAX / 2,
        )?;
        self.acquire_team_supervisor_under_node_lease(
            team_run_id,
            &run.execution_node_id,
            &parent.daemon_id,
            parent.generation,
            "space-test",
            &run.project_binding_id,
            supervisor_id,
            owner_process_id,
            owner_locator,
            now_unix_ms,
            ttl_ms,
        )
    }
}

fn append_sparse_row(root: &Path, file_name: &str, row: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(file_name))
        .expect("open jsonl for sparse row");
    writeln!(file, "{row}").expect("write sparse row");
    file.sync_all().expect("sync sparse row");
}

#[cfg(any())]
fn seed_provider_interaction_bridge(
    store: &HarnessStore,
    run_id: &str,
) -> (ProviderInteractionRequestBody, TeamMessageProjection) {
    let member_id = format!("member-{run_id}");
    let session_id = format!("session-{run_id}");
    store
        .legacy_import_append_team_run_projection(&AgentTeamRun {
            id: run_id.to_string(),
            agent_team_id: format!("team-{run_id}"),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some("host-thread".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "provider interaction bridge".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![member_id.clone()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        })
        .expect("seed TeamRun");
    store
        .legacy_import_append_member_run_projection(&ProviderRuntimeProjection {
            id: member_id.clone(),
            team_run_id: run_id.to_string(),
            slot_id: None,
            agent_member_id: format!("agent-{member_id}"),
            name: "provider member".into(),
            role: "worker".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 2,
            status: MemberRunStatus::Waiting,
            native_session: Some(NativeSessionRef {
                provider: "codex".into(),
                execution_mode: "codex_app_server".into(),
                native_session_id: session_id.clone(),
                native_locator_kind: "thread".into(),
                provider_version: None,
                adapter_contract_version: "test".into(),
                availability: NativeSessionAvailability::Available,
                supports_resume: true,
                last_verified_at: None,
                parent_native_session_id: None,
            }),
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            zero_output_streak: 0,
            last_consumed_work_version: None,
            started_at: "unix-ms:1".into(),
            last_event_at: Some("unix-ms:2".into()),
            finished_at: None,
        })
        .expect("seed ProviderRuntimeProjection");
    let body = ProviderInteractionRequestBody {
        interaction_type: ProviderInteractionType::Question,
        prompt: "Select a safe action".into(),
        options: vec![
            ProviderInteractionMessageOption {
                id: "continue".into(),
                label: "Continue".into(),
                intent: Some("approve".into()),
            },
            ProviderInteractionMessageOption {
                id: "stop".into(),
                label: "Stop".into(),
                intent: Some("deny".into()),
            },
        ],
        provider: "codex".into(),
        provider_request_id: format!("provider-request-{run_id}"),
        method: "item/tool/requestUserInput".into(),
        session: session_id,
        member: member_id.clone(),
        generation: 2,
    };
    let request = TeamMessageProjection {
        id: format!("request-{run_id}"),
        team_run_id: run_id.to_string(),
        work_id: None,
        source_plan_ref: None,
        sender: Some(TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member_id.clone(),
            display_name: None,
            authn_source: Some("provider_reverse_rpc".into()),
        }),
        sender_runtime_id: member_id,
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::Host,
            id: "host".into(),
        }],
        recipient_runtime_ids: vec!["host".into()],
        kind: ProviderDispatchIntent::ProviderInteractionRequest,
        body: body.to_canonical_json().expect("request body"),
        correlation_id: body.correlation_id(),
        causation_id: None,
        response_intent: Some(ProviderResponseIntent::ResponseRequired),
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "host".into(),
            policy: TeamDeliveryPolicy::ManualAck,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some("host-surface-receipt".into()),
            failure_reason: None,
            updated_at: "unix-ms:2".into(),
        }],
        created_at: "unix-ms:2".into(),
    };
    store
        .append_team_message_checked(&request)
        .expect("append request");
    (body, request)
}

#[cfg(any())]
fn provider_interaction_response(
    request_body: &ProviderInteractionRequestBody,
    request: &TeamMessageProjection,
    choice: &str,
) -> TeamMessageProjection {
    let body = ProviderInteractionResponseBody {
        interaction_type: request_body.interaction_type,
        choice: Some(choice.to_string()),
        text: None,
        session: request_body.session.clone(),
        member: request_body.member.clone(),
        generation: request_body.generation,
    };
    TeamMessageProjection {
        id: provider_interaction_response_id(&request.id).expect("stable response id"),
        team_run_id: request.team_run_id.clone(),
        work_id: None,
        source_plan_ref: None,
        sender: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: "host".into(),
            display_name: None,
            authn_source: Some("test_host".into()),
        }),
        sender_runtime_id: "host".into(),
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::ProviderRuntimeProjection,
            id: request_body.member.clone(),
        }],
        recipient_runtime_ids: vec![request_body.member.clone()],
        kind: ProviderDispatchIntent::ProviderInteractionResponse,
        body: body.to_canonical_json().expect("response body"),
        correlation_id: request.correlation_id.clone(),
        causation_id: Some(request.id.clone()),
        response_intent: Some(ProviderResponseIntent::Informational),
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: request_body.member.clone(),
            policy: TeamDeliveryPolicy::Inject,
            status: TeamDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: "unix-ms:3".into(),
        }],
        created_at: "unix-ms:3".into(),
    }
}

#[cfg(any())]
fn provider_control_action(run_id: &str, member_id: &str) -> MemberAction {
    MemberAction {
        id: format!("provider-control-{run_id}"),
        seq: 1,
        team_run_id: run_id.to_string(),
        member_run_id: member_id.to_string(),
        task_id: None,
        provider_call_id: Some("permission-request-1".into()),
        action_type: "provider_control".into(),
        status: MemberActionStatus::Succeeded,
        provider_status: Some("acknowledged".into()),
        semantic_status: Some("safe_auto_allow".into()),
        title: "Kimi full-access tool permission acknowledged".into(),
        summary: "bounded safe auto-allow receipt".into(),
        evidence_refs: Vec::new(),
        started_at: "unix-ms:3".into(),
        completed_at: Some("unix-ms:3".into()),
    }
}

fn work_test_fixture(
    name: &str,
) -> (
    PathBuf,
    HarnessStore,
    AgentTeamRun,
    ProviderRuntimeProjection,
    ProviderRuntimeProjection,
) {
    let root = team_test_root(name);
    let store = HarnessStore::new(&root);
    let run = AgentTeamRun {
        id: format!("tr-{name}"),
        agent_team_id: format!("team-{name}"),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "managed".into(),
        host_thread_id: None,
        host_actor: Some(firm_core::TeamActorRef {
            kind: firm_core::TeamActorKind::Host,
            id: "agent-host".into(),
            display_name: Some("Host".into()),
            authn_source: Some("team_membership:host".into()),
        }),
        host_control_mode: firm_core::HostControlMode::Managed,
        objective: "prove Works".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec![
            format!("mr-{name}-host"),
            format!("mr-{name}-a"),
            format!("mr-{name}-b"),
        ],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    let member = |suffix: &str| ProviderRuntimeProjection {
        id: format!("mr-{name}-{suffix}"),
        team_run_id: run.id.clone(),
        slot_id: Some(format!("slot-{suffix}")),
        agent_member_id: format!("agent-{suffix}"),
        name: format!("Member {suffix}"),
        role: "builder".into(),
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
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    };
    let mut host = member("host");
    host.agent_member_id = "agent-host".into();
    host.name = "Host".into();
    host.role = "host".into();
    let member_a = member("a");
    let member_b = member("b");
    seed_current_team_run_fixture(&store, &run, &[host, member_a.clone(), member_b.clone()]);
    (root, store, run, member_a, member_b)
}

fn assign_test_work_to_member(
    store: &HarnessStore,
    run: &AgentTeamRun,
    work: &Work,
    member: &ProviderRuntimeProjection,
    event_id: &str,
    key: &str,
    at: &str,
) -> Work {
    let membership_id = format!(
        "membership:{}:{}",
        run.agent_team_id, member.agent_member_id
    );
    store
        .assign_work_to_membership(
            &work.id,
            work.version,
            &membership_id,
            "unit-test-space",
            host_work_context(event_id, key, at),
        )
        .expect("assign stable Work responsibility")
}

fn host_work_context(id: &str, key: &str, at: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: id.into(),
        performed_by_actor: firm_core::TeamActorRef {
            kind: firm_core::TeamActorKind::Host,
            id: "agent-host".into(),
            display_name: Some("Host".into()),
            authn_source: Some("test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: at.into(),
        duplicate_ok: false,
    }
}

fn member_work_context(member_run_id: &str, id: &str, key: &str, at: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: id.into(),
        performed_by_actor: firm_core::TeamActorRef {
            kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.into(),
            display_name: None,
            authn_source: Some("bound-runtime:test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: at.into(),
        duplicate_ok: false,
    }
}

fn unassigned_test_work(run_id: &str, id: &str) -> Work {
    Work {
        id: id.into(),
        team_run_id: run_id.into(),
        accountable_team_id: None,
        assignee_membership_id: None,
        created_by_member_id: None,
        legacy_containment_ref: None,
        title: format!("Implement Work core — {id}"),
        context_markdown: "Build the smallest correct slice.".into(),
        completion_criteria_markdown: "Tests pass and state is reconstructable.".into(),
        phase: WorkPhase::Open,
        condition: WorkCondition::Normal,
        resolution: None,
        owner_member_id: None,
        active_member_run_id: None,
        claim_mode: WorkClaimMode::TeamClaim,
        eligible_member_ids: Vec::new(),
        prerequisite_work_ids: Vec::new(),
        priority: firm_core::WorkPriority::High,
        created_by_actor: host_work_context("ignored", "ignored", "unix-ms:1").performed_by_actor,
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

fn delegation_test_fixture(
    name: &str,
) -> (
    PathBuf,
    HarnessStore,
    AgentTeamRun,
    ProviderRuntimeProjection,
    AgentTeamRun,
    ProviderRuntimeProjection,
) {
    let root = team_test_root(name);
    let store = HarnessStore::new(&root);
    store.init().expect("initialize delegation store");
    let node_id = "00000000-0000-4000-8000-000000000001";
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: "delegation-test-node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: node_id.into(),
                execution_space_id: "delegation-test-space".into(),
                project_binding_id: "project-test".into(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            "delegation-test-space",
        )
        .expect("register project");

    let make_member = |team_run_id: &str, suffix: &str| ProviderRuntimeProjection {
        id: format!("member-{name}-{suffix}"),
        team_run_id: team_run_id.to_string(),
        slot_id: Some(format!("slot-{suffix}")),
        agent_member_id: format!("agent-{suffix}"),
        name: format!("Member {suffix}"),
        role: "builder".into(),
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
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    };
    let mut rows = Vec::new();
    for suffix in ["a", "b"] {
        let mission_id = format!("mission-{name}-{suffix}");
        let team_id = format!("team-{name}-{suffix}");
        let run_id = format!("run-{name}-{suffix}");
        let member = make_member(&run_id, suffix);
        store
            .append_mission(&Mission {
                id: mission_id.clone(),
                title: format!("Mission {suffix}"),
                objective: format!("Prove Team {suffix} delegation"),
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
            .expect("insert Mission");
        let team_creator = firm_core::agentfirm_api::ActorRef {
            kind: firm_core::agentfirm_api::ActorKind::Human,
            id: "fixture-host".into(),
        };
        let host_id = format!("host-{suffix}");
        let agent_id = format!("agent-{suffix}");
        for (member_id, role) in [(&host_id, "host"), (&agent_id, "builder")] {
            store
                .create_trust_agent_member(
                    &firm_core::agentfirm_api::MutationContext {
                        execution_space_id: "delegation-test-space".into(),
                        authenticated_actor: team_creator.clone(),
                        authority_actor: None,
                        command_name: "agent_member.create".into(),
                        idempotency_key: format!("fixture-member:{member_id}"),
                        expected_version: 0,
                        request_fingerprint: None,
                    },
                    firm_core::agentfirm_api::AgentMember {
                        id: member_id.clone(),
                        name: member_id.clone(),
                        description: "delegation fixture AgentMember".into(),
                        role: role.into(),
                        capabilities: Vec::new(),
                        skill_refs: Vec::new(),
                        provider_profile_ref: None,
                        model_preference: None,
                        workspace_policy: "test".into(),
                        permission_ceiling:
                            firm_core::agentfirm_api::PermissionCeiling::WorkspaceWrite,
                        organization_status:
                            firm_core::agentfirm_api::AgentMemberOrganizationStatus::Active,
                        version: 1,
                        created_by: team_creator.clone(),
                        created_at: "unix-ms:1".into(),
                        updated_at: "unix-ms:1".into(),
                    },
                )
                .expect("create delegation fixture AgentMember");
        }
        let team = AgentTeam {
            id: team_id.clone(),
            name: format!("Team {suffix}"),
            description: "Flat delegation test Team".into(),
            legacy_mission_id: Some(mission_id.clone()),
            mission_id,
            host_agent_id: host_id.clone(),
            node_id: node_id.into(),
            status: firm_core::AgentTeamStatus::Active,
            revision: 1,
            trashed_at: None,
            member_ids: vec![agent_id.clone()],
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        let memberships = [
            (
                host_id.clone(),
                firm_core::agentfirm_api::TeamMembershipRole::Host,
            ),
            (
                agent_id,
                firm_core::agentfirm_api::TeamMembershipRole::Member,
            ),
        ]
        .into_iter()
        .map(
            |(member_id, role)| firm_core::agentfirm_api::TeamMembership {
                id: format!("membership:{team_id}:{member_id}"),
                team_id: team_id.clone(),
                agent_member_id: member_id,
                node_id: node_id.into(),
                role,
                state: firm_core::agentfirm_api::TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: team_creator.clone(),
                revision: 1,
                joined_at: "unix-ms:1".into(),
                left_at: None,
            },
        )
        .collect();
        store
            .create_agent_team(
                &firm_core::agentfirm_api::MutationContext {
                    execution_space_id: "delegation-test-space".into(),
                    authenticated_actor: team_creator,
                    authority_actor: None,
                    command_name: "agent_team.create".into(),
                    idempotency_key: format!("fixture-team:{team_id}"),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                team,
                memberships,
            )
            .expect("create Team and durable Memberships");
        let mut host_runtime = member.clone();
        host_runtime.id = format!("member-{name}-{suffix}-host");
        host_runtime.agent_member_id = host_id.clone();
        host_runtime.name = format!("Host {suffix}");
        host_runtime.role = "host".into();
        let run = AgentTeamRun {
            id: run_id,
            agent_team_id: team_id,
            execution_node_id: node_id.into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "test".into(),
            host_thread_id: None,
            host_actor: Some(TeamActorRef {
                kind: TeamActorKind::Host,
                id: host_id,
                display_name: Some(format!("Host {suffix}")),
                authn_source: Some("test_team_membership:host".into()),
            }),
            host_control_mode: firm_core::HostControlMode::Managed,
            objective: format!("Run Team {suffix}"),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![host_runtime.id.clone(), member.id.clone()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let runtimes = [host_runtime, member.clone()];
        let canonical = runtimes
            .iter()
            .map(|runtime| canonical_member_admission_for_test("delegation-test-space", runtime))
            .collect::<Vec<_>>();
        store
            .create_team_run_with_member_runs_from_agent_team(
                &run,
                "delegation-test-space",
                &runtimes,
                &canonical,
            )
            .expect("create current TeamRun and exact Host MemberRun");
        rows.push((run, member));
    }
    let (run_a, member_a) = rows.remove(0);
    let (run_b, member_b) = rows.remove(0);
    (root, store, run_a, member_a, run_b, member_b)
}

fn assigned_delegation_work(
    run: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
    id: &str,
) -> Work {
    let mut work = unassigned_test_work(&run.id, id);
    work.claim_mode = WorkClaimMode::HostAssign;
    work.owner_member_id = Some(member.agent_member_id.clone());
    work.assignee_membership_id = Some(format!(
        "membership-{}-{}",
        run.agent_team_id, member.agent_member_id
    ));
    work
}

fn delegation_request(id: &str, source: &Work, target_team_id: &str) -> WorkDelegation {
    WorkDelegation {
        id: id.to_string(),
        source_work_ref: WorkRef {
            team_run_id: source.team_run_id.clone(),
            work_id: source.id.clone(),
        },
        source_work_version: source.version,
        source_owner_member_id: source
            .owner_member_id
            .clone()
            .expect("delegation source owner"),
        created_by_member_run_id: None,
        target_agent_team_id: target_team_id.to_string(),
        target_work_ref: WorkRef {
            team_run_id: String::new(),
            work_id: String::new(),
        },
        delegated_by_actor: host_work_context("unused", "unused", "unix-ms:1").performed_by_actor,
        state: WorkDelegationState::Active,
        resolution_summary: None,
        blocker_reason: None,
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn completed_team_run(run: &AgentTeamRun, at: &str) -> AgentTeamRun {
    let mut completed = run.clone();
    completed.status = TeamRunStatus::Completed;
    completed.updated_at = at.into();
    completed.completed_at = Some(at.into());
    completed
}

// ── duplicate-title guard ──────────────────────────────────────────

fn work_with_title(run_id: &str, id: &str, title: &str) -> Work {
    let mut work = unassigned_test_work(run_id, id);
    work.title = title.to_string();
    work
}

fn test_message(id: &str, agent_id: &str) -> RegistryMessage {
    RegistryMessage {
        id: id.into(),
        task_id: Some("task-1".into()),
        from_agent_id: "leader".into(),
        to_agent_id: Some(agent_id.into()),
        channel: Some("assignment".into()),
        kind: RegistryMessageIntent::Message,
        delivery_status: RegistryDeliveryStatus::Queued,
        content: "Do the task".into(),
        evidence_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        delivery: None,
        sender_kind: SenderKind::Agent,
    }
}

fn test_delivery(delivery_id: &str) -> RegistryDeliveryAttempt {
    RegistryDeliveryAttempt {
        delivery_id: Some(delivery_id.into()),
        execution_status: Some(ProviderExecutionStatus::Running),
        native_session: None,
        started_at: Some("unix-ms:1".into()),
        provider_request_id: None,
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: None,
        delivered_at: None,
        last_error: None,
    }
}

fn temp_store(label: &str) -> (PathBuf, HarnessStore) {
    let root = std::env::temp_dir().join(format!(
        "firm-store-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    let store = HarnessStore::new(&root);
    (root, store)
}

// ── Lane B: upstream event push — Work lifecycle → Host attention ──

fn test_github_link(status: &str, ci_status: Option<&str>) -> firm_core::GitHubLink {
    firm_core::GitHubLink {
        kind: firm_core::GitHubLinkKind::PullRequest,
        owner: "cyl19970726".into(),
        repo: "multi-agent-harness".into(),
        number: 365,
        url: "https://github.com/cyl19970726/multi-agent-harness/pull/365".into(),
        status: Some(status.into()),
        ci_status: ci_status.map(str::to_string),
        ci_url: Some("https://github.com/cyl19970726/multi-agent-harness/actions/runs/1".into()),
    }
}

mod append_and_read_delegation_run_jsonl;
mod append_and_read_member_action_jsonl;
mod append_and_read_member_run_jsonl;
#[cfg(any())]
mod append_and_read_team_message_jsonl;
mod append_and_read_team_run_event_jsonl;
mod append_and_read_team_run_jsonl_rejects_legacy_sparse_rows;
mod append_uses_unlocked_existing_lock_file;
mod blocked_work_can_be_resumed_by_owner_or_host_with_a_recorded_resolution;
mod claim_appends_survive_reopen;
mod claim_queued_message_is_atomic_and_blocks_second_claim;
mod closed_member_cannot_mutate_owned_work_until_reopen;
mod concurrent_appends_write_complete_jsonl_rows;
#[cfg(any())]
mod concurrent_current_member_receipts_append_exactly_once;
mod concurrent_distinct_provider_compatibility_admissions_do_not_lose_rows;
mod concurrent_provider_compatibility_command_replay_appends_once;
#[cfg(any())]
mod concurrent_provider_interaction_answers_have_one_winner;
mod concurrent_same_turn_handoffs_allow_exactly_one_append;
mod concurrent_work_claim_has_exactly_one_winner_and_idempotent_retry;
mod current_member_generation_advances_only_through_combined_canonical_cas;
mod duplicate_title_guard_allows_when_existing_is_done;
mod duplicate_title_guard_allows_when_flag_is_duplicate_ok;
mod duplicate_title_guard_normalizes_casing_and_spacing;
mod duplicate_title_guard_refuses_non_terminal_match;
#[cfg(any())]
mod durable_supervisor_lease_and_message_claim_are_cross_process_safe;
mod ensure_team_run_event_is_idempotent_and_rejects_semantic_mismatch;
mod exclusive_migration_guard_blocks_normal_store_writers_until_drop;
#[cfg(any())]
mod fail_queued_delivery_clears_pre_bind_mail_and_is_idempotent;
mod host_attention_dedup_ignores_duplicate_event;
mod host_attention_is_durable_exact_bound_and_semantically_separate;
mod host_binding_interactive_suppresses_dispatch_and_atomic_batch_has_one_winner;
mod host_binding_lease_acquire_renew_release_takeover_and_stale_fence;
mod host_binding_stale_attention_is_derived_and_idempotent;
mod host_mode_transition_is_closed_generation_fenced_and_atomic;
mod informational_mail_neither_fences_handoff_nor_requires_response;
mod jsonl_read_retries_a_concurrently_incomplete_final_row;
mod legacy_assignment_message_is_ignored_by_current_work_store;
mod legacy_raw_work_operation_is_rejected_without_a_read_fallback;
mod legacy_team_message_delivery_mutators_are_explicit_read_only_seams;
mod legacy_team_message_delivery_mutators_reject_with_zero_store_side_effects;
mod malformed_provider_compatibility_ledger_fails_closed_and_roots_are_isolated;
#[cfg(any())]
mod member_close_or_session_cas_wins_before_receipt_with_zero_action;
mod member_close_request_survives_store_reopen_and_is_idempotent;
mod member_created_work_is_limited_to_self_or_unassigned;
mod mission_and_legacy_wave_ledgers_keep_history_and_project_latest_rows;
mod node_project_registration_is_fenced_to_selected_execution_space;
mod provider_compatibility_admission_is_exact_and_preserves_policy;
mod provider_compatibility_admission_replay_is_idempotent_and_id_conflict_fails;
mod provider_compatibility_authority_requires_configured_exact_scope;
mod provider_compatibility_block_lifecycle_rejects_hostile_member_history;
mod provider_compatibility_command_replay_creates_after_terminal_row;
mod provider_compatibility_command_replay_rejects_semantic_drift;
mod provider_compatibility_command_replay_reuses_canonical_active_record;
mod provider_compatibility_ledger_semantic_corruption_fails_closed;
mod provider_compatibility_recovery_authorizes_refreshed_tuple_and_preserves_refusals;
mod provider_compatibility_recovery_rejects_closed_retired_or_finished_block;
mod provider_compatibility_revoke_and_supersede_fence_stale_predecessors;
mod provider_compatibility_scope_is_exact_on_the_same_physical_store;
#[cfg(any())]
mod provider_interaction_response_atomically_acks_and_is_strictly_idempotent;
#[cfg(any())]
mod provider_interaction_response_claim_fences_a_closed_generation;
#[cfg(any())]
mod provider_interaction_response_rejects_unknown_choice_and_predelivery;
#[cfg(any())]
mod raw_provider_interaction_appends_are_forbidden_but_trusted_seams_work;
mod rebind_redelivers_same_member_run_id_at_a_higher_runtime_generation;
mod response_required_mail_is_fenced_until_newer_correlation_reaches_provider;
mod retired_dynamic_workflow_writers_fail_without_creating_ledgers;
mod retired_provider_interaction_response_writer_cannot_reenter_the_store_api;
mod review_link_refresh_derives_a_report_bound_to_the_new_work_version;
mod sparse_mixed_version_rebound_recovers_and_repersists_work_provenance;
mod submit_work_on_pr_merge_transitions_in_progress_work_to_review;
mod submitted_and_blocked_work_reconcile_exactly_one_host_attention_each;
mod supervisor_lease_acquire_compacts_and_keeps_fencing;
mod supervisor_lease_rejects_caller_selected_foreign_execution_space_without_write;
mod supervisor_lease_tail_keeps_a_row_when_window_lands_on_a_boundary;
mod supervisor_lease_tail_read_agrees_with_full_scan;
#[cfg(any())]
mod team_message_work_link_must_resolve_inside_the_same_team_run;
mod team_run_completion_and_work_create_serialize_without_invalid_state;
mod team_run_completion_guard_is_store_authoritative;
mod terminal_team_runs_do_not_materialize_host_binding_stale_attention;
mod typed_provider_block_is_store_owned_and_recovery_is_exact;
mod unavailable_members_and_idempotency_key_reuse_are_rejected;
mod update_work_github_links_refreshes_snapshot_without_churn;
mod work_accept_emits_host_attention_for_bound_run;
mod work_block_emits_host_attention_for_bound_run;
mod work_cancel_emits_host_attention_for_bound_run;
mod work_changes_requested_emits_host_attention_for_bound_run;
mod work_delegation_cancel_is_cas_fenced_and_idempotent;
mod work_delegation_is_atomic_idempotent_and_prevents_flat_team_cycles;
mod work_delegation_rolls_up_target_condition_and_resolution_without_mutating_source;
mod work_dependency_graph_is_cas_fenced_and_cycle_safe;
mod work_event_id_reuse_is_rejected_before_delivery_identity_can_collide;
mod work_submit_emits_host_attention_for_bound_run;
mod work_transitions_dont_fail_for_unbound_run;
mod write_lock_contention_exhaustion_is_bounded_and_typed;
mod write_lock_contention_retries_until_the_owner_releases;
mod write_lock_fifo_admission_skips_timed_out_waiters;
