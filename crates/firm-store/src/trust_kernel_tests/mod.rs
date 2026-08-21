use super::*;
use firm_core::agentfirm_api::{ActorRef, AgentMemberOrganizationStatus, PermissionCeiling};
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static FABRIC_STORE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn actor(id: &str) -> ActorRef {
    ActorRef {
        kind: ActorKind::Human,
        id: id.into(),
    }
}

fn context(actor_id: &str, command: &str, key: &str, expected: u64) -> MutationContext {
    MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: actor(actor_id),
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version: expected,
        request_fingerprint: None,
    }
}

fn member(id: &str) -> AgentMember {
    AgentMember {
        id: id.into(),
        name: "Member".into(),
        description: "Canonical durable member".into(),
        role: "implementer".into(),
        capabilities: vec!["code".into()],
        skill_refs: Vec::new(),
        provider_profile_ref: Some("codex-default".into()),
        model_preference: None,
        workspace_policy: "managed-worktree".into(),
        permission_ceiling: PermissionCeiling::WorkspaceWrite,
        organization_status: AgentMemberOrganizationStatus::Active,
        version: 1,
        created_by: actor("host"),
        created_at: "t1".into(),
        updated_at: "t1".into(),
    }
}

fn service_context(command: &str, key: &str, expected: u64) -> MutationContext {
    MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version: expected,
        request_fingerprint: None,
    }
}

fn identity(id: &str) -> AgentIdentity {
    AgentIdentity {
        id: id.into(),
        display_name: id.into(),
        organization_status: AgentMemberOrganizationStatus::Active,
        permission_ceiling: PermissionCeiling::WorkspaceWrite,
        version: 1,
        created_at: "t1".into(),
        updated_at: "t1".into(),
    }
}

fn session(id: &str, identity_id: &str) -> AgentSession {
    AgentSession {
        id: id.into(),
        agent_member_id: identity_id.into(),
        node_id: "11111111-1111-4111-8111-111111111111".into(),
        execution_space_id: "space-test".into(),
        node_daemon_id: "daemon-1".into(),
        node_daemon_generation: 1,
        provider_kind: "codex".into(),
        provider_profile_ref: "codex-default".into(),
        permission_envelope_ref: "permission-default".into(),
        effective_permission_ceiling: PermissionCeiling::WorkspaceWrite,
        lifecycle: AgentSessionStatus::Idle,
        runtime_generation: 1,
        control_state: firm_core::agentfirm_api::AgentSessionControlState {
            driver_generation: 1,
            driver_ref: firm_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
                node_daemon_id: "daemon-1".into(),
                node_daemon_generation: 1,
            },
            composition_fingerprint: Some("composition:test".into()),
            capability_fingerprint: Some("capability:test".into()),
            ..Default::default()
        },
        native_session_ref: None,
        current_turn_id: None,
        queued_input_count: 0,
        version: 1,
        opened_at: "t1".into(),
        last_active_at: "t1".into(),
        closed_at: None,
    }
}

fn runtime_command_fixture(
    id: &str,
    kind: RuntimeCommandKind,
    session: &AgentSession,
    operation: &str,
) -> (ControlCommandEnvelope, MutationContext) {
    let payload = serde_json::json!({
        "session_id": session.id,
        "session_generation": session.runtime_generation,
        "operation": operation,
        "delivery_id": format!("delivery-{id}"),
    });
    let required_capability = runtime_command_capability(kind);
    let command = ControlCommandEnvelope {
        id: id.into(),
        execution_space_id: session.execution_space_id.clone(),
        target_node_id: session.node_id.clone(),
        target_node_daemon_id: session.node_daemon_id.clone(),
        target_node_daemon_generation: session.node_daemon_generation,
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: session.node_daemon_id.clone(),
        },
        command: kind,
        required_capability: required_capability.into(),
        idempotency_key: id.into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: firm_core::agentfirm_api::RuntimeCommandBinding {
            target_session_id: Some(session.id.clone()),
            target_runtime_generation: Some(session.runtime_generation),
            target_driver_generation: Some(session.control_state.driver_generation),
            target_driver: session.control_state.driver_ref.clone(),
            native_session_ref: session.native_session_ref.clone(),
            composition_fingerprint: session.control_state.composition_fingerprint.clone(),
            capability_fingerprint: session.control_state.capability_fingerprint.clone(),
            permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
            ..Default::default()
        },
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&payload),
        payload,
        issued_at: "t-command".into(),
    };
    let mut context = service_context("node_daemon.runtime.prepare", id, 0);
    context.authority_actor = Some(command.authenticated_actor.clone());
    context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    (command, context)
}

fn test_runtime_binding(session_id: &str) -> firm_core::agentfirm_api::RuntimeCommandBinding {
    firm_core::agentfirm_api::RuntimeCommandBinding {
        target_session_id: Some(session_id.to_string()),
        target_runtime_generation: Some(1),
        target_driver_generation: Some(1),
        target_driver: firm_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
            node_daemon_id: "daemon-1".into(),
            node_daemon_generation: 1,
        },
        composition_fingerprint: Some("composition:test".into()),
        capability_fingerprint: Some("capability:test".into()),
        permission_envelope_ref: Some("permission-default".into()),
        ..Default::default()
    }
}

fn fabric_store() -> (HarnessStore, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "firm-runtime-fabric-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        FABRIC_STORE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    let store = HarnessStore::new(&root);
    store.init().unwrap();
    store
        .insert_execution_node(&firm_core::ExecutionNode {
            id: "11111111-1111-4111-8111-111111111111".into(),
            display_name: "local".into(),
            status: firm_core::ExecutionNodeStatus::Active,
            created_at: "t1".into(),
            updated_at: "t1".into(),
        })
        .unwrap();
    store
        .register_node_project(
            &firm_core::NodeProjectRegistration {
                node_id: "11111111-1111-4111-8111-111111111111".into(),
                execution_space_id: "space-test".into(),
                project_binding_id: "project-1".into(),
                status: firm_core::NodeProjectRegistrationStatus::Active,
                created_at: "t1".into(),
                updated_at: "t1".into(),
            },
            "space-test",
        )
        .unwrap();
    store
        .acquire_node_daemon_lease(
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            "instance-1",
            current_unix_ms(),
            60_000,
        )
        .unwrap();
    (store, root)
}

fn membership_fixture(id: &str, generation: u64) -> TeamMembership {
    TeamMembership {
        id: id.into(),
        team_id: "team-membership-test".into(),
        agent_member_id: "membership-agent".into(),
        node_id: "11111111-1111-4111-8111-111111111111".into(),
        role: firm_core::agentfirm_api::TeamMembershipRole::Member,
        state: TeamMembershipStatus::Active,
        membership_generation: generation,
        default_subscription_refs: Vec::new(),
        created_by: actor("host"),
        revision: 1,
        joined_at: format!("t-join-{generation}"),
        left_at: None,
    }
}

fn append_runtime_team(store: &HarnessStore, team_id: &str, run_id: &str) {
    if !store.teams().unwrap().iter().any(|team| team.id == team_id) {
        let mission_id = format!("mission-{team_id}");
        if !store
            .trust_agent_members("space-test")
            .unwrap()
            .iter()
            .any(|member| member.id == "fixture-host")
        {
            store
                .create_trust_agent_member(
                    &context("host", "agent_member.create", "fixture-host", 0),
                    member("fixture-host"),
                )
                .unwrap();
        }
        if !store
            .latest_missions()
            .unwrap()
            .iter()
            .any(|mission| mission.id == mission_id)
        {
            store
                .append_mission(&firm_core::Mission {
                    id: mission_id.clone(),
                    title: mission_id.clone(),
                    objective: "runtime authority fixture".into(),
                    context: String::new(),
                    desired_outcome: None,
                    status: firm_core::MissionStatus::Running,
                    legacy_wave_ids: Vec::new(),
                    outcome_summary: None,
                    completed_by: None,
                    created_at: "t1".into(),
                    updated_at: "t1".into(),
                    completed_at: None,
                })
                .unwrap();
        }
        let existing_members = store.trust_agent_members("space-test").unwrap();
        let preferred_host = if team_id == "source-team"
            && existing_members
                .iter()
                .any(|member| member.id == "remote-sender")
        {
            "remote-sender".to_string()
        } else {
            let suffix_host = team_id
                .strip_prefix("team-")
                .map(|suffix| format!("host-{suffix}"));
            suffix_host
                .filter(|candidate| {
                    existing_members
                        .iter()
                        .any(|member| member.id == *candidate)
                })
                .unwrap_or_else(|| "fixture-host".into())
        };
        let team = firm_core::AgentTeam {
            id: team_id.into(),
            name: team_id.into(),
            description: "runtime authority fixture".into(),
            legacy_mission_id: Some(mission_id.clone()),
            mission_id,
            host_agent_id: preferred_host.clone(),
            node_id: "11111111-1111-4111-8111-111111111111".into(),
            status: firm_core::AgentTeamStatus::Active,
            revision: 1,
            trashed_at: None,
            member_ids: Vec::new(),
            created_at: "t1".into(),
            updated_at: "t1".into(),
        };
        store
            .create_agent_team(
                &context(
                    "fixture-host",
                    "agent_team.create",
                    &format!("team-{team_id}"),
                    0,
                ),
                team,
                vec![TeamMembership {
                    id: format!("membership:{team_id}:{preferred_host}"),
                    team_id: team_id.into(),
                    agent_member_id: preferred_host,
                    node_id: "11111111-1111-4111-8111-111111111111".into(),
                    role: TeamMembershipRole::Host,
                    state: TeamMembershipStatus::Active,
                    membership_generation: 1,
                    default_subscription_refs: Vec::new(),
                    created_by: actor("fixture-host"),
                    revision: 1,
                    joined_at: "t1".into(),
                    left_at: None,
                }],
            )
            .unwrap();
    }
    store
        .legacy_import_append_team_run_projection(&firm_core::AgentTeamRun {
            id: run_id.into(),
            agent_team_id: team_id.into(),
            execution_node_id: "11111111-1111-4111-8111-111111111111".into(),
            project_binding_id: "project-1".into(),
            previous_run_id: None,
            host_surface: "test".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: firm_core::HostControlMode::External,
            objective: format!("runtime authority for {team_id}"),
            execution_root: None,
            status: firm_core::TeamRunStatus::Running,
            member_run_ids: Vec::new(),
            budget_limit_usd: None,
            created_at: "t1".into(),
            updated_at: "t1".into(),
            completed_at: None,
        })
        .unwrap();
}

fn join_runtime_membership(
    store: &HarnessStore,
    id: &str,
    team_id: &str,
    identity_id: &str,
    role: firm_core::agentfirm_api::TeamMembershipRole,
) -> TeamMembership {
    let membership = TeamMembership {
        id: id.into(),
        team_id: team_id.into(),
        agent_member_id: identity_id.into(),
        node_id: "11111111-1111-4111-8111-111111111111".into(),
        role,
        state: TeamMembershipStatus::Active,
        membership_generation: 1,
        default_subscription_refs: Vec::new(),
        created_by: actor("fixture-host"),
        revision: 1,
        joined_at: "t-join".into(),
        left_at: None,
    };
    store
        .join_team_membership(
            &context("fixture-host", "membership.join", id, 0),
            membership.clone(),
        )
        .unwrap();
    membership
}

fn insert_runtime_work(
    store: &HarnessStore,
    id: &str,
    team_id: &str,
    team_run_id: &str,
) -> firm_core::Work {
    store
        .insert_work(
            firm_core::Work {
                id: id.into(),
                team_run_id: team_run_id.into(),
                accountable_team_id: Some(team_id.into()),
                assignee_membership_id: None,
                parent_work_id: None,
                title: format!("runtime binding {id}"),
                context_markdown: "runtime authority test".into(),
                completion_criteria_markdown: "binding is exact".into(),
                phase: firm_core::WorkPhase::Open,
                condition: firm_core::WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: firm_core::WorkClaimMode::TeamClaim,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: firm_core::WorkPriority::Normal,
                created_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::Host,
                    id: "fixture-host".into(),
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
            },
            firm_core::WorkCommandContext {
                event_id: format!("event-{id}"),
                performed_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::Host,
                    id: "fixture-host".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("work-{id}"),
                created_at: "t-work".into(),
                duplicate_ok: false,
            },
        )
        .unwrap()
}

fn seed_membership_scope(store: &HarnessStore) {
    append_runtime_team(store, "team-membership-test", "team-run-membership-test");
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-membership-agent", 0),
            identity("membership-agent"),
        )
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn peer_authority_fixture(
    company_id: &str,
    source_team: &firm_core::AgentTeam,
    source_membership: &TeamMembership,
    source_member_id: &str,
    source_session_id: &str,
    target_team: &firm_core::AgentTeam,
    target_subscription: &MessageSubscription,
    member_target: Option<&TeamMembership>,
) -> PeerTeamMessageAdmissionAuthority {
    let mut authority = PeerTeamMessageAdmissionAuthority {
        company_id: company_id.into(),
        source_execution_space_id: "space-test".into(),
        source_team_id: source_team.id.clone(),
        source_team_revision: source_team.revision,
        source_membership_id: source_membership.id.clone(),
        source_membership_generation: source_membership.membership_generation,
        source_agent_member_id: source_member_id.into(),
        source_session_id: source_session_id.into(),
        source_session_generation: 1,
        source_node_id: source_team.node_id.clone(),
        source_node_daemon_id: "daemon-1".into(),
        source_node_daemon_generation: 1,
        target_execution_space_id: "space-test".into(),
        target_team_id: target_team.id.clone(),
        target_team_revision: target_team.revision,
        target_node_id: target_team.node_id.clone(),
        target_membership_id: member_target.map(|membership| membership.id.clone()),
        target_membership_generation: member_target
            .map(|membership| membership.membership_generation),
        target_agent_member_id: member_target.map(|membership| membership.agent_member_id.clone()),
        source_policy_ref: "peer-team-message-admission.v1".into(),
        source_policy_revision: 1,
        source_policy_digest: String::new(),
        source_required_capability: "message.peer_team.author".into(),
        target_subscription_id: target_subscription.id.clone(),
        target_subscription_revision: target_subscription.revision,
        target_authorization_policy_ref: target_subscription.authorization_policy_ref.clone(),
        target_policy_revision: target_subscription.policy_revision,
        target_policy_digest: String::new(),
        target_required_capability: "collaboration.peer_message_deliver".into(),
        authority_digest: String::new(),
    };
    authority.source_policy_digest = peer_team_source_policy_digest(&authority);
    authority.target_policy_digest = peer_team_target_policy_digest(&authority);
    authority.authority_digest = peer_team_message_authority_digest(&authority);
    authority
}

fn peer_message_fixture(
    id: &str,
    source_team: &firm_core::AgentTeam,
    sender_member_id: &str,
    sender_session_id: &str,
    recipient: firm_core::agentfirm_api::MessageRecipientRef,
    work_id: Option<&str>,
) -> Message {
    let mut message = Message {
        id: id.into(),
        source_execution_space_id: "space-test".into(),
        source_node_id: source_team.node_id.clone(),
        source_node_daemon_id: "daemon-1".into(),
        source_authority_generation: 1,
        sender_actor_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: sender_member_id.into(),
        },
        sender_agent_member_id: Some(sender_member_id.into()),
        sender_session_id: Some(sender_session_id.into()),
        address_kind: match recipient.kind {
            MessageRecipientKind::Team => firm_core::agentfirm_api::MessageAddressKind::TeamChannel,
            _ => firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
        },
        target_ref: recipient.clone(),
        recipients: vec![recipient],
        team_id: Some(source_team.id.clone()),
        team_run_id: None,
        work_id: work_id.map(str::to_string),
        collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
            source_team_id: source_team.id.clone(),
            target_team_id: "target-team".into(),
            delegation_id: None,
            expected_delegation_revision: None,
            source_work_ref: None,
            target_work_ref: None,
        }),
        kind: firm_core::agentfirm_api::MessageKind::Message,
        body: format!("ordinary peer conversation {id}"),
        body_digest: String::new(),
        correlation_id: format!("correlation-{id}"),
        causation_id: None,
        response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: id.into(),
        created_at: format!("t-{id}"),
    };
    message.body_digest = format!("sha256:{:x}", Sha256::digest(message.body.as_bytes()));
    message.content_fingerprint = message_content_fingerprint(&message);
    message
}

fn seed_peer_message_scope(
    store: &HarnessStore,
) -> (
    firm_core::AgentTeam,
    firm_core::AgentTeam,
    TeamMembership,
    TeamMembership,
) {
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.migrate", "peer-sender", 0),
            identity("remote-sender"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "peer-sender-session", 0),
            session("session-peer-sender", "remote-sender"),
        )
        .unwrap();
    append_runtime_team(store, "source-team", "source-peer-run");
    append_runtime_team(store, "target-team", "target-peer-run");
    let source_team = store
        .agent_teams("space-test")
        .unwrap()
        .into_iter()
        .find(|team| team.id == "source-team")
        .unwrap();
    let target_team = store
        .agent_teams("space-test")
        .unwrap()
        .into_iter()
        .find(|team| team.id == "target-team")
        .unwrap();
    let source_membership = store
        .team_host_membership("space-test", "source-team", true)
        .unwrap();
    let target_membership = store
        .team_host_membership("space-test", "target-team", true)
        .unwrap();
    (
        source_team,
        target_team,
        source_membership,
        target_membership,
    )
}

fn settled_native_session(id: &str) -> NativeSessionRef {
    NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: id.into(),
        native_locator_kind: "codex_rollout".into(),
        provider_version: None,
        adapter_contract_version: "codex-app-server-v1".into(),
        availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("t2".into()),
        parent_native_session_id: None,
    }
}

mod admitted_stop_closes_exact_session_once_and_replays_after_terminal_state;
mod agent_session_reattach_preserves_native_identity_and_fences_daemon_driver;
mod agent_session_reattach_rejects_expiry_without_provider_drain_receipt;
mod bind_agent_session_native_session_is_cas_generation_fenced_and_idempotent;
mod bind_member_run_native_session_is_cas_generation_fenced_and_idempotent;
mod canonical_operation_is_atomic_scoped_and_exactly_idempotent;
mod concurrent_team_membership_join_has_one_linearized_winner;
mod control_state_binding_is_quiescent_generation_fenced_and_exactly_replayable;
mod interrupt_is_the_only_successor_admitted_while_start_cycle_is_in_flight;
mod legacy_session_json_is_readable_but_cannot_admit_an_unbound_new_effect;
mod node_daemon_authors_and_claims_identity_first_message;
mod peer_team_authority_keeps_source_and_target_fences_distinct_then_claims_one_membership;
mod peer_team_claim_replays_exactly_and_resolved_delivery_rejects_new_claims;
mod peer_team_direct_membership_target_binds_one_delivery_without_claim;
mod peer_team_message_work_link_is_context_bound_to_the_source_team;
mod peer_team_target_subscription_revision_advances_with_team_lifecycle;
mod provider_continuation_driver_requires_exact_active_armed_generation;
mod recovery_cannot_confirm_applied_after_semantic_precondition_drift;
mod remote_message_persists_before_delivery_and_replays_without_route_duplication;
mod runtime_command_effect_matrix_is_exactly_replayable_and_fingerprint_closed;
mod runtime_command_exact_binding_rejects_stale_fields_before_acceptance;
mod runtime_command_failure_certainty_and_torn_rows_recover_without_duplicate_effect;
mod runtime_command_hostile_member_and_permission_widening_have_zero_side_effects;
mod runtime_command_replay_and_ambiguous_effect_fail_closed;
mod runtime_command_replay_precedes_successor_fence_but_stale_settlement_is_zero_effect;
mod runtime_command_semantic_preconditions_are_lock_checked_with_zero_side_effects;
mod runtime_command_settlement_rechecks_the_prepared_semantic_snapshot;
mod runtime_command_team_supervisor_generation_is_live_fenced_at_prepare_and_settle;
mod runtime_control_rejects_missing_turn_and_requires_explicit_binding_release_before_stop;
mod runtime_recovery_resolution_is_operator_fenced_replay_safe_and_never_blind_replays;
mod same_id_team_migration_fails_closed_on_alias_and_purge_records_no_delete_tombstone;
mod source_node_authors_cross_node_message_only_with_frozen_delegation_authority;
mod standalone_session_is_machine_owned_and_team_membership_is_only_an_overlay;
mod team_host_cannot_stop_shared_session_and_active_bindings_require_explicit_release;
mod team_membership_is_single_active_generation_and_rejoin_is_exact_successor;
mod team_trash_restore_preserves_work_message_membership_and_native_session_records;
mod terminal_session_rejects_every_provider_runtime_effect_with_zero_delta;
