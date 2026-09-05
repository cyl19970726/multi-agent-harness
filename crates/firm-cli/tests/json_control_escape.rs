//! Regression coverage for #851: every JSON surface of the CLI must emit
//! strict-parser-valid JSON (RFC 8259) even when Host- or member-authored
//! Markdown fields carry raw control characters (tab, form feed, newline).

mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm,
    run_firm_with_env, TempHome,
};

const HOST_ID: &str = "agent-json-escape-host";
const TEAM_ID: &str = "team-json-escape-fixture";

fn ok(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn bootstrap(home: &TempHome) -> (String, String) {
    let root = home.base().to_path_buf();
    ok(&run_firm(home, &root, &["init"]), "firm init");
    let project_id = current_project_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    ok(&node, "node init");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();
    ok(
        &run_firm(
            home,
            &root,
            &[
                "node",
                "project",
                "register",
                "--node-id",
                &node_id,
                "--project-binding-id",
                &project_id,
            ],
        ),
        "node project register",
    );
    ok(
        &create_canonical_agent_member(
            home,
            &root,
            &project_id,
            HOST_ID,
            "json-escape-host",
            "host",
            "codex",
            &[],
        ),
        "host member create",
    );
    ok(
        &run_firm(
            home,
            &root,
            &[
                "team",
                "create",
                "--id",
                TEAM_ID,
                "--name",
                "JSON escape team",
                "--description",
                "Flat JSON escape test team",
                "--host-agent-id",
                HOST_ID,
                "--node-id",
                &node_id,
                "--member",
                HOST_ID,
            ],
        ),
        "team create",
    );
    let created = run_firm(
        home,
        &root,
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            TEAM_ID,
            "--objective",
            "JSON escape fixture",
            "--host-runtime-mode",
            "external_interactive",
            "--host-surface",
            "cli",
            "--host-thread-id",
            "json-escape-thread",
            "--member",
            "agent-json-escape-host:host:codex/external_interactive",
        ],
    );
    ok(&created, "team-run create");
    let run_id = String::from_utf8_lossy(&created.stdout).trim().to_string();
    (project_id, run_id)
}

fn strict_parse(output: &std::process::Output, surface: &str) -> serde_json::Value {
    match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut context = String::new();
            for (index, line) in text.lines().enumerate() {
                let number = index + 1;
                if number + 3 >= error.line() && number <= error.line() + 1 {
                    context.push_str(&format!("\n{:>4}: {line}", number));
                }
            }
            panic!("{surface} is not strict-parser valid: {error}{context}");
        }
    }
}

#[test]
fn work_show_and_list_stay_strict_json_with_control_characters() {
    let home = TempHome::new("json-control-escape");
    let (project_id, run_id) = bootstrap(&home);
    let title = "tab\ttitle";
    let context = "first line\nsecond line\ttabbed\x0Cform feed end";
    let criteria = "raw\x07bell and\x1Bescape survive strict JSON";
    let created = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            title,
            "--context",
            context,
            "--completion-criteria",
            criteria,
        ],
        &[],
    );
    ok(&created, "work create with control characters");
    let created: serde_json::Value =
        serde_json::from_slice(&created.stdout).expect("created Work JSON");
    let work_id = created["work"]["id"]
        .as_str()
        .or_else(|| created["work"]["work_id"].as_str())
        .or_else(|| created["id"].as_str())
        .expect("created Work id")
        .to_string();

    let show = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "show",
            "--work-id",
            &work_id,
        ],
        &[],
    );
    ok(&show, "team-run work show");
    let shown = strict_parse(&show, "team-run work show");
    assert_eq!(shown["work"]["title"], title);
    assert_eq!(shown["work"]["context_markdown"], context);
    assert_eq!(shown["work"]["completion_criteria_markdown"], criteria);

    let list = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "list",
            "--team-run-id",
            &run_id,
            "--json",
        ],
        &[],
    );
    ok(&list, "team-run work list --json");
    strict_parse(&list, "team-run work list --json");

    // `team-run host-inbox --json` and `team-run inbox --json` serialize
    // through the same `print_json` helper (team_run_cli.rs "host-inbox" /
    // "inbox" arms), so the message-body path is not a separate emitter and
    // needs no distinct live coverage; seeding a message there requires a
    // live NodeDaemon, which this fixture deliberately avoids.
}

/// Same strict-parser round-trip through a submitted Result report whose
/// member-authored summary carries the same raw control characters.
#[test]
fn work_show_stays_strict_json_with_a_control_character_result_summary() {
    let home = TempHome::new("json-control-escape-summary");
    let (project_id, run_id) = bootstrap(&home);
    let space_id = current_space_id(&home);
    let created = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            "summary fixture",
            "--completion-criteria",
            "submitted",
        ],
        &[],
    );
    ok(&created, "work create");
    let created: serde_json::Value =
        serde_json::from_slice(&created.stdout).expect("created Work JSON");
    let work_id = created["work"]["id"]
        .as_str()
        .or_else(|| created["id"].as_str())
        .expect("Work id")
        .to_string();
    let member_run_id = {
        let store = harness_store::HarnessStore::new(home.spaces_dir().join(&space_id));
        store
            .trust_member_runs(&space_id)
            .expect("member runs")
            .into_iter()
            .find(|run| run.team_run_id == run_id)
            .expect("host member run")
            .id
    };
    firm_env::work_execution::assign_work_for_member_run(
        &home,
        &space_id,
        &work_id,
        &member_run_id,
        true,
    );
    firm_env::provider_received_work::record_provider_received_work(
        &home,
        &space_id,
        &work_id,
        "json-escape-summary",
    );
    let member_env = [
        ("FIRM_MEMBER_RUN_ID", member_run_id.as_str()),
        ("FIRM_TEAM_RUN_ID", run_id.as_str()),
    ];
    let start = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--member-run-id",
            &member_run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "2",
        ],
        &member_env,
    );
    ok(&start, "work start");
    let summary = "result line one
result line two	with tabform feed end";
    let submit = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--member-run-id",
            &member_run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "3",
            "--result",
            summary,
            "--report-only",
        ],
        &member_env,
    );
    ok(&submit, "work submit");
    let show = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "show",
            "--work-id",
            &work_id,
        ],
        &[],
    );
    ok(&show, "team-run work show with result summary");
    let shown = strict_parse(&show, "team-run work show with result summary");
    let shown_text = serde_json::to_string(&shown).expect("show JSON");
    assert!(
        shown_text.contains("result line one"),
        "the control-character summary is visible: {shown_text}"
    );
}
