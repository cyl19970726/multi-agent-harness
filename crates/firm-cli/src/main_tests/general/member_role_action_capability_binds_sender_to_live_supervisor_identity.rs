use super::*;

    #[test]
    fn member_role_action_capability_binds_sender_to_live_supervisor_identity() {
        let (store, root) = temp_store("member-role-action-capability");
        let created = create_two_member_team_run(&store);
        let first = created.member_runs[0].clone();
        let second = created.member_runs[1].clone();
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-member-role-action",
                std::process::id(),
                "test://member-role-action",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire Supervisor lease");
        ensure_test_runtime_fabric(&store, &created, &lease);
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        transition_provider_session_for_member(
            &ledger,
            &first,
            harness_core::agentfirm_api::AgentSessionStatus::Active,
        )
        .expect("activate exact sender session");

        let first_token = "a".repeat(64);
        let second_token = "b".repeat(64);
        let (first_control, _first_registration) =
            register_live_member_control(&first, &first_token, 1);
        let (second_control, _second_registration) =
            register_live_member_control(&second, &second_token, 1);
        let supervisor_valid = AtomicBool::new(true);
        let authority_gate = Mutex::new(());
        author_test_canonical_message(
            &store,
            &created,
            &lease,
            &lease.execution_space_id,
            "bound-member-private-inbox",
            &second.agent_member_id,
            &first.agent_member_id,
            harness_core::agentfirm_api::MessageKind::Message,
            "Private exact-self inbox message",
            "bound-member-private-inbox-thread",
            None,
            harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
        );
        let forged_inbox = dispatch_local_live_member_control(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &supervisor_valid,
            &authority_gate,
            LiveMemberControlRequest::ReadInbox {
                team_run_id: created.team_run.id.clone(),
                member_run_id: second.id.clone(),
                capability_token: first_token.clone(),
                include_all: true,
            },
        )
        .expect_err("one member capability cannot read its sibling inbox");
        assert!(forged_inbox.to_string().contains("UNAUTHORIZED_ACTOR"));
        let own_inbox = dispatch_local_live_member_control(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &supervisor_valid,
            &authority_gate,
            LiveMemberControlRequest::ReadInbox {
                team_run_id: created.team_run.id.clone(),
                member_run_id: first.id.clone(),
                capability_token: first_token.clone(),
                include_all: true,
            },
        )
        .expect("exact live capability reads only its own Inbox");
        let own_inbox =
            serde_json::from_value::<Vec<TeamMessageProjection>>(own_inbox).expect("Inbox rows");
        assert_eq!(own_inbox.len(), 1);
        assert_eq!(own_inbox[0].body, "Private exact-self inbox message");
        let work_value = create_team_work_value(
            &store,
            &created.team_run.id,
            &serde_json::json!({
                "id": "member-capability-work",
                "title": "Prove bound member Role Action authority",
                "completion_criteria_markdown": "Only the exact live member can start it",
                "owner_member_run_id": first.id,
            }),
        )
        .expect("create assigned Work");
        let work: Work = serde_json::from_value(work_value).expect("decode assigned Work");
        let route = format!(
            "/v1/agentfirm/team-runs/{}/works/{}/start",
            created.team_run.id, work.id
        );
        let start_body = serde_json::json!({"action": "start_work"});
        let before_forgery = durable_store_file_bytes(&store);
        let forged = dispatch_local_live_member_control(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &supervisor_valid,
            &authority_gate,
            LiveMemberControlRequest::RoleAction {
                team_run_id: created.team_run.id.clone(),
                member_run_id: second.id.clone(),
                capability_token: first_token.clone(),
                path: route.clone(),
                expected_version: work.version,
                idempotency_key: "forged-sibling-work-start".into(),
                body: start_body.clone(),
            },
        )
        .expect_err("one member capability cannot select its sibling identity");
        assert!(forged.to_string().contains("UNAUTHORIZED_ACTOR"));
        assert_eq!(
            durable_store_file_bytes(&store),
            before_forgery,
            "rejected identity forgery must have byte-zero durable side effects"
        );

        dispatch_local_live_member_control(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &supervisor_valid,
            &authority_gate,
            LiveMemberControlRequest::RoleAction {
                team_run_id: created.team_run.id.clone(),
                member_run_id: first.id.clone(),
                capability_token: first_token,
                path: route,
                expected_version: work.version,
                idempotency_key: "valid-member-work-start".into(),
                body: start_body,
            },
        )
        .expect("exact live capability performs canonical Work Role Action");
        let started = store
            .latest_works()
            .expect("latest Works")
            .into_iter()
            .find(|candidate| candidate.id == work.id)
            .expect("started Work");
        assert_eq!(started.phase, WorkPhase::Active);
        assert_eq!(
            started.active_member_run_id.as_deref(),
            Some(first.id.as_str())
        );
        assert!(matches!(
            first_control.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert!(matches!(
            second_control.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

