use std::collections::BTreeSet;

use crate::artifacts::ArtifactKeyBackend;
use crate::enrollment;
use crate::node_gateway;
use crate::protocol::*;
use crate::router;
use crate::store::{FabricState, FabricStore};
use crate::{FabricError, FabricErrorCode, FABRIC_SCHEMA_VERSION};

pub struct ControlPlane<'a, K: ArtifactKeyBackend> {
    company_id: String,
    instance_id: String,
    store: &'a FabricStore,
    artifact_keys: &'a K,
    capability_signing_key: [u8; 32],
}

impl<'a, K: ArtifactKeyBackend> ControlPlane<'a, K> {
    pub fn new(
        company_id: impl Into<String>,
        instance_id: impl Into<String>,
        store: &'a FabricStore,
        artifact_keys: &'a K,
        capability_signing_key: [u8; 32],
    ) -> Self {
        Self {
            company_id: company_id.into(),
            instance_id: instance_id.into(),
            store,
            artifact_keys,
            capability_signing_key,
        }
    }

    pub fn company_id(&self) -> &str {
        &self.company_id
    }

    pub fn store(&self) -> &FabricStore {
        self.store
    }

    pub fn acquire_lease(
        &self,
        lease_id: &str,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<CompanyControlPlaneLease, FabricError> {
        self.store.transact(|state| {
            let prior = state.control_plane_leases.get(&self.company_id).cloned();
            if let Some(prior) = prior.as_ref() {
                if prior.revision != expected_revision {
                    return Err(revision_conflict(
                        "Control Plane lease revision mismatch",
                        expected_revision,
                        prior.revision,
                    ));
                }
                if prior.expires_at_unix_ms > now_unix_ms && prior.instance_id != self.instance_id {
                    return Err(FabricError::none(
                        FabricErrorCode::LeaseConflict,
                        "another Control Plane generation is still active",
                    ));
                }
            } else if expected_revision != 0 {
                return Err(revision_conflict(
                    "Control Plane lease does not exist",
                    expected_revision,
                    0,
                ));
            }
            let lease = CompanyControlPlaneLease {
                company_id: self.company_id.clone(),
                lease_id: lease_id.into(),
                instance_id: self.instance_id.clone(),
                control_plane_generation: prior
                    .as_ref()
                    .map_or(1, |lease| lease.control_plane_generation.saturating_add(1)),
                revision: prior
                    .as_ref()
                    .map_or(1, |lease| lease.revision.saturating_add(1)),
                acquired_at_unix_ms: now_unix_ms,
                expires_at_unix_ms: now_unix_ms.saturating_add(30_000),
                last_heartbeat_at_unix_ms: now_unix_ms,
                schema_version: FABRIC_SCHEMA_VERSION.into(),
            };
            state
                .control_plane_leases
                .insert(self.company_id.clone(), lease.clone());
            Ok(lease)
        })
    }

    pub fn heartbeat_lease(
        &self,
        generation: u64,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<CompanyControlPlaneLease, FabricError> {
        self.store.transact(|state| {
            let lease = require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            if lease.revision != expected_revision {
                return Err(revision_conflict(
                    "Control Plane heartbeat revision mismatch",
                    expected_revision,
                    lease.revision,
                ));
            }
            let mut next = lease.clone();
            next.revision = next.revision.saturating_add(1);
            next.last_heartbeat_at_unix_ms = now_unix_ms;
            next.expires_at_unix_ms = now_unix_ms.saturating_add(30_000);
            state
                .control_plane_leases
                .insert(self.company_id.clone(), next.clone());
            Ok(next)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_enrollment(
        &self,
        actor: &AuthenticatedActor,
        generation: u64,
        enrollment_id: &str,
        raw_token: &str,
        requested_name: &str,
        allowed_capabilities: BTreeSet<String>,
        expires_at_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<NodeEnrollment, FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            enrollment::create_enrollment(
                state,
                actor,
                &self.company_id,
                enrollment_id,
                raw_token,
                requested_name,
                allowed_capabilities,
                expires_at_unix_ms,
                now_unix_ms,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn consume_enrollment(
        &self,
        generation: u64,
        raw_token: &str,
        node_id: &str,
        display_name: &str,
        proof: &EnrollmentProof,
        certificate_serial: &str,
        certificate_expires_at_unix_ms: u64,
        schema_bundle_digest: &str,
        now_unix_ms: u64,
    ) -> Result<(CompanyNode, NodeCertificate), FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            enrollment::consume_enrollment(
                state,
                &self.company_id,
                raw_token,
                node_id,
                display_name,
                proof,
                certificate_serial,
                certificate_expires_at_unix_ms,
                schema_bundle_digest,
                now_unix_ms,
            )
        })
    }

    pub fn connect_gateway(
        &self,
        generation: u64,
        hello: &NodeHello,
        now_unix_ms: u64,
    ) -> Result<NodeWelcome, FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            node_gateway::connect(state, &self.company_id, generation, hello, now_unix_ms)
        })
    }

    pub fn heartbeat_gateway(
        &self,
        generation: u64,
        node_id: &str,
        gateway_generation: u64,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<NodeGatewayLease, FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            node_gateway::heartbeat(
                state,
                &self.company_id,
                generation,
                node_id,
                gateway_generation,
                expected_revision,
                now_unix_ms,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rotate_node_certificate(
        &self,
        generation: u64,
        node_id: &str,
        gateway_generation: u64,
        current_certificate_serial: &str,
        next_certificate_serial: &str,
        expected_node_revision: u64,
        proof: &EnrollmentProof,
        next_certificate_expires_at_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<(CompanyNode, NodeCertificate), FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            node_gateway::require_current_gateway(
                state,
                &self.company_id,
                generation,
                node_id,
                gateway_generation,
                now_unix_ms,
            )?;
            enrollment::rotate_certificate(
                state,
                &self.company_id,
                node_id,
                current_certificate_serial,
                next_certificate_serial,
                expected_node_revision,
                proof,
                next_certificate_expires_at_unix_ms,
                now_unix_ms,
            )
        })
    }

    pub fn accept_operation(
        &self,
        generation: u64,
        operation: RoutedOperation,
        now_unix_ms: u64,
    ) -> Result<(RoutedOperation, RouteAttempt, RouteReceipt, bool), FabricError> {
        let limits = self.store.limits();
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            router::accept_and_enqueue(
                state,
                &self.company_id,
                generation,
                operation,
                limits,
                now_unix_ms,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn persist_target_inbox(
        &self,
        generation: u64,
        node_id: &str,
        gateway_generation: u64,
        operation_id: &str,
        request_digest: &str,
        route_seq: u64,
        now_unix_ms: u64,
    ) -> Result<(LocalRemoteInbox, RouteReceipt, bool), FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            router::persist_target_inbox(
                state,
                &self.company_id,
                generation,
                node_id,
                gateway_generation,
                operation_id,
                request_digest,
                route_seq,
                now_unix_ms,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_application_result(
        &self,
        generation: u64,
        node_id: &str,
        gateway_generation: u64,
        operation_id: &str,
        result_schema: &str,
        result: serde_json::Value,
        applied: bool,
        now_unix_ms: u64,
    ) -> Result<(LocalRemoteInbox, RouteReceipt, bool), FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            router::record_application_result(
                state,
                &self.company_id,
                generation,
                node_id,
                gateway_generation,
                operation_id,
                result_schema,
                result,
                applied,
                now_unix_ms,
            )
        })
    }

    pub fn mark_unknown(
        &self,
        generation: u64,
        operation_id: &str,
        now_unix_ms: u64,
    ) -> Result<(), FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            router::mark_unknown(state, &self.company_id, operation_id)
        })
    }

    pub fn reconcile(
        &self,
        generation: u64,
        node_id: &str,
        gateway_generation: u64,
        operation_ids: &BTreeSet<String>,
        now_unix_ms: u64,
    ) -> Result<Vec<RouteReceipt>, FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            crate::reconcile::reconcile(
                state,
                &self.company_id,
                generation,
                node_id,
                gateway_generation,
                operation_ids,
                now_unix_ms,
            )
        })
    }

    pub fn revoke_node(
        &self,
        actor: &AuthenticatedActor,
        generation: u64,
        node_id: &str,
        expected_revision: u64,
        reason: &str,
        now_unix_ms: u64,
    ) -> Result<CompanyNode, FabricError> {
        self.store.transact(|state| {
            require_active_control_plane(
                state,
                &self.company_id,
                &self.instance_id,
                generation,
                now_unix_ms,
            )?;
            actor.require_company_and_role(&self.company_id, "company_host", now_unix_ms)?;
            let node = state.nodes.get(node_id).cloned().ok_or_else(|| {
                FabricError::none(FabricErrorCode::SourceMismatch, "Node does not exist")
            })?;
            if node.company_id != self.company_id {
                return Err(FabricError::none(
                    FabricErrorCode::WrongCompany,
                    "Node belongs to another Company",
                ));
            }
            if node.node_revision != expected_revision {
                return Err(revision_conflict(
                    "Node revision mismatch",
                    expected_revision,
                    node.node_revision,
                ));
            }
            let mut next = node;
            next.administrative_status = NodeAdministrativeStatus::Revoked;
            next.node_revision = next.node_revision.saturating_add(1);
            next.revoked_at_unix_ms = Some(now_unix_ms);
            next.revoke_reason = Some(reason.into());
            next.updated_at_unix_ms = now_unix_ms;
            state
                .revoked_certificate_serials
                .insert(next.certificate_serial.clone());
            if let Some(certificate) = state.certificates.get_mut(&next.certificate_serial) {
                certificate.revoked_at_unix_ms = Some(now_unix_ms);
            }
            state.gateway_leases.remove(node_id);
            state.nodes.insert(node_id.into(), next.clone());
            Ok(next)
        })
    }

    pub(crate) fn artifact_keys(&self) -> &K {
        self.artifact_keys
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn capability_signing_key(&self) -> &[u8; 32] {
        &self.capability_signing_key
    }
}

pub(crate) fn require_active_control_plane<'a>(
    state: &'a FabricState,
    company_id: &str,
    instance_id: &str,
    generation: u64,
    now_unix_ms: u64,
) -> Result<&'a CompanyControlPlaneLease, FabricError> {
    let lease = state.control_plane_leases.get(company_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::ControlPlaneStaleGeneration,
            "Company has no Control Plane lease",
        )
    })?;
    if lease.instance_id != instance_id
        || lease.control_plane_generation != generation
        || lease.expires_at_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::ControlPlaneStaleGeneration,
            "Control Plane generation is not current",
        ));
    }
    Ok(lease)
}

pub(crate) fn revision_conflict(message: &str, expected: u64, actual: u64) -> FabricError {
    let mut error = FabricError::none(FabricErrorCode::ExpectedRevisionConflict, message);
    error.expected_revision = Some(expected);
    error.actual_revision = Some(actual);
    error
}
