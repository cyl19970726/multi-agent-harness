use super::*;
use harness_application::{
    prepare_message_authoring, MessageAuthoringError, MessageAuthoringIntent,
    MessageAuthoringOperation, PrepareMessageAuthoringCommand, PreparedMessageAuthoring,
};

pub(super) fn prepare_canonical_message(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    team_run_id: &str,
    operation: &str,
    intent: RoleActionIntent,
) -> Result<PreparedMessageAuthoring, StoreError> {
    let operation = match operation {
        "send" => MessageAuthoringOperation::Send,
        "reply" => MessageAuthoringOperation::Reply,
        "request-decision" => MessageAuthoringOperation::RequestDecision,
        _ => return Err(message_route_mismatch(team_run_id)),
    };
    let intent = match intent {
        RoleActionIntent::SendMessage {
            recipient_ids,
            body,
            work_id,
            evidence_refs,
            response_required,
        } => MessageAuthoringIntent::Send {
            recipient_ids,
            body,
            work_id,
            evidence_refs,
            response_required,
        },
        RoleActionIntent::ReplyMessage {
            recipient_ids,
            body,
            correlation_id,
            causation_id,
            work_id,
            evidence_refs,
            response_required,
        } => MessageAuthoringIntent::Reply {
            recipient_ids,
            body,
            correlation_id,
            causation_id,
            work_id,
            evidence_refs,
            response_required,
        },
        RoleActionIntent::RequestDecision {
            body,
            work_id,
            evidence_refs,
        } => MessageAuthoringIntent::RequestDecision {
            body,
            work_id,
            evidence_refs,
        },
        _ => return Err(message_route_mismatch(team_run_id)),
    };
    let requested_work_id = match &intent {
        MessageAuthoringIntent::Send { work_id, .. }
        | MessageAuthoringIntent::Reply { work_id, .. }
        | MessageAuthoringIntent::RequestDecision { work_id, .. } => work_id.as_deref(),
    };
    let (_run, team) = team_for_run(store, team_run_id)?;
    let current_team_revision = store
        .teams()?
        .into_iter()
        .filter(|candidate| candidate.id == team.id)
        .count() as u64;
    let member_runs = store
        .trust_member_runs(&auth.execution_space_id)?
        .into_iter()
        .filter(|run| run.team_run_id == team_run_id)
        .collect::<Vec<_>>();
    let linked_work = match requested_work_id {
        Some(work_id) => store
            .latest_works()?
            .into_iter()
            .find(|work| work.id == work_id),
        None => None,
    };
    let command = PrepareMessageAuthoringCommand {
        operation,
        team_id: team.id.clone(),
        team_run_id: team_run_id.to_string(),
        host_agent_member_id: team.host_agent_id,
        team_member_ids: team.member_ids,
        current_team_revision,
        expected_team_revision: auth.expected_version,
        actor: auth.actor.clone(),
        authorized_authority_actors: auth.authorized_authority_actors.clone(),
        idempotency_key: auth.idempotency_key.clone(),
        intent,
        member_runs,
        memberships: store.fabric_team_memberships(&auth.execution_space_id)?,
        subscriptions: store.fabric_message_subscriptions(&auth.execution_space_id)?,
        linked_work,
    };
    prepare_message_authoring(command)
        .map_err(|error| map_message_authoring_error(error, team_run_id, &team.id))
}

fn message_route_mismatch(team_run_id: &str) -> StoreError {
    encoded_error(
        "INVALID_STATE_TRANSITION",
        "semantic action does not match message route",
        "team_run",
        team_run_id,
        None,
    )
}

fn map_message_authoring_error(
    error: MessageAuthoringError,
    team_run_id: &str,
    team_id: &str,
) -> StoreError {
    match error {
        MessageAuthoringError::UnauthorizedSender => encoded_error(
            "UNAUTHORIZED_ACTOR",
            "message sender must be the exact Team Host or one active Team Member",
            "team_run",
            team_run_id,
            None,
        ),
        MessageAuthoringError::SenderIdentityConflict { matches: _ } => encoded_error(
            "UNAUTHORIZED_ACTOR",
            "message sender must be the exact Team Host or one active Team Member",
            "team_run",
            team_run_id,
            None,
        ),
        MessageAuthoringError::TeamRevisionConflict { current_revision } => encoded_error(
            "VERSION_CONFLICT",
            "Team Message requires the exact current Team revision",
            "team",
            team_id,
            Some(current_revision),
        ),
        MessageAuthoringError::IntentRouteMismatch => message_route_mismatch(team_run_id),
        MessageAuthoringError::WorkNotFound { work_id } => encoded_error(
            "INVALID_STATE_TRANSITION",
            "Work does not exist",
            "work",
            &work_id,
            None,
        ),
        MessageAuthoringError::WorkOutsideTeamRun { work_id, version } => encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Work does not belong to the addressed TeamRun",
            "work",
            &work_id,
            Some(version),
        ),
        MessageAuthoringError::UnauthorizedWorkLink { work_id, version } => encoded_error(
            "UNAUTHORIZED_ACTOR",
            "member-owned Work mutation requires the exact accountable AgentMember and current active WorkExecutionBinding",
            "work",
            &work_id,
            Some(version),
        ),
        MessageAuthoringError::BodyOrRecipientsRequired => encoded_error(
            "INVALID_STATE_TRANSITION",
            "message body and recipients are required",
            "team_run",
            team_run_id,
            None,
        ),
        MessageAuthoringError::RecipientOutsideTeam { recipient_id: _ } => encoded_error(
            "UNAUTHORIZED_ACTOR",
            "every message recipient must belong to the exact Team",
            "team_run",
            team_run_id,
            None,
        ),
        MessageAuthoringError::RecipientRouteUnavailable { recipient_id } => encoded_error(
            "MESSAGE_ROUTE_UNAVAILABLE",
            "recipient requires one active canonical TeamMembership and MessageSubscription",
            "agent_identity",
            &recipient_id,
            None,
        ),
        MessageAuthoringError::RecipientRuntimeAmbiguous { recipient_id } => encoded_error(
            "AGENT_SESSION_AMBIGUOUS",
            "message recipient requires exactly one active Team MemberRun, including the Host",
            "agent_identity",
            &recipient_id,
            None,
        ),
    }
}
