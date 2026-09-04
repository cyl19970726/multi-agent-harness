use super::*;

pub(super) fn team_run_host_message_command(
    store: &HarnessStore,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(args, "team-run message send")?;
    if args[0] != "send" {
        return Err(CliError::Usage(format!(
            "unknown team-run message command: {}; expected send",
            args[0]
        )));
    }

    let team_run_id = required(args, "--team-run-id")?;
    let membership_id = required(args, "--to-membership")?;
    let body = required(args, "--body")?;
    if body.trim().is_empty() {
        return Err(CliError::Usage("--body must be non-empty Markdown".into()));
    }
    let surface = required(args, "--surface")?;
    let thread_id = required(args, "--thread-id")?;
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
        format!(
            "team-run-host-message:{}",
            harness_store::canonical_json_fingerprint(&serde_json::json!({
                "team_run_id": team_run_id,
                "host_surface": canonical_surface(&surface),
                "host_thread_id": thread_id,
                "body_digest": body_digest,
            }))
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
    let intent = serde_json::json!({
        "action": "send_message",
        "recipient_ids": [recipient.agent_member_id],
        "body": body,
        "work_id": value(args, "--work-id"),
        "response_required": has_flag(args, "--response-required"),
    });
    let route = format!("/v1/agentfirm/team-runs/{team_run_id}/messages/send");
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
