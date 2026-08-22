use super::*;
use std::sync::atomic::AtomicUsize;

trait TestSupervisorLeaseExt {
    fn acquire_test_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        owner_process_id: u32,
        owner_locator: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<TeamSupervisorLease, StoreError>;
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
    ) -> Result<TeamSupervisorLease, StoreError> {
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
                display_name: "test-node".to_string(),
                status: ExecutionNodeStatus::Active,
                created_at: "unix-ms:0".to_string(),
                updated_at: "unix-ms:0".to_string(),
            })?;
        }
        let daemon = match self.latest_node_daemon_lease(&run.execution_node_id)? {
            Some(lease)
                if lease.status == NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > now_unix_ms =>
            {
                lease
            }
            _ => self.acquire_node_daemon_lease(
                &run.execution_node_id,
                "test-node-daemon",
                "test-node-daemon-instance",
                now_unix_ms,
                u64::MAX / 2,
            )?,
        };
        let execution_space_id = run
            .member_run_ids
            .iter()
            .find_map(|member_run_id| self.trust_member_run_scope(member_run_id).ok().flatten())
            .unwrap_or_else(|| "test-execution-space".to_string());
        self.acquire_team_supervisor_under_node_lease(
            team_run_id,
            &run.execution_node_id,
            &daemon.daemon_id,
            daemon.generation,
            &execution_space_id,
            &run.project_binding_id,
            supervisor_id,
            owner_process_id,
            owner_locator,
            now_unix_ms,
            ttl_ms,
        )
    }
}

fn latest_heartbeat_ms(store: &HarnessStore, team_run_id: &str) -> u64 {
    store
        .latest_team_supervisor_lease(team_run_id)
        .expect("supervisor lease read")
        .expect("supervisor lease row")
        .heartbeat_unix_ms
}

/// Wait until the durable heartbeat moves past `from` (bounded by 5s).
fn wait_until_heartbeat_advances(
    store: &HarnessStore,
    team_run_id: &str,
    from: u64,
    what: &str,
) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let heartbeat = latest_heartbeat_ms(store, team_run_id);
        if heartbeat != from {
            return heartbeat;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the heartbeat to {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn migration_test_project(tag: &str) -> (PathBuf, PathBuf, ProjectContext) {
    let root = std::env::temp_dir().join(format!(
        "firm-space-atomic-migration-{tag}-{}",
        generated_id("test")
    ));
    let firm_home = root.join("home");
    let project_root = root.join("project");
    fs::create_dir_all(&project_root).expect("project root");
    let project_context = project::register_and_activate(&firm_home, &project_root, "unix-ms:1")
        .expect("register project");
    HarnessStore::new(project_context.store_root.clone())
        .init()
        .expect("source store");
    (root, firm_home, project_context)
}

fn migration_args(project_id: &str, space_id: &str, force: bool) -> Vec<String> {
    let mut args = vec![
        "--from-project".into(),
        project_id.into(),
        "--id".into(),
        space_id.into(),
    ];
    if force {
        args.push("--force".into());
    }
    args
}

fn hidden_migration_paths(firm_home: &Path, space_id: &str) -> Vec<PathBuf> {
    let prefix = format!(".{space_id}.");
    let Ok(entries) = fs::read_dir(execution_space::spaces_dir(firm_home)) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect()
}

fn continuation_test_work(
    phase: WorkPhase,
    condition: WorkCondition,
    resolution: Option<WorkResolution>,
) -> Work {
    serde_json::from_value(serde_json::json!({
        "id": format!("work-{phase:?}-{condition:?}"),
        "team_run_id": "team-run-test",
        "title": "continuation gate",
        "context_markdown": "",
        "completion_criteria_markdown": "prove the gate",
        "phase": phase,
        "condition": condition,
        "resolution": resolution,
        "owner_member_id": "agent-member-test",
        "active_member_run_id": "member-run-test",
        "claim_mode": "host_assign",
        "eligible_member_ids": [],
        "prerequisite_work_ids": [],
        "priority": "normal",
        "created_by_actor": {
            "kind": "host",
            "id": "host",
            "display_name": null,
            "authn_source": "test"
        },
        "result_summary": null,
        "blocker_reason": null,
        "artifact_refs": [],
        "check_refs": [],
        "version": 1,
        "created_at": "unix-ms:1",
        "updated_at": "unix-ms:1"
    }))
    .expect("valid continuation test Work")
}

fn native_open_test_member(
    provider: &str,
    mode: &str,
    session_id: &str,
) -> ProviderRuntimeProjection {
    ProviderRuntimeProjection {
        id: "member-native-open".into(),
        team_run_id: "team-native-open".into(),
        slot_id: None,
        agent_member_id: "agent-native-open".into(),
        name: "DesktopObserver".into(),
        role: "reviewer".into(),
        provider: provider.into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: MemberCoordinationStatus::Active,
        runtime_generation: 1,
        status: MemberRunStatus::Idle,
        native_session: Some(NativeSessionRef {
            provider: provider.into(),
            execution_mode: mode.into(),
            native_session_id: session_id.into(),
            native_locator_kind: "claude_project_jsonl".into(),
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
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    }
}

fn persisted_native_test_member(
    store: &HarnessStore,
    provider: &str,
    mode: &str,
    session_id: &str,
) -> (TeamRunLedger, ProviderRuntimeProjection) {
    let created = create_team_run(
        store,
        None,
        None,
        None,
        "Exercise provider callback validation",
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &[
            TeamMemberSpec {
                agent_member_id: "host".into(),
                name: "Host".into(),
                role: "host".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
            TeamMemberSpec {
                agent_member_id: "agent-native-open".into(),
                name: "ProviderCallback".into(),
                role: "reviewer".into(),
                provider: provider.into(),
                execution_mode: Some(mode.into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
        ],
    )
    .expect("create persisted provider callback member");
    let initial = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-native-open")
        .expect("provider callback MemberRun")
        .clone();
    let mut running = initial.clone();
    running.status = MemberRunStatus::Running;
    running.native_session = Some(NativeSessionRef {
        provider: provider.into(),
        execution_mode: mode.into(),
        native_session_id: session_id.into(),
        native_locator_kind: "test_native_session".into(),
        provider_version: None,
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: None,
        parent_native_session_id: None,
    });
    store
        .compare_and_append_member_run(&initial, &running)
        .expect("seed persisted provider callback member");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            &format!("provider-callback-{provider}"),
            std::process::id(),
            "test://provider-callback",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire provider callback Supervisor");
    ensure_test_runtime_fabric(store, &created, &lease);
    let ledger = TeamRunLedger::new(
        store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    transition_provider_session_for_member(
        &ledger,
        &running,
        harness_core::agentfirm_api::AgentSessionStatus::Active,
    )
    .expect("activate provider callback AgentSession");
    (ledger, running)
}

fn kimi_safe_approval_frame(session_id: &str, id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/request_permission",
        "params": {
            "sessionId": session_id,
            "options": [
                {"optionId": format!("tool_allow_always_{id}"), "name": "Always allow", "kind": "allow_always"},
                {"optionId": "tool_reject_once", "name": "Reject", "kind": "reject_once"}
            ],
            "toolCall": {"toolCallId": format!("{id}:bash"), "title": "Bash"}
        }
    })
}

fn assert_unchanged_profile_refresh_has_no_in_memory_revision() {
    let mut member = native_open_test_member("codex", "codex_app_server", "session-profile");
    member.provider_profile = Some(team_member_provider_profile_for_mode(
        "codex",
        Some("codex_app_server"),
    ));
    member.last_event_at = Some("unix-ms:stable".into());
    let before = member.clone();
    let changed = apply_refreshed_provider_profile(
        &mut member,
        before.provider_profile.clone().expect("profile"),
    );
    assert!(!changed);
    assert_eq!(member, before);

    let mut changed_profile = before.provider_profile.clone().expect("profile");
    changed_profile.compatibility_note = Some("refreshed contract".into());
    assert!(apply_refreshed_provider_profile(
        &mut member,
        changed_profile.clone()
    ));
    assert_eq!(member.provider_profile.as_ref(), Some(&changed_profile));
    assert_ne!(member.last_event_at.as_deref(), Some("unix-ms:stable"));
}

fn compatibility_block_action(
    member: &ProviderRuntimeProjection,
    profile: &ProviderIntegrationProfile,
    seq: u64,
) -> MemberAction {
    let resolution = ProviderCompatibilityResolution {
        allowed: false,
        needs_review: true,
        status: ProviderCompatibilityStatus::ReviewRequired,
        source: "adapter_compatibility",
        policy: None,
        admission: None,
        probe_error: None,
        warning: None,
    };
    MemberAction {
        id: format!("compatibility-block-{seq}"),
        seq,
        team_run_id: member.team_run_id.clone(),
        member_run_id: member.id.clone(),
        task_id: None,
        provider_call_id: None,
        action_type: "provider_compatibility_blocked".into(),
        status: MemberActionStatus::Failed,
        provider_status: None,
        semantic_status: None,
        title: "provider compatibility gate blocked persistent execution".into(),
        summary: provider_compatibility_block_reason(
            member,
            profile,
            &resolution,
            "start persistent execution",
        )
        .expect("review-required profile blocks"),
        evidence_refs: Vec::new(),
        started_at: "unix-ms:1".into(),
        completed_at: Some("unix-ms:1".into()),
    }
}

fn compatibility_test_cause(
    member: &ProviderRuntimeProjection,
    profile: &ProviderIntegrationProfile,
) -> ProviderCompatibilityBlockCause {
    ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: "typed-compatibility-cause".into(),
        member_run_id: member.id.clone(),
        provider: profile.provider.clone(),
        execution_mode: profile.execution_mode.clone(),
        provider_version: profile.provider_version.clone().unwrap(),
        adapter_contract_version: profile.adapter_contract_version.clone().unwrap(),
        boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        source: ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: "unix-ms:1".into(),
    }
}

fn make_member(id: &str) -> ProviderLaunchProfile {
    ProviderLaunchProfile {
        id: id.into(),
        name: "Member".into(),
        description: "Test member".into(),
        role: "worker".into(),
        provider: "codex".into(),
        model: None,
        profile: None,
        provider_config: harness_core::ProviderLaunchConfig::default(),
        capabilities: Vec::new(),
        team_ids: Vec::new(),
        prompt_ref: None,
        skill_refs: Vec::new(),
        workspace_policy: None,
        provider_cwd_hint: None,
        permission_profile: None,
        runtime_workspace_roots: Vec::new(),
        status: ProviderLaunchStatus::Idle,
        current_task_id: None,
        current_proposal_id: None,
        provider_runtime_id: None,
        native_session: None,
        provider_thread_id: None,
        provider_agent_path: None,
        provider_agent_nickname: None,
        provider_agent_role: None,
        control_endpoint: None,
        created_at: "unix-ms:1".into(),
        last_seen_at: None,
    }
}

fn append_test_delivery_attempt(
    store: &HarnessStore,
    agent_id: &str,
    task_id: Option<&str>,
    status: ProviderExecutionStatus,
    thread_id: Option<&str>,
    turn_id: Option<&str>,
) {
    store
        .append_message(&RegistryMessage {
            id: generated_id("message"),
            task_id: task_id.map(str::to_string),
            from_agent_id: "lead-1".into(),
            to_agent_id: Some(agent_id.into()),
            channel: Some("assignment".into()),
            kind: RegistryMessageIntent::Message,
            delivery_status: RegistryDeliveryStatus::Acknowledged,
            content: "test delivery".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: Some(RegistryDeliveryAttempt {
                delivery_id: Some("delivery-1".into()),
                execution_status: Some(status),
                native_session: None,
                started_at: Some("unix-ms:1".into()),
                provider_request_id: None,
                provider_thread_id: thread_id.map(str::to_string),
                provider_turn_id: turn_id.map(str::to_string),
                terminal_source: None,
                delivered_at: None,
                last_error: None,
            }),
            sender_kind: SenderKind::Agent,
        })
        .expect("append delivery attempt");
}

fn temp_store(label: &str) -> (HarnessStore, PathBuf) {
    let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id(label)));
    (HarnessStore::new(&root), root)
}

// ------------------------------------------------------------------
// Peer-Team messaging (DOC-106) surface fixtures and tests.
// ------------------------------------------------------------------

struct PeerMessagingFixture {
    store: HarnessStore,
    root: PathBuf,
    firm_home: PathBuf,
    node_id: String,
    source_team_id: String,
    target_team_id: String,
    sender_member_id: String,
    target_member_id: String,
}

impl PeerMessagingFixture {
    fn new(tag: &str) -> Self {
        let (store, root) = temp_store(tag);
        let firm_home =
            std::env::temp_dir().join(format!("harness-cli-home-{}", generated_id(tag)));
        std::fs::create_dir_all(&firm_home).expect("firm home");
        // DOC-108: no Company registry is seeded; the local peer-message
        // admission label defaults to the Execution Space scope.
        let node_id = "11111111-1111-4111-8111-111111111111".to_string();
        let space_id = "space-test";
        store.init().expect("store init");
        store
            .insert_execution_node(&harness_core::ExecutionNode {
                id: node_id.clone(),
                display_name: "local".into(),
                status: harness_core::ExecutionNodeStatus::Active,
                created_at: "t1".into(),
                updated_at: "t1".into(),
            })
            .expect("insert ExecutionNode");
        store
            .register_node_project(
                &harness_core::NodeProjectRegistration {
                    node_id: node_id.clone(),
                    execution_space_id: space_id.into(),
                    project_binding_id: "project-1".into(),
                    status: harness_core::NodeProjectRegistrationStatus::Active,
                    created_at: "t1".into(),
                    updated_at: "t1".into(),
                },
                space_id,
            )
            .expect("register Node project");
        store
            .acquire_node_daemon_lease(
                &node_id,
                "daemon-1",
                "instance-1",
                current_unix_ms_u64(),
                3_600_000,
            )
            .expect("daemon lease");
        let creator = harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Human,
            id: "operator".into(),
        };
        let member_context = |id: &str| harness_core::agentfirm_api::MutationContext {
            execution_space_id: space_id.into(),
            authenticated_actor: creator.clone(),
            authority_actor: None,
            command_name: "unit_test.agent_member.create".into(),
            idempotency_key: format!("unit-test-member:{id}"),
            expected_version: 0,
            request_fingerprint: None,
        };
        for id in ["sender-member", "target-member"] {
            store
                .create_trust_agent_member(
                    &member_context(id),
                    harness_core::agentfirm_api::AgentMember {
                        id: id.into(),
                        name: id.into(),
                        description: "peer messaging fixture".into(),
                        role: "host".into(),
                        capabilities: Vec::new(),
                        skill_refs: Vec::new(),
                        provider_profile_ref: Some("codex".into()),
                        model_preference: None,
                        workspace_policy: "managed-worktree".into(),
                        permission_ceiling:
                            harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite,
                        organization_status:
                            harness_core::agentfirm_api::AgentMemberOrganizationStatus::Active,
                        version: 1,
                        created_by: creator.clone(),
                        created_at: "t1".into(),
                        updated_at: "t1".into(),
                    },
                )
                .expect("create AgentMember");
        }
        let team_context = |key: &str| harness_core::agentfirm_api::MutationContext {
            execution_space_id: space_id.into(),
            authenticated_actor: creator.clone(),
            authority_actor: None,
            command_name: "unit_test.agent_team.create".into(),
            idempotency_key: key.into(),
            expected_version: 0,
            request_fingerprint: None,
        };
        let create_team = |store: &HarnessStore, team_id: &str, host_id: &str| {
            store
                .create_agent_team(
                    &team_context(&format!("unit-test-team:{team_id}")),
                    AgentTeam {
                        id: team_id.into(),
                        name: team_id.into(),
                        description: "peer messaging fixture".into(),
                        node_id: node_id.clone(),
                        status: AgentTeamStatus::Active,
                        revision: 1,
                        legacy_mission_id: Some(format!("mission-{team_id}")),
                        trashed_at: None,
                        mission_id: format!("mission-{team_id}"),
                        host_agent_id: host_id.into(),
                        member_ids: Vec::new(),
                        created_at: "t1".into(),
                        updated_at: "t1".into(),
                    },
                    vec![harness_core::agentfirm_api::TeamMembership {
                        id: format!("membership:{team_id}:{host_id}"),
                        team_id: team_id.into(),
                        agent_member_id: host_id.into(),
                        node_id: node_id.clone(),
                        role: harness_core::agentfirm_api::TeamMembershipRole::Host,
                        state: harness_core::agentfirm_api::TeamMembershipStatus::Active,
                        membership_generation: 1,
                        default_subscription_refs: Vec::new(),
                        created_by: creator.clone(),
                        revision: 1,
                        joined_at: "t1".into(),
                        left_at: None,
                    }],
                )
                .expect("create AgentTeam");
        };
        create_team(&store, "source-team", "sender-member");
        create_team(&store, "target-team", "target-member");
        store
            .create_agent_session(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: space_id.into(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: "daemon-1".into(),
                    },
                    authority_actor: None,
                    command_name: "unit_test.session.create".into(),
                    idempotency_key: "unit-test-session:sender".into(),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                harness_core::agentfirm_api::AgentSession {
                    id: "session-sender".into(),
                    agent_member_id: "sender-member".into(),
                    node_id: node_id.clone(),
                    execution_space_id: space_id.into(),
                    node_daemon_id: "daemon-1".into(),
                    node_daemon_generation: 1,
                    provider_kind: "codex".into(),
                    provider_profile_ref: "codex-default".into(),
                    permission_envelope_ref: "permission-default".into(),
                    effective_permission_ceiling:
                        harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite,
                    lifecycle: harness_core::agentfirm_api::AgentSessionStatus::Idle,
                    runtime_generation: 1,
                    control_state: harness_core::agentfirm_api::AgentSessionControlState {
                        driver_generation: 1,
                        driver_ref: harness_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
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
                },
            )
            .expect("sender AgentSession");
        Self {
            store,
            root,
            firm_home,
            node_id,
            source_team_id: "source-team".into(),
            target_team_id: "target-team".into(),
            sender_member_id: "sender-member".into(),
            target_member_id: "target-member".into(),
        }
    }

    fn team_draft(&self, body: &str) -> harness_core::agentfirm_api::MessageDraft {
        let recipient = harness_core::agentfirm_api::MessageRecipientRef {
            kind: harness_core::agentfirm_api::MessageRecipientKind::Team,
            id: self.target_team_id.clone(),
        };
        harness_core::agentfirm_api::MessageDraft {
            address_kind: harness_core::agentfirm_api::MessageAddressKind::TeamChannel,
            target_ref: recipient.clone(),
            recipients: vec![recipient],
            team_id: Some(self.source_team_id.clone()),
            team_run_id: None,
            work_id: None,
            collaboration_scope: Some(harness_core::collaboration::CollaborationScope {
                source_team_id: self.source_team_id.clone(),
                target_team_id: self.target_team_id.clone(),
                delegation_id: None,
                expected_delegation_revision: None,
                source_work_ref: None,
                target_work_ref: None,
            }),
            kind: harness_core::agentfirm_api::MessageKind::Message,
            body: body.into(),
            correlation_id: "correlation-test".into(),
            causation_id: None,
            response_intent: harness_core::agentfirm_api::ResponseIntent::Informational,
            evidence_refs: Vec::new(),
            schema_version: 1,
        }
    }

    fn sender_actor(&self) -> harness_core::agentfirm_api::ActorRef {
        harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: self.sender_member_id.clone(),
        }
    }

    fn cleanup(&self) {
        std::fs::remove_dir_all(&self.root).expect("cleanup store root");
        std::fs::remove_dir_all(&self.firm_home).expect("cleanup firm home");
    }
}

fn durable_store_file_bytes(store: &HarnessStore) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    if !store.root().exists() {
        return files;
    }
    for entry in std::fs::read_dir(store.root()).expect("read test Store") {
        let entry = entry.expect("read Store entry");
        let file_type = entry.file_type().expect("read Store entry type");
        if !file_type.is_file() || entry.file_name() == ".store.lock" {
            continue;
        }
        files.insert(
            entry.file_name().to_string_lossy().into_owned(),
            std::fs::read(entry.path()).expect("read durable Store file"),
        );
    }
    files
}

fn create_two_member_team_run(store: &HarnessStore) -> CreatedTeamRun {
    create_two_member_team_run_for_provider(store, "codex")
}

fn create_two_member_team_run_for_provider(store: &HarnessStore, provider: &str) -> CreatedTeamRun {
    let execution_mode = match provider {
        "codex" => "codex_app_server",
        "claude" => "claude_agent_sdk",
        "kimi" => "kimi_acp",
        "pi" => "pi_rpc",
        _ => panic!("unsupported test provider {provider}"),
    };
    create_team_run(
        store,
        None,
        None,
        None,
        "Build two independent modules",
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &[
            TeamMemberSpec {
                agent_member_id: "agent-builder-a".into(),
                name: "BuilderA".into(),
                role: "module_a".into(),
                provider: provider.into(),
                execution_mode: Some(execution_mode.into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: vec!["crates/a".into()],
                resume_native_session_id: None,
                initial_work: None,
            },
            TeamMemberSpec {
                agent_member_id: "agent-builder-b".into(),
                name: "BuilderB".into(),
                role: "module_b".into(),
                provider: provider.into(),
                execution_mode: Some(execution_mode.into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: vec!["crates/b".into()],
                resume_native_session_id: None,
                initial_work: None,
            },
            TeamMemberSpec {
                agent_member_id: "host".into(),
                name: "Host".into(),
                role: "host".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
        ],
    )
    .expect("create team run")
}

fn ensure_test_runtime_fabric(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    lease: &TeamSupervisorLease,
) {
    ensure_team_message_fabric(
        store,
        &created.team_run.id,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect("materialize canonical test AgentSessions and TeamMemberships");
}

fn ensure_foreign_test_message_fabric(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    lease: &TeamSupervisorLease,
    execution_space_id: &str,
) {
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: created.team_run.execution_node_id.clone(),
                execution_space_id: execution_space_id.to_string(),
                project_binding_id: created.team_run.project_binding_id.clone(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:foreign-space".into(),
                updated_at: "unix-ms:foreign-space".into(),
            },
            execution_space_id,
        )
        .expect("register colliding foreign Execution Space");
    let team = store
        .latest_teams()
        .expect("read test Team")
        .remove(&created.team_run.agent_team_id)
        .expect("test Team");
    let members = created
        .member_runs
        .iter()
        .map(|member| TeamMemberSpec {
            agent_member_id: member.agent_member_id.clone(),
            name: member.name.clone(),
            role: member.role.clone(),
            provider: member.provider.clone(),
            execution_mode: member
                .provider_profile
                .as_ref()
                .map(|profile| profile.execution_mode.clone()),
            model: member.model.clone(),
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: member.owned_paths.clone(),
            resume_native_session_id: None,
            initial_work: None,
        })
        .collect::<Vec<_>>();
    ensure_unit_test_canonical_team(store, execution_space_id, &team, &members)
        .expect("materialize foreign durable AgentTeam and TeamMemberships");
    ensure_team_message_fabric(
        store,
        &created.team_run.id,
        execution_space_id,
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect("materialize foreign canonical AgentSessions and TeamMemberships");
}

#[allow(clippy::too_many_arguments)]
fn author_test_canonical_message(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    lease: &TeamSupervisorLease,
    execution_space_id: &str,
    id: &str,
    sender_identity_id: &str,
    recipient_agent_member_id: &str,
    kind: harness_core::agentfirm_api::MessageKind,
    body: &str,
    correlation_id: &str,
    causation_id: Option<&str>,
    response_intent: harness_core::agentfirm_api::ResponseIntent,
) -> harness_core::agentfirm_api::Message {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, Message, MessageAddressKind, MessageRecipientKind,
        MessageRecipientRef, MutationContext,
    };
    use sha2::{Digest, Sha256};

    let session = store
        .fabric_agent_sessions(execution_space_id)
        .expect("canonical test AgentSessions")
        .into_iter()
        .find(|session| session.agent_member_id == sender_identity_id)
        .expect("sender has one canonical AgentSession");
    let sender = ActorRef {
        kind: ActorKind::AgentMember,
        id: sender_identity_id.to_string(),
    };
    let recipient = MessageRecipientRef {
        kind: MessageRecipientKind::AgentMember,
        id: recipient_agent_member_id.to_string(),
    };
    let recipients = vec![recipient.clone()];
    let body_digest = format!("sha256:{:x}", Sha256::digest(body.as_bytes()));
    let idempotency_key = format!("test-author:{id}");
    let content_fingerprint = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": sender,
        "sender_agent_member_id": sender_identity_id,
        "sender_session_id": session.id,
        "address_kind": MessageAddressKind::DirectAgent,
        "target_ref": recipient,
        "recipients": recipients,
        "team_id": created.team_run.agent_team_id,
        "team_run_id": created.team_run.id,
        "work_id": null,
        "collaboration_scope": null,
        "kind": kind,
        "body": body,
        "body_digest": body_digest,
        "correlation_id": correlation_id,
        "causation_id": causation_id,
        "response_intent": response_intent,
        "evidence_refs": Vec::<String>::new(),
        "schema_version": 1,
        "idempotency_key": idempotency_key,
    }));
    let message = Message {
        id: id.to_string(),
        source_execution_space_id: execution_space_id.to_string(),
        source_node_id: session.node_id.clone(),
        source_node_daemon_id: lease.node_daemon_id.clone(),
        source_authority_generation: lease.node_daemon_generation,
        sender_actor_ref: sender,
        sender_agent_member_id: Some(sender_identity_id.to_string()),
        sender_session_id: Some(session.id),
        address_kind: MessageAddressKind::DirectAgent,
        target_ref: recipient,
        recipients,
        team_id: Some(created.team_run.agent_team_id.clone()),
        team_run_id: Some(created.team_run.id.clone()),
        work_id: None,
        collaboration_scope: None,
        kind,
        body: body.to_string(),
        body_digest,
        correlation_id: correlation_id.to_string(),
        causation_id: causation_id.map(str::to_string),
        response_intent,
        evidence_refs: Vec::new(),
        content_fingerprint,
        schema_version: 1,
        idempotency_key: idempotency_key.clone(),
        created_at: now_string(),
    };
    store
        .author_message(
            &MutationContext {
                execution_space_id: execution_space_id.to_string(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: lease.node_daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "test.message.author".into(),
                idempotency_key,
                expected_version: 0,
                request_fingerprint: None,
            },
            message.clone(),
        )
        .expect("author canonical Message");
    message
}

struct FaithfulProviderControlShim {
    provider: &'static str,
    primitive: crate::provider_adapter::NativeControlPrimitive,
    native_effects: usize,
    fail_after_dispatch: bool,
}

impl crate::provider_adapter::ProviderNativeControl for FaithfulProviderControlShim {
    fn provider(&self) -> &'static str {
        self.provider
    }

    fn dispatch(
        &mut self,
        plan: &crate::provider_adapter::ProviderControlPlan,
    ) -> Result<(), String> {
        if plan.primitive != self.primitive {
            return Err("faithful shim primitive mismatch".into());
        }
        self.native_effects += 1;
        if self.fail_after_dispatch {
            Err("faithful shim transport lost after native dispatch".into())
        } else {
            Ok(())
        }
    }
}

fn test_provider_environment_observation(root: &Path) -> MemberWorkspaceSnapshot {
    snapshot_member_workspace(root, None, None, "explicit_unbound")
}

fn capacity_test_snapshot(state: ProviderCapacityState) -> ProviderCapacitySnapshot {
    ProviderCapacitySnapshot {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        account: ProviderAccountRef::unknown(),
        state,
        observed_at: "unix-ms:100".into(),
        observed_unix_ms: 100,
        reset_at: None,
        evidence_source: ProviderCapacityEvidence::ProviderError,
        confidence: ProviderCapacityConfidence::Observed,
        windows: Vec::new(),
        diagnosis: None,
        runtime_context: Vec::new(),
        detail: Some("capacity recovery regression fixture".into()),
    }
}

fn capacity_test_session() -> NativeSessionRef {
    NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "thread-capacity-recovery".into(),
        native_locator_kind: "thread_id".into(),
        provider_version: Some("test".into()),
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("unix-ms:99".into()),
        parent_native_session_id: None,
    }
}

fn seed_capacity_blocked_member(
    store: &HarnessStore,
    initial: &ProviderRuntimeProjection,
) -> ProviderRuntimeProjection {
    let mut blocked = initial.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.provider_capacity = Some(capacity_test_snapshot(ProviderCapacityState::Exhausted));
    blocked.last_event_at = Some("unix-ms:100".into());
    store
        .compare_and_append_member_run(initial, &blocked)
        .expect("seed capacity-origin Blocked member");
    blocked
}

#[cfg(any())]
fn seed_host_conversation(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    member_index: usize,
) -> TeamMessageProjection {
    send_team_message(
        store,
        &created.team_run.id,
        "host",
        vec![created.member_runs[member_index].id.clone()],
        ProviderDispatchIntent::Message,
        "Coordination context",
        None,
        None,
        None,
        Some(ProviderResponseIntent::ResponseRequired),
    )
    .expect("seed conversation")
}

struct FakeHostSessionValidator {
    receipt: Result<HostSessionValidationReceipt, String>,
}

impl HostSessionValidator for FakeHostSessionValidator {
    fn validate(
        &self,
        _request: &HostSessionValidationRequest<'_>,
    ) -> Result<HostSessionValidationReceipt, String> {
        self.receipt.clone()
    }
}

fn claude_probe_snapshot(proxy: Option<&str>) -> ProviderCapacitySnapshot {
    ProviderCapacitySnapshot {
        provider: "claude".into(),
        execution_mode: "claude_agent_sdk".into(),
        account: ProviderAccountRef {
            source: "oauth_credentials_file".into(),
            identifier: None,
            plan: None,
        },
        state: ProviderCapacityState::Unknown,
        observed_at: "unix-ms:2000".into(),
        observed_unix_ms: 2_000,
        reset_at: None,
        evidence_source: ProviderCapacityEvidence::AuthMetadata,
        confidence: ProviderCapacityConfidence::Unknown,
        windows: Vec::new(),
        diagnosis: None,
        runtime_context: vec![ProviderRuntimeContextFact {
            key: "HTTPS_PROXY".into(),
            present: proxy.is_some(),
            note: Some(proxy.unwrap_or("absent").into()),
        }],
        detail: Some("auth metadata only".into()),
    }
}

fn provider_error_action(
    member_run_id: &str,
    started_at: &str,
    summary: &str,
    provider_status: Option<&str>,
) -> MemberAction {
    MemberAction {
        id: generated_id("mact"),
        seq: 1,
        team_run_id: "team-run-1".into(),
        member_run_id: member_run_id.into(),
        task_id: None,
        provider_call_id: None,
        action_type: "provider_error".into(),
        status: MemberActionStatus::Failed,
        provider_status: provider_status.map(str::to_string),
        semantic_status: None,
        title: "round 1 provider_error".into(),
        summary: summary.into(),
        evidence_refs: Vec::new(),
        started_at: started_at.into(),
        completed_at: None,
    }
}

/// The canonical token a Claude Agent SDK 403 round writes.
fn claude_403_status() -> String {
    ProviderTerminalFailure {
        reason: "api_error".into(),
        http_status: Some(403),
    }
    .to_provider_status()
}

// --- cheatsheet anti-drift tests ---
//
// The CHEATSHEET_* consts above are hand-curated free text, not
// generated from a schema, so nothing stops them from drifting out of
// sync with the real argv-parsing code in this file. These tests treat
// main.rs's own source as the source of truth instead of checking the
// cheatsheet against a generic helper in isolation: every documented
// subcommand leaf must appear as a real match arm in the right
// dispatcher function, and every documented flag must appear as an
// argument to this file's own argv-parsing primitives
// (`value`/`many`/`has_flag`/`required`) somewhere in the file.
// `flag_checker_rejects_a_fabricated_flag` below proves the checker
// actually discriminates real flags from made-up ones.

/// Load the production command surface so the checks below follow commands
/// across the composition root and its semantically owned modules. Test source
/// is deliberately excluded so a fabricated flag in an assertion cannot make
/// itself appear wired.
fn cli_command_source() -> String {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = vec![src.join("main.rs")];
    paths.extend(
        std::fs::read_dir(src.join("main_modules"))
            .expect("main_modules directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs")),
    );
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract every distinct `--flag-name` token in `text`, including ones
/// glued to punctuation with no separating space (e.g. `[--to`,
/// `--all-delivered]`, `--kind|--other`). Scans byte-by-byte instead of
/// splitting on whitespace so no punctuation-adjacent flag is ever
/// silently skipped (the bug in the version this replaces: its
/// `starts_with("-[")` guard was backwards and dropped ~40% of the
/// flags in CHEATSHEET_ALL from coverage).
fn extract_flags(text: &str) -> std::collections::BTreeSet<&str> {
    let bytes = text.as_bytes();
    let mut flags = std::collections::BTreeSet::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if &bytes[i..i + 2] == b"--" && bytes[i + 2].is_ascii_lowercase() {
            let start = i;
            let mut end = i + 3;
            while end < bytes.len() && (bytes[end].is_ascii_lowercase() || bytes[end] == b'-') {
                end += 1;
            }
            // A real flag name never ends in '-': trim a trailing run
            // picked up from adjacent punctuation like
            // "--response-required|--informational".
            while end > start + 3 && bytes[end - 1] == b'-' {
                end -= 1;
            }
            flags.insert(&text[start..end]);
            i = end;
        } else {
            i += 1;
        }
    }
    flags
}

/// The exact source text of the named top-level function, bounded from
/// its `fn <name>(` signature to the function-closing `}` that starts a
/// line at column 0. rustfmt always places a top-level item's closing
/// brace there, and nothing inside a function body in this file --
/// even a `format!("...{}...")` string literal -- legitimately starts a
/// line with a bare `}`, so this stays correct without a full
/// brace-matching lexer.
fn function_body<'a>(source: &'a str, fn_name: &str) -> &'a str {
    let needle = format!("fn {fn_name}(");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("fn {fn_name} not found in the CLI command source"));
    let end = source[start..]
        .find("\n}\n")
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("end of fn {fn_name} not found"));
    &source[start..end]
}

/// True if `"leaf" =>` appears as a match arm inside `body` -- i.e. the
/// subcommand is a real match arm, not just documented text.
fn subcommand_is_real(body: &str, leaf: &str) -> bool {
    body.contains(&format!("\"{leaf}\" =>"))
}

/// True if `flag` is read by one of this file's own argv-parsing
/// primitives (`value`, `many`, `has_flag`, `required`) anywhere in
/// `source` -- i.e. the flag is really wired, not just typed into the
/// cheatsheet text. Matching against the whole file rather than one
/// dispatcher's body is deliberate: a few flags (e.g.
/// `--expected-version`) are read through a small named helper one
/// level removed from the dispatcher (`required_work_version`), and the
/// flag string only appears as a literal inside that helper.
fn flag_is_wired(source: &str, flag: &str) -> bool {
    ["value", "many", "has_flag", "required"]
        .iter()
        .any(|func| source.contains(&format!("{func}(args, \"{flag}\")")))
        || source.contains(&format!("args[i] == \"{flag}\""))
}

#[path = "general_suites/host_binding.rs"]
mod host_binding;
#[path = "general_suites/member_runtime.rs"]
mod member_runtime;
#[path = "general_suites/messaging_delivery.rs"]
mod messaging_delivery;
#[path = "general_suites/protocol_and_utilities.rs"]
mod protocol_and_utilities;
#[path = "general_suites/provider_admission_capacity.rs"]
mod provider_admission_capacity;
#[path = "general_suites/team_work.rs"]
mod team_work;
#[path = "general_suites/workspace_workflow.rs"]
mod workspace_workflow;
