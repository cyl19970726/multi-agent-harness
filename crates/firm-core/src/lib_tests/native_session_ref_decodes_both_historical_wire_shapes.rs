use super::*;
use crate::agentfirm_api::native_session_admits_resume_seed;

/// The ledger projection and the trust journal used to be two structurally
/// different types under one name: the trust one required `availability` and
/// denied unknown fields, the ledger one defaulted `availability` to `Unknown`.
/// There is now ONE type, so it must still read both historical wire shapes.
#[test]
fn native_session_ref_decodes_both_historical_wire_shapes() {
    // Shape A: the trust-journal shape, `availability` present.
    let trust_shape = serde_json::json!({
        "provider": "kimi",
        "execution_mode": "kimi_acp",
        "native_session_id": "session-a",
        "native_locator_kind": "kimi_code_session",
        "provider_version": "1.2.3",
        "adapter_contract_version": "kimi-acp-v1",
        "availability": "available",
        "supports_resume": true,
        "last_verified_at": "2026-09-14T00:00:00Z",
        "parent_native_session_id": null
    });
    let decoded: NativeSessionRef =
        serde_json::from_value(trust_shape).expect("trust-journal shape decodes");
    assert_eq!(decoded.availability, NativeSessionAvailability::Available);
    assert_eq!(decoded.native_locator_kind, "kimi_code_session");

    // Shape B: a legacy `member_runs.jsonl` row that omits `availability`
    // entirely, plus every other optional field. It must decode as honestly
    // Unknown rather than fail or claim Available.
    let legacy_ledger_shape = serde_json::json!({
        "provider": "codex",
        "execution_mode": "codex_app_server",
        "native_session_id": "thread-b",
        "native_locator_kind": "codex_rollout",
        "adapter_contract_version": "codex-app-server-v1",
        "supports_resume": true
    });
    let decoded: NativeSessionRef =
        serde_json::from_value(legacy_ledger_shape).expect("legacy ledger shape decodes");
    assert_eq!(
        decoded.availability,
        NativeSessionAvailability::Unknown,
        "an omitted availability is unknown, never an implied Available"
    );
    assert_eq!(decoded.provider_version, None);
    assert_eq!(decoded.last_verified_at, None);
    assert_eq!(decoded.parent_native_session_id, None);

    // `deny_unknown_fields` is kept: every row in the real stores satisfies it,
    // so a foreign field is a contract break, not a tolerated extra.
    let unknown_field = serde_json::json!({
        "provider": "pi",
        "execution_mode": "pi_rpc",
        "native_session_id": "session-c",
        "native_locator_kind": "pi_session",
        "adapter_contract_version": "pi-rpc-v1",
        "supports_resume": true,
        "transcript": "never mirrored into a Harness ledger"
    });
    serde_json::from_value::<NativeSessionRef>(unknown_field)
        .expect_err("an unknown field is rejected, so no provider stream can ride along");
}

/// Availability, resumability and verification timestamps are observations.
/// They must never split one provider-native conversation into two identities,
/// and the one asymmetric comparison must stay asymmetric.
#[test]
fn native_session_identity_ignores_observations_and_admits_only_the_named_asymmetry() {
    let observed = NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "thread-a".into(),
        native_locator_kind: "codex_rollout".into(),
        provider_version: Some("0.148.0".into()),
        adapter_contract_version: "codex-app-server-v1".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("2026-09-14T00:00:00Z".into()),
        parent_native_session_id: None,
    };

    let mut only_observations_differ = observed.clone();
    only_observations_differ.availability = NativeSessionAvailability::Stale;
    only_observations_differ.supports_resume = false;
    only_observations_differ.last_verified_at = None;
    assert!(observed.same_identity_as(&only_observations_differ));

    let mut different_locator_kind = observed.clone();
    different_locator_kind.native_locator_kind = "codex_thread".into();
    assert!(
        !observed.same_identity_as(&different_locator_kind),
        "the locator kind is part of identity, so a guessed kind is a different session"
    );

    // A resume seed names the conversation without knowing which provider
    // version opened it.
    let mut resume_seed = observed.clone();
    resume_seed.provider_version = None;
    assert!(!observed.same_identity_as(&resume_seed));
    assert!(native_session_admits_resume_seed(&observed, &resume_seed));
    assert!(
        !native_session_admits_resume_seed(&resume_seed, &observed),
        "the asymmetry has one direction; the reverse is not a weaker identity check"
    );
    assert!(
        !native_session_admits_resume_seed(&resume_seed, &resume_seed),
        "two version-less pointers are exact-identity business, not seed admission"
    );
}
