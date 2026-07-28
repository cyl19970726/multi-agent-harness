//! Integration coverage for ADR 0042 Company Store selection.
//!
//! These tests prove the first Company Store slice is not just a registry: once
//! a Company is current, `harness company ...` writes Company OS records to the
//! Company Store, while execution commands continue to use the selected project
//! store.

use std::path::Path;

mod harness_env;
use harness_env::{run_harness, run_harness_with_env, TempHome};

fn json_out(out: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout not JSON ({e}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn company_store_root(home: &TempHome, id: &str) -> std::path::PathBuf {
    home.harness_home().join("companies").join(id)
}

fn current_company_id(home: &TempHome, cwd: &Path) -> Option<String> {
    let out = run_harness(home, cwd, &["company", "current"]);
    assert!(out.status.success(), "company current failed: {out:?}");
    json_out(&out)["id"].as_str().map(str::to_string)
}

#[test]
fn company_init_materializes_registry_store_and_current_marker() {
    let home = TempHome::new("company-init");

    assert_eq!(current_company_id(&home, home.base()), None);

    let out = run_harness(
        &home,
        home.base(),
        &[
            "company",
            "init",
            "--id",
            "agent-company",
            "--name",
            "Agent Company",
        ],
    );
    assert!(out.status.success(), "company init failed: {out:?}");
    let created = json_out(&out);
    assert_eq!(created["id"], "agent-company");
    assert_eq!(created["name"], "Agent Company");
    assert_eq!(created["is_current"], true);
    assert_eq!(
        created["identity_boundary"], "company_store",
        "output must make the Company Store boundary explicit"
    );

    let store_root = company_store_root(&home, "agent-company");
    assert!(store_root.join("metadata.json").is_file());
    assert_eq!(
        std::fs::read_to_string(home.harness_home().join("ACTIVE_COMPANY"))
            .unwrap()
            .trim(),
        "agent-company"
    );

    let listed = json_out(&run_harness(&home, home.base(), &["company", "list"]));
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|company| company["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"agent-company"),
        "list missing company: {ids:?}"
    );
}

#[test]
fn active_company_routes_company_os_without_stealing_execution_store() {
    let home = TempHome::new("company-routing");
    let repo = home.home().join("multi-agent-harness");
    std::fs::create_dir_all(&repo).unwrap();

    let out = run_harness(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");
    let project_store = home
        .harness_home()
        .join("projects")
        .join("multi-agent-harness");

    let out = run_harness(
        &home,
        &repo,
        &[
            "company",
            "init",
            "--id",
            "main-company",
            "--name",
            "Main Company",
        ],
    );
    assert!(out.status.success(), "company init failed: {out:?}");
    let company_store = company_store_root(&home, "main-company");

    let out = run_harness(
        &home,
        &repo,
        &["--store-source", "company", "docs", "health"],
    );
    assert!(out.status.success(), "company docs health failed: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CompanyCurrent") && stderr.contains("company-context: id=main-company"),
        "company command should resolve through active Company Store, stderr: {stderr}"
    );

    let out = run_harness_with_env(
        &home,
        &repo,
        &[
            "company",
            "org",
            "create-human",
            "--id",
            "human-owner",
            "--display-name",
            "Human Owner",
            "--responsibility",
            "Company owner",
            "--permission",
            "company_os.admin",
            "--authority",
            "human-owner",
        ],
        &[("HARNESS_COMPANY_OS_TOKEN", "test-token")],
    );
    assert!(
        out.status.success(),
        "company org create-human failed: {out:?}"
    );
    assert!(
        company_store
            .join("company_os_human_members.jsonl")
            .is_file(),
        "Company OS actor should be written into the active Company Store"
    );
    assert!(
        !project_store
            .join("company_os_human_members.jsonl")
            .exists(),
        "active Company Store must not write Company OS rows into the project store"
    );

    let out = run_harness(&home, &repo, &["--store-source", "mission", "list"]);
    assert!(out.status.success(), "mission list failed: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("RegistryCurrent") && stderr.contains(project_store.to_str().unwrap()),
        "execution command should continue using project store, stderr: {stderr}"
    );

    let out = run_harness(
        &home,
        &repo,
        &[
            "--company",
            "main-company",
            "--store-source",
            "company",
            "docs",
            "health",
        ],
    );
    assert!(
        out.status.success(),
        "prefix --company global flag should work: {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CompanyFlag") && stderr.contains("company-context: id=main-company"),
        "prefix --company should route to Company Store, stderr: {stderr}"
    );
}

#[test]
fn stale_active_company_is_error_not_project_store_fallback() {
    let home = TempHome::new("company-stale-active");
    let repo = home.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let out = run_harness(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");

    std::fs::write(
        home.harness_home().join("ACTIVE_COMPANY"),
        "missing-company\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.harness_home().join("companies")).unwrap();
    std::fs::write(
        home.harness_home().join("companies").join("registry.json"),
        r#"{"format_version":1,"current_company_id":"missing-company","companies":[]}"#,
    )
    .unwrap();

    let out = run_harness(&home, &repo, &["company", "docs", "health"]);
    assert!(
        !out.status.success(),
        "stale active Company should not fall back to project store"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("active company is unknown: missing-company"),
        "stderr: {stderr}"
    );
}

#[test]
fn migrate_from_project_copies_only_company_os_ledgers() {
    let home = TempHome::new("company-migrate");
    let repo = home.home().join("multi-agent-harness");
    std::fs::create_dir_all(&repo).unwrap();

    let out = run_harness(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");
    let project_store = home
        .harness_home()
        .join("projects")
        .join("multi-agent-harness");

    let out = run_harness(
        &home,
        &repo,
        &[
            "mission",
            "create",
            "--id",
            "mission-project-only",
            "--title",
            "Project execution",
            "--objective",
            "Stay in project store",
        ],
    );
    assert!(out.status.success(), "mission create failed: {out:?}");
    assert!(project_store.join("missions.jsonl").is_file());

    let out = run_harness_with_env(
        &home,
        &repo,
        &[
            "company",
            "org",
            "create-human",
            "--id",
            "human-owner",
            "--display-name",
            "Human Owner",
            "--responsibility",
            "Company owner",
            "--permission",
            "company_os.admin",
            "--authority",
            "human-owner",
        ],
        &[("HARNESS_COMPANY_OS_TOKEN", "test-token")],
    );
    assert!(
        out.status.success(),
        "project compatibility company write failed: {out:?}"
    );
    assert!(project_store
        .join("company_os_human_members.jsonl")
        .is_file());

    let out = run_harness(
        &home,
        &repo,
        &[
            "company",
            "migrate-from-project",
            "--from-project",
            "multi-agent-harness",
            "--id",
            "main-company",
            "--name",
            "Main Company",
        ],
    );
    assert!(out.status.success(), "company migration failed: {out:?}");
    let migrated = json_out(&out);
    assert_eq!(migrated["ok"], true);
    assert_eq!(
        migrated["boundary"]["copied"], "company_os_*.jsonl only",
        "migration must declare its narrow copy boundary"
    );
    assert!(
        migrated["copied_records"].as_u64().unwrap() >= 1,
        "migration should copy Company OS rows: {migrated}"
    );

    let company_store = company_store_root(&home, "main-company");
    assert!(company_store
        .join("company_os_human_members.jsonl")
        .is_file());
    assert!(
        !company_store.join("missions.jsonl").exists(),
        "migration must not copy Mission/Wave execution ledgers into Company Store"
    );
}
