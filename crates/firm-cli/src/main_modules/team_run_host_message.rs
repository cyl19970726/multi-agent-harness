use super::*;

pub(super) fn team_run_host_message_command(
    store: &HarnessStore,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(args, "team-run message send|reply")?;
    let is_reply = match args[0].as_str() {
        "send" => false,
        "reply" => true,
        other => {
            return Err(CliError::Usage(format!(
                "unknown team-run message command: {other}; expected send or reply"
            )))
        }
    };

    let team_run_id = required(args, "--team-run-id")?;
    let membership_id = required(args, "--to-membership")?;
    let body = required(args, "--body")?;
    if body.trim().is_empty() {
        return Err(CliError::Usage("--body must be non-empty Markdown".into()));
    }
    let surface = required(args, "--surface")?;
    let thread_id = required(args, "--thread-id")?;
    let (correlation_id, causation_id) = if is_reply {
        let correlation_id = required(args, "--correlation-id")?;
        if correlation_id.trim().is_empty() {
            return Err(CliError::Usage("--correlation-id must not be empty".into()));
        }
        let causation_id = required(args, "--causation-id")?;
        if causation_id.trim().is_empty() {
            return Err(CliError::Usage("--causation-id must not be empty".into()));
        }
        (Some(correlation_id), Some(causation_id))
    } else {
        (None, None)
    };
    let run = latest_team_run(store, &team_run_id)?;
    require_external_interactive_host_binding(&run, &surface, &thread_id)?;

    let execution_space_id = team_run_execution_space_id(store, &run)?;
    let team = store
        .latest_teams()?
        .remove(&run.agent_team_id)
        .ok_or_else(|| CliError::Usage(format!("Team not found: {}", run.agent_team_id)))?;
    let matching_memberships = store
        .fabric_team_memberships(&execution_space_id)?
        .into_iter()
        .filter(|membership| {
            membership.id == membership_id
                && membership.team_id == team.id
                && membership.node_id == team.node_id
                && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
        })
        .collect::<Vec<_>>();
    let [recipient] = matching_memberships.as_slice() else {
        return Err(CliError::Usage(format!(
            "MESSAGE_ROUTE_UNAVAILABLE: --to-membership must identify one exact active membership of Team {}",
            team.id
        )));
    };
    let current_team_revision = derive_team_revisions(&store.teams()?)
        .get(&team.id)
        .copied()
        .unwrap_or_default();
    let body_digest = harness_core::agentfirm_api::message_body_digest(&body);
    let supplied_idempotency_key = value(args, "--idempotency-key");
    if supplied_idempotency_key
        .as_deref()
        .is_some_and(|key| key.trim().is_empty())
    {
        return Err(CliError::Usage(
            "--idempotency-key must not be empty".into(),
        ));
    }
    let idempotency_key = supplied_idempotency_key.unwrap_or_else(|| {
        let fingerprint = if is_reply {
            serde_json::json!({
                "team_run_id": team_run_id,
                "host_surface": canonical_surface(&surface),
                "host_thread_id": thread_id,
                "correlation_id": correlation_id,
                "causation_id": causation_id,
                "body_digest": body_digest,
            })
        } else {
            serde_json::json!({
                "team_run_id": team_run_id,
                "host_surface": canonical_surface(&surface),
                "host_thread_id": thread_id,
                "body_digest": body_digest,
            })
        };
        format!(
            "team-run-host-message:{}",
            harness_store::canonical_json_fingerprint(&fingerprint)
        )
    });
    let auth = crate::agentfirm_api::AuthenticatedMutation {
        execution_space_id: execution_space_id.clone(),
        actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: team.host_agent_id,
        },
        authorized_authority_actors: Vec::new(),
        idempotency_key,
        expected_version: current_team_revision,
        request_fingerprint: None,
    };
    let intent = if is_reply {
        serde_json::json!({
            "action": "reply_message",
            "recipient_ids": [recipient.agent_member_id],
            "body": body,
            "correlation_id": correlation_id,
            "causation_id": causation_id,
            "work_id": value(args, "--work-id"),
            "response_required": has_flag(args, "--response-required"),
        })
    } else {
        serde_json::json!({
            "action": "send_message",
            "recipient_ids": [recipient.agent_member_id],
            "body": body,
            "work_id": value(args, "--work-id"),
            "response_required": has_flag(args, "--response-required"),
        })
    };
    let operation = if is_reply { "reply" } else { "send" };
    let route = format!("/v1/agentfirm/team-runs/{team_run_id}/messages/{operation}");
    let result =
        crate::role_actions_api::execute(store, auth, &route, &serde_json::to_vec(&intent)?, None)?;
    let message_id = result.projection["id"]
        .as_str()
        .ok_or_else(|| CliError::Usage("canonical Message result has no message id".into()))?;
    let mut delivery_ids = store
        .fabric_message_deliveries(&execution_space_id)?
        .into_iter()
        .filter(|delivery| delivery.message_id == message_id)
        .map(|delivery| delivery.id)
        .collect::<Vec<_>>();
    delivery_ids.sort();
    print_json(&serde_json::json!({
        "message_id": message_id,
        "delivery_ids": delivery_ids,
        "replayed": result.replayed,
    }))
}
