use firm_core::agentfirm_api::{
    ActorKind, ActorRef, MutationContext, RuntimeCommandRecord, RuntimeRecoveryResolution,
};
use firm_core::{NodeDaemonLease, NodeDaemonLeaseStatus};

pub const RUNTIME_RECOVERY_CONFIRMATION: &str = "resolve_runtime_recovery";
pub const RUNTIME_RECOVERY_COMMAND_NAME: &str = "node_daemon.runtime_command.resolve";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRuntimeRecoveryCommand {
    pub execution_space_id: String,
    pub node_id: String,
    pub command_id: String,
    pub actor: ActorRef,
    pub idempotency_key: String,
    pub request_fingerprint: Option<String>,
    pub expected_version: u64,
    pub confirmation: Option<String>,
    pub resolution: RuntimeRecoveryResolution,
    pub evidence_ref: String,
    pub observed_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRecoveryCommit {
    pub context: MutationContext,
    pub command_id: String,
    pub node_id: String,
    pub daemon_id: String,
    pub daemon_generation: u64,
    pub resolution: RuntimeRecoveryResolution,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeRecoveryActionOutcome {
    pub projection: RuntimeCommandRecord,
    pub event_id: String,
    pub resulting_version: u64,
    pub store_sequence: u64,
    pub replayed: bool,
}

pub trait RuntimeRecoveryPersistence {
    type Error;

    fn current_node_daemon_lease(
        &mut self,
        node_id: &str,
    ) -> Result<Option<NodeDaemonLease>, Self::Error>;

    fn commit_runtime_recovery(
        &mut self,
        commit: RuntimeRecoveryCommit,
    ) -> Result<RuntimeRecoveryActionOutcome, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRecoveryActionError<E> {
    UnauthorizedNodeOperator,
    ConfirmationRequired,
    CurrentNodeDaemonUnavailable,
    EvidenceRequired,
    Persistence(E),
}

pub fn resolve_runtime_recovery<P: RuntimeRecoveryPersistence>(
    persistence: &mut P,
    command: ResolveRuntimeRecoveryCommand,
) -> Result<RuntimeRecoveryActionOutcome, RuntimeRecoveryActionError<P::Error>> {
    if command.actor.kind != ActorKind::Service || command.actor.id != command.node_id {
        return Err(RuntimeRecoveryActionError::UnauthorizedNodeOperator);
    }
    if command.confirmation.as_deref() != Some(RUNTIME_RECOVERY_CONFIRMATION) {
        return Err(RuntimeRecoveryActionError::ConfirmationRequired);
    }

    let lease = persistence
        .current_node_daemon_lease(&command.node_id)
        .map_err(RuntimeRecoveryActionError::Persistence)?
        .filter(|lease| {
            lease.node_id == command.node_id
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > command.observed_unix_ms
        })
        .ok_or(RuntimeRecoveryActionError::CurrentNodeDaemonUnavailable)?;

    if command.evidence_ref.trim().is_empty() {
        return Err(RuntimeRecoveryActionError::EvidenceRequired);
    }

    let authority_actor = command.actor.clone();
    persistence
        .commit_runtime_recovery(RuntimeRecoveryCommit {
            context: MutationContext {
                execution_space_id: command.execution_space_id,
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: lease.daemon_id.clone(),
                },
                authority_actor: Some(authority_actor.clone()),
                command_name: RUNTIME_RECOVERY_COMMAND_NAME.into(),
                idempotency_key: format!(
                    "role-runtime-recovery:{}:{}",
                    authority_actor.id, command.idempotency_key
                ),
                expected_version: command.expected_version,
                request_fingerprint: command.request_fingerprint,
            },
            command_id: command.command_id,
            node_id: command.node_id,
            daemon_id: lease.daemon_id,
            daemon_generation: lease.generation,
            resolution: command.resolution,
            evidence_ref: command.evidence_ref,
        })
        .map_err(RuntimeRecoveryActionError::Persistence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakeError;

    struct FakePersistence {
        lease: Option<NodeDaemonLease>,
        lease_reads: usize,
        commits: Vec<RuntimeRecoveryCommit>,
    }

    impl RuntimeRecoveryPersistence for FakePersistence {
        type Error = FakeError;

        fn current_node_daemon_lease(
            &mut self,
            _node_id: &str,
        ) -> Result<Option<NodeDaemonLease>, Self::Error> {
            self.lease_reads += 1;
            Ok(self.lease.clone())
        }

        fn commit_runtime_recovery(
            &mut self,
            commit: RuntimeRecoveryCommit,
        ) -> Result<RuntimeRecoveryActionOutcome, Self::Error> {
            self.commits.push(commit);
            Err(FakeError)
        }
    }

    fn lease(status: NodeDaemonLeaseStatus, expires_unix_ms: u64) -> NodeDaemonLease {
        NodeDaemonLease {
            node_id: "node-a".into(),
            daemon_id: "daemon-a".into(),
            generation: 7,
            instance_id: "instance-a".into(),
            status,
            acquired_unix_ms: 1,
            renewed_unix_ms: 2,
            expires_unix_ms,
            released_unix_ms: None,
        }
    }

    fn command() -> ResolveRuntimeRecoveryCommand {
        ResolveRuntimeRecoveryCommand {
            execution_space_id: "space-a".into(),
            node_id: "node-a".into(),
            command_id: "command-a".into(),
            actor: ActorRef {
                kind: ActorKind::Service,
                id: "node-a".into(),
            },
            idempotency_key: "request-a".into(),
            request_fingerprint: Some("fingerprint-a".into()),
            expected_version: 4,
            confirmation: Some(RUNTIME_RECOVERY_CONFIRMATION.into()),
            resolution: RuntimeRecoveryResolution::ConfirmNotApplied,
            evidence_ref: "evidence-a".into(),
            observed_unix_ms: 100,
        }
    }

    fn persistence(lease: Option<NodeDaemonLease>) -> FakePersistence {
        FakePersistence {
            lease,
            lease_reads: 0,
            commits: Vec::new(),
        }
    }

    #[test]
    fn exact_node_service_authority_and_confirmation_precede_persistence() {
        for actor in [
            ActorRef {
                kind: ActorKind::Human,
                id: "node-a".into(),
            },
            ActorRef {
                kind: ActorKind::AgentMember,
                id: "node-a".into(),
            },
            ActorRef {
                kind: ActorKind::Service,
                id: "node-b".into(),
            },
        ] {
            let mut command = command();
            command.actor = actor;
            let mut persistence = persistence(Some(lease(NodeDaemonLeaseStatus::Active, 101)));
            assert_eq!(
                resolve_runtime_recovery(&mut persistence, command),
                Err(RuntimeRecoveryActionError::UnauthorizedNodeOperator)
            );
            assert_eq!(persistence.lease_reads, 0);
            assert!(persistence.commits.is_empty());
        }

        let mut command = command();
        command.confirmation = Some("wrong".into());
        let mut persistence = persistence(Some(lease(NodeDaemonLeaseStatus::Active, 101)));
        assert_eq!(
            resolve_runtime_recovery(&mut persistence, command),
            Err(RuntimeRecoveryActionError::ConfirmationRequired)
        );
        assert_eq!(persistence.lease_reads, 0);
        assert!(persistence.commits.is_empty());
    }

    #[test]
    fn current_active_unexpired_exact_node_lease_is_required() {
        for candidate in [
            None,
            Some(lease(NodeDaemonLeaseStatus::Draining, 101)),
            Some(lease(NodeDaemonLeaseStatus::Released, 101)),
            Some(lease(NodeDaemonLeaseStatus::Expired, 101)),
            Some(lease(NodeDaemonLeaseStatus::Active, 100)),
        ] {
            let mut persistence = persistence(candidate);
            assert_eq!(
                resolve_runtime_recovery(&mut persistence, command()),
                Err(RuntimeRecoveryActionError::CurrentNodeDaemonUnavailable)
            );
            assert_eq!(persistence.lease_reads, 1);
            assert!(persistence.commits.is_empty());
        }

        let mut foreign = lease(NodeDaemonLeaseStatus::Active, 101);
        foreign.node_id = "node-b".into();
        let mut persistence = persistence(Some(foreign));
        assert_eq!(
            resolve_runtime_recovery(&mut persistence, command()),
            Err(RuntimeRecoveryActionError::CurrentNodeDaemonUnavailable)
        );
        assert!(persistence.commits.is_empty());
    }

    #[test]
    fn evidence_validation_preserves_lease_first_error_order_and_never_commits() {
        let mut command = command();
        command.evidence_ref = "  ".into();
        let mut no_lease = persistence(None);
        assert_eq!(
            resolve_runtime_recovery(&mut no_lease, command.clone()),
            Err(RuntimeRecoveryActionError::CurrentNodeDaemonUnavailable)
        );

        let mut valid_lease = persistence(Some(lease(NodeDaemonLeaseStatus::Active, 101)));
        assert_eq!(
            resolve_runtime_recovery(&mut valid_lease, command),
            Err(RuntimeRecoveryActionError::EvidenceRequired)
        );
        assert_eq!(valid_lease.lease_reads, 1);
        assert!(valid_lease.commits.is_empty());
    }

    #[test]
    fn application_freezes_exact_daemon_context_and_stable_idempotency_namespace() {
        for resolution in [
            RuntimeRecoveryResolution::ConfirmApplied,
            RuntimeRecoveryResolution::ConfirmNotApplied,
            RuntimeRecoveryResolution::KeepRecoveryRequired,
        ] {
            let mut command = command();
            command.resolution = resolution;
            let mut persistence = persistence(Some(lease(NodeDaemonLeaseStatus::Active, 101)));
            assert_eq!(
                resolve_runtime_recovery(&mut persistence, command),
                Err(RuntimeRecoveryActionError::Persistence(FakeError))
            );
            let commit = persistence.commits.pop().expect("one commit attempt");
            assert_eq!(commit.command_id, "command-a");
            assert_eq!(commit.node_id, "node-a");
            assert_eq!(commit.daemon_id, "daemon-a");
            assert_eq!(commit.daemon_generation, 7);
            assert_eq!(commit.resolution, resolution);
            assert_eq!(commit.evidence_ref, "evidence-a");
            assert_eq!(commit.context.execution_space_id, "space-a");
            assert_eq!(commit.context.authenticated_actor.id, "daemon-a");
            assert_eq!(commit.context.authority_actor.unwrap().id, "node-a");
            assert_eq!(commit.context.command_name, RUNTIME_RECOVERY_COMMAND_NAME);
            assert_eq!(
                commit.context.idempotency_key,
                "role-runtime-recovery:node-a:request-a"
            );
            assert_eq!(commit.context.expected_version, 4);
            assert_eq!(
                commit.context.request_fingerprint.as_deref(),
                Some("fingerprint-a")
            );
        }
    }
}
