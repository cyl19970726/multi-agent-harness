use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use crate::control_plane::{require_active_control_plane, ControlPlane};
use crate::protocol::*;
use crate::store::EncryptedArtifact;
use crate::{
    bytes_to_hex, canonical_digest, sha256_hex, FabricError, FabricErrorCode, FABRIC_SCHEMA_VERSION,
};

type HmacSha256 = Hmac<Sha256>;

pub trait ArtifactKeyBackend: Send + Sync {
    fn key_for_company(&self, company_id: &str) -> Result<[u8; 32], FabricError>;
}

#[derive(Default)]
pub struct InMemoryArtifactKeyBackend {
    keys: Mutex<BTreeMap<String, [u8; 32]>>,
}

impl InMemoryArtifactKeyBackend {
    pub fn insert(&self, company_id: impl Into<String>, key: [u8; 32]) {
        self.keys
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(company_id.into(), key);
    }
}

impl ArtifactKeyBackend for InMemoryArtifactKeyBackend {
    fn key_for_company(&self, company_id: &str) -> Result<[u8; 32], FabricError> {
        self.keys
            .lock()
            .map_err(|_| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "artifact key backend lock poisoned",
                )
            })?
            .get(company_id)
            .copied()
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "artifact encryption key is unavailable",
                )
            })
    }
}

impl<'a, K: ArtifactKeyBackend> ControlPlane<'a, K> {
    #[allow(clippy::too_many_arguments)]
    pub fn initiate_artifact(
        &self,
        actor: &AuthenticatedActor,
        generation: u64,
        artifact_id: &str,
        source_node_id: &str,
        operation_id: Option<&str>,
        media_type: &str,
        size_bytes: u64,
        sha256: &str,
        classification: ArtifactClassification,
        authorized_readers: BTreeSet<String>,
        now_unix_ms: u64,
    ) -> Result<(RemoteArtifactManifest, ArtifactCapability), FabricError> {
        let limits = self.store().limits();
        let company_id = self.company_id().to_string();
        let signing_key = *self.capability_signing_key();
        self.store().transact(|state| {
            let lease = require_active_control_plane(
                state,
                &company_id,
                self.instance_id(),
                generation,
                now_unix_ms,
            )?;
            if lease.control_plane_generation != generation {
                return Err(FabricError::none(
                    FabricErrorCode::ControlPlaneStaleGeneration,
                    "Control Plane generation is stale",
                ));
            }
            actor.require_company_and_role(&company_id, "artifact_write", now_unix_ms)?;
            let node = state.nodes.get(source_node_id).ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::SourceMismatch,
                    "artifact source Node does not exist",
                )
            })?;
            if node.company_id != company_id
                || node.administrative_status == NodeAdministrativeStatus::Revoked
            {
                return Err(FabricError::none(
                    FabricErrorCode::NodeRevoked,
                    "artifact source Node is foreign or revoked",
                ));
            }
            if !node.allowed_capabilities.contains("artifact-transfer") {
                return Err(FabricError::none(
                    FabricErrorCode::FeatureIncompatible,
                    "artifact source Node lacks artifact-transfer capability",
                ));
            }
            if state.artifacts.contains_key(artifact_id)
                || size_bytes > limits.max_artifact_bytes
                || sha256.len() != 64
                || authorized_readers.is_empty()
                || !allowed_media_type(media_type)
            {
                return Err(FabricError::none(
                    FabricErrorCode::ArtifactInvalid,
                    "artifact manifest is duplicate, oversized, or incomplete",
                ));
            }
            if let Some(operation_id) = operation_id {
                let total = state
                    .artifacts
                    .values()
                    .filter(|artifact| artifact.operation_id.as_deref() == Some(operation_id))
                    .map(|artifact| artifact.size_bytes)
                    .sum::<u64>();
                if total.saturating_add(size_bytes) > limits.max_operation_artifact_bytes {
                    return Err(FabricError::none(
                        FabricErrorCode::ArtifactInvalid,
                        "operation artifact total exceeds 256 MiB",
                    ));
                }
            }
            let manifest = RemoteArtifactManifest {
                id: artifact_id.into(),
                company_id: company_id.clone(),
                source_node_id: source_node_id.into(),
                source_team_id: None,
                source_work_id: None,
                operation_id: operation_id.map(str::to_string),
                media_type: media_type.into(),
                size_bytes,
                sha256: sha256.into(),
                classification,
                initiator: actor.actor_id.clone(),
                authorized_readers,
                created_by: actor.actor_id.clone(),
                created_at_unix_ms: now_unix_ms,
                expires_at_unix_ms: None,
                completed_at_unix_ms: None,
                deleted_at_unix_ms: None,
                revision: 1,
                schema_version: FABRIC_SCHEMA_VERSION.into(),
            };
            let capability = issue_capability(
                &signing_key,
                &manifest,
                source_node_id,
                ArtifactCapabilityPurpose::Upload,
                &actor.actor_id,
                now_unix_ms,
                true,
            )?;
            state
                .artifacts
                .insert(manifest.id.clone(), manifest.clone());
            Ok((manifest, capability))
        })
    }

    pub fn complete_artifact(
        &self,
        generation: u64,
        capability: &ArtifactCapability,
        bytes: &[u8],
        now_unix_ms: u64,
    ) -> Result<RemoteArtifactManifest, FabricError> {
        let company_id = self.company_id().to_string();
        let signing_key = *self.capability_signing_key();
        let encryption_key = self.artifact_keys().key_for_company(&company_id)?;
        self.store().transact(|state| {
            require_active_control_plane(
                state,
                &company_id,
                self.instance_id(),
                generation,
                now_unix_ms,
            )?;
            verify_capability(
                state,
                &signing_key,
                capability,
                ArtifactCapabilityPurpose::Upload,
                now_unix_ms,
            )?;
            let manifest = state
                .artifacts
                .get(&capability.artifact_id)
                .cloned()
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ArtifactInvalid,
                        "artifact manifest does not exist",
                    )
                })?;
            if manifest.completed_at_unix_ms.is_some()
                || manifest.deleted_at_unix_ms.is_some()
                || manifest.company_id != company_id
                || manifest.source_node_id != capability.node_id
                || manifest.sha256 != capability.artifact_digest
                || manifest.size_bytes != bytes.len() as u64
                || sha256_hex(bytes) != manifest.sha256
                || contains_forbidden_payload(bytes)
            {
                return Err(FabricError::none(
                    FabricErrorCode::ArtifactTampered,
                    "artifact bytes, digest, size, scope, or lifecycle do not match",
                ));
            }
            let nonce_bytes = artifact_nonce(&manifest);
            let cipher = ChaCha20Poly1305::new(Key::from_slice(&encryption_key));
            let ciphertext = cipher
                .encrypt(Nonce::from_slice(&nonce_bytes), bytes)
                .map_err(|_| {
                    FabricError::none(
                        FabricErrorCode::StoreUnavailable,
                        "artifact encryption failed",
                    )
                })?;
            let mut next = manifest;
            next.completed_at_unix_ms = Some(now_unix_ms);
            next.revision = next.revision.saturating_add(1);
            state.encrypted_artifacts.insert(
                next.id.clone(),
                EncryptedArtifact {
                    nonce: nonce_bytes.to_vec(),
                    ciphertext,
                },
            );
            state.artifacts.insert(next.id.clone(), next.clone());
            consume_capability(state, capability);
            Ok(next)
        })
    }

    pub fn issue_download_capability(
        &self,
        actor: &AuthenticatedActor,
        generation: u64,
        artifact_id: &str,
        node_id: &str,
        now_unix_ms: u64,
    ) -> Result<ArtifactCapability, FabricError> {
        let company_id = self.company_id().to_string();
        let signing_key = *self.capability_signing_key();
        self.store().transact(|state| {
            require_active_control_plane(
                state,
                &company_id,
                self.instance_id(),
                generation,
                now_unix_ms,
            )?;
            actor.require_company_and_role(&company_id, "artifact_read", now_unix_ms)?;
            let manifest = state.artifacts.get(artifact_id).ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ArtifactInvalid,
                    "artifact manifest does not exist",
                )
            })?;
            if manifest.completed_at_unix_ms.is_none()
                || manifest.deleted_at_unix_ms.is_some()
                || !manifest.authorized_readers.contains(&actor.actor_id)
            {
                return Err(FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    "actor is not an authorized reader of a complete artifact",
                ));
            }
            let target = state.nodes.get(node_id).ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::TargetNotPlaced,
                    "artifact target Node is not enrolled",
                )
            })?;
            if target.company_id != company_id
                || target.administrative_status == NodeAdministrativeStatus::Revoked
            {
                return Err(FabricError::none(
                    FabricErrorCode::NodeRevoked,
                    "artifact target Node is foreign or revoked",
                ));
            }
            if !target.allowed_capabilities.contains("artifact-transfer") {
                return Err(FabricError::none(
                    FabricErrorCode::FeatureIncompatible,
                    "artifact target Node lacks artifact-transfer capability",
                ));
            }
            issue_capability(
                &signing_key,
                manifest,
                node_id,
                ArtifactCapabilityPurpose::Download,
                &actor.actor_id,
                now_unix_ms,
                true,
            )
        })
    }

    pub fn download_artifact(
        &self,
        generation: u64,
        capability: &ArtifactCapability,
        now_unix_ms: u64,
    ) -> Result<Vec<u8>, FabricError> {
        let company_id = self.company_id().to_string();
        let signing_key = *self.capability_signing_key();
        let encryption_key = self.artifact_keys().key_for_company(&company_id)?;
        self.store().transact(|state| {
            require_active_control_plane(
                state,
                &company_id,
                self.instance_id(),
                generation,
                now_unix_ms,
            )?;
            verify_capability(
                state,
                &signing_key,
                capability,
                ArtifactCapabilityPurpose::Download,
                now_unix_ms,
            )?;
            let manifest = state
                .artifacts
                .get(&capability.artifact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ArtifactInvalid,
                        "artifact manifest does not exist",
                    )
                })?;
            let encrypted = state
                .encrypted_artifacts
                .get(&capability.artifact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ArtifactInvalid,
                        "artifact ciphertext does not exist",
                    )
                })?;
            let cipher = ChaCha20Poly1305::new(Key::from_slice(&encryption_key));
            let plaintext = cipher
                .decrypt(
                    Nonce::from_slice(&encrypted.nonce),
                    encrypted.ciphertext.as_ref(),
                )
                .map_err(|_| {
                    FabricError::none(
                        FabricErrorCode::ArtifactTampered,
                        "artifact ciphertext authentication failed",
                    )
                })?;
            if sha256_hex(&plaintext) != manifest.sha256 {
                return Err(FabricError::none(
                    FabricErrorCode::ArtifactTampered,
                    "decrypted artifact digest mismatch",
                ));
            }
            consume_capability(state, capability);
            Ok(plaintext)
        })
    }
}

fn issue_capability(
    signing_key: &[u8; 32],
    manifest: &RemoteArtifactManifest,
    node_id: &str,
    purpose: ArtifactCapabilityPurpose,
    issued_to: &str,
    now_unix_ms: u64,
    one_use: bool,
) -> Result<ArtifactCapability, FabricError> {
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        company_id: &'a str,
        node_id: &'a str,
        artifact_id: &'a str,
        artifact_digest: &'a str,
        purpose: ArtifactCapabilityPurpose,
        issued_to: &'a str,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        one_use: bool,
    }
    let expires_at_unix_ms = now_unix_ms.saturating_add(5 * 60 * 1000);
    let claims = Claims {
        company_id: &manifest.company_id,
        node_id,
        artifact_id: &manifest.id,
        artifact_digest: &manifest.sha256,
        purpose,
        issued_to,
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms,
        one_use,
    };
    let claims_digest = canonical_digest(&claims)?;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(signing_key).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "capability signing key is invalid",
        )
    })?;
    mac.update(claims_digest.as_bytes());
    let signature = bytes_to_hex(&mac.finalize().into_bytes());
    Ok(ArtifactCapability {
        token: format!("{claims_digest}.{signature}"),
        company_id: manifest.company_id.clone(),
        node_id: node_id.into(),
        artifact_id: manifest.id.clone(),
        artifact_digest: manifest.sha256.clone(),
        purpose,
        issued_to: issued_to.into(),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms,
        one_use,
    })
}

fn verify_capability(
    state: &crate::store::FabricState,
    signing_key: &[u8; 32],
    capability: &ArtifactCapability,
    purpose: ArtifactCapabilityPurpose,
    now_unix_ms: u64,
) -> Result<(), FabricError> {
    if capability.purpose != purpose {
        return Err(FabricError::none(
            FabricErrorCode::CapabilityInvalid,
            "artifact capability purpose mismatch",
        ));
    }
    if capability.expires_at_unix_ms <= now_unix_ms {
        return Err(FabricError::none(
            FabricErrorCode::CapabilityExpired,
            "artifact capability expired",
        ));
    }
    let token_digest = sha256_hex(capability.token.as_bytes());
    if state.consumed_capabilities.contains(&token_digest) {
        return Err(FabricError::none(
            FabricErrorCode::CapabilityConsumed,
            "artifact capability was already consumed",
        ));
    }
    let (claims_digest, signature_hex) = capability.token.split_once('.').ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::CapabilityInvalid,
            "artifact capability token is malformed",
        )
    })?;
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        company_id: &'a str,
        node_id: &'a str,
        artifact_id: &'a str,
        artifact_digest: &'a str,
        purpose: ArtifactCapabilityPurpose,
        issued_to: &'a str,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        one_use: bool,
    }
    let expected_claims = canonical_digest(&Claims {
        company_id: &capability.company_id,
        node_id: &capability.node_id,
        artifact_id: &capability.artifact_id,
        artifact_digest: &capability.artifact_digest,
        purpose: capability.purpose,
        issued_to: &capability.issued_to,
        issued_at_unix_ms: capability.issued_at_unix_ms,
        expires_at_unix_ms: capability.expires_at_unix_ms,
        one_use: capability.one_use,
    })?;
    if claims_digest != expected_claims {
        return Err(FabricError::none(
            FabricErrorCode::CapabilityInvalid,
            "artifact capability claims were modified",
        ));
    }
    let signature = decode_hex(signature_hex).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::CapabilityInvalid,
            "artifact capability signature is malformed",
        )
    })?;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(signing_key).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "capability signing key is invalid",
        )
    })?;
    mac.update(claims_digest.as_bytes());
    mac.verify_slice(&signature).map_err(|_| {
        FabricError::none(
            FabricErrorCode::CapabilityInvalid,
            "artifact capability signature is invalid",
        )
    })
}

fn consume_capability(state: &mut crate::store::FabricState, capability: &ArtifactCapability) {
    if capability.one_use || capability.purpose == ArtifactCapabilityPurpose::Upload {
        state
            .consumed_capabilities
            .insert(sha256_hex(capability.token.as_bytes()));
    }
}

fn artifact_nonce(manifest: &RemoteArtifactManifest) -> [u8; 12] {
    let digest = Sha256::digest(
        format!(
            "{}:{}:{}",
            manifest.company_id, manifest.id, manifest.sha256
        )
        .as_bytes(),
    );
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&digest[..12]);
    nonce
}

fn allowed_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/json"
            | "application/octet-stream"
            | "text/plain"
            | "image/png"
            | "image/jpeg"
            | "image/webp"
    )
}

fn contains_forbidden_payload(bytes: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    [
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "api_key=",
        "api-key:",
        "authorization: bearer ",
        "chain_of_thought",
        "provider transcript",
        ".git/objects/",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}
