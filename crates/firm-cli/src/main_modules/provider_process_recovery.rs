pub(super) const MAX_AUTOMATIC_PROVIDER_TRANSPORT_ATTEMPTS: u64 = 3;

pub(super) fn provider_process_idempotency_key(
    session: &harness_core::agentfirm_api::AgentSession,
    supervisor_generation: u64,
    transport_attempt: u64,
    kind: harness_core::agentfirm_api::RuntimeCommandKind,
) -> String {
    format!(
        "provider-process:{}:{}:{}:{}:{}:{}:{kind:?}",
        session.id,
        session.runtime_generation,
        session.node_daemon_generation,
        session.control_state.driver_generation,
        supervisor_generation,
        transport_attempt,
    )
}

pub(super) fn automatic_provider_transport_retry_exhausted(transport_attempt: u64) -> bool {
    transport_attempt >= MAX_AUTOMATIC_PROVIDER_TRANSPORT_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_process_retry_identity_is_stable_and_generation_scoped() {
        let session = test_agent_session();
        let first = provider_process_idempotency_key(
            &session,
            7,
            1,
            harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
        );
        assert_eq!(
            first,
            provider_process_idempotency_key(
                &session,
                7,
                1,
                harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
            )
        );
        assert_ne!(
            first,
            provider_process_idempotency_key(
                &session,
                7,
                2,
                harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
            )
        );
        assert_ne!(
            first,
            provider_process_idempotency_key(
                &session,
                8,
                1,
                harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
            )
        );
    }

    #[test]
    fn automatic_provider_transport_retries_are_bounded() {
        assert!(!automatic_provider_transport_retry_exhausted(1));
        assert!(!automatic_provider_transport_retry_exhausted(2));
        assert!(automatic_provider_transport_retry_exhausted(3));
        assert!(automatic_provider_transport_retry_exhausted(4));
    }

    fn test_agent_session() -> harness_core::agentfirm_api::AgentSession {
        serde_json::from_value(serde_json::json!({
            "id": "agent-session:member:node:1:1",
            "agent_member_id": "member",
            "node_id": "node",
            "execution_space_id": "space",
            "node_daemon_id": "node-daemon:node",
            "node_daemon_generation": 5,
            "provider_kind": "kimi",
            "provider_profile_ref": "provider-profile:kimi",
            "runtime_generation": 1,
            "lifecycle": "active",
            "effective_permission_ceiling": "full_access",
            "permission_envelope_ref": "permission:member",
            "native_session_ref": null,
            "current_turn_id": null,
            "queued_input_count": 0,
            "control_state": {
                "runtime_residency": "attached",
                "activity": "idle",
                "execution_driver": "host_driven",
                "driver_generation": 2,
                "driver_ref": {
                    "kind": "team_supervisor",
                    "team_run_id": "team-run",
                    "team_supervisor_id": "supervisor-7",
                    "team_supervisor_generation": 7
                },
                "composition_fingerprint": "composition",
                "capability_fingerprint": "capability"
            },
            "version": 1,
            "opened_at": "t0",
            "last_active_at": "t0",
            "closed_at": null
        }))
        .expect("test AgentSession must deserialize")
    }
}
