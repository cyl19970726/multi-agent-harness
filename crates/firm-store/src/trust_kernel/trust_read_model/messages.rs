use super::*;
type DeliveryRows = BTreeMap<String, (u64, CanonicalMessageDelivery)>;

#[derive(Clone, Default)]
pub(super) struct MessageSources {
    messages: BTreeMap<(String, String), BTreeMap<String, serde_json::Value>>,
    deliveries: BTreeMap<String, BTreeMap<String, CanonicalMessageDelivery>>,
    by_message: BTreeMap<(String, String), DeliveryRows>,
    ordinal: u64,
}
impl MessageSources {
    pub(super) fn observe(&mut self, e: &TrustOperationEnvelope) {
        let op = &e.operation;
        if op.event.aggregate_kind == "message" {
            if let Some(run) = op.resulting_projection["team_run_id"].as_str() {
                self.messages
                    .entry((e.execution_space_id.clone(), run.to_owned()))
                    .or_default()
                    .insert(
                        op.event.aggregate_id.clone(),
                        op.resulting_projection.clone(),
                    );
            }
        }
        for record in op
            .initial_outbox_records
            .iter()
            .chain(&op.immutable_side_records)
        {
            if let Ok(delivery) = serde_json::from_value::<CanonicalMessageDelivery>(record.clone())
            {
                self.by_message
                    .entry((e.execution_space_id.clone(), delivery.message_id.clone()))
                    .or_default()
                    .insert(delivery.id.clone(), (self.ordinal, delivery.clone()));
                self.deliveries
                    .entry(e.execution_space_id.clone())
                    .or_default()
                    .insert(delivery.id.clone(), delivery);
                self.ordinal += 1;
            }
        }
    }
    pub(super) fn messages(&self, space: &str, run: &str) -> StoreResult<Vec<Message>> {
        // Match the original filter-before-latest-before-decode semantics.
        self.messages
            .get(&(space.to_owned(), run.to_owned()))
            .into_iter()
            .flat_map(|rows| rows.values())
            .map(|value| serde_json::from_value(value.clone()).map_err(StoreError::from))
            .collect()
    }
    pub(super) fn deliveries(
        &self,
        space: &str,
        message_ids: Option<&std::collections::HashSet<String>>,
    ) -> Vec<CanonicalMessageDelivery> {
        let Some(ids) = message_ids else {
            return self
                .deliveries
                .get(space)
                .map(|rows| rows.values().cloned().collect())
                .unwrap_or_default();
        };
        let mut latest: BTreeMap<&str, &(u64, CanonicalMessageDelivery)> = BTreeMap::new();
        for id in ids {
            if let Some(rows) = self.by_message.get(&(space.to_owned(), id.clone())) {
                for (delivery_id, row) in rows {
                    if latest
                        .get(delivery_id.as_str())
                        .is_none_or(|old| row.0 > old.0)
                    {
                        latest.insert(delivery_id, row);
                    }
                }
            }
        }
        latest.into_values().map(|(_, row)| row.clone()).collect()
    }
}
