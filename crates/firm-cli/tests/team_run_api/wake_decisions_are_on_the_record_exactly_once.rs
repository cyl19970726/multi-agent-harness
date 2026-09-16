use super::*;

/// ADR 0078. Before this, the reason a member ran a cycle lived for one stack
/// frame. The provider turn, the Work transition and the message ACK were all
/// durable; the decision that caused them was not, so "why did this member wake
/// at 03:14?" could only be inferred backwards from its side effects.
///
/// The guarantee is one row per wake decision — no more (an idle stretch is two
/// rows, not one per poll) and no fewer (every cycle the member ran names the
/// arm that started it and the durable thing it was decided on).
#[test]
fn wake_decisions_are_on_the_record_exactly_once() {
    let home = TempHome::new("wake-decision-rows");
    let project_id = init_project(&home, "alpha");
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
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Record why every cycle ran",
            "members": [{
                "name": "kimi-recorded",
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
    let initial_work_id = created["result"]["works"][0]["id"]
        .as_str()
        .expect("initial Work id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

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

    // Let a real idle episode accumulate, then wake the member with mail that
    // is allowed to wake it.
    std::thread::sleep(Duration::from_secs(2));
    let (status, authored) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "response_intent": "response_required",
            "body": "WAKE_DECISION_MARKER",
        }),
    );
    assert_eq!(status, 200, "body: {authored}");
    let message_id = authored["result"]["id"]
        .as_str()
        .expect("Message id")
        .to_string();

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let decisions = |store: &HarnessStore| -> Vec<harness_core::TeamRunEvent> {
        store
            .current_team_run_events(&run_id)
            .expect("team run events")
            .into_iter()
            .filter(|event| {
                event.operation == "wake_decided"
                    && event.member_run_id.as_deref() == Some(member_id.as_str())
            })
            .collect()
    };

    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let rows = loop {
        let rows = decisions(&store);
        if rows.iter().any(|row| row.summary.contains(&message_id)) {
            break rows;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the message wake was never recorded: {:?}",
            decisions(&store)
                .iter()
                .map(|row| &row.summary)
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(50));
    };

    // Exactly two cycles ran, so exactly two rows exist. One row per decision
    // is the whole guarantee; a per-poll row would have written dozens by now.
    let summaries: Vec<&str> = rows.iter().map(|row| row.summary.as_str()).collect();
    assert_eq!(summaries.len(), 2, "one row per cycle: {summaries:?}");

    // The first cycle is the eager claim at the top of the poll, which runs
    // before `decide_wake` is consulted at all — its row must say so rather
    // than borrow a predicate arm that never fired.
    assert!(
        summaries[0].starts_with("EagerClaim work=") && summaries[0].contains(&initial_work_id),
        "the initial Work cycle names the eager claim and its Work: {summaries:?}"
    );
    // The second is the eager canonical-message claim, also above
    // `decide_wake`, keyed by the batch it took. Naming the arm rather than
    // deriving it from the delivery is what makes this visible: a row that
    // guessed from the `Messages` wake would have said `DeliverPending` and
    // asserted a predicate that never ran.
    assert_eq!(
        summaries[1],
        format!("CanonicalMessages messages=1 first={message_id}"),
        "the mail cycle names the arm and the batch: {summaries:?}"
    );
    assert!(
        !summaries[1].contains("WAKE_DECISION_MARKER"),
        "a decision row never copies message content: {summaries:?}"
    );

    // One provider prompt per recorded decision, in both directions: no cycle
    // ran unrecorded, and no row exists for a cycle that never ran. The row is
    // written when the decision is made, which is BEFORE the provider turn it
    // authorizes — so this waits for the turn rather than assuming the two are
    // simultaneous. A row is a record of the decision, not a claim about its
    // outcome; the MemberAction rows own that.
    let prompt_deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let prompt_count = std::fs::read_to_string(&prompts)
            .unwrap_or_default()
            .lines()
            .count();
        assert!(
            prompt_count <= rows.len(),
            "a provider turn ran with no decision row: {prompt_count} turns, {} rows",
            rows.len()
        );
        if prompt_count == rows.len() {
            break;
        }
        assert!(
            std::time::Instant::now() < prompt_deadline,
            "a recorded decision never reached a provider: {prompt_count} turns, {} rows",
            rows.len()
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // The idle stretch between them is one closing row carrying the poll count,
    // not one row per poll.
    let episodes: Vec<String> = store
        .current_team_run_events(&run_id)
        .expect("team run events")
        .into_iter()
        .filter(|event| {
            event.operation == "wake_idle_ended"
                && event.member_run_id.as_deref() == Some(member_id.as_str())
        })
        .map(|event| event.summary)
        .collect();
    assert_eq!(
        episodes.len(),
        1,
        "one idle episode ended, so one closing row: {episodes:?}"
    );
    assert!(
        episodes[0].ends_with(": CanonicalMessages") && !episodes[0].contains("after 0 polls"),
        "the closing row names what ended the episode and how long it ran: {episodes:?}"
    );
    // A 2s idle stretch at a 500ms-doubling backoff never reaches the 30s
    // ceiling, so it writes no cap row at all.
    assert!(
        store
            .current_team_run_events(&run_id)
            .expect("team run events")
            .iter()
            .all(|event| event.operation != "wake_idle_capped"),
        "a short episode must not write a cap row"
    );
}
