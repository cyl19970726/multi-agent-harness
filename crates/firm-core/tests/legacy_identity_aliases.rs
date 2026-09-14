//! ADR 0069 read tolerance.
//!
//! The `AgentIdentity` projection is retired, but the legacy `agent_identity`
//! spellings it left in already-persisted rows must keep decoding. No current
//! writer emits them: every one of these fields serializes back as
//! `agent_member*`, which is what this test also proves.

use firm_core::agentfirm_api::{
    AgentSession, MessageRecipientKind, MessageRecipientRef, TeamMembership, WorkExecutionBinding,
};

fn assert_round_trips_to_member_spelling<T>(legacy: serde_json::Value, field: &str)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let decoded: T = serde_json::from_value(legacy).expect("legacy agent_identity row decodes");
    let reserialized = serde_json::to_value(&decoded).expect("row reserializes");
    let object = reserialized.as_object().expect("row is an object");
    assert!(
        object.contains_key(field),
        "reserialized row must carry the canonical {field}"
    );
    assert!(
        !object.contains_key("agent_identity_id"),
        "no writer may emit the retired agent_identity_id spelling"
    );
}

#[test]
fn retired_agent_identity_spellings_still_decode_and_never_reserialize() {
    let session = serde_json::json!({
        "id": "session-legacy",
        "agent_identity_id": "member-legacy",
        "node_id": "node-legacy",
        "execution_space_id": "space-legacy",
        "node_daemon_id": "daemon-legacy",
        "node_daemon_generation": 1,
        "provider_kind": "codex",
        "provider_profile_ref": "codex-default",
        "permission_envelope_ref": "agent-identity:member-legacy:permission:v1",
        "effective_permission_ceiling": "workspace_write",
        "lifecycle": "idle",
        "runtime_generation": 1,
        "queued_input_count": 0,
        "version": 1,
        "opened_at": "t1",
        "last_active_at": "t1"
    });
    assert_round_trips_to_member_spelling::<AgentSession>(session, "agent_member_id");

    let membership = serde_json::json!({
        "id": "membership-legacy",
        "team_id": "team-legacy",
        "agent_identity_id": "member-legacy",
        "node_id": "node-legacy",
        "role": "member",
        "state": "active",
        "membership_generation": 1,
        "created_by": {"kind": "human", "id": "host"},
        "revision": 1,
        "joined_at": "t1"
    });
    assert_round_trips_to_member_spelling::<TeamMembership>(membership, "agent_member_id");

    let binding = serde_json::json!({
        "id": "binding-legacy",
        "work_id": "work-legacy",
        "work_revision": 1,
        "team_id": "team-legacy",
        "team_membership_id": "membership-legacy",
        "agent_identity_id": "member-legacy",
        "agent_session_id": "session-legacy",
        "agent_session_generation": 1,
        "delivery_id": "work-delivery:work-legacy:1",
        "binding_generation": 1,
        "status": "active",
        "version": 1,
        "created_by": {"kind": "service", "id": "daemon-legacy"},
        "bound_at": "t1"
    });
    assert_round_trips_to_member_spelling::<WorkExecutionBinding>(binding, "agent_member_id");

    let recipient: MessageRecipientRef = serde_json::from_value(
        serde_json::json!({"kind": "agent_identity", "id": "member-legacy"}),
    )
    .expect("legacy agent_identity recipient kind decodes");
    assert_eq!(recipient.kind, MessageRecipientKind::AgentMember);
    assert_eq!(
        serde_json::to_value(&recipient).unwrap()["kind"],
        serde_json::json!("agent_member"),
        "recipients reserialize under the canonical AgentMember spelling"
    );
}
