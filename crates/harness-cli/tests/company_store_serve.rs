//! Live HTTP coverage for ADR 0042 Company Store selection in `harness serve`.

mod harness_env;
use harness_env::{collect_sse_data, run_harness, run_harness_with_env, ServeHandle, TempHome};
use std::time::Duration;

#[test]
fn serve_company_compatibility_uses_project_binding_not_execution_space() {
    let home = TempHome::new("company-serve-project-compat");
    let repo = home.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let out = run_harness(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");
    let out = run_harness_with_env(
        &home,
        &repo,
        &[
            "company",
            "org",
            "create-human",
            "--id",
            "compat-human",
            "--display-name",
            "Compatibility Human",
            "--responsibility",
            "Prove compatibility routing",
            "--permission",
            "company_os.admin",
            "--authority",
            "compat-human",
        ],
        &[("HARNESS_COMPANY_OS_TOKEN", "test-token")],
    );
    assert!(out.status.success(), "compatibility write failed: {out:?}");

    let project_id = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(home.registry_path()).unwrap(),
    )
    .unwrap()["current_project_id"]
        .as_str()
        .unwrap()
        .to_string();
    let space_id = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(home.space_registry_path()).unwrap(),
    )
    .unwrap()["current_space_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(home
        .projects_dir()
        .join(&project_id)
        .join("company_os_human_members.jsonl")
        .is_file());
    assert!(
        !home
            .spaces_dir()
            .join(&space_id)
            .join("company_os_human_members.jsonl")
            .exists(),
        "Company compatibility truth must not enter the Execution Space"
    );

    let server = ServeHandle::spawn(&home, &repo, &[]);
    let (status, snapshot) = server.get_json(&format!(
        "/v1/company-os/snapshot?space={space_id}&project={project_id}"
    ));
    assert_eq!(status, 200);
    assert!(
        snapshot["result"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|actor| actor["id"] == "compat-human"),
        "HTTP compatibility read should use the Project Binding store: {snapshot}"
    );
}

#[test]
fn serve_lists_switches_and_routes_company_os_by_company_store() {
    let home = TempHome::new("company-serve");
    let repo = home.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let out = run_harness(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");

    let out = run_harness(
        &home,
        &repo,
        &[
            "company",
            "init",
            "--id",
            "company-a",
            "--name",
            "Company A",
        ],
    );
    assert!(out.status.success(), "company-a init failed: {out:?}");
    let out = run_harness_with_env(
        &home,
        &repo,
        &[
            "--company",
            "company-a",
            "company",
            "org",
            "create-human",
            "--id",
            "human-a",
            "--display-name",
            "Human A",
            "--responsibility",
            "Owns company A",
            "--permission",
            "company_os.admin",
            "--authority",
            "human-a",
        ],
        &[("HARNESS_COMPANY_OS_TOKEN", "test-token")],
    );
    assert!(out.status.success(), "company-a write failed: {out:?}");

    let out = run_harness(
        &home,
        &repo,
        &[
            "company",
            "init",
            "--id",
            "company-b",
            "--name",
            "Company B",
        ],
    );
    assert!(out.status.success(), "company-b init failed: {out:?}");

    let server = ServeHandle::spawn(&home, &repo, &[]);

    let (status, companies) = server.get_json("/v1/companies");
    assert_eq!(status, 200);
    assert_eq!(companies["current"], "company-b");
    let ids: Vec<&str> = companies["companies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|company| company["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"company-a") && ids.contains(&"company-b"));

    let (status, explicit_a) = server.get_json("/v1/company-os/snapshot?company=company-a");
    assert_eq!(status, 200);
    assert!(
        explicit_a["result"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|actor| actor["id"] == "human-a"),
        "explicit company snapshot should read company-a: {explicit_a}"
    );

    let (status, blended_a) = server.get_json("/v1/snapshot?company=company-a");
    assert_eq!(status, 200);
    assert!(
        blended_a["company_os"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|actor| actor["id"] == "human-a"),
        "dashboard snapshot should blend company_os from the explicit company store: {blended_a}"
    );
    assert!(
        blended_a["teams"].is_array(),
        "dashboard snapshot should keep execution/project keys while overriding company_os: {blended_a}"
    );

    let (status, missing) = server.get_json("/v1/company-os/snapshot?company=missing-company");
    assert_eq!(status, 404);
    assert!(
        missing["error"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown company"),
        "unknown company should not fall back to project store: {missing}"
    );
    let (status, missing_blended) = server.get_json("/v1/snapshot?company=missing-company");
    assert_eq!(status, 404);
    assert!(
        missing_blended["error"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown company"),
        "unknown company dashboard snapshot should not fall back to project store: {missing_blended}"
    );

    let (status, switched) = server.post_json(
        "/v1/companies/switch",
        &serde_json::json!({
            "company": "company-a"
        }),
    );
    assert_eq!(status, 200);
    assert_eq!(switched["result"]["current"], "company-a");

    let (status, current) = server.get_json("/v1/companies/current");
    assert_eq!(status, 200);
    assert_eq!(current["current"], "company-a");

    let (status, active_snapshot) = server.get_json("/v1/company-os/snapshot");
    assert_eq!(status, 200);
    assert!(
        active_snapshot["result"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|actor| actor["id"] == "human-a"),
        "active company snapshot should read switched company-a: {active_snapshot}"
    );

    let (status, active_blended) = server.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert!(
        active_blended["company_os"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|actor| actor["id"] == "human-a"),
        "dashboard snapshot should blend company_os from active company store: {active_blended}"
    );
}

#[test]
fn external_company_write_invalidates_only_subscribers_selecting_that_company() {
    let home = TempHome::new("company-sse-invalidation");
    let repo = home.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let out = run_harness(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");

    for (id, name) in [("company-a", "Company A"), ("company-b", "Company B")] {
        let out = run_harness(
            &home,
            &repo,
            &["company", "init", "--id", id, "--name", name],
        );
        assert!(out.status.success(), "{id} init failed: {out:?}");
    }
    let project_id = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(home.registry_path()).unwrap(),
    )
    .unwrap()["current_project_id"]
        .as_str()
        .unwrap()
        .to_string();
    let space_id = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(home.space_registry_path()).unwrap(),
    )
    .unwrap()["current_space_id"]
        .as_str()
        .unwrap()
        .to_string();

    let server = ServeHandle::spawn(&home, &repo, &[]);
    let mut company_a = server.open_sse(&format!(
        "?space={space_id}&project={project_id}&company=company-a"
    ));
    let mut company_b = server.open_sse(&format!(
        "?space={space_id}&project={project_id}&company=company-b"
    ));

    // A separate CLI writes native Company Store truth after both streams are
    // connected. The Company row never enters the Execution Space.
    let out = run_harness_with_env(
        &home,
        &repo,
        &[
            "--company",
            "company-a",
            "company",
            "org",
            "create-human",
            "--id",
            "human-a-live",
            "--display-name",
            "Human A Live",
            "--responsibility",
            "Prove scoped Company convergence",
            "--permission",
            "company_os.admin",
            "--authority",
            "human-a-live",
        ],
        &[("HARNESS_COMPANY_OS_TOKEN", "test-token")],
    );
    assert!(
        out.status.success(),
        "company-a external write failed: {out:?}"
    );

    let frames_a = collect_sse_data(&mut company_a, Duration::from_secs(6), 1);
    let invalidation = frames_a
        .iter()
        .find(|frame| frame["ledger"] == "company_os_human_members.jsonl")
        .unwrap_or_else(|| panic!("company A stream missed invalidation: {frames_a:?}"));
    assert_eq!(invalidation["scope"], "company");
    assert_eq!(invalidation["scope_id"], "company-a");
    assert_eq!(invalidation["reason"], "append");

    let frames_b = collect_sse_data(&mut company_b, Duration::from_millis(500), 1);
    assert!(
        frames_b
            .iter()
            .all(|frame| frame["scope_id"] != "company-a"),
        "company A invalidation leaked to company B: {frames_b:?}"
    );

    let (status, snapshot) = server.get_json(&format!(
        "/v1/snapshot?space={space_id}&project={project_id}&company=company-a"
    ));
    assert_eq!(status, 200);
    assert!(
        snapshot["company_os"]["actors"]
            .as_array()
            .expect("Company actors")
            .iter()
            .any(|actor| actor["id"] == "human-a-live"),
        "Company snapshot did not converge: {snapshot}"
    );
    assert!(
        !home
            .spaces_dir()
            .join(&space_id)
            .join("company_os_human_members.jsonl")
            .exists(),
        "Company truth must not be copied into the Execution Space"
    );
}
