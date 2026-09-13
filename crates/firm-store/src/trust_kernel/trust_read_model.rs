//! Disposable source projections built during the same Trust envelope scan as
//! the latest-kind index. No clock/lease decision is memoized here.
use super::*;
use firm_core::{WorkEvent, WorkEventKind};
use std::sync::Arc;
#[path = "trust_read_model/messages.rs"]
mod messages;

#[derive(Clone, Default)]
pub(crate) struct TrustReadModel {
    messages: messages::MessageSources,
    sequences: BTreeMap<String, u64>,
    team_scopes: BTreeMap<String, String>,
    memberships: BTreeMap<String, BTreeMap<String, TeamMembership>>,
    memberships_by_team: BTreeMap<(String, String), BTreeMap<String, TeamMembership>>,
    invalid_memberships: BTreeMap<String, Vec<serde_json::Value>>,
    attention_outbox: Vec<HostAttention>,
    runtime_bindings: BTreeMap<(String, String), Vec<serde_json::Value>>,
    work_revisions: BTreeMap<String, Vec<Work>>,
    latest_work: BTreeMap<String, Work>,
    invalid_work: Option<serde_json::Value>,
    work_journal: Vec<crate::work_history::WorkJournalRecord>,
    work_journal_seen: BTreeMap<(String, u64), usize>,
    work_journal_error: Option<String>,
    attentions: BTreeMap<String, BTreeMap<String, HostAttention>>,
    attention_errors: BTreeMap<String, String>,
    bindings: BTreeMap<String, BTreeMap<String, WorkExecutionBinding>>,
    invalid_bindings: BTreeMap<String, serde_json::Value>,
    deliveries: BTreeMap<String, BTreeMap<String, CanonicalWorkDelivery>>,
    delivery_errors: BTreeMap<String, String>,
}

impl TrustReadModel {
    pub(super) fn observe(&mut self, e: &TrustOperationEnvelope) {
        self.messages.observe(e);
        let space = &e.execution_space_id;
        let op = &e.operation;
        if op.event.aggregate_kind == "agent_team" {
            self.team_scopes
                .insert(op.event.aggregate_id.clone(), space.clone());
        }
        if op.event.aggregate_kind == "team_membership" {
            match serde_json::from_value::<TeamMembership>(op.resulting_projection.clone()) {
                Ok(membership) => self.observe_membership(space, membership),
                Err(_) => self
                    .invalid_memberships
                    .entry(space.clone())
                    .or_default()
                    .push(op.resulting_projection.clone()),
            }
        }
        for value in &op.initial_outbox_records {
            if let Ok(attention) = serde_json::from_value::<HostAttention>(value.clone()) {
                self.attention_outbox.push(attention);
            }
        }
        if op.event.aggregate_kind == "work_execution_binding" && op.event.transition == "bound" {
            self.runtime_bindings
                .entry((space.clone(), op.event.aggregate_id.clone()))
                .or_default()
                .push(op.event.payload["runtime_binding"].clone());
        }

        let seq = self.sequences.entry(space.clone()).or_default();
        *seq = (*seq).max(op.event.store_sequence);
        if let Ok(work) = serde_json::from_value::<Work>(op.resulting_projection.clone()) {
            self.work_revisions
                .entry(space.clone())
                .or_default()
                .push(work.clone());
            if op.event.aggregate_kind == "work" {
                self.observe_latest_work(work);
            }
        } else if op.event.aggregate_kind == "work" && self.invalid_work.is_none() {
            self.invalid_work = Some(op.resulting_projection.clone());
        }
        self.observe_work_journal(space, op);
        if op.event.aggregate_kind == "work_execution_binding" {
            match serde_json::from_value::<WorkExecutionBinding>(op.resulting_projection.clone()) {
                Ok(binding) => {
                    self.bindings
                        .entry(space.clone())
                        .or_default()
                        .insert(binding.id.clone(), binding);
                }
                Err(_) => {
                    self.invalid_bindings
                        .entry(space.clone())
                        .or_insert_with(|| op.resulting_projection.clone());
                }
            }
        }
        for record in &op.immutable_side_records {
            if let Ok(work) = serde_json::from_value::<Work>(record.clone()) {
                self.work_revisions
                    .entry(space.clone())
                    .or_default()
                    .push(work.clone());
                self.observe_latest_work(work);
            }
            if let Ok(binding) = serde_json::from_value::<WorkExecutionBinding>(record.clone()) {
                let bindings = self.bindings.entry(space.clone()).or_default();
                if bindings
                    .get(&binding.id)
                    .is_none_or(|old| binding.version > old.version)
                {
                    bindings.insert(binding.id.clone(), binding);
                }
            }
        }
        // Preserve the original trust_side_records order: outbox, then side.
        for record in op
            .initial_outbox_records
            .iter()
            .chain(&op.immutable_side_records)
        {
            if let Ok(membership) = serde_json::from_value::<TeamMembership>(record.clone()) {
                self.observe_membership(space, membership);
            }

            if let Ok(attention) = serde_json::from_value::<HostAttention>(record.clone()) {
                let sources = self.attentions.entry(space.clone()).or_default();
                match firm_application::fold_host_attention_source(
                    sources.get(&attention.id),
                    &attention,
                ) {
                    Ok(firm_application::ProjectionFoldDecision::Replay) => {}
                    Ok(_) => {
                        sources.insert(attention.id.clone(), attention);
                    }
                    Err(error) => {
                        self.attention_errors.entry(space.clone()).or_insert_with(|| format!("HOST_ATTENTION_SOURCE_FACT_CONFLICT: canonical source {}: {error}", attention.id));
                    }
                }
            }
            if let Ok(delivery) = serde_json::from_value::<CanonicalWorkDelivery>(record.clone()) {
                let deliveries = self.deliveries.entry(space.clone()).or_default();
                match firm_application::fold_canonical_work_delivery(
                    deliveries.get(&delivery.id),
                    &delivery,
                ) {
                    Ok(firm_application::ProjectionFoldDecision::Replay) => {}
                    Ok(_) => {
                        deliveries.insert(delivery.id.clone(), delivery);
                    }
                    Err(error) => {
                        self.delivery_errors
                            .entry(space.clone())
                            .or_insert_with(|| {
                                format!(
                                    "CANONICAL_WORK_DELIVERY_FOLD_CONFLICT: delivery {}: {error}",
                                    delivery.id
                                )
                            });
                    }
                }
            }
        }
    }
    pub(crate) fn latest_works(&self) -> StoreResult<Vec<Work>> {
        if let Some(value) = &self.invalid_work {
            let _: Work = serde_json::from_value(value.clone())?;
        }
        Ok(self.latest_work.values().cloned().collect())
    }
    pub(crate) fn work_revisions_for_space(&self, space: &str) -> Vec<Work> {
        self.work_revisions.get(space).cloned().unwrap_or_default()
    }
    pub(crate) fn outbox_sources(&self) -> Vec<HostAttention> {
        self.attention_outbox.clone()
    }
    pub(crate) fn host_attention_sources(&self) -> StoreResult<BTreeMap<String, HostAttention>> {
        let mut all = BTreeMap::new();
        for (space, sources) in &self.attentions {
            if let Some(error) = self.attention_errors.get(space) {
                return Err(StoreError::Conflict(error.clone()));
            }
            for source in sources.values() {
                let decision =
                    firm_application::fold_host_attention_source(all.get(&source.id), source)
                        .map_err(|error| {
                            StoreError::Conflict(format!(
                                "HOST_ATTENTION_SOURCE_FACT_CONFLICT: canonical source {}: {error}",
                                source.id
                            ))
                        })?;
                if decision != firm_application::ProjectionFoldDecision::Replay {
                    all.insert(source.id.clone(), source.clone());
                }
            }
        }
        Ok(all)
    }
    fn observe_membership(&mut self, space: &str, membership: TeamMembership) {
        self.memberships_by_team
            .entry((space.to_owned(), membership.team_id.clone()))
            .or_default()
            .insert(membership.id.clone(), membership.clone());
        self.memberships
            .entry(space.to_owned())
            .or_default()
            .insert(membership.id.clone(), membership);
    }
    /// Materialize the Work revisions this canonical operation produced as
    /// WorkOperation-shaped records, so the one Work reader sees the same
    /// chain the ledger exposes. Current writers commit a real `WorkEvent`
    /// side record with the transition; a pre-W3 envelope carries none, and
    /// its event is derived from the canonical event it already persisted.
    /// Nothing here is ever written back: this is a read projection.
    fn observe_work_journal(&mut self, space: &str, op: &CanonicalOperation) {
        if op.event.aggregate_kind == "work" {
            let Ok(work) = serde_json::from_value::<Work>(op.resulting_projection.clone()) else {
                return;
            };
            let kind = match work_transition_event_kind(&op.event.transition) {
                Some(kind) => kind,
                None => {
                    self.work_journal_error.get_or_insert_with(|| {
                        format!(
                            "WORK_JOURNAL_UNKNOWN_TRANSITION: canonical Work operation {} names transition {:?}, which this binary cannot read as a WorkEvent",
                            op.event.id, op.event.transition
                        )
                    });
                    return;
                }
            };
            let event = committed_work_event(op, &work)
                .unwrap_or_else(|| derived_work_event(op, &work, kind, op.event.expected_version));
            self.record_work_journal(space, work, event, true);
            return;
        }
        // Pre-cutover Result submission: the Review revision exists only as an
        // immutable side record of the report envelope. Current writers pair
        // that envelope with a `work`/`submitted` operation in the same atomic
        // write; the pair is deduplicated on (Work, version) below.
        if op.event.aggregate_kind != "work_report" || op.event.transition != "created" {
            return;
        }
        let Some(work_id) = op.resulting_projection["work_id"].as_str() else {
            return;
        };
        let revision = op.resulting_projection["work_revision"].as_u64();
        for record in &op.immutable_side_records {
            let Ok(work) = serde_json::from_value::<Work>(record.clone()) else {
                continue;
            };
            if work.id != work_id || Some(work.version) != revision {
                continue;
            }
            let event = derived_work_event(
                op,
                &work,
                WorkEventKind::Submitted,
                work.version.saturating_sub(1),
            );
            self.record_work_journal(space, work, event, false);
        }
    }

    /// Keep exactly one record per (Work, version). A `work`-aggregate record
    /// is the authority for its revision and replaces a report-derived one.
    fn record_work_journal(
        &mut self,
        space: &str,
        work: Work,
        event: WorkEvent,
        from_work_aggregate: bool,
    ) {
        let key = (work.id.clone(), event.resulting_version);
        let record = crate::work_history::WorkJournalRecord {
            source: crate::work_history::WorkJournalSource::Trust,
            execution_space_id: Some(space.to_owned()),
            event,
            work,
        };
        match self.work_journal_seen.get(&key) {
            Some(&index) => {
                if from_work_aggregate {
                    self.work_journal[index] = record;
                }
            }
            None => {
                self.work_journal_seen.insert(key, self.work_journal.len());
                self.work_journal.push(record);
            }
        }
    }

    pub(crate) fn work_journal_records(
        &self,
    ) -> StoreResult<Vec<crate::work_history::WorkJournalRecord>> {
        if let Some(error) = &self.work_journal_error {
            return Err(StoreError::Conflict(error.clone()));
        }
        Ok(self.work_journal.clone())
    }

    fn observe_latest_work(&mut self, work: Work) {
        if self
            .latest_work
            .get(&work.id)
            .is_none_or(|old| old.version < work.version)
        {
            self.latest_work.insert(work.id.clone(), work);
        }
    }
}
/// Every `work`-aggregate transition that may appear in a store, and the
/// WorkEvent kind it means.
///
/// What binds this map is not what this binary writes — it is what any binary
/// ever wrote into a store this one may read. `submitted`, `accepted`,
/// `cancelled` and `dependencies_changed` are the current writers;
/// `updated` is carried for the canonical Work-update shape. An unrecognized
/// transition is a Work revision this binary cannot name honestly, so the Work
/// journal fails closed rather than labelling it, and the latest-Work fold is
/// deliberately independent of this map so such a store still reads its
/// current Work.
///
/// The one retired transition needs no arm here: a pre-W1 `failed` envelope
/// carries `resolution: "failed"`, which `WorkResolution` no longer decodes, so
/// the Work decode above refuses it before this map is consulted.
fn work_transition_event_kind(transition: &str) -> Option<WorkEventKind> {
    Some(match transition {
        "submitted" => WorkEventKind::Submitted,
        "accepted" => WorkEventKind::Accepted,
        "cancelled" => WorkEventKind::Cancelled,
        "dependencies_changed" => WorkEventKind::DependenciesChanged,
        "updated" => WorkEventKind::Updated,
        _ => return None,
    })
}

/// The WorkEvent a current writer committed alongside its canonical Work
/// transition, when it bound this exact revision.
fn committed_work_event(op: &CanonicalOperation, work: &Work) -> Option<WorkEvent> {
    op.immutable_side_records.iter().find_map(|record| {
        serde_json::from_value::<WorkEvent>(record.clone())
            .ok()
            .filter(|event| event.work_id == work.id && event.resulting_version == work.version)
    })
}

/// Read a pre-W3 canonical operation as the WorkEvent it already implies. The
/// canonical actor model has no Host kind, so a derived actor is never Host:
/// no authority check can be widened by reading these rows.
pub(super) fn derived_work_event(
    op: &CanonicalOperation,
    work: &Work,
    kind: WorkEventKind,
    expected_version: u64,
) -> WorkEvent {
    let actor = |actor: firm_core::agentfirm_api::ActorRef| TeamActorRef {
        kind: match actor.kind {
            firm_core::agentfirm_api::ActorKind::AgentMember => TeamActorKind::AgentMember,
            firm_core::agentfirm_api::ActorKind::Service => TeamActorKind::Service,
            _ => TeamActorKind::Operator,
        },
        id: actor.id,
        display_name: None,
        authn_source: Some("canonical-work-event".into()),
    };
    WorkEvent {
        id: op.event.id.clone(),
        team_run_id: work.team_run_id.clone(),
        work_id: work.id.clone(),
        sequence: work.version,
        kind,
        expected_version,
        resulting_version: work.version,
        performed_by_actor: actor(op.event.performed_by_actor.clone()),
        authority_actor: op.event.authority_actor.clone().map(actor),
        causation_ref: None,
        idempotency_key: op.event.idempotency_key.clone(),
        payload: op.event.payload.clone(),
        created_at: op.event.created_at.clone(),
    }
}

impl HarnessStore {
    pub(crate) fn trust_read_model(&self) -> StoreResult<Arc<TrustReadModel>> {
        Ok(self
            .cached_latest_jsonl_derived(
                TRUST_OPERATIONS_LEDGER,
                trust_cache_key,
                |e| e.execution_space_id.clone(),
                crate::store_read_cache::CacheSelection::Groups,
                TrustReadModel::observe,
            )?
            .2)
    }
    pub(crate) fn cached_messages_for_team_run(
        &self,
        space: &str,
        run: &str,
    ) -> StoreResult<Vec<Message>> {
        self.trust_read_model()?.messages.messages(space, run)
    }
    pub(crate) fn cached_message_deliveries(
        &self,
        space: &str,
        message_ids: Option<&std::collections::HashSet<String>>,
    ) -> StoreResult<Vec<CanonicalMessageDelivery>> {
        Ok(self
            .trust_read_model()?
            .messages
            .deliveries(space, message_ids))
    }
    pub(crate) fn cached_agent_team_scope(&self, team_id: &str) -> StoreResult<Option<String>> {
        Ok(self.trust_read_model()?.team_scopes.get(team_id).cloned())
    }
    pub(crate) fn cached_team_memberships(
        &self,
        space: &str,
        team_id: Option<&str>,
    ) -> StoreResult<Vec<TeamMembership>> {
        let model = self.trust_read_model()?;
        for value in model.invalid_memberships.get(space).into_iter().flatten() {
            if team_id.is_none_or(|team| value["team_id"].as_str() == Some(team)) {
                let _: TeamMembership = serde_json::from_value(value.clone())?;
            }
        }
        let rows = match team_id {
            Some(team) => model
                .memberships_by_team
                .get(&(space.to_owned(), team.to_owned())),
            None => model.memberships.get(space),
        };
        Ok(rows
            .map(|rows| rows.values().cloned().collect())
            .unwrap_or_default())
    }
    pub(crate) fn cached_work_runtime_binding_sources(
        &self,
        space: &str,
        binding_id: &str,
    ) -> StoreResult<Vec<serde_json::Value>> {
        Ok(self
            .trust_read_model()?
            .runtime_bindings
            .get(&(space.to_owned(), binding_id.to_owned()))
            .cloned()
            .unwrap_or_default())
    }
    pub(crate) fn cached_host_attention_outbox(&self) -> StoreResult<Vec<HostAttention>> {
        Ok(self.trust_read_model()?.outbox_sources())
    }
    pub(crate) fn canonical_store_sequence_for_space(&self, space: &str) -> StoreResult<u64> {
        Ok(self
            .trust_read_model()?
            .sequences
            .get(space)
            .copied()
            .unwrap_or(0))
    }
    pub(crate) fn cached_trust_work_latest_unlocked(&self) -> StoreResult<Vec<Work>> {
        self.trust_read_model()?.latest_works()
    }
    pub(crate) fn cached_canonical_work_bindings(
        &self,
        space: &str,
    ) -> StoreResult<Vec<WorkExecutionBinding>> {
        let model = self.trust_read_model()?;
        if let Some(value) = model.invalid_bindings.get(space) {
            let _: WorkExecutionBinding = serde_json::from_value(value.clone())?;
        }
        Ok(model
            .bindings
            .get(space)
            .map(|rows| rows.values().cloned().collect())
            .unwrap_or_default())
    }
    pub(crate) fn cached_canonical_work_deliveries(
        &self,
        space: &str,
    ) -> StoreResult<BTreeMap<String, CanonicalWorkDelivery>> {
        let model = self.trust_read_model()?;
        if let Some(error) = model.delivery_errors.get(space) {
            return Err(StoreError::Conflict(error.clone()));
        }
        Ok(model.deliveries.get(space).cloned().unwrap_or_default())
    }
}
