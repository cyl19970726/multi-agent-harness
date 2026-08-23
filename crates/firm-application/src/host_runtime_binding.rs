use std::fmt;

use firm_core::agentfirm_api::{
    AgentSession, AgentSessionStatus, MemberCoordinationStatus, MemberExecutionDriver, MemberRun,
    RuntimeDriverRef, RuntimeResidency, TeamMembership, TeamMembershipRole, TeamMembershipStatus,
};
use firm_core::{
    AgentTeam, AgentTeamRun, HostControlMode, NodeDaemonLease, NodeDaemonLeaseStatus,
    ProviderRuntimeProjection, TeamActorKind, TeamSupervisorLease, TeamSupervisorLeaseStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostMemberBinding {
    pub host_agent_member_id: String,
    pub host_membership: TeamMembership,
    pub member_run: MemberRun,
    pub runtime: ProviderRuntimeProjection,
    pub mode: HostControlMode,
}

impl HostMemberBinding {
    pub fn is_active(&self) -> bool {
        self.member_run.coordination_status == MemberCoordinationStatus::Active
            && self.runtime.coordination_is_active()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedHostRuntimeBinding {
    pub host_agent_member_id: String,
    pub host_membership: TeamMembership,
    pub member_run: MemberRun,
    pub runtime: ProviderRuntimeProjection,
    pub agent_session: AgentSession,
    pub node_daemon: NodeDaemonLease,
    pub team_supervisor: TeamSupervisorLease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalInteractiveHostRuntimeBinding {
    pub host_agent_member_id: String,
    pub host_membership: TeamMembership,
    pub member_run: MemberRun,
    pub runtime: ProviderRuntimeProjection,
    pub pull_inbox_member_run_id: String,
    pub external_surface: String,
    pub external_thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeBinding {
    Managed(Box<ManagedHostRuntimeBinding>),
    ExternalInteractive(Box<ExternalInteractiveHostRuntimeBinding>),
}

impl HostRuntimeBinding {
    pub fn host_agent_member_id(&self) -> &str {
        match self {
            Self::Managed(binding) => &binding.host_agent_member_id,
            Self::ExternalInteractive(binding) => &binding.host_agent_member_id,
        }
    }

    pub fn member_run_id(&self) -> &str {
        match self {
            Self::Managed(binding) => &binding.member_run.id,
            Self::ExternalInteractive(binding) => &binding.member_run.id,
        }
    }

    pub fn mode(&self) -> HostControlMode {
        match self {
            Self::Managed(_) => HostControlMode::Managed,
            Self::ExternalInteractive(_) => HostControlMode::ExternalInteractive,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeBindingError {
    pub code: &'static str,
    pub detail: String,
}

impl HostRuntimeBindingError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for HostRuntimeBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for HostRuntimeBindingError {}

#[derive(Clone, Copy)]
pub struct HostRuntimeBindingFacts<'a> {
    pub team: &'a AgentTeam,
    pub team_run: &'a AgentTeamRun,
    pub memberships: &'a [TeamMembership],
    pub member_runs: &'a [MemberRun],
    pub runtimes: &'a [ProviderRuntimeProjection],
    pub agent_sessions: &'a [AgentSession],
    pub node_daemon: Option<&'a NodeDaemonLease>,
    pub team_supervisor: Option<&'a TeamSupervisorLease>,
    pub observed_unix_ms: u64,
}

pub fn resolve_host_runtime_binding(
    facts: HostRuntimeBindingFacts<'_>,
) -> Result<HostRuntimeBinding, HostRuntimeBindingError> {
    let host = resolve_host_member_binding(&facts)?;
    match host.mode {
        HostControlMode::Managed => resolve_managed(facts, host),
        HostControlMode::ExternalInteractive => resolve_external(facts, host),
    }
}

pub fn resolve_host_member_binding(
    facts: &HostRuntimeBindingFacts<'_>,
) -> Result<HostMemberBinding, HostRuntimeBindingError> {
    let run = facts.team_run;
    let team = facts.team;
    if run.agent_team_id != team.id || run.execution_node_id != team.node_id {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_TEAM_FENCED",
            format!(
                "TeamRun {} does not bind exact AgentTeam {} placement",
                run.id, team.id
            ),
        ));
    }
    let memberships = facts
        .memberships
        .iter()
        .filter(|membership| {
            membership.team_id == team.id
                && membership.role == TeamMembershipRole::Host
                && membership.state == TeamMembershipStatus::Active
        })
        .collect::<Vec<_>>();
    let [membership] = memberships.as_slice() else {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_MEMBERSHIP_AMBIGUOUS",
            format!(
                "AgentTeam {} has {} active Host memberships",
                team.id,
                memberships.len()
            ),
        ));
    };
    let host_id = membership.agent_member_id.as_str();
    if team.host_agent_id != host_id
        || run
            .host_actor
            .as_ref()
            .is_none_or(|actor| actor.kind != TeamActorKind::Host || actor.id != host_id)
    {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_AUTHORITY_MISMATCH",
            format!(
                "Team {}, TeamRun {}, and Host membership do not identify one exact AgentMember",
                team.id, run.id
            ),
        ));
    }
    let member_runs = facts
        .member_runs
        .iter()
        .filter(|member| {
            member.team_run_id == run.id
                && member.agent_member_id == host_id
                && run.member_run_ids.iter().any(|id| id == &member.id)
        })
        .collect::<Vec<_>>();
    let [member_run] = member_runs.as_slice() else {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_MEMBER_RUN_AMBIGUOUS",
            format!(
                "TeamRun {} has {} active exact Host MemberRuns",
                run.id,
                member_runs.len()
            ),
        ));
    };
    let runtimes = facts
        .runtimes
        .iter()
        .filter(|runtime| {
            runtime.id == member_run.id
                && runtime.team_run_id == run.id
                && runtime.agent_member_id == host_id
                && runtime.runtime_generation == member_run.runtime_generation
        })
        .collect::<Vec<_>>();
    let [runtime] = runtimes.as_slice() else {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_PROJECTION_AMBIGUOUS",
            format!(
                "Host MemberRun {} has {} exact current runtime projections",
                member_run.id,
                runtimes.len()
            ),
        ));
    };
    Ok(HostMemberBinding {
        host_agent_member_id: host_id.to_string(),
        host_membership: (*membership).clone(),
        member_run: (*member_run).clone(),
        runtime: (*runtime).clone(),
        mode: run.host_control_mode,
    })
}

fn resolve_managed(
    facts: HostRuntimeBindingFacts<'_>,
    host: HostMemberBinding,
) -> Result<HostRuntimeBinding, HostRuntimeBindingError> {
    require_active_host(&host)?;
    if host.runtime.is_external_interactive() || facts.team_run.host_thread_id.is_some() {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_MODE_CONFLICT",
            "managed Host carries external runtime or thread binding",
        ));
    }
    let supervisor = facts.team_supervisor.ok_or_else(|| {
        HostRuntimeBindingError::new(
            "HOST_RUNTIME_SUPERVISOR_MISSING",
            format!(
                "managed Host TeamRun {} has no Supervisor",
                facts.team_run.id
            ),
        )
    })?;
    let daemon = facts.node_daemon.ok_or_else(|| {
        HostRuntimeBindingError::new(
            "HOST_RUNTIME_NODE_DAEMON_MISSING",
            format!("managed Host Team {} has no NodeDaemon", facts.team.id),
        )
    })?;
    if supervisor.team_run_id != facts.team_run.id
        || supervisor.node_id != facts.team.node_id
        || supervisor.project_binding_id != facts.team_run.project_binding_id
        || supervisor.status != TeamSupervisorLeaseStatus::Active
        || supervisor.expires_unix_ms <= facts.observed_unix_ms
        || daemon.node_id != facts.team.node_id
        || daemon.daemon_id != supervisor.node_daemon_id
        || daemon.generation != supervisor.node_daemon_generation
        || daemon.status != NodeDaemonLeaseStatus::Active
        || daemon.expires_unix_ms <= facts.observed_unix_ms
    {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_SUPERVISOR_FENCED",
            format!(
                "managed Host TeamRun {} does not own the exact live NodeDaemon/Supervisor generations",
                facts.team_run.id
            ),
        ));
    }
    let expected_driver = RuntimeDriverRef::TeamSupervisor {
        team_run_id: facts.team_run.id.clone(),
        team_supervisor_id: supervisor.supervisor_id.clone(),
        team_supervisor_generation: supervisor.generation,
    };
    let sessions = facts
        .agent_sessions
        .iter()
        .filter(|session| {
            session.agent_member_id == host.member_run.agent_member_id
                && session.execution_space_id == supervisor.execution_space_id
                && session.node_id == daemon.node_id
                && session.node_daemon_id == daemon.daemon_id
                && session.node_daemon_generation == daemon.generation
                && session.provider_kind == host.runtime.provider
                && session.lifecycle != AgentSessionStatus::Closed
                && session.control_state.runtime_residency == RuntimeResidency::Attached
                && session.control_state.execution_driver == MemberExecutionDriver::HostDriven
                && session.control_state.driver_ref == expected_driver
        })
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_SESSION_AMBIGUOUS",
            format!(
                "managed Host {} has {} exact current AgentSessions",
                host.member_run.agent_member_id,
                sessions.len()
            ),
        ));
    };
    let member_native = host.member_run.native_session.as_ref().map(|native| {
        (
            native.provider.as_str(),
            native.execution_mode.as_str(),
            native.native_session_id.as_str(),
        )
    });
    let runtime_native = host.runtime.native_session.as_ref().map(|native| {
        (
            native.provider.as_str(),
            native.execution_mode.as_str(),
            native.native_session_id.as_str(),
        )
    });
    let session_native = session.native_session_ref.as_ref().map(|native| {
        (
            native.provider.as_str(),
            native.execution_mode.as_str(),
            native.native_session_id.as_str(),
        )
    });
    if member_native != runtime_native || member_native != session_native {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_NATIVE_SESSION_FENCED",
            format!(
                "managed Host MemberRun {}, runtime projection, and AgentSession do not bind one exact provider-native Session",
                host.member_run.id
            ),
        ));
    }
    Ok(HostRuntimeBinding::Managed(Box::new(
        ManagedHostRuntimeBinding {
            host_agent_member_id: host.host_agent_member_id,
            host_membership: host.host_membership,
            member_run: host.member_run,
            runtime: host.runtime,
            agent_session: (*session).clone(),
            node_daemon: daemon.clone(),
            team_supervisor: supervisor.clone(),
        },
    )))
}

fn resolve_external(
    facts: HostRuntimeBindingFacts<'_>,
    host: HostMemberBinding,
) -> Result<HostRuntimeBinding, HostRuntimeBindingError> {
    require_active_host(&host)?;
    if !host.runtime.is_external_interactive() {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_MODE_CONFLICT",
            "external_interactive Host carries a managed runtime profile",
        ));
    }
    if host.member_run.native_session.is_some() || host.runtime.native_session.is_some() {
        return Err(HostRuntimeBindingError::new(
            "EXTERNAL_HOST_NATIVE_SESSION_FORBIDDEN",
            format!(
                "external_interactive Host MemberRun {} carries a managed native-session projection",
                host.member_run.id
            ),
        ));
    }
    let current_sessions = facts
        .agent_sessions
        .iter()
        .filter(|session| {
            session.agent_member_id == host.member_run.agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
        })
        .count();
    if current_sessions != 0 {
        return Err(HostRuntimeBindingError::new(
            "EXTERNAL_HOST_SESSION_FORBIDDEN",
            format!(
                "external_interactive Host {} has {current_sessions} current AgentSessions",
                host.member_run.agent_member_id
            ),
        ));
    }
    Ok(HostRuntimeBinding::ExternalInteractive(Box::new(
        ExternalInteractiveHostRuntimeBinding {
            host_agent_member_id: host.host_agent_member_id,
            host_membership: host.host_membership,
            pull_inbox_member_run_id: host.member_run.id.clone(),
            member_run: host.member_run,
            runtime: host.runtime,
            external_surface: facts.team_run.host_surface.clone(),
            external_thread_id: facts.team_run.host_thread_id.clone(),
        },
    )))
}

fn require_active_host(host: &HostMemberBinding) -> Result<(), HostRuntimeBindingError> {
    if !host.is_active() {
        return Err(HostRuntimeBindingError::new(
            "HOST_RUNTIME_MEMBER_RUN_INACTIVE",
            format!(
                "Host MemberRun {} is not active in both canonical and runtime projections",
                host.member_run.id
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use firm_core::agentfirm_api::{
        AgentSessionControlState, MemberRuntimeStatus, NativeSessionAvailability, NativeSessionRef,
        PermissionCeiling, RuntimeActivity,
    };
    use firm_core::{
        AgentTeamStatus, ControlTopology, MemberCoordinationStatus as RuntimeCoordinationStatus,
        MemberRunStatus, OrdinaryMessageBoundary, ProviderBindingAdmission,
        ProviderCompatibilityStatus, ProviderEventFidelity, ProviderExecutionControls,
        ProviderFeatureMode, ProviderIntegrationProfile, ProviderInteractionMode,
        SecurityEnforcementLocus, EXECUTION_MODE_EXTERNAL_INTERACTIVE,
    };

    struct Fixture {
        team: AgentTeam,
        run: AgentTeamRun,
        memberships: Vec<TeamMembership>,
        members: Vec<MemberRun>,
        runtimes: Vec<ProviderRuntimeProjection>,
        sessions: Vec<AgentSession>,
        daemon: NodeDaemonLease,
        supervisor: TeamSupervisorLease,
    }

    impl Fixture {
        fn managed() -> Self {
            let host_id = "host-member".to_string();
            let member_run_id = "host-member-run".to_string();
            let team = AgentTeam {
                id: "team-1".into(),
                name: "Team".into(),
                description: "fixture".into(),
                node_id: "node-1".into(),
                status: AgentTeamStatus::Active,
                revision: 1,
                legacy_mission_id: None,
                trashed_at: None,
                created_at: "t1".into(),
                updated_at: "t1".into(),
                mission_id: String::new(),
                host_agent_id: host_id.clone(),
                member_ids: Vec::new(),
            };
            let run = AgentTeamRun {
                id: "run-1".into(),
                agent_team_id: team.id.clone(),
                execution_node_id: team.node_id.clone(),
                project_binding_id: "project-1".into(),
                previous_run_id: None,
                host_surface: "codex".into(),
                host_thread_id: None,
                host_actor: Some(firm_core::TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: host_id.clone(),
                    display_name: None,
                    authn_source: None,
                }),
                host_control_mode: HostControlMode::Managed,
                objective: "test".into(),
                execution_root: None,
                status: firm_core::TeamRunStatus::Running,
                member_run_ids: vec![member_run_id.clone()],
                budget_limit_usd: None,
                created_at: "t1".into(),
                updated_at: "t1".into(),
                completed_at: None,
            };
            let membership = TeamMembership {
                id: "membership-host".into(),
                team_id: team.id.clone(),
                agent_member_id: host_id.clone(),
                node_id: team.node_id.clone(),
                role: TeamMembershipRole::Host,
                state: TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: firm_core::agentfirm_api::ActorRef {
                    kind: firm_core::agentfirm_api::ActorKind::Human,
                    id: "operator".into(),
                },
                revision: 1,
                joined_at: "t1".into(),
                left_at: None,
            };
            let member_native_session = NativeSessionRef {
                provider: "codex".into(),
                execution_mode: "codex_app_server".into(),
                native_session_id: "thread-1".into(),
                native_locator_kind: "thread_id".into(),
                provider_version: None,
                adapter_contract_version: "codex-app-server-v1".into(),
                availability: NativeSessionAvailability::Available,
                supports_resume: true,
                last_verified_at: None,
                parent_native_session_id: None,
            };
            let member = MemberRun {
                id: member_run_id.clone(),
                agent_member_id: host_id.clone(),
                team_run_id: run.id.clone(),
                role_snapshot: "Host".into(),
                provider_profile_snapshot: Some("codex-default".into()),
                requested_controls: Default::default(),
                effective_controls: Default::default(),
                coordination_status: MemberCoordinationStatus::Active,
                runtime_status: MemberRuntimeStatus::Running,
                runtime_generation: 2,
                workspace_binding_id: None,
                native_session: Some(member_native_session.clone()),
                version: 1,
                started_at: "t1".into(),
                last_event_at: None,
                finished_at: None,
            };
            let runtime = ProviderRuntimeProjection {
                id: member_run_id,
                team_run_id: run.id.clone(),
                slot_id: None,
                agent_member_id: host_id.clone(),
                name: "Host".into(),
                role: "Host".into(),
                provider: "codex".into(),
                model: None,
                provider_controls: ProviderExecutionControls::default(),
                provider_profile: None,
                provider_capacity: None,
                provider_compatibility_block_cause: None,
                coordination_status: RuntimeCoordinationStatus::Active,
                runtime_generation: 2,
                status: MemberRunStatus::Running,
                native_session: Some(firm_core::NativeSessionRef {
                    provider: "codex".into(),
                    execution_mode: "codex_app_server".into(),
                    native_session_id: "thread-1".into(),
                    native_locator_kind: "thread_id".into(),
                    provider_version: None,
                    adapter_contract_version: "codex-app-server-v1".into(),
                    availability: firm_core::NativeSessionAvailability::Available,
                    supports_resume: true,
                    last_verified_at: None,
                    parent_native_session_id: None,
                }),
                provider_cwd_hint: None,
                provider_environment_observation: None,
                owned_paths: Vec::new(),
                zero_output_streak: 0,
                last_consumed_work_version: None,
                started_at: "t1".into(),
                last_event_at: None,
                finished_at: None,
            };
            let daemon = NodeDaemonLease {
                node_id: team.node_id.clone(),
                daemon_id: "daemon-1".into(),
                generation: 7,
                instance_id: "instance-1".into(),
                status: NodeDaemonLeaseStatus::Active,
                acquired_unix_ms: 1,
                renewed_unix_ms: 1,
                expires_unix_ms: 10_000,
                released_unix_ms: None,
            };
            let supervisor = TeamSupervisorLease {
                team_run_id: run.id.clone(),
                node_id: team.node_id.clone(),
                node_daemon_id: daemon.daemon_id.clone(),
                node_daemon_generation: daemon.generation,
                execution_space_id: "space-1".into(),
                project_binding_id: run.project_binding_id.clone(),
                supervisor_id: "supervisor-1".into(),
                generation: 3,
                owner_process_id: 1,
                owner_locator: "test".into(),
                status: TeamSupervisorLeaseStatus::Active,
                acquired_unix_ms: 1,
                heartbeat_unix_ms: 1,
                expires_unix_ms: 10_000,
                released_unix_ms: None,
            };
            let session = AgentSession {
                id: "session-1".into(),
                agent_member_id: host_id,
                node_id: team.node_id.clone(),
                execution_space_id: supervisor.execution_space_id.clone(),
                node_daemon_id: daemon.daemon_id.clone(),
                node_daemon_generation: daemon.generation,
                provider_kind: "codex".into(),
                provider_profile_ref: "codex-default".into(),
                permission_envelope_ref: "trusted-development".into(),
                effective_permission_ceiling: PermissionCeiling::FullAccess,
                workspace_cwd: Some("/tmp/worktree".into()),
                lifecycle: AgentSessionStatus::Idle,
                runtime_generation: 1,
                control_state: AgentSessionControlState {
                    runtime_residency: RuntimeResidency::Attached,
                    activity: RuntimeActivity::Idle,
                    execution_driver: MemberExecutionDriver::HostDriven,
                    driver_generation: supervisor.generation,
                    driver_ref: RuntimeDriverRef::TeamSupervisor {
                        team_run_id: run.id.clone(),
                        team_supervisor_id: supervisor.supervisor_id.clone(),
                        team_supervisor_generation: supervisor.generation,
                    },
                    ..Default::default()
                },
                native_session_ref: Some(member_native_session),
                current_turn_id: None,
                queued_input_count: 0,
                version: 1,
                opened_at: "t1".into(),
                last_active_at: "t1".into(),
                closed_at: None,
            };
            Self {
                team,
                run,
                memberships: vec![membership],
                members: vec![member],
                runtimes: vec![runtime],
                sessions: vec![session],
                daemon,
                supervisor,
            }
        }

        fn facts(&self) -> HostRuntimeBindingFacts<'_> {
            HostRuntimeBindingFacts {
                team: &self.team,
                team_run: &self.run,
                memberships: &self.memberships,
                member_runs: &self.members,
                runtimes: &self.runtimes,
                agent_sessions: &self.sessions,
                node_daemon: Some(&self.daemon),
                team_supervisor: Some(&self.supervisor),
                observed_unix_ms: 100,
            }
        }

        fn external() -> Self {
            let mut fixture = Self::managed();
            fixture.run.host_control_mode = HostControlMode::ExternalInteractive;
            fixture.run.host_thread_id = Some("external-thread-1".into());
            fixture.sessions.clear();
            fixture.members[0].native_session = None;
            fixture.runtimes[0].native_session = None;
            fixture.runtimes[0].provider_profile = Some(ProviderIntegrationProfile {
                agent_runtime_provider: None,
                model_route: None,
                provider: "codex".into(),
                execution_mode: EXECUTION_MODE_EXTERNAL_INTERACTIVE.into(),
                execution_driver: MemberExecutionDriver::UserDriven,
                provider_version: None,
                adapter_contract_version: None,
                reviewed_provider_versions: Vec::new(),
                compatibility_status: ProviderCompatibilityStatus::Current,
                adapter_reviewed_at: None,
                compatibility_note: None,
                interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
                ordinary_message_boundary: OrdinaryMessageBoundary::Unknown,
                plan_mode: ProviderFeatureMode::Unsupported,
                goal_mode: ProviderFeatureMode::Unsupported,
                tool_event_fidelity: ProviderEventFidelity::None,
                artifact_event_fidelity: ProviderEventFidelity::None,
                supports_cancel: false,
                supports_resume: false,
                observes_native_subagents: false,
                observes_background_tasks: false,
                thinking_transient_only: true,
                control_topology: ControlTopology::default(),
                composition_fingerprint: None,
                capability_fingerprint: None,
                capability_bindings: Vec::new(),
                binding_admission: ProviderBindingAdmission::Failed,
                adapter_bridge_revision: None,
                security_enforcement_locus: SecurityEnforcementLocus::default(),
            });
            fixture
        }
    }

    #[test]
    fn managed_binding_uses_one_exact_identity_and_independent_generations() {
        let fixture = Fixture::managed();
        let HostRuntimeBinding::Managed(binding) =
            resolve_host_runtime_binding(fixture.facts()).expect("exact managed binding")
        else {
            panic!("expected managed binding")
        };
        assert_eq!(binding.host_agent_member_id, "host-member");
        assert_eq!(binding.member_run.runtime_generation, 2);
        assert_eq!(binding.agent_session.runtime_generation, 1);
        assert_eq!(
            binding.agent_session.effective_permission_ceiling,
            PermissionCeiling::FullAccess
        );
    }

    #[test]
    fn duplicate_host_or_detached_session_fails_closed() {
        let mut duplicate = Fixture::managed();
        duplicate.memberships.push(duplicate.memberships[0].clone());
        let error = resolve_host_runtime_binding(duplicate.facts()).unwrap_err();
        assert_eq!(error.code, "HOST_RUNTIME_MEMBERSHIP_AMBIGUOUS");

        let mut detached = Fixture::managed();
        detached.sessions[0].control_state.runtime_residency = RuntimeResidency::Detached;
        let error = resolve_host_runtime_binding(detached.facts()).unwrap_err();
        assert_eq!(error.code, "HOST_RUNTIME_SESSION_AMBIGUOUS");

        let mut native_drift = Fixture::managed();
        native_drift.sessions[0]
            .native_session_ref
            .as_mut()
            .expect("native session")
            .native_session_id = "foreign-thread".into();
        let error = resolve_host_runtime_binding(native_drift.facts()).unwrap_err();
        assert_eq!(error.code, "HOST_RUNTIME_NATIVE_SESSION_FENCED");
    }

    #[test]
    fn stale_supervisor_or_daemon_fails_closed() {
        let mut fixture = Fixture::managed();
        fixture.supervisor.generation += 1;
        let error = resolve_host_runtime_binding(fixture.facts()).unwrap_err();
        assert_eq!(error.code, "HOST_RUNTIME_SESSION_AMBIGUOUS");

        let mut fixture = Fixture::managed();
        fixture.daemon.generation += 1;
        let error = resolve_host_runtime_binding(fixture.facts()).unwrap_err();
        assert_eq!(error.code, "HOST_RUNTIME_SUPERVISOR_FENCED");
    }

    #[test]
    fn external_host_is_exact_pull_only_and_cannot_own_a_session() {
        let fixture = Fixture::external();
        let HostRuntimeBinding::ExternalInteractive(binding) =
            resolve_host_runtime_binding(fixture.facts()).expect("exact external binding")
        else {
            panic!("expected external binding")
        };
        assert_eq!(binding.host_agent_member_id, "host-member");
        assert_eq!(binding.pull_inbox_member_run_id, "host-member-run");
        assert_eq!(
            binding.external_thread_id.as_deref(),
            Some("external-thread-1")
        );

        let mut invalid = Fixture::external();
        invalid.sessions = Fixture::managed().sessions;
        let error = resolve_host_runtime_binding(invalid.facts()).unwrap_err();
        assert_eq!(error.code, "EXTERNAL_HOST_SESSION_FORBIDDEN");

        let mut invalid = Fixture::external();
        invalid.members[0].native_session = Fixture::managed().members[0].native_session.clone();
        let error = resolve_host_runtime_binding(invalid.facts()).unwrap_err();
        assert_eq!(error.code, "EXTERNAL_HOST_NATIVE_SESSION_FORBIDDEN");
    }
}
