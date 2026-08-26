use firm_core::agentfirm_api::{
    ActorKind, ActorRef, MemberCoordinationStatus, MemberRun, Message, MessageAddressKind,
    MessageDraft, MessageKind, MessageRecipientKind, MessageRecipientRef, MessageSubjectKind,
    MessageSubscription, MessageSubscriptionStatus, ResponseIntent, TeamMembership,
    TeamMembershipStatus,
};
use firm_core::{TeamActorKind, TeamActorRef, Work};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAuthoringOperation {
    Send,
    Reply,
    RequestDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageAuthoringIntent {
    Send {
        recipient_ids: Vec<String>,
        body: String,
        work_id: Option<String>,
        evidence_refs: Vec<String>,
        response_required: bool,
    },
    Reply {
        recipient_ids: Vec<String>,
        body: String,
        correlation_id: String,
        causation_id: String,
        work_id: Option<String>,
        evidence_refs: Vec<String>,
        response_required: bool,
    },
    RequestDecision {
        body: String,
        work_id: Option<String>,
        evidence_refs: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrepareMessageAuthoringCommand {
    pub operation: MessageAuthoringOperation,
    pub team_id: String,
    pub team_run_id: String,
    pub host_agent_member_id: String,
    pub team_member_ids: Vec<String>,
    pub current_team_revision: u64,
    pub expected_team_revision: u64,
    pub actor: ActorRef,
    pub authorized_authority_actors: Vec<ActorRef>,
    pub idempotency_key: String,
    pub intent: MessageAuthoringIntent,
    pub member_runs: Vec<MemberRun>,
    pub memberships: Vec<TeamMembership>,
    pub subscriptions: Vec<MessageSubscription>,
    pub linked_work: Option<Work>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMessageAuthoring {
    pub sender: TeamActorRef,
    pub sender_runtime_id: String,
    pub recipient_runtime_ids: Vec<String>,
    pub draft: MessageDraft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageAuthoringError {
    UnauthorizedSender,
    SenderIdentityConflict { matches: usize },
    TeamRevisionConflict { current_revision: u64 },
    IntentRouteMismatch,
    WorkNotFound { work_id: String },
    WorkOutsideTeamRun { work_id: String, version: u64 },
    UnauthorizedWorkLink { work_id: String, version: u64 },
    BodyOrRecipientsRequired,
    RecipientOutsideTeam { recipient_id: String },
    RecipientRouteUnavailable { recipient_id: String },
    RecipientRuntimeAmbiguous { recipient_id: String },
}

pub fn prepare_message_authoring(
    command: PrepareMessageAuthoringCommand,
) -> Result<PreparedMessageAuthoring, MessageAuthoringError> {
    let actor_is_host = (command.actor.kind == ActorKind::AgentMember
        && command.actor.id == command.host_agent_member_id)
        || command.authorized_authority_actors.iter().any(|actor| {
            actor.kind == ActorKind::AgentMember && actor.id == command.host_agent_member_id
        });
    let sender_runs = command
        .member_runs
        .iter()
        .filter(|run| {
            run.agent_member_id == command.actor.id
                && run.team_run_id == command.team_run_id
                && run.coordination_status == MemberCoordinationStatus::Active
        })
        .collect::<Vec<_>>();
    if !actor_is_host {
        if command.actor.kind != ActorKind::AgentMember {
            return Err(MessageAuthoringError::UnauthorizedSender);
        }
        if sender_runs.len() != 1 {
            return Err(MessageAuthoringError::SenderIdentityConflict {
                matches: sender_runs.len(),
            });
        }
    }
    if command.expected_team_revision != command.current_team_revision {
        return Err(MessageAuthoringError::TeamRevisionConflict {
            current_revision: command.current_team_revision,
        });
    }

    let (
        recipient_ids,
        body,
        work_id,
        evidence_refs,
        response_required,
        correlation_id,
        causation_id,
        kind,
    ) = match (command.operation, command.intent) {
        (
            MessageAuthoringOperation::Send,
            MessageAuthoringIntent::Send {
                recipient_ids,
                body,
                work_id,
                evidence_refs,
                response_required,
            },
        ) => (
            recipient_ids,
            body,
            work_id,
            evidence_refs,
            response_required,
            format!("correlation:{}", command.idempotency_key),
            None,
            MessageKind::Message,
        ),
        (
            MessageAuthoringOperation::Reply,
            MessageAuthoringIntent::Reply {
                recipient_ids,
                body,
                correlation_id,
                causation_id,
                work_id,
                evidence_refs,
                response_required,
            },
        ) => (
            recipient_ids,
            body,
            work_id,
            evidence_refs,
            response_required,
            correlation_id,
            Some(causation_id),
            MessageKind::Reply,
        ),
        (
            MessageAuthoringOperation::RequestDecision,
            MessageAuthoringIntent::RequestDecision {
                body,
                work_id,
                evidence_refs,
            },
        ) => (
            vec![command.host_agent_member_id.clone()],
            body,
            work_id,
            evidence_refs,
            true,
            format!("decision:{}", command.idempotency_key),
            None,
            MessageKind::RequestDecision,
        ),
        _ => return Err(MessageAuthoringError::IntentRouteMismatch),
    };

    if let Some(work_id) = work_id.as_deref() {
        let work = command
            .linked_work
            .as_ref()
            .filter(|work| work.id == work_id)
            .ok_or_else(|| MessageAuthoringError::WorkNotFound {
                work_id: work_id.to_string(),
            })?;
        if work.team_run_id != command.team_run_id {
            return Err(MessageAuthoringError::WorkOutsideTeamRun {
                work_id: work.id.clone(),
                version: work.version,
            });
        }
        if !actor_is_host {
            let sender_run_id = sender_runs[0].id.as_str();
            if work.owner_member_id.as_deref() != Some(command.actor.id.as_str())
                || work.active_member_run_id.as_deref() != Some(sender_run_id)
            {
                return Err(MessageAuthoringError::UnauthorizedWorkLink {
                    work_id: work.id.clone(),
                    version: work.version,
                });
            }
        }
    }
    if body.trim().is_empty() || recipient_ids.is_empty() {
        return Err(MessageAuthoringError::BodyOrRecipientsRequired);
    }

    let allowed = command
        .team_member_ids
        .iter()
        .chain(std::iter::once(&command.host_agent_member_id))
        .collect::<BTreeSet<_>>();
    let mut recipient_runtime_ids = Vec::with_capacity(recipient_ids.len());
    for recipient_id in &recipient_ids {
        if !allowed.contains(recipient_id) {
            return Err(MessageAuthoringError::RecipientOutsideTeam {
                recipient_id: recipient_id.clone(),
            });
        }
        let matching_memberships = command
            .memberships
            .iter()
            .filter(|membership| {
                membership.team_id == command.team_id
                    && membership.agent_member_id == *recipient_id
                    && membership.state == TeamMembershipStatus::Active
            })
            .collect::<Vec<_>>();
        if matching_memberships.len() != 1
            || !command.subscriptions.iter().any(|subscription| {
                subscription.subscriber_kind == MessageSubjectKind::AgentMember
                    && subscription.subscriber_ref == *recipient_id
                    && subscription.membership_ref.as_deref()
                        == Some(matching_memberships[0].id.as_str())
                    && subscription.status == MessageSubscriptionStatus::Active
            })
        {
            return Err(MessageAuthoringError::RecipientRouteUnavailable {
                recipient_id: recipient_id.clone(),
            });
        }
        let matching_runs = command
            .member_runs
            .iter()
            .filter(|run| {
                run.agent_member_id == *recipient_id
                    && run.team_run_id == command.team_run_id
                    && run.coordination_status == MemberCoordinationStatus::Active
            })
            .collect::<Vec<_>>();
        if matching_runs.len() != 1 {
            return Err(MessageAuthoringError::RecipientRuntimeAmbiguous {
                recipient_id: recipient_id.clone(),
            });
        }
        recipient_runtime_ids.push(matching_runs[0].id.clone());
    }

    let recipients = recipient_ids
        .into_iter()
        .map(|id| MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id,
        })
        .collect::<Vec<_>>();
    let target_ref = recipients[0].clone();
    let address_kind = if recipients.len() == 1 {
        MessageAddressKind::DirectAgent
    } else {
        MessageAddressKind::AuthorizedBroadcast
    };
    let sender = TeamActorRef {
        kind: if actor_is_host {
            TeamActorKind::Host
        } else {
            TeamActorKind::AgentMember
        },
        id: if actor_is_host {
            command.host_agent_member_id
        } else {
            command.actor.id.clone()
        },
        display_name: None,
        authn_source: Some("agentfirm_http_credential".into()),
    };
    let sender_runtime_id = match sender_runs.as_slice() {
        [run] => run.id.clone(),
        _ => command.actor.id,
    };

    Ok(PreparedMessageAuthoring {
        sender,
        sender_runtime_id,
        recipient_runtime_ids,
        draft: MessageDraft {
            address_kind,
            target_ref,
            recipients,
            team_id: Some(command.team_id),
            team_run_id: Some(command.team_run_id),
            work_id,
            collaboration_scope: None,
            kind,
            body,
            correlation_id,
            causation_id,
            response_intent: if response_required {
                ResponseIntent::ResponseRequired
            } else {
                ResponseIntent::Informational
            },
            evidence_refs,
            schema_version: 1,
        },
    })
}

pub fn prepared_message_matches_canonical(
    prepared: &PreparedMessageAuthoring,
    canonical: &Message,
    idempotency_key: &str,
) -> bool {
    canonical.id == format!("message:{idempotency_key}")
        && canonical.sender_actor_ref
            == (ActorRef {
                kind: ActorKind::AgentMember,
                id: prepared.sender.id.clone(),
            })
        && canonical.address_kind == prepared.draft.address_kind
        && canonical.target_ref == prepared.draft.target_ref
        && canonical.recipients == prepared.draft.recipients
        && canonical.team_id == prepared.draft.team_id
        && canonical.team_run_id == prepared.draft.team_run_id
        && canonical.work_id == prepared.draft.work_id
        && canonical.collaboration_scope == prepared.draft.collaboration_scope
        && canonical.kind == prepared.draft.kind
        && canonical.body == prepared.draft.body
        && canonical.correlation_id == prepared.draft.correlation_id
        && canonical.causation_id == prepared.draft.causation_id
        && canonical.response_intent == prepared.draft.response_intent
        && canonical.evidence_refs == prepared.draft.evidence_refs
        && canonical.schema_version == prepared.draft.schema_version
        && canonical.idempotency_key == idempotency_key
}

#[cfg(test)]
mod tests {
    use super::*;
    use firm_core::agentfirm_api::{
        MemberRuntimeStatus, MessageHistoryPolicy, MessageSubscriptionKind, RuntimeDispatchMode,
        TeamMembershipRole,
    };
    use firm_core::{WorkClaimMode, WorkCondition, WorkPhase, WorkPriority};

    fn actor(id: &str) -> ActorRef {
        ActorRef {
            kind: ActorKind::AgentMember,
            id: id.into(),
        }
    }

    fn member_run(id: &str, member_id: &str) -> MemberRun {
        MemberRun {
            id: id.into(),
            agent_member_id: member_id.into(),
            team_run_id: "run-a".into(),
            role_snapshot: "member".into(),
            provider_profile_snapshot: None,
            requested_controls: serde_json::json!({}),
            effective_controls: serde_json::json!({}),
            coordination_status: MemberCoordinationStatus::Active,
            runtime_status: MemberRuntimeStatus::Idle,
            runtime_generation: 1,
            workspace_binding_id: None,
            native_session: None,
            version: 1,
            started_at: "t0".into(),
            last_event_at: None,
            finished_at: None,
        }
    }

    fn membership(id: &str, member_id: &str, role: TeamMembershipRole) -> TeamMembership {
        TeamMembership {
            id: id.into(),
            team_id: "team-a".into(),
            agent_member_id: member_id.into(),
            node_id: "node-a".into(),
            role,
            state: TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: vec![format!("sub-{member_id}")],
            created_by: actor("host-a"),
            revision: 1,
            joined_at: "t0".into(),
            left_at: None,
        }
    }

    fn subscription(id: &str, member_id: &str, membership_id: &str) -> MessageSubscription {
        MessageSubscription {
            id: id.into(),
            subscriber_kind: MessageSubjectKind::AgentMember,
            subscriber_ref: member_id.into(),
            execution_space_id: "space-a".into(),
            target_team_id: Some("team-a".into()),
            target_node_id: "node-a".into(),
            source_kind: MessageSubscriptionKind::Team,
            source_ref: "team-a".into(),
            delivery_mode: RuntimeDispatchMode::QueueOnly,
            history_policy: MessageHistoryPolicy::FromJoin,
            membership_ref: Some(membership_id.into()),
            authorization_policy_ref: "policy-a".into(),
            policy_revision: 1,
            policy_digest: "digest-a".into(),
            status: MessageSubscriptionStatus::Active,
            revision: 1,
            created_by: actor("host-a"),
            created_at: "t0".into(),
            revoked_at: None,
        }
    }

    fn linked_work(owner: &str, run_id: &str) -> Work {
        Work {
            id: "work-a".into(),
            team_run_id: "run-a".into(),
            accountable_team_id: Some("team-a".into()),
            assignee_membership_id: Some("membership-member".into()),
            legacy_containment_ref: None,
            title: "work".into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "done".into(),
            phase: WorkPhase::Active,
            condition: WorkCondition::Normal,
            resolution: None,
            owner_member_id: Some(owner.into()),
            active_member_run_id: Some(run_id.into()),
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            created_by_actor: TeamActorRef {
                kind: TeamActorKind::Host,
                id: "host-a".into(),
                display_name: None,
                authn_source: None,
            },
            created_by_member_id: None,
            result_summary: None,
            blocker_reason: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            version: 4,
            created_at: "t0".into(),
            updated_at: "t0".into(),
        }
    }

    fn command(
        operation: MessageAuthoringOperation,
        intent: MessageAuthoringIntent,
    ) -> PrepareMessageAuthoringCommand {
        PrepareMessageAuthoringCommand {
            operation,
            team_id: "team-a".into(),
            team_run_id: "run-a".into(),
            host_agent_member_id: "host-a".into(),
            team_member_ids: vec!["member-a".into()],
            current_team_revision: 2,
            expected_team_revision: 2,
            actor: actor("member-a"),
            authorized_authority_actors: Vec::new(),
            idempotency_key: "message-key".into(),
            intent,
            member_runs: vec![
                member_run("host-run", "host-a"),
                member_run("member-run", "member-a"),
            ],
            memberships: vec![
                membership("membership-host", "host-a", TeamMembershipRole::Host),
                membership("membership-member", "member-a", TeamMembershipRole::Member),
            ],
            subscriptions: vec![
                subscription("sub-host", "host-a", "membership-host"),
                subscription("sub-member", "member-a", "membership-member"),
            ],
            linked_work: None,
        }
    }

    #[test]
    fn send_reply_and_request_decision_produce_exact_typed_drafts() {
        let send = prepare_message_authoring(command(
            MessageAuthoringOperation::Send,
            MessageAuthoringIntent::Send {
                recipient_ids: vec!["host-a".into(), "member-a".into()],
                body: "status".into(),
                work_id: None,
                evidence_refs: vec!["check:send".into()],
                response_required: false,
            },
        ))
        .expect("send");
        assert_eq!(send.sender.kind, TeamActorKind::AgentMember);
        assert_eq!(send.sender.id, "member-a");
        assert_eq!(send.sender_runtime_id, "member-run");
        assert_eq!(send.recipient_runtime_ids, ["host-run", "member-run"]);
        assert_eq!(
            send.draft.address_kind,
            MessageAddressKind::AuthorizedBroadcast
        );
        assert_eq!(send.draft.kind, MessageKind::Message);
        assert_eq!(send.draft.correlation_id, "correlation:message-key");
        assert_eq!(send.draft.causation_id, None);
        assert_eq!(send.draft.response_intent, ResponseIntent::Informational);

        let reply = prepare_message_authoring(command(
            MessageAuthoringOperation::Reply,
            MessageAuthoringIntent::Reply {
                recipient_ids: vec!["host-a".into()],
                body: "answer".into(),
                correlation_id: "correlation-original".into(),
                causation_id: "message-original".into(),
                work_id: None,
                evidence_refs: Vec::new(),
                response_required: true,
            },
        ))
        .expect("reply");
        assert_eq!(reply.draft.address_kind, MessageAddressKind::DirectAgent);
        assert_eq!(reply.draft.kind, MessageKind::Reply);
        assert_eq!(reply.draft.correlation_id, "correlation-original");
        assert_eq!(
            reply.draft.causation_id.as_deref(),
            Some("message-original")
        );
        assert_eq!(
            reply.draft.response_intent,
            ResponseIntent::ResponseRequired
        );

        let decision = prepare_message_authoring(command(
            MessageAuthoringOperation::RequestDecision,
            MessageAuthoringIntent::RequestDecision {
                body: "choose".into(),
                work_id: None,
                evidence_refs: vec!["check:decision".into()],
            },
        ))
        .expect("decision");
        assert_eq!(
            decision.draft.recipients,
            vec![MessageRecipientRef {
                kind: MessageRecipientKind::AgentMember,
                id: "host-a".into(),
            }]
        );
        assert_eq!(decision.recipient_runtime_ids, ["host-run"]);
        assert_eq!(decision.draft.kind, MessageKind::RequestDecision);
        assert_eq!(decision.draft.correlation_id, "decision:message-key");
        assert_eq!(
            decision.draft.response_intent,
            ResponseIntent::ResponseRequired
        );
    }

    #[test]
    fn exact_host_or_single_active_member_and_team_revision_are_required() {
        let intent = MessageAuthoringIntent::Send {
            recipient_ids: vec!["host-a".into()],
            body: "status".into(),
            work_id: None,
            evidence_refs: Vec::new(),
            response_required: false,
        };
        let mut unauthorized = command(MessageAuthoringOperation::Send, intent.clone());
        unauthorized.actor = ActorRef {
            kind: ActorKind::Human,
            id: "operator-a".into(),
        };
        assert_eq!(
            prepare_message_authoring(unauthorized),
            Err(MessageAuthoringError::UnauthorizedSender)
        );

        let mut ambiguous = command(MessageAuthoringOperation::Send, intent.clone());
        ambiguous
            .member_runs
            .push(member_run("member-run-2", "member-a"));
        assert_eq!(
            prepare_message_authoring(ambiguous),
            Err(MessageAuthoringError::SenderIdentityConflict { matches: 2 })
        );

        let mut stale = command(MessageAuthoringOperation::Send, intent.clone());
        stale.expected_team_revision = 1;
        assert_eq!(
            prepare_message_authoring(stale),
            Err(MessageAuthoringError::TeamRevisionConflict {
                current_revision: 2
            })
        );

        let mut external_host = command(MessageAuthoringOperation::Send, intent);
        external_host.actor = ActorRef {
            kind: ActorKind::Human,
            id: "human-host-session".into(),
        };
        external_host.authorized_authority_actors = vec![actor("host-a")];
        let prepared = prepare_message_authoring(external_host).expect("authorized Host");
        assert_eq!(prepared.sender.kind, TeamActorKind::Host);
        assert_eq!(prepared.sender.id, "host-a");
        assert_eq!(prepared.sender_runtime_id, "human-host-session");
    }

    #[test]
    fn work_link_and_recipient_route_authority_fail_closed() {
        let intent = MessageAuthoringIntent::Send {
            recipient_ids: vec!["host-a".into()],
            body: "work status".into(),
            work_id: Some("work-a".into()),
            evidence_refs: Vec::new(),
            response_required: false,
        };
        let missing = command(MessageAuthoringOperation::Send, intent.clone());
        assert_eq!(
            prepare_message_authoring(missing),
            Err(MessageAuthoringError::WorkNotFound {
                work_id: "work-a".into()
            })
        );

        let mut foreign_owner = command(MessageAuthoringOperation::Send, intent.clone());
        foreign_owner.linked_work = Some(linked_work("other-member", "other-run"));
        assert_eq!(
            prepare_message_authoring(foreign_owner),
            Err(MessageAuthoringError::UnauthorizedWorkLink {
                work_id: "work-a".into(),
                version: 4
            })
        );

        let mut valid = command(MessageAuthoringOperation::Send, intent);
        valid.linked_work = Some(linked_work("member-a", "member-run"));
        assert!(prepare_message_authoring(valid).is_ok());

        let outside_intent = MessageAuthoringIntent::Send {
            recipient_ids: vec!["outsider".into()],
            body: "hello".into(),
            work_id: None,
            evidence_refs: Vec::new(),
            response_required: false,
        };
        assert_eq!(
            prepare_message_authoring(command(MessageAuthoringOperation::Send, outside_intent)),
            Err(MessageAuthoringError::RecipientOutsideTeam {
                recipient_id: "outsider".into()
            })
        );

        let mut no_subscription = command(
            MessageAuthoringOperation::Send,
            MessageAuthoringIntent::Send {
                recipient_ids: vec!["host-a".into()],
                body: "hello".into(),
                work_id: None,
                evidence_refs: Vec::new(),
                response_required: false,
            },
        );
        no_subscription.subscriptions.clear();
        assert_eq!(
            prepare_message_authoring(no_subscription),
            Err(MessageAuthoringError::RecipientRouteUnavailable {
                recipient_id: "host-a".into()
            })
        );
    }

    #[test]
    fn mismatch_empty_runtime_ambiguity_and_replay_semantics_are_exact() {
        let reply_intent = MessageAuthoringIntent::Reply {
            recipient_ids: vec!["host-a".into()],
            body: "answer".into(),
            correlation_id: "correlation-a".into(),
            causation_id: "message-a".into(),
            work_id: None,
            evidence_refs: Vec::new(),
            response_required: false,
        };
        assert_eq!(
            prepare_message_authoring(command(MessageAuthoringOperation::Send, reply_intent)),
            Err(MessageAuthoringError::IntentRouteMismatch)
        );

        let empty_intent = MessageAuthoringIntent::Send {
            recipient_ids: vec!["host-a".into()],
            body: "  ".into(),
            work_id: None,
            evidence_refs: Vec::new(),
            response_required: false,
        };
        assert_eq!(
            prepare_message_authoring(command(MessageAuthoringOperation::Send, empty_intent)),
            Err(MessageAuthoringError::BodyOrRecipientsRequired)
        );

        let send_intent = MessageAuthoringIntent::Send {
            recipient_ids: vec!["host-a".into()],
            body: "status".into(),
            work_id: None,
            evidence_refs: vec!["check:a".into()],
            response_required: false,
        };
        let stable = command(MessageAuthoringOperation::Send, send_intent.clone());
        assert_eq!(
            prepare_message_authoring(stable.clone()),
            prepare_message_authoring(stable),
            "the same command must produce the same authoring plan"
        );

        let mut ambiguous_recipient = command(MessageAuthoringOperation::Send, send_intent.clone());
        ambiguous_recipient
            .member_runs
            .push(member_run("host-run-2", "host-a"));
        assert_eq!(
            prepare_message_authoring(ambiguous_recipient),
            Err(MessageAuthoringError::RecipientRuntimeAmbiguous {
                recipient_id: "host-a".into()
            })
        );

        let prepared =
            prepare_message_authoring(command(MessageAuthoringOperation::Send, send_intent))
                .expect("prepared Message");
        let mut canonical = Message {
            id: "message:role-message-key".into(),
            source_execution_space_id: "space-a".into(),
            source_node_id: "node-a".into(),
            source_node_daemon_id: "daemon-a".into(),
            source_authority_generation: 3,
            sender_actor_ref: actor("member-a"),
            sender_agent_member_id: Some("member-a".into()),
            sender_session_id: Some("session-a".into()),
            address_kind: prepared.draft.address_kind,
            target_ref: prepared.draft.target_ref.clone(),
            recipients: prepared.draft.recipients.clone(),
            team_id: prepared.draft.team_id.clone(),
            team_run_id: prepared.draft.team_run_id.clone(),
            work_id: prepared.draft.work_id.clone(),
            collaboration_scope: None,
            kind: prepared.draft.kind,
            body: prepared.draft.body.clone(),
            body_digest: "sha256:body".into(),
            correlation_id: prepared.draft.correlation_id.clone(),
            causation_id: prepared.draft.causation_id.clone(),
            response_intent: prepared.draft.response_intent,
            evidence_refs: prepared.draft.evidence_refs.clone(),
            content_fingerprint: "sha256:content".into(),
            schema_version: 1,
            idempotency_key: "role-message-key".into(),
            created_at: "t1".into(),
        };
        assert!(prepared_message_matches_canonical(
            &prepared,
            &canonical,
            "role-message-key"
        ));
        canonical.body = "changed".into();
        assert!(!prepared_message_matches_canonical(
            &prepared,
            &canonical,
            "role-message-key"
        ));
    }
}
