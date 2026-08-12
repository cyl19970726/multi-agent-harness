use crate::protocol::*;
use crate::store::FabricState;
use crate::{
    sha256_hex, FabricError, FabricErrorCode, FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

pub const ENROLLMENT_LIFETIME_MAX_MS: u64 = 10 * 60 * 1000;
pub const NODE_CERTIFICATE_LIFETIME_MAX_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_enrollment(
    state: &mut FabricState,
    actor: &AuthenticatedActor,
    company_id: &str,
    enrollment_id: &str,
    raw_token: &str,
    requested_name: &str,
    allowed_capabilities: std::collections::BTreeSet<String>,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
) -> Result<NodeEnrollment, FabricError> {
    actor.require_company_and_role(company_id, "company_host", now_unix_ms)?;
    if raw_token.len() < 32 || expires_at_unix_ms <= now_unix_ms {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "enrollment token must be strong and have a future expiry",
        ));
    }
    if expires_at_unix_ms.saturating_sub(now_unix_ms) > ENROLLMENT_LIFETIME_MAX_MS {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "enrollment token lifetime exceeds 10 minutes",
        ));
    }
    if state.enrollments.contains_key(enrollment_id) {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "enrollment id already exists",
        ));
    }
    let token_digest = sha256_hex(raw_token.as_bytes());
    if state
        .enrollments
        .values()
        .any(|enrollment| enrollment.token_digest == token_digest)
    {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "enrollment token was already issued",
        ));
    }
    let enrollment = NodeEnrollment {
        id: enrollment_id.into(),
        company_id: company_id.into(),
        token_digest,
        requested_name: requested_name.into(),
        allowed_capabilities,
        created_by: actor.actor_id.clone(),
        expires_at_unix_ms,
        consumed_at_unix_ms: None,
        consumed_by_node_id: None,
        status: EnrollmentStatus::Pending,
        revision: 1,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
    };
    state
        .enrollments
        .insert(enrollment.id.clone(), enrollment.clone());
    Ok(enrollment)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn consume_enrollment(
    state: &mut FabricState,
    company_id: &str,
    raw_token: &str,
    node_id: &str,
    display_name: &str,
    proof: &EnrollmentProof,
    certificate_serial: &str,
    certificate_expires_at_unix_ms: u64,
    schema_bundle_digest: &str,
    now_unix_ms: u64,
) -> Result<(CompanyNode, NodeCertificate), FabricError> {
    let token_digest = sha256_hex(raw_token.as_bytes());
    if certificate_expires_at_unix_ms <= now_unix_ms
        || certificate_expires_at_unix_ms.saturating_sub(now_unix_ms)
            > NODE_CERTIFICATE_LIFETIME_MAX_MS
    {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "Node certificate lifetime must be positive and no longer than 30 days",
        ));
    }
    let Some(enrollment_id) = state
        .enrollments
        .values()
        .find(|enrollment| enrollment.token_digest == token_digest)
        .map(|enrollment| enrollment.id.clone())
    else {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "enrollment token is invalid",
        ));
    };
    let enrollment = state
        .enrollments
        .get_mut(&enrollment_id)
        .expect("selected enrollment exists");
    if enrollment.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "enrollment token belongs to another Company",
        ));
    }
    match enrollment.status {
        EnrollmentStatus::Consumed => {
            return Err(FabricError::none(
                FabricErrorCode::EnrollmentConsumed,
                "enrollment token was already consumed",
            ))
        }
        EnrollmentStatus::Revoked => {
            return Err(FabricError::none(
                FabricErrorCode::EnrollmentRevoked,
                "enrollment token was revoked",
            ))
        }
        EnrollmentStatus::Expired => {
            return Err(FabricError::none(
                FabricErrorCode::EnrollmentExpired,
                "enrollment token expired",
            ))
        }
        EnrollmentStatus::Pending => {}
    }
    if enrollment.expires_at_unix_ms <= now_unix_ms {
        enrollment.status = EnrollmentStatus::Expired;
        enrollment.updated_at_unix_ms = now_unix_ms;
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentExpired,
            "enrollment token expired",
        ));
    }
    let expected_challenge = enrollment_challenge(
        company_id,
        &enrollment_id,
        node_id,
        certificate_serial,
        schema_bundle_digest,
    );
    if proof.challenge != expected_challenge {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "enrollment proof challenge does not match the exact Node and certificate scope",
        ));
    }
    let public_key_bytes: [u8; 32] = proof.public_key.as_slice().try_into().map_err(|_| {
        FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "Ed25519 public key must contain exactly 32 bytes",
        )
    })?;
    let signature_bytes: [u8; 64] = proof.signature.as_slice().try_into().map_err(|_| {
        FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "Ed25519 proof signature must contain exactly 64 bytes",
        )
    })?;
    let verifying_key = VerifyingKey::from_bytes(&public_key_bytes).map_err(|_| {
        FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "Ed25519 public key is invalid",
        )
    })?;
    verifying_key
        .verify(
            expected_challenge.as_bytes(),
            &Signature::from_bytes(&signature_bytes),
        )
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::EnrollmentInvalid,
                "enrollment proof-of-possession signature is invalid",
            )
        })?;
    let public_key_fingerprint = sha256_hex(public_key_bytes);
    let proof_of_possession_digest = sha256_hex(signature_bytes);
    if state.nodes.contains_key(node_id) || state.certificates.contains_key(certificate_serial) {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "node identity, certificate, or proof of possession is invalid",
        ));
    }
    enrollment.status = EnrollmentStatus::Consumed;
    enrollment.revision = enrollment.revision.saturating_add(1);
    enrollment.consumed_at_unix_ms = Some(now_unix_ms);
    enrollment.consumed_by_node_id = Some(node_id.into());
    enrollment.updated_at_unix_ms = now_unix_ms;
    let allowed_capabilities = enrollment.allowed_capabilities.clone();
    let node = CompanyNode {
        id: node_id.into(),
        company_id: company_id.into(),
        display_name: display_name.into(),
        public_key_fingerprint: public_key_fingerprint.clone(),
        certificate_serial: certificate_serial.into(),
        allowed_capabilities,
        administrative_status: NodeAdministrativeStatus::Active,
        node_revision: 1,
        enrolled_at_unix_ms: now_unix_ms,
        last_seen_at_unix_ms: None,
        revoked_at_unix_ms: None,
        revoke_reason: None,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: schema_bundle_digest.into(),
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
    };
    let certificate = NodeCertificate {
        serial: certificate_serial.into(),
        company_id: company_id.into(),
        node_id: node_id.into(),
        public_key_fingerprint,
        node_daemon_id: format!("node-daemon:{node_id}"),
        node_daemon_generation: 1,
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: certificate_expires_at_unix_ms,
        revoked_at_unix_ms: None,
        proof_of_possession_digest,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state.nodes.insert(node.id.clone(), node.clone());
    state
        .certificates
        .insert(certificate.serial.clone(), certificate.clone());
    Ok((node, certificate))
}

/// Consume a one-use enrollment using a verified CSR as proof-of-possession.
/// This is the production one-request enrollment path: the Node cannot sign a
/// challenge containing a certificate serial that the CA has not issued yet.
#[allow(clippy::too_many_arguments)]
pub(crate) fn consume_enrollment_csr(
    state: &mut FabricState,
    company_id: &str,
    raw_token: &str,
    node_id: &str,
    display_name: &str,
    public_key_fingerprint: &str,
    csr_digest: &str,
    certificate_serial: &str,
    certificate_expires_at_unix_ms: u64,
    schema_bundle_digest: &str,
    now_unix_ms: u64,
) -> Result<(CompanyNode, NodeCertificate), FabricError> {
    let token_digest = sha256_hex(raw_token.as_bytes());
    if certificate_expires_at_unix_ms <= now_unix_ms
        || certificate_expires_at_unix_ms.saturating_sub(now_unix_ms)
            > NODE_CERTIFICATE_LIFETIME_MAX_MS
        || public_key_fingerprint.len() != 64
        || csr_digest.len() != 64
    {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "CSR enrollment certificate lifetime or proof is invalid",
        ));
    }
    let enrollment_id = state
        .enrollments
        .values()
        .find(|enrollment| enrollment.token_digest == token_digest)
        .map(|enrollment| enrollment.id.clone())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::EnrollmentInvalid,
                "enrollment token is invalid",
            )
        })?;
    let enrollment = state
        .enrollments
        .get_mut(&enrollment_id)
        .expect("selected enrollment exists");
    if enrollment.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "enrollment token belongs to another Company",
        ));
    }
    match enrollment.status {
        EnrollmentStatus::Consumed => {
            return Err(FabricError::none(
                FabricErrorCode::EnrollmentConsumed,
                "enrollment token was already consumed",
            ))
        }
        EnrollmentStatus::Revoked => {
            return Err(FabricError::none(
                FabricErrorCode::EnrollmentRevoked,
                "enrollment token was revoked",
            ))
        }
        EnrollmentStatus::Expired => {
            return Err(FabricError::none(
                FabricErrorCode::EnrollmentExpired,
                "enrollment token expired",
            ))
        }
        EnrollmentStatus::Pending => {}
    }
    if enrollment.expires_at_unix_ms <= now_unix_ms {
        enrollment.status = EnrollmentStatus::Expired;
        enrollment.updated_at_unix_ms = now_unix_ms;
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentExpired,
            "enrollment token expired",
        ));
    }
    if state.nodes.contains_key(node_id) || state.certificates.contains_key(certificate_serial) {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "node identity or certificate already exists",
        ));
    }
    enrollment.status = EnrollmentStatus::Consumed;
    enrollment.revision = enrollment.revision.saturating_add(1);
    enrollment.consumed_at_unix_ms = Some(now_unix_ms);
    enrollment.consumed_by_node_id = Some(node_id.into());
    enrollment.updated_at_unix_ms = now_unix_ms;
    let node = CompanyNode {
        id: node_id.into(),
        company_id: company_id.into(),
        display_name: display_name.into(),
        public_key_fingerprint: public_key_fingerprint.into(),
        certificate_serial: certificate_serial.into(),
        allowed_capabilities: enrollment.allowed_capabilities.clone(),
        administrative_status: NodeAdministrativeStatus::Active,
        node_revision: 1,
        enrolled_at_unix_ms: now_unix_ms,
        last_seen_at_unix_ms: None,
        revoked_at_unix_ms: None,
        revoke_reason: None,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: schema_bundle_digest.into(),
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
    };
    let certificate = NodeCertificate {
        serial: certificate_serial.into(),
        company_id: company_id.into(),
        node_id: node_id.into(),
        public_key_fingerprint: public_key_fingerprint.into(),
        node_daemon_id: format!("node-daemon:{node_id}"),
        node_daemon_generation: 1,
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: certificate_expires_at_unix_ms,
        revoked_at_unix_ms: None,
        proof_of_possession_digest: csr_digest.into(),
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state.nodes.insert(node.id.clone(), node.clone());
    state
        .certificates
        .insert(certificate.serial.clone(), certificate.clone());
    Ok((node, certificate))
}

pub(crate) fn revoke_enrollment(
    state: &mut FabricState,
    actor: &AuthenticatedActor,
    company_id: &str,
    enrollment_id: &str,
    expected_revision: u64,
    now_unix_ms: u64,
) -> Result<NodeEnrollment, FabricError> {
    actor.require_company_and_role(company_id, "company_host", now_unix_ms)?;
    let enrollment = state
        .enrollments
        .get(enrollment_id)
        .cloned()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::EnrollmentInvalid,
                "enrollment does not exist",
            )
        })?;
    if enrollment.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "enrollment belongs to another Company",
        ));
    }
    if enrollment.revision != expected_revision {
        return Err(crate::control_plane::revision_conflict(
            "enrollment revision mismatch",
            expected_revision,
            enrollment.revision,
        ));
    }
    if enrollment.status != EnrollmentStatus::Pending {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "only a pending enrollment can be revoked",
        ));
    }
    let mut next = enrollment;
    next.status = EnrollmentStatus::Revoked;
    next.revision = next.revision.saturating_add(1);
    next.updated_at_unix_ms = now_unix_ms;
    state.enrollments.insert(enrollment_id.into(), next.clone());
    Ok(next)
}

pub fn enrollment_challenge(
    company_id: &str,
    enrollment_id: &str,
    node_id: &str,
    certificate_serial: &str,
    schema_bundle_digest: &str,
) -> String {
    format!(
        "agentfirm.remote_fabric.v1:enroll:{company_id}:{enrollment_id}:{node_id}:{certificate_serial}:{schema_bundle_digest}"
    )
}

pub fn certificate_rotation_challenge(
    company_id: &str,
    node_id: &str,
    prior_certificate_serial: &str,
    next_certificate_serial: &str,
    expected_node_revision: u64,
    schema_bundle_digest: &str,
) -> String {
    format!(
        "agentfirm.remote_fabric.v1:rotate:{company_id}:{node_id}:{prior_certificate_serial}:{next_certificate_serial}:{expected_node_revision}:{schema_bundle_digest}"
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rotate_certificate(
    state: &mut FabricState,
    company_id: &str,
    node_id: &str,
    current_certificate_serial: &str,
    next_certificate_serial: &str,
    expected_node_revision: u64,
    proof: &EnrollmentProof,
    next_certificate_expires_at_unix_ms: u64,
    now_unix_ms: u64,
) -> Result<(CompanyNode, NodeCertificate), FabricError> {
    let node =
        state.nodes.get(node_id).cloned().ok_or_else(|| {
            FabricError::none(FabricErrorCode::SourceMismatch, "Node does not exist")
        })?;
    if node.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "Node belongs to another Company",
        ));
    }
    if node.administrative_status == NodeAdministrativeStatus::Revoked {
        return Err(FabricError::none(
            FabricErrorCode::NodeRevoked,
            "revoked Node cannot rotate a certificate",
        ));
    }
    if node.node_revision != expected_node_revision {
        return Err(crate::control_plane::revision_conflict(
            "Node revision changed before certificate rotation",
            expected_node_revision,
            node.node_revision,
        ));
    }
    if node.certificate_serial != current_certificate_serial
        || state
            .revoked_certificate_serials
            .contains(current_certificate_serial)
        || state.certificates.contains_key(next_certificate_serial)
        || next_certificate_expires_at_unix_ms <= now_unix_ms
        || next_certificate_expires_at_unix_ms.saturating_sub(now_unix_ms)
            > NODE_CERTIFICATE_LIFETIME_MAX_MS
    {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "certificate rotation serial, lifetime, or current binding is invalid",
        ));
    }
    let challenge = certificate_rotation_challenge(
        company_id,
        node_id,
        current_certificate_serial,
        next_certificate_serial,
        expected_node_revision,
        &node.schema_bundle_digest,
    );
    if proof.challenge != challenge {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "certificate rotation proof has the wrong scope",
        ));
    }
    let public_key_bytes: [u8; 32] = proof.public_key.as_slice().try_into().map_err(|_| {
        FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "Ed25519 public key must contain exactly 32 bytes",
        )
    })?;
    let signature_bytes: [u8; 64] = proof.signature.as_slice().try_into().map_err(|_| {
        FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "Ed25519 proof signature must contain exactly 64 bytes",
        )
    })?;
    VerifyingKey::from_bytes(&public_key_bytes)
        .and_then(|key| {
            key.verify(
                challenge.as_bytes(),
                &Signature::from_bytes(&signature_bytes),
            )
        })
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::EnrollmentInvalid,
                "certificate rotation proof-of-possession signature is invalid",
            )
        })?;
    let fingerprint = sha256_hex(public_key_bytes);
    let prior_certificate = state
        .certificates
        .get(current_certificate_serial)
        .cloned()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::EnrollmentInvalid,
                "current certificate authority record is missing",
            )
        })?;
    let certificate = NodeCertificate {
        serial: next_certificate_serial.into(),
        company_id: company_id.into(),
        node_id: node_id.into(),
        public_key_fingerprint: fingerprint.clone(),
        node_daemon_id: prior_certificate.node_daemon_id,
        node_daemon_generation: prior_certificate.node_daemon_generation.saturating_add(1),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: next_certificate_expires_at_unix_ms,
        revoked_at_unix_ms: None,
        proof_of_possession_digest: sha256_hex(signature_bytes),
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let mut next = node;
    next.certificate_serial = next_certificate_serial.into();
    next.public_key_fingerprint = fingerprint;
    next.node_revision = next.node_revision.saturating_add(1);
    next.updated_at_unix_ms = now_unix_ms;
    state
        .revoked_certificate_serials
        .insert(current_certificate_serial.into());
    if let Some(prior) = state.certificates.get_mut(current_certificate_serial) {
        prior.revoked_at_unix_ms = Some(now_unix_ms);
    }
    if let Some(lease) = state.gateway_leases.get_mut(node_id) {
        lease.expires_at_unix_ms = now_unix_ms;
        lease.revision = lease.revision.saturating_add(1);
    }
    state
        .certificates
        .insert(certificate.serial.clone(), certificate.clone());
    state.nodes.insert(node_id.into(), next.clone());
    Ok((next, certificate))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rotate_certificate_csr(
    state: &mut FabricState,
    company_id: &str,
    node_id: &str,
    current_certificate_serial: &str,
    next_certificate_serial: &str,
    expected_node_revision: u64,
    next_public_key_fingerprint: &str,
    csr_digest: &str,
    next_certificate_expires_at_unix_ms: u64,
    now_unix_ms: u64,
) -> Result<(CompanyNode, NodeCertificate), FabricError> {
    let node =
        state.nodes.get(node_id).cloned().ok_or_else(|| {
            FabricError::none(FabricErrorCode::SourceMismatch, "Node does not exist")
        })?;
    if node.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "Node belongs to another Company",
        ));
    }
    if node.administrative_status == NodeAdministrativeStatus::Revoked {
        return Err(FabricError::none(
            FabricErrorCode::NodeRevoked,
            "revoked Node cannot rotate a certificate",
        ));
    }
    if node.node_revision != expected_node_revision {
        return Err(crate::control_plane::revision_conflict(
            "Node revision changed before certificate rotation",
            expected_node_revision,
            node.node_revision,
        ));
    }
    if node.certificate_serial != current_certificate_serial
        || state
            .revoked_certificate_serials
            .contains(current_certificate_serial)
        || state.certificates.contains_key(next_certificate_serial)
        || next_public_key_fingerprint.trim().is_empty()
        || csr_digest.trim().is_empty()
        || next_certificate_expires_at_unix_ms <= now_unix_ms
        || next_certificate_expires_at_unix_ms.saturating_sub(now_unix_ms)
            > NODE_CERTIFICATE_LIFETIME_MAX_MS
    {
        return Err(FabricError::none(
            FabricErrorCode::EnrollmentInvalid,
            "certificate rotation CSR, serial, lifetime, or current binding is invalid",
        ));
    }
    let prior_certificate = state
        .certificates
        .get(current_certificate_serial)
        .cloned()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::EnrollmentInvalid,
                "current certificate authority record is missing",
            )
        })?;
    let certificate = NodeCertificate {
        serial: next_certificate_serial.into(),
        company_id: company_id.into(),
        node_id: node_id.into(),
        public_key_fingerprint: next_public_key_fingerprint.into(),
        node_daemon_id: prior_certificate.node_daemon_id,
        node_daemon_generation: prior_certificate.node_daemon_generation.saturating_add(1),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: next_certificate_expires_at_unix_ms,
        revoked_at_unix_ms: None,
        proof_of_possession_digest: csr_digest.into(),
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let mut next = node;
    next.certificate_serial = next_certificate_serial.into();
    next.public_key_fingerprint = next_public_key_fingerprint.into();
    next.node_revision = next.node_revision.saturating_add(1);
    next.updated_at_unix_ms = now_unix_ms;
    state
        .revoked_certificate_serials
        .insert(current_certificate_serial.into());
    if let Some(prior) = state.certificates.get_mut(current_certificate_serial) {
        prior.revoked_at_unix_ms = Some(now_unix_ms);
    }
    if let Some(lease) = state.gateway_leases.get_mut(node_id) {
        lease.expires_at_unix_ms = now_unix_ms;
        lease.revision = lease.revision.saturating_add(1);
    }
    state
        .certificates
        .insert(certificate.serial.clone(), certificate.clone());
    state.nodes.insert(node_id.into(), next.clone());
    Ok((next, certificate))
}
