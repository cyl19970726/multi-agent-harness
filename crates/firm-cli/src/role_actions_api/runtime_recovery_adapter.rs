use harness_application::{
    ResolveRuntimeRecoveryCommand, RuntimeRecoveryActionError, RuntimeRecoveryActionOutcome,
    RuntimeRecoveryCommit, RuntimeRecoveryPersistence,
};
use harness_core::agentfirm_api::RuntimeRecoveryResolution;
use harness_core::NodeDaemonLease;
use harness_store::{HarnessStore, StoreError};

use super::{encoded_error, now_string, AuthenticatedMutation, RoleActionResult};

pub(super) struct HarnessStoreRuntimeRecovery<'a> {
    store: &'a HarnessStore,
}

impl<'a> HarnessStoreRuntimeRecovery<'a> {
    pub(super) fn new(store: &'a HarnessStore) -> Self {
        Self { store }
    }
}

impl RuntimeRecoveryPersistence for HarnessStoreRuntimeRecovery<'_> {
    type Error = StoreError;

    fn current_node_daemon_lease(
        &mut self,
        node_id: &str,
    ) -> Result<Option<NodeDaemonLease>, Self::Error> {
        self.store.latest_node_daemon_lease(node_id)
    }

    fn commit_runtime_recovery(
        &mut self,
        commit: RuntimeRecoveryCommit,
    ) -> Result<RuntimeRecoveryActionOutcome, Self::Error> {
        let result = self.store.resolve_runtime_command_recovery(
            &commit.context,
            &commit.command_id,
            &commit.node_id,
            &commit.daemon_id,
            commit.daemon_generation,
            commit.resolution,
            &commit.evidence_ref,
            &now_string(),
        )?;
        let resulting_version = result.projection.version;
        Ok(RuntimeRecoveryActionOutcome {
            projection: result.projection,
            event_id: result.event.id,
            resulting_version,
            store_sequence: result.event.store_sequence,
            replayed: result.replayed,
        })
    }
}

pub(super) fn execute(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    node_id: &str,
    command_id: &str,
    confirmed_action: Option<&str>,
    resolution: RuntimeRecoveryResolution,
    evidence_ref: String,
) -> Result<RoleActionResult, StoreError> {
    let mut persistence = HarnessStoreRuntimeRecovery::new(store);
    match harness_application::resolve_runtime_recovery(
        &mut persistence,
        ResolveRuntimeRecoveryCommand {
            execution_space_id: auth.execution_space_id,
            node_id: node_id.into(),
            command_id: command_id.into(),
            actor: auth.actor,
            idempotency_key: auth.idempotency_key,
            request_fingerprint: auth.request_fingerprint,
            expected_version: auth.expected_version,
            confirmation: confirmed_action.map(str::to_string),
            resolution,
            evidence_ref,
            observed_unix_ms: crate::current_unix_ms_u64(),
        },
    ) {
        Ok(outcome) => Ok(RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: serde_json::to_value(outcome.projection)?,
            event_id: outcome.event_id,
            resulting_version: outcome.resulting_version,
            store_sequence: outcome.store_sequence,
            replayed: outcome.replayed,
        }),
        Err(RuntimeRecoveryActionError::UnauthorizedNodeOperator) => Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "RuntimeCommand recovery requires the exact Execution Node Operator",
            "execution_node",
            node_id,
            None,
        )),
        Err(RuntimeRecoveryActionError::ConfirmationRequired) => Err(encoded_error(
            "CONFIRMATION_REQUIRED",
            "server confirmation must exactly confirm resolve_runtime_recovery",
            "runtime_command",
            command_id,
            None,
        )),
        Err(RuntimeRecoveryActionError::CurrentNodeDaemonUnavailable) => Err(encoded_error(
            "NODE_DAEMON_GENERATION_FENCED",
            "RuntimeCommand recovery requires the exact current NodeDaemon",
            "execution_node",
            node_id,
            None,
        )),
        Err(RuntimeRecoveryActionError::EvidenceRequired) => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "RuntimeCommand recovery evidence_ref must not be empty",
            "request",
            "RuntimeCommand recovery evidence_ref",
            None,
        )),
        Err(RuntimeRecoveryActionError::Persistence(error)) => Err(error),
    }
}
