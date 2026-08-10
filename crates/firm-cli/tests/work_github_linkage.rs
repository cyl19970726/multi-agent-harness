//! Integration coverage for GitHub work linkage (issue #369):
//!   - `work create --github-issue owner/repo#N` links the Work to a GitHub
//!     issue and auto-populates `artifact_refs` with the issue URL,
//!   - `work submit --github-pr owner/repo#N` attaches a PR link with a live
//!     CI snapshot (`ci_status`/`ci_url`) and auto-populates `artifact_refs`
//!     and `check_refs`,
//!   - `work show` renders `github_links` inline in the Work JSON.
//!
//! The metadata fetch shells out to the real `gh` CLI -- the same mechanism
//! the feature uses -- so the live assertions run against the real GitHub API
//! for a public repo. When `gh` is missing or unauthenticated, the live test
//! skips with an explicit message so the CI gate stays green on runners
//! without a GitHub token; the malformed-input assertion runs everywhere.

mod firm_env;

use std::process::Command;

use firm_env::{
    create_canonical_agent_member, current_project_id, run_firm, run_firm_with_env, TempHome,
};

/// Public repo plus stable GitHub objects used by the live assertions.
const GH_REPO: &str = "cyl19970726/multi-agent-harness";
/// Open issue: "GitHub integration: work ↔ issue/PR linkage with auto status sync".
const GH_ISSUE_NUMBER: u64 = 369;
/// Merged PR whose CI checks are archived and stable.
const GH_PR_NUMBER: u64 = 365;

fn real_home() -> String {
    std::env::var("HOME").expect("test runner HOME")
}

fn gh_ready() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    let registration = run_firm(
        home,
        &root,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    assert!(
        registration.status.success(),
        "register failed: {registration:?}"
    );
    let mission = run_firm(
        home,
        &root,
        &[
            "mission",
            "create",
            "--id",
            "mission-github-fixture",
            "--title",
            "GitHub linkage mission",
            "--objective",
            "Verify GitHub-linked Work",
        ],
    );
    assert!(
        mission.status.success(),
        "mission create failed: {mission:?}"
    );
    let host = create_canonical_agent_member(
        home,
        &root,
        &project_id,
        "agent-github-host",
        "github-host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host create failed: {host:?}");
    let team = run_firm(
        home,
        &root,
        &[
            "team",
            "create",
            "--id",
            "team-github-fixture",
            "--name",
            "GitHub linkage team",
            "--description",
            "Flat GitHub linkage test team",
            "--mission-id",
            "mission-github-fixture",
            "--host-agent-id",
            "agent-github-host",
            "--node-id",
            node_id,
            "--member",
            "agent-github-host",
        ],
    );
    assert!(team.status.success(), "team create failed: {team:?}");
    project_id
}

/// Host-side harness command against the isolated store. `HOME` stays at the
/// real value so the child `gh` process finds its authenticated config; the
/// harness store itself stays isolated via `FIRM_HOME`.
fn host_firm_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(home, home.base(), &full, &[("HOME", &real_home())]);
    assert!(
        out.status.success(),
        "harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?} stdout not JSON ({error}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// Member-side harness command with the bound ProviderRuntimeProjection/TeamRun environment.
fn member_firm_json(
    home: &TempHome,
    project_id: &str,
    run_id: &str,
    member_id: &str,
    args: &[&str],
) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(
        home,
        home.base(),
        &full,
        &[
            ("HOME", &real_home()),
            ("FIRM_TEAM_RUN_ID", run_id),
            ("FIRM_MEMBER_RUN_ID", member_id),
        ],
    );
    assert!(
        out.status.success(),
        "member harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
        panic!(
            "member harness {args:?} stdout not JSON ({error}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// Create a TeamRun with one `github-member`; return
/// `(home, project_id, run_id, member_id)`.
fn github_fixture(tag: &str) -> (TempHome, String, String, String) {
    let home = TempHome::new(tag);
    let project_id = init_project(&home, "alpha");
    let member = create_canonical_agent_member(
        &home,
        home.base(),
        &project_id,
        "github-member",
        "github-member",
        "implementer",
        "kimi",
        &[],
    );
    assert!(member.status.success(), "member create failed: {member:?}");
    let placed = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team",
            "add-member",
            "--id",
            "team-github-fixture",
            "--member",
            "github-member",
        ],
    );
    assert!(
        placed.status.success(),
        "member placement failed: {placed:?}"
    );
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            "team-github-fixture",
            "--objective",
            "GitHub linkage fixture",
            "--member",
            "github-member:implementer:kimi",
        ],
    );
    assert!(
        out.status.success(),
        "team-run create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let status = host_firm_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    let member_id = status["members"][0]["member_run"]["id"]
        .as_str()
        .expect("member run id")
        .to_string();
    (home, project_id, run_id, member_id)
}

#[test]
fn github_issue_and_pr_linkage_roundtrip() {
    if !gh_ready() {
        eprintln!("skipping live GitHub linkage assertions: `gh` is not authenticated");
        return;
    }
    let (home, project_id, run_id, member_id) = github_fixture("github-linkage-roundtrip");
    let github_issue = format!("{GH_REPO}#{GH_ISSUE_NUMBER}");

    // Create links the Work to the issue and auto-populates artifact_refs.
    let created = host_firm_json(
        &home,
        &project_id,
        &[
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            "GitHub-linked Work",
            "--completion-criteria",
            "link stored and shown",
            "--owner-member-run-id",
            &member_id,
            "--github-issue",
            &github_issue,
        ],
    );
    assert_eq!(created["phase"].as_str(), Some("open"));
    let issue_url = format!("https://github.com/{GH_REPO}/issues/{GH_ISSUE_NUMBER}");
    assert_eq!(
        created["artifact_refs"][0].as_str(),
        Some(issue_url.as_str()),
        "issue URL auto-populated into artifact_refs"
    );
    let issue_link = &created["github_links"][0];
    assert_eq!(issue_link["kind"].as_str(), Some("issue"));
    assert_eq!(issue_link["owner"].as_str(), Some("cyl19970726"));
    assert_eq!(issue_link["repo"].as_str(), Some("multi-agent-harness"));
    assert_eq!(issue_link["number"].as_u64(), Some(GH_ISSUE_NUMBER));
    assert_eq!(issue_link["url"].as_str(), Some(issue_url.as_str()));
    assert!(
        issue_link["status"]
            .as_str()
            .is_some_and(|status| !status.is_empty()),
        "issue state must be snapshotted: {issue_link}"
    );
    let work_id = created["id"].as_str().expect("work id").to_string();

    // Member starts and submits with a PR: the PR link + CI snapshot attach
    // while the issue link from create is preserved (merge, not replace).
    member_firm_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "1",
            "--member-run-id",
            &member_id,
        ],
    );
    let pr_ref = format!("{GH_REPO}#{GH_PR_NUMBER}");
    let submitted = member_firm_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &member_id,
            "--result",
            "submission with GitHub PR linkage",
            "--github-pr",
            &pr_ref,
        ],
    );
    assert_eq!(submitted["phase"].as_str(), Some("review"));
    let links = submitted["github_links"]
        .as_array()
        .expect("github_links array");
    assert_eq!(
        links.len(),
        2,
        "issue link kept and PR link attached: {links:?}"
    );
    let pr_link = &links[1];
    assert_eq!(pr_link["kind"].as_str(), Some("pull_request"));
    assert_eq!(pr_link["owner"].as_str(), Some("cyl19970726"));
    assert_eq!(pr_link["repo"].as_str(), Some("multi-agent-harness"));
    assert_eq!(pr_link["number"].as_u64(), Some(GH_PR_NUMBER));
    let pr_url = format!("https://github.com/{GH_REPO}/pull/{GH_PR_NUMBER}");
    assert_eq!(pr_link["url"].as_str(), Some(pr_url.as_str()));
    assert_eq!(pr_link["status"].as_str(), Some("MERGED"));
    let ci_status = pr_link["ci_status"].as_str().expect("PR CI snapshot");
    assert!(
        matches!(ci_status, "success" | "failure" | "pending" | "unknown"),
        "unexpected ci_status: {ci_status}"
    );
    let ci_url = pr_link["ci_url"].as_str().expect("PR CI url");
    assert!(
        ci_url.starts_with("https://github.com/"),
        "ci_url: {ci_url}"
    );
    assert!(
        submitted["artifact_refs"]
            .as_array()
            .expect("artifact_refs")
            .iter()
            .any(|value| value.as_str() == Some(pr_url.as_str())),
        "PR URL auto-populated into artifact_refs"
    );
    assert!(
        submitted["check_refs"]
            .as_array()
            .expect("check_refs")
            .iter()
            .any(|value| value.as_str() == Some(ci_url)),
        "CI URL auto-populated into check_refs"
    );

    // `work show` renders the linked issue/PR status inline.
    let shown = host_firm_json(
        &home,
        &project_id,
        &["team-run", "work", "show", "--work-id", &work_id],
    );
    let shown_links = shown["work"]["github_links"]
        .as_array()
        .expect("shown github_links array");
    assert_eq!(shown_links.len(), 2);
    assert_eq!(shown_links[0]["kind"].as_str(), Some("issue"));
    assert_eq!(shown_links[1]["kind"].as_str(), Some("pull_request"));
    assert_eq!(shown_links[1]["ci_status"].as_str(), Some(ci_status));
    assert!(
        shown["work"]["artifact_refs"]
            .as_array()
            .expect("shown artifact_refs")
            .iter()
            .any(|value| value.as_str() == Some(pr_url.as_str())),
        "show carries the PR artifact ref"
    );

    // `work show` also renders the Phase 2 top-level GitHub linkage section,
    // live-refreshed when `gh` is available.
    let shown_github = shown["github_links"]
        .as_array()
        .expect("shown github section");
    assert_eq!(shown_github.len(), 2);
    assert_eq!(shown_github[0]["kind"].as_str(), Some("issue"));
    assert_eq!(shown_github[0]["source"].as_str(), Some("live"));
    assert_eq!(shown_github[1]["kind"].as_str(), Some("pull_request"));
    assert_eq!(shown_github[1]["ci_status"].as_str(), Some(ci_status));
    assert_eq!(shown_github[1]["url"].as_str(), Some(pr_url.as_str()));
}

#[test]
fn github_pr_merge_auto_submits_in_progress_work() {
    if !gh_ready() {
        eprintln!("skipping live GitHub linkage assertions: `gh` is not authenticated");
        return;
    }
    let (home, project_id, run_id, member_id) = github_fixture("github-merge-auto-submit");
    // Merged PR with green CI (all checks SUCCESS).
    let pr_ref = format!("{GH_REPO}#362");
    let created = host_firm_json(
        &home,
        &project_id,
        &[
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            "Auto-submit on merge",
            "--completion-criteria",
            "daemon submits when the linked PR merges",
            "--owner-member-run-id",
            &member_id,
            "--github-pr",
            &pr_ref,
        ],
    );
    assert_eq!(created["phase"].as_str(), Some("open"));
    assert_eq!(
        created["github_links"][0]["kind"].as_str(),
        Some("pull_request")
    );
    assert_eq!(
        created["github_links"][0]["status"].as_str(),
        Some("MERGED")
    );
    let work_id = created["id"].as_str().expect("work id").to_string();

    // Member starts; the Work is in_progress carrying the linked PR.
    member_firm_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "1",
            "--member-run-id",
            &member_id,
        ],
    );

    // The poll observes the merged green PR and auto-submits to review.
    let polled = host_firm_json(
        &home,
        &project_id,
        &[
            "team-run",
            "work",
            "poll-github-ci",
            "--team-run-id",
            &run_id,
        ],
    );
    assert_eq!(polled["gh_unavailable"].as_bool(), Some(false));
    assert!(
        polled["auto_submitted"]
            .as_array()
            .expect("auto_submitted")
            .iter()
            .any(|value| value.as_str() == Some(work_id.as_str())),
        "merged green PR must auto-submit the Work: {polled}"
    );
    let submitted = host_firm_json(
        &home,
        &project_id,
        &["team-run", "work", "show", "--work-id", &work_id],
    );
    assert_eq!(submitted["work"]["phase"].as_str(), Some("review"));
    let result = submitted["work"]["result_summary"]
        .as_str()
        .expect("result");
    assert!(
        result.contains("auto-submitted by GitHub merge observation"),
        "result must explain the merge observation: {result}"
    );
    assert!(result.contains("#362"), "result names the PR: {result}");
}

#[test]
fn github_pr_merge_on_red_ci_is_held_for_host() {
    if !gh_ready() {
        eprintln!("skipping live GitHub linkage assertions: `gh` is not authenticated");
        return;
    }
    let (home, project_id, run_id, member_id) = github_fixture("github-merge-red-ci");
    // Merged PR with red CI (rust check FAILURE).
    let pr_ref = format!("{GH_REPO}#{GH_PR_NUMBER}");
    let created = host_firm_json(
        &home,
        &project_id,
        &[
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            "Hold on red CI",
            "--completion-criteria",
            "red-CI merges stay open for Host judgment",
            "--owner-member-run-id",
            &member_id,
            "--github-pr",
            &pr_ref,
        ],
    );
    let work_id = created["id"].as_str().expect("work id").to_string();
    assert_eq!(
        created["github_links"][0]["ci_status"].as_str(),
        Some("failure"),
        "fixture PR must have red CI"
    );
    member_firm_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "1",
            "--member-run-id",
            &member_id,
        ],
    );

    let polled = host_firm_json(
        &home,
        &project_id,
        &[
            "team-run",
            "work",
            "poll-github-ci",
            "--team-run-id",
            &run_id,
        ],
    );
    assert!(
        polled["blocked_on_failure"]
            .as_array()
            .expect("blocked_on_failure")
            .iter()
            .any(|value| value.as_str() == Some(work_id.as_str())),
        "red-CI merge must be held for the Host, not auto-submitted: {polled}"
    );
    let held = host_firm_json(
        &home,
        &project_id,
        &["team-run", "work", "show", "--work-id", &work_id],
    );
    assert_eq!(
        held["work"]["phase"].as_str(),
        Some("active"),
        "Work stays active on red CI"
    );
}

#[test]
fn github_ref_malformed_input_is_rejected() {
    let (home, project_id, run_id, _member_id) = github_fixture("github-ref-malformed");
    for bad in [
        "not-a-ref",
        "owner/repo",
        "owner/repo#0",
        "owner/repo#abc",
        "owner/repo#1/2",
    ] {
        let out = run_firm(
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
                "malformed github ref",
                "--completion-criteria",
                "must be rejected",
                "--github-issue",
                bad,
            ],
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && stderr.contains(bad),
            "malformed ref {bad:?} must fail and name the offending value: {stderr}"
        );
    }
}
