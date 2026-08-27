//! Regression coverage for Issue #631: help is an effect-free CLI boundary.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use harness_core::{
    AgentTeamRun, ExecutionNode, ExecutionNodeStatus, HostControlMode, NodeProjectRegistration,
    NodeProjectRegistrationStatus, TeamRunStatus,
};
use harness_store::HarnessStore;

mod firm_env;
use firm_env::{run_firm, TempHome};

const RUN_ID: &str = "team-run-help-boundary";
const NODE_ID: &str = "00000000-0000-4000-8000-000000000631";
const SPACE_ID: &str = "space-help-boundary";
const PROJECT_ID: &str = "project-help-boundary";

fn append_jsonl<T: serde::Serialize>(path: &Path, value: &T) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open fixture ledger");
    serde_json::to_writer(&mut file, value).expect("serialize fixture row");
    file.write_all(b"\n").expect("terminate fixture row");
    file.sync_all().expect("persist fixture row");
}

fn seed_running_team_run(root: &Path) -> HarnessStore {
    let store = HarnessStore::new(root);
    store.init().expect("initialize fixture Store");
    store
        .insert_execution_node(&ExecutionNode {
            id: NODE_ID.into(),
            display_name: "help-boundary-node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert fixture ExecutionNode");
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: NODE_ID.into(),
                execution_space_id: SPACE_ID.into(),
                project_binding_id: PROJECT_ID.into(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            SPACE_ID,
        )
        .expect("register fixture project");
    append_jsonl(
        &root.join("team_runs.jsonl"),
        &AgentTeamRun {
            id: RUN_ID.into(),
            agent_team_id: "team-help-boundary".into(),
            execution_node_id: NODE_ID.into(),
            project_binding_id: PROJECT_ID.into(),
            previous_run_id: None,
            host_surface: "test".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: HostControlMode::ExternalInteractive,
            objective: "prove help has no durable effect".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: Vec::new(),
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        },
    );
    store
}

fn file_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .expect("read snapshot directory")
            .map(|entry| entry.expect("snapshot entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(base, &path, snapshot);
            } else {
                snapshot.insert(
                    path.strip_prefix(base)
                        .expect("relative snapshot path")
                        .into(),
                    fs::read(&path).expect("read snapshot file"),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn assert_help(output: &std::process::Output, command: &str) {
    assert!(
        output.status.success(),
        "{command} help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(command),
        "{command} help was not contextual: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn mutating_help_is_effect_free_and_normal_dispatch_is_unchanged() {
    let home = TempHome::new("mutation-help-boundary");
    let store_root = home.base().join("store");
    let store = seed_running_team_run(&store_root);
    let store_arg = store_root.to_str().expect("UTF-8 fixture path");
    let before_help = file_snapshot(&store_root);

    for flag in ["--help", "-h"] {
        let output = run_firm(
            &home,
            home.base(),
            &[
                "--store", store_arg, "team-run", "complete", "--id", RUN_ID, flag,
            ],
        );
        assert_help(&output, "team-run complete");
        assert_eq!(
            file_snapshot(&store_root),
            before_help,
            "{flag} changed durable Store bytes"
        );
    }

    for (args, command) in [
        (
            vec!["--store", store_arg, "team", "rename", "-h"],
            "team rename",
        ),
        (
            vec!["--store", store_arg, "team-run", "work", "cancel", "--help"],
            "team-run work cancel",
        ),
        (
            vec!["--store", store_arg, "member", "work", "submit", "-h"],
            "member work submit",
        ),
    ] {
        let output = run_firm(&home, home.base(), &args);
        assert_help(&output, command);
        assert_eq!(
            file_snapshot(&store_root),
            before_help,
            "{command} help changed durable Store bytes"
        );
    }

    let missing_id = run_firm(
        &home,
        home.base(),
        &["--store", store_arg, "team-run", "complete"],
    );
    assert!(!missing_id.status.success());
    assert!(String::from_utf8_lossy(&missing_id.stderr).contains("--id is required"));
    assert_eq!(file_snapshot(&store_root), before_help);

    for args in [
        vec![
            "--store",
            store_arg,
            "team-run",
            "work",
            "create",
            "--owner-member-run-id",
            "legacy-runtime",
        ],
        vec![
            "--store",
            store_arg,
            "team-run",
            "work",
            "assign",
            "--member-run-id",
            "legacy-runtime",
        ],
        vec![
            "--store",
            store_arg,
            "team-run",
            "work",
            "retarget",
            "--successor-member-run-id",
            "legacy-runtime",
        ],
    ] {
        let rejected = run_firm(&home, home.base(), &args);
        assert!(!rejected.status.success());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("unknown work option"));
        assert_eq!(
            file_snapshot(&store_root),
            before_help,
            "retired runtime ownership option changed durable Store bytes"
        );
    }

    let completed = run_firm(
        &home,
        home.base(),
        &["--store", store_arg, "team-run", "complete", "--id", RUN_ID],
    );
    assert!(
        completed.status.success(),
        "normal completion failed: {}",
        String::from_utf8_lossy(&completed.stderr)
    );
    let latest = store
        .team_runs()
        .expect("read TeamRuns")
        .into_iter()
        .rfind(|run| run.id == RUN_ID)
        .expect("fixture TeamRun");
    assert_eq!(latest.status, TeamRunStatus::Completed);
    assert!(latest.completed_at.is_some());
}
