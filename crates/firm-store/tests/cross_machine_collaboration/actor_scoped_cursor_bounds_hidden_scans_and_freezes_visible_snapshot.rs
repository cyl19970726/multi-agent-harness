use super::*;

#[test]
fn actor_scoped_cursor_bounds_hidden_scans_and_freezes_visible_snapshot() {
    let test = TestStore::new("scoped-cursor-hidden-pages");
    let mut open_policy = policy();
    open_policy.max_active_delegations = 300;
    let target_host = actor(ActorKind::AgentMember, "host-b");
    test.store
        .put_collaboration_inbound_policy(
            &context(
                target_host.clone(),
                "delegation.policy.put",
                "policy-hidden-pages",
                0,
            ),
            &open_policy,
            &target_host,
        )
        .unwrap();

    let visible_actor = actor(ActorKind::AgentMember, "visible-owner");
    for ordinal in 0..=205 {
        let mut attestation = source_attestation();
        attestation.id = format!("source-attestation-hidden-{ordinal}");
        attestation.source_owner_ref = if ordinal >= 203 {
            visible_actor.clone()
        } else {
            actor(ActorKind::AgentMember, &format!("hidden-owner-{ordinal}"))
        };
        attestation.source_work_ref.work_id = format!("work-hidden-{ordinal}");
        attestation.source_work_ref.work_event_id = format!("event-hidden-{ordinal}");
        attestation.attestation_digest = canonical_json_fingerprint(&serde_json::json!({
            "id": attestation.id,
            "company_id": attestation.company_id,
            "source_work_ref": attestation.source_work_ref,
            "source_owner_ref": attestation.source_owner_ref,
            "source_host_ref": attestation.source_host_ref,
            "work_application_service_ref": attestation.work_application_service_ref,
            "source_gateway_generation": attestation.source_gateway_generation,
            "issued_at": attestation.issued_at,
        }));
        let service = attestation.work_application_service_ref.clone();
        test.store
            .put_source_work_attestation(
                &context(
                    service.clone(),
                    "source_work.attest",
                    &format!("attest-hidden-{ordinal}"),
                    0,
                ),
                &attestation,
                &service,
                8,
            )
            .unwrap();
        let mut resolved = authority();
        resolved.source_work_owner = attestation.source_owner_ref.clone();
        let mut request = proposal();
        request.delegation_id = format!("delegation-hidden-{ordinal}");
        request.source_work_attestation_id = attestation.id;
        request.operation_id = format!("route-hidden-{ordinal}");
        test.store
            .propose_collaboration_delegation(
                &context(
                    resolved.source_host.clone(),
                    "delegation.propose",
                    &format!("propose-hidden-{ordinal}"),
                    0,
                ),
                &request,
                &resolved,
                &open_policy,
            )
            .unwrap();
    }

    let filter = CollaborationDelegationFilter::default();
    let first = test
        .store
        .list_collaboration_delegations_for_actor("company-1", &visible_actor, &filter, None, 2)
        .unwrap();
    assert!(first.items.is_empty(), "hidden-only raw page stays empty");
    let frozen_sequence = first.as_of_store_sequence;
    let mut cursor = first.next_cursor;

    let mut late_attestation = source_attestation();
    late_attestation.id = "source-attestation-late-visible".into();
    late_attestation.source_owner_ref = visible_actor.clone();
    late_attestation.source_work_ref.work_id = "work-late-visible".into();
    late_attestation.source_work_ref.work_event_id = "event-late-visible".into();
    late_attestation.attestation_digest = canonical_json_fingerprint(&serde_json::json!({
        "id": late_attestation.id,
        "company_id": late_attestation.company_id,
        "source_work_ref": late_attestation.source_work_ref,
        "source_owner_ref": late_attestation.source_owner_ref,
        "source_host_ref": late_attestation.source_host_ref,
        "work_application_service_ref": late_attestation.work_application_service_ref,
        "source_gateway_generation": late_attestation.source_gateway_generation,
        "issued_at": late_attestation.issued_at,
    }));
    let late_service = late_attestation.work_application_service_ref.clone();
    test.store
        .put_source_work_attestation(
            &context(
                late_service.clone(),
                "source_work.attest",
                "attest-late-visible",
                0,
            ),
            &late_attestation,
            &late_service,
            8,
        )
        .unwrap();
    let mut late_authority = authority();
    late_authority.source_work_owner = visible_actor.clone();
    let mut late_request = proposal();
    late_request.delegation_id = "delegation-late-visible".into();
    late_request.source_work_attestation_id = late_attestation.id;
    late_request.operation_id = "route-late-visible".into();
    test.store
        .propose_collaboration_delegation(
            &context(
                late_authority.source_host.clone(),
                "delegation.propose",
                "propose-late-visible",
                0,
            ),
            &late_request,
            &late_authority,
            &open_policy,
        )
        .unwrap();

    let mut visible = Vec::new();
    let mut empty_pages = 1usize;
    while let Some(current) = cursor {
        let page = test
            .store
            .list_collaboration_delegations_for_actor(
                "company-1",
                &visible_actor,
                &filter,
                Some(current),
                2,
            )
            .unwrap();
        assert_eq!(page.as_of_store_sequence, frozen_sequence);
        if page.items.is_empty() {
            empty_pages += 1;
        }
        visible.extend(page.items);
        cursor = page.next_cursor;
    }
    assert!(
        empty_pages > 1,
        "opaque cursor advances across hidden-only pages"
    );
    assert_eq!(visible.len(), 3);
    assert_eq!(
        visible
            .iter()
            .map(|row| row.id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );

    // The visible mutation after page one is excluded from that frozen
    // traversal but appears in a fresh snapshot.
    let fresh = test
        .store
        .list_collaboration_delegations_for_actor("company-1", &visible_actor, &filter, None, 500)
        .unwrap();
    assert_eq!(fresh.items.len(), 4);
    assert!(fresh.as_of_store_sequence > frozen_sequence);
}
