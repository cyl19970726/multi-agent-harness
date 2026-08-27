use super::*;

pub(super) fn delegation_work(run: &AgentTeamRun, id: &str) -> Work {
    let mut work = unassigned_test_work(&run.id, id);
    work.claim_mode = WorkClaimMode::HostAssign;
    work
}

pub(super) fn insert_assigned_delegation_work(
    store: &HarnessStore,
    run: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
    id: &str,
    event_id: &str,
    idempotency_key: &str,
    created_at: &str,
) -> Work {
    let created = store
        .insert_work(
            delegation_work(run, id),
            run_host_work_context(run, event_id, idempotency_key, created_at),
        )
        .expect("create unassigned delegation Work");
    store
        .assign_work_to_membership(
            &created.id,
            created.version,
            &format!(
                "membership:{}:{}",
                run.agent_team_id, member.agent_member_id
            ),
            "delegation-test-space",
            run_host_work_context(
                run,
                &format!("{event_id}:assign"),
                &format!("{idempotency_key}:assign"),
                created_at,
            ),
        )
        .expect("assign stable delegation Work responsibility")
}

pub(super) fn delegation_request(id: &str, source: &Work, target_team_id: &str) -> WorkDelegation {
    WorkDelegation {
        id: id.to_string(),
        source_work_ref: WorkRef {
            team_run_id: source.team_run_id.clone(),
            work_id: source.id.clone(),
        },
        source_work_version: source.version,
        source_owner_member_id: source
            .owner_member_id
            .clone()
            .expect("delegation source owner"),
        created_by_member_run_id: None,
        target_agent_team_id: target_team_id.to_string(),
        target_work_ref: WorkRef {
            team_run_id: String::new(),
            work_id: String::new(),
        },
        delegated_by_actor: host_work_context("unused", "unused", "unix-ms:1").performed_by_actor,
        state: WorkDelegationState::Active,
        resolution_summary: None,
        blocker_reason: None,
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}
