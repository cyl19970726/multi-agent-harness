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

    // DOC-108 retired the Mission writers this test used; the retained
    // space-owned writer proving the same store/binding split is
    // `team create`, which lands trust envelopes in the Execution Space
    // store while the provider cwd comes from the Project Binding.
    let host = crate::firm_env::create_canonical_agent_member(
        &home,
        &first,
        &second_binding,
        "agent-space-host",
        "space-host",
        "host",
        "codex",
        &[("FIRM_SPACE", space_id.as_str())],
    );
    assert!(host.status.success(), "host create failed: {host:?}");
    let node = json(&run(
        &home,
        &first,
        &[
            "--space",
            &space_id,
            "--project",
            &second_binding,
            "node",
            "init",
        ],
    ));
    let node_id = node["id"].as_str().expect("node id");
    let team = json(&run(
        &home,
        &first,
        &[
            "--space",
            &space_id,
            "--project",
            &second_binding,
            "team",
            "create",
            "--name",
            "Independent binding",
            "--description",
            "prove the store/binding split",
            "--host-agent-id",
            "agent-space-host",
            "--node-id",
            node_id,
            "--member",
            "agent-space-host",
        ],
    ));
    assert!(team["id"].as_str().is_some());

    let store = home.spaces_dir().join(&space_id);
    assert!(store.join("agentfirm_trust_operations.jsonl").is_file());
    let project_store = home.projects_dir().join(&second_binding);
    assert!(
        !project_store
            .join("agentfirm_trust_operations.jsonl")
            .is_file(),
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

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn rewrite_store_roots_to_absolute(home: &TempHome) {
    for (path, key) in [
        (home.registry_path(), "projects"),
        (home.space_registry_path(), "spaces"),
    ] {
        let mut registry: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for entry in registry[key].as_array_mut().unwrap() {
            let relative = entry["store_root"].as_str().unwrap().to_string();
            entry["store_root"] =
                serde_json::Value::String(home.firm_home().join(&relative).display().to_string());
        }
        std::fs::write(&path, serde_json::to_string_pretty(&registry).unwrap()).unwrap();
    }
}

#[test]
fn fresh_init_writes_relative_store_root_and_absolute_inside_home_still_loads() {
    let home = TempHome::new("space-registry-relative-root");
    let root = home.base().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    init(&home, &root);
    for (path, key) in [
        (home.registry_path(), "projects"),
        (home.space_registry_path(), "spaces"),
    ] {
        let registry: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for entry in registry[key].as_array().unwrap() {
            let store_root = entry["store_root"].as_str().expect("store_root recorded");
            assert!(
                !store_root.starts_with('/'),
                "store_root must be recorded relative to FIRM_HOME in {}: {store_root}",
                path.display()
            );
        }
    }
    // The relative form reloads.
    assert!(run(&home, &root, &["space", "list"]).status.success());

    // An absolute store_root inside FIRM_HOME still loads unchanged.
    rewrite_store_roots_to_absolute(&home);
    let listed = run(&home, &root, &["space", "list"]);
    assert!(
        listed.status.success(),
        "absolute in-home store_root must load: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
}

#[test]
fn copied_home_refuses_external_store_root_until_explicitly_allowed() {
    let home = TempHome::new("space-registry-external-root");
    let root = home.base().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    init(&home, &root);
    // The pre-#794 registry form: absolute store_root paths, as produced by
    // older installs (and by the DEV-189 incident).
    rewrite_store_roots_to_absolute(&home);

    let copied = TempHome::new("space-registry-external-root-copy");
    copy_tree(home.firm_home(), copied.firm_home());

    // `space list` refuses: the recorded roots point at the ORIGINAL home.
    let refused = run(&copied, &root, &["space", "list"]);
    assert!(
        !refused.status.success(),
        "a copied home must refuse external store_root"
    );
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("refusing external store_root"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(home.firm_home().to_string_lossy().as_ref()),
        "the refusal names the recorded path: {stderr}"
    );
    assert!(
        stderr.contains(copied.firm_home().to_string_lossy().as_ref()),
        "the refusal names the current FIRM_HOME: {stderr}"
    );

    // `serve` refuses at startup the same way.
    let serve = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(["serve", "--addr", "127.0.0.1:0"])
        .current_dir(&root)
        .envs(copied.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .output()
        .expect("run serve");
    assert!(
        !serve.status.success(),
        "serve must refuse external store_root"
    );
    assert!(
        String::from_utf8_lossy(&serve.stderr).contains("refusing external store_root"),
        "serve stderr: {}",
        String::from_utf8_lossy(&serve.stderr)
    );

    // With the explicit override the copied home runs and warns.
    let allowed = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(["space", "list"])
        .current_dir(&root)
        .envs(copied.envs())
        .env("FIRM_ALLOW_EXTERNAL_STORE_ROOT", "1")
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .output()
        .expect("run harness");
    assert!(
        allowed.status.success(),
        "override must allow the copied home: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&allowed.stderr).contains("WARNING: external store_root"),
        "the override run must print the WARNING line: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );

    // And serve starts under the override, serving the copied home's
    // (external) roots.
    let serve = firm_env::ServeHandle::spawn_with_env(
        &copied,
        &root,
        &[],
        &[("FIRM_ALLOW_EXTERNAL_STORE_ROOT", "1")],
    );
    let (status, _snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200, "serve must run under the override");
}
