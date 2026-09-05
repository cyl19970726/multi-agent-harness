use super::*;
use firm_application::{
    CurrentWorkDeliveryAuthority, CurrentWorkDeliveryIntegrityAnnotation, CurrentWorkDeliveryView,
};
use firm_core::agentfirm_api::{TeamMembershipStatus, WorkExecutionBindingStatus};

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
            .team_run_rows(team_run_id)?
            .into_iter()
            .rev()
            .find(|run| run.id == team_run_id)
            .ok_or_else(|| StoreError::Conflict(format!("TeamRun not found: {team_run_id}")))?;
        let execution_space_id = self.current_team_run_execution_space(&run)?;
        self.current_work_deliveries_scoped(&execution_space_id, None, Some(team_run_id))
    }

    /// Project current Work deliveries only from canonical trust authority.
    /// Broken canonical joins fail closed.
    pub fn current_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        self.current_work_deliveries_scoped(execution_space_id, None, None)
    }

    /// Project the exact requested Team scope before strict canonical joins.
    /// Foreign rows are not inputs; every row inside the scope still fails
    /// closed on a broken join.
    pub fn current_work_deliveries_for_teams(
        &self,
        execution_space_id: &str,
        team_ids: &std::collections::BTreeSet<String>,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        self.current_work_deliveries_scoped(execution_space_id, Some(team_ids), None)
    }

    fn current_work_deliveries_scoped(
        &self,
        execution_space_id: &str,
        team_ids: Option<&std::collections::BTreeSet<String>>,
        team_run_id: Option<&str>,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        // Canonical Work, binding, delivery, Session, membership, and MemberRun
        // facts can span several append-only files. Use the canonical trust
        // sequence as a seqlock: a changed sequence discards the mixed read,
        // while a stable sequence returns either the complete projection or a
        // stable integrity error. A short bounded backoff lets reads converge
        // between active provider writes without taking the Store writer lock,
        // so diagnostics remain available during write contention.
        let mut backoff_ms = 1_u64;
        for attempt in 0..16 {
            let before = self.current_work_delivery_store_sequence(execution_space_id)?;
            let projected =
                self.current_work_deliveries_once(execution_space_id, team_ids, team_run_id);
            let after = self.current_work_delivery_store_sequence(execution_space_id)?;
            if before == after {
                return projected;
            }
            if attempt < 15 {
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                backoff_ms = backoff_ms.saturating_mul(2).min(25);
            }
        }
        Err(StoreError::CurrentWorkDeliverySnapshotUnstable)
    }

    fn current_work_deliveries_once(
        &self,
        execution_space_id: &str,
        team_ids: Option<&std::collections::BTreeSet<String>>,
        team_run_id: Option<&str>,
    ) -> StoreResult<Vec<CurrentWorkDeliveryView>> {
        let team_runs = match team_run_id {
            Some(id) => self.team_run_rows(id)?,
            None => self.team_runs()?,
        }
        .into_iter()
        .map(|run| (run.id.clone(), run))
        .collect::<std::collections::BTreeMap<_, _>>();
        let scoped_team_run_ids = team_runs
            .values()
            .filter(|run| {
                team_run_id.is_none_or(|id| run.id == id)
                    && team_ids.is_none_or(|ids| ids.contains(&run.agent_team_id))
            })
            .filter_map(|run| {
                self.current_team_run_execution_space(run)
                    .ok()
                    .filter(|scope| scope == execution_space_id)
                    .map(|_| run.id.clone())
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut work_revisions = std::collections::BTreeMap::<(String, u64), Work>::new();
        for work in self
            .work_operations_for_team_run_unlocked(team_run_id)?
            .into_iter()
            .map(|operation| operation.work)
            .filter(|work| scoped_team_run_ids.contains(&work.team_run_id))
        {
            insert_work_revision(&mut work_revisions, work)?;
        }
        for operation in self.canonical_operations_for_space(execution_space_id)? {
            let projection_is_scoped = operation.resulting_projection["team_run_id"]
                .as_str()
                .is_some_and(|id| scoped_team_run_ids.contains(id));
            if projection_is_scoped {
                if let Ok(work) = serde_json::from_value::<Work>(operation.resulting_projection) {
                    insert_work_revision(&mut work_revisions, work)?;
                }
            }
            for record in operation.immutable_side_records {
                let record_is_scoped = record["team_run_id"]
                    .as_str()
                    .is_some_and(|id| scoped_team_run_ids.contains(id));
                if record_is_scoped {
                    if let Ok(work) = serde_json::from_value::<Work>(record) {
                        insert_work_revision(&mut work_revisions, work)?;
                    }
                }
            }
        }
        let work_ids = work_revisions
            .values()
            .map(|work| work.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let bindings = match team_run_id {
            Some(_) => {
                self.fabric_work_execution_bindings_for_works(execution_space_id, &work_ids)?
            }
            None => self.fabric_work_execution_bindings(execution_space_id)?,
        }
        .into_iter()
        .filter(|binding| team_ids.is_none_or(|ids| ids.contains(&binding.team_id)))
        .map(|binding| (binding.id.clone(), binding))
        .collect::<std::collections::BTreeMap<_, _>>();
        let member_ids = bindings
            .values()
            .map(|binding| binding.agent_member_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let sessions = match team_run_id {
            Some(_) => self.fabric_agent_sessions_for_members(execution_space_id, &member_ids)?,
            None => self.fabric_agent_sessions(execution_space_id)?,
        }
        .into_iter()
        .map(|session| (session.id.clone(), session))
        .collect::<std::collections::BTreeMap<_, _>>();
        let team_id = team_runs
            .values()
            .next()
            .map(|run| run.agent_team_id.as_str());
        let memberships = match team_id {
            Some(id) if team_run_id.is_some() => {
                self.fabric_team_memberships_for_team(execution_space_id, id)?
            }
            _ => self.fabric_team_memberships(execution_space_id)?,
        }
        .into_iter()
        .map(|membership| (membership.id.clone(), membership))
        .collect::<std::collections::BTreeMap<_, _>>();
        let member_runs = match team_run_id {
            Some(id) => self.trust_member_runs_for_team_run(execution_space_id, id)?,
            None => self.trust_member_runs(execution_space_id)?,
        };
        let mut views = Vec::new();

        let deliveries = match team_run_id {
            Some(_) => self.fabric_work_deliveries_for_works(execution_space_id, &work_ids)?,
            None => self.fabric_work_deliveries(execution_space_id)?,
        };
        for delivery in deliveries.into_iter().filter(|delivery| {
            (team_ids.is_none() && team_run_id.is_none())
                || bindings.contains_key(&delivery.work_execution_binding_id)
        }) {
            let work = work_revisions
                .get(&(delivery.work_id.clone(), delivery.work_revision))
                .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_WORK_REVISION_MISSING: delivery {} references missing Work {} revision {}",
                    delivery.id, delivery.work_id, delivery.work_revision
                ))
            })?;
            let current_work = work_revisions
                .values()
                .filter(|candidate| candidate.id == delivery.work_id)
                .max_by_key(|candidate| candidate.version)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "CURRENT_WORK_DELIVERY_WORK_MISSING: delivery {} references missing current Work {}",
                        delivery.id, delivery.work_id
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
            let runtime_binding =
                self.work_execution_runtime_binding(execution_space_id, &binding.id)?;
            let team_run = team_runs.get(&work.team_run_id).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_TEAM_RUN_MISSING: delivery {} references missing TeamRun {}",
                    delivery.id, work.team_run_id
                ))
            })?;
            let mut integrity_annotations = Vec::new();
            let responsibility_changed = self.work_responsibility_changed_after_revision_unlocked(
                &delivery.work_id,
                binding.work_revision,
            )?;
            let binding_conflicts = binding.work_id != delivery.work_id
                || binding.work_revision != delivery.work_revision
                || binding.delivery_id != delivery.id
                || binding.agent_member_id != delivery.recipient_agent_member_id
                || binding.agent_session_id != delivery.recipient_session_id
                || binding.agent_session_generation != delivery.recipient_session_generation
                || runtime_binding.target_session_id.as_deref()
                    != Some(binding.agent_session_id.as_str())
                || runtime_binding.target_runtime_generation
                    != Some(binding.agent_session_generation)
                || runtime_binding.target_member_run_id.is_none()
                || runtime_binding.target_member_run_generation.is_none()
                || work.accountable_team_id.as_deref() != Some(binding.team_id.as_str())
                || team_run.agent_team_id != binding.team_id;
            let binding_is_active = binding.status == WorkExecutionBindingStatus::Active;
            let current_responsibility_conflicts = binding_is_active
                && (responsibility_changed
                    || current_work.active_member_run_id.is_some()
                    || current_work.team_run_id != work.team_run_id
                    || current_work.accountable_team_id.as_deref()
                        != Some(binding.team_id.as_str())
                    || current_work.assignee_membership_id.as_deref()
                        != Some(binding.team_membership_id.as_str())
                    || current_work.owner_member_id.as_deref()
                        != Some(binding.agent_member_id.as_str()));
            let session_conflicts = binding_is_active
                && (session.execution_space_id != execution_space_id
                    || session.agent_member_id != delivery.recipient_agent_member_id
                    || session.runtime_generation != delivery.recipient_session_generation
                    || session.node_id != delivery.target_node_id);
            let membership_conflicts = binding_is_active
                && (membership.team_id != binding.team_id
                    || membership.agent_member_id != delivery.recipient_agent_member_id
                    || membership.state != TeamMembershipStatus::Active);
            if binding_conflicts
                || current_responsibility_conflicts
                || session_conflicts
                || membership_conflicts
            {
                return Err(StoreError::Conflict(format!(
                    "CURRENT_WORK_DELIVERY_CANONICAL_JOIN_CONFLICT: delivery {} does not match its exact Work, binding, session, membership, or TeamRun (binding={binding_conflicts}, responsibility={current_responsibility_conflicts}[changed={responsibility_changed}, status={:?}, legacy={}, run={}, team={}, assignment={}, owner={}], session={session_conflicts}, membership={membership_conflicts})",
                    delivery.id,
                    binding.status,
                    current_work.active_member_run_id.is_some(),
                    current_work.team_run_id != work.team_run_id,
                    current_work.accountable_team_id.as_deref() != Some(binding.team_id.as_str()),
                    current_work.assignee_membership_id.as_deref() != Some(binding.team_membership_id.as_str()),
                    current_work.owner_member_id.as_deref() != Some(binding.agent_member_id.as_str()),
                )));
            }

            let matching_member_runs = member_runs
                .iter()
                .filter(|member_run| {
                    binding_is_active
                        && member_run.team_run_id == work.team_run_id
                        && member_run.agent_member_id == delivery.recipient_agent_member_id
                        && member_run.has_live_runtime_authority()
                        && match (
                            member_run.native_session.as_ref(),
                            session.native_session_ref.as_ref(),
                        ) {
                            (Some(member_native), Some(session_native)) => {
                                firm_core::agentfirm_api::native_session_identity_matches(
                                    member_native,
                                    session_native,
                                )
                            }
                            (None, None) => true,
                            _ => false,
                        }
                        && runtime_binding.target_member_run_id.as_deref()
                            == Some(member_run.id.as_str())
                        && runtime_binding.target_member_run_generation
                            == Some(member_run.runtime_generation)
                        && runtime_binding.target_session_id.as_deref() == Some(session.id.as_str())
                        && runtime_binding.target_runtime_generation
                            == Some(session.runtime_generation)
                })
                .collect::<Vec<_>>();
            let recipient_member_run_id = if let [member_run] = matching_member_runs.as_slice() {
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
