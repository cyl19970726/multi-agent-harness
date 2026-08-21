use super::*;

#[test]
fn source_work_attestation_and_placement_v1_fail_closed() {
    let test = TestStore::new("attestation");
    install_policy(&test.store);
    let auth = authority();
    let before = test.store.collaboration_operations().unwrap();

    let mut caller_authored = source_attestation();
    caller_authored.id = "caller-authored-attestation".into();
    caller_authored.work_application_service_ref = auth.source_host.clone();
    assert!(test
        .store
        .put_source_work_attestation(
            &context(
                auth.source_host.clone(),
                "source_work.attest",
                "caller-authored-attestation",
                0,
            ),
            &caller_authored,
            &auth.source_work_application_service,
            auth.source_gateway_generation,
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let mut stale_authority = auth.clone();
    stale_authority.source_gateway_generation = 9;
    assert!(test
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "stale-attestation-propose",
                0,
            ),
            &proposal(),
            &stale_authority,
            &policy(),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let mut non_v1 = proposal();
    non_v1.target_placement.placement_generation = 2;
    assert!(test
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "non-v1-placement",
                0,
            ),
            &non_v1,
            &auth,
            &policy(),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let mut revoked_policy = policy();
    revoked_policy.revision = 2;
    revoked_policy.revoked_at = Some("2026-08-11T00:00:03Z".into());
    test.store
        .put_collaboration_inbound_policy(
            &context(
                auth.target_host.clone(),
                "delegation.policy.put",
                "policy-revoke-1",
                1,
            ),
            &revoked_policy,
            &auth.target_host,
        )
        .expect("target Host may revoke the exact inbound policy revision");
    let after_revoke = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "revoked-policy-propose",
                0,
            ),
            &proposal(),
            &auth,
            &revoked_policy,
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), after_revoke);

    assert_eq!(
        CollaborationRetentionAnchor {
            terminal_transport_at_unix_ms: Some(100),
            terminal_delegation_at_unix_ms: Some(300),
            source_import_completed_at_unix_ms: Some(200),
        }
        .safe_retention_start_unix_ms(),
        Some(300)
    );
    assert_eq!(
        CollaborationRetentionAnchor {
            terminal_transport_at_unix_ms: Some(100),
            terminal_delegation_at_unix_ms: Some(300),
            source_import_completed_at_unix_ms: None,
        }
        .retain_until_unix_ms(30 * 24 * 60 * 60 * 1_000),
        None
    );
}
