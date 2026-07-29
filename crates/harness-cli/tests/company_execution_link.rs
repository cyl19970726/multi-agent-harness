//! End-to-end acceptance for the governed StandingAgent -> AgentMember link.
//!
//! The Company CLI resolves the Company Store and returns before the global
//! `--space` selector is consumed, so AgentMember truth is only reachable through
//! an explicit `--execution-space` selector (ADR 0042). These tests pin that
//! cross-store contract at the real CLI boundary rather than in unit code:
//! the space must be named, the AgentMember must exist inside it, equal ids never
//! bind implicitly, and every other StandingAgent field survives the re-append.

mod harness_env;
use harness_env::{current_project_id, run_harness, run_harness_with_env, TempHome};

const COMPANY_OS_TEST_TOKEN: &str = "company-execution-link-test-capability";

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

fn run_raw(home: &TempHome, project_id: &str, args: &[&str]) -> std::process::Output {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    run_harness_with_env(
        home,
        home.base(),
        &full,
        &[("HARNESS_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
    )
}

fn run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let out = run_raw(home, project_id, args);
    assert!(
        out.status.success(),
        "harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("harness {args:?} stdout was not JSON ({error})"))
}

fn run_failure(home: &TempHome, project_id: &str, args: &[&str]) -> String {
    let out = run_raw(home, project_id, args);
    assert!(
        !out.status.success(),
        "harness {args:?} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn standing_agent(home: &TempHome, project_id: &str, actor_id: &str) -> serde_json::Value {
    let listed = run_json(
        home,
        project_id,
        &[
            "company", "org", "actor", "show", "--kind", "agent", "--id", actor_id,
        ],
    );
    listed["result"]["actor"].clone()
}

/// Create one Human authority, one AgentMember in the Execution Space, and one
/// StandingAgent in the Company Store that is deliberately left unlinked.
fn seed(home: &TempHome, project_id: &str) {
    run_json(
        home,
        project_id,
        &[
            "agent",
            "create",
            "--id",
            "agent-wcw-ops",
            "--name",
            "WcwOps",
            "--role",
            "operations",
            "--provider",
            "codex",
        ],
    );
    run_json(
        home,
        project_id,
        &[
            "agent",
            "create",
            "--id",
            "agent-wcw-other",
            "--name",
            "WcwOther",
            "--role",
            "operations",
            "--provider",
            "kimi",
        ],
    );
    run_json(
        home,
        project_id,
        &[
            "company",
            "org",
            "actor",
            "create-human",
            "--id",
            "human-wcw-owner",
            "--name",
            "Wcw Owner",
            "--responsibility",
            "Final company authority",
        ],
    );
    run_json(
        home,
        project_id,
        &[
            "company",
            "org",
            "create-agent",
            "--authority",
            "human-wcw-owner",
            "--id",
            "agent-wcw-ops",
            "--display-name",
            "Wcw Operations",
            "--role",
            "operations",
            "--responsibility",
            "Own merchant operations",
            "--capability",
            "ops.review",
            "--tool",
            "harness",
            "--skill",
            "company-org-operator",
            "--capacity",
            "3",
        ],
    );
}

#[test]
fn explicit_link_requires_a_named_execution_space_and_an_existing_agent_member() {
    let home = TempHome::new("company-execution-link-validation");
    let project_id = init_project(&home, "wcw");
    seed(&home, &project_id);

    // The Standing Agent starts unlinked even though an identically named
    // AgentMember exists: equal ids never bind on their own.
    let before = standing_agent(&home, &project_id, "agent-wcw-ops");
    assert!(
        before["execution_agent_member_ref"].is_null(),
        "same-id AgentMember must not imply a link: {before}"
    );

    // The Execution Space selector is mandatory: the Company CLI never guesses
    // which store owns AgentMember truth.
    let missing_space = run_failure(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
        ],
    );
    assert!(
        missing_space.contains("--execution-space is required"),
        "expected a required-selector error, got: {missing_space}"
    );

    let unknown_space = run_failure(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            "space-that-does-not-exist",
        ],
    );
    assert!(
        unknown_space.contains("unknown execution space: space-that-does-not-exist"),
        "expected an unknown-space error, got: {unknown_space}"
    );

    // A typo in the AgentMember id must fail loudly against the named space
    // instead of silently persisting a dangling reference.
    let unknown_member = run_failure(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-typo",
            "--execution-space",
            &project_id,
        ],
    );
    assert!(
        unknown_member.contains("AgentMember not found in execution space"),
        "expected a member-validation error, got: {unknown_member}"
    );
    assert!(
        unknown_member.contains("agent-wcw-typo"),
        "the failure must name the rejected id: {unknown_member}"
    );

    // The Standing Agent itself must already exist; the command never creates one.
    let unknown_actor = run_failure(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-absent",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            &project_id,
        ],
    );
    assert!(
        unknown_actor.contains("Company actor not found"),
        "expected a missing-StandingAgent error, got: {unknown_actor}"
    );

    // Nothing above may have mutated the record.
    let after = standing_agent(&home, &project_id, "agent-wcw-ops");
    assert_eq!(before, after, "failed validation must not write a row");
}

#[test]
fn explicit_link_preserves_actor_fields_and_is_idempotent_across_relink_and_unlink() {
    let home = TempHome::new("company-execution-link-lifecycle");
    let project_id = init_project(&home, "wcw");
    seed(&home, &project_id);
    let before = standing_agent(&home, &project_id, "agent-wcw-ops");

    let linked = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            &project_id,
        ],
    );
    assert_eq!(linked["result"]["changed"], true);
    assert_eq!(linked["result"]["standing_agent_id"], "agent-wcw-ops");
    assert_eq!(linked["result"]["agent_member_id"], "agent-wcw-ops");
    assert_eq!(linked["result"]["inference"], "none_explicit_ids_only");
    assert_eq!(
        linked["result"]["validated_against"]["execution_space_id"], project_id,
        "the validating space must be reported"
    );
    assert_eq!(
        linked["result"]["write_path"], "administrative_standing_agent_execution_link_append",
        "the write must go through the governed administrative append"
    );

    // Every field except the reference and the timestamp round-trips.
    let after = standing_agent(&home, &project_id, "agent-wcw-ops");
    assert_eq!(after["execution_agent_member_ref"], "agent-wcw-ops");
    for field in [
        "id",
        "display_name",
        "role",
        "status",
        "availability",
        "assignment_capacity",
        "responsibility_summary",
        "capability_refs",
        "tool_refs",
        "skill_refs",
        "permission_policy_refs",
        "membership_refs",
        "created_at",
    ] {
        assert_eq!(
            before[field], after[field],
            "link must preserve StandingAgent.{field}"
        );
    }
    assert_eq!(after["capability_refs"], serde_json::json!(["ops.review"]));
    assert_eq!(after["assignment_capacity"], 3);

    // Re-running the same explicit pair is a no-op, so a migration is re-runnable.
    let repeat = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            &project_id,
        ],
    );
    assert_eq!(repeat["result"]["changed"], false);
    assert_eq!(repeat["result"]["reason"], "already_linked");

    // Repointing to a different AgentMember requires an explicit --replace.
    let guarded = run_failure(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-other",
            "--execution-space",
            &project_id,
        ],
    );
    assert!(
        guarded.contains("pass --replace"),
        "an unguarded repoint must be refused: {guarded}"
    );
    assert_eq!(
        standing_agent(&home, &project_id, "agent-wcw-ops")["execution_agent_member_ref"],
        "agent-wcw-ops",
        "a refused repoint must not write"
    );

    let replaced = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-other",
            "--execution-space",
            &project_id,
            "--replace",
        ],
    );
    assert_eq!(replaced["result"]["changed"], true);
    assert_eq!(
        replaced["result"]["previous_agent_member_ref"], "agent-wcw-ops",
        "a repoint must record what it replaced"
    );

    // Unlink guards on the caller's expectation before clearing the reference.
    let stale_guard = run_failure(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "unlink-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--expect-agent-member",
            "agent-wcw-ops",
        ],
    );
    assert!(
        stale_guard.contains("refusing to unlink"),
        "a stale expectation must be refused: {stale_guard}"
    );

    let unlinked = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "unlink-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--expect-agent-member",
            "agent-wcw-other",
        ],
    );
    assert_eq!(unlinked["result"]["changed"], true);
    assert_eq!(
        unlinked["result"]["previous_agent_member_ref"],
        "agent-wcw-other"
    );
    assert!(
        standing_agent(&home, &project_id, "agent-wcw-ops")["execution_agent_member_ref"].is_null()
    );

    // Unlinking twice is also a no-op.
    let repeat_unlink = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "unlink-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
        ],
    );
    assert_eq!(repeat_unlink["result"]["changed"], false);
    assert_eq!(repeat_unlink["result"]["reason"], "already_unlinked");
}

#[test]
fn idempotent_no_op_relation_commands_still_require_admin_authority() {
    let home = TempHome::new("company-execution-link-authority");
    let project_id = init_project(&home, "wcw");
    seed(&home, &project_id);

    // A Human without company_os.admin exists alongside the root authority.
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-human",
            "--authority",
            "human-wcw-owner",
            "--id",
            "human-wcw-clerk",
            "--name",
            "Wcw Clerk",
            "--responsibility",
            "Reads company records",
            "--permission",
            "company_os.read",
            "--authority-policy",
            "company_os.read",
        ],
    );

    let ledger = home
        .harness_home()
        .join("projects")
        .join(&project_id)
        .join("company_os_standing_agents.jsonl");
    let rows_before = std::fs::read_to_string(&ledger).unwrap_or_default();

    // Establish the link with the real authority, then re-attempt the SAME pair
    // with authorities that must never succeed. Each attempt would be a no-op,
    // so "nothing to change" is exactly the case that could hide a bypass.
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            &project_id,
        ],
    );
    let rows_after_link = std::fs::read_to_string(&ledger).unwrap();

    for (authority, expected) in [
        ("human-wcw-ghost", "actor:human-wcw-ghost"),
        ("human-wcw-clerk", "lacks permission company_os.admin"),
    ] {
        let denied_link = run_failure(
            &home,
            &project_id,
            &[
                "company",
                "org",
                "link-execution",
                "--authority",
                authority,
                "--actor",
                "agent-wcw-ops",
                "--agent-member",
                "agent-wcw-ops",
                "--execution-space",
                &project_id,
            ],
        );
        assert!(
            denied_link.contains(expected),
            "no-op link with authority {authority} must be rejected ({expected}), got: {denied_link}"
        );

        // While still linked this exercises the mutating unlink path.
        let denied_unlink = run_failure(
            &home,
            &project_id,
            &[
                "company",
                "org",
                "unlink-execution",
                "--authority",
                authority,
                "--actor",
                "agent-wcw-ops",
            ],
        );
        assert!(
            denied_unlink.contains(expected),
            "unlink with authority {authority} must be rejected ({expected}), got: {denied_unlink}"
        );
    }

    // The rejected attempts changed nothing, and the authorized no-op still
    // appends no extra row.
    assert_eq!(
        standing_agent(&home, &project_id, "agent-wcw-ops")["execution_agent_member_ref"],
        "agent-wcw-ops",
        "rejected attempts must leave the relation intact"
    );
    let repeat = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            &project_id,
        ],
    );
    assert_eq!(repeat["result"]["changed"], false);
    let rows_now = std::fs::read_to_string(&ledger).unwrap();
    assert_eq!(
        rows_now, rows_after_link,
        "an authorized no-op must not append a Standing Agent row"
    );
    assert_ne!(
        rows_now, rows_before,
        "the original authorized link must have appended exactly one row"
    );

    // Now reach the `already_unlinked` no-op specifically: unlink for real
    // first, so the following attempts cannot fall through to the mutating
    // path that the append-time authorization already covers.
    let unlinked = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "unlink-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
        ],
    );
    assert_eq!(unlinked["result"]["changed"], true);
    let rows_after_unlink = std::fs::read_to_string(&ledger).unwrap();

    for (authority, expected) in [
        ("human-wcw-ghost", "actor:human-wcw-ghost"),
        ("human-wcw-clerk", "lacks permission company_os.admin"),
    ] {
        let denied_no_op_unlink = run_failure(
            &home,
            &project_id,
            &[
                "company",
                "org",
                "unlink-execution",
                "--authority",
                authority,
                "--actor",
                "agent-wcw-ops",
            ],
        );
        assert!(
            denied_no_op_unlink.contains(expected),
            "already_unlinked no-op with authority {authority} must be rejected ({expected}), got: {denied_no_op_unlink}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&ledger).unwrap(),
        rows_after_unlink,
        "rejected already_unlinked no-ops must not touch the ledger"
    );

    // The authorized already_unlinked no-op still writes nothing.
    let authorized_no_op = run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "unlink-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
        ],
    );
    assert_eq!(authorized_no_op["result"]["changed"], false);
    assert_eq!(authorized_no_op["result"]["reason"], "already_unlinked");
    assert_eq!(
        std::fs::read_to_string(&ledger).unwrap(),
        rows_after_unlink,
        "an authorized already_unlinked no-op must not append a row"
    );
}

#[test]
fn dashboard_snapshot_reports_link_conflicts_without_failing() {
    let home = TempHome::new("company-execution-link-snapshot");
    let project_id = init_project(&home, "wcw");
    seed(&home, &project_id);
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "link-execution",
            "--authority",
            "human-wcw-owner",
            "--actor",
            "agent-wcw-ops",
            "--agent-member",
            "agent-wcw-ops",
            "--execution-space",
            &project_id,
        ],
    );

    let snapshot = run_json(&home, &project_id, &["dashboard", "snapshot"]);
    let company = &snapshot["company_os"];
    assert!(
        company["standing_assignment_conflicts"].is_array(),
        "the snapshot must always carry the conflict key: {company}"
    );
    assert_eq!(
        company["standing_assignment_conflicts"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "a healthy store reports an empty conflict list"
    );
}
