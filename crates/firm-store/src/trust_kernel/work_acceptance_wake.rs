use super::*;
use firm_core::agentfirm_api::{
    AgentMemberOrganizationStatus, MemberCoordinationStatus, MemberExecutionDriver,
    RuntimeCommandKind, RuntimeDriverRef, TeamMembershipStatus,
};
use firm_core::work_acceptance::{select_acceptance_wake, AcceptanceWake};
use firm_core::{TeamRunStatus, WorkCondition, WorkEvent, WorkEventKind, WorkPhase};

impl HarnessStore {
    /// Read-only candidate selection. Preparation rechecks this under the
    /// canonical writer lock; this observation never grants provider authority.
    pub fn pending_work_acceptance_wake(
        &self,
        space_id: &str,
        member_run_id: &str,
    ) -> StoreResult<Option<AcceptanceWake>> {
        let Some(member) = self
            .trust_member_runs(space_id)?
            .into_iter()
            .find(|member| {
                member.id == member_run_id
                    && member.coordination_status == MemberCoordinationStatus::Active
            })
        else {
            return Ok(None);
        };
        if self
            .latest_team_member_close_request(member_run_id)?
            .is_some_and(|close| close.status == firm_core::TeamMemberCloseStatus::Pending)
        {
            return Ok(None);
        }
        let runs = self.latest_team_runs()?;
        let Some(run) = runs
            .iter()
            .find(|run| run.id == member.team_run_id && run.status == TeamRunStatus::Running)
        else {
            return Ok(None);
        };
        if !self.trust_agent_members(space_id)?.iter().any(|agent| {
            agent.id == member.agent_member_id
                && agent.organization_status == AgentMemberOrganizationStatus::Active
        }) {
            return Ok(None);
        }
        let memberships: Vec<_> = self
            .fabric_team_memberships_for_team(space_id, &run.agent_team_id)?
            .into_iter()
            .filter(|membership| {
                membership.agent_member_id == member.agent_member_id
                    && membership.state == TeamMembershipStatus::Active
            })
            .collect();
        let [membership] = memberships.as_slice() else {
            return Ok(None);
        };
        let sessions = self.fabric_agent_sessions(space_id)?;
        // A provider-native goal must never gain a second ordinary driver.
        if !sessions.iter().any(|session| session.agent_member_id == member.agent_member_id
            && session.control_state.execution_driver == MemberExecutionDriver::HostDriven
            && matches!(&session.control_state.driver_ref, RuntimeDriverRef::TeamSupervisor { team_run_id, .. }
                if team_run_id == &run.id)) { return Ok(None); }
        // Do not scan acceptance or runtime history for members without any
        // currently blocked responsibility (the normal idle case).
        let works: Vec<_> = self
            .latest_works()?
            .into_iter()
            .filter(|work| work.accountable_team_id.as_deref() == Some(run.agent_team_id.as_str()))
            .collect();
        if !works.iter().any(|work| {
            work.condition == WorkCondition::Blocked
                && work.phase != WorkPhase::Closed
                && work.assignee_membership_id.as_deref() == Some(membership.id.as_str())
        }) {
            return Ok(None);
        }
        let work_ids = works.iter().map(|work| work.id.clone()).collect();
        let mut events: Vec<_> = self
            .work_operations_for_ids_unlocked(&work_ids)?
            .into_iter()
            .map(|operation| operation.event)
            .collect();
        events.extend(self.trust_work_events_for_ids_unlocked(&work_ids)?);
        // Current acceptance is the canonical Work transition itself, not an
        // immutable WorkEvent side record. Normalize only for this pure read;
        // preserve its native event id and never write a compatibility event.
        for envelope in self.trust_operation_envelopes_unlocked()? {
            let event = envelope.operation.event;
            if envelope.execution_space_id != space_id
                || event.aggregate_kind != "work"
                || event.transition != "accepted"
                || !work_ids.contains(&event.aggregate_id)
            {
                continue;
            }
            let Some(work) = works.iter().find(|work| work.id == event.aggregate_id) else {
                continue;
            };
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
            events.push(WorkEvent {
                id: event.id,
                team_run_id: work.team_run_id.clone(),
                work_id: event.aggregate_id,
                sequence: event.sequence,
                kind: WorkEventKind::Accepted,
                expected_version: event.expected_version,
                resulting_version: event.resulting_version,
                performed_by_actor: actor(event.performed_by_actor),
                authority_actor: event.authority_actor.map(actor),
                causation_ref: None,
                idempotency_key: event.idempotency_key,
                payload: event.payload,
                created_at: event.created_at,
            });
        }
        let Some(candidate) = select_acceptance_wake(
            &works,
            &events,
            &run.agent_team_id,
            &membership.id,
            &member.agent_member_id,
        ) else {
            return Ok(None);
        };
        let source = candidate.source_record_id();
        // Preparing once is the automatic-attempt boundary, including a later
        // proven NotApplied failure. Explicit Host recovery/message remains.
        let consumed = self.runtime_commands(space_id)?.iter().any(|command|
            command.command == RuntimeCommandKind::StartCycle
                && command.source_record_id.as_deref() == Some(source.as_str())
                && matches!(&command.binding.target_driver, RuntimeDriverRef::TeamSupervisor { team_run_id, .. }
                    if runs.iter().any(|r| &r.id == team_run_id && r.agent_team_id == run.agent_team_id))
                && sessions.iter().any(|session| Some(&session.id) == command.target_session_id.as_ref()
                    && session.agent_member_id == member.agent_member_id));
        Ok((!consumed).then_some(candidate))
    }
}
