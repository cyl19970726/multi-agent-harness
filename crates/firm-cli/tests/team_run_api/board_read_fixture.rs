use super::*;

// ---------------------------------------------------------------------------
// Decision-shaped board reads (issue #305): `work list --brief`, `work list
// --since`, and `team-run board-summary`. All three read the same
// authoritative store as `work list`'s full JSON; they only change the
// projection.
// ---------------------------------------------------------------------------

/// A TeamRun with three members -- alice and bob each own Work, charlie stays
/// idle -- and six Works spanning the Work lifecycle axes. Every board-read test
/// seeds its own fixture so the three read paths stay independent.
pub struct BoardReadFixture {
    pub home: TempHome,
    pub project_id: String,
    pub run_id: String,
    pub alice_agent_member_id: String,
    pub bob_agent_member_id: String,
    #[allow(dead_code)] // read by the board-summary test only
    pub charlie_id: String,
    pub work_open_id: String,
    pub work_in_progress_id: String,
    pub work_review_id: String,
    pub work_blocked_id: String,
    pub work_done_id: String,
    pub work_cancelled_id: String,
}

/// Create one Work and return its id. `owner` is the owning ProviderRuntimeProjection id, or
/// `None` to leave it unassigned in the shared Ready Pool.
pub fn create_fixture_work(
    home: &TempHome,
    project_id: &str,
    run_id: &str,
    title: &str,
    owner: Option<&str>,
) -> String {
    let args = vec![
        "work",
        "create",
        "--team-run-id",
        run_id,
        "--title",
        title,
        "--completion-criteria",
        "Done when the fixture says so",
    ];
    let created = team_run_json(home, project_id, &args);
    let work_id = created["id"].as_str().expect("Work id");
    if let Some(owner) = owner {
        firm_env::work_execution::assign_work_for_member_run(
            home, project_id, work_id, owner, true,
        );
        firm_env::provider_received_work::record_provider_received_work(
            home,
            project_id,
            work_id,
            &format!("board-fixture-{work_id}"),
        );
    }
    work_id.to_string()
}

pub fn seed_board_read_fixture(tag: &str) -> BoardReadFixture {
    let home = TempHome::new(tag);
    let project_id = init_project(&home, "alpha");

    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--objective",
            "Exercise decision-shaped board reads",
            "--member",
            "alice:implementer:codex",
            "--member",
            "bob:implementer:codex",
            "--member",
            "charlie:implementer:codex",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    let members = status["members"].as_array().expect("members").clone();
    let member_identity = |name: &str| -> (String, String) {
        let member = &members
            .iter()
            .find(|entry| entry["member_run"]["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("member {name} not found: {members:?}"))["member_run"];
        (
            member["id"].as_str().expect("MemberRun id").to_string(),
            member["agent_member_id"]
                .as_str()
                .expect("AgentMember id")
                .to_string(),
        )
    };
    let (alice_id, alice_agent_member_id) = member_identity("alice");
    let (bob_id, bob_agent_member_id) = member_identity("bob");
    let (charlie_id, _) = member_identity("charlie");

    // Work A: created unassigned, never claimed -- stays `open`. Title is
    // deliberately >60 chars to exercise --brief's title truncation.
    let long_title =
        "Open unassigned Work whose title runs well past the sixty character brief cutoff";
    let work_open_id = create_fixture_work(&home, &project_id, &run_id, long_title, None);

    // Work D: alice owns it, starts it, and the Host blocks it -- `blocked`.
    // Driven to completion before Work B starts so alice never holds two
    // simultaneously `in_progress` Works (the store rejects that as
    // MEMBER_BUSY).
    let work_blocked_id =
        create_fixture_work(&home, &project_id, &run_id, "Blocked Work", Some(&alice_id));
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &alice_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_blocked_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &alice_id,
        ],
    );
    team_run_json(
        &home,
        &project_id,
        &[
            "work",
            "block",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_blocked_id,
            "--expected-version",
            "3",
            "--reason",
            "Waiting on an external dependency",
        ],
    );

    // Work B: alice owns it and starts it -- stays `in_progress`.
    let work_in_progress_id = create_fixture_work(
        &home,
        &project_id,
        &run_id,
        "In-progress Work",
        Some(&alice_id),
    );
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &alice_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_in_progress_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &alice_id,
        ],
    );

    // Work C: bob owns it, starts it, and submits -- `review`. Driven to
    // completion before Work E for the same MEMBER_BUSY reason as above.
    let work_review_id =
        create_fixture_work(&home, &project_id, &run_id, "Review Work", Some(&bob_id));
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_review_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &bob_id,
        ],
    );
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_review_id,
            "--expected-version",
            "3",
            "--member-run-id",
            &bob_id,
            "--result",
            "Submitted for Host review",
        ],
    );

    // Work E: bob owns it, starts, submits, and the Host accepts -- `done`.
    let work_done_id = create_fixture_work(&home, &project_id, &run_id, "Done Work", Some(&bob_id));
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_done_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &bob_id,
        ],
    );
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_done_id,
            "--expected-version",
            "3",
            "--member-run-id",
            &bob_id,
            "--result",
            "Done and submitted",
        ],
    );
    team_run_json(
        &home,
        &project_id,
        &[
            "work",
            "accept",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_done_id,
            "--expected-version",
            "4",
        ],
    );

    // Work F: created unassigned, then the Host cancels it -- `cancelled`.
    let work_cancelled_id =
        create_fixture_work(&home, &project_id, &run_id, "Cancelled Work", None);
    team_run_json(
        &home,
        &project_id,
        &[
            "work",
            "cancel",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_cancelled_id,
            "--expected-version",
            "1",
            "--reason",
            "No longer needed",
        ],
    );

    BoardReadFixture {
        home,
        project_id,
        run_id,
        alice_agent_member_id,
        bob_agent_member_id,
        charlie_id,
        work_open_id,
        work_in_progress_id,
        work_review_id,
        work_blocked_id,
        work_done_id,
        work_cancelled_id,
    }
}
