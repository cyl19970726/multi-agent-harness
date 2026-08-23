use super::*;

impl Validate for ProviderLaunchProfile {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderLaunchProfile.id")?;
        require_non_empty(&self.name, "ProviderLaunchProfile.name")?;
        require_non_empty(&self.description, "ProviderLaunchProfile.description")?;
        require_non_empty(&self.role, "ProviderLaunchProfile.role")?;
        require_non_empty(&self.provider, "ProviderLaunchProfile.provider")?;
        require_non_empty(&self.created_at, "ProviderLaunchProfile.created_at")
    }
}

impl Validate for AgentTeam {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentTeam.id")?;
        require_non_empty(&self.name, "AgentTeam.name")?;
        require_non_empty(&self.description, "AgentTeam.description")?;
        require_uuid(&self.node_id, "AgentTeam.node_id")?;
        if self.revision == 0 {
            return Err(ValidationError::Invalid {
                field: "AgentTeam.revision",
                reason: "must be at least 1",
            });
        }
        if self.legacy_mission_id.as_deref().is_some_and(str::is_empty) {
            return Err(ValidationError::Invalid {
                field: "AgentTeam.legacy_mission_id",
                reason: "must not be empty when present",
            });
        }
        match self.status {
            AgentTeamStatus::Trashed if self.trashed_at.as_deref().is_none_or(str::is_empty) => {
                return Err(ValidationError::Invalid {
                    field: "AgentTeam.trashed_at",
                    reason: "must be present for a Trashed Team",
                });
            }
            AgentTeamStatus::Active | AgentTeamStatus::Inactive if self.trashed_at.is_some() => {
                return Err(ValidationError::Invalid {
                    field: "AgentTeam.trashed_at",
                    reason: "must be absent unless the Team is Trashed",
                });
            }
            _ => {}
        }
        require_non_empty(&self.created_at, "AgentTeam.created_at")?;
        require_non_empty(&self.updated_at, "AgentTeam.updated_at")
    }
}

impl Validate for agentfirm_api::AgentSession {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentSession.id")?;
        require_non_empty(&self.agent_member_id, "AgentSession.agent_member_id")?;
        require_uuid(&self.node_id, "AgentSession.node_id")?;
        require_non_empty(&self.execution_space_id, "AgentSession.execution_space_id")?;
        require_non_empty(&self.node_daemon_id, "AgentSession.node_daemon_id")?;
        if self.node_daemon_generation == 0 || self.runtime_generation == 0 || self.version == 0 {
            return Err(ValidationError::Invalid {
                field: "AgentSession.generation",
                reason: "daemon/runtime generations and version must be at least 1",
            });
        }
        require_non_empty(&self.provider_kind, "AgentSession.provider_kind")?;
        require_non_empty(
            &self.provider_profile_ref,
            "AgentSession.provider_profile_ref",
        )?;
        require_non_empty(
            &self.permission_envelope_ref,
            "AgentSession.permission_envelope_ref",
        )?;
        if let Some(workspace_cwd) = &self.workspace_cwd {
            require_non_empty(workspace_cwd, "AgentSession.workspace_cwd")?;
        }
        if self.effective_permission_ceiling == agentfirm_api::PermissionCeiling::FullAccess
            && self.workspace_cwd.is_none()
        {
            return Err(ValidationError::Invalid {
                field: "AgentSession.workspace_cwd",
                reason: "FullAccess requires an exact canonical workspace cwd",
            });
        }
        require_non_empty(&self.opened_at, "AgentSession.opened_at")?;
        require_non_empty(&self.last_active_at, "AgentSession.last_active_at")
    }
}

impl Validate for agentfirm_api::TeamMembership {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "TeamMembership.id")?;
        require_non_empty(&self.team_id, "TeamMembership.team_id")?;
        require_non_empty(&self.agent_member_id, "TeamMembership.agent_member_id")?;
        require_uuid(&self.node_id, "TeamMembership.node_id")?;
        if self.membership_generation == 0 || self.revision == 0 {
            return Err(ValidationError::Invalid {
                field: "TeamMembership.generation",
                reason: "membership generation and revision must be at least 1",
            });
        }
        require_non_empty(&self.created_by.id, "TeamMembership.created_by.id")?;
        require_non_empty(&self.joined_at, "TeamMembership.joined_at")
    }
}

impl Validate for agentfirm_api::AgentTeamMigrationBundle {
    fn validate(&self) -> Result<(), ValidationError> {
        use agentfirm_api::{LegacyAgentTeamStatus, TeamMembershipRole, TeamMembershipStatus};
        require_non_empty(&self.migration_id, "AgentTeamMigrationBundle.migration_id")?;
        require_non_empty(
            &self.source_fingerprint,
            "AgentTeamMigrationBundle.source_fingerprint",
        )?;
        require_non_empty(&self.source.id, "LegacyAgentTeamProjection.id")?;
        require_non_empty(
            &self.source.host_agent_id,
            "LegacyAgentTeamProjection.host_agent_id",
        )?;
        require_uuid(&self.source.node_id, "LegacyAgentTeamProjection.node_id")?;
        self.target.validate()?;
        let expected_status = match self.source.status {
            LegacyAgentTeamStatus::Active => AgentTeamStatus::Active,
            LegacyAgentTeamStatus::Closed => AgentTeamStatus::Inactive,
            LegacyAgentTeamStatus::Archived => AgentTeamStatus::Trashed,
        };
        if self.target.id != self.source.id
            || self.target.name != self.source.name
            || self.target.description != self.source.description
            || self.target.node_id != self.source.node_id
            || self.target.status != expected_status
            || self.target.revision != 1
            || self.target.legacy_mission_id.as_deref() != Some(self.source.mission_id.as_str())
            || self.target.created_at != self.source.created_at
            || self.target.updated_at != self.source.updated_at
        {
            return Err(ValidationError::Invalid {
                field: "AgentTeamMigrationBundle.target",
                reason: "must preserve Team id, placement, provenance, timestamps and closed lifecycle mapping",
            });
        }
        let mut expected_ids = BTreeSet::from([self.source.host_agent_id.clone()]);
        if self
            .source
            .member_ids
            .iter()
            .any(|id| !expected_ids.insert(id.clone()))
        {
            return Err(ValidationError::Invalid {
                field: "LegacyAgentTeamProjection.member_ids",
                reason: "legacy Host/member identity is ambiguous",
            });
        }
        let mapped_ids = self
            .identity_id_map
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if mapped_ids != expected_ids
            || self
                .identity_id_map
                .iter()
                .any(|(legacy, member)| legacy != member)
        {
            return Err(ValidationError::Invalid {
                field: "AgentTeamMigrationBundle.identity_id_map",
                reason: "every AgentIdentity must map to the same AgentMember id with no omission or alias",
            });
        }
        let membership_ids = self
            .memberships
            .iter()
            .map(|membership| membership.agent_member_id.clone())
            .collect::<BTreeSet<_>>();
        let unique_membership_rows = self
            .memberships
            .iter()
            .map(|membership| membership.id.clone())
            .collect::<BTreeSet<_>>();
        let expected_membership_state = if expected_status == AgentTeamStatus::Active {
            TeamMembershipStatus::Active
        } else {
            TeamMembershipStatus::Inactive
        };
        let hosts = self
            .memberships
            .iter()
            .filter(|membership| membership.role == TeamMembershipRole::Host)
            .count();
        if membership_ids != expected_ids
            || unique_membership_rows.len() != self.memberships.len()
            || hosts != 1
            || self.memberships.iter().any(|membership| {
                membership.team_id != self.target.id
                    || membership.node_id != self.target.node_id
                    || membership.membership_generation != 1
                    || membership.revision != 1
                    || membership.state != expected_membership_state
                    || (membership.agent_member_id == self.source.host_agent_id)
                        != (membership.role == TeamMembershipRole::Host)
            })
        {
            return Err(ValidationError::Invalid {
                field: "AgentTeamMigrationBundle.memberships",
                reason:
                    "must preserve one exact Host, all same-ID members, placement and generation 1",
            });
        }
        Ok(())
    }
}

impl Validate for agentfirm_api::AgentTeamPurgeRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.tombstone_id, "AgentTeamPurgeRequest.tombstone_id")?;
        require_non_empty(&self.team_id, "AgentTeamPurgeRequest.team_id")?;
        if self.expected_team_revision == 0 {
            return Err(ValidationError::Invalid {
                field: "AgentTeamPurgeRequest.expected_team_revision",
                reason: "must be at least 1",
            });
        }
        require_non_empty(&self.approval_ref, "AgentTeamPurgeRequest.approval_ref")?;
        require_non_empty(
            &self.export_manifest_ref,
            "AgentTeamPurgeRequest.export_manifest_ref",
        )?;
        require_non_empty(
            &self.restore_window_closed_at,
            "AgentTeamPurgeRequest.restore_window_closed_at",
        )?;
        require_non_empty(
            &self.requested_by.id,
            "AgentTeamPurgeRequest.requested_by.id",
        )?;
        require_non_empty(&self.requested_at, "AgentTeamPurgeRequest.requested_at")
    }
}

impl Validate for agentfirm_api::WorkExecutionBinding {
    fn validate(&self) -> Result<(), ValidationError> {
        for (value, field) in [
            (&self.id, "WorkExecutionBinding.id"),
            (&self.work_id, "WorkExecutionBinding.work_id"),
            (&self.team_id, "WorkExecutionBinding.team_id"),
            (
                &self.team_membership_id,
                "WorkExecutionBinding.team_membership_id",
            ),
            (
                &self.agent_member_id,
                "WorkExecutionBinding.agent_member_id",
            ),
            (
                &self.agent_session_id,
                "WorkExecutionBinding.agent_session_id",
            ),
            (&self.delivery_id, "WorkExecutionBinding.delivery_id"),
        ] {
            require_non_empty(value, field)?;
        }
        if self.work_revision == 0
            || self.agent_session_generation == 0
            || self.binding_generation == 0
            || self.version == 0
        {
            return Err(ValidationError::Invalid {
                field: "WorkExecutionBinding.generation",
                reason: "all revisions, generations and version must be at least 1",
            });
        }
        require_non_empty(&self.created_by.id, "WorkExecutionBinding.created_by.id")?;
        require_non_empty(&self.bound_at, "WorkExecutionBinding.bound_at")
    }
}

impl Validate for agentfirm_api::Message {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Message.id")?;
        require_non_empty(
            &self.source_execution_space_id,
            "Message.source_execution_space_id",
        )?;
        require_uuid(&self.source_node_id, "Message.source_node_id")?;
        require_non_empty(&self.source_node_daemon_id, "Message.source_node_daemon_id")?;
        if self.source_authority_generation == 0 || self.schema_version == 0 {
            return Err(ValidationError::Invalid {
                field: "Message.generation",
                reason: "authority generation and schema version must be at least 1",
            });
        }
        if self.recipients.is_empty() {
            return Err(ValidationError::Invalid {
                field: "Message.recipients",
                reason: "must contain at least one recipient",
            });
        }
        require_non_empty(&self.sender_actor_ref.id, "Message.sender_actor_ref.id")?;
        require_non_empty(&self.target_ref.id, "Message.target_ref.id")?;
        require_non_empty(&self.body, "Message.body")?;
        require_non_empty(&self.body_digest, "Message.body_digest")?;
        require_non_empty(&self.correlation_id, "Message.correlation_id")?;
        require_non_empty(&self.content_fingerprint, "Message.content_fingerprint")?;
        require_non_empty(&self.idempotency_key, "Message.idempotency_key")?;
        require_non_empty(&self.created_at, "Message.created_at")
    }
}

impl Validate for agentfirm_api::MessageSubscription {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "MessageSubscription.id")?;
        require_non_empty(&self.subscriber_ref, "MessageSubscription.subscriber_ref")?;
        require_non_empty(
            &self.execution_space_id,
            "MessageSubscription.execution_space_id",
        )?;
        require_uuid(&self.target_node_id, "MessageSubscription.target_node_id")?;
        require_non_empty(&self.source_ref, "MessageSubscription.source_ref")?;
        require_non_empty(
            &self.authorization_policy_ref,
            "MessageSubscription.authorization_policy_ref",
        )?;
        require_non_empty(&self.policy_digest, "MessageSubscription.policy_digest")?;
        if self.policy_revision == 0 || self.revision == 0 {
            return Err(ValidationError::Invalid {
                field: "MessageSubscription.revision",
                reason: "policy and subscription revisions must be at least 1",
            });
        }
        if self.subscriber_kind == agentfirm_api::MessageSubjectKind::Team
            && (self.target_team_id.as_deref().is_none_or(str::is_empty)
                || self.source_kind != agentfirm_api::MessageSubscriptionKind::AllAuthorized
                || self.source_ref != "authorized_peer_teams"
                || self.membership_ref.is_some())
        {
            return Err(ValidationError::Invalid {
                field: "MessageSubscription.subscriber_kind",
                reason: "Team subject requires an unresolved all-authorized Team inbox",
            });
        }
        require_non_empty(&self.created_by.id, "MessageSubscription.created_by.id")?;
        require_non_empty(&self.created_at, "MessageSubscription.created_at")
    }
}

impl Validate for agentfirm_api::SubscriptionCursor {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.subscription_id, "SubscriptionCursor.subscription_id")?;
        require_non_empty(
            &self.recipient_agent_member_id,
            "SubscriptionCursor.recipient_agent_member_id",
        )?;
        if self.cursor_revision == 0 {
            return Err(ValidationError::Invalid {
                field: "SubscriptionCursor.cursor_revision",
                reason: "must be at least 1",
            });
        }
        require_non_empty(&self.updated_at, "SubscriptionCursor.updated_at")
    }
}

impl Validate for agentfirm_api::CanonicalMessageDelivery {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "CanonicalMessageDelivery.id")?;
        require_non_empty(&self.message_id, "CanonicalMessageDelivery.message_id")?;
        require_non_empty(
            &self.subscription_id,
            "CanonicalMessageDelivery.subscription_id",
        )?;
        require_non_empty(
            &self.subscription_policy_digest,
            "CanonicalMessageDelivery.subscription_policy_digest",
        )?;
        require_non_empty(
            &self.recipient_ref,
            "CanonicalMessageDelivery.recipient_ref",
        )?;
        require_uuid(
            &self.target_node_id,
            "CanonicalMessageDelivery.target_node_id",
        )?;
        if self.subscription_revision == 0 || self.attempt == 0 || self.version == 0 {
            return Err(ValidationError::Invalid {
                field: "CanonicalMessageDelivery.version",
                reason: "attempt and version must be at least 1",
            });
        }
        match self.recipient_kind {
            agentfirm_api::MessageSubjectKind::AgentMember
                if self
                    .recipient_agent_member_id
                    .as_deref()
                    .is_none_or(str::is_empty) =>
            {
                return Err(ValidationError::Invalid {
                    field: "CanonicalMessageDelivery.recipient_agent_member_id",
                    reason: "AgentMember delivery requires its durable member identity",
                });
            }
            agentfirm_api::MessageSubjectKind::Team
                if self.target_team_id.as_deref().is_none_or(str::is_empty) =>
            {
                return Err(ValidationError::Invalid {
                    field: "CanonicalMessageDelivery.target_team_id",
                    reason: "Team delivery requires its target Team",
                });
            }
            _ => {}
        }
        if self.recipient_kind == agentfirm_api::MessageSubjectKind::Team
            && matches!(
                self.status,
                agentfirm_api::CanonicalMessageDeliveryStatus::Routed
                    | agentfirm_api::CanonicalMessageDeliveryStatus::Claimed
                    | agentfirm_api::CanonicalMessageDeliveryStatus::ProviderReceived
                    | agentfirm_api::CanonicalMessageDeliveryStatus::Acknowledged
            )
            && (self
                .resolved_team_membership_id
                .as_deref()
                .is_none_or(str::is_empty)
                || self
                    .recipient_agent_member_id
                    .as_deref()
                    .is_none_or(str::is_empty)
                || self.claim_id.as_deref().is_none_or(str::is_empty)
                || self
                    .claimed_node_daemon_generation
                    .is_none_or(|value| value == 0))
        {
            return Err(ValidationError::Invalid {
                field: "CanonicalMessageDelivery.claim_id",
                reason: "routed Team delivery requires an exact membership/daemon claim",
            });
        }
        require_non_empty(&self.created_at, "CanonicalMessageDelivery.created_at")?;
        require_non_empty(&self.updated_at, "CanonicalMessageDelivery.updated_at")
    }
}

impl Validate for agentfirm_api::TeamMessageDeliveryClaim {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.claim_id, "TeamMessageDeliveryClaim.claim_id")?;
        require_non_empty(
            &self.team_membership_id,
            "TeamMessageDeliveryClaim.team_membership_id",
        )?;
        if self.membership_generation == 0 || self.node_daemon_generation == 0 {
            return Err(ValidationError::Invalid {
                field: "TeamMessageDeliveryClaim.generation",
                reason: "membership and daemon generations must be at least 1",
            });
        }
        require_non_empty(
            &self.claim_expires_at,
            "TeamMessageDeliveryClaim.claim_expires_at",
        )
    }
}

impl Validate for collaboration::MessageAdmissionAuthority {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            collaboration::MessageAdmissionAuthority::PeerTeam(authority) => {
                for (value, field) in [
                    (
                        &authority.company_id,
                        "PeerTeamMessageAdmissionAuthority.company_id",
                    ),
                    (
                        &authority.source_execution_space_id,
                        "PeerTeamMessageAdmissionAuthority.source_execution_space_id",
                    ),
                    (
                        &authority.source_team_id,
                        "PeerTeamMessageAdmissionAuthority.source_team_id",
                    ),
                    (
                        &authority.source_membership_id,
                        "PeerTeamMessageAdmissionAuthority.source_membership_id",
                    ),
                    (
                        &authority.source_agent_member_id,
                        "PeerTeamMessageAdmissionAuthority.source_agent_member_id",
                    ),
                    (
                        &authority.source_session_id,
                        "PeerTeamMessageAdmissionAuthority.source_session_id",
                    ),
                    (
                        &authority.target_execution_space_id,
                        "PeerTeamMessageAdmissionAuthority.target_execution_space_id",
                    ),
                    (
                        &authority.target_team_id,
                        "PeerTeamMessageAdmissionAuthority.target_team_id",
                    ),
                    (
                        &authority.target_subscription_id,
                        "PeerTeamMessageAdmissionAuthority.target_subscription_id",
                    ),
                    (
                        &authority.authority_digest,
                        "PeerTeamMessageAdmissionAuthority.authority_digest",
                    ),
                ] {
                    require_non_empty(value, field)?;
                }
                require_uuid(
                    &authority.source_node_id,
                    "PeerTeamMessageAdmissionAuthority.source_node_id",
                )?;
                require_uuid(
                    &authority.target_node_id,
                    "PeerTeamMessageAdmissionAuthority.target_node_id",
                )?;
                let member_target = authority.target_membership_id.is_some()
                    || authority.target_membership_generation.is_some()
                    || authority.target_agent_member_id.is_some();
                let expected_policy_ref = if member_target {
                    "team.direct.active-members"
                } else {
                    "collaboration.peer_message_deliver"
                };
                if authority.source_required_capability != "message.peer_team.author"
                    || authority.target_required_capability != "collaboration.peer_message_deliver"
                    || authority.target_authorization_policy_ref != expected_policy_ref
                {
                    return Err(ValidationError::Invalid {
                        field: "PeerTeamMessageAdmissionAuthority.capability",
                        reason:
                            "source authoring and target delivery capabilities must remain distinct",
                    });
                }
                if member_target
                    && (authority
                        .target_membership_id
                        .as_deref()
                        .is_none_or(str::is_empty)
                        || authority
                            .target_agent_member_id
                            .as_deref()
                            .is_none_or(str::is_empty)
                        || authority.target_membership_generation.is_none())
                {
                    return Err(ValidationError::Invalid {
                        field: "PeerTeamMessageAdmissionAuthority.target_membership",
                        reason:
                            "a direct TeamMembership target sets the membership id, generation, and AgentMember together",
                    });
                }
                if [
                    authority.source_team_revision,
                    authority.source_membership_generation,
                    authority.source_session_generation,
                    authority.source_node_daemon_generation,
                    authority.target_team_revision,
                    authority.source_policy_revision,
                    authority.target_subscription_revision,
                    authority.target_policy_revision,
                ]
                .contains(&0)
                    || authority.target_membership_generation == Some(0)
                {
                    return Err(ValidationError::Invalid {
                        field: "PeerTeamMessageAdmissionAuthority.generation",
                        reason: "all revisions and generations must be at least 1",
                    });
                }
                Ok(())
            }
            collaboration::MessageAdmissionAuthority::WorkDelegation(authority) => {
                require_non_empty(
                    &authority.authority_digest,
                    "CollaborationMessageAuthority.authority_digest",
                )
            }
        }
    }
}

impl Validate for RegistryMessage {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "RegistryMessage.id")?;
        require_non_empty(&self.from_agent_id, "RegistryMessage.from_agent_id")?;
        require_non_empty(&self.content, "RegistryMessage.content")?;
        require_non_empty(&self.created_at, "RegistryMessage.created_at")
    }
}

impl Validate for ProviderProcess {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderProcess.id")?;
        require_non_empty(&self.agent_member_id, "ProviderProcess.agent_member_id")?;
        require_non_empty(&self.provider, "ProviderProcess.provider")?;
        require_non_empty(&self.command, "ProviderProcess.command")?;
        require_non_empty(&self.started_at, "ProviderProcess.started_at")
    }
}

impl Validate for ProviderDispatchEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderDispatchEvent.id")?;
        require_non_empty(
            &self.agent_member_id,
            "ProviderDispatchEvent.agent_member_id",
        )?;
        require_non_empty(&self.provider, "ProviderDispatchEvent.provider")?;
        require_non_empty(&self.event_type, "ProviderDispatchEvent.event_type")?;
        require_non_empty(&self.summary, "ProviderDispatchEvent.summary")?;
        require_non_empty(&self.created_at, "ProviderDispatchEvent.created_at")
    }
}

impl Validate for Proposal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Proposal.id")?;
        require_non_empty(&self.task_id, "Proposal.task_id")?;
        require_non_empty(&self.agent_member_id, "Proposal.agent_member_id")?;
        require_non_empty(&self.title, "Proposal.title")?;
        require_non_empty(&self.summary, "Proposal.summary")?;
        require_non_empty(&self.created_at, "Proposal.created_at")?;
        require_non_empty(&self.updated_at, "Proposal.updated_at")
    }
}

impl Validate for Evidence {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Evidence.id")?;
        require_non_empty(&self.source_type, "Evidence.source_type")?;
        require_non_empty(&self.source_ref, "Evidence.source_ref")?;
        require_non_empty(&self.summary, "Evidence.summary")?;
        require_non_empty(&self.created_at, "Evidence.created_at")
    }
}

impl Validate for Decision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Decision.id")?;
        require_non_empty(&self.task_id, "Decision.task_id")?;
        require_non_empty(&self.decision, "Decision.decision")?;
        require_non_empty(&self.rationale, "Decision.rationale")?;
        require_non_empty(&self.created_at, "Decision.created_at")
    }
}

impl Validate for Review {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Review.id")?;
        require_non_empty(&self.reviewer_agent_id, "Review.reviewer_agent_id")?;
        require_non_empty(&self.review_kind, "Review.review_kind")?;
        require_non_empty(self.verdict.as_str(), "Review.verdict")?;
        require_non_empty(&self.summary, "Review.summary")?;
        require_non_empty(&self.created_at, "Review.created_at")?;
        for blocker in &self.blockers {
            if blocker.is_empty() {
                return Err(ValidationError::Required {
                    field: "Review.blockers[]",
                });
            }
        }
        for item in &self.missing_validation {
            if item.is_empty() {
                return Err(ValidationError::Required {
                    field: "Review.missing_validation[]",
                });
            }
        }
        for evidence_id in &self.evidence_ids {
            if evidence_id.is_empty() {
                return Err(ValidationError::Required {
                    field: "Review.evidence_ids[]",
                });
            }
        }
        for (field, actor) in [
            (
                "Review.performed_by_actor",
                self.performed_by_actor.as_ref(),
            ),
            ("Review.authority_actor", self.authority_actor.as_ref()),
        ] {
            if let Some(actor) = actor {
                require_non_empty(&actor.id, field)?;
                validate_actor_metadata(actor, field)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_actor_metadata(
    actor: &TeamActorRef,
    field: &'static str,
) -> Result<(), ValidationError> {
    if actor.display_name.as_deref().is_some_and(str::is_empty)
        || actor.authn_source.as_deref().is_some_and(str::is_empty)
    {
        return Err(ValidationError::Invalid {
            field,
            reason: "display_name and authn_source must not be empty when present",
        });
    }
    Ok(())
}

impl Validate for Gap {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Gap.id")?;
        require_non_empty(&self.category, "Gap.category")?;
        require_non_empty(&self.summary, "Gap.summary")?;
        require_non_empty(&self.created_at, "Gap.created_at")?;
        require_non_empty(&self.updated_at, "Gap.updated_at")
    }
}

impl Validate for Vision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Vision.id")?;
        require_non_empty(&self.summary, "Vision.summary")?;
        require_non_empty(&self.created_at, "Vision.created_at")
    }
}

impl Validate for Mission {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Mission.id")?;
        require_non_empty(&self.title, "Mission.title")?;
        require_non_empty(&self.objective, "Mission.objective")?;
        validate_non_empty_unique_strings(&self.legacy_wave_ids, "Mission.legacy_wave_ids", true)?;
        for (value, field) in [
            (self.desired_outcome.as_deref(), "Mission.desired_outcome"),
            (self.outcome_summary.as_deref(), "Mission.outcome_summary"),
            (self.completed_by.as_deref(), "Mission.completed_by"),
            (self.completed_at.as_deref(), "Mission.completed_at"),
        ] {
            if let Some(value) = value {
                require_non_empty(value, field)?;
            }
        }
        require_non_empty(&self.created_at, "Mission.created_at")?;
        require_non_empty(&self.updated_at, "Mission.updated_at")
    }
}

impl Validate for LegacyWave {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "LegacyWave.id")?;
        require_non_empty(&self.mission_id, "LegacyWave.mission_id")?;
        require_non_empty(&self.title, "LegacyWave.title")?;
        require_non_empty(&self.objective, "LegacyWave.objective")?;
        require_non_empty(&self.created_at, "LegacyWave.created_at")?;
        require_non_empty(&self.updated_at, "LegacyWave.updated_at")
    }
}

// ---------------------------------------------------------------------------
// Mission Log (ADR 0051)
//
// Mission replaces the retired Wave lifecycle with an append-only Mission Log.
