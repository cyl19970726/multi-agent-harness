use super::*;

impl HarnessStore {
    pub fn fabric_work_execution_bindings_for_works(
        &self,
        execution_space_id: &str,
        work_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<WorkExecutionBinding>> {
        let mut latest = BTreeMap::<String, WorkExecutionBinding>::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id != execution_space_id {
                continue;
            }
            if envelope.operation.event.aggregate_kind == "work_execution_binding"
                && envelope.operation.resulting_projection["work_id"]
                    .as_str()
                    .is_some_and(|id| work_ids.contains(id))
            {
                let binding = event_projection::<WorkExecutionBinding>(&envelope)?;
                latest.insert(binding.id.clone(), binding);
            }
            for record in envelope.operation.immutable_side_records {
                if !record["work_id"]
                    .as_str()
                    .is_some_and(|id| work_ids.contains(id))
                {
                    continue;
                }
                if let Ok(binding) = serde_json::from_value::<WorkExecutionBinding>(record) {
                    let replace = latest
                        .get(&binding.id)
                        .is_none_or(|current| binding.version > current.version);
                    if replace {
                        latest.insert(binding.id.clone(), binding);
                    }
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn fabric_work_deliveries_for_works(
        &self,
        execution_space_id: &str,
        work_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<CanonicalWorkDelivery>> {
        let mut latest = BTreeMap::<String, CanonicalWorkDelivery>::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in envelope
                .operation
                .initial_outbox_records
                .iter()
                .chain(&envelope.operation.immutable_side_records)
                .filter(|value| {
                    value["work_id"]
                        .as_str()
                        .is_some_and(|id| work_ids.contains(id))
                })
            {
                if let Ok(delivery) = serde_json::from_value::<CanonicalWorkDelivery>(value.clone())
                {
                    latest.insert(delivery.id.clone(), delivery);
                }
            }
        }
        Ok(latest.into_values().collect())
    }
}
