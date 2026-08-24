use super::*;

const SUPERVISOR_RECOVERY_REQUIRED: &str = "team_supervisor_recovery_required";
const SUPERVISOR_RECOVERED: &str = "team_supervisor_recovered";

fn latest_supervisor_recovery_action(
    store: &HarnessStore,
    run_id: &str,
) -> CliResult<Option<harness_core::MemberAction>> {
    Ok(store
        .member_actions()?
        .into_iter()
        .filter(|action| {
            action.team_run_id == run_id
                && matches!(
                    action.action_type.as_str(),
                    SUPERVISOR_RECOVERY_REQUIRED | SUPERVISOR_RECOVERED
                )
        })
        .max_by_key(|action| action.seq))
}

fn recovery_marker_is_blocking(action: Option<&harness_core::MemberAction>) -> bool {
    action.is_some_and(|action| {
        action.action_type == SUPERVISOR_RECOVERY_REQUIRED
            && action.status == harness_core::MemberActionStatus::Failed
    })
}

pub(super) fn team_run_has_unresolved_runtime_command(
    store: &HarnessStore,
    execution_space_id: &str,
    run_id: &str,
) -> CliResult<bool> {
    let members = crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == run_id)
        .map(|member| (member.id, member.runtime_generation))
        .collect::<HashMap<_, _>>();
    Ok(store
        .runtime_commands(execution_space_id)?
        .into_iter()
        .any(|command| {
            command
                .binding
                .target_member_run_id
                .as_ref()
                .and_then(|member_id| {
                    members
                        .get(member_id)
                        .map(|generation| (member_id, generation))
                })
                .is_some_and(|(_, generation)| {
                    command.binding.target_member_run_generation == Some(*generation)
                        && command.status
                            == harness_core::agentfirm_api::RuntimeCommandStatus::Accepted
                        && command.effect_certainty
                            == harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown
                        && command.postcondition_status
                            == harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown
                })
        }))
}

pub(crate) fn reconcile_team_run_start_postcondition(
    store: &HarnessStore,
    status_json: &str,
    node_id: &str,
    execution_space_id: &str,
    run_id: &str,
) -> Option<CliResult<serde_json::Value>> {
    let status = serde_json::from_str::<serde_json::Value>(status_json).ok()?;
    if status["ok"].as_bool() != Some(true) || status["node_id"].as_str() != Some(node_id) {
        return None;
    }
    let instance_id = status["instance_id"].as_str()?;
    let process_id = status["process_id"].as_u64()?;
    let process_id = u32::try_from(process_id).ok()?;
    let matching_runs = status["runs"]
        .as_array()?
        .iter()
        .filter(|candidate| {
            candidate["execution_space_id"].as_str() == Some(execution_space_id)
                && candidate["run_id"].as_str() == Some(run_id)
                && candidate["status"].as_str() == Some("running")
        })
        .collect::<Vec<_>>();
    let [run] = matching_runs.as_slice() else {
        return None;
    };
    let daemon_generation = run["daemon_generation"].as_u64()?;
    let project_binding_id = run["project_binding_id"].as_str()?;
    let supervisor_id = run["supervisor_id"].as_str()?;
    let supervisor_generation = run["supervisor_generation"].as_u64()?;
    let now = current_unix_ms_u64();
    let daemon = match store.latest_node_daemon_lease(node_id) {
        Ok(Some(daemon)) => daemon,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    let supervisor = match store.latest_team_supervisor_lease(run_id) {
        Ok(Some(supervisor)) => supervisor,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    let exact = start_postcondition_matches(
        &daemon,
        &supervisor,
        node_id,
        instance_id,
        daemon_generation,
        execution_space_id,
        project_binding_id,
        run_id,
        supervisor_id,
        supervisor_generation,
        process_id,
        now,
    );
    exact.then(|| {
        Ok(serde_json::json!({
            "node_id": node_id,
            "execution_space_id": execution_space_id,
            "team_run_id": run_id,
            "daemon_response": {
                "ok": true,
                "reconciled_after_transport_error": true,
                "daemon_generation": daemon_generation,
                "supervisor_id": supervisor_id,
                "supervisor_generation": supervisor_generation,
            },
        }))
    })
}

#[allow(clippy::too_many_arguments)]
fn start_postcondition_matches(
    daemon: &harness_core::NodeDaemonLease,
    supervisor: &harness_core::TeamSupervisorLease,
    node_id: &str,
    instance_id: &str,
    daemon_generation: u64,
    execution_space_id: &str,
    project_binding_id: &str,
    run_id: &str,
    supervisor_id: &str,
    supervisor_generation: u64,
    process_id: u32,
    now: u64,
) -> bool {
    daemon.node_id == node_id
        && daemon.daemon_id == format!("node-daemon:{node_id}")
        && daemon.instance_id == instance_id
        && daemon.generation == daemon_generation
        && daemon.status == harness_core::NodeDaemonLeaseStatus::Active
        && daemon.expires_unix_ms > now
        && supervisor.team_run_id == run_id
        && supervisor.node_id == node_id
        && supervisor.node_daemon_id == daemon.daemon_id
        && supervisor.node_daemon_generation == daemon.generation
        && supervisor.execution_space_id == execution_space_id
        && supervisor.project_binding_id == project_binding_id
        && supervisor.supervisor_id == supervisor_id
        && supervisor.generation == supervisor_generation
        && supervisor.owner_process_id == process_id
        && supervisor.status == harness_core::TeamSupervisorLeaseStatus::Active
        && supervisor.expires_unix_ms > now
}

impl MultiTeamDaemon {
    pub(super) fn block_start_failure_if_unresolved(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        error: &CliError,
    ) {
        match team_run_has_unresolved_runtime_command(store, execution_space_id, run_id) {
            Ok(false) => {}
            Ok(true) => {
                let lease = store.latest_team_supervisor_lease(run_id).ok().flatten();
                if let Err(marker_error) = self.block_team_run_after_supervisor_failure(
                    execution_space_id,
                    store,
                    run_id,
                    lease.as_ref().map(|lease| lease.supervisor_id.as_str()),
                    lease.as_ref().map(|lease| lease.generation),
                    error,
                ) {
                    eprintln!(
                        "[node-daemon] could not persist start-failure recovery marker for {execution_space_id}/{run_id}: {marker_error}"
                    );
                }
            }
            Err(read_error) => eprintln!(
                "[node-daemon] could not inspect unresolved RuntimeCommands after start failure for {execution_space_id}/{run_id}: {read_error}"
            ),
        }
    }

    pub(super) fn block_finished_supervisor_if_unresolved(&self, context: &MultiTeamContext) {
        let store = match self.store_for_space(&context.execution_space_id) {
            Ok(store) => store,
            Err(error) => {
                self.block_finished_supervisor_volatile(context, &error);
                return;
            }
        };
        match team_run_has_unresolved_runtime_command(
            &store,
            &context.execution_space_id,
            &context.run_id,
        ) {
            Ok(false) => {}
            Ok(true) => self.block_finished_supervisor(
                context,
                &store,
                &CliError::Usage("TEAM_SUPERVISOR_EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND".into()),
            ),
            Err(error) => self.block_finished_supervisor_volatile(context, &error),
        }
    }

    pub(super) fn block_finished_supervisor_failure(
        &self,
        context: &MultiTeamContext,
        error: &CliError,
    ) {
        match self.store_for_space(&context.execution_space_id) {
            Ok(store) => self.block_finished_supervisor(context, &store, error),
            Err(store_error) => self.block_finished_supervisor_volatile(context, &store_error),
        }
    }

    fn block_finished_supervisor(
        &self,
        context: &MultiTeamContext,
        store: &HarnessStore,
        error: &CliError,
    ) {
        if let Err(marker_error) = self.block_team_run_after_supervisor_failure(
            &context.execution_space_id,
            store,
            &context.run_id,
            Some(&context.supervisor_id),
            Some(context.supervisor_generation),
            error,
        ) {
            eprintln!(
                "[node-daemon] could not persist finished-Supervisor recovery marker for {}/{}: {marker_error}",
                context.execution_space_id, context.run_id
            );
        }
    }

    fn block_finished_supervisor_volatile(&self, context: &MultiTeamContext, error: &CliError) {
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner())
            .insert((context.execution_space_id.clone(), context.run_id.clone()));
        eprintln!(
            "[node-daemon] volatile Supervisor recovery block for {}/{} because its durable projection is unavailable: {error}",
            context.execution_space_id, context.run_id
        );
    }

    pub(super) fn store_for_space(&self, execution_space_id: &str) -> CliResult<HarnessStore> {
        let space = crate::execution_space::context_for_id(&self.firm_home, execution_space_id)
            .map_err(|error| CliError::Usage(error.to_string()))?
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "Execution Space not found while reconciling Supervisor: {execution_space_id}"
                ))
            })?;
        Ok(HarnessStore::new(space.store_root))
    }

    pub(super) fn team_run_recovery_is_blocking(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
    ) -> CliResult<bool> {
        if self
            .recovery_blocked_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(&(execution_space_id.to_string(), run_id.to_string()))
        {
            return Ok(true);
        }
        Ok(recovery_marker_is_blocking(
            latest_supervisor_recovery_action(store, run_id)?.as_ref(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn block_team_run_after_supervisor_failure(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        supervisor_id: Option<&str>,
        supervisor_generation: Option<u64>,
        error: &CliError,
    ) -> CliResult<()> {
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner())
            .insert((execution_space_id.to_string(), run_id.to_string()));
        if recovery_marker_is_blocking(latest_supervisor_recovery_action(store, run_id)?.as_ref()) {
            return Ok(());
        }
        let host = crate::latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.team_run_id == run_id && member.role == "host")
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "TEAM_RUN_HOST_NOT_FOUND: cannot project Supervisor recovery for {run_id}"
                ))
            })?;
        let failure_code = if error.is_supervisor_lease_lost() {
            "TEAM_SUPERVISOR_LEASE_LOST"
        } else if error.to_string().contains("PANICKED") {
            "TEAM_SUPERVISOR_PANICKED"
        } else if error
            .to_string()
            .contains("EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND")
        {
            "TEAM_SUPERVISOR_EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND"
        } else {
            "TEAM_SUPERVISOR_START_OR_RUNTIME_FAILED"
        };
        let authority = match (supervisor_id, supervisor_generation) {
            (Some(id), Some(generation)) => format!("{id} generation {generation}"),
            _ => "latest attempted Supervisor generation".to_string(),
        };
        TeamRunLedger::without_supervisor(store, run_id).append_action(
            &host.id,
            SUPERVISOR_RECOVERY_REQUIRED,
            harness_core::MemberActionStatus::Failed,
            "TeamSupervisor recovery required",
            &format!(
                "{failure_code}: {authority} stopped before a safe runnable postcondition; automatic adoption is blocked. Inspect the canonical RuntimeCommand inventory, then issue explicit operator recovery or a new start intent."
            ),
        )?;
        Ok(())
    }

    pub(super) fn clear_team_run_supervisor_recovery(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
    ) -> CliResult<()> {
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&(execution_space_id.to_string(), run_id.to_string()));
        if !recovery_marker_is_blocking(latest_supervisor_recovery_action(store, run_id)?.as_ref())
        {
            return Ok(());
        }
        let host = crate::latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.team_run_id == run_id && member.role == "host")
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "TEAM_RUN_HOST_NOT_FOUND: cannot settle Supervisor recovery for {run_id}"
                ))
            })?;
        TeamRunLedger::without_supervisor(store, run_id).append_action(
            &host.id,
            SUPERVISOR_RECOVERED,
            harness_core::MemberActionStatus::Succeeded,
            "TeamSupervisor explicitly recovered",
            &format!(
                "Explicit start established {supervisor_id} generation {supervisor_generation}; automatic adoption may resume after this generation ends safely."
            ),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_latest_failed_supervisor_recovery_marker_blocks_adoption() {
        let action = |seq: u64, action_type: &str, status: harness_core::MemberActionStatus| {
            harness_core::MemberAction {
                id: format!("action-{seq}"),
                seq,
                team_run_id: "run".into(),
                member_run_id: "host".into(),
                task_id: None,
                provider_call_id: None,
                action_type: action_type.into(),
                status,
                provider_status: None,
                semantic_status: None,
                title: "Supervisor".into(),
                summary: "bounded coordination diagnostic".into(),
                evidence_refs: Vec::new(),
                started_at: "unix-ms:1".into(),
                completed_at: Some("unix-ms:1".into()),
            }
        };
        let blocked = action(
            1,
            SUPERVISOR_RECOVERY_REQUIRED,
            harness_core::MemberActionStatus::Failed,
        );
        let recovered = action(
            2,
            SUPERVISOR_RECOVERED,
            harness_core::MemberActionStatus::Succeeded,
        );
        assert!(recovery_marker_is_blocking(Some(&blocked)));
        assert!(!recovery_marker_is_blocking(Some(&recovered)));
        assert!(!recovery_marker_is_blocking(None));
    }

    #[test]
    fn start_reconciliation_requires_one_exact_live_daemon_and_supervisor_generation() {
        let daemon = harness_core::NodeDaemonLease {
            node_id: "node".into(),
            daemon_id: "node-daemon:node".into(),
            generation: 7,
            instance_id: "instance".into(),
            status: harness_core::NodeDaemonLeaseStatus::Active,
            acquired_unix_ms: 10,
            renewed_unix_ms: 20,
            expires_unix_ms: 100,
            released_unix_ms: None,
        };
        let supervisor = harness_core::TeamSupervisorLease {
            team_run_id: "run".into(),
            node_id: "node".into(),
            node_daemon_id: daemon.daemon_id.clone(),
            node_daemon_generation: daemon.generation,
            execution_space_id: "space".into(),
            project_binding_id: "project".into(),
            supervisor_id: "supervisor".into(),
            generation: 3,
            owner_process_id: 42,
            owner_locator: "test://supervisor".into(),
            status: harness_core::TeamSupervisorLeaseStatus::Active,
            acquired_unix_ms: 10,
            heartbeat_unix_ms: 20,
            expires_unix_ms: 100,
            released_unix_ms: None,
        };
        assert!(start_postcondition_matches(
            &daemon,
            &supervisor,
            "node",
            "instance",
            7,
            "space",
            "project",
            "run",
            "supervisor",
            3,
            42,
            50,
        ));
        assert!(!start_postcondition_matches(
            &daemon,
            &supervisor,
            "node",
            "instance",
            8,
            "space",
            "project",
            "run",
            "supervisor",
            3,
            42,
            50,
        ));
        assert!(!start_postcondition_matches(
            &daemon,
            &supervisor,
            "node",
            "instance",
            7,
            "space",
            "project",
            "run",
            "supervisor",
            3,
            43,
            50,
        ));
    }
}
