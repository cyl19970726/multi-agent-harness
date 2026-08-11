//! Control Plane CA and Node client-certificate contract.

use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, SigningKey};
use rcgen::{
    BasicConstraints, CertificateParams, CertificateSigningRequestParams, DistinguishedName,
    DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType, PKCS_ED25519,
};
use rustls_pki_types::CertificateDer;
use time::{Duration, OffsetDateTime};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, ParsedExtension, X509Certificate};

use crate::{sha256_hex, FabricError, FabricErrorCode};

pub const NODE_CERTIFICATE_LIFETIME_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricCaMaterial {
    pub certificate_pem: String,
    pub private_key_pem: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCsrMaterial {
    pub csr_pem: String,
    pub private_key_pem: String,
    pub public_key_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedNodeCertificate {
    pub certificate_pem: String,
    pub serial: String,
    pub public_key_fingerprint: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedServerCertificate {
    pub certificate_chain_pem: String,
    pub private_key_pem: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerNodeIdentity {
    pub company_id: String,
    pub node_id: String,
    pub certificate_serial: String,
    pub public_key_fingerprint: String,
}

pub fn generate_ca(company_id: &str) -> Result<FabricCaMaterial, FabricError> {
    validate_identity_part(company_id, "Company")?;
    let key = KeyPair::generate_for(&PKCS_ED25519).map_err(pki_error)?;
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(
        DnType::CommonName,
        format!("AgentFirm Company {company_id} CA"),
    );
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let certificate = params.self_signed(&key).map_err(pki_error)?;
    Ok(FabricCaMaterial {
        certificate_pem: certificate.pem(),
        private_key_pem: key.serialize_pem(),
    })
}

pub fn generate_node_csr(company_id: &str, node_id: &str) -> Result<NodeCsrMaterial, FabricError> {
    validate_identity_part(company_id, "Company")?;
    validate_identity_part(node_id, "Node")?;
    let key = KeyPair::generate_for(&PKCS_ED25519).map_err(pki_error)?;
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CommonName, node_id);
    params.subject_alt_names = vec![SanType::URI(
        node_identity_uri(company_id, node_id)
            .try_into()
            .map_err(pki_error)?,
    )];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let csr = params.serialize_request(&key).map_err(pki_error)?;
    let csr_pem = csr.pem().map_err(pki_error)?;
    let public_key_fingerprint = csr_public_key_fingerprint(&csr_pem)?;
    Ok(NodeCsrMaterial {
        csr_pem,
        private_key_pem: key.serialize_pem(),
        public_key_fingerprint,
    })
}

/// Produce the enrollment proof with the exact private key used by the CSR.
/// The proof can be sent to the Control Plane; the PEM private key cannot.
pub fn enrollment_proof_from_node_key(
    private_key_pem: &str,
    challenge: String,
) -> Result<crate::EnrollmentProof, FabricError> {
    let signing_key = SigningKey::from_pkcs8_pem(private_key_pem).map_err(pki_error)?;
    Ok(crate::EnrollmentProof {
        public_key: signing_key.verifying_key().to_bytes().to_vec(),
        signature: signing_key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    })
}

fn csr_public_key_fingerprint(csr_pem: &str) -> Result<String, FabricError> {
    let pem = x509_parser::pem::parse_x509_pem(csr_pem.as_bytes())
        .map_err(pki_error)?
        .1;
    let (_, csr) =
        x509_parser::certification_request::X509CertificationRequest::from_der(&pem.contents)
            .map_err(pki_error)?;
    csr.verify_signature().map_err(pki_error)?;
    Ok(sha256_hex(
        csr.certification_request_info
            .subject_pki
            .subject_public_key
            .data
            .as_ref(),
    ))
}

pub fn issue_node_certificate(
    ca: &FabricCaMaterial,
    csr_pem: &str,
    company_id: &str,
    node_id: &str,
    now_unix_ms: u64,
) -> Result<IssuedNodeCertificate, FabricError> {
    validate_identity_part(company_id, "Company")?;
    validate_identity_part(node_id, "Node")?;
    let key = KeyPair::from_pem(&ca.private_key_pem).map_err(pki_error)?;
    let issuer = Issuer::from_ca_cert_pem(&ca.certificate_pem, key).map_err(pki_error)?;
    let mut request = CertificateSigningRequestParams::from_pem(csr_pem).map_err(pki_error)?;
    let expected_uri = node_identity_uri(company_id, node_id);
    if request.params.subject_alt_names
        != vec![SanType::URI(
            expected_uri.clone().try_into().map_err(pki_error)?,
        )]
        || !request
            .params
            .extended_key_usages
            .contains(&ExtendedKeyUsagePurpose::ClientAuth)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "Node CSR must bind the exact Company/ExecutionNode URI and clientAuth purpose",
        ));
    }
    let not_before = OffsetDateTime::from_unix_timestamp_nanos(now_unix_ms as i128 * 1_000_000)
        .map_err(pki_error)?;
    let not_after = not_before + Duration::days(NODE_CERTIFICATE_LIFETIME_DAYS);
    request.params.not_before = not_before;
    request.params.not_after = not_after;
    request.params.is_ca = IsCa::ExplicitNoCa;
    request.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    request.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    request.params.distinguished_name = DistinguishedName::new();
    request
        .params
        .distinguished_name
        .push(DnType::CommonName, node_id);
    request.params.subject_alt_names =
        vec![SanType::URI(expected_uri.try_into().map_err(pki_error)?)];
    let certificate = request.signed_by(&issuer).map_err(pki_error)?;
    let identity = parse_peer_node_identity(certificate.der())?;
    if identity.company_id != company_id || identity.node_id != node_id {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "issued certificate identity changed during signing",
        ));
    }
    Ok(IssuedNodeCertificate {
        certificate_pem: certificate.pem(),
        serial: identity.certificate_serial,
        public_key_fingerprint: identity.public_key_fingerprint,
        expires_at_unix_ms: (not_after.unix_timestamp_nanos() / 1_000_000) as u64,
    })
}

pub fn issue_control_plane_server_certificate(
    ca: &FabricCaMaterial,
    dns_name: &str,
    now_unix_ms: u64,
) -> Result<IssuedServerCertificate, FabricError> {
    if dns_name.trim().is_empty() || dns_name.contains('/') {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "Control Plane DNS name is invalid",
        ));
    }
    let ca_key = KeyPair::from_pem(&ca.private_key_pem).map_err(pki_error)?;
    let issuer = Issuer::from_ca_cert_pem(&ca.certificate_pem, ca_key).map_err(pki_error)?;
    let key = KeyPair::generate().map_err(pki_error)?;
    let mut params = CertificateParams::new(vec![dns_name.to_string()]).map_err(pki_error)?;
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CommonName, dns_name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let not_before = OffsetDateTime::from_unix_timestamp_nanos(now_unix_ms as i128 * 1_000_000)
        .map_err(pki_error)?;
    params.not_before = not_before;
    params.not_after = not_before + Duration::days(NODE_CERTIFICATE_LIFETIME_DAYS);
    let certificate = params.signed_by(&key, &issuer).map_err(pki_error)?;
    Ok(IssuedServerCertificate {
        certificate_chain_pem: format!("{}{}", certificate.pem(), ca.certificate_pem),
        private_key_pem: key.serialize_pem(),
    })
}

pub fn parse_peer_node_identity(
    certificate: &CertificateDer<'_>,
) -> Result<PeerNodeIdentity, FabricError> {
    let (_, parsed) = X509Certificate::from_der(certificate.as_ref()).map_err(pki_error)?;
    let mut identity_uri = None;
    for extension in parsed.extensions() {
        if let ParsedExtension::SubjectAlternativeName(names) = extension.parsed_extension() {
            for name in &names.general_names {
                if let GeneralName::URI(uri) = name {
                    if identity_uri.replace((*uri).to_string()).is_some() {
                        return Err(FabricError::none(
                            FabricErrorCode::UnauthorizedActor,
                            "Node certificate has multiple URI identities",
                        ));
                    }
                }
            }
        }
    }
    let identity_uri = identity_uri.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "Node certificate has no AgentFirm identity URI",
        )
    })?;
    let rest = identity_uri
        .strip_prefix("agentfirm://company/")
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "invalid Node certificate URI",
            )
        })?;
    let (company_id, node_id) = rest.split_once("/node/").ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "invalid Node certificate URI",
        )
    })?;
    validate_identity_part(company_id, "Company")?;
    validate_identity_part(node_id, "Node")?;
    Ok(PeerNodeIdentity {
        company_id: company_id.into(),
        node_id: node_id.into(),
        certificate_serial: parsed.raw_serial_as_string(),
        public_key_fingerprint: sha256_hex(parsed.public_key().subject_public_key.data.as_ref()),
    })
}

fn node_identity_uri(company_id: &str, node_id: &str) -> String {
    format!("agentfirm://company/{company_id}/node/{node_id}")
}

fn validate_identity_part(value: &str, label: &str) -> Result<(), FabricError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("{label} identity is not canonical"),
        ));
    }
    Ok(())
}

fn pki_error(error: impl std::fmt::Display) -> FabricError {
    FabricError::none(
        FabricErrorCode::EnrollmentInvalid,
        format!("Remote Fabric PKI failed: {error}"),
    )
}
