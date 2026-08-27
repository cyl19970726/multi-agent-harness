use super::*;

impl HarnessStore {
    pub fn fabric_messages(&self, execution_space_id: &str) -> StoreResult<Vec<Message>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "message")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn fabric_message_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalMessageDelivery>> {
        Ok(self
            .latest_fabric_side_records_unlocked(
                execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .into_values()
            .collect())
    }

    pub fn author_message(
        &self,
        context: &MutationContext,
        message: Message,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        self.author_message_with_admission_authority(context, message, None)
    }

    /// Compatibility entry point for persisted pre-DEV-35 daemon payloads.
    /// New callers serialize [`MessageAdmissionAuthority`] explicitly.
    pub fn author_message_with_collaboration_authority(
        &self,
        context: &MutationContext,
        message: Message,
        collaboration_authority: Option<&CollaborationMessageAuthority>,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        let authority = collaboration_authority
            .cloned()
            .map(MessageAdmissionAuthority::WorkDelegation);
        self.author_message_with_admission_authority(context, message, authority.as_ref())
    }

    pub fn author_message_with_admission_authority(
        &self,
        context: &MutationContext,
        message: Message,
        admission_authority: Option<&MessageAdmissionAuthority>,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&message.id, "Message.id")?;
        required(&message.sender_actor_ref.id, "Message.sender_actor_ref.id")?;
        required(&message.body, "Message.body")?;
        if message.source_execution_space_id != context.execution_space_id
            || message.recipients.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Message must have recipients in the authenticated Execution Space",
                "message",
                &message.id,
                None,
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &message.source_node_id,
            &message.source_node_daemon_id,
            message.source_authority_generation,
            &context.authenticated_actor,
            "message",
            &message.id,
        )?;
        if let Some(sender_agent_member_id) = message.sender_agent_member_id.as_deref() {
            let sender_sessions = self
                .fabric_agent_sessions(&context.execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.agent_member_id == sender_agent_member_id
                        && session.node_id == message.source_node_id
                        && session.node_daemon_generation == message.source_authority_generation
                        && session.lifecycle != AgentSessionStatus::Closed
                        && message.sender_session_id.as_deref() == Some(session.id.as_str())
                })
                .count();
            if sender_sessions != 1 || message.sender_actor_ref.id != sender_agent_member_id {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "Agent Message author must resolve to the exact current local AgentSession",
                    "message",
                    &message.id,
                    None,
                ));
            }
        } else if context.authority_actor.as_ref() != Some(&message.sender_actor_ref) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "Human/Service Message actor must be server-resolved as command authority",
                "message",
                &message.id,
                None,
            ));
        }
        let expected_fingerprint = message_content_fingerprint(&message);
        if message.content_fingerprint != expected_fingerprint {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Message content_fingerprint does not match immutable authored content",
                "message",
                &message.id,
                None,
            ));
        }
        crate::validate_message_collaboration_scope(&message)?;
        let subscriptions = self.fabric_message_subscriptions(&context.execution_space_id)?;
        let sessions = self.fabric_agent_sessions(&context.execution_space_id)?;
        let memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        let collaboration_authority = match admission_authority {
            Some(MessageAdmissionAuthority::WorkDelegation(authority)) => Some(authority),
            _ => None,
        };
        let peer_authority = match admission_authority {
            Some(MessageAdmissionAuthority::PeerTeam(authority)) => Some(authority),
            _ => None,
        };
        if let Some(authority) = peer_authority {
            self.validate_peer_team_message_admission_unlocked(
                context,
                &message,
                authority,
                &sessions,
                &memberships,
            )?;
        }
        if let Some(authority) = collaboration_authority {
            let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "cross-Team Message lacks frozen CollaborationScope",
                    "message",
                    &message.id,
                    None,
                )
            })?;
            let expected_authority_digest = canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
            let source_work = self
                .latest_works()?
                .into_iter()
                .find(|work| work.id == authority.source_work_ref.work_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Delegation source Work is not current in the source Execution Space",
                        "message",
                        &message.id,
                        None,
                    )
                })?;
            let source_team_revision = self
                .teams()?
                .iter()
                .filter(|team| team.id == authority.source_work_ref.team_id)
                .count() as u64;
            let exact_source_scope = authority.authority_digest == expected_authority_digest
                && authority.delegation_revision > 0
                && scope.delegation_id.as_deref() == Some(authority.delegation_id.as_str())
                && scope.expected_delegation_revision == Some(authority.delegation_revision)
                && scope.source_work_ref.as_ref() == Some(&authority.source_work_ref)
                && scope.target_work_ref.as_ref() == Some(&authority.target_work_ref)
                && scope.source_team_id == authority.source_work_ref.team_id
                && scope.target_team_id == authority.target_placement.team_id
                && message.team_id.as_deref() == Some(authority.source_work_ref.team_id.as_str())
                && message.work_id.as_deref() == Some(authority.source_work_ref.work_id.as_str())
                && authority.source_work_ref.execution_space_id == context.execution_space_id
                && authority.source_work_ref.node_id == message.source_node_id
                && authority.source_work_ref.placement_generation == 1
                && authority.source_work_ref.team_revision == source_team_revision
                && source_work.id == authority.source_work_ref.work_id
                && source_work.accountable_team_id.as_deref()
                    == Some(authority.source_work_ref.team_id.as_str())
                && source_work.version == authority.source_work_ref.work_revision;
            let current_owner_bindings = self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .into_iter()
                .filter(|binding| {
                    binding.work_id == source_work.id
                        && binding.work_revision == source_work.version
                        && binding.team_id == authority.source_work_ref.team_id
                        && binding.agent_member_id == authority.source_owner_ref.id
                        && sessions.iter().any(|session| {
                            session.id == binding.agent_session_id
                                && session.runtime_generation == binding.agent_session_generation
                                && session.node_daemon_generation
                                    == message.source_authority_generation
                                && session.lifecycle != AgentSessionStatus::Closed
                        })
                        && binding.status == WorkExecutionBindingStatus::Active
                })
                .collect::<Vec<_>>();
            let exact_owner_binding = message.sender_actor_ref == authority.source_owner_ref
                && message.sender_agent_member_id.as_deref()
                    == Some(authority.source_owner_ref.id.as_str())
                && current_owner_bindings.len() == 1
                && message.sender_session_id.as_deref()
                    == Some(current_owner_bindings[0].agent_session_id.as_str());
            let exact_source_host = message.sender_actor_ref == authority.source_host_ref
                && message.sender_agent_member_id.as_deref()
                    == Some(authority.source_host_ref.id.as_str())
                && memberships
                    .iter()
                    .filter(|membership| {
                        membership.team_id == authority.source_work_ref.team_id
                            && membership.agent_member_id == authority.source_host_ref.id
                            && membership.role == TeamMembershipRole::Host
                            && membership.state == TeamMembershipStatus::Active
                    })
                    .count()
                    == 1
                && current_owner_bindings.len() == 1;
            if !exact_source_scope || (!exact_owner_binding && !exact_source_host) {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "cross-Team Message requires exact current Delegation, source Work, and source owner binding or Host membership",
                    "message",
                    &message.id,
                    Some(source_work.version),
                ));
            }
        } else if message.collaboration_scope.is_some() && peer_authority.is_none() {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "cross-Team Message authoring requires server-frozen Delegation authority",
                "message",
                &message.id,
                None,
            ));
        }
        if let Some(team_id) = message.team_id.as_deref() {
            let sender_is_member =
                message
                    .sender_agent_member_id
                    .as_deref()
                    .is_some_and(|sender| {
                        memberships.iter().any(|membership| {
                            membership.team_id == team_id
                                && membership.agent_member_id == sender
                                && membership.state == TeamMembershipStatus::Active
                        })
                    });
            let control_plane_sender = message.sender_agent_member_id.is_none()
                && context.authority_actor.as_ref() == Some(&message.sender_actor_ref);
            if !sender_is_member && !control_plane_sender {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "Message sender is not an active member or server-resolved control-plane actor for the Team",
                    "message",
                    &message.id,
                    None,
                ));
            }
        }
        if admission_authority.is_none() {
            if let Some(work_id) = message.work_id.as_deref() {
                let work = self
                    .latest_works_unlocked()?
                    .remove(work_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::WorkRevisionStale,
                            "Work-linked Message references a missing current Work",
                            "message",
                            &message.id,
                            None,
                        )
                    })?;
                if work.team_run_id != message.team_run_id.as_deref().unwrap_or_default()
                    || work.accountable_team_id.as_deref() != message.team_id.as_deref()
                {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Work-linked Message must name current Work in the exact Team and TeamRun",
                        "message",
                        &message.id,
                        Some(work.version),
                    ));
                }
                if let Some(sender_id) = message.sender_agent_member_id.as_deref() {
                    // A Work link supplies immutable conversation context. It
                    // must fence the sender to this TeamRun, but it must not
                    // borrow the Work owner's mutation binding or delivery.
                    let active_sender_runs = self
                        .trust_member_runs(&context.execution_space_id)?
                        .into_iter()
                        .filter(|run| {
                            run.agent_member_id == sender_id
                                && run.team_run_id == work.team_run_id
                                && run.has_live_runtime_authority()
                        })
                        .count();
                    if active_sender_runs != 1 {
                        return Err(trust_error(
                            TrustErrorCode::MemberRunGenerationFenced,
                            "Work-linked Message requires exactly one current active sender MemberRun in the addressed TeamRun",
                            "message",
                            &message.id,
                            Some(work.version),
                        ));
                    }
                } else {
                    let exact_host = self.exact_team_run_host_actor(&work.team_run_id)?;
                    if exact_host.id != message.sender_actor_ref.id {
                        return Err(trust_error(
                            TrustErrorCode::UnauthorizedActor,
                            "control-plane Work-linked Message requires the exact TeamRun Host authority",
                            "message",
                            &message.id,
                            Some(work.version),
                        ));
                    }
                }
            }
        }
        let mut delivery_rows = Vec::new();
        let mut delivered_subjects = BTreeSet::new();
        // A peer-Team direct Message binds the recipient membership in the
        // collaboration target Team, not the source Team (the author's scope).
        // Same-Space peer authoring resolves the target direct subscription in
        // this store; a remote target leaves delivery creation to its own Node.
        let peer_target_team_id = peer_authority.map(|authority| authority.target_team_id.as_str());
        for recipient in &message.recipients {
            let matching = subscriptions.iter().filter(|subscription| {
                subscription.status == MessageSubscriptionStatus::Active
                    && match recipient.kind {
                        MessageRecipientKind::AgentMember => {
                            subscription.subscriber_kind == MessageSubjectKind::AgentMember
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind == MessageSubscriptionKind::Agent
                                && if let Some(team_id) = message.team_id.as_deref() {
                                    subscription.membership_ref.as_deref().is_some_and(
                                        |membership_id| {
                                            memberships.iter().any(|membership| {
                                                membership.id == membership_id
                                                    && membership.state
                                                        == TeamMembershipStatus::Active
                                                    && membership.team_id
                                                        == peer_target_team_id.unwrap_or(team_id)
                                            })
                                        },
                                    )
                                } else {
                                    subscription.membership_ref.is_none()
                                        && message.sender_agent_member_id.as_deref()
                                            == Some(subscription.source_ref.as_str())
                                }
                        }
                        MessageRecipientKind::Team => {
                            subscription.subscriber_kind == MessageSubjectKind::Team
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind
                                    == MessageSubscriptionKind::AllAuthorized
                                && subscription.source_ref == "authorized_peer_teams"
                                && subscription.target_team_id.as_deref()
                                    == Some(recipient.id.as_str())
                        }
                        MessageRecipientKind::ControlPlaneActor => false,
                    }
            });
            for subscription in matching {
                let subject_key = (
                    subscription.subscriber_kind,
                    subscription.subscriber_ref.clone(),
                );
                if !delivered_subjects.insert(subject_key) {
                    continue;
                }
                let resolved_team_membership_id = (subscription.subscriber_kind
                    == MessageSubjectKind::AgentMember)
                    .then(|| subscription.membership_ref.clone())
                    .flatten();
                let recipient_agent_member_id = (subscription.subscriber_kind
                    == MessageSubjectKind::AgentMember)
                    .then(|| subscription.subscriber_ref.clone());
                delivery_rows.push(CanonicalMessageDelivery {
                    id: format!("{}:{}", message.id, subscription.id),
                    message_id: message.id.clone(),
                    subscription_id: subscription.id.clone(),
                    subscription_revision: subscription.revision,
                    subscription_policy_digest: subscription.policy_digest.clone(),
                    recipient_kind: subscription.subscriber_kind,
                    recipient_ref: subscription.subscriber_ref.clone(),
                    target_team_id: subscription.target_team_id.clone(),
                    target_node_id: subscription.target_node_id.clone(),
                    resolved_team_membership_id,
                    recipient_agent_member_id,
                    recipient_session_id: None,
                    recipient_session_generation: None,
                    status: CanonicalMessageDeliveryStatus::Queued,
                    attempt: 1,
                    claim_id: None,
                    claimed_node_daemon_generation: None,
                    provider_receipt_id: None,
                    failure_code: None,
                    failure_detail: None,
                    version: 1,
                    created_at: message.created_at.clone(),
                    updated_at: message.created_at.clone(),
                });
            }
        }
        let cross_node_collaboration = message
            .collaboration_scope
            .as_ref()
            .is_some_and(|scope| scope.source_team_id != scope.target_team_id);
        if delivery_rows.is_empty()
            && !cross_node_collaboration
            && !message
                .recipients
                .iter()
                .all(|recipient| recipient.kind == MessageRecipientKind::ControlPlaneActor)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Message recipients resolved to no active subscription",
                "message",
                &message.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "message",
            &message.id,
            "authored",
            serde_json::to_value(&message)?,
            &message,
            Vec::new(),
            delivery_rows
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    pub(super) fn validate_peer_team_message_admission_unlocked(
        &self,
        context: &MutationContext,
        message: &Message,
        authority: &PeerTeamMessageAdmissionAuthority,
        sessions: &[AgentSession],
        memberships: &[TeamMembership],
    ) -> StoreResult<()> {
        let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team Message lacks frozen CollaborationScope",
                "message",
                &message.id,
                None,
            )
        })?;
        let expected_source_policy_digest = peer_team_source_policy_digest(authority);
        let expected_target_policy_digest = peer_team_target_policy_digest(authority);
        let expected_authority_digest = peer_team_message_authority_digest(authority);
        let source_teams = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .filter(|team| team.id == authority.source_team_id)
            .collect::<Vec<_>>();
        if source_teams.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team source Team is missing or ambiguous",
                "message",
                &message.id,
                None,
            ));
        }
        let source_team = &source_teams[0];
        let exact_membership = memberships
            .iter()
            .filter(|membership| {
                membership.id == authority.source_membership_id
                    && membership.team_id == authority.source_team_id
                    && membership.agent_member_id == authority.source_agent_member_id
                    && membership.node_id == authority.source_node_id
                    && membership.membership_generation == authority.source_membership_generation
                    && membership.state == TeamMembershipStatus::Active
            })
            .count()
            == 1;
        let exact_session = sessions
            .iter()
            .filter(|session| {
                session.id == authority.source_session_id
                    && session.agent_member_id == authority.source_agent_member_id
                    && session.node_id == authority.source_node_id
                    && session.node_daemon_id == authority.source_node_daemon_id
                    && session.node_daemon_generation == authority.source_node_daemon_generation
                    && session.runtime_generation == authority.source_session_generation
                    && session.lifecycle != AgentSessionStatus::Closed
            })
            .count()
            == 1;
        let exact_active_member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .filter(|member| {
                member.id == authority.source_agent_member_id
                    && member.organization_status == AgentMemberOrganizationStatus::Active
            })
            .count()
            == 1;
        let member_target = authority.target_membership_id.is_some()
            || authority.target_membership_generation.is_some()
            || authority.target_agent_member_id.is_some();
        let member_target_complete = authority.target_membership_id.is_some()
            && authority.target_membership_generation.is_some()
            && authority.target_agent_member_id.is_some();
        let exact_team_recipient = message.recipients.len() == 1
            && message.recipients[0].kind == MessageRecipientKind::Team
            && message.recipients[0].id == authority.target_team_id
            && message.target_ref == message.recipients[0];
        let exact_member_recipient = message.recipients.len() == 1
            && message.recipients[0].kind == MessageRecipientKind::AgentMember
            && Some(message.recipients[0].id.as_str())
                == authority.target_agent_member_id.as_deref()
            && message.target_ref == message.recipients[0];
        let exact_target_subscription = if member_target {
            member_target_complete
                && authority.target_membership_generation != Some(0)
                && authority.target_authorization_policy_ref == "team.direct.active-members"
                && authority.target_subscription_id
                    == format!(
                        "direct:{}:{}",
                        authority
                            .target_agent_member_id
                            .as_deref()
                            .unwrap_or_default(),
                        authority
                            .target_membership_id
                            .as_deref()
                            .unwrap_or_default()
                    )
                && exact_member_recipient
        } else {
            authority.target_authorization_policy_ref == "collaboration.peer_message_deliver"
                && authority.target_subscription_id
                    == format!("team-inbox:{}", authority.target_team_id)
                && exact_team_recipient
        };
        if authority.source_required_capability != "message.peer_team.author"
            || authority.target_required_capability != "collaboration.peer_message_deliver"
            || authority.target_subscription_revision == 0
            || authority.source_policy_revision == 0
            || authority.target_policy_revision == 0
            || authority.source_membership_generation == 0
            || authority.source_session_generation == 0
            || authority.source_node_daemon_generation == 0
            || authority.target_team_revision == 0
            || authority.source_policy_digest != expected_source_policy_digest
            || authority.target_policy_digest != expected_target_policy_digest
            || authority.authority_digest != expected_authority_digest
            || authority.source_execution_space_id != context.execution_space_id
            || authority.source_execution_space_id != message.source_execution_space_id
            || authority.source_team_revision != source_team.revision
            || source_team.status != AgentTeamStatus::Active
            || source_team.node_id != authority.source_node_id
            || message.source_node_id != authority.source_node_id
            || message.source_node_daemon_id != authority.source_node_daemon_id
            || message.source_authority_generation != authority.source_node_daemon_generation
            || message.sender_agent_member_id.as_deref()
                != Some(authority.source_agent_member_id.as_str())
            || message.sender_session_id.as_deref() != Some(authority.source_session_id.as_str())
            || message.sender_actor_ref.kind != ActorKind::AgentMember
            || message.sender_actor_ref.id != authority.source_agent_member_id
            || message.team_id.as_deref() != Some(authority.source_team_id.as_str())
            || scope.source_team_id != authority.source_team_id
            || scope.target_team_id != authority.target_team_id
            || scope.delegation_id.is_some()
            || scope.expected_delegation_revision.is_some()
            || scope.source_work_ref.is_some()
            || scope.target_work_ref.is_some()
            || !exact_target_subscription
            || !exact_membership
            || !exact_session
            || !exact_active_member
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team Message is outside the exact membership, session, daemon, policy, capability, or target placement authority",
                "message",
                &message.id,
                None,
            ));
        }
        // A Message Work link is context only (DOC-106): it cannot assign,
        // accept, close, or transfer Work. When present it must resolve to a
        // current Work accountable to the source Team so the author cannot
        // invent cross-Team provenance.
        if let Some(work_id) = message.work_id.as_deref() {
            let work_matches = self.latest_works()?.into_iter().any(|work| {
                work.id == work_id
                    && work.accountable_team_id.as_deref()
                        == Some(authority.source_team_id.as_str())
            });
            if !work_matches {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "peer-Team Message Work link must name a current Work of the source Team",
                    "message",
                    &message.id,
                    None,
                ));
            }
        }
        // When source and target share this Execution Space, the target fence
        // is revalidated against the same durable store at author time; a
        // genuinely remote target revalidates on its own Node before any
        // delivery mutation.
        if authority.target_execution_space_id == context.execution_space_id {
            self.revalidate_peer_team_delivery_subscription(
                &context.execution_space_id,
                authority,
            )?;
        }
        Ok(())
    }

    /// Revalidate the target half of a frozen peer-Team authority against the
    /// durable target subscription. A Team target revalidates the shared
    /// `team-inbox:` Team-subject subscription; a direct TeamMembership target
    /// revalidates that membership's durable `direct:` subscription plus the
    /// exact membership generation. This grants target delivery only;
    /// callers must separately prove source admission before route creation.
    pub fn revalidate_peer_team_delivery_subscription(
        &self,
        execution_space_id: &str,
        authority: &PeerTeamMessageAdmissionAuthority,
    ) -> StoreResult<MessageSubscription> {
        let member_target = authority.target_membership_id.is_some()
            || authority.target_membership_generation.is_some()
            || authority.target_agent_member_id.is_some();
        let expected_policy_ref = if member_target {
            "team.direct.active-members"
        } else {
            "collaboration.peer_message_deliver"
        };
        if authority.target_required_capability != "collaboration.peer_message_deliver"
            || authority.target_authorization_policy_ref != expected_policy_ref
            || authority.target_execution_space_id != execution_space_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team authority does not carry the target delivery capability",
                "message_subscription",
                &authority.target_subscription_id,
                None,
            ));
        }
        let teams = self
            .agent_teams(execution_space_id)?
            .into_iter()
            .filter(|team| team.id == authority.target_team_id)
            .collect::<Vec<_>>();
        let subscriptions = self
            .fabric_message_subscriptions(execution_space_id)?
            .into_iter()
            .filter(|subscription| subscription.id == authority.target_subscription_id)
            .collect::<Vec<_>>();
        if teams.len() != 1 || subscriptions.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team target Team or durable subscription is missing or ambiguous",
                "message_subscription",
                &authority.target_subscription_id,
                None,
            ));
        }
        let team = &teams[0];
        let subscription = &subscriptions[0];
        let expected_policy_digest = peer_team_target_policy_digest(authority);
        let subscription_matches = if member_target {
            subscription.subscriber_kind == MessageSubjectKind::AgentMember
                && Some(subscription.subscriber_ref.as_str())
                    == authority.target_agent_member_id.as_deref()
                && subscription.membership_ref.as_deref()
                    == authority.target_membership_id.as_deref()
                && subscription.source_kind == MessageSubscriptionKind::Agent
                && subscription.source_ref == "active_team_members"
        } else {
            subscription.subscriber_kind == MessageSubjectKind::Team
                && subscription.subscriber_ref == authority.target_team_id
                && subscription.source_kind == MessageSubscriptionKind::AllAuthorized
                && subscription.source_ref == "authorized_peer_teams"
                && subscription.membership_ref.is_none()
        };
        if team.status != AgentTeamStatus::Active
            || team.node_id != authority.target_node_id
            || team.revision != authority.target_team_revision
            || !subscription_matches
            || subscription.target_team_id.as_deref() != Some(authority.target_team_id.as_str())
            || subscription.target_node_id != authority.target_node_id
            || subscription.status != MessageSubscriptionStatus::Active
            || subscription.revision != authority.target_subscription_revision
            || subscription.authorization_policy_ref != authority.target_authorization_policy_ref
            || subscription.policy_revision != authority.target_policy_revision
            || subscription.policy_digest != authority.target_policy_digest
            || authority.target_policy_digest != expected_policy_digest
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team target delivery rejected stale or cross-wired Team/subscription policy authority",
                "message_subscription",
                &authority.target_subscription_id,
                Some(subscription.revision),
            ));
        }
        if member_target {
            let members = self
                .trust_agent_members(execution_space_id)?
                .into_iter()
                .filter(|member| {
                    Some(member.id.as_str()) == authority.target_agent_member_id.as_deref()
                        && member.organization_status == AgentMemberOrganizationStatus::Active
                })
                .count();
            let memberships = self
                .fabric_team_memberships(execution_space_id)?
                .into_iter()
                .filter(|membership| {
                    Some(membership.id.as_str()) == authority.target_membership_id.as_deref()
                        && membership.team_id == authority.target_team_id
                        && membership.node_id == authority.target_node_id
                        && Some(membership.agent_member_id.as_str())
                            == authority.target_agent_member_id.as_deref()
                        && Some(membership.membership_generation)
                            == authority.target_membership_generation
                        && membership.state == TeamMembershipStatus::Active
                })
                .count();
            if members != 1 || memberships != 1 {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "peer-Team direct delivery requires one exact active target TeamMembership generation",
                    "message_subscription",
                    &authority.target_subscription_id,
                    Some(subscription.revision),
                ));
            }
        }
        Ok(subscription.clone())
    }

    /// Persist an immutable source-authored cross-node Message before creating
    /// target-owned MessageDelivery rows. Fabric route journals remain the
    /// only cross-node route truth; this canonical operation records target
    /// application state and cannot re-author the Message.
    pub fn persist_remote_message(
        &self,
        context: &MutationContext,
        operation: &firm_fabric::RoutedOperation,
        message: Message,
        target_node_id: &str,
        target_daemon_id: &str,
        target_daemon_generation: u64,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            target_node_id,
            target_daemon_id,
            target_daemon_generation,
            &context.authenticated_actor,
            "message",
            &message.id,
        )?;
        let (reference, collaboration_authority, peer_authority) = match operation
            .closed_body()
            .map_err(|error| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    format!("Remote Message route is invalid: {error}"),
                    "message",
                    &message.id,
                    None,
                )
            })? {
            firm_fabric::ClosedOperationBody::Message(reference) => (reference, None, None),
            firm_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "peer_message_deliver"
                    && reference.required_capability == "collaboration.peer_message_deliver" =>
            {
                let message_reference = serde_json::from_value::<firm_fabric::MessageReference>(
                    reference
                        .payload
                        .get("message_reference")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::InvalidStateTransition,
                                "peer_message_deliver lacks server-frozen message_reference",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        format!("peer_message_deliver payload is not a MessageReference: {error}"),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                let admission_authority = serde_json::from_value::<MessageAdmissionAuthority>(
                    reference
                        .payload
                        .get("message_admission_authority")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::UnauthorizedActor,
                                "peer_message_deliver lacks canonical Message admission authority",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        format!(
                            "peer_message_deliver Message admission authority is invalid: {error}"
                        ),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                let MessageAdmissionAuthority::PeerTeam(authority) = admission_authority else {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "peer_message_deliver requires PeerTeam admission authority",
                        "message",
                        &message.id,
                        None,
                    ));
                };
                (message_reference, None, Some(authority))
            }
            firm_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "team_message_deliver"
                    && reference.required_capability == "collaboration.team_message_deliver" =>
            {
                let message_reference = serde_json::from_value::<firm_fabric::MessageReference>(
                    reference
                        .payload
                        .get("message_reference")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::InvalidStateTransition,
                                "team_message_deliver lacks server-frozen message_reference",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        format!("team_message_deliver payload is not a MessageReference: {error}"),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                let authority = serde_json::from_value::<CollaborationMessageAuthority>(
                    reference
                        .payload
                        .get("delegation_authority")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::UnauthorizedActor,
                                "team_message_deliver lacks central Delegation authority",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        format!("team_message_deliver Delegation authority is invalid: {error}"),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                (message_reference, Some(authority), None)
            }
            _ => {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Remote persistence requires a closed Message route",
                    "message",
                    &message.id,
                    None,
                ))
            }
        };
        if operation.target_node_id != target_node_id
            || operation.target_execution_space_id.as_deref()
                != Some(context.execution_space_id.as_str())
            || operation.source_execution_space_id.as_deref()
                != Some(message.source_execution_space_id.as_str())
            || operation.source_node_id.as_deref() != Some(message.source_node_id.as_str())
            || operation.source_node_daemon_id.as_deref()
                != Some(message.source_node_daemon_id.as_str())
            || operation.source_node_daemon_generation != Some(message.source_authority_generation)
            || reference.message_id != message.id
            || reference.body_digest != message.body_digest
            || reference.canonical_message_envelope.as_ref()
                != Some(&serde_json::to_value(&message)?)
            || message.body_digest
                != format!("sha256:{:x}", Sha256::digest(message.body.as_bytes()))
            || message.content_fingerprint != message_content_fingerprint(&message)
            || message.recipients.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "Remote Message or route disagrees with immutable source/target authority",
                "message",
                &message.id,
                None,
            ));
        }
        crate::validate_message_collaboration_scope(&message)?;
        if let Some(authority) = collaboration_authority.as_ref() {
            let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "cross-Team Message lacks CollaborationScope",
                    "message",
                    &message.id,
                    None,
                )
            })?;
            let expected_authority_digest = canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
            let target_work = self
                .latest_works()?
                .into_iter()
                .find(|work| work.id == authority.target_work_ref.work_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Delegation target Work is not present on the target Node",
                        "message",
                        &message.id,
                        None,
                    )
                })?;
            let target_teams = self.teams()?;
            let target_team_revision = target_teams
                .iter()
                .filter(|team| team.id == authority.target_placement.team_id)
                .count() as u64;
            let target_team = target_teams
                .into_iter()
                .rev()
                .find(|team| team.id == authority.target_placement.team_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Delegation target Team is not present on the target Node",
                        "message",
                        &message.id,
                        None,
                    )
                })?;
            if authority.authority_digest != expected_authority_digest
                || authority.delegation_revision == 0
                || scope.delegation_id.as_deref() != Some(authority.delegation_id.as_str())
                || scope.expected_delegation_revision != Some(authority.delegation_revision)
                || scope.source_work_ref.as_ref() != Some(&authority.source_work_ref)
                || scope.target_work_ref.as_ref() != Some(&authority.target_work_ref)
                || scope.source_team_id != authority.source_work_ref.team_id
                || scope.target_team_id != authority.target_placement.team_id
                || operation.expected_target_revision != Some(authority.delegation_revision)
                || operation.target_node_id != authority.target_placement.node_id
                || target_team.node_id != target_node_id
                || target_team_revision != authority.target_placement.team_revision
                || target_team.id != authority.target_work_ref.team_id
                || target_work.accountable_team_id.as_deref() != Some(target_team.id.as_str())
                || target_work.id != authority.target_work_ref.work_id
                || target_work.version != authority.target_work_ref.work_revision
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "target Message application rejected stale or widened Delegation/Work authority",
                    "message",
                    &message.id,
                    None,
                ));
            }
        } else if let Some(authority) = peer_authority.as_ref() {
            let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "peer-Team Message lacks CollaborationScope",
                    "message",
                    &message.id,
                    None,
                )
            })?;
            let exact_recipient = if authority.target_membership_id.is_some() {
                message.recipients.len() == 1
                    && message.recipients[0].kind == MessageRecipientKind::AgentMember
                    && Some(message.recipients[0].id.as_str())
                        == authority.target_agent_member_id.as_deref()
                    && message.target_ref == message.recipients[0]
            } else {
                message.recipients.len() == 1
                    && message.recipients[0].kind == MessageRecipientKind::Team
                    && message.recipients[0].id == authority.target_team_id
                    && message.target_ref == message.recipients[0]
            };
            if authority.authority_digest != peer_team_message_authority_digest(authority)
                || authority.source_policy_digest != peer_team_source_policy_digest(authority)
                || authority.target_policy_digest != peer_team_target_policy_digest(authority)
                || authority.company_id != operation.company_id
                || authority.source_required_capability != "message.peer_team.author"
                || authority.target_required_capability != "collaboration.peer_message_deliver"
                || authority.source_execution_space_id != message.source_execution_space_id
                || authority.source_team_id != scope.source_team_id
                || authority.source_team_revision == 0
                || authority.source_membership_generation == 0
                || authority.source_session_generation == 0
                || authority.source_agent_member_id != message.sender_actor_ref.id
                || message.sender_actor_ref.kind != ActorKind::AgentMember
                || message.sender_agent_member_id.as_deref()
                    != Some(authority.source_agent_member_id.as_str())
                || message.sender_session_id.as_deref()
                    != Some(authority.source_session_id.as_str())
                || message.team_id.as_deref() != Some(authority.source_team_id.as_str())
                || scope.target_team_id != authority.target_team_id
                || scope.delegation_id.is_some()
                || scope.expected_delegation_revision.is_some()
                || scope.source_work_ref.is_some()
                || scope.target_work_ref.is_some()
                || authority.source_node_id != message.source_node_id
                || authority.source_node_daemon_id != message.source_node_daemon_id
                || authority.source_node_daemon_generation != message.source_authority_generation
                || authority.target_execution_space_id != context.execution_space_id
                || authority.target_node_id != target_node_id
                || operation.actor.actor_kind != firm_fabric::ActorKind::Service
                || operation.actor.actor_id != authority.source_node_id
                || operation.actor.session_id
                    != format!(
                        "{}:{}",
                        authority.source_node_daemon_id, authority.source_node_daemon_generation
                    )
                || operation.source_gateway_generation.unwrap_or_default() == 0
                || operation
                    .authorization_context
                    .get("business_actor_kind")
                    .map(String::as_str)
                    != Some("agent_member")
                || operation.authorization_context.get("business_actor_id")
                    != Some(&authority.source_agent_member_id)
                || operation
                    .authorization_context
                    .get("business_actor_session_id")
                    != Some(&authority.source_session_id)
                || operation.actor_runtime_generation != Some(authority.source_session_generation)
                || operation.expected_target_revision
                    != Some(authority.target_subscription_revision)
                || operation.authorization_context.get("target_team_id")
                    != Some(&authority.target_team_id)
                || operation.authorization_context.get("target_team_revision")
                    != Some(&authority.target_team_revision.to_string())
                || operation.authorization_context.get("required_capability")
                    != Some(&authority.target_required_capability)
                || !exact_recipient
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "target Message application rejected widened, cross-wired, or stale peer-Team authority",
                    "message",
                    &message.id,
                    None,
                ));
            }
            self.revalidate_peer_team_delivery_subscription(
                &context.execution_space_id,
                authority,
            )?;
        } else if message.collaboration_scope.is_some() {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "cross-Team Message route requires server-frozen Message admission authority",
                "message",
                &message.id,
                None,
            ));
        }
        let request_fingerprint = match context.request_fingerprint.clone() {
            Some(fingerprint) => fingerprint,
            None => canonical_json_fingerprint(&serde_json::to_value(operation)?),
        };
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "message",
            &message.id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let subscriptions = self.fabric_message_subscriptions(&context.execution_space_id)?;
        let memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        let mut deliveries = if let Some(authority) = peer_authority.as_ref() {
            let subscription = subscriptions
                .iter()
                .find(|subscription| subscription.id == authority.target_subscription_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "revalidated peer-Team subscription disappeared before delivery creation",
                        "message_subscription",
                        &authority.target_subscription_id,
                        None,
                    )
                })?;
            // A Team target stays unresolved in the shared Team Inbox until one
            // exact membership generation claims it; a direct TeamMembership
            // target is bound at admission and needs no claim.
            let member_bound = authority.target_membership_id.is_some();
            vec![CanonicalMessageDelivery {
                id: format!("{}:{}", message.id, subscription.id),
                message_id: message.id.clone(),
                subscription_id: subscription.id.clone(),
                subscription_revision: subscription.revision,
                subscription_policy_digest: subscription.policy_digest.clone(),
                recipient_kind: if member_bound {
                    MessageSubjectKind::AgentMember
                } else {
                    MessageSubjectKind::Team
                },
                recipient_ref: if member_bound {
                    authority.target_agent_member_id.clone().unwrap_or_default()
                } else {
                    authority.target_team_id.clone()
                },
                target_team_id: Some(authority.target_team_id.clone()),
                target_node_id: target_node_id.into(),
                resolved_team_membership_id: authority.target_membership_id.clone(),
                recipient_agent_member_id: authority.target_agent_member_id.clone(),
                recipient_session_id: None,
                recipient_session_generation: None,
                status: CanonicalMessageDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_node_daemon_generation: None,
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                created_at: message.created_at.clone(),
                updated_at: message.created_at.clone(),
            }]
        } else {
            Vec::new()
        };
        let mut delivered_subjects = BTreeSet::new();
        let routed_recipients = if peer_authority.is_none() {
            message.recipients.as_slice()
        } else {
            &[]
        };
        for recipient in routed_recipients {
            for subscription in subscriptions.iter().filter(|subscription| {
                subscription.status == MessageSubscriptionStatus::Active
                    && subscription.target_node_id == target_node_id
                    && match recipient.kind {
                        MessageRecipientKind::AgentMember => {
                            subscription.subscriber_kind == MessageSubjectKind::AgentMember
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind == MessageSubscriptionKind::Agent
                        }
                        MessageRecipientKind::Team => {
                            subscription.subscriber_kind == MessageSubjectKind::Team
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind
                                    == MessageSubscriptionKind::AllAuthorized
                                && subscription.source_ref == "authorized_peer_teams"
                                && subscription.target_team_id.as_deref()
                                    == Some(recipient.id.as_str())
                        }
                        MessageRecipientKind::ControlPlaneActor => false,
                    }
            }) {
                let subject_key = (
                    subscription.subscriber_kind,
                    subscription.subscriber_ref.clone(),
                );
                if !delivered_subjects.insert(subject_key) {
                    continue;
                }
                // `Message.team_id` remains the immutable source-Team scope.
                // On the target Node, recipient authorization must bind the
                // collaboration target Team; requiring a target membership in
                // the source Team would make every valid cross-Team transfer
                // undeliverable (or tempt a split-Team model).
                let recipient_team_id = message
                    .collaboration_scope
                    .as_ref()
                    .map(|scope| scope.target_team_id.as_str())
                    .or(message.team_id.as_deref());
                if let Some(team_id) = recipient_team_id {
                    match subscription.subscriber_kind {
                        MessageSubjectKind::AgentMember => {
                            let exact_membership = subscription
                                .membership_ref
                                .as_deref()
                                .is_some_and(|membership_id| {
                                    memberships.iter().any(|membership| {
                                        membership.id == membership_id
                                            && membership.team_id == team_id
                                            && membership.agent_member_id
                                                == subscription.subscriber_ref
                                            && membership.node_id == target_node_id
                                            && membership.state == TeamMembershipStatus::Active
                                    })
                                });
                            if !exact_membership {
                                continue;
                            }
                        }
                        MessageSubjectKind::Team => {
                            if subscription.target_team_id.as_deref() != Some(team_id) {
                                continue;
                            }
                        }
                    }
                }
                deliveries.push(CanonicalMessageDelivery {
                    id: format!("{}:{}", message.id, subscription.id),
                    message_id: message.id.clone(),
                    subscription_id: subscription.id.clone(),
                    subscription_revision: subscription.revision,
                    subscription_policy_digest: subscription.policy_digest.clone(),
                    recipient_kind: subscription.subscriber_kind,
                    recipient_ref: subscription.subscriber_ref.clone(),
                    target_team_id: subscription.target_team_id.clone(),
                    target_node_id: target_node_id.into(),
                    resolved_team_membership_id: (subscription.subscriber_kind
                        == MessageSubjectKind::AgentMember)
                        .then(|| subscription.membership_ref.clone())
                        .flatten(),
                    recipient_agent_member_id: (subscription.subscriber_kind
                        == MessageSubjectKind::AgentMember)
                        .then(|| subscription.subscriber_ref.clone()),
                    recipient_session_id: None,
                    recipient_session_generation: None,
                    status: CanonicalMessageDeliveryStatus::Queued,
                    attempt: 1,
                    claim_id: None,
                    claimed_node_daemon_generation: None,
                    provider_receipt_id: None,
                    failure_code: None,
                    failure_detail: None,
                    version: 1,
                    created_at: message.created_at.clone(),
                    updated_at: message.created_at.clone(),
                });
            }
        }
        if deliveries.is_empty()
            && !message
                .recipients
                .iter()
                .all(|recipient| recipient.kind == MessageRecipientKind::ControlPlaneActor)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "remote Message has no authorized local recipient subscription",
                "message",
                &message.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "message",
            &message.id,
            "remote_persisted",
            serde_json::to_value(operation)?,
            &message,
            Vec::new(),
            deliveries
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}
