use super::*;

fn run_team_run_json_with_env(
    home: &TempHome,
    project_id: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> serde_json::Value {
    let mut full = vec!["--project", project_id, "team-run"];
    full.extend_from_slice(args);
    let out = run_firm_with_env(home, home.base(), &full, env);
    assert!(
        out.status.success(),
        "team-run {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("team-run output JSON")
}

#[test]
fn list_and_status_return_explicit_dashboard_urls() {
    let home = TempHome::new("team-run-dashboard-urls");
    let project_id = init_project(&home, "alpha");
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Expose canonical dashboard links",
            "--member",
            "dashboard-worker:implementer:codex",
            "--json",
        ],
    );
    let run_id = created["team_run"]["id"]
        .as_str()
        .expect("TeamRun id")
        .to_string();
    let member_run_id = created["member_runs"]
        .as_array()
        .expect("member runs")
        .iter()
        .find(|member| member["name"] == "dashboard-worker")
        .and_then(|member| member["id"].as_str())
        .expect("dashboard worker MemberRun")
        .to_string();
    let space_id = current_space_id(&home);
    let base = "https://dashboard.example.test/workbench/";
    let run_url = format!(
        "https://dashboard.example.test/workbench/?space={space_id}&project={project_id}&surface=team&team={FIXTURE_TEAM_ID}"
    );

    let list = team_run_json(
        &home,
        &project_id,
        &["list", "--dashboard-base", base, "--json"],
    );
    let listed = list
        .as_array()
        .expect("run list")
        .iter()
        .find(|run| run["id"] == run_id)
        .expect("listed TeamRun");
    assert_eq!(listed["dashboard_url"], run_url);

    let status = run_team_run_json_with_env(
        &home,
        &project_id,
        &[
            "status",
            "--id",
            &run_id,
            "--dashboard-base",
            base,
            "--json",
        ],
        &[("FIRM_DASHBOARD_BASE", "https://ignored.example.test")],
    );
    assert_eq!(status["dashboard_url"], run_url);
    let worker = status["members"]
        .as_array()
        .expect("status members")
        .iter()
        .find(|member| member["member_run"]["id"] == member_run_id)
        .expect("status worker");
    assert_eq!(
        worker["dashboard_url"],
        format!("{run_url}&memberRun={member_run_id}")
    );

    let from_env = run_team_run_json_with_env(
        &home,
        &project_id,
        &["list", "--json"],
        &[("FIRM_DASHBOARD_BASE", "https://env.example.test")],
    );
    assert_eq!(
        from_env[0]["dashboard_url"],
        run_url.replace(
            "https://dashboard.example.test/workbench/",
            "https://env.example.test/"
        )
    );

    let without_base = run_team_run_json_with_env(
        &home,
        &project_id,
        &["status", "--id", &run_id, "--json"],
        &[("FIRM_DASHBOARD_BASE", "")],
    );
    assert!(without_base["dashboard_url"].is_null());
    assert!(without_base["members"]
        .as_array()
        .expect("members without base")
        .iter()
        .all(|member| member["dashboard_url"].is_null()));
}
