use super::*;

// Historical TeamMessageProjection end-to-end scenario. Its private
// claim/receipt helpers were removed from the integration surface by the
// canonical MessageDelivery clean cutover, so keep the source as migration
// evidence without compiling it against current APIs. One-for-one executable
// coverage lives in `member_execution_trust`:
// - `delivery_claim_and_receipt_are_generation_fenced_and_reconcile_is_explicit`
// - `successor_supervisor_fences_stale_claim_before_any_canonical_side_effect`
#[cfg(any())]
#[test]
#[ignore = "retired TeamMessageProjection claim API; canonical MessageDelivery generation/receipt coverage replaces it"]
fn stale_supervisor_quiesces_and_successor_resumes_mail_once() {
    let home = TempHome::new("team-run-stale-supervisor-quiescence");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let first_prompt_ready = home.base().join("stale-first-prompt-ready");
    let first_prompt_release = home.base().join("stale-first-prompt-release");
    let prompt_marker = home.base().join("stale-kimi-prompts.jsonl");
    let attach_marker = home.base().join("stale-kimi-attach.log");
    let first_prompt_ready_value = first_prompt_ready.display().to_string();
    let first_prompt_release_value = first_prompt_release.display().to_string();
    let prompt_marker_value = prompt_marker.display().to_string();
    let attach_marker_value = attach_marker.display().to_string();
    let mut serve_env = vec![
        ("KIMI_CODE_BIN", fake_kimi.as_str()),
        ("FAKE_KIMI_RESULT", "done"),
        (
            "FAKE_KIMI_FIRST_PROMPT_READY",
            first_prompt_ready_value.as_str(),
        ),
        (
            "FAKE_KIMI_FIRST_PROMPT_RELEASE",
            first_prompt_release_value.as_str(),
        ),
        ("FAKE_KIMI_PROMPT_MARKER", prompt_marker_value.as_str()),
        ("FAKE_KIMI_ATTACH_MARKER", attach_marker_value.as_str()),
        ("FIRM_TEAM_SUPERVISOR_LEASE_MS", "300"),
        ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "300"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Prove stale Supervisor quiescence",
            "members": [{
                "name": "kimi-lease-fence",
                "role": "runtime_reliability",
                "provider": "kimi",
                "initial_work": "Exercise lease fencing"
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
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    for _ in 0..300 {
        if first_prompt_ready.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        first_prompt_ready.exists(),
        "stale generation never reached its first provider prompt"
    );

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let old_lease = store
        .latest_team_supervisor_lease(&run_id)
        .expect("read old lease")
        .expect("old lease");
    assert_eq!(old_lease.generation, 1);
    let initial_session = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .and_then(|member| member.native_session)
        .map(|session| session.native_session_id)
        .expect("initial native session");

    let (status, queued_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "QUEUED_FOR_SUCCESSOR",
        }),
    );
    assert_eq!(status, 200, "body: {queued_mail}");
    let queued_id = queued_mail["result"]["id"]
        .as_str()
        .expect("queued id")
        .to_string();
    let correlation = queued_mail["result"]["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();
    let (status, accepted_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "PROVIDER_ACCEPTED_BEFORE_LOSS",
            "correlation_id": correlation,
            "causation_id": queued_id,
        }),
    );
    assert_eq!(status, 200, "body: {accepted_mail}");
    let accepted_id = accepted_mail["result"]["id"]
        .as_str()
        .expect("accepted id")
        .to_string();
    let (status, uncertain_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "CLAIMED_WITHOUT_RECEIPT_BEFORE_LOSS",
            "correlation_id": correlation,
            "causation_id": accepted_id,
        }),
    );
    assert_eq!(status, 200, "body: {uncertain_mail}");
    let uncertain_id = uncertain_mail["result"]["id"]
        .as_str()
        .expect("uncertain id")
        .to_string();

    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis()
            .min(u64::MAX as u128) as u64
    };
    let claim_id = "claim-with-provider-receipt-before-lease-loss";
    let claimed = store
        .claim_team_message_delivery(
            &run_id,
            &accepted_id,
            &member_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            claim_id,
            now_ms(),
            15_000,
            "unix-ms:test-claimed",
        )
        .expect("claim accepted boundary");
    assert!(
        matches!(
            claimed,
            harness_store::TeamMessageDeliveryClaimResult::Claimed(_)
        ),
        "accepted boundary must be claimed exactly once"
    );
    store
        .complete_team_message_delivery_claim(
            &run_id,
            &accepted_id,
            &member_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            claim_id,
            "native-receipt-before-lease-loss",
            now_ms(),
            "unix-ms:test-delivered",
        )
        .expect("complete accepted boundary");
    let uncertain_claim_id = "claim-without-provider-receipt-before-lease-loss";
    let uncertain_claimed = store
        .claim_team_message_delivery(
            &run_id,
            &uncertain_id,
            &member_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            uncertain_claim_id,
            now_ms(),
            15_000,
            "unix-ms:test-uncertain-claimed",
        )
        .expect("claim uncertain boundary");
    assert!(
        matches!(
            uncertain_claimed,
            harness_store::TeamMessageDeliveryClaimResult::Claimed(_)
        ),
        "uncertain boundary must be claimed exactly once"
    );

    let prompt_count = || {
        std::fs::read_to_string(&prompt_marker)
            .unwrap_or_default()
            .lines()
            .count()
    };
    assert_eq!(
        prompt_count(),
        1,
        "only the blocked stale prompt may have reached the provider"
    );

    // Supersede generation 1 while its first prompt is blocked. uncertain_mail
    // is the claimed-without-receipt boundary, accepted_mail is the
    // claimed+receipt boundary, and queued_mail remains successor-owned work.
    store
        .release_team_supervisor_lease(
            &run_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            now_ms(),
        )
        .expect("release stale generation");
    let fencing_lease = store
        .acquire_team_supervisor_under_node_lease(
            &run_id,
            &old_lease.node_id,
            &old_lease.node_daemon_id,
            old_lease.node_daemon_generation,
            &old_lease.execution_space_id,
            &old_lease.project_binding_id,
            "test-fencing-supervisor",
            std::process::id(),
            "tcp://127.0.0.1:1",
            now_ms(),
            10_000,
        )
        .expect("acquire fencing generation");
    assert_eq!(fencing_lease.generation, 2);

    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        prompt_count(),
        1,
        "stale generation must not start, resume, or prompt after lease loss"
    );
    let (control_status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({
            "reason": "must be fenced",
            "requested_by": "test"
        }),
    );
    assert_ne!(
        control_status, 200,
        "live control must reject the stale generation"
    );
    assert_eq!(
        prompt_count(),
        1,
        "rejected stale live control must not touch the provider"
    );
    let disconnected_actions = store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .filter(|action| action.member_run_id == member_id && action.action_type == "disconnected")
        .count();
    assert_eq!(
        disconnected_actions, 0,
        "lease loss must not enter the retrying starting-disconnected loop"
    );

    // Every fake ACP process treats its own first prompt as prompt #1. The
    // stale process is already quiesced, so release future successor prompts.
    std::fs::write(&first_prompt_release, b"release successor")
        .expect("release successor fake prompt");
    store
        .release_team_supervisor_lease(
            &run_id,
            &fencing_lease.supervisor_id,
            fencing_lease.generation,
            now_ms(),
        )
        .expect("release fencing generation");
    let mut successor_started = false;
    for _ in 0..200 {
        let (status, _) = serve.post_json(
            &format!("/v1/team-runs/{run_id}/start"),
            &serde_json::json!({}),
        );
        if status == 202 {
            successor_started = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        successor_started,
        "successor could not attach after stale generation quiesced"
    );

    let mut converged = None;
    let mut last_snapshot = None;
    for _ in 0..400 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"]
            .as_array()
            .expect("snapshot messages");
        let delivery = |id: &str| {
            messages
                .iter()
                .find(|message| message["id"].as_str() == Some(id))
                .map(|message| message["deliveries"][0].clone())
        };
        let queued_delivery = delivery(&queued_id);
        let accepted_delivery = delivery(&accepted_id);
        let uncertain_delivery = delivery(&uncertain_id);
        let member = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()));
        // The successor emits one batched mail prompt after the stale Work
        // prompt. It must not invent a third prompt by replaying the initial
        // Work whose old-generation claim has no provider receipt; that
        // uncertainty remains durable until explicit reconciliation.
        let ready = uncertain_delivery.as_ref().is_some_and(|delivery| {
            delivery["status"] == "claimed"
                && delivery["attempt"] == 1
                && delivery["claimed_generation"] == 1
                && delivery["provider_receipt_id"].is_null()
        }) && queued_delivery.as_ref().is_some_and(|delivery| {
            delivery["status"] == "delivered"
                && delivery["attempt"] == 1
                && delivery["claimed_generation"] == 3
                && delivery["provider_receipt_id"]
                    .as_str()
                    .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:"))
        }) && accepted_delivery.as_ref().is_some_and(|delivery| {
            delivery["status"] == "delivered"
                && delivery["attempt"] == 1
                && delivery["claimed_generation"] == 1
                && delivery["provider_receipt_id"] == "native-receipt-before-lease-loss"
        }) && member.is_some_and(|member| {
            member["status"] == "idle"
                && member["native_session"]["native_session_id"] == initial_session
        }) && prompt_count() == 2;
        if ready {
            converged = Some(snapshot);
            break;
        }
        last_snapshot = Some(snapshot);
        std::thread::sleep(Duration::from_millis(20));
    }
    let snapshot = converged.unwrap_or_else(|| {
        panic!(
            "successor did not converge all delivery boundaries; prompts={}; attach={:?}; snapshot={}",
            prompt_count(),
            std::fs::read_to_string(&attach_marker),
            last_snapshot.unwrap_or_default(),
        )
    });
    let attach_log = std::fs::read_to_string(&attach_marker).expect("successor attach log");
    assert_eq!(
        attach_log.lines().count(),
        1,
        "successor must resume the same native session exactly once: {attach_log}"
    );
    assert!(
        attach_log.contains(&initial_session),
        "successor resumed a different native session: {attach_log}"
    );
    let handoffs = snapshot["team_messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| {
            message["kind"] == "handoff"
                && message["sender_runtime_id"].as_str() == Some(member_id.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handoffs.len(),
        0,
        "the unresolved claimed delivery must continue fencing semantic handoff"
    );
    let prompt_log = std::fs::read_to_string(&prompt_marker).expect("successor prompt log");
    assert_eq!(
        prompt_log.matches("PROVIDER_ACCEPTED_BEFORE_LOSS").count(),
        0,
        "provider-accepted mail is already present in the resumed native session and must not be replayed: {prompt_log}"
    );
    assert_eq!(
        prompt_log.matches("QUEUED_FOR_SUCCESSOR").count(),
        1,
        "queued mail must reach exactly one successor prompt: {prompt_log}"
    );
    assert_eq!(
        prompt_log
            .matches("CLAIMED_WITHOUT_RECEIPT_BEFORE_LOSS")
            .count(),
        0,
        "claimed-without-receipt mail must remain uncertain and unreplayed: {prompt_log}"
    );
    assert_eq!(
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| action["action_type"] == "runtime_recovery")
            .count(),
        0,
        "provider-accepted mail lives in the resumed native session and must not invent a Work runtime-recovery action"
    );
}
