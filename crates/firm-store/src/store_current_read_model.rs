//! Current queries share source folds; history and mutation authority remain
//! in their original ledgers. Every memo depends on freshly observed sources.
use super::*;
use std::sync::Arc;

pub(super) struct CurrentWorkSources {
    pub operations: Vec<WorkOperation>,
    /// The same rows with immutable additive provenance folded through them.
    /// Both the latest-Work fold and the Work journal read this, so a store
    /// pays for the recovery once.
    pub recovered: Result<Vec<WorkOperation>, String>,
}

impl HarnessStore {
    pub(super) fn current_work_sources(&self) -> StoreResult<Arc<CurrentWorkSources>> {
        let ordinary = self.cached_jsonl_source_fold(
            "work_operations.jsonl",
            false,
            |rows: &mut Vec<WorkOperation>, row: &WorkOperation| rows.push(row.clone()),
        )?;
        self.cached_combined_projection("work-current-sources", vec![ordinary.clone()], || {
            let operations = (*ordinary).clone();
            let recovered = self
                .recover_work_operation_provenance(operations.clone())
                .map_err(|error| match error {
                    StoreError::Conflict(message) => message,
                    other => other.to_string(),
                });
            Ok(CurrentWorkSources {
                operations,
                recovered,
            })
        })
    }

    pub(super) fn current_host_attention_projection(
        &self,
    ) -> StoreResult<Arc<std::collections::BTreeMap<String, HostAttention>>> {
        let trust = self.trust_read_model()?;
        let lifecycle = self.cached_jsonl_source_fold(
            "host_attentions.jsonl",
            false,
            |rows: &mut Vec<HostAttention>, row: &HostAttention| rows.push(row.clone()),
        )?;
        self.cached_combined_projection(
            "host-attention-current",
            vec![trust.clone(), lifecycle.clone()],
            || {
                let mut latest = trust.host_attention_sources()?;
                for attention in lifecycle.iter() {
                    let decision = firm_application::fold_host_attention_lifecycle(
                        latest.get(&attention.id),
                        attention,
                    )
                    .map_err(|error| {
                        let code = if error
                            == firm_application::ProjectionFoldViolation::ImmutableIdentityConflict
                        {
                            "HOST_ATTENTION_SOURCE_FACT_CONFLICT"
                        } else {
                            "HOST_ATTENTION_LIFECYCLE_FOLD_CONFLICT"
                        };
                        StoreError::Conflict(format!(
                            "{code}: lifecycle projection {}: {error}",
                            attention.id
                        ))
                    })?;
                    if decision != firm_application::ProjectionFoldDecision::Replay {
                        latest.insert(attention.id.clone(), attention.clone());
                    }
                }
                Ok(latest)
            },
        )
    }
}
