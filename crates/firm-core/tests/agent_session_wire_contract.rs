//! ADR 0070 wire contract for `AgentSession`.
//!
//! Two decisions are proven here, and they fail closed in opposite directions:
//! a retired lifecycle value must stop decoding, while a renamed field must
//! keep decoding from its persisted spelling.

use firm_core::agentfirm_api::{AgentSession, RuntimeActivity};

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

/// The field was renamed, not re-specified. Every persisted row spells it
/// `current_turn_id`, and `AgentSession` is `deny_unknown_fields`, so the
/// alias is the whole reason those rows keep decoding. Nothing may emit the
/// old spelling again.
#[test]
fn legacy_current_turn_id_decodes_and_reserializes_as_current_cycle_marker() {
    let mut legacy = session_row("active");
    legacy["current_turn_id"] =
        serde_json::json!("provider-turn:agent-session:member-adr-0070:daemon-adr-0070:1:1:3");
    let decoded: AgentSession =
        serde_json::from_value(legacy).expect("a pre-cutover row must still decode");
    assert_eq!(
        decoded.current_cycle_marker.as_deref(),
        Some("provider-turn:agent-session:member-adr-0070:daemon-adr-0070:1:1:3"),
        "the legacy spelling must land on the renamed field, not be dropped"
    );

    let reserialized = serde_json::to_value(&decoded).expect("row reserializes");
    let object = reserialized.as_object().expect("row is an object");
    assert!(
        object.contains_key("current_cycle_marker"),
        "writers emit the canonical current_cycle_marker spelling"
    );
    assert!(
        !object.contains_key("current_turn_id"),
        "no writer may emit the retired current_turn_id spelling"
    );
}

/// The marker is a presence fact about one Harness-scheduled cycle. The
/// terminal-boundary predicate is the only thing that reads it, and it reads
/// it as present/absent — never as an id that could address a provider turn.
#[test]
fn an_open_cycle_marker_keeps_a_lane_off_the_terminal_cycle_boundary() {
    let mut session: AgentSession =
        serde_json::from_value(session_row("idle")).expect("quiet lane decodes");
    session.control_state.activity = RuntimeActivity::Idle;
    assert!(
        session.is_at_terminal_cycle_boundary(),
        "an idle, cycle-free lane sits at the boundary"
    );
    session.current_cycle_marker = Some("harness-cycle:session-adr-0070:4".into());
    assert!(
        !session.is_at_terminal_cycle_boundary(),
        "an open cycle marker keeps the lane off the boundary"
    );
}
