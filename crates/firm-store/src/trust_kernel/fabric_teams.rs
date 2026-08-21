use super::*;

impl HarnessStore {
    pub fn create_agent_team(
        &self,
        context: &MutationContext,
        team: AgentTeam,
        memberships: Vec<TeamMembership>,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        team.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let request_payload = serde_json::json!({"team": team, "memberships": memberships});
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team",
            &team.id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        if team.revision != 1
            || team.status != AgentTeamStatus::Active
            || context.expected_version != 0
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new AgentTeam must be Active at revision 1 with absent CAS",
                "agent_team",
                &team.id,
                Some(0),
            ));
        }
        let node = self
            .latest_execution_nodes()?
            .into_iter()
            .find(|node| node.id == team.node_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam requires its immutable placement Node to exist",
                    "agent_team",
                    &team.id,
                    None,
                )
            })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentTeam requires an Active placement Node",
                "agent_team",
                &team.id,
                None,
            ));
        }
        let members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        let mut membership_ids = BTreeSet::new();
        let mut member_ids = BTreeSet::new();
        let mut active_hosts = 0usize;
        for membership in &memberships {
            required(&membership.id, "TeamMembership.id")?;
            required(
                &membership.agent_member_id,
                "TeamMembership.agent_member_id",
            )?;
            if !membership_ids.insert(membership.id.clone())
                || !member_ids.insert(membership.agent_member_id.clone())
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam creation contains a duplicate Membership or AgentMember",
                    "agent_team",
                    &team.id,
                    None,
                ));
            }
            if membership.team_id != team.id
                || membership.node_id != team.node_id
                || membership.state != TeamMembershipStatus::Active
                || membership.membership_generation != 1
                || membership.revision != 1
                || membership.left_at.is_some()
                || membership.created_by != context.authenticated_actor
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "initial TeamMembership must be active generation/revision 1 on the Team Node and created by the authenticated actor",
                    "team_membership",
                    &membership.id,
                    None,
                ));
            }
            let member = members.get(&membership.agent_member_id).ok_or_else(|| {
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
                    "initial TeamMembership requires an Active AgentMember",
                    "team_membership",
                    &membership.id,
                    Some(member.version),
                ));
            }
            active_hosts += usize::from(membership.role == TeamMembershipRole::Host);
        }
        if active_hosts != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "an Active AgentTeam requires exactly one active Host TeamMembership",
                "agent_team",
                &team.id,
                None,
            ));
        }
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        if committed.iter().any(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && ((envelope.operation.event.aggregate_kind == "agent_team"
                    && envelope.operation.event.aggregate_id == team.id)
                    || (envelope.operation.event.aggregate_kind == "team_membership"
                        && membership_ids.contains(&envelope.operation.event.aggregate_id)))
        }) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentTeam or one of its initial TeamMembership ids already exists",
                "agent_team",
                &team.id,
                Some(0),
            ));
        }
        let mut store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let team_event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "agent_team".into(),
            aggregate_id: team.id.clone(),
            sequence: 1,
            store_sequence,
            transition: "created".into(),
            expected_version: 0,
            resulting_version: team.revision,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: request_fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation: CanonicalOperation {
                event: team_event.clone(),
                resulting_projection: serde_json::to_value(&team)?,
                immutable_side_records: vec![serde_json::to_value(team_inbox_subscription(
                    &context.execution_space_id,
                    &team,
                    MessageSubscriptionStatus::Active,
                    1,
                    &context.authenticated_actor,
                    &team.created_at,
                ))?],
                initial_outbox_records: Vec::new(),
            },
        });
        for membership in &memberships {
            store_sequence += 1;
            let payload = serde_json::to_value(membership)?;
            let membership_event = CanonicalMutationEvent {
                id: format!("trust-event-{store_sequence}"),
                aggregate_kind: "team_membership".into(),
                aggregate_id: membership.id.clone(),
                sequence: 1,
                store_sequence,
                transition: "joined_with_team".into(),
                expected_version: 0,
                resulting_version: membership.revision,
                performed_by_actor: context.authenticated_actor.clone(),
                authority_actor: context.authority_actor.clone(),
                causation_ref: Some(team_event.id.clone()),
                idempotency_key: format!(
                    "{}:initial-membership:{}",
                    context.idempotency_key, membership.id
                ),
                canonical_request_fingerprint: canonical_json_fingerprint(&payload),
                payload,
                created_at: now_string(),
            };
            let subscriptions = membership_subscriptions(
                &context.execution_space_id,
                membership,
                MessageSubscriptionStatus::Active,
                1,
                &membership.joined_at,
            )?
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: format!("{}:initial-membership", context.command_name),
                operation: CanonicalOperation {
                    event: membership_event,
                    resulting_projection: serde_json::to_value(membership)?,
                    immutable_side_records: subscriptions,
                    initial_outbox_records: Vec::new(),
                },
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: team,
            event: team_event,
            replayed: false,
        })
    }

    /// Atomically import one reviewed legacy Team projection without inferring
    /// identities or changing ids. Ambiguous Host/member maps fail before the
    /// trust ledger is mutated.
    pub fn migrate_legacy_agent_team_same_ids(
        &self,
        context: &MutationContext,
        bundle: AgentTeamMigrationBundle,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        bundle
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if context.expected_version != 0
            || bundle.source_fingerprint
                != canonical_json_fingerprint(&serde_json::to_value(&bundle.source)?)
            || bundle
                .memberships
                .iter()
                .any(|membership| membership.created_by != context.authenticated_actor)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "legacy Team migration requires exact source fingerprint, version 0 and authenticated membership creator",
                "agent_team",
                &bundle.target.id,
                Some(0),
            ));
        }
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::to_value(&bundle)?;
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team",
            &bundle.target.id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        for member_id in bundle.identity_id_map.values() {
            let member = members.get(member_id).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "legacy Team migration references a missing same-ID AgentMember",
                    "agent_team",
                    &bundle.target.id,
                    None,
                )
            })?;
            if bundle.target.status == AgentTeamStatus::Active
                && member.organization_status != AgentMemberOrganizationStatus::Active
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Active migrated Team requires every same-ID AgentMember to be Active",
                    "agent_team",
                    &bundle.target.id,
                    Some(member.version),
                ));
            }
        }
        let node = self
            .execution_nodes()?
            .into_iter()
            .find(|node| node.id == bundle.target.node_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "legacy Team migration references a missing immutable Node",
                    "agent_team",
                    &bundle.target.id,
                    None,
                )
            })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "legacy Team migration requires an Active immutable Node placement",
                "agent_team",
                &bundle.target.id,
                None,
            ));
        }
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let membership_ids = bundle
            .memberships
            .iter()
            .map(|membership| membership.id.as_str())
            .collect::<BTreeSet<_>>();
        if committed.iter().any(|envelope| {
            envelope.operation.event.aggregate_id == bundle.target.id
                && envelope.operation.event.aggregate_kind == "agent_team"
                || (envelope.operation.event.aggregate_kind == "team_membership"
                    && membership_ids.contains(envelope.operation.event.aggregate_id.as_str()))
        }) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "legacy Team migration target id already exists",
                "agent_team",
                &bundle.target.id,
                Some(0),
            ));
        }
        let mut store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let team_event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "agent_team".into(),
            aggregate_id: bundle.target.id.clone(),
            sequence: 1,
            store_sequence,
            transition: "migrated_same_ids".into(),
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: bundle.migration_id.clone(),
            canonical_request_fingerprint: request_fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        let subscription_status = if bundle.target.status == AgentTeamStatus::Active {
            MessageSubscriptionStatus::Active
        } else {
            MessageSubscriptionStatus::Paused
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation: CanonicalOperation {
                event: team_event.clone(),
                resulting_projection: serde_json::to_value(&bundle.target)?,
                immutable_side_records: vec![serde_json::to_value(team_inbox_subscription(
                    &context.execution_space_id,
                    &bundle.target,
                    subscription_status,
                    1,
                    &context.authenticated_actor,
                    &bundle.target.updated_at,
                ))?],
                initial_outbox_records: Vec::new(),
            },
        });
        for membership in &bundle.memberships {
            store_sequence += 1;
            let membership_payload = serde_json::to_value(membership)?;
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: format!("{}:membership", context.command_name),
                operation: CanonicalOperation {
                    event: CanonicalMutationEvent {
                        id: format!("trust-event-{store_sequence}"),
                        aggregate_kind: "team_membership".into(),
                        aggregate_id: membership.id.clone(),
                        sequence: 1,
                        store_sequence,
                        transition: "migrated_same_id".into(),
                        expected_version: 0,
                        resulting_version: 1,
                        performed_by_actor: context.authenticated_actor.clone(),
                        authority_actor: context.authority_actor.clone(),
                        causation_ref: Some(team_event.id.clone()),
                        idempotency_key: format!("{}:{}", bundle.migration_id, membership.id),
                        canonical_request_fingerprint: canonical_json_fingerprint(
                            &membership_payload,
                        ),
                        payload: membership_payload,
                        created_at: now_string(),
                    },
                    resulting_projection: serde_json::to_value(membership)?,
                    immutable_side_records: membership_subscriptions(
                        &context.execution_space_id,
                        membership,
                        subscription_status,
                        1,
                        &bundle.target.updated_at,
                    )?
                    .into_iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
                    initial_outbox_records: Vec::new(),
                },
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: bundle.target,
            event: team_event,
            replayed: false,
        })
    }

    pub fn transition_agent_team(
        &self,
        context: &MutationContext,
        team_id: &str,
        next_status: AgentTeamStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "team_id": team_id,
            "next_status": next_status,
            "updated_at": updated_at,
        });
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team",
            team_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let mut current = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_team")?
            .remove(team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam not found",
                    "agent_team",
                    team_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentTeam>(&envelope))?;
        if context.expected_version != current.revision {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentTeam lifecycle CAS does not match the current revision",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        let allowed = matches!(
            (current.status, next_status),
            (AgentTeamStatus::Active, AgentTeamStatus::Inactive)
                | (AgentTeamStatus::Active, AgentTeamStatus::Trashed)
                | (AgentTeamStatus::Inactive, AgentTeamStatus::Active)
                | (AgentTeamStatus::Inactive, AgentTeamStatus::Trashed)
                | (AgentTeamStatus::Trashed, AgentTeamStatus::Inactive)
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentTeam lifecycle transition is not allowed",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        let members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        let mut memberships = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| membership.team_id == team_id)
            .collect::<Vec<_>>();
        let retained_hosts = memberships
            .iter()
            .filter(|membership| membership.role == TeamMembershipRole::Host)
            .collect::<Vec<_>>();
        if retained_hosts.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Inactive/Trashed/Restore requires exactly one retained Host role",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        let host_member = members
            .get(&retained_hosts[0].agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "retained Host TeamMembership references a missing AgentMember",
                    "agent_team",
                    team_id,
                    Some(current.revision),
                )
            })?;
        let authorized = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        ) || (context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == host_member.id);
        if !authorized {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentTeam lifecycle transition requires its retained Host or control-plane authority",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        if current.status == AgentTeamStatus::Trashed
            && next_status == AgentTeamStatus::Inactive
            && host_member.organization_status == AgentMemberOrganizationStatus::Retired
        {
            return Err(trust_error(
                TrustErrorCode::AgentMemberRetired,
                "Trashed AgentTeam cannot restore with a Retired retained Host",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        if next_status == AgentTeamStatus::Active {
            let active_hosts = memberships
                .iter()
                .filter(|membership| {
                    membership.role == TeamMembershipRole::Host
                        && membership.state == TeamMembershipStatus::Active
                })
                .collect::<Vec<_>>();
            if active_hosts.len() != 1
                || members
                    .get(&active_hosts[0].agent_member_id)
                    .is_none_or(|member| {
                        member.organization_status != AgentMemberOrganizationStatus::Active
                    })
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam activation requires exactly one active Host Membership backed by an Active AgentMember",
                    "agent_team",
                    team_id,
                    Some(current.revision),
                ));
            }
            if memberships.iter().any(|membership| {
                membership.state == TeamMembershipStatus::Active
                    && members
                        .get(&membership.agent_member_id)
                        .is_none_or(|member| {
                            member.organization_status != AgentMemberOrganizationStatus::Active
                        })
            }) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam activation found an active Membership without an Active AgentMember",
                    "agent_team",
                    team_id,
                    Some(current.revision),
                ));
            }
        }
        let mut changed_memberships = Vec::new();
        if matches!(
            next_status,
            AgentTeamStatus::Inactive | AgentTeamStatus::Trashed
        ) && current.status != AgentTeamStatus::Trashed
        {
            for membership in &mut memberships {
                if membership.state != TeamMembershipStatus::Inactive {
                    membership.state = TeamMembershipStatus::Inactive;
                    membership.revision += 1;
                    changed_memberships.push(membership.clone());
                }
            }
        }
        current.status = next_status;
        current.revision += 1;
        current.updated_at = updated_at.to_string();
        current.trashed_at =
            (next_status == AgentTeamStatus::Trashed).then(|| updated_at.to_string());
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let previous_team_event = committed
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "agent_team"
                    && envelope.operation.event.aggregate_id == team_id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence)
            .map(|envelope| envelope.operation.event.clone())
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam canonical event is missing",
                    "agent_team",
                    team_id,
                    None,
                )
            })?;
        let current_subscriptions = self
            .fabric_message_subscriptions(&context.execution_space_id)?
            .into_iter()
            .map(|subscription| (subscription.id.clone(), subscription))
            .collect::<BTreeMap<_, _>>();
        let current_team_subscription = current_subscriptions
            .get(&format!("team-inbox:{team_id}"))
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam durable Team-subject subscription is missing",
                    "message_subscription",
                    &format!("team-inbox:{team_id}"),
                    None,
                )
            })?;
        let team_subscription = team_inbox_subscription(
            &context.execution_space_id,
            &current,
            if next_status == AgentTeamStatus::Active {
                MessageSubscriptionStatus::Active
            } else {
                MessageSubscriptionStatus::Paused
            },
            current_team_subscription.revision + 1,
            &current_team_subscription.created_by,
            updated_at,
        );
        let mut store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let team_event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "agent_team".into(),
            aggregate_id: team_id.to_string(),
            sequence: previous_team_event.sequence + 1,
            store_sequence,
            transition: match next_status {
                AgentTeamStatus::Active => "activated",
                AgentTeamStatus::Inactive if previous_team_event.transition == "trashed" => {
                    "restored"
                }
                AgentTeamStatus::Inactive => "deactivated",
                AgentTeamStatus::Trashed => "trashed",
            }
            .into(),
            expected_version: context.expected_version,
            resulting_version: current.revision,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: request_fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation: CanonicalOperation {
                event: team_event.clone(),
                resulting_projection: serde_json::to_value(&current)?,
                immutable_side_records: vec![serde_json::to_value(team_subscription)?],
                initial_outbox_records: Vec::new(),
            },
        });
        for membership in &changed_memberships {
            store_sequence += 1;
            let previous = committed
                .iter()
                .filter(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.operation.event.aggregate_kind == "team_membership"
                        && envelope.operation.event.aggregate_id == membership.id
                })
                .max_by_key(|envelope| envelope.operation.event.sequence)
                .map(|envelope| envelope.operation.event.clone())
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "TeamMembership canonical event is missing",
                        "team_membership",
                        &membership.id,
                        None,
                    )
                })?;
            let subscriptions = membership_subscriptions(
                &context.execution_space_id,
                membership,
                MessageSubscriptionStatus::Paused,
                current_subscriptions
                    .values()
                    .filter(|subscription| {
                        subscription.membership_ref.as_deref() == Some(membership.id.as_str())
                    })
                    .map(|subscription| subscription.revision)
                    .max()
                    .unwrap_or(0)
                    + 1,
                updated_at,
            )?
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
            let membership_payload = serde_json::json!({
                "team_event_id": team_event.id,
                "state": membership.state,
                "updated_at": updated_at,
            });
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: format!("{}:membership", context.command_name),
                operation: CanonicalOperation {
                    event: CanonicalMutationEvent {
                        id: format!("trust-event-{store_sequence}"),
                        aggregate_kind: "team_membership".into(),
                        aggregate_id: membership.id.clone(),
                        sequence: previous.sequence + 1,
                        store_sequence,
                        transition: "team_deactivated".into(),
                        expected_version: previous.resulting_version,
                        resulting_version: membership.revision,
                        performed_by_actor: context.authenticated_actor.clone(),
                        authority_actor: context.authority_actor.clone(),
                        causation_ref: Some(team_event.id.clone()),
                        idempotency_key: format!(
                            "{}:membership:{}",
                            context.idempotency_key, membership.id
                        ),
                        canonical_request_fingerprint: canonical_json_fingerprint(
                            &membership_payload,
                        ),
                        payload: membership_payload,
                        created_at: now_string(),
                    },
                    resulting_projection: serde_json::to_value(membership)?,
                    immutable_side_records: subscriptions,
                    initial_outbox_records: Vec::new(),
                },
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: current,
            event: team_event,
            replayed: false,
        })
    }

    /// Record purge authorization after every recoverable Team lifecycle and
    /// runtime reference is closed. This method never deletes related rows;
    /// physical legacy-ledger deletion remains outside DEV-35.
    pub fn record_agent_team_purge_tombstone(
        &self,
        context: &MutationContext,
        request: AgentTeamPurgeRequest,
    ) -> StoreResult<CanonicalMutationResult<AgentTeamPurgeTombstone>> {
        self.init()?;
        request
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if request.requested_by != context.authenticated_actor || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "Team purge tombstone requires the exact authenticated approved requester and version 0",
                "agent_team_purge_tombstone",
                &request.tombstone_id,
                Some(0),
            ));
        }
        let _lock = self.acquire_write_lock()?;
        let payload = serde_json::to_value(&request)?;
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team_purge_tombstone",
            &request.tombstone_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == request.team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Team purge references a missing AgentTeam",
                    "agent_team",
                    &request.team_id,
                    None,
                )
            })?;
        if team.status != AgentTeamStatus::Trashed
            || team.revision != request.expected_team_revision
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team purge requires the exact current Trashed Team revision",
                "agent_team",
                &request.team_id,
                Some(team.revision),
            ));
        }
        let memberships = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| membership.team_id == team.id)
            .collect::<Vec<_>>();
        let member_ids = memberships
            .iter()
            .map(|membership| membership.agent_member_id.as_str())
            .collect::<BTreeSet<_>>();
        let has_active_reference = memberships
            .iter()
            .any(|membership| membership.state != TeamMembershipStatus::Inactive)
            || self.team_runs()?.iter().any(|run| {
                run.agent_team_id == team.id
                    && !matches!(
                        run.status,
                        firm_core::TeamRunStatus::Completed
                            | firm_core::TeamRunStatus::Failed
                            | firm_core::TeamRunStatus::Cancelled
                    )
            })
            || self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .iter()
                .any(|binding| {
                    binding.team_id == team.id
                        && matches!(
                            binding.status,
                            WorkExecutionBindingStatus::Offered
                                | WorkExecutionBindingStatus::Accepted
                                | WorkExecutionBindingStatus::Active
                        )
                })
            || self
                .fabric_agent_sessions(&context.execution_space_id)?
                .iter()
                .any(|session| {
                    member_ids.contains(session.agent_member_id.as_str())
                        && session.lifecycle != AgentSessionStatus::Closed
                })
            || self
                .fabric_message_deliveries(&context.execution_space_id)?
                .iter()
                .any(|delivery| {
                    delivery.target_team_id.as_deref() == Some(team.id.as_str())
                        && matches!(
                            delivery.status,
                            CanonicalMessageDeliveryStatus::Queued
                                | CanonicalMessageDeliveryStatus::Routed
                                | CanonicalMessageDeliveryStatus::Claimed
                                | CanonicalMessageDeliveryStatus::ProviderReceived
                        )
                });
        if has_active_reference {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team purge is blocked until memberships, runs, bindings, sessions and deliveries are closed",
                "agent_team",
                &request.team_id,
                Some(team.revision),
            ));
        }
        let tombstone = AgentTeamPurgeTombstone {
            id: request.tombstone_id.clone(),
            team_id: team.id,
            team_revision: team.revision,
            approval_ref: request.approval_ref,
            export_manifest_ref: request.export_manifest_ref,
            restore_window_closed_at: request.restore_window_closed_at,
            recorded_by: request.requested_by,
            recorded_at: request.requested_at,
        };
        self.commit_trust_projection_unlocked(
            context,
            "agent_team_purge_tombstone",
            &tombstone.id,
            "recorded_no_delete",
            payload,
            &tombstone,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn update_agent_team_profile(
        &self,
        context: &MutationContext,
        team_id: &str,
        name: &str,
        description: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(name, "AgentTeam.name")?;
        required(description, "AgentTeam.description")?;
        let mut team = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_team")?
            .remove(team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam not found",
                    "agent_team",
                    team_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentTeam>(&envelope))?;
        team.name = name.to_string();
        team.description = description.to_string();
        team.revision += 1;
        team.updated_at = updated_at.to_string();
        team.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.commit_trust_projection_unlocked(
            context,
            "agent_team",
            team_id,
            "profile_updated",
            serde_json::json!({
                "name": name,
                "description": description,
                "updated_at": updated_at,
            }),
            &team,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Explicit pre-activation step used after Inactive/Restore. Activating a
    /// membership never starts or resumes an AgentSession.
    pub fn activate_team_membership(
        &self,
        context: &MutationContext,
        membership_id: &str,
        updated_at: &str,
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
        if membership.state != TeamMembershipStatus::Inactive {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an Inactive TeamMembership can be activated",
                "team_membership",
                membership_id,
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
                    "TeamMembership references a missing AgentTeam",
                    "team_membership",
                    membership_id,
                    None,
                )
            })?;
        if team.status != AgentTeamStatus::Inactive {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership activation is allowed only while its Team is Inactive",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let retained_host =
            self.team_host_membership(&context.execution_space_id, &membership.team_id, false)?;
        let authorized = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        ) || (context.authenticated_actor.kind == ActorKind::AgentMember
            && (context.authenticated_actor.id == membership.agent_member_id
                || context.authenticated_actor.id == retained_host.agent_member_id));
        if !authorized {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership activation requires the Member, retained Host, or control-plane authority",
                "team_membership",
                membership_id,
                Some(membership.revision),
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
                    membership_id,
                    None,
                )
            })?;
        if member.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership activation requires an Active AgentMember",
                "team_membership",
                membership_id,
                Some(member.version),
            ));
        }
        if membership.role == TeamMembershipRole::Host
            && self
                .fabric_team_memberships(&context.execution_space_id)?
                .iter()
                .any(|row| {
                    row.team_id == membership.team_id
                        && row.id != membership.id
                        && row.role == TeamMembershipRole::Host
                        && row.state == TeamMembershipStatus::Active
                })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only one Host TeamMembership may be active",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let subscriptions = membership_subscriptions(
            &context.execution_space_id,
            &membership,
            MessageSubscriptionStatus::Active,
            self.fabric_message_subscriptions(&context.execution_space_id)?
                .into_iter()
                .filter(|subscription| {
                    subscription.membership_ref.as_deref() == Some(membership_id)
                })
                .map(|subscription| subscription.revision)
                .max()
                .unwrap_or(0)
                + 1,
            updated_at,
        )?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
        membership.state = TeamMembershipStatus::Active;
        membership.revision += 1;
        membership.left_at = None;
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            membership_id,
            "activated",
            serde_json::json!({"updated_at": updated_at}),
            &membership,
            subscriptions,
            Vec::new(),
        )
    }
}
