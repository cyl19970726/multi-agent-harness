use super::*;

#[test]
fn latest_cache_tracks_atomic_commits_and_keeps_scope_inventory() {
    let h = TestStore::new("latest-cache-commits");
    let host = human("host");
    assert!(h.store.canonical_execution_space_ids().unwrap().is_empty());
    for i in 0..4 {
        let id = format!("member-{i}");
        let mut ctx = context(host.clone(), "member.create", &id, 0);
        if i % 2 == 1 {
            ctx.execution_space_id = "second-space".into();
        }
        h.store
            .create_trust_agent_member(&ctx, member(&id, &host))
            .unwrap();
        let observed = h
            .store
            .trust_agent_members(&ctx.execution_space_id)
            .unwrap();
        assert!(observed.iter().any(|m| m.id == id));
        assert_eq!(h.store.read_scan_metrics()[0].decoded_rows, i + 1);
        h.store
            .trust_agent_members(&ctx.execution_space_id)
            .unwrap();
        assert_eq!(h.store.read_scan_metrics()[0].decoded_rows, 0);
    }
    assert_eq!(
        h.store.canonical_execution_space_ids().unwrap(),
        vec!["second-space", SPACE]
    );
    // Every atomic writer replaces the history; no synthetic append shortcut.
    assert_eq!(h.store.canonical_operations().unwrap().len(), 4);
    assert_eq!(h.store.trust_agent_members(SPACE).unwrap().len(), 2);
}

#[test]
fn actual_idle_delivery_work_and_attention_queries_do_not_rescan_unrelated_history() {
    let h = TestStore::new("actual-idle-cache");
    let host = human("host");
    for i in 0..40 {
        let id = format!("unrelated-member-{i}");
        h.store
            .create_trust_agent_member(
                &context(host.clone(), "member.create", &id, 0),
                member(&id, &host),
            )
            .unwrap();
    }
    let query = || {
        assert!(h.store.current_work_deliveries(SPACE).unwrap().is_empty());
        assert!(h.store.latest_works().unwrap().is_empty());
        assert!(h.store.host_attentions().unwrap().is_empty());
    };
    query();
    let before = h
        .store
        .read_scan_metrics()
        .into_iter()
        .find(|m| m.ledger == "agentfirm_trust_operations.jsonl")
        .unwrap();
    for _ in 0..20 {
        query();
    }
    let after = h
        .store
        .read_scan_metrics()
        .into_iter()
        .find(|m| m.ledger == "agentfirm_trust_operations.jsonl")
        .unwrap();
    assert_eq!(before.total_bytes_read, after.total_bytes_read);
    assert_eq!(before.total_decoded_rows, after.total_decoded_rows);
    assert_eq!(before.total_cloned_rows, after.total_cloned_rows);
}

#[test]
fn historical_side_work_survives_a_newer_unrelated_projection() {
    let h = TestStore::new("historical-side-work-cache");
    seed_team_work(&h.store, "side-history", "work-side-history");
    let mut side_work = h
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|w| w.id == "work-side-history")
        .unwrap();
    side_work.version += 1;
    side_work.title = "historical atomic side projection".into();
    let ledger = h.root.join("agentfirm_trust_operations.jsonl");
    let content = std::fs::read_to_string(&ledger).unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_str(content.lines().last().unwrap()).unwrap();
    // Synthetic persisted history fixture: the subsequent aggregate update
    // omits an earlier atomic side Work. A latest-envelope fold loses it.
    envelope["operation"]["immutable_side_records"] = serde_json::json!([side_work]);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .unwrap();
    writeln!(file, "{}", envelope).unwrap();
    envelope["operation"]["immutable_side_records"] = serde_json::json!([]);
    writeln!(file, "{}", envelope).unwrap();
    let actual = h
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|w| w.id == side_work.id)
        .unwrap();
    assert_eq!(actual, side_work);
    let before = h
        .store
        .read_scan_metrics()
        .into_iter()
        .find(|m| m.ledger == "agentfirm_trust_operations.jsonl")
        .unwrap();
    assert_eq!(
        h.store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|w| w.id == side_work.id)
            .unwrap(),
        side_work
    );
    let after = h
        .store
        .read_scan_metrics()
        .into_iter()
        .find(|m| m.ledger == "agentfirm_trust_operations.jsonl")
        .unwrap();
    assert_eq!(before.total_decoded_rows, after.total_decoded_rows);
}

#[test]
fn membership_cache_keeps_filter_before_fold_and_scope_local_errors() {
    let h = TestStore::new("membership-scope-cache");
    seed_team(&h.store, "membership-cache", &["host"]);
    let original = h
        .store
        .fabric_team_memberships(SPACE)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let ledger = h.root.join("agentfirm_trust_operations.jsonl");
    let content = std::fs::read_to_string(&ledger).unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_str(content.lines().last().unwrap()).unwrap();
    let mut foreign = original.clone();
    foreign.team_id = "other-team".into();
    envelope["execution_space_id"] = serde_json::json!(SPACE);
    envelope["operation"]["event"]["aggregate_kind"] = serde_json::json!("team_membership");
    envelope["operation"]["event"]["aggregate_id"] = serde_json::json!(original.id);
    envelope["operation"]["resulting_projection"] = serde_json::to_value(&foreign).unwrap();
    envelope["operation"]["immutable_side_records"] = serde_json::json!([]);
    envelope["operation"]["initial_outbox_records"] = serde_json::json!([]);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .unwrap();
    writeln!(file, "{}", envelope).unwrap();
    assert_eq!(
        h.store
            .fabric_team_memberships_for_team(SPACE, &original.team_id)
            .unwrap()
            .into_iter()
            .find(|m| m.id == original.id)
            .unwrap(),
        original
    );
    assert_eq!(
        h.store
            .fabric_team_memberships_for_team(SPACE, "other-team")
            .unwrap(),
        vec![foreign]
    );
    // A malformed primary projection in another Team must not newly block the
    // original scope; the all-Space reader still rejects it as before.
    envelope["operation"]["resulting_projection"] = serde_json::json!({"team_id":"other-team"});
    writeln!(file, "{}", envelope).unwrap();
    assert!(h.store.fabric_team_memberships(SPACE).is_err());
    assert!(h
        .store
        .fabric_team_memberships_for_team(SPACE, "other-team")
        .is_err());
    assert!(h
        .store
        .fabric_team_memberships_for_team(SPACE, &original.team_id)
        .is_ok());
}

#[test]
fn message_delivery_scope_uses_original_source_order_across_message_ids() {
    let h = TestStore::new("message-delivery-source-order");
    let host = human("host");
    h.store
        .create_trust_agent_member(
            &context(host.clone(), "member.create", "seed", 0),
            member("seed", &host),
        )
        .unwrap();
    let ledger = h.root.join("agentfirm_trust_operations.jsonl");
    let mut envelope: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(&ledger)
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    let delivery = |message: &str, attempt: u64| {
        serde_json::json!({
            "id":"same-delivery", "message_id":message, "subscription_id":"subscription", "subscription_revision":1,
            "subscription_policy_digest":"policy", "recipient_kind":"agent_member", "recipient_ref":"recipient",
            "target_node_id":NODE, "status":"queued", "attempt":attempt, "version":attempt+1,
            "created_at":"t1", "updated_at":"t2"
        })
    };
    envelope["operation"]["initial_outbox_records"] = serde_json::json!([delivery("z-message", 0)]);
    envelope["operation"]["immutable_side_records"] =
        serde_json::json!([delivery("a-message", 1), delivery("z-message", 2)]);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .unwrap();
    writeln!(file, "{}", envelope).unwrap();
    for _ in 0..20 {
        let ids = std::collections::HashSet::from(["a-message".into(), "z-message".into()]);
        let rows = h
            .store
            .fabric_message_deliveries_for_messages(SPACE, &ids)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message_id, "z-message");
        assert_eq!(rows[0].attempt, 2);
    }
    let rows = h
        .store
        .fabric_message_deliveries_for_messages(
            SPACE,
            &std::collections::HashSet::from(["a-message".into()]),
        )
        .unwrap();
    assert_eq!(rows[0].message_id, "a-message");
    assert_eq!(
        h.store.fabric_message_deliveries(SPACE).unwrap()[0].attempt,
        2
    );
}
