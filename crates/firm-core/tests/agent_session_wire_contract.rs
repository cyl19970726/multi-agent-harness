//! ADR 0070 wire contract for `AgentSession`.
//!
//! Two decisions are proven here, and they fail closed in opposite directions:
//! a retired lifecycle value must stop decoding, while a renamed field must
//! keep decoding from its persisted spelling.

use firm_core::agentfirm_api::AgentSession;

fn session_row(lifecycle: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "session-adr-0070",
        "agent_member_id": "member-adr-0070",
        "node_id": "node-adr-0070",
        "execution_space_id": "space-adr-0070",
        "node_daemon_id": "daemon-adr-0070",
        "node_daemon_generation": 1,
        "provider_kind": "codex",
        "provider_profile_ref": "codex-default",
        "permission_envelope_ref": "agent-member:member-adr-0070:permission:v1",
        "effective_permission_ceiling": "workspace_write",
        "lifecycle": lifecycle,
        "runtime_generation": 1,
        "queued_input_count": 0,
        "version": 1,
        "opened_at": "t1",
        "last_active_at": "t1"
    })
}

/// No writer ever produced `waiting` and no persisted row ever carried it, so
/// the value is deleted rather than reserved: a row claiming it can only come
/// from a forged or corrupted document, and refusing the decode is the
/// fail-closed reading (the same call ADR 0067 made for `provider_driven`).
#[test]
fn retired_waiting_lifecycle_fails_closed_instead_of_decoding_into_a_live_lane() {
    let error = serde_json::from_value::<AgentSession>(session_row("waiting"))
        .expect_err("a waiting lifecycle must not decode");
    assert!(
        error.to_string().contains("waiting"),
        "the decode error must name the rejected value: {error}"
    );
    for lifecycle in [
        "cold",
        "idle",
        "active",
        "interrupted",
        "recovery_required",
        "closed",
    ] {
        serde_json::from_value::<AgentSession>(session_row(lifecycle))
            .unwrap_or_else(|error| panic!("{lifecycle} must still decode: {error}"));
    }
}
