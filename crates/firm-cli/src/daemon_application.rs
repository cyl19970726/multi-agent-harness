//! CLI application composition used by the machine daemon.
//! Private registrations/provider adapters never cross the owned-handle boundary.
use crate::daemon_application_port::NativeSessionWakeSink;
use crate::daemon_application_port::*;
use crate::*;
use std::sync::{Arc, Mutex};

struct CliPreparedRun {
    inner: PreparedTeamRunStart,
}

impl PreparedRun for CliPreparedRun {
    fn drive(
        self: Box<Self>,
        space: harness_core::ExecutionSpace,
        max_concurrency: usize,
        input_acceptance_secs: u64,
        live_sink: NativeSessionWakeSink,
        serving_status: Arc<Mutex<String>>,
    ) -> CliResult<TeamRunDriveOutcome> {
        drive_prepared_team_run(
            self.inner,
            Some(space),
            None,
            max_concurrency,
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_secs(
                input_acceptance_secs,
            )),
            Some(live_sink),
            Some(serving_status),
        )
    }
}

/// Concrete application port, deliberately finite rather than a JSON dispatcher.
#[derive(Clone, Copy, Default)]
pub(crate) struct DaemonApplication;

impl DaemonApplicationPort for DaemonApplication {
    #[allow(clippy::too_many_arguments)]
    fn prepare_team_run(
        &self,
        store: &HarnessStore,
        run_id: &str,
        space: &harness_core::ExecutionSpace,
        node_id: &str,
        daemon_id: &str,
        instance_id: &str,
        max_concurrency: usize,
    ) -> CliResult<PreparedDaemonRun> {
        let body = prepare_team_run_start_body(store, run_id, max_concurrency)?;
        if body.run.execution_node_id != node_id {
            return Err(CliError::Usage(format!(
                "REMOTE_TEAM_RUN_NOT_ADOPTED: TeamRun {run_id} belongs to Node {}, local Node is {}",
                body.run.execution_node_id, node_id
            )));
        }
        let project_binding_id = body.run.project_binding_id.clone();
        let daemon_generation = store
            .latest_node_daemon_lease(node_id)?
            .filter(|lease| lease.daemon_id == daemon_id && lease.instance_id == instance_id)
            .ok_or_else(|| {
                CliError::Usage("NODE_DAEMON_GENERATION_FENCED: current lease is missing".into())
            })?
            .generation;
        ensure_team_runtime_fabric(store, &body, &space.id, daemon_id, daemon_generation)?;
        let registration = TeamSupervisorRegistration::start(store, run_id, Some(&space.id))?;
        let supervisor_id = registration.supervisor_id.clone();
        let supervisor_generation = registration.generation;
        bind_team_runtime_supervisor(
            store,
            &body,
            &space.id,
            daemon_id,
            &registration.supervisor_id,
            registration.generation,
        )?;
        let heartbeat_valid = Arc::clone(&registration.heartbeat_valid);

        // Transition Planning→Running only after the child Supervisor is
        // admitted under this exact daemon generation.
        use crate::now_string;
        use harness_core::TeamRunStatus;

        let running = if body.run.status == TeamRunStatus::Planning {
            let mut running = body.run.clone();
            running.status = TeamRunStatus::Running;
            running.updated_at = now_string();
            // Keep the typed Store error. Flattening a CAS conflict into
            // `CliError::Usage` hides it from the adoption-hold classifier,
            // which would then read an ordinary lost race as a structural
            // defect and wedge a healthy run until canonical state changed.
            store
                .compare_and_append_team_run_lifecycle(&body.run, &running)
                .map_err(CliError::Store)?;
            running
        } else {
            body.run.clone()
        };

        let ledger = Arc::new(TeamRunLedger::new(
            store,
            run_id,
            &registration.supervisor_id,
            registration.generation,
            Arc::clone(&registration.heartbeat_valid),
        ));

        ledger.fold_event(
            harness_core::TeamRunEventSourceKind::Host,
            None,
            "team_run",
            run_id,
            "updated",
            &format!(
                "member supervisor {} generation {} {} ({} unclosed member(s), max-concurrency {max_concurrency})",
                registration.supervisor_id,
                registration.generation,
                if body.run.status == TeamRunStatus::Planning {
                    "started"
                } else {
                    "reattached"
                },
                body.members.len(),
            ),
        )?;

        let prepared = PreparedTeamRunStart {
            run_id: body.run_id,
            objective: body.objective,
            running,
            members: body.members,
            ledger,
            supervisor_registration: registration,
        };

        Ok(PreparedDaemonRun {
            handle: Box::new(CliPreparedRun { inner: prepared }),
            daemon_generation,
            project_binding_id,
            supervisor_id,
            supervisor_generation,
            heartbeat_valid,
        })
    }

    fn open_node_session(
        &self,
        session: &harness_core::agentfirm_api::AgentSession,
        cwd: &Path,
        display_name: &str,
    ) -> Result<OpenedSession, String> {
        let opened = crate::provider_adapter::open_node_session(session, cwd, display_name)?;
        Ok(OpenedSession {
            runtime: Box::new(CliNodeSession(opened.runtime)),
            native_session_ref: opened.native_session_ref,
            permission_mapping: opened.permission_mapping,
        })
    }
    fn node_session_capabilities(
        &self,
        provider: &str,
    ) -> Option<crate::provider_adapter::NodeSessionCapabilities> {
        crate::provider_adapter::node_session_capabilities(provider)
    }
    fn map_permission(
        &self,
        provider: &str,
        permission: harness_core::agentfirm_api::PermissionCeiling,
    ) -> Result<crate::provider_adapter::ProviderPermissionMapping, String> {
        crate::provider_adapter::map_permission(provider, permission)
    }
    fn effective_delivery_mode(
        &self,
        provider: &str,
        requested: harness_core::agentfirm_api::RuntimeDispatchMode,
        lifecycle: harness_core::agentfirm_api::AgentSessionStatus,
        busy: bool,
    ) -> Result<harness_core::agentfirm_api::RuntimeDispatchMode, String> {
        crate::provider_adapter::effective_delivery_mode(provider, requested, lifecycle, busy)
    }
    #[allow(clippy::too_many_arguments)]
    fn read_native_session(
        &self,
        home: &Path,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: Option<&str>,
        request: &crate::daemon_protocol::PersistedSessionReadRequest,
    ) -> CliResult<crate::daemon_protocol::PersistedSessionReadResponse> {
        crate::provider_event_api::read_persisted_session_for_daemon(
            home,
            node_id,
            daemon_id,
            generation,
            instance_id,
            request,
        )
    }
    fn ensure_team_message_fabric(
        &self,
        store: &HarnessStore,
        run_id: &str,
        space_id: &str,
        daemon_id: &str,
        generation: u64,
    ) -> CliResult<()> {
        ensure_team_message_fabric(store, run_id, space_id, daemon_id, generation)
    }
    fn append_recovery_action(
        &self,
        store: &HarnessStore,
        run_id: &str,
        member_id: &str,
        action: &str,
        status: harness_core::MemberActionStatus,
        summary: &str,
        detail: &str,
    ) -> CliResult<harness_core::MemberAction> {
        TeamRunLedger::without_supervisor(store, run_id)
            .append_action(member_id, action, status, summary, detail)
    }
    #[allow(clippy::too_many_arguments)]
    fn append_recovery_action_with_evidence(
        &self,
        store: &HarnessStore,
        run_id: &str,
        member_id: &str,
        action: &str,
        status: harness_core::MemberActionStatus,
        summary: &str,
        detail: &str,
        provider_status: Option<String>,
        evidence: &[String],
    ) -> CliResult<harness_core::MemberAction> {
        TeamRunLedger::without_supervisor(store, run_id).append_action_with_provider_status(
            member_id,
            action,
            status,
            summary,
            detail,
            provider_status,
            evidence,
        )
    }
    fn list_execution_spaces(
        &self,
        home: &Path,
    ) -> Result<Vec<harness_core::ExecutionSpace>, String> {
        crate::execution_space::list_spaces(home).map_err(|error| error.to_string())
    }
    fn execution_space(
        &self,
        home: &Path,
        id: &str,
    ) -> Result<Option<harness_core::ExecutionSpace>, String> {
        crate::execution_space::context_for_id(home, id).map_err(|error| error.to_string())
    }
    fn canonical_run_state(
        &self,
        store: &HarnessStore,
        space_id: Option<&str>,
        run_id: &str,
    ) -> CliResult<String> {
        crate::team_run_canonical_state_fingerprint(store, space_id, run_id)
    }
    fn post_native_wake(
        &self,
        endpoint: &crate::daemon_protocol::NativeSessionWakeEndpoint,
        space_id: &str,
        update: &NativeSessionWakeUpdate,
    ) -> Result<(), crate::daemon_protocol::NativeSessionWakePostError> {
        crate::daemon_client::post_native_session_wake(endpoint, space_id, update)
    }
    fn daemon_log_path(&self, home: &Path, node_id: &str) -> std::path::PathBuf {
        crate::daemon_cli::node_daemon_log_path(home, node_id)
    }
}

struct CliNodeSession(crate::provider_adapter::NodeSessionRuntime);
impl NodeSessionHandle for CliNodeSession {
    fn provider(&self) -> &'static str {
        self.0.provider()
    }
    fn native_session_id(&self) -> &str {
        self.0.native_session_id()
    }
}

#[cfg(test)]
mod tests;
