use super::*;

pub(super) fn wait_for_file(path: &std::path::Path, context: &str) {
    for _ in 0..500 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {context}: {}", path.display());
}

/// DEV-21 regression guard: a provider-driven settle persists the native
/// Session binding through `TeamRunLedger::save_member_run`, which must sync
/// the same binding onto the trust MemberRun (selector layer) and the current
/// canonical AgentSession (exact binding layer) of the same runtime generation.
pub(super) fn assert_trust_native_binding_synced(
    store: &HarnessStore,
    member_run_id: &str,
    expected_native_id: &str,
) {
    let space_id = store
        .trust_member_run_scope(member_run_id)
        .expect("read trust MemberRun scope")
        .expect("trust MemberRun fabric exists for the settled run");
    let trust_run = store
        .trust_member_runs(&space_id)
        .expect("read trust MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("canonical trust MemberRun");
    assert_eq!(
        trust_run
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some(expected_native_id),
        "provider settle must sync the native Session binding onto the trust MemberRun: {trust_run:?}"
    );
    let sessions = store
        .fabric_agent_sessions(&space_id)
        .expect("read canonical AgentSessions")
        .into_iter()
        .filter(|session| session.agent_member_id == trust_run.agent_member_id)
        .filter(|session| session.runtime_generation == trust_run.runtime_generation)
        .filter(|session| {
            session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sessions.len(),
        1,
        "exactly one current AgentSession for the settled generation: {sessions:?}"
    );
    assert_eq!(
        sessions[0]
            .native_session_ref
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some(expected_native_id),
        "provider settle must sync the native Session binding onto the canonical AgentSession: {:?}",
        sessions[0]
    );
}
