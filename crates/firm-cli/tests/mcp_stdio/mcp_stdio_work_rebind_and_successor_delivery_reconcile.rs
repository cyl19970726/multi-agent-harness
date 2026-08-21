use super::*;

#[test]
fn mcp_stdio_work_rebind_and_successor_delivery_reconcile() {
    let home = TempHome::new("mcp-work-rebind-reconcile");
    let project_id = init_project(&home, "mcp-work-control-proj");
    let project_root = std::fs::canonicalize(home.base().join("mcp-work-control-proj"))
        .expect("canonical project root");
    let stable_agent_id =
        seed_canonical_member(&home, &project_root, &project_id, "rebind", "implementer");
    let mission = run_firm(
        &home,
        &project_root,
        &[
            "mission",
            "create",
            "--title",
            "Rebind",
            "--objective",
            "Exercise recovery",
        ],
    );
    assert!(
        mission.status.success(),
        "mission create failed: {mission:?}"
    );
    let mission_id = String::from_utf8_lossy(&mission.stdout).trim().to_string();
    let team_id = seed_team_for_mission(
        &home,
        &project_root,
        &mission_id,
        &stable_agent_id,
        "rebind",
        &[],
    );

    // This is the successful rebind path, so pin the provider probe to the
    // reviewed fake Kimi version instead of inheriting a developer machine's
    // potentially review_required installation.
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let mut mcp = McpClient::spawn(&home, &project_id, &[("KIMI_CODE_BIN", fake_kimi.as_str())]);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Exercise MCP Work lifecycle recovery",
                "agent_team_id": team_id,
                "execution_root": project_root,
                "members": [{
                    "name": "stable-worker",
                    "role": "implementer",
                    "provider": "kimi",
                    "agent_member_id": stable_agent_id,
                    "initial_work": "Preserve durable ownership across runtime replacement."
                }]
            }
        }),
    );
    let created = call_payload(&response);
    let team_run_id = created["team_run_id"]
        .as_str()
        .expect("team run id")
        .to_string();
    let old_member_id = created["member_run_ids"][0]
        .as_str()
        .expect("old member id")
        .to_string();
    let work_id = created["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let initial_version = created["works"][0]["version"]
        .as_u64()
        .expect("initial Work version");

    // Assign is deliberately not a reassignment primitive. An already-owned
    // Work must move to another runtime only through rebind.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_assign",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": work_id,
                "member_run_id": old_member_id,
                "expected_version": initial_version
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("assign conflict")
        .contains("must be open to assign"));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_deactivate_member",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": old_member_id,
                "reason": "replace crashed runtime generation"
            }
        }),
    );
    call_payload(&response);

    // Runtime replacement is normally produced by the lifecycle controller.
    // The focused MCP test materializes that prerequisite directly, then proves
    // the public rebind tool preserves the stable AgentMember owner.
    let store = HarnessStore::new(home.spaces_dir().join("mcp-space-rebind"));
    let old_member = store
        .member_runs()
        .expect("member runs")
        .into_iter()
        .rev()
        .find(|member| member.id == old_member_id)
        .expect("deactivated member");
    let mut replacement = old_member.clone();
    replacement.id = "member-mcp-stable-worker-generation-2".to_string();
    replacement.coordination_status = MemberCoordinationStatus::Active;
    replacement.runtime_generation += 1;
    replacement.status = MemberRunStatus::Idle;
    replacement.native_session = None;
    replacement.started_at = "unix-ms:mcp-replacement".to_string();
    replacement.last_event_at = None;
    replacement.finished_at = None;
    let run = store
        .team_runs()
        .expect("team runs")
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .expect("TeamRun");
    let mut next_run = run.clone();
    next_run.member_run_ids.push(replacement.id.clone());
    let _ = (&run, &next_run, &replacement);
    panic!("historical projection-only MemberRun reconstruction was retired; use current combined TeamRun admission evidence");

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_rebind",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": work_id,
                "member_run_id": replacement.id,
                "expected_version": initial_version
            }
        }),
    );
    let rebound = call_payload(&response);
    assert_eq!(
        rebound["owner_member_id"].as_str(),
        created["works"][0]["owner_member_id"].as_str()
    );
    assert_eq!(
        rebound["active_member_run_id"].as_str(),
        Some(replacement.id.as_str())
    );
    let rebound_version = rebound["version"].as_u64().expect("rebound version");
    assert_eq!(rebound_version, initial_version + 1);

    let delivery = store
        .latest_work_deliveries()
        .expect("latest WorkDeliveries")
        .into_iter()
        .find(|delivery| {
            delivery.work_id == work_id
                && delivery.work_version == rebound_version
                && delivery.recipient_member_run_id == replacement.id
        })
        .expect("replacement ProviderWorkDispatch");
    assert_eq!(delivery.status, ProviderWorkDispatchStatus::Queued);
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64;
    let node_daemon = store
        .acquire_node_daemon_lease(
            &run.execution_node_id,
            "daemon-mcp",
            "instance-mcp",
            now_unix_ms,
            60_000,
        )
        .expect("parent NodeDaemon lease");
    let first = store
        .acquire_team_supervisor_under_node_lease(
            &team_run_id,
            &run.execution_node_id,
            &node_daemon.daemon_id,
            node_daemon.generation,
            &project_id,
            &run.project_binding_id,
            "supervisor-mcp-generation-1",
            11,
            "mcp:test:first",
            now_unix_ms,
            10,
        )
        .expect("first Supervisor lease");
    let claimed = match store
        .claim_work_delivery(
            &team_run_id,
            &delivery.id,
            &replacement.id,
            &first.supervisor_id,
            first.generation,
            "claim-mcp-work-generation-1",
            now_unix_ms + 1,
            "unix-ms:mcp-claim",
        )
        .expect("claim replacement delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("replacement delivery must be queued"),
    };
    assert_eq!(claimed.status, ProviderWorkDispatchStatus::Claimed);
    let successor = store
        .acquire_team_supervisor_under_node_lease(
            &team_run_id,
            &run.execution_node_id,
            &node_daemon.daemon_id,
            node_daemon.generation,
            &project_id,
            &run.project_binding_id,
            "supervisor-mcp-generation-2",
            22,
            "mcp:test:successor",
            now_unix_ms + 11,
            60_000,
        )
        .expect("successor Supervisor lease");
    assert_eq!(successor.generation, first.generation + 1);

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_reconcile_delivery",
            "arguments": {
                "team_run_id": team_run_id,
                "delivery_id": delivery.id,
                "supervisor_id": successor.supervisor_id,
                "supervisor_generation": successor.generation
            }
        }),
    );
    let reconciled = call_payload(&response);
    assert_eq!(reconciled["status"].as_str(), Some("queued"));
    assert!(reconciled["claim_id"].is_null());
    assert!(reconciled["claimed_by_supervisor_id"].is_null());
    assert!(reconciled["claimed_generation"].is_null());
}
