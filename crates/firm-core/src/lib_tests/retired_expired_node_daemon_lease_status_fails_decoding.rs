use super::*;

/// No production writer ever produced `"expired"`: the Store writes only
/// `Active` (acquire), `Draining` (drain) and `Released` (release), and a
/// read-only scan of 862,784 persisted `node_daemon_lease` rows across 29
/// real stores found zero rows carrying it. The variant existed only as three
/// dead read branches, so it is deleted rather than frozen — following the
/// ADR 0067/0070 precedent that an unwritten wire value must fail decoding
/// instead of silently becoming a live state.
#[test]
fn retired_expired_node_daemon_lease_status_fails_decoding() {
    for status in ["active", "draining", "released"] {
        let decoded: NodeDaemonLeaseStatus =
            serde_json::from_value(serde_json::Value::String(status.to_string()))
                .unwrap_or_else(|error| panic!("{status} must still decode: {error}"));
        assert_eq!(
            serde_json::to_value(decoded).expect("status re-encodes"),
            serde_json::Value::String(status.to_string())
        );
    }

    let retired = serde_json::from_value::<NodeDaemonLeaseStatus>(serde_json::Value::String(
        "expired".to_string(),
    ));
    assert!(
        retired.is_err(),
        "the retired `expired` lease status must fail decoding, not resolve to a live status"
    );

    // A whole persisted row carrying it is refused the same way, so a hand-edited
    // or pre-cutover ledger row cannot re-enter the model through the row decoder.
    let row = serde_json::json!({
        "node_id": "0f95cac7-5ff8-4c76-8f36-9c8f208815d3",
        "daemon_id": "daemon-a",
        "generation": 3,
        "instance_id": "pid:4242:start:1000",
        "status": "expired",
        "acquired_unix_ms": 1000,
        "renewed_unix_ms": 1200,
        "expires_unix_ms": 6200
    });
    assert!(
        serde_json::from_value::<NodeDaemonLease>(row).is_err(),
        "a NodeDaemonLease row carrying the retired status must fail decoding"
    );
}
