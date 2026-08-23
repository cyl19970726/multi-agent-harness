use super::*;
use firm_application::{
    CurrentWorkDeliveryAuthority, CurrentWorkDeliveryIntegrityAnnotation, CurrentWorkDeliveryView,
};

fn insert_work_revision(
    revisions: &mut std::collections::BTreeMap<(String, u64), Work>,
    work: Work,
) -> StoreResult<()> {
    let key = (work.id.clone(), work.version);
    if let Some(existing) = revisions.get(&key) {
        if existing != &work {
            return Err(StoreError::Conflict(format!(
                "CURRENT_WORK_DELIVERY_WORK_REVISION_CONFLICT: Work {} revision {} has conflicting canonical projections",
                work.id, work.version
            )));
        }
    } else {
        revisions.insert(key, work);
    }
    Ok(())
}

impl HarnessStore {
    fn current_work_delivery_store_sequence(&self, execution_space_id: &str) -> StoreResult<u64> {
        Ok(self
            .canonical_operations_for_space(execution_space_id)?
            .into_iter()
            .map(|operation| operation.event.store_sequence)
            .max()
            .unwrap_or(0))
    }

    pub fn current_work_deliveries_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        let run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|run| run.id == team_run_id)
            .ok_or_else(|| StoreError::Conflict(format!("TeamRun not found: {team_run_id}")))?;
        let execution_space_id = self.current_team_run_execution_space(&run)?;
        Ok(self
            .current_work_deliveries(&execution_space_id)?
            .into_iter()
            .filter(|delivery| delivery.team_run_id == team_run_id)
            .collect())
    }

    /// Project current Work deliveries only from canonical trust authority.
    /// Broken canonical joins fail closed.
    pub fn current_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        for _ in 0..4 {
            let before = self.current_work_delivery_store_sequence(execution_space_id)?;
            let projected = self.current_work_deliveries_once(execution_space_id);
            let after = self.current_work_delivery_store_sequence(execution_space_id)?;
            if before == after {
                return projected;
            }
        }
        Err(StoreError::Conflict(
            "CURRENT_WORK_DELIVERY_SNAPSHOT_UNSTABLE: canonical trust authority changed during four bounded projection attempts".into(),
        ))
    }

    fn current_work_deliveries_once(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        let team_runs = self
            .team_runs()?
            .into_iter()
            .map(|run| (run.id.clone(), run))
            .collect::<std::collections::BTreeMap<_, _>>();
        let scoped_team_run_ids = team_runs
            .values()
            .filter_map(|run| {
                self.current_team_run_execution_space(run)
                    .ok()
                    .filter(|scope| scope == execution_space_id)
                    .map(|_| run.id.clone())
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut work_revisions = std::collections::BTreeMap::<(String, u64), Work>::new();
        for work in self
            .work_operations_unlocked()?
            .into_iter()
            .map(|operation| operation.work)
            .filter(|work| scoped_team_run_ids.contains(&work.team_run_id))
        {
            insert_work_revision(&mut work_revisions, work)?;
        }
        for operation in self.canonical_operations_for_space(execution_space_id)? {
            if let Ok(work) = serde_json::from_value::<Work>(operation.resulting_projection) {
                insert_work_revision(&mut work_revisions, work)?;
            }
            for record in operation.immutable_side_records {
                if let Ok(work) = serde_json::from_value::<Work>(record) {
                    insert_work_revision(&mut work_revisions, work)?;
                }
            }
        }
        let bindings = self
            .fabric_work_execution_bindings(execution_space_id)?
            .into_iter()
            .map(|binding| (binding.id.clone(), binding))
            .collect::<std::collections::BTreeMap<_, _>>();
        let sessions = self
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .map(|session| (session.id.clone(), session))
            .collect::<std::collections::BTreeMap<_, _>>();
        let memberships = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .map(|membership| (membership.id.clone(), membership))
            .collect::<std::collections::BTreeMap<_, _>>();
        let member_runs = self.trust_member_runs(execution_space_id)?;
        let mut views = Vec::new();

        for delivery in self.fabric_work_deliveries(execution_space_id)? {
            let work = work_revisions
                .get(&(delivery.work_id.clone(), delivery.work_revision))
                .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_WORK_REVISION_MISSING: delivery {} references missing Work {} revision {}",
                    delivery.id, delivery.work_id, delivery.work_revision
                ))
            })?;
            let binding = bindings
                .get(&delivery.work_execution_binding_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "CURRENT_WORK_DELIVERY_BINDING_MISSING: delivery {} references missing WorkExecutionBinding {}",
                        delivery.id, delivery.work_execution_binding_id
                    ))
                })?;
            let session = sessions.get(&delivery.recipient_session_id).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_SESSION_MISSING: delivery {} references missing AgentSession {}",
                    delivery.id, delivery.recipient_session_id
                ))
            })?;
            let membership = memberships.get(&binding.team_membership_id).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_MEMBERSHIP_MISSING: delivery {} binding {} references missing TeamMembership {}",
                    delivery.id, binding.id, binding.team_membership_id
                ))
            })?;
            let team_run = team_runs.get(&work.team_run_id).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_TEAM_RUN_MISSING: delivery {} references missing TeamRun {}",
                    delivery.id, work.team_run_id
                ))
            })?;
            let mut integrity_annotations = Vec::new();
            let binding_conflicts = binding.work_id != delivery.work_id
                || binding.work_revision != delivery.work_revision
                || binding.delivery_id != delivery.id
                || binding.agent_member_id != delivery.recipient_agent_member_id
                || binding.agent_session_id != delivery.recipient_session_id
                || binding.agent_session_generation != delivery.recipient_session_generation
                || work.accountable_team_id.as_deref() != Some(binding.team_id.as_str())
                || team_run.agent_team_id != binding.team_id;
            let session_conflicts = session.execution_space_id != execution_space_id
                || session.agent_member_id != delivery.recipient_agent_member_id
                || session.runtime_generation != delivery.recipient_session_generation
                || session.node_id != delivery.target_node_id;
            let membership_conflicts = membership.team_id != binding.team_id
                || membership.agent_member_id != delivery.recipient_agent_member_id;
            if binding_conflicts || session_conflicts || membership_conflicts {
                return Err(StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_CANONICAL_JOIN_CONFLICT: delivery {} does not match its exact Work, binding, session, membership, or TeamRun",
                    delivery.id
                )));
            }

            let matching_member_runs = member_runs
                .iter()
                .filter(|member_run| {
                    member_run.team_run_id == work.team_run_id
                        && member_run.agent_member_id == delivery.recipient_agent_member_id
                })
                .collect::<Vec<_>>();
            let exact_active_member_run = work.active_member_run_id.as_deref().and_then(|id| {
                matching_member_runs
                    .iter()
                    .find(|member_run| member_run.id == id)
            });
            let recipient_member_run_id = if let Some(member_run) = exact_active_member_run {
                Some(member_run.id.clone())
            } else if let [member_run] = matching_member_runs.as_slice() {
                Some(member_run.id.clone())
            } else {
                integrity_annotations
                    .push(CurrentWorkDeliveryIntegrityAnnotation::RecipientMemberRunNotProvable);
                None
            };
            views.push(CurrentWorkDeliveryView {
                authority: CurrentWorkDeliveryAuthority::CanonicalTrust,
                read_only: true,
                execution_space_id: Some(execution_space_id.to_string()),
                team_run_id: work.team_run_id.clone(),
                work_id: delivery.work_id,
                work_revision: delivery.work_revision,
                work_execution_binding_id: Some(delivery.work_execution_binding_id),
                delivery_id: delivery.id,
                recipient_agent_member_id: Some(delivery.recipient_agent_member_id),
                recipient_member_run_id,
                recipient_agent_session_id: Some(delivery.recipient_session_id),
                recipient_agent_session_generation: Some(delivery.recipient_session_generation),
                target_node_id: Some(delivery.target_node_id),
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id,
                claimed_node_daemon_generation: delivery.claimed_node_daemon_generation,
                provider_receipt_id: delivery.provider_receipt_id,
                failure_code: delivery.failure_code,
                version: delivery.version,
                created_at: delivery.created_at,
                updated_at: delivery.updated_at,
                integrity_annotations,
            });
        }
        views.sort_by(|left, right| left.delivery_id.cmp(&right.delivery_id));
        Ok(views)
    }
}
