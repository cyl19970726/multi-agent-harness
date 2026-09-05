use super::*;

pub(super) enum RuntimeBindingAdmission {
    Invocation,
    RuntimeCommand {
        allow_native_session_attachment: bool,
        settlement_only: bool,
    },
}

pub(super) enum RuntimeCommandPoststate {
    None,
    Command,
    CommandWithNativeSessionAttachment,
}

impl HarnessStore {
    pub(super) fn latest_fabric_side_records_unlocked<T, F>(
        &self,
        execution_space_id: &str,
        mut id: F,
    ) -> StoreResult<BTreeMap<String, T>>
    where
        T: for<'de> Deserialize<'de>,
        F: FnMut(&T) -> String,
    {
        let mut rows = BTreeMap::new();
        for row in self.trust_side_records::<T>(execution_space_id)? {
            rows.insert(id(&row), row);
        }
        Ok(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn require_current_node_daemon_unlocked(
        &self,
        execution_space_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        actor: &ActorRef,
        resource_kind: &str,
        resource_id: &str,
    ) -> StoreResult<()> {
        if actor.kind != ActorKind::Service || actor.id != daemon_id {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "runtime mutation requires the exact authenticated NodeDaemon service",
                resource_kind,
                resource_id,
                None,
            ));
        }
        let lease = self.latest_node_daemon_lease(node_id)?.ok_or_else(|| {
            trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "NodeDaemon lease is missing",
                resource_kind,
                resource_id,
                None,
            )
        })?;
        let registered = self
            .latest_node_project_registrations()?
            .iter()
            .any(|registration| {
                registration.node_id == node_id
                    && registration.execution_space_id == execution_space_id
                    && registration.status == firm_core::NodeProjectRegistrationStatus::Active
            });
        if crate::process_node_daemon_admission_is_closed(&lease.daemon_id, &lease.instance_id) {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "NodeDaemon process authority is permanently closed for new provider effects",
                resource_kind,
                resource_id,
                None,
            ));
        }
        if !registered
            || lease.daemon_id != daemon_id
            || lease.generation != daemon_generation
            || lease.status != firm_core::NodeDaemonLeaseStatus::Active
            || lease.expires_unix_ms <= current_unix_ms()
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "runtime mutation used a stale, foreign, or expired NodeDaemon generation",
                resource_kind,
                resource_id,
                None,
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn require_node_daemon_settlement_authority_unlocked(
        &self,
        execution_space_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        actor: &ActorRef,
        resource_kind: &str,
        resource_id: &str,
    ) -> StoreResult<()> {
        if actor.kind != ActorKind::Service || actor.id != daemon_id {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "runtime settlement requires the exact authenticated NodeDaemon service",
                resource_kind,
                resource_id,
                None,
            ));
        }
        let lease = self.latest_node_daemon_lease(node_id)?.ok_or_else(|| {
            trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "NodeDaemon settlement lease is missing",
                resource_kind,
                resource_id,
                None,
            )
        })?;
        if lease.daemon_id != daemon_id
            || lease.generation != daemon_generation
            || !matches!(
                lease.status,
                firm_core::NodeDaemonLeaseStatus::Active
                    | firm_core::NodeDaemonLeaseStatus::Draining
                    | firm_core::NodeDaemonLeaseStatus::Expired
            )
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "runtime settlement used a released, successor, or foreign NodeDaemon generation",
                resource_kind,
                resource_id,
                None,
            ));
        }
        let registered = self
            .latest_node_project_registrations()?
            .iter()
            .any(|registration| {
                registration.node_id == node_id
                    && registration.execution_space_id == execution_space_id
                    && registration.status == firm_core::NodeProjectRegistrationStatus::Active
            });
        if !registered {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "runtime settlement belongs to an unregistered Execution Space",
                resource_kind,
                resource_id,
                None,
            ));
        }
        Ok(())
    }

    /// Prove that a provider-facing effect still targets the one live
    /// execution driver for this exact AgentSession generation.  This is a
    /// read performed while the caller holds the Store write lock; admission
    /// and the fence observation therefore cannot race another canonical
    /// control-state mutation.
    pub(super) fn require_live_runtime_binding_unlocked(
        &self,
        session: &AgentSession,
        binding: &firm_core::agentfirm_api::RuntimeCommandBinding,
        admission: RuntimeBindingAdmission,
        resource_kind: &str,
        resource_id: &str,
        current_version: Option<u64>,
    ) -> StoreResult<()> {
        let (require_member_run_binding, allow_native_session_attachment, settlement_only) =
            match admission {
                RuntimeBindingAdmission::Invocation => (false, false, false),
                RuntimeBindingAdmission::RuntimeCommand {
                    allow_native_session_attachment,
                    settlement_only,
                } => (
                    matches!(
                        session.control_state.driver_ref,
                        RuntimeDriverRef::TeamSupervisor { .. }
                    ),
                    allow_native_session_attachment,
                    settlement_only,
                ),
            };
        match (
            binding.target_member_run_id.as_deref(),
            binding.target_member_run_generation,
        ) {
            (Some(member_run_id), Some(member_run_generation)) => {
                let members = self
                    .trust_member_runs(&session.execution_space_id)?
                    .into_iter()
                    .filter(|member| member.id == member_run_id)
                    .collect::<Vec<_>>();
                match members.as_slice() {
                    [member]
                        if member.agent_member_id == session.agent_member_id
                            && member.runtime_generation == member_run_generation
                            && member.has_live_runtime_authority() =>
                    {
                        if let RuntimeDriverRef::TeamSupervisor { team_run_id, .. } =
                            &session.control_state.driver_ref
                        {
                            if member.team_run_id != *team_run_id {
                                return Err(trust_error(
                                    TrustErrorCode::MemberRunGenerationFenced,
                                    "provider effect MemberRun belongs to another TeamRun",
                                    resource_kind,
                                    resource_id,
                                    current_version,
                                ));
                            }
                        }
                    }
                    _ => {
                        return Err(trust_error(
                            TrustErrorCode::MemberRunGenerationFenced,
                            "provider effect does not bind the exact active MemberRun identity and generation",
                            resource_kind,
                            resource_id,
                            current_version,
                        ))
                    }
                }
            }
            (None, None) if !require_member_run_binding => (),
            (None, None) => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "team-supervised provider effect requires an exact MemberRun identity and generation",
                    resource_kind,
                    resource_id,
                    current_version,
                ))
            }
            _ => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "provider effect carries a partial MemberRun binding",
                    resource_kind,
                    resource_id,
                    current_version,
                ))
            }
        }
        let composition_matches = session
            .control_state
            .composition_fingerprint
            .as_deref()
            .is_some_and(|current| {
                !current.trim().is_empty()
                    && binding.composition_fingerprint.as_deref() == Some(current)
            });
        let capability_matches = session
            .control_state
            .capability_fingerprint
            .as_deref()
            .is_some_and(|current| {
                !current.trim().is_empty()
                    && binding.capability_fingerprint.as_deref() == Some(current)
            });
        let native_session_matches = binding.native_session_ref == session.native_session_ref
            || (allow_native_session_attachment
                && binding.native_session_ref.is_none()
                && session.native_session_ref.as_ref().is_some_and(|native| {
                    native.provider == session.provider_kind
                        && !native.native_session_id.trim().is_empty()
                }));
        if binding.target_session_id.as_deref() != Some(session.id.as_str())
            || binding.target_runtime_generation != Some(session.runtime_generation)
            || session.control_state.driver_generation == 0
            || binding.target_driver_generation != Some(session.control_state.driver_generation)
            || binding.target_driver != session.control_state.driver_ref
            || !native_session_matches
            || binding.permission_envelope_ref.as_deref()
                != Some(session.permission_envelope_ref.as_str())
            || !composition_matches
            || !capability_matches
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider effect does not bind the exact current session/runtime/driver/native-session/composition/capability/permission state",
                resource_kind,
                resource_id,
                current_version,
            ));
        }

        // NodeDaemon is always the Runtime Supervisor, even when the current
        // next-cycle driver is a TeamSupervisor or provider continuation.
        let daemon_actor = ActorRef {
            kind: ActorKind::Service,
            id: session.node_daemon_id.clone(),
        };
        if settlement_only {
            self.require_node_daemon_settlement_authority_unlocked(
                &session.execution_space_id,
                &session.node_id,
                &session.node_daemon_id,
                session.node_daemon_generation,
                &daemon_actor,
                resource_kind,
                resource_id,
            )?;
        } else {
            self.require_current_node_daemon_unlocked(
                &session.execution_space_id,
                &session.node_id,
                &session.node_daemon_id,
                session.node_daemon_generation,
                &daemon_actor,
                resource_kind,
                resource_id,
            )?;
        }

        match (
            &session.control_state.execution_driver,
            &binding.target_driver,
        ) {
            (
                MemberExecutionDriver::HostDriven,
                RuntimeDriverRef::NodeDaemon {
                    node_daemon_id,
                    node_daemon_generation,
                },
            ) if node_daemon_id == &session.node_daemon_id
                && *node_daemon_generation == session.node_daemon_generation
                && !matches!(
                    session.control_state.continuation.activation,
                    NativeContinuationActivation::Armed { .. }
                ) => {}
            (
                MemberExecutionDriver::HostDriven,
                RuntimeDriverRef::TeamSupervisor {
                    team_run_id,
                    team_supervisor_id,
                    team_supervisor_generation,
                },
            ) => {
                let lease = self
                    .latest_team_supervisor_lease(team_run_id)?
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::SupervisorGenerationFenced,
                            "runtime driver TeamSupervisor lease is missing",
                            resource_kind,
                            resource_id,
                            current_version,
                        )
                    })?;
                let team_run = self
                    .team_runs()?
                    .into_iter()
                    .rev()
                    .find(|run| run.id == *team_run_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::SupervisorGenerationFenced,
                            "runtime driver TeamRun is missing",
                            resource_kind,
                            resource_id,
                            current_version,
                        )
                    })?;
                if lease.supervisor_id != *team_supervisor_id
                    || lease.generation != *team_supervisor_generation
                    || lease.status != firm_core::TeamSupervisorLeaseStatus::Active
                    || lease.expires_unix_ms <= current_unix_ms()
                    || lease.execution_space_id != session.execution_space_id
                    || lease.node_id != session.node_id
                    || lease.node_daemon_id != session.node_daemon_id
                    || lease.node_daemon_generation != session.node_daemon_generation
                    || team_run.execution_node_id != session.node_id
                    || team_run.project_binding_id != lease.project_binding_id
                    || matches!(
                        session.control_state.continuation.activation,
                        NativeContinuationActivation::Armed { .. }
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::SupervisorGenerationFenced,
                        "runtime effect used a stale, foreign, expired, or parent-fenced TeamSupervisor generation",
                        resource_kind,
                        resource_id,
                        current_version,
                    ));
                }
            }
            (
                MemberExecutionDriver::ProviderDriven,
                RuntimeDriverRef::ProviderContinuation {
                    provider,
                    continuation_id,
                    continuation_revision,
                    runtime_generation,
                },
            ) => {
                let continuation = &session.control_state.continuation;
                let activation_matches = matches!(
                    continuation.activation,
                    NativeContinuationActivation::Armed {
                        runtime_generation: armed_runtime_generation,
                        driver_generation: armed_driver_generation,
                    } if armed_runtime_generation == session.runtime_generation
                        && armed_driver_generation == session.control_state.driver_generation
                );
                if provider != &session.provider_kind
                    || *runtime_generation != session.runtime_generation
                    || continuation.definition.continuation_ref.as_deref()
                        != Some(continuation_id.as_str())
                    || continuation.definition.revision != *continuation_revision
                    || continuation.definition.phase != NativeContinuationPhase::Active
                    || !activation_matches
                {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunGenerationFenced,
                        "provider continuation is not the exact active and armed continuation for this runtime/driver generation",
                        resource_kind,
                        resource_id,
                        current_version,
                    ));
                }
            }
            (MemberExecutionDriver::UserDriven, _) => {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "Harness cannot drive provider effects for a user-driven external runtime",
                    resource_kind,
                    resource_id,
                    current_version,
                ));
            }
            _ => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "runtime driver reference is unknown or incompatible with the declared execution driver",
                    resource_kind,
                    resource_id,
                    current_version,
                ));
            }
        }
        Ok(())
    }

    /// Evaluate the semantic predicate carried by a RuntimeCommand against the
    /// same AgentSession snapshot used for driver fencing.  A predicate is not
    /// documentation: if the Store cannot prove it from canonical control
    /// state, the provider effect is rejected before crossing the boundary.
    pub(super) fn require_runtime_command_precondition_unlocked(
        session: &AgentSession,
        command: RuntimeCommandKind,
        precondition: &RuntimeCommandPrecondition,
        poststate: RuntimeCommandPoststate,
        resource_kind: &str,
        resource_id: &str,
        current_version: Option<u64>,
    ) -> StoreResult<()> {
        let fenced = |message: &str| {
            trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                message,
                resource_kind,
                resource_id,
                current_version,
            )
        };

        // At most two canonical mutations may legitimately move the session
        // version past the prepared expectation: a close/resume of the kind
        // this command applies (the predicate reads the current lifecycle, so
        // a session already in that state contributes no bump), and the one
        // write-once native-session bind that attached the provider's id after
        // the command was prepared (GitHub #583). Each is exactly one version
        // bump; they compose (a StopSession prepared before the bind sees both);
        // nothing else may move the version, so anything past the tolerated
        // range is fenced.
        let expected_version_advanced_by_this_command =
            !matches!(poststate, RuntimeCommandPoststate::None)
                && matches!(
                    (command, session.lifecycle),
                    (RuntimeCommandKind::StopSession, AgentSessionStatus::Closed)
                        | (RuntimeCommandKind::ResumeSession, AgentSessionStatus::Cold)
                );
        let expected_version_advanced_by_native_attachment = matches!(
            poststate,
            RuntimeCommandPoststate::CommandWithNativeSessionAttachment
        );
        let tolerated_advance = u64::from(expected_version_advanced_by_this_command)
            + u64::from(expected_version_advanced_by_native_attachment);
        if precondition
            .expected_session_version
            .is_some_and(|expected| {
                session.version < expected
                    || session.version > expected.saturating_add(tolerated_advance)
            })
        {
            return Err(fenced(
                "RuntimeCommand expected_session_version no longer matches the canonical AgentSession",
            ));
        }
        if precondition
            .expected_residency
            .is_some_and(|expected| expected != session.control_state.runtime_residency)
        {
            return Err(fenced(
                "RuntimeCommand expected_residency no longer matches the canonical AgentSession",
            ));
        }
        if precondition
            .expected_activity
            .is_some_and(|expected| expected != session.control_state.activity)
        {
            return Err(fenced(
                "RuntimeCommand expected_activity no longer matches the canonical AgentSession",
            ));
        }
        if precondition
            .expected_execution_driver
            .is_some_and(|expected| expected != session.control_state.execution_driver)
        {
            return Err(fenced(
                "RuntimeCommand expected_execution_driver no longer matches the canonical AgentSession",
            ));
        }

        if let Some(expected) = precondition.expected_cycle_ref.as_ref() {
            if expected.revision.is_some() || expected.fingerprint.is_some() {
                return Err(fenced(
                    "RuntimeCommand cycle revision/fingerprint cannot be proven from canonical AgentSession control state",
                ));
            }
            if session.current_turn_id.as_deref() != Some(expected.id.as_str()) {
                return Err(fenced(
                    "RuntimeCommand expected_cycle_ref no longer matches the current provider cycle",
                ));
            }
        }

        if let Some(expected) = precondition.expected_continuation_ref.as_ref() {
            let definition = &session.control_state.continuation.definition;
            if expected.fingerprint.is_some()
                || definition.continuation_ref.as_deref() != Some(expected.id.as_str())
                || expected
                    .revision
                    .is_some_and(|revision| definition.revision != Some(revision))
            {
                return Err(fenced(
                    "RuntimeCommand expected_continuation_ref cannot be proven against the current continuation definition",
                ));
            }
        }
        if precondition
            .expected_continuation_phase
            .is_some_and(|expected| expected != session.control_state.continuation.definition.phase)
        {
            return Err(fenced(
                "RuntimeCommand expected_continuation_phase no longer matches the canonical continuation definition",
            ));
        }

        let safe_point_satisfied = match precondition.safe_point {
            // Unknown is the serde-default for legacy callers. It makes no
            // claim and therefore contributes no positive proof.
            RuntimeSafePointRequirement::Unknown | RuntimeSafePointRequirement::Immediate => true,
            RuntimeSafePointRequirement::CurrentCycle => {
                session.lifecycle == AgentSessionStatus::Active
                    && session.current_turn_id.is_some()
                    && matches!(
                        session.control_state.activity,
                        RuntimeActivity::Running
                            | RuntimeActivity::WaitingInput
                            | RuntimeActivity::Interrupting
                    )
            }
            RuntimeSafePointRequirement::CycleBoundary => {
                session.control_state.activity == RuntimeActivity::Idle
                    && !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Closed | AgentSessionStatus::RecoveryRequired
                    )
            }
            RuntimeSafePointRequirement::RuntimeIdle => {
                session.control_state.runtime_residency == RuntimeResidency::Attached
                    && session.control_state.activity == RuntimeActivity::Idle
                    && !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Closed | AgentSessionStatus::RecoveryRequired
                    )
            }
            // AgentSession intentionally does not mirror provider child/job
            // or durable-flush state. Only a verified adapter receipt can
            // prove full execution-lane quiescence.
            RuntimeSafePointRequirement::ExecutionLaneQuiesced => false,
        };
        if !safe_point_satisfied {
            return Err(fenced(
                "RuntimeCommand safe_point is not proven by the current canonical AgentSession state",
            ));
        }

        // One-driver authority is independent from a syntactically exact
        // RuntimeDriverRef. A provider continuation may be the live driver,
        // but that never authorizes Harness to start a second top-level cycle.
        if matches!(
            command,
            RuntimeCommandKind::DispatchProvider | RuntimeCommandKind::StartCycle
        ) && session.control_state.execution_driver != MemberExecutionDriver::HostDriven
        {
            return Err(fenced(
                "Harness cannot start a provider cycle while the AgentSession is provider-driven or user-driven",
            ));
        }

        Ok(())
    }

    pub(super) fn hydrate_agent_team_compatibility_projection(
        &self,
        execution_space_id: &str,
        mut team: AgentTeam,
    ) -> StoreResult<AgentTeam> {
        let memberships = self.fabric_team_memberships_for_team(execution_space_id, &team.id)?;
        let hosts = memberships
            .iter()
            .filter(|membership| membership.role == TeamMembershipRole::Host)
            .collect::<Vec<_>>();
        if hosts.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentTeam compatibility read failed closed because Host Membership authority is ambiguous",
                "agent_team",
                &team.id,
                Some(team.revision),
            ));
        }
        team.mission_id = team.legacy_mission_id.clone().unwrap_or_default();
        team.host_agent_id = hosts[0].agent_member_id.clone();
        team.member_ids = memberships
            .into_iter()
            .filter(|membership| {
                membership.role != TeamMembershipRole::Host
                    && membership.state == TeamMembershipStatus::Active
            })
            .map(|membership| membership.agent_member_id)
            .collect();
        Ok(team)
    }

    /// Durable AgentTeams are canonical trust aggregates. Mission linkage is
    /// optional migration provenance and never participates in identity or
    /// creation authority.
    pub fn agent_teams(&self, execution_space_id: &str) -> StoreResult<Vec<AgentTeam>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_team")?
            .values()
            .map(|envelope| {
                event_projection::<AgentTeam>(envelope).and_then(|team| {
                    self.hydrate_agent_team_compatibility_projection(execution_space_id, team)
                })
            })
            .collect()
    }

    pub fn agent_team(
        &self,
        execution_space_id: &str,
        team_id: &str,
    ) -> StoreResult<Option<AgentTeam>> {
        let mut latest = None;
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id == execution_space_id
                && envelope.operation.event.aggregate_kind == "agent_team"
                && envelope.operation.event.aggregate_id == team_id
            {
                latest = Some(envelope);
            }
        }
        latest
            .as_ref()
            .map(|envelope| {
                event_projection::<AgentTeam>(envelope).and_then(|team| {
                    self.hydrate_agent_team_compatibility_projection(execution_space_id, team)
                })
            })
            .transpose()
    }

    /// Scope-preserving Company/read projection. Duplicate ids across spaces
    /// are retained as distinct rows and must never be used as mutation input.
    pub fn all_agent_teams(&self) -> StoreResult<Vec<AgentTeam>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "agent_team" {
                latest.insert(
                    (
                        envelope.execution_space_id.clone(),
                        envelope.operation.event.aggregate_id.clone(),
                    ),
                    envelope,
                );
            }
        }
        latest
            .into_iter()
            .map(|((execution_space_id, _), envelope)| {
                event_projection::<AgentTeam>(&envelope).and_then(|team| {
                    self.hydrate_agent_team_compatibility_projection(&execution_space_id, team)
                })
            })
            .collect()
    }

    pub fn agent_team_scope(&self, team_id: &str) -> StoreResult<Option<String>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .rev()
            .find(|envelope| {
                envelope.operation.event.aggregate_kind == "agent_team"
                    && envelope.operation.event.aggregate_id == team_id
            })
            .map(|envelope| envelope.execution_space_id))
    }
}
