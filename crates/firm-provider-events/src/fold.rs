use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    ProviderNativeEventRecord, SessionEventProjection, TeamRuntimeActivity,
    PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProviderEventFoldError {
    #[error("observation schema version is unsupported")]
    UnsupportedVersion,
    #[error("observation identity conflicts with a different payload")]
    IdentityConflict,
    #[error("observation belongs to a different AgentSession authority")]
    AuthorityMismatch,
    #[error("observation has an impossible ordering position")]
    InvalidOrdering,
    #[error("observation semantic contract is invalid: {0}")]
    InvalidObservation(#[from] crate::ObservationValidationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldOutcome {
    Inserted,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEpisode {
    pub episode_id: String,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    pub records: Vec<ProviderNativeEventRecord>,
    pub terminal: bool,
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEventFold {
    pub schema_version: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    records: BTreeMap<String, ProviderNativeEventRecord>,
    source_fingerprints: BTreeMap<String, String>,
}

impl ProviderEventFold {
    pub fn new(
        agent_session_id: impl Into<String>,
        agent_session_generation: u64,
        node_daemon_id: impl Into<String>,
        node_daemon_generation: u64,
    ) -> Self {
        Self {
            schema_version: PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION.into(),
            agent_session_id: agent_session_id.into(),
            agent_session_generation,
            node_daemon_id: node_daemon_id.into(),
            node_daemon_generation,
            records: BTreeMap::new(),
            source_fingerprints: BTreeMap::new(),
        }
    }

    pub fn ingest(
        &mut self,
        record: ProviderNativeEventRecord,
    ) -> Result<FoldOutcome, ProviderEventFoldError> {
        record.validate()?;
        if record.agent_session_id != self.agent_session_id
            || record.agent_session_generation != self.agent_session_generation
            || record.node_daemon_id != self.node_daemon_id
            || record.node_daemon_generation != self.node_daemon_generation
        {
            return Err(ProviderEventFoldError::AuthorityMismatch);
        }
        if let Some(existing) = self.records.get(&record.record_id) {
            return if existing == &record {
                Ok(FoldOutcome::Replay)
            } else {
                Err(ProviderEventFoldError::IdentityConflict)
            };
        }
        self.source_fingerprints.insert(
            record.record_id.clone(),
            record.source_content_fingerprint.clone(),
        );
        self.records.insert(record.record_id.clone(), record);
        Ok(FoldOutcome::Inserted)
    }

    pub fn session_projection(&self, limit: usize) -> SessionEventProjection {
        let sorted = self.sorted_records();
        let truncated = sorted.len() > limit;
        let visible = if truncated {
            &sorted[sorted.len() - limit..]
        } else {
            &sorted[..]
        };
        let mut episodes = Vec::<SessionEpisode>::new();
        let mut episode_indexes = BTreeMap::<String, usize>::new();
        for record in visible {
            let episode_id = record
                .provider_turn_id
                .clone()
                .unwrap_or_else(|| format!("unscoped:{}", record.ordering_position));
            let index = *episode_indexes
                .entry(episode_id.clone())
                .or_insert_with(|| {
                    episodes.push(SessionEpisode {
                        provider_turn_id: record.provider_turn_id.clone(),
                        episode_id,
                        records: Vec::new(),
                        terminal: false,
                        incomplete: false,
                    });
                    episodes.len() - 1
                });
            let episode = &mut episodes[index];
            episode.terminal |= record.fragments.iter().any(|fragment| {
                matches!(
                    fragment.semantic_kind,
                    crate::SemanticKind::TurnCompleted
                        | crate::SemanticKind::TurnFailed
                        | crate::SemanticKind::TurnCancelled
                )
            });
            episode.incomplete |= record.fragments.iter().any(|fragment| {
                matches!(
                    fragment.completeness,
                    crate::Completeness::Incomplete | crate::Completeness::RecoveryRequired
                )
            });
            episode.records.push((*record).clone());
        }
        for episode in &mut episodes {
            // A visible episode without a terminal observation is explicitly
            // incomplete, even when every individual streaming row was valid.
            episode.incomplete |= !episode.terminal;
        }
        SessionEventProjection {
            schema_version: PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION.into(),
            agent_session_id: self.agent_session_id.clone(),
            agent_session_generation: self.agent_session_generation,
            source_snapshot_fingerprint: self.snapshot_fingerprint(),
            episodes,
            truncated,
            availability: crate::SessionProjectionAvailability::Available,
            unavailable_reason_code: None,
            disabled_reason: None,
        }
    }

    pub fn team_public_projection(&self) -> Vec<TeamRuntimeActivity> {
        self.sorted_records()
            .into_iter()
            .flat_map(|record| {
                record
                    .fragments
                    .iter()
                    .filter(|fragment| {
                        ProviderNativeEventRecord::is_team_public_allowlisted(fragment)
                    })
                    .map(|fragment| TeamRuntimeActivity {
                        record_id: record.record_id.clone(),
                        fragment_id: fragment.fragment_id.clone(),
                        agent_member_id: record.agent_member_id.clone(),
                        semantic_kind: fragment.semantic_kind,
                        lifecycle_phase: fragment.lifecycle_phase,
                        completeness: fragment.completeness,
                        effect_certainty: fragment.effect_certainty,
                        occurred_at: record.occurred_at.clone(),
                        payload: fragment.payload.clone(),
                    })
            })
            .collect()
    }

    pub fn snapshot_fingerprint(&self) -> String {
        let mut digest = Sha256::new();
        for (id, fingerprint) in &self.source_fingerprints {
            digest.update(id.as_bytes());
            digest.update([0]);
            digest.update(fingerprint.as_bytes());
            digest.update([0xff]);
        }
        format!("sha256:{:x}", digest.finalize())
    }

    fn sorted_records(&self) -> Vec<&ProviderNativeEventRecord> {
        let mut values = self.records.values().collect::<Vec<_>>();
        values.sort_by(|left, right| {
            left.ordering_position
                .cmp(&right.ordering_position)
                .then(left.record_id.cmp(&right.record_id))
        });
        values
    }

    pub fn runtime_command_ids(&self) -> BTreeSet<&str> {
        self.records
            .values()
            .filter_map(|item| item.runtime_command_id.as_deref())
            .collect()
    }
}
