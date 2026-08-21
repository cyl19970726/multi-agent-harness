use super::*;

#[test]
fn two_peer_ack_only_mail_converges_without_extra_rounds_and_batches_on_next_trigger() {
    let home = TempHome::new("team-run-two-peer-convergence");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompts = home.base().join("kimi-prompts.jsonl");
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
            // Keep idle members inside their wake loop for the whole scenario;
            // the default 250ms test grace would retire them before the later
            // response-required triggers arrive.
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Two-peer bounded convergence on acknowledgement-only mail",
            "members": [
                {"name": "peer-a", "role": "implementer", "provider": "kimi",
                 "initial_work": "Complete peer A lane"},
                {"name": "peer-b", "role": "reviewer", "provider": "kimi",
                 "initial_work": "Complete peer B lane"}
            ]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_a = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_b = created["result"]["member_runs"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let snapshot_messages = |serve: &ServeHandle| -> Vec<serde_json::Value> {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["team_messages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let member_status = |serve: &ServeHandle, member_id: &str| -> Option<String> {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id))
            .and_then(|member| member["status"].as_str().map(str::to_string))
    };
    let completed_rounds = |serve: &ServeHandle, member_id: &str| -> usize {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id)
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count()
    };
    let follow_up_rounds = |prompts: &std::path::Path| -> usize {
        std::fs::read_to_string(prompts)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("TEAM MESSAGES arrived."))
            .count()
    };

    let mut round_one = false;
    for _ in 0..300 {
        round_one = completed_rounds(&serve, &member_a) >= 1
            && completed_rounds(&serve, &member_b) >= 1
            && member_status(&serve, &member_a).as_deref() == Some("idle")
            && member_status(&serve, &member_b).as_deref() == Some("idle");
        if round_one {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(round_one, "both peers must finish round one and go idle");

    // Ack-only PEER mail must NOT wake an idle peer into a provider round
    // (ADR 0046 §4); the delivery stays durable and queued. This is the
    // sender-aware default: no explicit intent is set on the wire, only
    // explicit member provenance.
    let (status, fyi) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_kind": "member_run",
            "sender_id": member_b,
            "sender_runtime_id": member_b,
            "recipient_runtime_ids": [member_a],
            "kind": "message",
            "body": "ACK: your lane note landed; no reply needed",
        }),
    );
    assert_eq!(status, 200, "body: {fyi}");
    let fyi_id = fyi["result"]["id"].as_str().unwrap().to_string();
    let correlation_a = fyi["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        fyi["result"]["response_intent"].as_str(),
        Some("informational")
    );

    // Host mail is response-required by DEFAULT (Host questions, revisions,
    // and acceptance decisions all ride on `message`), so an FYI-only Host
    // note must say so explicitly. That explicit override is also non-waking.
    let (status, host_fyi) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_a],
            "kind": "message",
            "response_intent": "informational",
            "body": "FYI: the wave advanced; no reply needed",
            "correlation_id": correlation_a,
        }),
    );
    assert_eq!(status, 200, "body: {host_fyi}");
    let host_fyi_id = host_fyi["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        host_fyi["result"]["response_intent"].as_str(),
        Some("informational")
    );

    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        follow_up_rounds(&prompts),
        0,
        "informational mail must not start a provider round: {}",
        std::fs::read_to_string(&prompts).unwrap_or_default()
    );
    assert_eq!(
        member_status(&serve, &member_a).as_deref(),
        Some("idle"),
        "informational mail must not even mark the member busy"
    );
    for queued_id in [&fyi_id, &host_fyi_id] {
        let delivery = snapshot_messages(&serve)
            .into_iter()
            .find(|message| message["id"].as_str() == Some(queued_id.as_str()))
            .and_then(|message| message["deliveries"][0].clone().into());
        let delivery: serde_json::Value = delivery.expect("informational delivery row");
        assert_eq!(delivery["status"].as_str(), Some("queued"), "{queued_id}");
        assert_eq!(delivery["attempt"].as_u64(), Some(1), "{queued_id}");
    }

    // A response-required question wakes peer A. During that round the
    // scripted provider answers with acknowledgement-only mail to peer B.
    let (status, question) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_a],
            "kind": "message",
            "response_intent": "response_required",
            "body": "QUESTION: confirm your lane state",
            "correlation_id": correlation_a,
        }),
    );
    assert_eq!(status, 200, "body: {question}");
    let question_id = question["result"]["id"].as_str().unwrap().to_string();

    let mut a_second_round = false;
    for _ in 0..300 {
        let messages = snapshot_messages(&serve);
        let question_delivered = messages
            .iter()
            .find(|message| message["id"].as_str() == Some(question_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "acknowledged");
        a_second_round = question_delivered
            && completed_rounds(&serve, &member_a) >= 2
            && member_status(&serve, &member_a).as_deref() == Some("idle");
        if a_second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        a_second_round,
        "response-required question must drive exactly one follow-up round on peer A"
    );
    let (status, peer_ack) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_kind": "member_run",
            "sender_id": member_a,
            "sender_runtime_id": member_a,
            "recipient_runtime_ids": [member_b],
            "kind": "message",
            "response_intent": "informational",
            "body": "ACK: noted, no reply needed",
            "correlation_id": correlation_a,
            "causation_id": question_id,
        }),
    );
    assert_eq!(status, 200, "body: {peer_ack}");
    let peer_ack_id = peer_ack["result"]["id"]
        .as_str()
        .expect("peer ack id")
        .to_string();
    // Both earlier informational notes (the bare peer ack and the explicitly
    // informational Host FYI) rode along with the triggered round and were
    // delivered exactly once with that round's receipt.
    let messages = snapshot_messages(&serve);
    for queued_id in [&fyi_id, &host_fyi_id] {
        let delivery = messages
            .iter()
            .find(|message| message["id"].as_str() == Some(queued_id.as_str()))
            .map(|message| message["deliveries"][0].clone())
            .expect("informational delivery");
        assert_eq!(
            delivery["status"].as_str(),
            Some("acknowledged"),
            "{queued_id}"
        );
        assert_eq!(delivery["attempt"].as_u64(), Some(1), "{queued_id}");
        assert!(
            delivery["provider_receipt_id"]
                .as_str()
                .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")),
            "{queued_id}"
        );
    }

    // Bounded convergence: peer B must NOT start a round for the ack-only
    // mail. Wait long enough for any erroneous round to begin.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        follow_up_rounds(&prompts),
        1,
        "ack-only peer mail must not trigger another provider round: {}",
        std::fs::read_to_string(&prompts).unwrap_or_default()
    );
    let messages = snapshot_messages(&serve);
    let ack_message = messages
        .iter()
        .find(|message| message["id"].as_str() == Some(peer_ack_id.as_str()))
        .expect("peer ack message")
        .clone();
    assert_eq!(
        ack_message["deliveries"][0]["status"].as_str(),
        Some("queued"),
        "ack-only mail stays durable and queued without a round"
    );
    assert_eq!(member_status(&serve, &member_b).as_deref(), Some("idle"));

    // An ordinary Host message now triggers peer B on the sender-aware
    // default alone (no explicit intent on the wire); the queued ack batches
    // into that round and both are delivered exactly once with the same
    // provider receipt.
    let (status, b_trigger) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_b],
            "kind": "message",
            "body": "Start your reviewed lane now",
        }),
    );
    assert_eq!(status, 200, "body: {b_trigger}");
    let b_trigger_id = b_trigger["result"]["id"].as_str().unwrap().to_string();
    let mut b_second_round = false;
    for _ in 0..300 {
        let messages = snapshot_messages(&serve);
        let trigger_delivered = messages
            .iter()
            .find(|message| message["id"].as_str() == Some(b_trigger_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "acknowledged");
        b_second_round = trigger_delivered && completed_rounds(&serve, &member_b) >= 2;
        if b_second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        b_second_round,
        "ordinary Host mail must drive peer B's follow-up round on the sender-aware default"
    );
    let messages = snapshot_messages(&serve);
    let delivery_of = |message_id: &str| -> serde_json::Value {
        messages
            .iter()
            .find(|message| message["id"].as_str() == Some(message_id))
            .map(|message| message["deliveries"][0].clone())
            .expect("delivery row")
    };
    let ack_delivery = delivery_of(ack_message["id"].as_str().unwrap());
    let trigger_delivery = delivery_of(&b_trigger_id);
    assert_eq!(ack_delivery["status"].as_str(), Some("acknowledged"));
    assert_eq!(ack_delivery["attempt"].as_u64(), Some(1));
    assert_eq!(trigger_delivery["status"].as_str(), Some("acknowledged"));
    assert_eq!(trigger_delivery["attempt"].as_u64(), Some(1));
    assert_eq!(
        ack_delivery["provider_receipt_id"].as_str(),
        trigger_delivery["provider_receipt_id"].as_str(),
        "queued informational mail batches into the triggered round"
    );
    // Exactly two follow-up rounds happened in the whole team (A then B):
    // convergence is bounded, no acknowledgement ping-pong.
    assert_eq!(follow_up_rounds(&prompts), 2);
    let prompt_log = std::fs::read_to_string(&prompts).expect("prompt log");
    let b_round_line = prompt_log
        .lines()
        .filter(|line| line.contains("TEAM MESSAGES arrived."))
        .find(|line| line.contains("Start your reviewed lane now"))
        .expect("peer B follow-up prompt");
    let ack_position = b_round_line
        .find("ACK: noted, no reply needed")
        .expect("ack batched first");
    let trigger_position = b_round_line
        .find("Start your reviewed lane now")
        .expect("trigger batched second");
    assert!(
        ack_position < trigger_position,
        "batched mail preserves append order: {b_round_line}"
    );
}
