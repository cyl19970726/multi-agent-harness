//! Integration coverage for ADR 0042's native Execution Space / Project
//! Binding split. All commands run against an isolated HOME.

use std::path::Path;
use std::process::{Command, Output};

mod firm_env;
use firm_env::TempHome;

fn run(home: &TempHome, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(args)
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .output()
        .expect("run harness")
}

fn json(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn init(home: &TempHome, root: &Path) -> (String, String) {
    assert!(run(home, root, &["init"]).status.success());
    let projects: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let spaces: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.space_registry_path()).unwrap())
            .unwrap();
    (
        projects["current_project_id"].as_str().unwrap().to_string(),
        spaces["current_space_id"].as_str().unwrap().to_string(),
    )
}

#[test]
fn project_binding_selection_never_changes_execution_store() {
    let home = TempHome::new("space-binding-independent");
    let first = home.base().join("first");
    let second = home.base().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let (_first_binding, space_id) = init(&home, &first);
    let added = run(
        &home,
        &second,
        &["project", "add", second.to_str().unwrap()],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let projects: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let second_binding = projects["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("/second"))
        })
        .and_then(|entry| entry["id"].as_str())
        .unwrap()
        .to_string();

    let mission = json(&run(
        &home,
        &first,
        &[
            "--space",
            &space_id,
            "--project",
            &second_binding,
            "mission",
            "create",
            "--title",
            "Independent binding",
            "--objective",
            "prove the store/binding split",
            "--context",
            "The Mission is stored in the space while provider cwd uses the binding.",
            "--json",
        ],
    ));
    assert!(mission["id"].as_str().is_some());

    let store = home.spaces_dir().join(&space_id);
    assert!(store.join("missions.jsonl").is_file());
    let project_store = home.projects_dir().join(&second_binding);
    assert!(
        !project_store.join("missions.jsonl").is_file(),
        "selecting a Project Binding must not create execution truth in its compatibility store"
    );
}

#[test]
fn explicit_migration_copies_only_execution_truth_and_keeps_source() {
    let home = TempHome::new("space-migration");
    let project_root = home.base().join("repo");
    std::fs::create_dir_all(&project_root).unwrap();
    let (binding_id, _initial_space) = init(&home, &project_root);
    let source = home.projects_dir().join(&binding_id);
    std::fs::write(
        source.join("missions.jsonl"),
        "{\"id\":\"mission-legacy\"}\n",
    )
    .unwrap();
    std::fs::write(
        source.join("host_attentions.jsonl"),
        b"{\"id\":\"host-attention-legacy\",\"status\":\"actionable\"}\n",
    )
    .unwrap();
    std::fs::write(
        source.join("company_os_documents.jsonl"),
        "{\"id\":\"company-doc\"}\n",
    )
    .unwrap();
    std::fs::create_dir_all(source.join("checks/nested")).unwrap();
    std::fs::write(
        source.join("checks/nested/evidence.json"),
        b"{\"ok\":true}\n",
    )
    .unwrap();
    std::fs::create_dir_all(source.join("provider-sessions")).unwrap();
    std::fs::write(source.join("provider-sessions/native.jsonl"), "native\n").unwrap();

    let migrated = json(&run(
        &home,
        &project_root,
        &[
            "space",
            "migrate-from-project",
            "--from-project",
            &binding_id,
            "--id",
            "migrated-space",
            "--name",
            "Migrated Space",
        ],
    ));
    let target = home.spaces_dir().join("migrated-space");
    assert_eq!(
        std::fs::read(source.join("missions.jsonl")).unwrap(),
        std::fs::read(target.join("missions.jsonl")).unwrap()
    );
    assert_eq!(
        std::fs::read(source.join("host_attentions.jsonl")).unwrap(),
        std::fs::read(target.join("host_attentions.jsonl")).unwrap(),
        "Host-attention execution truth must be copied and byte-verified"
    );
    assert!(!target.join("company_os_documents.jsonl").exists());
    assert!(!target.join("provider-sessions").exists());
    assert_eq!(
        std::fs::read(source.join("checks/nested/evidence.json")).unwrap(),
        std::fs::read(target.join("checks/nested/evidence.json")).unwrap(),
        "whitelisted execution evidence must be copied and byte-verified"
    );
    assert!(
        source.join("missions.jsonl").exists(),
        "migration is copy-only"
    );
    assert_eq!(migrated["migration"]["verified_records"].as_u64(), Some(2));
    assert_eq!(
        migrated["migration"]["copied_files"].as_u64(),
        Some(3),
        "two JSONL records plus one evidence file must be byte-verified"
    );
    assert_eq!(
        migrated["migration"]["source_retained"].as_bool(),
        Some(true)
    );
    assert!(migrated["migration"].get("rollback").is_none());
    assert_eq!(
        migrated["migration"]["registration"]["status"].as_str(),
        Some("complete")
    );
    assert_eq!(
        migrated["migration"]["registration"]["recovery_command"].as_str(),
        Some("harness space switch migrated-space")
    );
    assert!(target.join("execution_space_migration.json").is_file());
}

#[test]
fn migration_rejects_an_unsafe_space_id_before_creating_a_target() {
    let home = TempHome::new("space-migration-unsafe-id");
    let project_root = home.base().join("repo");
    std::fs::create_dir_all(&project_root).unwrap();
    let (binding_id, _initial_space) = init(&home, &project_root);
    let outside = home.firm_home().join("escape");

    let output = run(
        &home,
        &project_root,
        &[
            "space",
            "migrate-from-project",
            "--from-project",
            &binding_id,
            "--id",
            "../escape",
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid execution space id"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !outside.exists(),
        "unsafe id must not materialize any target"
    );
}
