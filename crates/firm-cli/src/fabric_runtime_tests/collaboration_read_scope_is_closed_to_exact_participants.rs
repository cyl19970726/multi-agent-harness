use super::*;

    #[test]
    fn collaboration_read_scope_is_closed_to_exact_participants() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-collaboration-read-scope-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut attestation: harness_core::collaboration::SourceWorkAttestation =
            serde_json::from_str(include_str!(
                "../../../../schemas/collaboration/fixtures/source-work-attestation/valid/server-authored.json"
            ))
            .unwrap();
        let delegation: harness_core::collaboration::WorkDelegationV1 =
            serde_json::from_str(include_str!(
                "../../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
            ))
            .unwrap();
        attestation.id = delegation.source_work_attestation_id.clone();
        let make_operation = |aggregate_kind: &str,
                              aggregate_id: &str,
                              sequence: u64,
                              projection: serde_json::Value| {
            harness_store::CollaborationOperation {
                store_version: harness_core::collaboration::COLLABORATION_STORE_VERSION.into(),
                company_id: "company-1".into(),
                command_name: "fixture".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "fixture".into(),
                },
                idempotency_key: format!("fixture-{sequence}"),
                request_fingerprint: format!("sha256:{sequence:064x}"),
                aggregate_kind: aggregate_kind.into(),
                aggregate_id: aggregate_id.into(),
                store_sequence: sequence,
                resulting_revision: 1,
                resulting_projection: projection,
                immutable_side_records: Vec::new(),
                created_at: "t1".into(),
            }
        };
        let rows = [
            make_operation(
                "source_work_attestation",
                &attestation.id,
                1,
                serde_json::to_value(&attestation).unwrap(),
            ),
            make_operation(
                "work_delegation_v1",
                &delegation.id,
                2,
                serde_json::to_value(&delegation).unwrap(),
            ),
        ];
        std::fs::write(
            root.join("agentfirm_collaboration_operations.jsonl"),
            rows.iter()
                .map(|row| serde_json::to_string(row).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let store = HarnessStore::new(&root);
        for (label, actor) in [
            ("source owner", attestation.source_owner_ref.clone()),
            ("source Host", attestation.source_host_ref.clone()),
            ("target Host", delegation.target_host_ref.clone()),
        ] {
            assert!(
                collaboration_actor_can_read_delegation(&store, "company-1", &actor, &delegation)
                    .unwrap(),
                "{label} must see the exact Delegation: {actor:?}"
            );
        }
        assert!(!collaboration_actor_can_read_delegation(
            &store,
            "company-1",
            &harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: "host-from-sibling-team".into(),
            },
            &delegation,
        )
        .unwrap());
        assert!(!collaboration_actor_can_read_delegation(
            &store,
            "company-2",
            &attestation.source_host_ref,
            &delegation,
        )
        .unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

