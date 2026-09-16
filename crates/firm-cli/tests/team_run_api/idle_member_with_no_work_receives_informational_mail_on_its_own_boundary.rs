use super::*;

/// ADR 0077. An informational Message to a member that is idle with no Work
/// has no cycle to ride on: only `response_required` (or a provider-interaction
/// response) wakes a member, and #941 folds queued mail into a cycle that runs
/// for some OTHER reason. When no such cycle ever comes, the mail waits
/// forever — 60 of 111 authored Messages in the S1 dogfood never reached a
/// provider for exactly this reason.
///
/// The guarantee: after the member has been idle past the policy interval, the
/// queued informational batch earns its own Messages boundary. This drives the
/// real member loop and proves the mail reaches a provider input and is
/// acknowledged with that cycle's own provider receipt.
#[test]
fn idle_member_with_no_work_receives_informational_mail_on_its_own_boundary() {
    let home = TempHome::new("informational-idle-boundary");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompts = home.base().join("kimi-prompts.jsonl");
    let prompts_value = prompts.display().to_string();
    // The production interval is 120s (ADR 0077); no deterministic test can
    // wait for it, so the loop reads this test-only override — the same seam
    // shape as FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS. Nothing in production
    // sets it.
    const IDLE_DELIVERY_MS: u64 = 1_500;
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
            (
                "FIRM_TEST_INFORMATIONAL_IDLE_DELIVERY_MS",
                &IDLE_DELIVERY_MS.to_string(),
            ),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Deliver informational mail to an idle member",
            "members": [{
                "name": "kimi-idle-mail",
                "role": "implementer",
                "provider": "kimi",
                "initial_work": "Finish this Work and go idle"
            }]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("MemberRun id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    // Wait until the member has finished its Work and is genuinely idle with
    // nothing left to run — the state in which mail used to be stranded.
    let idle_deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let first_round = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            });
        if idle && first_round {
            break;
        }
        assert!(
            std::time::Instant::now() < idle_deadline,
            "member never reached idle with a completed round: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let prompts_before = std::fs::read_to_string(&prompts)
        .unwrap_or_default()
        .lines()
        .count();

    let (status, authored) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "response_intent": "informational",
            "body": "IDLE_INFORMATIONAL_MARKER",
        }),
    );
    assert_eq!(status, 200, "body: {authored}");
    assert_eq!(
        authored["result"]["response_intent"].as_str(),
        Some("informational"),
        "this must be the kind that cannot wake a member on its own: {authored}"
    );
    let message_id = authored["result"]["id"]
        .as_str()
        .expect("Message id")
        .to_string();
    assert_eq!(
        authored["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );

    // The guarantee is a bound on latency, not an instant wake: nothing may be
    // delivered before the interval elapses.
    std::thread::sleep(Duration::from_millis(IDLE_DELIVERY_MS / 3));
    let (_, early) = serve.get_json("/v1/snapshot");
    let early_status = early["team_messages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|message| message["id"].as_str() == Some(message_id.as_str()))
        .map(|message| message["deliveries"][0]["status"].clone());
    assert_eq!(
        early_status.as_ref().and_then(|value| value.as_str()),
        Some("queued"),
        "informational mail must not interrupt an idle member before the interval: {early_status:?}"
    );

    // ... and then it must arrive, without any other reason to run a cycle.
    // The bound is the interval plus one poll; the poll backoff climbs to 30s,
    // so the deadline is generous while the assertion stays exact.
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let delivery = loop {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let delivery = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(message_id.as_str()))
            .map(|message| message["deliveries"][0].clone());
        if delivery
            .as_ref()
            .is_some_and(|delivery| delivery["status"].as_str() == Some("acknowledged"))
        {
            break delivery.expect("acknowledged delivery");
        }
        assert!(
            std::time::Instant::now() < deadline,
            "informational mail to an idle member was never delivered: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };

    // It reached a real provider input on its own boundary, not by riding some
    // other cycle: there was no other cycle.
    let prompt_log = std::fs::read_to_string(&prompts).unwrap_or_default();
    let new_prompts: Vec<&str> = prompt_log.lines().skip(prompts_before).collect();
    assert_eq!(
        new_prompts.len(),
        1,
        "the batch earns exactly one boundary, not a turn each: {prompt_log}"
    );
    assert!(
        new_prompts[0].contains("IDLE_INFORMATIONAL_MARKER"),
        "the queued informational batch must be the cycle's input: {}",
        new_prompts[0]
    );
    // And it is acknowledged with THAT cycle's own provider receipt, not a
    // fabricated one.
    assert!(
        delivery["provider_receipt_id"]
            .as_str()
            .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")),
        "the ACK must carry the delivering cycle's provider receipt: {delivery}"
    );
}
