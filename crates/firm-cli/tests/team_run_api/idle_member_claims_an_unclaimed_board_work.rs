use super::*;

/// ADR 0078. An unclaimed `team_claim` Work has no `owner_member_id`, and every
/// other path into the member loop filters on `owner_member_id == this member`:
/// the eager claim (`claim_canonical_work_for_member`,
/// member_work_coordination.rs:170), `DeliverPending` (`queued_works_for`,
/// :939) and `Continue` (`is_active_work_continuation_candidate`, :402). The
/// `ClaimBoardWork` arm is the ONLY wake that reaches board Work, and the
/// `SHARED WORK AVAILABLE` prompt branch (provider_interactions.rs:1201, keyed
/// on `Open && Normal && owner_member_id.is_none()`) is reachable only through
/// it.
///
/// That arm used to be called `ClaimHint`, carried a `Vec<String>` of work ids
/// its driver discarded, and claimed in a comment to inject a discovery prompt
/// — so it read as dead, and the X4 scope very nearly deleted it. Nothing would
/// have caught that: before this test, neither the arm nor the prompt branch it
/// feeds had a single test anywhere in the repository. This is that test.
#[test]
fn idle_member_claims_an_unclaimed_board_work() {
    let home = TempHome::new("idle-board-claim");
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
            "objective": "Pick up Work from the board",
            "members": [{
                "name": "kimi-board",
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
    let member = member_run_for_work_owner(&created["result"], 0).clone();
    let member_id = member["id"].as_str().expect("MemberRun id").to_string();
    let stable_member_id = member["agent_member_id"]
        .as_str()
        .expect("AgentMember id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    // Drive the member to genuinely idle with its own Work finished, so the
    // board arm is the only thing that can wake it again.
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

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let works = store.latest_works().expect("Work rows");
    // The premise: this member owns no Active Work, so no continuation,
    // delivery or claim path can run a cycle for it.
    assert!(
        !works.iter().any(|work| {
            work.team_run_id == run_id
                && work.owner_member_id.as_deref() == Some(stable_member_id.as_str())
                && work.phase == harness_core::WorkPhase::Active
        }),
        "the member must hold no Active Work or another arm could wake it: {works:?}"
    );
    let team_id = store
        .team_runs()
        .expect("TeamRun rows")
        .into_iter()
        .rev()
        .find(|run| run.id == run_id)
        .expect("TeamRun")
        .agent_team_id;
    let prompts_before = std::fs::read_to_string(&prompts)
        .unwrap_or_default()
        .lines()
        .count();

    // Let the member actually sit idle before the Work appears. The backoff is
    // 500ms doubling, so two seconds is at least two polls that found nothing:
    // this is a member woken OUT of an idle stretch, not one that happened to
    // be mid-poll when the Work landed.
    std::thread::sleep(Duration::from_secs(2));

    // An unclaimed, eligible team_claim Work: no owner, nobody assigned, no
    // delivery row. Nothing about it names this member.
    let board_work = crate::firm_env::member_work::create_work_for_member_run(
        &home,
        &current_space_id(&home),
        &run_id,
        &team_id,
        "work-board-claim-1",
        "Take this from the board",
        "The idle member is woken for it without anyone assigning it",
        &member_id,
        "work-command-board-claim-create",
    );
    assert!(
        board_work.owner_member_id.is_none(),
        "the premise of this test is an UNOWNED board Work: {board_work:?}"
    );
    assert_eq!(
        board_work.claim_mode,
        harness_core::WorkClaimMode::TeamClaim
    );
    assert_eq!(board_work.phase, harness_core::WorkPhase::Open);

    // The idle member must be woken for it. The bound is one poll; the backoff
    // climbs to 30s, so the deadline is generous while the assertion is exact.
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let new_prompt = loop {
        let log = std::fs::read_to_string(&prompts).unwrap_or_default();
        let new: Vec<String> = log
            .lines()
            .skip(prompts_before)
            .map(|line| line.to_string())
            .collect();
        if let Some(prompt) = new.first() {
            break prompt.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "an idle member was never woken for the unclaimed board Work: {log}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        new_prompt.contains("work-board-claim-1"),
        "the wake must carry the board Work: {new_prompt}"
    );
    // The board branch of the continuation prompt, not the ownership branch:
    // the member is told to claim it atomically itself, because the wake is
    // the wake and never the claim.
    assert!(
        new_prompt.contains("SHARED WORK AVAILABLE"),
        "an unowned board Work must arrive on the shared-board branch: {new_prompt}"
    );
    assert!(
        !new_prompt.contains("ACTIVE WORK CONTINUATION"),
        "this member never owned it: {new_prompt}"
    );

    // ADR 0078. The durable decision row must name the arm that actually fired.
    // The delivery is an `ActiveWorkContinuation`, which three arms produce —
    // so a row derived from the delivery would call this one `Continue` and
    // assert the member resumed a Work it owned. It owned nothing. Naming the
    // arm `Continue` is how it became invisible enough to nearly delete, and a
    // row that repeats the mistake is worse than no row.
    let decisions: Vec<harness_core::TeamRunEvent> = store
        .current_team_run_events(&run_id)
        .expect("team run events")
        .into_iter()
        .filter(|event| {
            event.operation == "wake_decided"
                && event.member_run_id.as_deref() == Some(member_id.as_str())
                && event.summary.contains("work-board-claim-1")
        })
        .collect();
    assert_eq!(
        decisions.len(),
        1,
        "one decision, one row: {:?}",
        decisions.iter().map(|e| &e.summary).collect::<Vec<_>>()
    );
    assert_eq!(
        decisions[0].summary,
        format!(
            "ClaimBoardWork work=work-board-claim-1 version={}",
            board_work.version
        ),
        "the row must name the true arm and the Work it was decided on"
    );
    assert_eq!(decisions[0].entity_type, "member_run");
    assert_eq!(decisions[0].entity_id, member_id);
    assert!(
        !decisions[0].summary.contains("SHARED WORK")
            && !decisions[0].summary.contains("Take this from the board"),
        "the row is Harness-owned ids only, never prompt or Work text: {}",
        decisions[0].summary
    );

    // The wake ended an idle episode, so that episode is on the record too —
    // one row for however many polls it took, not one row per poll.
    let ended: Vec<String> = store
        .current_team_run_events(&run_id)
        .expect("team run events")
        .into_iter()
        .filter(|event| {
            event.operation == "wake_idle_ended"
                && event.member_run_id.as_deref() == Some(member_id.as_str())
                && event.summary.ends_with("ClaimBoardWork")
        })
        .map(|event| event.summary)
        .collect();
    assert_eq!(ended.len(), 1, "one episode, one closing row: {ended:?}");
    assert!(
        ended[0].starts_with("idle episode ended after ") && !ended[0].contains("after 0 polls"),
        "the closing row carries what no other row can — how long the member \
         found nothing: {ended:?}"
    );
}
