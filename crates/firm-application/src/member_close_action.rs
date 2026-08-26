use firm_core::agentfirm_api::{ActorKind, ActorRef, MemberCoordinationStatus};

pub const MEMBER_CLOSE_CONFIRMATION: &str = "close_member_run";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberCloseRuntimeKind {
    Managed,
    ExternalInteractive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareMemberCloseCommand {
    pub member_run_id: String,
    pub actor: ActorRef,
    pub authorized_authority_actors: Vec<ActorRef>,
    pub expected_version: u64,
    pub confirmation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberCloseFacts {
    pub member_run_id: String,
    pub team_run_id: String,
    pub agent_member_id: String,
    pub host_agent_member_id: String,
    pub current_version: u64,
    pub coordination_status: MemberCoordinationStatus,
    pub runtime_kind: MemberCloseRuntimeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMemberClose {
    pub team_run_id: String,
    pub member_run_id: String,
    pub requested_by: String,
    pub runtime_kind: MemberCloseRuntimeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberCloseActionError {
    ConfirmationRequired,
    MemberRunMismatch,
    UnauthorizedActor,
    VersionConflict { current_version: u64 },
    RetiredMemberRun { current_version: u64 },
}

/// Resolve the canonical reversible Close intent into an effect-free plan.
///
/// The application layer owns actor, confirmation, version and lifecycle
/// policy. The caller still owns authoritative fact collection and the actual
/// Supervisor/Store transaction. In particular, a managed plan is not a
/// provider Close receipt and cannot authorize a Closed/Stopped projection.
pub fn prepare_member_close(
    command: PrepareMemberCloseCommand,
    facts: MemberCloseFacts,
) -> Result<PreparedMemberClose, MemberCloseActionError> {
    if command.confirmation.as_deref() != Some(MEMBER_CLOSE_CONFIRMATION) {
        return Err(MemberCloseActionError::ConfirmationRequired);
    }
    if command.member_run_id != facts.member_run_id {
        return Err(MemberCloseActionError::MemberRunMismatch);
    }

    let is_self =
        command.actor.kind == ActorKind::AgentMember && command.actor.id == facts.agent_member_id;
    let is_host = (command.actor.kind == ActorKind::AgentMember
        && command.actor.id == facts.host_agent_member_id)
        || command.authorized_authority_actors.iter().any(|actor| {
            actor.kind == ActorKind::AgentMember && actor.id == facts.host_agent_member_id
        });
    if !is_self && !is_host {
        return Err(MemberCloseActionError::UnauthorizedActor);
    }
    if command.expected_version != facts.current_version {
        return Err(MemberCloseActionError::VersionConflict {
            current_version: facts.current_version,
        });
    }
    if facts.coordination_status == MemberCoordinationStatus::Retired {
        return Err(MemberCloseActionError::RetiredMemberRun {
            current_version: facts.current_version,
        });
    }

    Ok(PreparedMemberClose {
        team_run_id: facts.team_run_id,
        member_run_id: facts.member_run_id,
        requested_by: command.actor.id,
        runtime_kind: facts.runtime_kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(id: &str) -> ActorRef {
        ActorRef {
            kind: ActorKind::AgentMember,
            id: id.into(),
        }
    }

    fn command(actor_id: &str) -> PrepareMemberCloseCommand {
        PrepareMemberCloseCommand {
            member_run_id: "member-run-a".into(),
            actor: actor(actor_id),
            authorized_authority_actors: Vec::new(),
            expected_version: 4,
            confirmation: Some(MEMBER_CLOSE_CONFIRMATION.into()),
        }
    }

    fn facts(runtime_kind: MemberCloseRuntimeKind) -> MemberCloseFacts {
        MemberCloseFacts {
            member_run_id: "member-run-a".into(),
            team_run_id: "team-run-a".into(),
            agent_member_id: "member-a".into(),
            host_agent_member_id: "host-a".into(),
            current_version: 4,
            coordination_status: MemberCoordinationStatus::Active,
            runtime_kind,
        }
    }

    #[test]
    fn self_member_exact_host_and_delegated_host_authority_prepare_the_same_close() {
        for (mut command, requested_by) in [
            (command("member-a"), "member-a"),
            (command("host-a"), "host-a"),
            (
                {
                    let mut command = command("host-session-a");
                    command.authorized_authority_actors = vec![actor("host-a")];
                    command
                },
                "host-session-a",
            ),
        ] {
            command.confirmation = Some(MEMBER_CLOSE_CONFIRMATION.into());
            let prepared = prepare_member_close(command, facts(MemberCloseRuntimeKind::Managed))
                .expect("authorized close");
            assert_eq!(prepared.team_run_id, "team-run-a");
            assert_eq!(prepared.member_run_id, "member-run-a");
            assert_eq!(prepared.requested_by, requested_by);
            assert_eq!(prepared.runtime_kind, MemberCloseRuntimeKind::Managed);
        }
    }

    #[test]
    fn confirmation_actor_version_and_retired_fences_are_typed_and_ordered() {
        let mut missing_confirmation = command("outsider-a");
        missing_confirmation.confirmation = None;
        assert_eq!(
            prepare_member_close(missing_confirmation, facts(MemberCloseRuntimeKind::Managed)),
            Err(MemberCloseActionError::ConfirmationRequired)
        );

        assert_eq!(
            prepare_member_close(
                command("outsider-a"),
                facts(MemberCloseRuntimeKind::Managed)
            ),
            Err(MemberCloseActionError::UnauthorizedActor)
        );

        let mut stale = command("member-a");
        stale.expected_version = 3;
        assert_eq!(
            prepare_member_close(stale, facts(MemberCloseRuntimeKind::Managed)),
            Err(MemberCloseActionError::VersionConflict { current_version: 4 })
        );

        let mut retired = facts(MemberCloseRuntimeKind::Managed);
        retired.coordination_status = MemberCoordinationStatus::Retired;
        assert_eq!(
            prepare_member_close(command("member-a"), retired),
            Err(MemberCloseActionError::RetiredMemberRun { current_version: 4 })
        );
    }

    #[test]
    fn route_identity_and_runtime_ownership_are_explicit_without_effect_authority() {
        let mut mismatched = command("member-a");
        mismatched.member_run_id = "member-run-b".into();
        assert_eq!(
            prepare_member_close(mismatched, facts(MemberCloseRuntimeKind::Managed)),
            Err(MemberCloseActionError::MemberRunMismatch)
        );

        let prepared = prepare_member_close(
            command("member-a"),
            facts(MemberCloseRuntimeKind::ExternalInteractive),
        )
        .expect("external close plan");
        assert_eq!(
            prepared.runtime_kind,
            MemberCloseRuntimeKind::ExternalInteractive
        );
    }
}
