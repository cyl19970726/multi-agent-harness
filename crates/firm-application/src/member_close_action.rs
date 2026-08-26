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
    pub runtime_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedMemberClose {
    pub team_run_id: String,
    pub member_run_id: String,
    pub requested_by: String,
    pub runtime_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberCloseRuntimeFacts {
    pub runtime_generation: u64,
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
    RuntimeGenerationMismatch { current_runtime_generation: u64 },
}

/// Authorize the canonical reversible Close intent before reading provider
/// runtime facts. This preserves confirmation, actor, version, and lifecycle
/// error priority even when the provider projection is absent or stale.
///
/// The application layer owns actor, confirmation, version and lifecycle
/// policy. The caller still owns authoritative fact collection and the actual
/// Supervisor/Store transaction. In particular, a managed plan is not a
/// provider Close receipt and cannot authorize a Closed/Stopped projection.
pub fn authorize_member_close(
    command: PrepareMemberCloseCommand,
    facts: MemberCloseFacts,
) -> Result<AuthorizedMemberClose, MemberCloseActionError> {
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

    Ok(AuthorizedMemberClose {
        team_run_id: facts.team_run_id,
        member_run_id: facts.member_run_id,
        requested_by: command.actor.id,
        runtime_generation: facts.runtime_generation,
    })
}

/// Bind an authorized Close to the exact provider runtime generation and
/// return an effect-free plan. This is not a provider Close receipt.
pub fn prepare_member_close(
    authorized: AuthorizedMemberClose,
    runtime: MemberCloseRuntimeFacts,
) -> Result<PreparedMemberClose, MemberCloseActionError> {
    if runtime.runtime_generation != authorized.runtime_generation {
        return Err(MemberCloseActionError::RuntimeGenerationMismatch {
            current_runtime_generation: authorized.runtime_generation,
        });
    }
    Ok(PreparedMemberClose {
        team_run_id: authorized.team_run_id,
        member_run_id: authorized.member_run_id,
        requested_by: authorized.requested_by,
        runtime_kind: runtime.runtime_kind,
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

    fn facts() -> MemberCloseFacts {
        MemberCloseFacts {
            member_run_id: "member-run-a".into(),
            team_run_id: "team-run-a".into(),
            agent_member_id: "member-a".into(),
            host_agent_member_id: "host-a".into(),
            current_version: 4,
            coordination_status: MemberCoordinationStatus::Active,
            runtime_generation: 2,
        }
    }

    fn runtime(runtime_kind: MemberCloseRuntimeKind) -> MemberCloseRuntimeFacts {
        MemberCloseRuntimeFacts {
            runtime_generation: 2,
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
            let authorized = authorize_member_close(command, facts()).expect("authorized close");
            let prepared =
                prepare_member_close(authorized, runtime(MemberCloseRuntimeKind::Managed))
                    .expect("exact runtime plan");
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
            authorize_member_close(missing_confirmation, facts()),
            Err(MemberCloseActionError::ConfirmationRequired)
        );

        assert_eq!(
            authorize_member_close(command("outsider-a"), facts()),
            Err(MemberCloseActionError::UnauthorizedActor)
        );

        let mut stale = command("member-a");
        stale.expected_version = 3;
        assert_eq!(
            authorize_member_close(stale, facts()),
            Err(MemberCloseActionError::VersionConflict { current_version: 4 })
        );

        let mut retired = facts();
        retired.coordination_status = MemberCoordinationStatus::Retired;
        assert_eq!(
            authorize_member_close(command("member-a"), retired),
            Err(MemberCloseActionError::RetiredMemberRun { current_version: 4 })
        );
    }

    #[test]
    fn route_identity_and_runtime_ownership_are_explicit_without_effect_authority() {
        let mut mismatched = command("member-a");
        mismatched.member_run_id = "member-run-b".into();
        assert_eq!(
            authorize_member_close(mismatched, facts()),
            Err(MemberCloseActionError::MemberRunMismatch)
        );

        let authorized = authorize_member_close(command("member-a"), facts())
            .expect("authorized external close");
        let prepared = prepare_member_close(
            authorized,
            runtime(MemberCloseRuntimeKind::ExternalInteractive),
        )
        .expect("external close plan");
        assert_eq!(
            prepared.runtime_kind,
            MemberCloseRuntimeKind::ExternalInteractive
        );
    }

    #[test]
    fn generation_drift_cannot_override_confirmation_actor_or_version_priority() {
        let drifted_runtime = MemberCloseRuntimeFacts {
            runtime_generation: 3,
            runtime_kind: MemberCloseRuntimeKind::Managed,
        };

        let mut missing_confirmation = command("outsider-a");
        missing_confirmation.confirmation = None;
        assert_eq!(
            authorize_member_close(missing_confirmation, facts()),
            Err(MemberCloseActionError::ConfirmationRequired)
        );
        assert_eq!(
            authorize_member_close(command("outsider-a"), facts()),
            Err(MemberCloseActionError::UnauthorizedActor)
        );
        let mut stale = command("member-a");
        stale.expected_version = 3;
        assert_eq!(
            authorize_member_close(stale, facts()),
            Err(MemberCloseActionError::VersionConflict { current_version: 4 })
        );

        let authorized = authorize_member_close(command("member-a"), facts())
            .expect("valid canonical authorization");
        assert_eq!(
            prepare_member_close(authorized, drifted_runtime),
            Err(MemberCloseActionError::RuntimeGenerationMismatch {
                current_runtime_generation: 2
            })
        );
    }
}
