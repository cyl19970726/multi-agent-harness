use firm_application::{
    resolve_host_member_binding, resolve_host_runtime_binding, HostMemberBinding,
    HostRuntimeBinding, HostRuntimeBindingFacts,
};

use crate::*;

impl HarnessStore {
    pub fn host_member_binding(&self, team_run_id: &str) -> StoreResult<HostMemberBinding> {
        let run = latest_by_id(self.team_run_rows(team_run_id)?, |run| run.id.clone())
            .remove(team_run_id)
            .ok_or_else(|| StoreError::Conflict(format!("TeamRun not found: {team_run_id}")))?;
        let execution_space_id = self.current_team_run_execution_space(&run)?;
        let team = self
            .agent_team(&execution_space_id, &run.agent_team_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_RUNTIME_TEAM_MISSING: AgentTeam {} not found",
                    run.agent_team_id
                ))
            })?;
        self.host_team_execution_space(&team, &run)?;
        let memberships = self.fabric_team_memberships_for_team(&execution_space_id, &team.id)?;
        let member_runs = self.trust_member_runs_for_team_run(&execution_space_id, team_run_id)?;
        let runtimes = latest_by_id(self.member_run_rows_for_team_run(team_run_id)?, |runtime| {
            runtime.id.clone()
        })
        .into_values()
        .collect::<Vec<_>>();
        resolve_host_member_binding(&HostRuntimeBindingFacts {
            team: &team,
            team_run: &run,
            memberships: &memberships,
            member_runs: &member_runs,
            runtimes: &runtimes,
            agent_sessions: &[],
            node_daemon: None,
            team_supervisor: None,
            observed_unix_ms: 0,
        })
        .map_err(|error| StoreError::Conflict(error.to_string()))
    }

    pub fn active_host_member_binding(&self, team_run_id: &str) -> StoreResult<HostMemberBinding> {
        let binding = self.host_member_binding(team_run_id)?;
        if !binding.is_active() {
            return Err(StoreError::Conflict(format!(
                "HOST_RUNTIME_MEMBER_RUN_INACTIVE: Host MemberRun {} is not active in both canonical and runtime projections",
                binding.member_run.id
            )));
        }
        Ok(binding)
    }

    pub fn host_runtime_binding(
        &self,
        team_run_id: &str,
        observed_unix_ms: u64,
    ) -> StoreResult<HostRuntimeBinding> {
        self.host_runtime_binding_unlocked(team_run_id, observed_unix_ms)
    }

    pub(super) fn host_runtime_binding_unlocked(
        &self,
        team_run_id: &str,
        observed_unix_ms: u64,
    ) -> StoreResult<HostRuntimeBinding> {
        let run = latest_by_id(self.team_run_rows(team_run_id)?, |run| run.id.clone())
            .remove(team_run_id)
            .ok_or_else(|| StoreError::Conflict(format!("TeamRun not found: {team_run_id}")))?;
        let execution_space_id = self.current_team_run_execution_space(&run)?;
        let team = self
            .agent_team(&execution_space_id, &run.agent_team_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_RUNTIME_TEAM_MISSING: AgentTeam {} not found",
                    run.agent_team_id
                ))
            })?;
        self.host_team_execution_space(&team, &run)?;
        let memberships = self.fabric_team_memberships_for_team(&execution_space_id, &team.id)?;
        let member_runs = self.trust_member_runs_for_team_run(&execution_space_id, team_run_id)?;
        let runtimes = latest_by_id(self.member_run_rows_for_team_run(team_run_id)?, |runtime| {
            runtime.id.clone()
        })
        .into_values()
        .collect::<Vec<_>>();
        let member_ids = member_runs
            .iter()
            .map(|run| run.agent_member_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let agent_sessions =
            self.fabric_agent_sessions_for_members(&execution_space_id, &member_ids)?;
        let team_supervisor = self.latest_team_supervisor_lease(&run.id)?;
        let node_daemon = self.latest_node_daemon_lease(&team.node_id)?;
        resolve_host_runtime_binding(HostRuntimeBindingFacts {
            team: &team,
            team_run: &run,
            memberships: &memberships,
            member_runs: &member_runs,
            runtimes: &runtimes,
            agent_sessions: &agent_sessions,
            node_daemon: node_daemon.as_ref(),
            team_supervisor: team_supervisor.as_ref(),
            observed_unix_ms,
        })
        .map_err(|error| StoreError::Conflict(error.to_string()))
    }

    /// Resolve the Host from the Team's canonical scope without materializing
    /// every MemberRun declared by the TeamRun. Host binding is an exact-Host
    /// read: an unrelated Member may legitimately be between the legacy
    /// runtime append and its canonical lifecycle projection while the Host is
    /// receiving a message. The binding resolver still compares the exact Host
    /// canonical MemberRun and ProviderRuntimeProjection and fails closed on
    /// any Host divergence.
    fn host_team_execution_space(
        &self,
        team: &firm_core::AgentTeam,
        run: &firm_core::AgentTeamRun,
    ) -> StoreResult<String> {
        if let Some(supervisor) = self.latest_team_supervisor_lease(&run.id)? {
            if supervisor.team_run_id != run.id
                || supervisor.node_id != team.node_id
                || supervisor.node_id != run.execution_node_id
                || supervisor.project_binding_id != run.project_binding_id
            {
                return Err(StoreError::Conflict(format!(
                    "HOST_RUNTIME_SUPERVISOR_SCOPE_FENCED: TeamSupervisor for {} does not bind its exact Team placement and Project Binding",
                    run.id
                )));
            }
            return Ok(supervisor.execution_space_id);
        }

        let host_agent_member_id = run
            .host_actor
            .as_ref()
            .filter(|actor| actor.kind == firm_core::TeamActorKind::Host)
            .map(|actor| actor.id.as_str())
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_RUNTIME_AUTHORITY_MISSING: TeamRun {} has no exact Host actor",
                    run.id
                ))
            })?;
        let mut matching_scopes = Vec::new();
        for scope in self.canonical_execution_space_ids()? {
            let has_team = self
                .agent_teams(&scope)?
                .into_iter()
                .any(|candidate| candidate.id == team.id && candidate.node_id == team.node_id);
            if !has_team {
                continue;
            }
            let has_host_membership =
                self.fabric_team_memberships(&scope)?
                    .into_iter()
                    .any(|membership| {
                        membership.team_id == team.id
                            && membership.agent_member_id == host_agent_member_id
                            && membership.role == firm_core::agentfirm_api::TeamMembershipRole::Host
                            && membership.state
                                == firm_core::agentfirm_api::TeamMembershipStatus::Active
                    });
            let has_host_member_run = self.trust_member_runs(&scope)?.into_iter().any(|member| {
                member.team_run_id == run.id
                    && member.agent_member_id == host_agent_member_id
                    && run.member_run_ids.iter().any(|id| id == &member.id)
            });
            if has_host_membership && has_host_member_run {
                matching_scopes.push(scope);
            }
        }
        match matching_scopes.as_slice() {
            [scope] => Ok(scope.clone()),
            scopes => Err(StoreError::Conflict(format!(
                "HOST_RUNTIME_TEAM_SCOPE_AMBIGUOUS: AgentTeam {} TeamRun {} resolves to {} canonical Host scopes",
                team.id,
                run.id,
                scopes.len()
            ))),
        }
    }
}
