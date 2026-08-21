use super::*;

#[test]
#[ignore = "retired member-to-Host Handoff projection; canonical WorkReport and MessageDelivery paths are covered separately"]
fn codex_app_server_post_handoff_steer_is_independent_and_converges_before_follow_up_round() {
    let home = TempHome::new("team-run-codex-post-handoff-steer");
    let project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-post-handoff"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_AUTO_COMPLETE_AFTER_STEER", "1"),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Converge one native turn after an explicit Handoff",
            "members": [{
                "name": "codex-convergence",
                "role": "implementer",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise same-turn convergence"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut live = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let member_running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        let work_delivered = snapshot["work_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
            .is_some_and(|delivery| {
                matches!(
                    delivery["status"].as_str(),
                    Some("claimed" | "provider_received")
                )
            });
        live = member_running && work_delivered;
        if live {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(live, "app-server member never became live");

    let (status, explicit_handoff) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_kind": "member_run",
            "sender_id": member_id,
            "recipient_runtime_ids": [member_id.clone()],
            "kind": "message",
            "body": "## RESULT\ndone\n## SUMMARY\nexplicit same-turn handoff",
        }),
    );
    assert_eq!(status, 200, "body: {explicit_handoff}");
    let explicit_handoff_id = explicit_handoff["result"]["id"]
        .as_str()
        .expect("handoff id")
        .to_string();
    let conversation_correlation = explicit_handoff["result"]["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();

    let control_client = ServeHandle::spawn(&home, home.base(), &[]);
    let descendant_client = ServeHandle::spawn(&home, home.base(), &[]);
    let observer_barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let observer_ready = std::sync::Arc::clone(&observer_barrier);
    let observer = std::thread::spawn(move || {
        observer_ready.wait();
        for _ in 0..200 {
            let (_, snapshot) = descendant_client.get_json("/v1/snapshot");
            let control = snapshot["team_messages"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|message| {
                    message["kind"].as_str() == Some("control")
                        && message["body"].as_str()
                            == Some("incorporate the correction before ending this turn")
                });
            if let Some(control) = control {
                let control_id = control["id"].as_str().expect("Control id").to_string();
                let correlation_id = control["correlation_id"]
                    .as_str()
                    .expect("Control correlation")
                    .to_string();
                let observed_delivery = control["deliveries"][0].clone();
                return (control_id, correlation_id, observed_delivery);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("concurrent observer never saw the Steer Control")
    });
    observer_barrier.wait();
    let (status, steered) = control_client.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({
            "content": "incorporate the correction before ending this turn",
            "requested_by": "operator"
        }),
    );
    assert_eq!(status, 200, "body: {steered}");
    let steer_correlation = steered["result"]["message"]["correlation_id"]
        .as_str()
        .expect("Steer correlation")
        .to_string();
    assert_ne!(
        steer_correlation, conversation_correlation,
        "live control must not infer Work or conversation ownership from a prior Handoff"
    );
    assert!(steered["result"]["message"]["causation_id"].is_null());
    let steer_message_id = steered["result"]["message"]["id"]
        .as_str()
        .expect("Steer control message")
        .to_string();
    let (observed_control_id, observed_correlation, observed_delivery) =
        observer.join().expect("concurrent Control observer");
    assert_eq!(observed_control_id, steer_message_id);
    assert_eq!(observed_correlation, steer_correlation);
    assert_eq!(observed_delivery["policy"], "inject");
    assert_eq!(observed_delivery["status"], "delivered");
    let physical_control_rows = std::fs::read_to_string(
        home.spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl"),
    )
    .expect("read physical TeamMessageProjection rows")
    .lines()
    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    .filter(|message| message["id"].as_str() == Some(steer_message_id.as_str()))
    .collect::<Vec<_>>();
    assert_eq!(
        physical_control_rows.len(),
        1,
        "Steer Control must be published exactly once: {physical_control_rows:?}"
    );
    assert_eq!(
        physical_control_rows[0]["deliveries"][0]["policy"],
        "inject"
    );
    assert_eq!(
        physical_control_rows[0]["deliveries"][0]["status"],
        "delivered"
    );

    let mut converged = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let explicit_message_present = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| message["id"].as_str() == Some(explicit_handoff_id.as_str()));
        converged = idle && explicit_message_present;
        if converged {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        converged,
        "same-turn Steer must not append a sibling fallback Handoff"
    );

    let (status, follow_up) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "OPEN NEXT ROUND after idle",
            "correlation_id": conversation_correlation,
            "causation_id": explicit_handoff_id,
        }),
    );
    assert_eq!(status, 200, "body: {follow_up}");
    let follow_up_id = follow_up["result"]["id"]
        .as_str()
        .expect("follow-up id")
        .to_string();

    let completed_before = {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count()
    };
    let mut next_round = false;
    for _ in 0..150 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let follow_up_delivered = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(follow_up_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "delivered");
        let completed_after = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count();
        next_round = follow_up_delivered && completed_after > completed_before;
        if next_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        next_round,
        "ordinary post-idle correlated follow-up must open a new provider round without fabricating a Handoff"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let work = snapshot["works"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|work| work["id"].as_str() == Some(work_id.as_str()))
        .expect("Work in snapshot");
    assert_eq!(
        work["phase"].as_str(),
        Some("open"),
        "provider receipt, conversation Handoff, and provider RESULT must not infer Work start/submission/completion: {work}"
    );
}
