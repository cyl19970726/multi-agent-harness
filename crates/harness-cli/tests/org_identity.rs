//! ADR 0051 durable AgentMember identity and explicit root Lead bootstrap.

mod harness_env;

use harness_env::{current_project_id, run_harness, run_harness_with_env, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    current_project_id(home)
}

fn run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_harness_with_env(home, home.base(), &full, &[]);
    assert!(
        out.status.success(),
        "harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("harness {args:?} stdout was not JSON ({error})"))
}

fn run_err(home: &TempHome, project_id: &str, args: &[&str]) -> String {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_harness_with_env(home, home.base(), &full, &[]);
    assert!(
        !out.status.success(),
        "harness {args:?} unexpectedly passed"
    );
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn root_lead_bootstrap_separates_durable_identity_from_runtime_and_cuts_over_authority() {
    let home = TempHome::new("org-identity-bootstrap");
    let project_id = init_project(&home, "identity");

    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-root",
            "--name",
            "Root Team",
            "--description",
            "Compatibility root before durable Lead bootstrap",
            "--lead",
            "host",
        ],
    );
    let before = run_err(&home, &project_id, &["org", "cutover-audit"]);
    assert!(before.contains("Host cutover"), "stderr: {before}");

    let bootstrapped = run_json(
        &home,
        &project_id,
        &[
            "org",
            "bootstrap-lead",
            "--team",
            "team-root",
            "--id",
            "agent-lead",
            "--name",
            "Lead",
            "--description",
            "Durable root Lead",
            "--role",
            "lead",
            "--provider-profile",
            "kimi/qwen3.8-max",
            "--model",
            "qwen/qwen3.8-max",
            "--project-binding",
            &project_id,
            "--business-access-ceiling",
            "company_os.read",
        ],
    );
    assert_eq!(bootstrapped["member"]["status"], "active");
    assert_eq!(
        bootstrapped["member"]["native_session"],
        serde_json::Value::Null
    );
    assert_eq!(bootstrapped["team"]["owner_agent_id"], "agent-lead");
    assert_eq!(bootstrapped["team"]["host_member_id"], "agent-lead");

    let host = run_json(&home, &project_id, &["org", "host", "--team", "team-root"]);
    assert_eq!(host["host_member_id"], "agent-lead");
    assert_eq!(host["source"], "explicit");

    let audit = run_json(&home, &project_id, &["org", "cutover-audit"]);
    assert_eq!(audit["ready"], true);
    assert_eq!(audit["authority"], "host_member_id");
    assert_eq!(audit["team_count"], 1);
    assert_eq!(audit["durable_member_count"], 1);

    // Durable identities, not only compatibility runtime rows, can populate
    // recursive Team topology without inventing a MemberRun or Session.
    run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "create",
            "--id",
            "agent-cto",
            "--name",
            "CTO",
            "--description",
            "Durable child Host",
            "--role",
            "cto",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "add-member",
            "--id",
            "team-root",
            "--member",
            "agent-cto",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-child",
            "--name",
            "Child Team",
            "--description",
            "Hosted by the durable CTO identity",
            "--lead",
            "agent-cto",
            "--parent-team",
            "team-root",
            "--host-member",
            "agent-cto",
        ],
    );
    let recursive_audit = run_json(&home, &project_id, &["org", "cutover-audit"]);
    assert_eq!(recursive_audit["team_count"], 2);
    assert_eq!(recursive_audit["durable_member_count"], 2);
}

#[test]
fn compatibility_registry_convergence_is_explicit_and_idempotent() {
    let home = TempHome::new("org-identity-converge");
    let project_id = init_project(&home, "converge");
    run_json(
        &home,
        &project_id,
        &[
            "agent",
            "create",
            "--id",
            "agent-cto",
            "--name",
            "CTO",
            "--description",
            "Compatibility runtime registry row",
            "--role",
            "cto",
            "--provider",
            "kimi",
            "--model",
            "qwen/qwen3.8-max",
        ],
    );
    let first = run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "converge",
            "--id",
            "agent-cto",
            "--project-binding",
            &project_id,
        ],
    );
    let second = run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "converge",
            "--id",
            "agent-cto",
            "--project-binding",
            &project_id,
        ],
    );
    assert_eq!(first, second);
    assert_eq!(first["provider_profile"], "kimi");
    assert!(first.get("provider_runtime_id").is_none());
    assert!(first.get("native_session").is_none());

    let members = run_json(&home, &project_id, &["org", "member", "list"]);
    assert_eq!(members.as_array().unwrap().len(), 1);
}
