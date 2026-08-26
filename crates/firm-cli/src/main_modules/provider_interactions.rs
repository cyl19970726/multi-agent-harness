use super::*;

#[cfg(test)]
mod tests_live_provider_preview {
    use super::*;

    #[test]
    fn stale_live_adapter_generation_is_fenced_before_registry_ingress() {
        require_live_member_run_generation("member-run-1", 2, 1)
            .expect_err("a pre-Reopen adapter must not publish into the current generation");
        require_live_member_run_generation("member-run-1", 2, 2)
            .expect("the exact current adapter generation may publish");
    }
}

pub(super) fn new_live_provider_activity_token() -> String {
    let mut bytes = [0u8; 32];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .is_ok()
    {
        return bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    }
    // A callback token is process-local defense-in-depth over a user-owned
    // Unix control socket. Fall back without creating a durable identifier.
    format!(
        "serve-{}-{}-{}",
        std::process::id(),
        current_unix_ms_u64(),
        generated_id("live-token")
    )
}

pub(super) fn kimi_interaction_prompt(frame: &serde_json::Value) -> String {
    frame
        .pointer("/params/toolCall/content")
        .and_then(|content| content.as_array())
        .into_iter()
        .flatten()
        .filter_map(|block| {
            block
                .pointer("/content/text")
                .or_else(|| block.get("text"))
                .and_then(|text| text.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug)]
pub(super) struct ProviderInteractionReply {
    pub(super) result: serde_json::Value,
    pub(super) claimed_response: Option<TeamMessageProjection>,
}

pub(super) fn provider_interaction_request_message(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    provider_request_id: &str,
    method: &str,
    interaction_type: ProviderInteractionType,
    prompt: String,
    options: Vec<ProviderInteractionMessageOption>,
) -> CliResult<(TeamMessageProjection, bool)> {
    let native_session = member.native_session.as_ref().ok_or_else(|| {
        CliError::Usage(format!(
            "ProviderRuntimeProjection {} has no native session for provider interaction",
            member.id
        ))
    })?;
    let body = ProviderInteractionRequestBody {
        interaction_type,
        prompt,
        options,
        provider: member.provider.clone(),
        provider_request_id: provider_request_id.to_string(),
        method: method.to_string(),
        session: native_session.native_session_id.clone(),
        member: member.id.clone(),
        generation: member.runtime_generation,
    };
    let canonical_body = body.to_canonical_json().map_err(CliError::Usage)?;
    let correlation_id = body.correlation_id();
    let _guard = ledger.write_lock();
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    let host_member_run_id =
        store_conflict_as_usage(ledger.store.active_host_member_binding(&ledger.run_id))?
            .member_run
            .id;
    let existing = ledger
        .store
        .fabric_messages(&execution_space_id)?
        .into_iter()
        .filter(|message| {
            message.team_run_id.as_deref() == Some(ledger.run_id.as_str())
                && message.kind
                    == harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest
                && message.correlation_id == correlation_id
        })
        .collect::<Vec<_>>();
    if let Some(existing_request) = existing.first() {
        if existing.len() != 1 || existing_request.body != canonical_body {
            return Err(CliError::Usage(format!(
                "provider request {provider_request_id} was replayed with different semantics"
            )));
        }
        let mut replay = prepare_team_message_as(
            &ledger.store,
            &ledger.run_id,
            &TeamActorRef {
                kind: TeamActorKind::ProviderRuntimeProjection,
                id: member.id.clone(),
                display_name: Some(member.name.clone()),
                authn_source: Some("provider_reverse_request".into()),
            },
            vec![host_member_run_id.clone()],
            ProviderDispatchIntent::ProviderInteractionRequest,
            &canonical_body,
            None,
            Some(correlation_id.clone()),
            None,
            TeamMessageDeliveryMode::Routed,
            Some(ProviderResponseIntent::ResponseRequired),
        )?;
        replay.id = existing_request.id.clone();
        replay.deliveries.clear();
        return Ok((replay, false));
    }

    let created_at = now_string();
    let sender = TeamActorRef {
        kind: TeamActorKind::ProviderRuntimeProjection,
        id: member.id.clone(),
        display_name: Some(member.name.clone()),
        authn_source: Some("provider_reverse_request".to_string()),
    };
    let host_recipient_id = host_member_run_id.clone();
    let request = TeamMessageProjection {
        id: format!(
            "tmsg-provider-request-{}",
            content_hash_hex16(&correlation_id)
        ),
        team_run_id: ledger.run_id.clone(),
        work_id: None,
        source_plan_ref: None,
        sender: Some(sender.clone()),
        sender_runtime_id: member.id.clone(),
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::Host,
            id: host_recipient_id.clone(),
        }],
        recipient_runtime_ids: vec![host_member_run_id],
        kind: ProviderDispatchIntent::ProviderInteractionRequest,
        body: canonical_body,
        correlation_id,
        causation_id: None,
        response_intent: Some(ProviderResponseIntent::ResponseRequired),
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: host_recipient_id,
            policy: TeamDeliveryPolicy::ManualAck,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some(format!("provider-reverse-request:{provider_request_id}")),
            failure_reason: None,
            updated_at: created_at.clone(),
        }],
        created_at,
    };
    let published = publish_team_message(&ledger.store, &sender, request)?;
    Ok((published, true))
}

pub(super) fn wait_for_provider_interaction_response(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    request: &TeamMessageProjection,
) -> CliResult<
    Option<(
        ProviderInteractionResponseBody,
        Option<TeamMessageProjection>,
    )>,
> {
    loop {
        require_provider_session_authority(ledger, &member.agent_member_id, true)?;
        let latest_member = ledger
            .latest_member_run(&member.id)?
            .ok_or_else(|| CliError::Usage(format!("member run {} not found", member.id)))?;
        let same_generation = latest_member.runtime_generation == member.runtime_generation
            && latest_member.native_session == member.native_session;
        if !same_generation
            || !latest_member.coordination_is_active()
            || matches!(
                latest_member.status,
                MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
            )
            || latest_team_run(&ledger.store, &ledger.run_id)?.status != TeamRunStatus::Running
        {
            return Ok(None);
        }
        let lifecycle_cancelled =
            current_team_run_events_in_append_order(&ledger.store, &ledger.run_id)?
                .into_iter()
                .any(|event| {
                    event.entity_type == "message"
                        && event.entity_id == request.id
                        && event.operation == "cancelled"
                });
        if lifecycle_cancelled {
            return Ok(None);
        }

        let messages = claim_canonical_messages_for_member(ledger, &latest_member)?;
        let response = messages
            .iter()
            .find(|message| {
                message.kind == ProviderDispatchIntent::ProviderInteractionResponse
                    && message.causation_id.as_deref() == Some(request.id.as_str())
            })
            .cloned();
        if let Some(response) = response {
            let body = ProviderInteractionResponseBody::parse_canonical_json(&response.body)
                .map_err(CliError::Usage)?;
            return Ok(Some((body, Some(response))));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn complete_provider_interaction_reply(
    ledger: &TeamRunLedger,
    member_id: &str,
    reply: &ProviderInteractionReply,
    provider_receipt_id: &str,
) -> CliResult<()> {
    if let Some(message) = reply.claimed_response.as_ref() {
        let member = ledger
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
        mark_message_delivered(
            ledger,
            message,
            member_id,
            &member.name,
            provider_receipt_id,
        )?;
    }
    Ok(())
}

pub(super) fn handle_codex_provider_request(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    frame: &serde_json::Value,
) -> CliResult<ProviderInteractionReply> {
    let supplied_member = member;
    let member = ledger
        .latest_member_run(&supplied_member.id)?
        .ok_or_else(|| CliError::Usage(format!("member run {} not found", supplied_member.id)))?;
    validate_provider_callback_drift(supplied_member, &member)?;
    let method = frame
        .get("method")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let params = frame.get("params").unwrap_or(frame);
    let provider_request_id = frame
        .get("id")
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string())
        })
        .unwrap_or_else(|| generated_id("provider-request"));

    // The AgentSession's effective permission ceiling is frozen before Codex
    // starts and is enforced by its native sandbox with approvalPolicy=never.
    // A later approval callback cannot widen that ceiling and does not become
    // a second Harness permission workflow. Unexpected callbacks fail closed.
    if matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    ) {
        ledger.append_provider_control_receipt_once(
            &member,
            "Codex approval callback rejected",
            "session-start permission ceiling is immutable; unexpected approval callback failed closed",
        )?;
        return Ok(ProviderInteractionReply {
            result: if method == "item/permissions/requestApproval" {
                serde_json::json!({"permissions": {}, "scope": "turn"})
            } else {
                serde_json::json!({"decision": "decline"})
            },
            claimed_response: None,
        });
    }

    let (interaction_type, title, prompt, options) = if method == "item/tool/requestUserInput" {
        let questions = params
            .get("questions")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                CliError::Usage(
                    "Codex requestUserInput must contain exactly one question; denied fail-closed"
                        .to_string(),
                )
            })?;
        if questions.len() != 1 {
            return Err(CliError::Usage(format!(
                "Codex requestUserInput supports exactly one question; received {}; denied fail-closed",
                questions.len()
            )));
        }
        let question = questions.first();
        let question_id = question
            .and_then(|question| question.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("answer");
        let options = question
            .and_then(|question| question.get("options"))
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, option)| ProviderInteractionMessageOption {
                id: format!("{question_id}::{index}"),
                label: option
                    .get("label")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Option")
                    .to_string(),
                intent: Some("answer".to_string()),
            })
            .collect::<Vec<_>>();
        (
            ProviderInteractionType::Question,
            question
                .and_then(|question| question.get("header"))
                .and_then(|value| value.as_str())
                .unwrap_or("Codex question")
                .to_string(),
            question
                .and_then(|question| question.get("question"))
                .and_then(|value| value.as_str())
                .unwrap_or("Codex requested input")
                .to_string(),
            options,
        )
    } else {
        return Err(CliError::Usage(format!(
            "unsupported Codex app-server request {method}; denied fail-closed"
        )));
    };

    let (request, created) = provider_interaction_request_message(
        ledger,
        &member,
        &provider_request_id,
        method,
        interaction_type,
        prompt.clone(),
        options.clone(),
    )?;
    if created {
        ledger.append_action(
            &member.id,
            "waiting_for_input",
            MemberActionStatus::Started,
            &title,
            &prompt,
        )?;
    }
    let waiting =
        match transition_provider_interaction_member(ledger, &member, MemberRunStatus::Waiting)? {
            ProviderInteractionMemberTransition::Applied(waiting) => waiting,
            ProviderInteractionMemberTransition::LifecycleSuperseded => {
                unreachable!(
                    "entering Waiting never treats lifecycle change as a resumable outcome"
                )
            }
        };

    let resolved = wait_for_provider_interaction_response(ledger, &member, &request)?;
    let lifecycle_superseded =
        match transition_provider_interaction_member(ledger, &waiting, MemberRunStatus::Running)? {
            ProviderInteractionMemberTransition::Applied(_) => false,
            ProviderInteractionMemberTransition::LifecycleSuperseded => true,
        };
    ledger.append_action(
        &member.id,
        "provider_question_resolved",
        if resolved.is_some() {
            MemberActionStatus::Succeeded
        } else {
            MemberActionStatus::Cancelled
        },
        &title,
        if resolved.is_some() {
            "correlated provider answer received"
        } else {
            "provider question cancelled by lifecycle"
        },
    )?;

    let (response, claimed_response) = match resolved {
        Some((response, claimed_response)) if !lifecycle_superseded => {
            (Some(response), claimed_response)
        }
        _ => (None, None),
    };

    if method == "item/tool/requestUserInput" {
        let question_id = params
            .pointer("/questions/0/id")
            .and_then(|value| value.as_str())
            .unwrap_or("answer");
        let answer = response
            .as_ref()
            .and_then(|response| {
                response.text.clone().or_else(|| {
                    response.choice.as_deref().and_then(|selected| {
                        options
                            .iter()
                            .find(|option| option.id == selected)
                            .map(|option| option.label.clone())
                    })
                })
            })
            .unwrap_or_default();
        return Ok(ProviderInteractionReply {
            result: serde_json::json!({
                "answers": {question_id: {"answers": [answer]}}
            }),
            claimed_response,
        });
    }
    Err(CliError::Usage(format!(
        "unsupported Codex app-server response path for {method}; denied fail-closed"
    )))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KimiAcpV1PermissionIntent {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

impl KimiAcpV1PermissionIntent {
    pub(super) fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            Some("allow_once") => Some(Self::AllowOnce),
            Some("allow_always") => Some(Self::AllowAlways),
            Some("reject_once") => Some(Self::RejectOnce),
            Some("reject_always") => Some(Self::RejectAlways),
            _ => None,
        }
    }

    pub(super) fn is_allow(self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways)
    }
}

pub(super) fn kimi_acp_v1_indexed_option_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub(super) fn kimi_acp_v1_reserved_option_id(id: &str) -> bool {
    id.starts_with("q0_") || id.starts_with("plan_")
}

/// Classify the reviewed Kimi ACP v1 `session/request_permission` wire shape.
///
/// Permission `kind` is the canonical discriminator. Opaque option ids are
/// considered only after every kind is recognized and at least one exact
/// allow intent exists. This prevents a reject-only or future/unknown tool
/// callback from becoming a user-facing Message merely by colliding with the
/// current Kimi Code question or plan option-id namespace.
pub(super) fn classify_kimi_acp_v1_interaction(
    title: &str,
    options: &[ProviderInteractionMessageOption],
) -> ProviderInteractionType {
    if options.is_empty() {
        return ProviderInteractionType::Unknown;
    }
    let Some(intents) = options
        .iter()
        .map(|option| KimiAcpV1PermissionIntent::parse(option.intent.as_deref()))
        .collect::<Option<Vec<_>>>()
    else {
        return ProviderInteractionType::Unknown;
    };
    if !intents.iter().any(|intent| intent.is_allow()) {
        return ProviderInteractionType::RejectOnly;
    }

    let question_shape = title == "AskUserQuestion"
        && options.iter().zip(&intents).all(|(option, intent)| {
            matches!(intent, KimiAcpV1PermissionIntent::AllowOnce)
                && kimi_acp_v1_indexed_option_id(&option.id, "q0_opt_")
                || matches!(intent, KimiAcpV1PermissionIntent::RejectOnce) && option.id == "q0_skip"
        })
        && intents
            .iter()
            .any(|intent| matches!(intent, KimiAcpV1PermissionIntent::AllowOnce))
        && options.iter().any(|option| option.id == "q0_skip");
    if question_shape {
        return ProviderInteractionType::Question;
    }

    let plan_shape = title == "ExitPlanMode"
        && options.iter().zip(&intents).all(|(option, intent)| {
            matches!(intent, KimiAcpV1PermissionIntent::AllowOnce)
                && (option.id == "plan_approve"
                    || kimi_acp_v1_indexed_option_id(&option.id, "plan_opt_"))
                || matches!(intent, KimiAcpV1PermissionIntent::RejectOnce)
                    && matches!(option.id.as_str(), "plan_revise" | "plan_reject_and_exit")
        })
        && intents
            .iter()
            .any(|intent| matches!(intent, KimiAcpV1PermissionIntent::AllowOnce))
        && options.iter().any(|option| option.id == "plan_revise")
        && options
            .iter()
            .any(|option| option.id == "plan_reject_and_exit");
    if plan_shape {
        return ProviderInteractionType::PlanReview;
    }

    // These titles and option-id namespaces are reserved Kimi user-decision
    // protocols. A non-canonical or mismatched reserved shape is unknown; it
    // must never fall through into unattended tool approval.
    if matches!(title, "AskUserQuestion" | "ExitPlanMode")
        || options
            .iter()
            .any(|option| kimi_acp_v1_reserved_option_id(&option.id))
    {
        return ProviderInteractionType::Unknown;
    }

    ProviderInteractionType::ToolApproval
}

/// Decode the reviewed Kimi ACP v1 option array as one indivisible wire
/// shape. Silently dropping one malformed entry can turn a reserved question
/// or plan prompt into a tool approval, so any absent, non-string, empty, or
/// otherwise incomplete option invalidates the whole callback.
pub(super) fn decode_kimi_acp_v1_options(
    params: &serde_json::Value,
) -> Option<Vec<ProviderInteractionMessageOption>> {
    let raw = params.get("options")?.as_array()?;
    if raw.is_empty() {
        return None;
    }
    raw.iter()
        .map(|option| {
            let id = option.get("optionId")?.as_str()?.trim();
            let label = option.get("name")?.as_str()?.trim();
            let intent = option.get("kind")?.as_str()?.trim();
            if id.is_empty() || label.is_empty() || intent.is_empty() {
                return None;
            }
            Some(ProviderInteractionMessageOption {
                id: id.to_string(),
                label: label.to_string(),
                intent: Some(intent.to_string()),
            })
        })
        .collect()
}

pub(super) fn decode_kimi_acp_v1_title(params: &serde_json::Value) -> Option<&str> {
    let title = params.pointer("/toolCall/title")?.as_str()?;
    if title.is_empty() || title.trim() != title {
        return None;
    }
    Some(title)
}

pub(super) fn handle_kimi_provider_request(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    frame: &serde_json::Value,
) -> CliResult<ProviderInteractionReply> {
    let supplied_member = member;
    let params = frame.get("params").unwrap_or(frame);
    let (Some(title), Some(options)) = (
        decode_kimi_acp_v1_title(params),
        decode_kimi_acp_v1_options(params),
    ) else {
        return Ok(ProviderInteractionReply {
            result: serde_json::json!({"outcome": {"outcome": "cancelled"}}),
            claimed_response: None,
        });
    };
    let title = title.to_string();
    let interaction_type = classify_kimi_acp_v1_interaction(&title, &options);
    let provider_request_id = frame
        .get("id")
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string())
        })
        .unwrap_or_else(|| generated_id("provider-request"));
    let prompt = kimi_interaction_prompt(frame);
    // Re-read and validate the canonical ProviderRuntimeProjection before any callback branch
    // can acknowledge provider work. In particular, the bounded full-access
    // receipt below must never be written from a stale callback snapshot.
    let member = ledger
        .latest_member_run(&supplied_member.id)?
        .ok_or_else(|| CliError::Usage(format!("member run {} not found", supplied_member.id)))?;
    validate_provider_callback_drift(supplied_member, &member)?;

    // Kimi exposes no native narrow sandbox. Only an exact allow intent on an
    // exact current FullAccess AgentSession is therefore provably in-ceiling.
    // Option ids and labels are untrusted display strings and never grant
    // authority.
    if interaction_type == ProviderInteractionType::ToolApproval {
        let session = require_member_provider_session_authority(ledger, &member, true)?;
        if session.effective_permission_ceiling
            != harness_core::agentfirm_api::PermissionCeiling::FullAccess
        {
            return Err(CliError::Usage(format!(
                "PROVIDER_PERMISSION_MISMATCH: Kimi permission callback cannot widen frozen {:?} session {}",
                session.effective_permission_ceiling, session.id
            )));
        }
        if let Some(option_id) = options
            .iter()
            .find(|option| option.intent.as_deref() == Some("allow_always"))
            .or_else(|| {
                options
                    .iter()
                    .find(|option| option.intent.as_deref() == Some("allow_once"))
            })
            .map(|option| option.id.clone())
        {
            ledger.append_provider_control_receipt_once(
                &member,
                "Kimi full-access tool permission acknowledged",
                "provider exposed an exact allow intent; the current frozen full-access AgentSession acknowledged it directly",
            )?;
            return Ok(ProviderInteractionReply {
                result: serde_json::json!({
                    "outcome": {"outcome": "selected", "optionId": option_id}
                }),
                claimed_response: None,
            });
        }
    }
    // A permission callback without an exact allow intent cannot widen the
    // frozen AgentSession ceiling. Reject-only and unknown requests fail closed
    // in-process; only real user questions and plan-review prompts become
    // correlated Messages.
    if matches!(
        interaction_type,
        ProviderInteractionType::RejectOnly | ProviderInteractionType::Unknown
    ) {
        return Ok(ProviderInteractionReply {
            result: serde_json::json!({"outcome": {"outcome": "cancelled"}}),
            claimed_response: None,
        });
    }
    let prompt = if prompt.trim().is_empty() {
        format!("{title} requires a decision")
    } else {
        prompt
    };
    let (request, created) = provider_interaction_request_message(
        ledger,
        &member,
        &provider_request_id,
        "session/request_permission",
        interaction_type,
        prompt.clone(),
        options,
    )?;
    if created {
        ledger.append_action(
            &member.id,
            "waiting_for_input",
            MemberActionStatus::Started,
            &title,
            &prompt,
        )?;
    }

    let waiting =
        match transition_provider_interaction_member(ledger, &member, MemberRunStatus::Waiting)? {
            ProviderInteractionMemberTransition::Applied(waiting) => waiting,
            ProviderInteractionMemberTransition::LifecycleSuperseded => {
                unreachable!(
                    "entering Waiting never treats lifecycle change as a resumable outcome"
                )
            }
        };

    let resolved = wait_for_provider_interaction_response(ledger, &member, &request)?;
    let lifecycle_superseded =
        match transition_provider_interaction_member(ledger, &waiting, MemberRunStatus::Running)? {
            ProviderInteractionMemberTransition::Applied(_) => false,
            ProviderInteractionMemberTransition::LifecycleSuperseded => true,
        };
    let (response, claimed_response) = match resolved {
        Some((response, claimed_response)) if !lifecycle_superseded => {
            (Some(response), claimed_response)
        }
        _ => (None, None),
    };
    ledger.append_action(
        &member.id,
        "provider_question_resolved",
        if response.is_some() {
            MemberActionStatus::Succeeded
        } else {
            MemberActionStatus::Cancelled
        },
        &title,
        if response.is_some() {
            "correlated provider answer received"
        } else {
            "provider question cancelled by lifecycle"
        },
    )?;
    Ok(ProviderInteractionReply {
        result: match response.and_then(|response| response.choice) {
            Some(option_id) => serde_json::json!({
                "outcome": {"outcome": "selected", "optionId": option_id}
            }),
            None => serde_json::json!({"outcome": {"outcome": "cancelled"}}),
        },
        claimed_response,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberRoundResult {
    Done,
    Blocked,
    Failed,
}

pub(super) fn parse_round_result(final_text: &str) -> MemberRoundResult {
    match extract_report_section(final_text, "RESULT") {
        Some(section) => {
            let lower = section.to_lowercase();
            if lower.contains("blocked") {
                MemberRoundResult::Blocked
            } else if lower.contains("fail") {
                MemberRoundResult::Failed
            } else {
                MemberRoundResult::Done
            }
        }
        None => MemberRoundResult::Done,
    }
}

/// Return the final structured member report when a provider stream contains
/// interim assistant prose or more than one report. Reports that predate the
/// `## RESULT` contract remain readable as their original trimmed text.
pub(super) fn canonical_member_report_text(text: &str) -> &str {
    let upper = text.to_ascii_uppercase();
    let marker = "## RESULT";
    let last_result = upper
        .match_indices(marker)
        .filter_map(|(start, _)| {
            let heading_tail = &text[start + marker.len()..];
            let (same_line_tail, has_line_break) = heading_tail
                .split_once('\n')
                .map(|(tail, _)| (tail.trim_end_matches('\r'), true))
                .unwrap_or((heading_tail, false));
            (has_line_break && same_line_tail.trim().is_empty()).then_some(start)
        })
        .last();
    last_result
        .map(|start| text[start..].trim())
        .unwrap_or_else(|| text.trim())
}

/// Loose `## <NAME>` section extractor: the trimmed body between the heading
/// (matched case-insensitively) and the next `## ` heading or EOF.
pub(super) fn extract_report_section(text: &str, name: &str) -> Option<String> {
    let text = canonical_member_report_text(text);
    let marker = format!("## {name}").to_uppercase();
    let mut in_section = false;
    let mut body = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_uppercase().starts_with(&marker) {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with("## ") {
            break;
        }
        if in_section {
            body.push(line);
        }
    }
    if !in_section {
        return None;
    }
    let joined = body.join("\n").trim().to_string();
    (!joined.is_empty()).then_some(joined)
}

/// The delivery-contract prompt every member's first round runs on.
pub(super) struct MemberCollaborationEnvelope {
    pub(super) harness_bin: Option<String>,
    pub(super) execution_space_id: Option<String>,
    pub(super) project_id: Option<String>,
    pub(super) project_selector: Option<String>,
    pub(super) mission_id: Option<String>,
    pub(super) team_run_id: String,
    pub(super) member_run_id: String,
    pub(super) work_id: Option<String>,
    pub(super) work_version: Option<u64>,
    pub(super) roster: Vec<ProviderRuntimeProjection>,
}

impl MemberCollaborationEnvelope {
    pub(super) fn environment(
        &self,
        mut capability_environment: harness_runtime_contract::CollaborationCapabilityEnvironment,
    ) -> harness_runtime_contract::CollaborationCapabilityEnvironment {
        let mut values = vec![
            ("FIRM_TEAM_RUN_ID".to_string(), self.team_run_id.clone()),
            ("FIRM_MEMBER_RUN_ID".to_string(), self.member_run_id.clone()),
            ("HARNESS_TEAM_RUN_ID".to_string(), self.team_run_id.clone()),
            (
                "HARNESS_MEMBER_RUN_ID".to_string(),
                self.member_run_id.clone(),
            ),
        ];
        for (suffix, value) in [
            ("BIN", self.harness_bin.as_deref()),
            ("SPACE", self.execution_space_id.as_deref()),
            (
                "PROJECT",
                self.project_selector
                    .as_deref()
                    .or(self.project_id.as_deref()),
            ),
            ("PROJECT_ID", self.project_id.as_deref()),
            ("MISSION_ID", self.mission_id.as_deref()),
            ("WORK_ID", self.work_id.as_deref()),
        ] {
            if let Some(value) = value {
                values.push((format!("FIRM_{suffix}"), value.to_string()));
                values.push((format!("HARNESS_{suffix}"), value.to_string()));
            }
        }
        if let Some(version) = self.work_version {
            values.push(("FIRM_WORK_VERSION".to_string(), version.to_string()));
            values.push(("HARNESS_WORK_VERSION".to_string(), version.to_string()));
        }
        capability_environment.extend_non_secret(values);
        capability_environment
    }
}

pub(super) fn collaboration_capability_envelope(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    session: &harness_core::agentfirm_api::AgentSession,
    role_action_token: &str,
    mechanism: harness_runtime_contract::CollaborationCapabilityMechanism,
) -> CliResult<harness_runtime_contract::CollaborationCapabilityEnvelope> {
    ledger.require_supervisor_lease()?;
    if session.agent_member_id != member.agent_member_id {
        return Err(CliError::Usage(
            "COLLABORATION_CAPABILITY_SESSION_MISMATCH: AgentSession is not owned by the exact AgentMember"
                .into(),
        ));
    }
    let secret =
        harness_runtime_contract::CollaborationCapabilitySecret::new(role_action_token.to_string())
            .map_err(|error| CliError::Usage(error.to_string()))?;
    harness_runtime_contract::CollaborationCapabilityEnvelope::new(
        secret,
        harness_runtime_contract::CollaborationCapabilityBinding {
            team_run_id: ledger.run_id.clone(),
            member_run_id: member.id.clone(),
            member_run_generation: member.runtime_generation,
            agent_session_id: session.id.clone(),
            agent_session_generation: session.runtime_generation,
            node_daemon_id: session.node_daemon_id.clone(),
            node_daemon_generation: session.node_daemon_generation,
            supervisor_id: ledger.supervisor_id.clone(),
            supervisor_generation: ledger.supervisor_generation,
        },
        mechanism,
    )
    .map_err(|error| CliError::Usage(error.to_string()))
}

pub(super) fn member_collaboration_envelope(
    ledger: &TeamRunLedger,
    execution_space_id: Option<&str>,
    project_id: Option<&str>,
    project_selector: Option<&str>,
    member: &ProviderRuntimeProjection,
) -> CliResult<MemberCollaborationEnvelope> {
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let roster = latest_member_runs_in_append_order(&ledger.store)?
        .into_iter()
        .filter(|candidate| candidate.team_run_id == ledger.run_id)
        .collect();
    Ok(MemberCollaborationEnvelope {
        harness_bin: std::env::current_exe()
            .ok()
            .map(|path| path.to_string_lossy().into_owned()),
        execution_space_id: execution_space_id.map(str::to_string),
        project_id: project_id.map(str::to_string),
        project_selector: project_selector.map(str::to_string),
        mission_id: team_run_mission_id(&ledger.store, &run)?,
        team_run_id: run.id,
        member_run_id: member.id.clone(),
        work_id: None,
        work_version: None,
        roster,
    })
}

pub(super) fn member_work_collaboration_envelope(
    ledger: &TeamRunLedger,
    execution_space_id: Option<&str>,
    project_id: Option<&str>,
    project_selector: Option<&str>,
    member: &ProviderRuntimeProjection,
    work: Option<&Work>,
) -> CliResult<MemberCollaborationEnvelope> {
    let mut envelope = member_collaboration_envelope(
        ledger,
        execution_space_id,
        project_id,
        project_selector,
        member,
    )?;
    envelope.work_id = work.map(|work| work.id.clone());
    envelope.work_version = work.map(|work| work.version);
    Ok(envelope)
}

pub(crate) fn team_messages_prompt(
    introduction: &str,
    messages: &[TeamMessageProjection],
) -> String {
    let mut prompt = format!("{introduction}\n\n");
    for message in messages {
        prompt.push_str(&format!(
            "--- {} ({}, message_id={}, correlation_id={}{}) ---\n{}\n",
            message.sender_runtime_id,
            team_message_kind_label(&message.kind),
            message.id,
            message.correlation_id,
            message
                .work_id
                .as_deref()
                .map(|work_id| format!(", work_id={work_id}"))
                .unwrap_or_default(),
            message.body
        ));
        if let Some(sender) = message.sender.as_ref() {
            let reply_command = crate::collaboration::member_operating_contract::render_incoming_message_reply_command(
                &sender.id,
                &message.correlation_id,
                &message.id,
                message.work_id.as_deref(),
            );
            prompt.push_str(&format!("Reply canonically with: {reply_command}\n"));
        } else {
            prompt.push_str(
                "This historical Message has no typed sender identity. Resolve the sender's stable AgentIdentity from the Team roster before replying.\n",
            );
        }
        prompt.push('\n');
    }
    prompt
}

pub(super) fn work_contract_prompt(
    objective: &str,
    member: &ProviderRuntimeProjection,
    work: &Work,
    envelope: &MemberCollaborationEnvelope,
) -> String {
    let owned_paths = if member.owned_paths.is_empty() {
        "(none — read-only)".to_string()
    } else {
        member.owned_paths.join(", ")
    };
    let roster = envelope
        .roster
        .iter()
        .map(|peer| {
            format!(
                "- {}: {} ({}, provider {})",
                peer.id, peer.name, peer.role, peer.provider
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let member_operating_contract =
        crate::collaboration::member_operating_contract::MemberOperatingContract::new(&work.id)
            .render_provider_prompt();
    format!(
        "You are {name}, the {role} member of Agent Team run \"{objective}\".\n\
         \n\
         CURRENT WORK (the Works board is the sole ownership authority)\n\
         - TeamRun: {team_run_id}\n\
         - Member coordination record (not provider runtime authority): {member_run_id}\n\
         - Work: {work_id} version {work_version}\n\
         - Title: {title}\n\
         - Context:\n{work_context}\n\
         - Completion criteria:\n{criteria}\n\
         - Owned paths: {owned_paths}\n\
         \n\
         TEAM ROSTER\n{roster}\n\
         \n\
         OPERATING CONTRACT\n\
         - Do NOT use EnterPlanMode, ExitPlanMode, or any provider-native plan gate. Harness has no Plan Gate; discuss a Markdown plan through the Work-linked conversation when the Host requests one.\n\
         - Before implementation, mark this assigned Work in progress:\n\
           \"$FIRM_BIN\" member work start --work-id {work_id} --expected-version {work_version}\n\
         - Read the board: \"$FIRM_BIN\" team-run work list --team-run-id {team_run_id}\n\
         - Inspect the latest version before every transition: \"$FIRM_BIN\" team-run work show --work-id {work_id}\n\
         - Ordinary canonical Message is conversation only. Messages never change Work ownership or status. Link each discussion to the exact Work being discussed; your current Work is {work_id}. A Host assigning, retrying, or reviewing another member must use that member's exact Work id from the board, never the Host Work id.\n\
         - Read actionable mail through the same exact-self Supervisor binding with: \"$FIRM_BIN\" member inbox --all --json.\n\
{member_operating_contract}\n\
         - If blocked, inspect the latest version, then run: \"$FIRM_BIN\" member work block --work-id {work_id} --expected-version <latest-version> --reason '<reason>'; follow it with a concise Work-linked Message. Resume with member work resume and the next exact version.\n\
         - When complete, inspect the latest version, then run: \"$FIRM_BIN\" member work submit --work-id {work_id} --expected-version <latest-version> --result-summary '<result>' --candidate-revision '<exact-revision>' --artifact-ref '<artifact>' --check-ref '<check>'. Host acceptance, not provider completion, moves Work to done.\n\
         - You may propose scoped follow-up Work, and may use provider-native subagents as implementation details.\n\
         - Do not deploy, push, merge, or perform sensitive external actions unless the Host explicitly gave that authority.\n\
         \n\
         Your final provider message is a concise conversational update; durable completion belongs in Work, not an automatic Handoff message.",
        name = member.name,
        role = member.role,
        team_run_id = envelope.team_run_id,
        member_run_id = envelope.member_run_id,
        work_id = work.id,
        work_version = work.version,
        title = work.title,
        work_context = work.context_markdown,
        criteria = work.completion_criteria_markdown,
    )
}

pub(super) fn active_work_continuation_prompt(
    objective: &str,
    member: &ProviderRuntimeProjection,
    work: &Work,
    envelope: &MemberCollaborationEnvelope,
) -> String {
    if work.phase == WorkPhase::Open
        && work.condition == WorkCondition::Normal
        && work.owner_member_id.is_none()
    {
        return format!(
            "SHARED WORK AVAILABLE\n\
             Work {work_id} version {work_version} is ready on the Team Works board and this Member is eligible to claim it. No canonical Work delivery or Assignment RegistryMessage was created. Inspect the latest board, then use the bound member CLI to claim it atomically before doing any work; if the claim loses a race, refresh the board and do not duplicate effects.\n\n{}",
            work_contract_prompt(objective, member, work, envelope),
            work_id = work.id,
            work_version = work.version,
        );
    }
    format!(
        "ACTIVE WORK CONTINUATION\n\
         Work {work_id} version {work_version} is still assigned to this ProviderRuntimeProjection and has not reached review or a terminal state. No new canonical Work delivery or TeamMessageProjection was created: continue the same durable responsibility in the same provider-native session. Inspect the native session and Workspace before acting so you do not duplicate completed effects.\n\n{}",
        work_contract_prompt(objective, member, work, envelope),
        work_id = work.id,
        work_version = work.version,
    )
}

pub(super) fn parse_unix_ms(value: &str) -> Option<u128> {
    value.strip_prefix("unix-ms:")?.parse().ok()
}
