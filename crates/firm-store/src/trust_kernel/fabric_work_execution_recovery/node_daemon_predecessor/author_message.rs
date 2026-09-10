//! Local message-creation proof under the predecessor recovery write lock.
//! No provider effects, message replay, delivery receipts or general recovery API.
use super::*;
use crate::trust_kernel::{event_projection, TrustOperationEnvelope, TRUST_OPERATIONS_LEDGER};
use firm_core::agentfirm_api::message_content_fingerprint;
use firm_core::agentfirm_api::*;

fn unproven(reason: &str) -> StoreError {
    StoreError::Conflict(format!(
        "NODE_DAEMON_PREDECESSOR_MESSAGE_UNPROVEN: {reason}"
    ))
}

impl HarnessStore {
    pub(super) fn reconcile_predecessor_author_messages_unlocked(
        &self,
        context: &MutationContext,
        spaces: &[String],
        lease: &NodeDaemonLease,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<()> {
        let mut pending = Vec::new();
        for space in spaces {
            for record in self.runtime_commands(space)? {
                if record.target_node_id != lease.node_id
                    || record.target_node_daemon_id != lease.daemon_id
                    || record.target_node_daemon_generation != lease.generation
                    || (matches!(
                        record.phase,
                        RuntimeCommandPhase::Settled | RuntimeCommandPhase::Rejected
                    ) && matches!(
                        record.effect_certainty,
                        RuntimeEffectCertainty::Applied | RuntimeEffectCertainty::NotApplied
                    ))
                {
                    continue;
                }
                if record.command != RuntimeCommandKind::AuthorMessage
                    || !matches!(
                        record.phase,
                        RuntimeCommandPhase::Prepared | RuntimeCommandPhase::RecoveryRequired
                    )
                    || record.effect_certainty != RuntimeEffectCertainty::Unknown
                {
                    return Err(StoreError::Conflict(format!(
                        "NODE_DAEMON_PREDECESSOR_RECOVERY_COMMAND_UNSETTLED: RuntimeCommand {} is {:?}/{:?}",
                        record.id, record.phase, record.effect_certainty,
                    )));
                }
                pending.push(record);
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        // The ordinary reader intentionally ignores old torn tails. Absence proof
        // must not use that compatibility behavior. Read every durable frame here.
        let bytes = std::fs::read(self.root.join(TRUST_OPERATIONS_LEDGER))?;
        if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
            return Err(unproven("canonical history has an unterminated tail"));
        }
        let mut history: Vec<TrustOperationEnvelope> = Vec::new();
        let mut versions = std::collections::BTreeMap::new();
        for row in bytes
            .split(|byte| *byte == b'\n')
            .filter(|row| !row.is_empty())
        {
            let envelope: TrustOperationEnvelope = serde_json::from_slice(row)?;
            let event = &envelope.operation.event;
            let key = (
                envelope.execution_space_id.clone(),
                event.aggregate_kind.clone(),
                event.aggregate_id.clone(),
            );
            let (prior_sequence, prior_version) = *versions.get(&key).unwrap_or(&(0, 0));
            // Work revisions also advance through WorkOperation history; its
            // canonical event count is independent of that revision sequence.
            // Message proof never depends on a Work projection. Other aggregates
            // retain the complete canonical version chain used by that proof.
            if event.store_sequence != history.len() as u64 + 1
                || event.sequence != prior_sequence + 1
                || (event.aggregate_kind != "work" && event.expected_version != prior_version)
                || event.expected_version.checked_add(1) != Some(event.resulting_version)
            {
                return Err(unproven("canonical history has a sequence/version gap"));
            }
            versions.insert(key, (event.sequence, event.resulting_version));
            history.push(envelope);
        }
        // Validate every outcome before appending anything. Another Unknown command
        // or conflicting evidence must not leave a half-applied recovery attempt.
        let outcomes = pending
            .into_iter()
            .map(|record| {
                let certainty = prove_author_message(&history, &record)?;
                Ok((record, certainty))
            })
            .collect::<StoreResult<Vec<_>>>()?;
        for (mut record, certainty) in outcomes {
            let mut mutation = context.clone();
            mutation.execution_space_id = record.execution_space_id.clone();
            mutation.command_name = "node_daemon.predecessor_recovery.author_message".into();
            mutation.idempotency_key = format!(
                "predecessor-author-message:{}:{}:{}:{}:{}",
                lease.node_id, lease.daemon_id, lease.generation, lease.instance_id, record.id
            );
            mutation.expected_version = record.version;
            mutation.request_fingerprint = None;
            record.phase = if certainty == RuntimeEffectCertainty::Applied {
                RuntimeCommandPhase::Settled
            } else {
                RuntimeCommandPhase::Rejected
            };
            record.effect_certainty = certainty;
            record.postcondition_status = RuntimePostconditionStatus::Unknown;
            record.failure_code = (certainty == RuntimeEffectCertainty::NotApplied)
                .then(|| "PREDECESSOR_MESSAGE_NOT_AUTHORED".into());
            record.result = Some(
                serde_json::json!({"evidence_ref": evidence_ref, "local_authoring_only": true, "blind_replay": false}),
            );
            record.version += 1;
            record.updated_at = updated_at.into();
            self.commit_trust_projection_unlocked(
                &mutation,
                "runtime_command",
                &record.id,
                "predecessor_message_reconciled",
                serde_json::json!({
                    "command_id": record.id, "request_fingerprint": record.request_fingerprint,
                    "effect_certainty": certainty, "evidence_ref": evidence_ref,
                    "node_id": lease.node_id, "daemon_id": lease.daemon_id,
                    "generation": lease.generation, "instance_id": lease.instance_id,
                }),
                &record,
                Vec::new(),
                Vec::new(),
            )?;
        }
        Ok(())
    }
}

fn prove_author_message(
    history: &[TrustOperationEnvelope],
    record: &RuntimeCommandRecord,
) -> StoreResult<RuntimeEffectCertainty> {
    let in_space =
        |entry: &&TrustOperationEnvelope| entry.execution_space_id == record.execution_space_id;
    let accepted = history
        .iter()
        .filter(in_space)
        .filter(|entry| {
            entry.operation.event.aggregate_kind == "runtime_command"
                && entry.operation.event.aggregate_id == record.id
                && entry.operation.event.transition == "accepted"
        })
        .collect::<Vec<_>>();
    if accepted.len() != 1 {
        return Err(unproven(
            "accepted command envelope is missing or duplicated",
        ));
    }
    let accepted = accepted[0];
    let command: ControlCommandEnvelope =
        serde_json::from_value(accepted.operation.event.payload.clone())?;
    let original: RuntimeCommandRecord = event_projection(accepted)?;
    if command.command != RuntimeCommandKind::AuthorMessage
        || command.id != record.id
        || command.execution_space_id != record.execution_space_id
        || command.target_node_id != record.target_node_id
        || command.target_node_daemon_id != record.target_node_daemon_id
        || command.target_node_daemon_generation != record.target_node_daemon_generation
        || command.authenticated_actor != record.authenticated_actor
        || command.idempotency_key != record.idempotency_key
        || canonical_json_fingerprint(&command.payload) != command.payload_fingerprint
        || runtime_command_envelope_fingerprint(&command)? != record.request_fingerprint
        || original.request_fingerprint != record.request_fingerprint
    {
        return Err(unproven(
            "accepted envelope disagrees with the exact command",
        ));
    }
    let draft: MessageDraft = serde_json::from_value(command.payload["draft"].clone())?;
    let id = format!("message:{}", command.idempotency_key);
    let candidates = history
        .iter()
        .filter(in_space)
        .filter(|entry| {
            entry.operation.event.aggregate_kind == "message"
                && (entry.operation.event.aggregate_id == id
                    || entry.operation.resulting_projection["idempotency_key"]
                        == command.idempotency_key)
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        let orphan = history.iter().filter(in_space).any(|entry| {
            entry.operation.resulting_projection["message_id"] == id
                || entry
                    .operation
                    .immutable_side_records
                    .iter()
                    .chain(&entry.operation.initial_outbox_records)
                    .any(|row| row["message_id"] == id || row["id"] == id)
        });
        if orphan {
            return Err(unproven(
                "message absent but related canonical records exist",
            ));
        }
        return Ok(RuntimeEffectCertainty::NotApplied);
    }
    if candidates.len() != 1 {
        return Err(unproven("conflicting message history"));
    }
    let authored = candidates[0];
    let event = &authored.operation.event;
    let message: Message = event_projection(authored)?;
    let serialized = serde_json::to_value(&message)?;
    let serialized_draft = serde_json::to_value(&draft)?;
    let draft_matches = serialized_draft
        .as_object()
        .unwrap()
        .iter()
        .all(|(key, value)| serialized.get(key) == Some(value));
    if event.transition != "authored"
        || event.store_sequence <= accepted.operation.event.store_sequence
        || event.aggregate_id != id
        || event.payload != serialized
        || !draft_matches
        || message.id != id
        || message.idempotency_key != command.idempotency_key
        || message.sender_actor_ref != command.authenticated_actor
        || message.source_execution_space_id != command.execution_space_id
        || message.source_node_id != command.target_node_id
        || message.source_node_daemon_id != command.target_node_daemon_id
        || message.source_authority_generation != command.target_node_daemon_generation
        || message.body_digest != message_body_digest(&message.body)
        || message.content_fingerprint != message_content_fingerprint(&message)
    {
        return Err(unproven(
            "authored message does not prove the accepted command effect",
        ));
    }
    // Never derive sender identity from a successor Session. Validate the exact
    // historical projection visible before this immutable author operation.
    if let Some(session_id) = &message.sender_session_id {
        let prior = history
            .iter()
            .filter(in_space)
            .rfind(|entry| {
                entry.operation.event.aggregate_kind == "agent_session"
                    && entry.operation.event.aggregate_id == *session_id
                    && entry.operation.event.store_sequence < event.store_sequence
            })
            .ok_or_else(|| unproven("historical sender Session missing"))?;
        let session: AgentSession = event_projection(prior)?;
        if Some(&session.agent_member_id) != message.sender_agent_member_id.as_ref()
            || session.agent_member_id != message.sender_actor_ref.id
            || session.node_id != message.source_node_id
            || session.node_daemon_id != message.source_node_daemon_id
            || session.node_daemon_generation != message.source_authority_generation
            || session.lifecycle == AgentSessionStatus::Closed
        {
            return Err(unproven("historical sender Session mismatch"));
        }
    } else if message.sender_agent_member_id.is_some() {
        return Err(unproven("sender identity lacks its historical Session"));
    }
    let before_author = history
        .iter()
        .filter(in_space)
        .filter(|entry| entry.operation.event.store_sequence < event.store_sequence);
    let subscriptions =
        crate::trust_kernel::fabric_identity_sessions::message_subscriptions_from_history(
            before_author.clone(),
        )?;
    let mut memberships = std::collections::BTreeMap::new();
    for entry in
        before_author.filter(|entry| entry.operation.event.aggregate_kind == "team_membership")
    {
        let membership: TeamMembership = event_projection(entry)?;
        memberships.insert(membership.id.clone(), membership);
    }
    let authority: Option<firm_core::collaboration::MessageAdmissionAuthority> = command
        .payload
        .get("message_admission_authority")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    let target_team = match &authority {
        Some(firm_core::collaboration::MessageAdmissionAuthority::PeerTeam(authority)) => {
            Some(authority.target_team_id.as_str())
        }
        _ => None,
    };
    let expected = crate::trust_kernel::fabric_message_authoring::initial_message_deliveries(
        &message,
        &subscriptions,
        &memberships.into_values().collect::<Vec<_>>(),
        target_team,
    );
    let observed = authored
        .operation
        .initial_outbox_records
        .iter()
        .map(|value| serde_json::from_value::<CanonicalMessageDelivery>(value.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    if observed != expected {
        return Err(unproven(
            "initial delivery set differs from historical subscriptions and memberships",
        ));
    }
    if expected.is_empty()
        && !message
            .recipients
            .iter()
            .all(|recipient| recipient.kind == MessageRecipientKind::ControlPlaneActor)
        && message
            .collaboration_scope
            .as_ref()
            .is_none_or(|scope| scope.source_team_id == scope.target_team_id)
    {
        return Err(unproven("missing initial message delivery evidence"));
    }
    Ok(RuntimeEffectCertainty::Applied)
}
