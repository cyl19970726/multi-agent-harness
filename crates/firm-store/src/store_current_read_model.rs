//! Current queries share source folds; history and mutation authority remain
//! in their original ledgers. Every memo depends on freshly observed sources.
use super::*;
use std::sync::Arc;

pub(super) struct CurrentWorkSources {
    pub operations: Vec<WorkOperation>,
    pub latest: Result<std::collections::BTreeMap<String, Work>, String>,
    pub attention_sources: Result<Vec<(bool, HostAttention)>, String>,
    pub responsibility_versions: std::collections::BTreeMap<String, u64>,
}

pub(super) fn merge_work_operation_sources(
    mut operations: Vec<WorkOperation>,
    mut delegated: Vec<WorkOperation>,
) -> Vec<WorkOperation> {
    // WorkDelegation creation is crash-atomic in a separate composite
    // ledger, while later target transitions use the ordinary Work ledger.
    // Concatenating files would place every delegated Work's version 1
    // after its later versions and make the projection regress. Preserve
    // the ordinary ledger's exact append order (the durable `--since`
    // cursor), then insert each composite creation at its temporal slot
    // and always before any later revision of that same Work.
    delegated.sort_by(|left, right| work_event_order(&left.event, &right.event));
    for operation in delegated {
        let same_work = operations
            .iter()
            .position(|existing| existing.work.id == operation.work.id)
            .unwrap_or(operations.len());
        let temporal = operations
            .iter()
            .position(|existing| work_event_order(&operation.event, &existing.event).is_lt())
            .unwrap_or(operations.len());
        operations.insert(same_work.min(temporal), operation);
    }
    operations
}

impl HarnessStore {
    pub(super) fn current_work_sources(&self) -> StoreResult<Arc<CurrentWorkSources>> {
        let ordinary = self.cached_jsonl_source_fold(
            "work_operations.jsonl",
            false,
            |rows: &mut Vec<WorkOperation>, row: &WorkOperation| rows.push(row.clone()),
        )?;
        let delegated = self.cached_jsonl_source_fold(
            "work_delegation_operations.jsonl",
            false,
            |rows: &mut Vec<WorkOperation>, row: &WorkDelegationOperation| {
                rows.push(row.target_work_operation.clone())
            },
        )?;
        self.cached_combined_projection(
            "work-current-sources",
            vec![ordinary.clone(), delegated.clone()],
            || {
                let operations =
                    merge_work_operation_sources((*ordinary).clone(), (*delegated).clone());
                let latest = self
                    .recover_work_operation_provenance(operations.clone())
                    .map(|recovered| {
                        latest_by_id(recovered, |op| op.work.id.clone())
                            .into_iter()
                            .map(|(id, op)| (id, op.work))
                            .collect()
                    })
                    .map_err(|error| match error {
                        StoreError::Conflict(message) => message,
                        other => other.to_string(),
                    });
                let attention_sources = (|| -> StoreResult<Vec<(bool, HostAttention)>> {
                    let mut sources = Vec::new();
                    for operation in &operations {
                        sources.extend(
                            Self::downstream_host_attentions_for_work_operation(operation)?
                                .into_iter()
                                .map(|row| (true, row)),
                        );
                        if let Some(row) = Self::host_attention_for_work_operation(operation) {
                            sources.push((false, row));
                        }
                    }
                    Ok(sources)
                })()
                .map_err(|error| match error {
                    StoreError::Conflict(message) => message,
                    other => other.to_string(),
                });
                let mut responsibility_versions = std::collections::BTreeMap::<String, u64>::new();
                for operation in &operations {
                    if matches!(
                        operation.event.kind,
                        WorkEventKind::Assigned
                            | WorkEventKind::Claimed
                            | WorkEventKind::Released
                            | WorkEventKind::Rebound
                            | WorkEventKind::ExecutionRetargeted
                            | WorkEventKind::ExecutionRecovered
                    ) {
                        let version = responsibility_versions
                            .entry(operation.work.id.clone())
                            .or_default();
                        *version = (*version).max(operation.event.resulting_version);
                    }
                }
                Ok(CurrentWorkSources {
                    operations,
                    latest,
                    responsibility_versions,
                    attention_sources,
                })
            },
        )
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
