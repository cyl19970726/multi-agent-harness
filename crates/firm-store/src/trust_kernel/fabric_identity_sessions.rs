use super::fabric_foundation::RuntimeBindingAdmission;
use super::*;

impl HarnessStore {
    fn validate_full_access_workspace_unlocked(&self, incoming: &AgentSession) -> StoreResult<()> {
        if incoming.effective_permission_ceiling != PermissionCeiling::FullAccess {
            return Ok(());
        }
        let raw_workspace = incoming.workspace_cwd.as_deref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "FULL_ACCESS_WORKSPACE_REQUIRED: FullAccess AgentSession requires an exact canonical cwd",
                "agent_session",
                &incoming.id,
                None,
            )
        })?;
        let canonical = std::fs::canonicalize(raw_workspace).map_err(|error| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "FULL_ACCESS_WORKSPACE_NOT_CANONICAL: workspace {} cannot be resolved: {error}",
                    raw_workspace
                ),
                "agent_session",
                &incoming.id,
                None,
            )
        })?;
        if canonical.as_path() != Path::new(raw_workspace) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "FULL_ACCESS_WORKSPACE_NOT_CANONICAL: expected exact canonical cwd {}",
                    canonical.display()
                ),
                "agent_session",
                &incoming.id,
                None,
            ));
        }

        Ok(())
    }

    /// AF-ADR-014 compatibility projection. There is no AgentIdentity writer:
    /// every row is derived from the sole durable AgentMember with the same id.
    pub fn fabric_agent_identities(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<AgentIdentity>> {
        Ok(self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .map(|member| AgentIdentity {
                id: member.id,
                display_name: member.name,
                organization_status: member.organization_status,
                permission_ceiling: member.permission_ceiling,
                version: member.version,
                created_at: member.created_at,
                updated_at: member.updated_at,
            })
            .collect())
    }

    pub fn fabric_agent_identities_for_members(
        &self,
        execution_space_id: &str,
        member_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<AgentIdentity>> {
        Ok(self
            .trust_agent_members_for_ids(execution_space_id, member_ids)?
            .into_iter()
            .map(|member| AgentIdentity {
                id: member.id,
                display_name: member.name,
                organization_status: member.organization_status,
                permission_ceiling: member.permission_ceiling,
                version: member.version,
                created_at: member.created_at,
                updated_at: member.updated_at,
            })
            .collect())
    }

    #[deprecated(note = "AgentIdentity is a same-id read-only AgentMember projection")]
    pub fn create_agent_identity(
        &self,
        _context: &MutationContext,
        identity: AgentIdentity,
    ) -> StoreResult<CanonicalMutationResult<AgentIdentity>> {
        Err(trust_error(
            TrustErrorCode::InvalidStateTransition,
            "AGENT_IDENTITY_READ_ONLY: create the sole durable AgentMember instead",
            "agent_identity",
            &identity.id,
            None,
        ))
    }

    /// Explicit one-way AF-ADR-014 migration. The legacy projection id is
    /// preserved exactly while the only written durable aggregate is
    /// AgentMember; no AgentIdentity event or ledger is created.
    pub fn migrate_legacy_agent_identity_same_id(
        &self,
        context: &MutationContext,
        identity: AgentIdentity,
    ) -> StoreResult<CanonicalMutationResult<AgentIdentity>> {
        required(&identity.id, "AgentIdentity.id")?;
        if identity.version != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "legacy AgentIdentity migration requires an explicit version-1 same-ID source",
                "agent_member",
                &identity.id,
                Some(identity.version),
            ));
        }
        let member = AgentMember {
            id: identity.id.clone(),
            name: identity.display_name.clone(),
            description: "Migrated same-ID AgentMember authority".into(),
            role: "agent".into(),
            capabilities: Vec::new(),
            skill_refs: Vec::new(),
            provider_profile_ref: None,
            model_preference: None,
            workspace_policy: "legacy-explicit-migration".into(),
            permission_ceiling: identity.permission_ceiling,
            organization_status: identity.organization_status,
            version: 1,
            created_by: context.authenticated_actor.clone(),
            created_at: identity.created_at.clone(),
            updated_at: identity.updated_at.clone(),
        };
        let migrated = self.create_trust_agent_member(context, member)?;
        let projection = self
            .fabric_agent_identities(&context.execution_space_id)?
            .into_iter()
            .find(|candidate| candidate.id == identity.id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "same-ID AgentIdentity projection was not reconstructable after migration",
                    "agent_member",
                    &identity.id,
                    Some(1),
                )
            })?;
        Ok(CanonicalMutationResult {
            projection,
            event: migrated.event,
            replayed: migrated.replayed,
        })
    }

    pub fn fabric_agent_sessions(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<AgentSession>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_session")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn fabric_agent_sessions_for_members(
        &self,
        execution_space_id: &str,
        member_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<AgentSession>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            let event = &envelope.operation.event;
            if envelope.execution_space_id == execution_space_id
                && event.aggregate_kind == "agent_session"
                && envelope.operation.resulting_projection["agent_member_id"]
                    .as_str()
                    .is_some_and(|id| member_ids.contains(id))
            {
                latest.insert(event.aggregate_id.clone(), envelope);
            }
        }
        latest.values().map(event_projection).collect()
    }

    pub fn create_agent_session(
        &self,
        context: &MutationContext,
        session: AgentSession,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&session.id, "AgentSession.id")?;
        required(&session.agent_member_id, "AgentSession.agent_member_id")?;
        required(&session.node_id, "AgentSession.node_id")?;
        required(&session.provider_kind, "AgentSession.provider_kind")?;
        if session.execution_space_id != context.execution_space_id || session.version != 1 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentSession must start at version 1 in the authenticated Execution Space",
                "agent_session",
                &session.id,
                Some(session.version),
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            &session.id,
        )?;
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == session.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession references a missing AgentMember",
                    "agent_session",
                    &session.id,
                    None,
                )
            })?;
        if member.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession requires an Active AgentMember",
                "agent_session",
                &session.id,
                None,
            ));
        }
        if session.effective_permission_ceiling > member.permission_ceiling {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentSession effective permission exceeds the frozen AgentMember ceiling",
                "agent_session",
                &session.id,
                None,
            ));
        }
        self.validate_full_access_workspace_unlocked(&session)?;
        let current_count = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|row| {
                row.agent_member_id == session.agent_member_id
                    && row.lifecycle != AgentSessionStatus::Closed
            })
            .count();
        if current_count != 0 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentMember already has a current AgentSession; explicit stop or recovery is required",
                "agent_member",
                &session.agent_member_id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "agent_session",
            &session.id,
            "created",
            serde_json::to_value(&session)?,
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn fabric_team_memberships(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<TeamMembership>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            if envelope.operation.event.aggregate_kind == "team_membership" {
                let membership = event_projection::<TeamMembership>(&envelope)?;
                latest.insert(membership.id.clone(), membership);
            }
            for value in envelope
                .operation
                .initial_outbox_records
                .iter()
                .chain(&envelope.operation.immutable_side_records)
            {
                if let Ok(membership) = serde_json::from_value::<TeamMembership>(value.clone()) {
                    latest.insert(membership.id.clone(), membership);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn fabric_team_memberships_for_team(
        &self,
        execution_space_id: &str,
        team_id: &str,
    ) -> StoreResult<Vec<TeamMembership>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            let event = &envelope.operation.event;
            if event.aggregate_kind == "team_membership"
                && envelope.operation.resulting_projection["team_id"].as_str() == Some(team_id)
            {
                let membership = event_projection::<TeamMembership>(&envelope)?;
                latest.insert(membership.id.clone(), membership);
            }
            for value in envelope
                .operation
                .initial_outbox_records
                .iter()
                .chain(&envelope.operation.immutable_side_records)
                .filter(|value| value["team_id"].as_str() == Some(team_id))
            {
                if let Ok(membership) = serde_json::from_value::<TeamMembership>(value.clone()) {
                    latest.insert(membership.id.clone(), membership);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn team_host_membership(
        &self,
        execution_space_id: &str,
        team_id: &str,
        require_active: bool,
    ) -> StoreResult<TeamMembership> {
        let matching = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team_id
                    && membership.role == TeamMembershipRole::Host
                    && (!require_active || membership.state == TeamMembershipStatus::Active)
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "AgentTeam requires exactly one {}Host TeamMembership; found {}",
                    if require_active { "active " } else { "" },
                    matching.len()
                ),
                "agent_team",
                team_id,
                None,
            ));
        }
        Ok(matching.into_iter().next().expect("length checked"))
    }

    pub fn join_team_membership(
        &self,
        context: &MutationContext,
        membership: TeamMembership,
    ) -> StoreResult<CanonicalMutationResult<TeamMembership>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&membership.id, "TeamMembership.id")?;
        required(&membership.team_id, "TeamMembership.team_id")?;
        required(
            &membership.agent_member_id,
            "TeamMembership.agent_member_id",
        )?;
        if membership.revision != 1 || membership.state != TeamMembershipStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new TeamMembership must be active at version 1",
                "team_membership",
                &membership.id,
                Some(membership.revision),
            ));
        }
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == membership.team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing durable AgentTeam",
                    "team_membership",
                    &membership.id,
                    None,
                )
            })?;
        if team.status != AgentTeamStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new TeamMembership requires an Active AgentTeam",
                "team_membership",
                &membership.id,
                Some(team.revision),
            ));
        }
        if team.node_id != membership.node_id {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership must remain on the Team's immutable Node",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        if membership.created_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership.created_by must equal the authenticated actor",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == membership.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing AgentMember",
                    "team_membership",
                    &membership.id,
                    None,
                )
            })?;
        if member.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership requires an Active AgentMember",
                "team_membership",
                &membership.id,
                Some(member.version),
            ));
        }
        // Membership is a generation-fenced collaboration binding.  The
        // cardinality check and the append deliberately share this Store
        // write lock so two concurrent joins cannot both observe an empty
        // active set and create ambiguous authority.
        let prior_memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        if prior_memberships.iter().any(|row| {
            row.team_id == membership.team_id
                && row.agent_member_id == membership.agent_member_id
                && row.state == TeamMembershipStatus::Active
        }) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team and AgentMember already have an active TeamMembership generation",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let expected_generation = prior_memberships
            .iter()
            .filter(|row| {
                row.team_id == membership.team_id
                    && row.agent_member_id == membership.agent_member_id
            })
            .map(|row| row.membership_generation)
            .max()
            .unwrap_or(0)
            + 1;
        if membership.membership_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "TeamMembership generation must be the exact successor generation {expected_generation}"
                ),
                "team_membership",
                &membership.id,
                Some(expected_generation.saturating_sub(1)),
            ));
        }
        if membership.role == TeamMembershipRole::Host
            && prior_memberships.iter().any(|row| {
                row.team_id == membership.team_id
                    && row.role == TeamMembershipRole::Host
                    && row.state == TeamMembershipStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "an Active AgentTeam cannot have more than one active Host TeamMembership",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let subscriptions = membership_subscriptions(
            &context.execution_space_id,
            &membership,
            MessageSubscriptionStatus::Active,
            1,
            &membership.joined_at,
        )?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            &membership.id,
            "joined",
            serde_json::to_value(&membership)?,
            &membership,
            subscriptions,
            Vec::new(),
        )
    }

    pub fn fabric_message_subscriptions(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MessageSubscription>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            if envelope.operation.event.aggregate_kind == "message_subscription" {
                let subscription = event_projection::<MessageSubscription>(&envelope)?;
                latest.insert(subscription.id.clone(), subscription);
            }
            for value in envelope
                .operation
                .initial_outbox_records
                .iter()
                .chain(&envelope.operation.immutable_side_records)
            {
                if let Ok(subscription) =
                    serde_json::from_value::<MessageSubscription>(value.clone())
                {
                    latest.insert(subscription.id.clone(), subscription);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn create_message_subscription(
        &self,
        context: &MutationContext,
        subscription: MessageSubscription,
    ) -> StoreResult<CanonicalMutationResult<MessageSubscription>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&subscription.id, "MessageSubscription.id")?;
        required(
            &subscription.subscriber_ref,
            "MessageSubscription.subscriber_ref",
        )?;
        required(
            &subscription.target_node_id,
            "MessageSubscription.target_node_id",
        )?;
        if subscription.execution_space_id != context.execution_space_id
            || subscription.revision != 1
            || subscription.status != MessageSubscriptionStatus::Active
            || subscription.created_by != context.authenticated_actor
            || context.expected_version != 0
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new MessageSubscription must be active revision 1 in the authenticated Execution Space",
                "message_subscription",
                &subscription.id,
                Some(0),
            ));
        }
        match subscription.subscriber_kind {
            MessageSubjectKind::AgentMember => {
                let member = self
                    .trust_agent_members(&context.execution_space_id)?
                    .into_iter()
                    .find(|member| member.id == subscription.subscriber_ref)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "AgentMember subscription references a missing AgentMember",
                            "message_subscription",
                            &subscription.id,
                            None,
                        )
                    })?;
                if member.organization_status != AgentMemberOrganizationStatus::Active {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "AgentMember subscription requires an Active AgentMember",
                        "message_subscription",
                        &subscription.id,
                        Some(member.version),
                    ));
                }
            }
            MessageSubjectKind::Team => {
                let target_team_id = subscription.target_team_id.as_deref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Team-subject subscription requires target_team_id",
                        "message_subscription",
                        &subscription.id,
                        None,
                    )
                })?;
                let team = self
                    .agent_teams(&context.execution_space_id)?
                    .into_iter()
                    .find(|team| team.id == target_team_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "Team-subject subscription references a missing AgentTeam",
                            "message_subscription",
                            &subscription.id,
                            None,
                        )
                    })?;
                if subscription.subscriber_ref != team.id
                    || subscription.target_node_id != team.node_id
                    || subscription.membership_ref.is_some()
                    || subscription.source_kind != MessageSubscriptionKind::AllAuthorized
                    || subscription.source_ref != "authorized_peer_teams"
                    || subscription.authorization_policy_ref != "collaboration.peer_message_deliver"
                    || team.status != AgentTeamStatus::Active
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Team-subject subscription must name one Active Team/Node and cannot preselect a membership",
                        "message_subscription",
                        &subscription.id,
                        Some(team.revision),
                    ));
                }
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "message_subscription",
            &subscription.id,
            "created",
            serde_json::to_value(&subscription)?,
            &subscription,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn leave_team_membership(
        &self,
        context: &MutationContext,
        membership_id: &str,
        ended_at: &str,
    ) -> StoreResult<CanonicalMutationResult<TeamMembership>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut membership = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "team_membership")?
            .remove(membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership not found",
                    "team_membership",
                    membership_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<TeamMembership>(&envelope))?;
        if membership.state != TeamMembershipStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active TeamMembership can leave",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        if membership.role == TeamMembershipRole::Host
            && self
                .agent_teams(&context.execution_space_id)?
                .into_iter()
                .find(|team| team.id == membership.team_id)
                .is_some_and(|team| team.status == AgentTeamStatus::Active)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "the sole active Host Membership cannot leave an Active AgentTeam; deactivate the Team first",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let active_bindings = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .filter(|binding| {
                binding.team_membership_id == membership.id
                    && binding.status == WorkExecutionBindingStatus::Active
            })
            .map(|binding| binding.work_id)
            .collect::<Vec<_>>();
        if !active_bindings.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "TeamMembership cannot leave with active WorkExecutionBindings: {}",
                    active_bindings.join(",")
                ),
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let host_id = self
            .team_host_membership(&context.execution_space_id, &membership.team_id, false)?
            .agent_member_id;
        let authorized = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        ) || (context.authenticated_actor.kind == ActorKind::AgentMember
            && (context.authenticated_actor.id == membership.agent_member_id
                || context.authenticated_actor.id == host_id));
        if !authorized {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership leave requires the exact stable AgentMember",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let revoked = self
            .fabric_message_subscriptions(&context.execution_space_id)?
            .into_iter()
            .filter(|subscription| {
                subscription.membership_ref.as_deref() == Some(membership_id)
                    && subscription.status == MessageSubscriptionStatus::Active
            })
            .map(|mut subscription| {
                subscription.status = MessageSubscriptionStatus::Revoked;
                subscription.revision += 1;
                subscription.revoked_at = Some(ended_at.to_string());
                serde_json::to_value(subscription)
            })
            .collect::<Result<Vec<_>, _>>()?;
        membership.state = TeamMembershipStatus::Inactive;
        membership.revision += 1;
        membership.left_at = Some(ended_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            membership_id,
            "left",
            serde_json::json!({"ended_at": ended_at}),
            &membership,
            revoked,
            Vec::new(),
        )
    }

    pub fn transition_agent_session(
        &self,
        context: &MutationContext,
        session_id: &str,
        next_status: AgentSessionStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut session = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_session")?
            .remove(session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession not found",
                    "agent_session",
                    session_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentSession>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        let executing_runtime_key = context.idempotency_key.strip_suffix(":effect");
        let runtime_commands = self.runtime_commands(&context.execution_space_id)?;
        let authorized_stop = executing_runtime_key.is_some_and(|key| {
            runtime_commands.iter().any(|command| {
                command.idempotency_key == key
                    && command.command == RuntimeCommandKind::StopSession
                    && command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && command.target_node_daemon_id == session.node_daemon_id
                    && command.target_node_daemon_generation == session.node_daemon_generation
                    && matches!(
                        (command.status, command.effect_certainty),
                        (
                            RuntimeCommandStatus::Accepted,
                            RuntimeEffectCertainty::Unknown
                        ) | (
                            RuntimeCommandStatus::Applied,
                            RuntimeEffectCertainty::Applied
                        )
                    )
            })
        });
        let executing_stop = authorized_stop
            && runtime_commands.iter().any(|command| {
                executing_runtime_key == Some(command.idempotency_key.as_str())
                    && command.status == RuntimeCommandStatus::Accepted
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
            });
        let ambiguous_effect_for_session = runtime_commands.iter().any(|command| {
            command.target_session_id.as_deref() == Some(session.id.as_str())
                && command.target_session_generation == Some(session.runtime_generation)
                && matches!(
                    command.status,
                    RuntimeCommandStatus::Accepted
                        | RuntimeCommandStatus::Quiesced
                        | RuntimeCommandStatus::RecoveryRequired
                )
                && command.effect_certainty == RuntimeEffectCertainty::Unknown
        });
        // A NodeDaemon drain or hard-crash recovery kills the owned provider
        // process groups and settles every mid-turn Session as `Interrupted`.
        // That is an honest record of a cycle that never reached its own end,
        // not a terminal state: the successor generation must be able to open a
        // fresh cycle on the same provider-native session. This is the one exit
        // from `Interrupted` back into the ordinary lane, and it is admitted
        // only while the lane still proves the killed runtime is gone — no live
        // handle, no cycle, no turn, continuation disarmed, no queued native
        // input, and no ambiguous RuntimeCommand that a resume could replay.
        // The exact-current-NodeDaemon fence above is the other half of the
        // proof: a drained Session still carries its dead daemon generation and
        // can only reach the live one through `reattach_agent_session_to_node_daemon`,
        // which itself requires the predecessor lease to be explicitly Released.
        let interrupted_runtime_is_terminated = session.control_state.runtime_residency
            == RuntimeResidency::Detached
            && session.control_state.activity == RuntimeActivity::Idle
            && session.control_state.handoff_state
                == firm_core::agentfirm_api::DriverHandoffState::None
            && session.control_state.continuation.activation
                == NativeContinuationActivation::Disarmed
            && session.current_turn_id.is_none()
            && session.queued_input_count == 0
            && !ambiguous_effect_for_session;
        let resumes_terminated_interrupted_lane = session.lifecycle
            == AgentSessionStatus::Interrupted
            && next_status == AgentSessionStatus::Idle
            && interrupted_runtime_is_terminated;
        // A lane that reached `RecoveryRequired` (an unrecoverable provider
        // error on an open cycle, or a Cold session that could not open) had no
        // exit at all (GitHub #755). After operator reconciliation it re-enters
        // the ordinary lane through `Idle` — the one exit, admitted only under
        // the same terminated-lane proof as the drain exit above, never on the
        // lifecycle alone; `team-run recover` is its writer, and Close or a
        // fresh start then proceed from `Idle` through the ordinary paths.
        let recovers_reconciled_lane = session.lifecycle == AgentSessionStatus::RecoveryRequired
            && next_status == AgentSessionStatus::Idle
            && interrupted_runtime_is_terminated;
        let allowed = matches!(
            (session.lifecycle, next_status),
            (AgentSessionStatus::Cold, AgentSessionStatus::Idle)
                | (
                    AgentSessionStatus::Cold,
                    AgentSessionStatus::RecoveryRequired
                )
                | (AgentSessionStatus::Idle, AgentSessionStatus::Active)
                | (AgentSessionStatus::Idle, AgentSessionStatus::Closed)
                | (AgentSessionStatus::Active, AgentSessionStatus::Waiting)
                | (AgentSessionStatus::Active, AgentSessionStatus::Idle)
                | (AgentSessionStatus::Active, AgentSessionStatus::Interrupted)
                | (
                    AgentSessionStatus::Active,
                    AgentSessionStatus::RecoveryRequired
                )
                | (AgentSessionStatus::Waiting, AgentSessionStatus::Active)
                | (AgentSessionStatus::Waiting, AgentSessionStatus::Idle)
                | (AgentSessionStatus::Waiting, AgentSessionStatus::Closed)
                | (AgentSessionStatus::Interrupted, AgentSessionStatus::Cold)
                | (AgentSessionStatus::Interrupted, AgentSessionStatus::Closed)
        ) || (matches!(
            session.lifecycle,
            AgentSessionStatus::Cold | AgentSessionStatus::Active
        ) && next_status == AgentSessionStatus::Closed
            && authorized_stop)
            || resumes_terminated_interrupted_lane
            || recovers_reconciled_lane;
        if !allowed {
            // Name the exact fence for the drain-recovery case so an operator
            // reads "the lane is not proven dead yet", not a bare table miss.
            let message = if session.lifecycle == AgentSessionStatus::Interrupted
                && next_status == AgentSessionStatus::Idle
            {
                firm_core::agentfirm_api::AGENT_SESSION_DRAIN_RESUME_NOT_YET_RESUMABLE.to_string()
            } else if session.lifecycle == AgentSessionStatus::RecoveryRequired
                && next_status == AgentSessionStatus::Idle
            {
                firm_core::agentfirm_api::AGENT_SESSION_RECOVERY_REQUIRED_NOT_YET_RESUMABLE
                    .to_string()
            } else {
                format!(
                    "invalid AgentSession transition {:?}->{next_status:?}",
                    session.lifecycle
                )
            };
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                message,
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if matches!(
            next_status,
            AgentSessionStatus::Closed | AgentSessionStatus::Interrupted
        ) {
            let active_work = self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .into_iter()
                .any(|binding| {
                    binding.agent_session_id == session.id
                        && binding.agent_session_generation == session.runtime_generation
                        && binding.status == WorkExecutionBindingStatus::Active
                });
            let uncertain_command = runtime_commands.into_iter().any(|command| {
                command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && matches!(
                        command.status,
                        RuntimeCommandStatus::Accepted
                            | RuntimeCommandStatus::Quiesced
                            | RuntimeCommandStatus::RecoveryRequired
                    )
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
                    && !(executing_stop
                        && executing_runtime_key == Some(command.idempotency_key.as_str()))
            });
            if active_work || uncertain_command {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    if active_work {
                        "AgentSession cannot close or interrupt while an active WorkExecutionBinding exists; release or atomically rebind it first"
                    } else {
                        "AgentSession cannot close or interrupt while a RuntimeCommand effect is ambiguous; reconcile it first"
                    },
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        }
        session.lifecycle = next_status;
        session.version += 1;
        session.last_active_at = updated_at.to_string();
        match next_status {
            AgentSessionStatus::Active => {
                session.current_turn_id =
                    Some(format!("provider-turn:{}:{}", session.id, session.version));
                session.queued_input_count = session.queued_input_count.saturating_sub(1);
            }
            AgentSessionStatus::Idle
            | AgentSessionStatus::Waiting
            | AgentSessionStatus::Interrupted
            | AgentSessionStatus::RecoveryRequired
            | AgentSessionStatus::Closed => session.current_turn_id = None,
            AgentSessionStatus::Cold => {}
        }
        if next_status == AgentSessionStatus::Closed {
            session.closed_at = Some(updated_at.to_string());
        }
        self.commit_trust_projection_unlocked(
            context,
            "agent_session",
            session_id,
            "status_changed",
            serde_json::json!({"status": next_status, "updated_at": updated_at}),
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Transfer one quiescent AgentSession to the current NodeDaemon
    /// generation without changing the provider-native session identity or
    /// the AgentSession runtime generation. The daemon and driver generations
    /// are independent fences: advancing them invalidates every old provider
    /// command while preserving exact WorkExecutionBindings to this session.
    ///
    /// A session that may have owned a provider process can move only after
    /// the predecessor lease was explicitly released. Lease expiry alone is
    /// not evidence that writable children were drained.
    #[allow(clippy::too_many_arguments)] // exact old/new daemon and session fences stay explicit at this mutation boundary
    pub fn reattach_agent_session_to_node_daemon(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_runtime_generation: u64,
        expected_predecessor_daemon_generation: u64,
        successor_daemon_id: &str,
        successor_daemon_generation: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "session_id": session_id,
            "expected_runtime_generation": expected_runtime_generation,
            "expected_predecessor_daemon_generation": expected_predecessor_daemon_generation,
            "successor_daemon_id": successor_daemon_id,
            "successor_daemon_generation": successor_daemon_generation,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        let mut commit_context = context.clone();
        commit_context.request_fingerprint = Some(request_fingerprint.clone());
        if let Some(replay) = self.replay_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }

        let mut session = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_session")?
            .remove(session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession not found",
                    "agent_session",
                    session_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentSession>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            successor_daemon_id,
            successor_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        if session.runtime_generation != expected_runtime_generation
            || session.node_daemon_generation != expected_predecessor_daemon_generation
            || expected_predecessor_daemon_generation >= successor_daemon_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "AgentSession reattach used a stale session or daemon generation",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        // A reconciled `RecoveryRequired` lane (detached, turn-free) is skipped
        // by both drain settlements, so it can outlive its daemon generation;
        // the successor must be able to reattach it, or no writer could ever
        // reach it again (GitHub #755). The clauses below plus the released
        // predecessor lease are the same terminated-lane proof.
        let lane_is_quiescent = matches!(
            session.lifecycle,
            AgentSessionStatus::Cold
                | AgentSessionStatus::Idle
                | AgentSessionStatus::Interrupted
                | AgentSessionStatus::RecoveryRequired
        ) && session.current_turn_id.is_none()
            && session.queued_input_count == 0
            && matches!(
                session.control_state.runtime_residency,
                RuntimeResidency::Detached | RuntimeResidency::Attached
            )
            && session.control_state.activity == RuntimeActivity::Idle
            && session.control_state.handoff_state
                == firm_core::agentfirm_api::DriverHandoffState::None
            && matches!(
                session.control_state.continuation.activation,
                NativeContinuationActivation::Disarmed
            );
        if !lane_is_quiescent {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession reattach requires a quiescent, continuation-disarmed lane with no queued native input",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        let ambiguous_effect = self
            .runtime_commands(&context.execution_space_id)?
            .into_iter()
            .any(|command| {
                command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && command.target_node_daemon_generation
                        == expected_predecessor_daemon_generation
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
                    && matches!(
                        command.status,
                        RuntimeCommandStatus::Accepted
                            | RuntimeCommandStatus::Quiesced
                            | RuntimeCommandStatus::RecoveryRequired
                    )
            });
        if ambiguous_effect {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession reattach requires reconciliation of every predecessor RuntimeCommand",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let predecessor_was_released = self
            .read_jsonl::<firm_core::NodeDaemonLease>("node_daemon_leases.jsonl")?
            .into_iter()
            .rfind(|lease| {
                lease.node_id == session.node_id
                    && lease.daemon_id == session.node_daemon_id
                    && lease.generation == expected_predecessor_daemon_generation
            })
            .is_some_and(|lease| lease.status == firm_core::NodeDaemonLeaseStatus::Released);
        let predecessor_may_have_owned_runtime = session.native_session_ref.is_some()
            || session.control_state.runtime_residency != RuntimeResidency::Detached;
        if predecessor_may_have_owned_runtime && !predecessor_was_released {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession reattach requires an explicit predecessor NodeDaemon release; lease expiry is not a provider-drain receipt",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let predecessor_daemon_id = session.node_daemon_id.clone();
        session.node_daemon_id = successor_daemon_id.to_string();
        session.node_daemon_generation = successor_daemon_generation;
        session.control_state.runtime_residency = RuntimeResidency::Detached;
        session.control_state.activity = RuntimeActivity::Idle;
        session.control_state.driver_generation = session
            .control_state
            .driver_generation
            .saturating_add(1)
            .max(1);
        session.control_state.driver_ref = RuntimeDriverRef::NodeDaemon {
            node_daemon_id: successor_daemon_id.to_string(),
            node_daemon_generation: successor_daemon_generation,
        };
        session.control_state.handoff_state = firm_core::agentfirm_api::DriverHandoffState::None;
        session.control_state.continuation.activation = NativeContinuationActivation::Disarmed;
        session.control_state.last_reconciled_at = Some(updated_at.to_string());
        session.version += 1;
        session.last_active_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            "node_daemon_reattached",
            serde_json::json!({
                "predecessor_daemon_id": predecessor_daemon_id,
                "predecessor_daemon_generation": expected_predecessor_daemon_generation,
                "successor_daemon_id": successor_daemon_id,
                "successor_daemon_generation": successor_daemon_generation,
                "runtime_generation": session.runtime_generation,
                "driver_generation": session.control_state.driver_generation,
                "updated_at": updated_at,
            }),
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Write the settled provider-native Session binding onto the canonical
    /// AgentSession. A fresh-start session is materialized before the provider
    /// thread exists (`native_session_ref` starts unset), so the settled binding
    /// lands later as its own CAS + generation-fenced mutation. Lifecycle and
    /// runtime generation are untouched. The write is idempotent for the same
    /// native id and rejects a conflicting rebind to another id.
    pub fn bind_agent_session_native_session(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_generation: u64,
        native_session_ref: NativeSessionRef,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(
            &native_session_ref.native_session_id,
            "NativeSessionRef.native_session_id",
        )?;
        required(&native_session_ref.provider, "NativeSessionRef.provider")?;
        let mut session = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_session")?
            .remove(session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession not found",
                    "agent_session",
                    session_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentSession>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        if session.lifecycle == AgentSessionStatus::Closed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "a closed AgentSession cannot bind a provider-native Session",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if session.runtime_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "AgentSession runtime generation is {}, the settled binding observed {expected_generation}",
                    session.runtime_generation
                ),
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if let Some(current) = session.native_session_ref.as_ref() {
            if current.native_session_id != native_session_ref.native_session_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession already binds another provider-native Session",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        }
        session.native_session_ref = Some(native_session_ref.clone());
        session.version += 1;
        self.commit_trust_projection_unlocked(
            context,
            "agent_session",
            session_id,
            "native_session_bound",
            serde_json::json!({
                "session_id": session_id,
                "runtime_generation": expected_generation,
                "native_session_ref": native_session_ref,
            }),
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Replace the bounded runtime-control projection for one exact session
    /// generation.  This is not a runtime event stream: it is the current
    /// fencing state used to decide whether a later provider effect is still
    /// authorized. Driver or composition changes require a provably quiet
    /// lane and advance the driver generation exactly once.
    pub fn bind_agent_session_control_state(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_runtime_generation: u64,
        next_control_state: AgentSessionControlState,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "session_id": session_id,
            "expected_runtime_generation": expected_runtime_generation,
            "next_control_state": next_control_state,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        let mut commit_context = context.clone();
        commit_context.request_fingerprint = Some(request_fingerprint.clone());
        if let Some(replay) = self.replay_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }

        let mut session = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_session")?
            .remove(session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession not found",
                    "agent_session",
                    session_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentSession>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        if session.runtime_generation != expected_runtime_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "control-state mutation used a stale AgentSession runtime generation",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        let ambiguous = self
            .runtime_commands(&context.execution_space_id)?
            .into_iter()
            .any(|command| {
                command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
                    && matches!(
                        command.status,
                        RuntimeCommandStatus::Accepted
                            | RuntimeCommandStatus::Quiesced
                            | RuntimeCommandStatus::RecoveryRequired
                    )
            });
        if ambiguous {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "control-state mutation requires reconciliation of every ambiguous RuntimeCommand",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let driver_changed = session.control_state.execution_driver
            != next_control_state.execution_driver
            || session.control_state.driver_ref != next_control_state.driver_ref
            || session.control_state.driver_generation != next_control_state.driver_generation;
        let composition_changed = session.control_state.composition_fingerprint
            != next_control_state.composition_fingerprint
            || session.control_state.capability_fingerprint
                != next_control_state.capability_fingerprint;
        if driver_changed || composition_changed {
            let lane_is_quiet = session.current_turn_id.is_none()
                && (session.control_state.runtime_residency == RuntimeResidency::Detached
                    || session.control_state.activity == RuntimeActivity::Idle);
            if !lane_is_quiet {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "driver/composition transfer requires a provably Detached or Idle execution lane",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
            if next_control_state.driver_generation
                != session.control_state.driver_generation.saturating_add(1)
                || next_control_state.handoff_state
                    != firm_core::agentfirm_api::DriverHandoffState::None
            {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "driver/composition transfer must advance the driver generation exactly once and finish the handoff",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        } else if next_control_state.driver_generation == 0 {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "an active control-state binding requires a non-zero driver generation",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let mut candidate = session.clone();
        candidate.control_state = next_control_state.clone();
        match candidate.control_state.execution_driver {
            MemberExecutionDriver::UserDriven => {
                if candidate.control_state.driver_generation == 0
                    || candidate.control_state.driver_ref != RuntimeDriverRef::Unknown
                    || matches!(
                        candidate.control_state.continuation.activation,
                        NativeContinuationActivation::Armed { .. }
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunGenerationFenced,
                        "user-driven runtimes must remain non-driven by Harness and continuation-disarmed",
                        "agent_session",
                        session_id,
                        Some(session.version),
                    ));
                }
            }
            MemberExecutionDriver::HostDriven | MemberExecutionDriver::ProviderDriven => {
                let candidate_binding = runtime_binding_for_session(&candidate);
                self.require_live_runtime_binding_unlocked(
                    &candidate,
                    &candidate_binding,
                    RuntimeBindingAdmission::Invocation,
                    "agent_session",
                    session_id,
                    Some(session.version),
                )?;
            }
        }

        session.control_state = next_control_state;
        session.version += 1;
        self.commit_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            "control_state_bound",
            request_payload,
            &session,
            Vec::new(),
            Vec::new(),
        )
    }
}
