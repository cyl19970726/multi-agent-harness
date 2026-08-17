//! INACTIVE HISTORICAL (DOC-108 Stage B): the `harness company` registry,
//! routing, and migration commands this file exercised are retired; every
//! subcommand now fails with an explicit retired error. The successor
//! acceptance is the Stage A export/verify contract in
//! `tests/legacy_company_os.rs` (`harness legacy-company-os export|verify`).
//! Kept source-only per the inactive-historical convention.
#![cfg(any())]

//! Integration coverage for ADR 0042 Company Store selection.
//!
//! These tests prove the first Company Store slice is not just a registry: once
//! a Company is current, `harness company ...` writes Company OS records to the
//! Company Store, while execution commands continue to use the selected project
//! store.

use std::path::Path;

mod firm_env;
use firm_env::{run_firm, run_firm_with_env, TempHome};

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
    home.firm_home().join("companies").join(id)
}

fn current_company_id(home: &TempHome, cwd: &Path) -> Option<String> {
    let out = run_firm(home, cwd, &["company", "current"]);
    assert!(out.status.success(), "company current failed: {out:?}");
    json_out(&out)["id"].as_str().map(str::to_string)
}

#[test]
fn company_init_materializes_registry_store_and_current_marker() {
    let home = TempHome::new("company-init");

    assert_eq!(current_company_id(&home, home.base()), None);

    let out = run_firm(
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
        std::fs::read_to_string(home.firm_home().join("ACTIVE_COMPANY"))
            .unwrap()
            .trim(),
        "agent-company"
    );

    let listed = json_out(&run_firm(&home, home.base(), &["company", "list"]));
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

    let out = run_firm(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");
    let project_store = home
        .firm_home()
        .join("projects")
        .join("multi-agent-harness");
    let execution_store = home
        .firm_home()
        .join("execution-spaces")
        .join("multi-agent-harness");

    let out = run_firm(
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

    let out = run_firm(
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

    let out = run_firm_with_env(
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
        &[("FIRM_COMPANY_OS_TOKEN", "test-token")],
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

    let out = run_firm(&home, &repo, &["--store-source", "mission", "list"]);
    assert!(out.status.success(), "mission list failed: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("SpaceCurrent") && stderr.contains(execution_store.to_str().unwrap()),
        "execution command should use the active Execution Space, stderr: {stderr}"
    );

    let out = run_firm(
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
fn firm_company_is_canonical_and_harness_company_is_a_fallback() {
    let home = TempHome::new("company-env-precedence");
    let repo = home.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    for (id, name) in [("company-a", "Company A"), ("company-b", "Company B")] {
        let out = run_firm(
            &home,
            &repo,
            &["company", "init", "--id", id, "--name", name],
        );
        assert!(out.status.success(), "company init failed: {out:?}");
    }

    let canonical = run_firm_with_env(
        &home,
        &repo,
        &["--store-source", "company", "docs", "health"],
        &[
            ("FIRM_COMPANY", "company-a"),
            ("HARNESS_COMPANY", "company-b"),
        ],
    );
    assert!(
        canonical.status.success(),
        "canonical selection failed: {canonical:?}"
    );
    let canonical_stderr = String::from_utf8_lossy(&canonical.stderr);
    assert!(
        canonical_stderr.contains("company-context: id=company-a"),
        "stderr: {canonical_stderr}"
    );
    assert!(!canonical_stderr.contains("HARNESS_COMPANY is deprecated"));

    let alias = run_firm_with_env(
        &home,
        &repo,
        &["--store-source", "company", "docs", "health"],
        &[("HARNESS_COMPANY", "company-a")],
    );
    assert!(alias.status.success(), "alias selection failed: {alias:?}");
    let alias_stderr = String::from_utf8_lossy(&alias.stderr);
    assert!(
        alias_stderr.contains("company-context: id=company-a"),
        "stderr: {alias_stderr}"
    );
    assert!(
        alias_stderr.contains("HARNESS_COMPANY is deprecated; prefer `FIRM_COMPANY`"),
        "stderr: {alias_stderr}"
    );
}

#[test]
fn stale_active_company_is_error_not_project_store_fallback() {
    let home = TempHome::new("company-stale-active");
    let repo = home.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let out = run_firm(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");

    std::fs::write(home.firm_home().join("ACTIVE_COMPANY"), "missing-company\n").unwrap();
    std::fs::create_dir_all(home.firm_home().join("companies")).unwrap();
    std::fs::write(
        home.firm_home().join("companies").join("registry.json"),
        r#"{"format_version":1,"current_company_id":"missing-company","companies":[]}"#,
    )
    .unwrap();

    let out = run_firm(&home, &repo, &["company", "docs", "health"]);
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

    let out = run_firm(&home, &repo, &["init"]);
    assert!(out.status.success(), "project init failed: {out:?}");
    let project_store = home
        .firm_home()
        .join("projects")
        .join("multi-agent-harness");

    let out = run_firm(
        &home,
        &repo,
        &[
            "--store",
            project_store.to_str().unwrap(),
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
    assert!(
        out.status.success(),
        "legacy project-store mission seed failed: {out:?}"
    );
    assert!(project_store.join("missions.jsonl").is_file());

    let out = run_firm_with_env(
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
        &[("FIRM_COMPANY_OS_TOKEN", "test-token")],
    );
    assert!(
        out.status.success(),
        "project compatibility company write failed: {out:?}"
    );
    assert!(project_store
        .join("company_os_human_members.jsonl")
        .is_file());
    for retired in [
        "company_os_work_items.jsonl",
        "company_os_assignments.jsonl",
        "company_os_work_cutover_fences.jsonl",
    ] {
        std::fs::write(
            project_store.join(retired),
            format!("{{\"id\":\"retired-{retired}\"}}\n"),
        )
        .unwrap();
    }

    let out = run_firm(
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
        migrated["boundary"]["copied"], "active Company OS ledger allowlist only",
        "migration must declare its narrow copy boundary"
    );
    assert_eq!(
        migrated["boundary"]["retired_workitem_history_migrated"],
        false
    );
    assert!(
        migrated["copied_records"].as_u64().unwrap() >= 1,
        "migration should copy Company OS rows: {migrated}"
    );
    assert_eq!(migrated["verification"]["status"], "verified");
    assert_eq!(migrated["verification"]["missing_source_records"], 0);

    let company_store = company_store_root(&home, "main-company");
    assert!(company_store
        .join("company_os_human_members.jsonl")
        .is_file());
    for retired in [
        "company_os_work_items.jsonl",
        "company_os_assignments.jsonl",
        "company_os_work_cutover_fences.jsonl",
    ] {
        assert!(
            !company_store.join(retired).exists(),
            "retired ledger must not be copied: {retired}"
        );
    }
    assert!(company_store
        .join("company_store_migrations.jsonl")
        .is_file());
    assert!(project_store
        .join("COMPANY_OS_MIGRATED_TO_COMPANY.json")
        .is_file());
    assert!(
        !company_store.join("missions.jsonl").exists(),
        "migration must not copy Mission/Wave execution ledgers into Company Store"
    );

    let marker: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(project_store.join("COMPANY_OS_MIGRATED_TO_COMPANY.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(marker["status"], "migrated_and_verified");
    assert_eq!(marker["recommended_access"], "read_only_audit");
    assert_eq!(marker["read_only_enforced"], false);

    let migrations = json_out(&run_firm(
        &home,
        &repo,
        &["--company", "main-company", "company", "migrations"],
    ));
    assert_eq!(migrations["records"].as_array().unwrap().len(), 1);

    let target_ledger = company_store.join("company_os_human_members.jsonl");
    let mut target_text = std::fs::read_to_string(&target_ledger).unwrap();
    target_text.push_str("{\"id\":\"target-newer-record\"}\n");
    std::fs::write(&target_ledger, target_text).unwrap();
    let verify_only = run_firm(
        &home,
        &repo,
        &[
            "company",
            "migrate-from-project",
            "--from-project",
            "multi-agent-harness",
            "--id",
            "main-company",
            "--verify-only",
        ],
    );
    assert!(
        verify_only.status.success(),
        "verify-only should accept a destination superset: {verify_only:?}"
    );
    let verified = json_out(&verify_only);
    assert_eq!(verified["mode"], "verify_only");
    assert_eq!(verified["copied_records"], 0);
    assert_eq!(verified["verification"]["status"], "verified");
    assert!(
        verified["verification"]["target_records"].as_u64().unwrap()
            > verified["verification"]["source_records"].as_u64().unwrap()
    );

    let migrations = json_out(&run_firm(
        &home,
        &repo,
        &["--company", "main-company", "company", "migrations"],
    ));
    assert_eq!(migrations["records"].as_array().unwrap().len(), 2);

    std::fs::write(&target_ledger, "{\"id\":\"target-only\"}\n").unwrap();
    let failed_verify = run_firm(
        &home,
        &repo,
        &[
            "company",
            "migrate-from-project",
            "--from-project",
            "multi-agent-harness",
            "--id",
            "main-company",
            "--verify-only",
        ],
    );
    assert!(
        !failed_verify.status.success(),
        "verify-only must fail when exact source rows are missing"
    );
    assert!(
        String::from_utf8_lossy(&failed_verify.stderr).contains("missing 1 exact source record"),
        "unexpected verification error: {}",
        String::from_utf8_lossy(&failed_verify.stderr)
    );
}
