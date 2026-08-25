use super::*;

fn current_store_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

impl HarnessStore {
    pub(super) fn require_exact_supervisor_authority_unlocked(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        // This helper is called only while the Store writer lock is held.
        // Sample time here, at the authority linearization point, so lock
        // contention cannot carry a pre-lock timestamp past lease expiry.
        let now_unix_ms = current_store_unix_ms();
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_SUPERVISOR_LEASE_LOST: TeamRun {team_run_id} has no Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_LEASE_LOST: TeamRun {team_run_id} is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let parent = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |parent| parent.node_id.clone(),
        )
        .remove(&lease.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: Node {} has no active parent",
                lease.node_id
            ))
        })?;
        if parent.status != NodeDaemonLeaseStatus::Active
            || parent.daemon_id != lease.node_daemon_id
            || parent.generation != lease.node_daemon_generation
            || parent.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun {team_run_id}"
            )));
        }
        Ok(lease)
    }

    pub fn append_team_message(&self, value: &TeamMessageProjection) -> StoreResult<()> {
        let _ = value;
        Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use NodeDaemon-authored canonical Message".into(),
        ))
    }

    /// Retired TeamMessageProjection write seam.
    ///
    /// Historical rows remain readable, but new runtime messages must use the
    /// identity-first canonical Message path owned by the NodeDaemon.
    pub fn append_team_message_checked(&self, value: &TeamMessageProjection) -> StoreResult<()> {
        let _ = value;
        Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use NodeDaemon-authored canonical Message".into(),
        ))
    }

    pub fn insert_execution_node(&self, value: &ExecutionNode) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let nodes = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        );
        if nodes.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "execution node already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("execution_nodes.jsonl", value)
    }

    pub fn transition_execution_node(
        &self,
        expected: &ExecutionNode,
        next: &ExecutionNode,
    ) -> StoreResult<()> {
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("execution node not found: {}", expected.id))
        })?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "execution node {} changed concurrently",
                expected.id
            )));
        }
        if next.id != current.id
            || next.display_name != current.display_name
            || next.created_at != current.created_at
            || !matches!(
                (current.status, next.status),
                (ExecutionNodeStatus::Active, ExecutionNodeStatus::Draining)
                    | (ExecutionNodeStatus::Draining, ExecutionNodeStatus::Retired)
            )
        {
            return Err(StoreError::Conflict(
                "NODE_TRANSITION_INVALID: allowed transitions are active->draining->retired"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("execution_nodes.jsonl", next)
    }

    pub fn register_node_project(
        &self,
        value: &NodeProjectRegistration,
        execution_space_id: &str,
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        require_non_empty_store(execution_space_id, "Execution Space id")?;
        if value.execution_space_id != execution_space_id {
            return Err(StoreError::Conflict(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: registration names {}, selected Store is {execution_space_id}",
                value.execution_space_id
            )));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&value.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("NODE_NOT_ACTIVE: {} not found", value.node_id))
        })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {} is {:?}",
                node.id, node.status
            )));
        }
        let key = node_project_registration_identity(value);
        let registrations = latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        );
        if let Some(current) = registrations.get(&key) {
            if current == value {
                return Ok(());
            }
            if current.created_at != value.created_at {
                return Err(StoreError::Conflict(format!(
                    "node project registration identity already exists: {key}"
                )));
            }
        }
        self.append_jsonl_unlocked("node_project_registrations.jsonl", value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn acquire_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        instance_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_NOT_ACTIVE: {node_id} not found")))?;
        if node.status == ExecutionNodeStatus::Retired {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {node_id} is retired"
            )));
        }
        let current = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id);
        if let Some(current) = current.as_ref() {
            if matches!(
                current.status,
                NodeDaemonLeaseStatus::Active | NodeDaemonLeaseStatus::Draining
            ) && current.expires_unix_ms > now_unix_ms
            {
                if current.daemon_id == daemon_id && current.instance_id == instance_id {
                    return Ok(current.clone());
                }
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_LEASE_HELD: Node {node_id} is held by {} generation {}",
                    current.daemon_id, current.generation
                )));
            }
        }
        let generation = current
            .as_ref()
            .map(|lease| lease.generation.saturating_add(1))
            .unwrap_or(1);
        let lease = NodeDaemonLease {
            node_id: node_id.to_string(),
            daemon_id: daemon_id.to_string(),
            generation,
            instance_id: instance_id.to_string(),
            status: NodeDaemonLeaseStatus::Active,
            acquired_unix_ms: now_unix_ms,
            renewed_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            released_unix_ms: None,
        };
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn renew_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.status != NodeDaemonLeaseStatus::Active
            || lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: {daemon_id} generation {generation} no longer owns Node {node_id}"
            )));
        }
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Fence every new provider effect while a NodeDaemon drains its owned
    /// supervisors. A successor cannot acquire the Node until this bounded
    /// drain lease expires or the current daemon publishes `Released` after
    /// all provider handles have been reaped.
    pub fn drain_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
        drain_ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: stale daemon cannot drain Node {node_id}"
            )));
        }
        if lease.status == NodeDaemonLeaseStatus::Draining {
            return Ok(lease);
        }
        if lease.status != NodeDaemonLeaseStatus::Active || lease.expires_unix_ms <= now_unix_ms {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: Node {node_id} is no longer actively owned"
            )));
        }
        lease.status = NodeDaemonLeaseStatus::Draining;
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms.saturating_add(drain_ttl_ms.max(1));
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn release_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: stale daemon cannot release Node {node_id}"
            )));
        }
        if lease.status == NodeDaemonLeaseStatus::Released {
            return Ok(lease);
        }
        lease.status = NodeDaemonLeaseStatus::Released;
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Acquire the one durable Supervisor lease for a TeamRun. An active,
    /// unexpired lease held by another Supervisor rejects the attach before any
    /// provider side effect. Reacquisition after expiry increments generation.
    #[allow(clippy::too_many_arguments)]
    pub fn acquire_team_supervisor_under_node_lease(
        &self,
        team_run_id: &str,
        node_id: &str,
        node_daemon_id: &str,
        node_daemon_generation: u64,
        execution_space_id: &str,
        project_binding_id: &str,
        supervisor_id: &str,
        owner_process_id: u32,
        owner_locator: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(team_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {team_run_id}")))?;
        if run.execution_node_id != node_id || run.project_binding_id != project_binding_id {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_NODE_MISMATCH: TeamRun {team_run_id} is bound to Node {} / project {}, not {node_id} / {project_binding_id}",
                run.execution_node_id, run.project_binding_id
            )));
        }
        let resolved_execution_space_id = self.current_team_run_execution_space_unlocked(&run)?;
        if resolved_execution_space_id != execution_space_id {
            return Err(StoreError::Conflict(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {team_run_id} belongs to Execution Space {resolved_execution_space_id}, not {execution_space_id}"
            )));
        }
        let registrations = latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        )
        .into_values()
        .filter(|registration| {
            registration.node_id == node_id
                && registration.project_binding_id == project_binding_id
                && registration.execution_space_id == execution_space_id
                && registration.status == NodeProjectRegistrationStatus::Active
        })
        .count();
        if registrations != 1 {
            return Err(StoreError::Conflict(format!(
                "PROJECT_NOT_REGISTERED_ON_NODE: expected one active registration for TeamRun {team_run_id} in Execution Space {execution_space_id}, found {registrations}"
            )));
        }
        let parent = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: Node {node_id} has no NodeDaemon lease"
            ))
        })?;
        if parent.status != NodeDaemonLeaseStatus::Active
            || parent.daemon_id != node_daemon_id
            || parent.generation != node_daemon_generation
            || parent.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: NodeDaemon {node_daemon_id} generation {node_daemon_generation} is not the active parent for Node {node_id}"
            )));
        }
        let current = self.latest_lease_for_run_unlocked(team_run_id)?;
        if let Some(current) = current.as_ref() {
            if current.status == TeamSupervisorLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
                && current.supervisor_id != supervisor_id
            {
                return Err(StoreError::Conflict(format!(
                    "team run {team_run_id} is supervised by {} generation {} until unix-ms:{}",
                    current.supervisor_id, current.generation, current.expires_unix_ms
                )));
            }
            if current.status == TeamSupervisorLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
                && current.supervisor_id == supervisor_id
            {
                return Ok(current.clone());
            }
        }
        let generation = current
            .as_ref()
            .map(|lease| lease.generation.saturating_add(1))
            .unwrap_or(1);
        let lease = TeamSupervisorLease {
            team_run_id: team_run_id.to_string(),
            node_id: node_id.to_string(),
            node_daemon_id: node_daemon_id.to_string(),
            node_daemon_generation,
            execution_space_id: execution_space_id.to_string(),
            project_binding_id: project_binding_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            generation,
            owner_process_id,
            owner_locator: owner_locator.to_string(),
            status: TeamSupervisorLeaseStatus::Active,
            acquired_unix_ms: now_unix_ms,
            heartbeat_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            released_unix_ms: None,
        };
        // Acquisition is rare (one per Supervisor generation) while heartbeats
        // are ~1/s, so this is where compaction belongs.
        self.compact_supervisor_leases_unlocked()?;
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn renew_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        generation: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no Supervisor lease to renew"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "Supervisor lease for team run {team_run_id} is no longer owned by {supervisor_id} generation {generation}"
            )));
        }
        let parent = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |parent| parent.node_id.clone(),
        )
        .remove(&lease.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: Node {} has no active parent",
                lease.node_id
            ))
        })?;
        if parent.status != NodeDaemonLeaseStatus::Active
            || parent.daemon_id != lease.node_daemon_id
            || parent.generation != lease.node_daemon_generation
            || parent.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun {team_run_id}"
            )));
        }
        lease.heartbeat_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn release_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        generation: u64,
        now_unix_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no Supervisor lease to release"
            ))
        })?;
        if lease.supervisor_id != supervisor_id || lease.generation != generation {
            return Err(StoreError::Conflict(format!(
                "Supervisor lease for team run {team_run_id} belongs to {} generation {}, not {supervisor_id} generation {generation}",
                lease.supervisor_id, lease.generation
            )));
        }
        if lease.status == TeamSupervisorLeaseStatus::Released {
            return Ok(lease);
        }
        lease.status = TeamSupervisorLeaseStatus::Released;
        lease.heartbeat_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Persist a Host Close before touching the process-local provider handle.
    /// Repeated requests while one is pending are idempotent.
    pub fn latch_team_member_close(
        &self,
        value: &TeamMemberCloseRequest,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                value.member_run_id
            ))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        self.require_current_member_mutation_scope_unlocked(&member)?;
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Persist a Host Close only while the named Supervisor generation and
    /// its parent NodeDaemon generation still hold current durable authority.
    ///
    /// The child/parent lease checks and the Close append share the Store
    /// writer lock with lease renewal, release, and successor acquisition.
    /// This is the live-control admission linearization point: a stale
    /// generation can never pass an optimistic lease read and append a Close
    /// after another generation has taken over.
    pub fn latch_team_member_close_for_supervisor(
        &self,
        value: &TeamMemberCloseRequest,
        supervisor_id: &str,
        supervisor_generation: u64,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_exact_supervisor_authority_unlocked(
            &value.team_run_id,
            supervisor_id,
            supervisor_generation,
        )?;
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                value.member_run_id
            ))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        self.require_current_member_mutation_scope_unlocked(&member)?;
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Persist a Host Close only when no current Supervisor generation owns
    /// the TeamRun. The absence check and Close latch share the Store write
    /// lock with Supervisor acquisition, closing the race where a successor
    /// generation could acquire authority after a caller observed no lease
    /// but before the durable Close became visible.
    ///
    /// A successor that acquires after this method returns will observe the
    /// pending Close at the pre-provider-spawn fence and must not start the
    /// member. A generation that acquires first makes this method fail closed
    /// so the caller can route control through that exact live owner.
    pub fn latch_team_member_close_without_current_supervisor(
        &self,
        value: &TeamMemberCloseRequest,
        now_unix_ms: u64,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(lease) = self.latest_lease_for_run_unlocked(&value.team_run_id)? {
            if lease.status == TeamSupervisorLeaseStatus::Active
                && lease.expires_unix_ms > now_unix_ms
            {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SUPERVISOR_LEASE_CURRENT: TeamRun {} is owned by {} generation {} until {}",
                    value.team_run_id,
                    lease.supervisor_id,
                    lease.generation,
                    lease.expires_unix_ms
                )));
            }
        }
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                value.member_run_id
            ))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        self.require_current_member_mutation_scope_unlocked(&member)?;
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Mark one durable Close as applied after the ProviderRuntimeProjection is stopped.
    pub fn complete_team_member_close(
        &self,
        team_run_id: &str,
        member_run_id: &str,
        request_id: &str,
        applied_at: &str,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut request = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} has no durable Close request"
            ))
        })?;
        if request.team_run_id != team_run_id || request.id != request_id {
            return Err(StoreError::Conflict(format!(
                "Close request {request_id} does not own ProviderRuntimeProjection {member_run_id} in TeamRun {team_run_id}"
            )));
        }
        let run = self.require_team_run_unlocked(team_run_id)?;
        self.current_team_run_execution_space_unlocked(&run)?;
        if request.status == TeamMemberCloseStatus::Applied {
            return Ok(request);
        }
        request.status = TeamMemberCloseStatus::Applied;
        request.applied_at = Some(applied_at.to_string());
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", &request)?;
        Ok(request)
    }

    /// Claim one queued TeamMessageProjection delivery under the same durable lock used
    /// for the Supervisor lease. A claim must be completed with a real provider
    /// receipt or explicitly reconciled; it is never auto-requeued on expiry.
    #[cfg(any())]
    #[allow(clippy::too_many_arguments, unreachable_code, unused_variables)]
    pub fn claim_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        claim_ttl_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessageDeliveryClaimResult> {
        return Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use identity-first canonical Delivery".into(),
        ));
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut message = match latest_by_id(
            self.read_jsonl::<TeamMessageProjection>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        {
            Some(message) if message.team_run_id == team_run_id => message,
            _ => return Ok(TeamMessageDeliveryClaimResult::NotQueued),
        };
        if message.kind == ProviderDispatchIntent::ProviderInteractionResponse {
            let body = ProviderInteractionResponseBody::parse_canonical_json(&message.body)
                .map_err(StoreError::Conflict)?;
            let member = self.require_member_run_unlocked(&body.member, team_run_id)?;
            let same_live_generation = member.coordination_is_active()
                && member.runtime_generation == body.generation
                && member
                    .native_session
                    .as_ref()
                    .is_some_and(|native| native.native_session_id == body.session);
            if !same_live_generation {
                return Err(StoreError::Conflict(format!(
                    "provider interaction response is stale for ProviderRuntimeProjection {} generation/session",
                    body.member
                )));
            }
        }
        let Some(delivery) = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
        else {
            return Ok(TeamMessageDeliveryClaimResult::NotQueued);
        };
        if delivery.status != TeamDeliveryStatus::Queued {
            return Ok(TeamMessageDeliveryClaimResult::NotQueued);
        }
        delivery.status = TeamDeliveryStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.claimed_unix_ms = Some(now_unix_ms);
        delivery.claim_expires_unix_ms = Some(now_unix_ms.saturating_add(claim_ttl_ms.max(1)));
        delivery.provider_receipt_id = None;
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(TeamMessageDeliveryClaimResult::Claimed(Box::new(message)))
    }

    #[cfg(any())]
    #[allow(clippy::too_many_arguments, unreachable_code, unused_variables)]
    pub fn complete_team_message_delivery_claim(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessageProjection> {
        return Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use NodeDaemon provider receipt on canonical Delivery".into(),
        ));
        if provider_receipt_id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "provider receipt id is required to complete a TeamMessageProjection delivery"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut message = latest_by_id(
            self.read_jsonl::<TeamMessageProjection>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        if delivery.status == TeamDeliveryStatus::Delivered
            && delivery.claim_id.as_deref() == Some(claim_id)
        {
            if delivery.provider_receipt_id.as_deref() == Some(provider_receipt_id) {
                return Ok(message);
            }
            return Err(StoreError::Conflict(format!(
                "delivery claim {claim_id} for message {message_id} was already completed with a different provider receipt"
            )));
        }
        if delivery.status != TeamDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_by_supervisor_id.as_deref() != Some(supervisor_id)
            || delivery.claimed_generation != Some(supervisor_generation)
        {
            return Err(StoreError::Conflict(format!(
                "delivery claim {claim_id} no longer owns message {message_id} for {member_run_id}"
            )));
        }
        delivery.status = TeamDeliveryStatus::Delivered;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    /// Retired compatibility seam for the historical `team_messages.jsonl`
    /// delivery projection.
    ///
    /// Current callers must acknowledge the identity-first canonical Delivery
    /// through [`HarnessStore::acknowledge_message_delivery`]. Historical
    /// `TeamMessageProjection` rows remain readable, but this method must never
    /// append a compatibility mutation or bypass canonical actor authority.
    #[allow(unused_variables)]
    pub fn acknowledge_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        updated_at: &str,
    ) -> StoreResult<TeamMessageProjection> {
        Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use identity-first canonical Delivery acknowledgement".into(),
        ))
    }

    /// Retired compatibility seam for reconciling the historical
    /// `team_messages.jsonl` delivery projection.
    ///
    /// Current operator recovery must route through
    /// [`HarnessStore::reconcile_canonical_message_delivery`], which fences the
    /// exact NodeDaemon and canonical delivery generation before mutation.
    /// Historical rows are read-only and are never dual-written.
    #[allow(clippy::too_many_arguments, unused_variables)]
    pub fn reconcile_team_message_delivery_claim(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        claim_id: &str,
        provider_accepted: bool,
        provider_receipt_id: Option<&str>,
        updated_at: &str,
    ) -> StoreResult<TeamMessageProjection> {
        Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use canonical Delivery reconciliation under exact NodeDaemon authority"
                .into(),
        ))
    }

    /// Fail a TeamMessageProjection delivery that can never be completed because the
    /// target member has stopped / failed / been retired.
    ///
    /// Transitions from `Queued` (pre-bind failure) or `Claimed` (transport
    /// disconnect) to `Failed`. A delivery already at `Failed` with the same
    /// reason is idempotent.
    #[cfg(any())]
    #[allow(clippy::too_many_arguments, unreachable_code, unused_variables)]
    pub fn fail_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        reason: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessageProjection> {
        return Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use canonical Delivery reconciliation".into(),
        ));
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "TeamMessageProjection delivery failure reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no active Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }

        let mut message = latest_by_id(
            self.read_jsonl::<TeamMessageProjection>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;

        // Idempotent: already failed with same reason.
        if delivery.status == TeamDeliveryStatus::Failed {
            if delivery
                .failure_reason
                .as_deref()
                .is_some_and(|existing| existing == reason)
            {
                return Ok(message);
            }
            return Err(StoreError::Conflict(format!(
                "message {message_id} delivery for {member_run_id} was already failed with a different reason"
            )));
        }

        // Allowed transitions: Queued→Failed (pre-bind), Claimed→Failed
        // (post-bind / transport disconnect).
        match delivery.status {
            TeamDeliveryStatus::Queued => {}
            TeamDeliveryStatus::Claimed => {
                // Only the owning Supervisor generation may fail its own claim.
                if delivery.claimed_by_supervisor_id.as_deref() != Some(supervisor_id)
                    || delivery.claimed_generation != Some(supervisor_generation)
                {
                    return Err(StoreError::Conflict(format!(
                        "message {message_id} delivery for {member_run_id} was claimed by a different Supervisor generation"
                    )));
                }
            }
            _ => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} delivery for {member_run_id} is already {:?}",
                    delivery.status
                )));
            }
        }

        delivery.status = TeamDeliveryStatus::Failed;
        delivery.claim_id = None;
        delivery.claimed_by_supervisor_id = None;
        delivery.claimed_generation = None;
        delivery.claimed_unix_ms = None;
        delivery.claim_expires_unix_ms = None;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = Some(reason.to_string());
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }
}
