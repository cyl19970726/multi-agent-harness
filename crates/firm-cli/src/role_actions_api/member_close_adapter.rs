use super::*;
use harness_application::{
    prepare_member_close, MemberCloseActionError, MemberCloseFacts, MemberCloseRuntimeKind,
    PrepareMemberCloseCommand, PreparedMemberClose,
};

/// Parse the canonical HTTP intent, collect authoritative facts, and delegate
/// all actor/version/lifecycle policy to `firm-application`.
pub(crate) fn authorize_member_close(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    path: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<PreparedMemberClose, StoreError> {
    let Some(CanonicalRoute::MemberRun {
        member_run_id,
        operation: "close",
    }) = parse_canonical_route(path)
    else {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic Close intent does not match the exact MemberRun route",
            "route",
            path,
            None,
        ));
    };
    let RoleActionIntent::CloseMemberRun = serde_json::from_slice::<RoleActionIntent>(body)
        .map_err(|error| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                format!("invalid MemberRun Close intent: {error}"),
                "member_run",
                member_run_id,
                None,
            )
        })?
    else {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match MemberRun Close route",
            "member_run",
            member_run_id,
            None,
        ));
    };

    let (run, _, team) = team_for_member_run(store, &auth.execution_space_id, member_run_id)?;
    let runtime = store
        .member_runs()?
        .into_iter()
        .rev()
        .find(|candidate| candidate.id == member_run_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "MemberRun has no current provider runtime projection",
                "member_run",
                member_run_id,
                Some(run.version),
            )
        })?;
    if runtime.runtime_generation != run.runtime_generation {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "MemberRun Close requires the exact current provider runtime generation",
            "member_run",
            member_run_id,
            Some(run.version),
        ));
    }
    let runtime_kind = if runtime.is_external_interactive() {
        MemberCloseRuntimeKind::ExternalInteractive
    } else {
        MemberCloseRuntimeKind::Managed
    };
    let command = PrepareMemberCloseCommand {
        member_run_id: member_run_id.to_string(),
        actor: auth.actor.clone(),
        authorized_authority_actors: auth.authorized_authority_actors.clone(),
        expected_version: auth.expected_version,
        confirmation: confirmed_action.map(str::to_string),
    };
    let current_version = run.version;
    let facts = MemberCloseFacts {
        member_run_id: run.id,
        team_run_id: run.team_run_id,
        agent_member_id: run.agent_member_id,
        host_agent_member_id: team.host_agent_id,
        current_version,
        coordination_status: run.coordination_status,
        runtime_kind,
    };

    prepare_member_close(command, facts).map_err(|error| match error {
        MemberCloseActionError::ConfirmationRequired => encoded_error(
            "CONFIRMATION_REQUIRED",
            "server confirmation must exactly confirm close_member_run",
            "member_run",
            member_run_id,
            None,
        ),
        MemberCloseActionError::MemberRunMismatch => encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic Close intent does not match the exact MemberRun route",
            "member_run",
            member_run_id,
            None,
        ),
        MemberCloseActionError::UnauthorizedActor => encoded_error(
            "UNAUTHORIZED_ACTOR",
            "credential is neither this MemberRun's AgentMember nor its exact Team Host",
            "member_run",
            member_run_id,
            Some(current_version),
        ),
        MemberCloseActionError::VersionConflict { current_version } => encoded_error(
            "VERSION_CONFLICT",
            "MemberRun Close requires its exact current revision",
            "member_run",
            member_run_id,
            Some(current_version),
        ),
        MemberCloseActionError::RetiredMemberRun { current_version } => encoded_error(
            "INVALID_STATE_TRANSITION",
            "a retired MemberRun cannot be closed or reopened",
            "member_run",
            member_run_id,
            Some(current_version),
        ),
    })
}
