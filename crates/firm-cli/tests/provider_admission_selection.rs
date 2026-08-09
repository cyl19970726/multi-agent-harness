//! Real-CLI regression coverage for the provider-admission Project Binding /
//! Execution Space authority boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod firm_env;
use firm_env::TempHome;

const VERSION: &str = "9.9.9";
const SPACE_ID: &str = "provider-admission-space";

fn run(
    home: &TempHome,
    cwd: &Path,
    fake_bin: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args(args)
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env("PATH", path);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("run harness")
}

fn ok(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn succeeds(output: Output) {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn project_id_for_path(home: &TempHome, path: &Path) -> String {
    let expected = std::fs::canonicalize(path).unwrap();
    let registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.registry_path()).unwrap()).unwrap();
    registry["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["path"]
                .as_str()
                .is_some_and(|registered| Path::new(registered) == expected)
        })
        .and_then(|entry| entry["id"].as_str())
        .unwrap()
        .to_string()
}

fn install_version_probe(home: &TempHome) -> PathBuf {
    let bin = home.base().join("fakebin-provider-admit");
    std::fs::create_dir_all(&bin).unwrap();
    let shim = bin.join("codex");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo 'codex-cli {VERSION}'\n  exit 0\nfi\nexit 2\n"
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).unwrap();
    }
    bin
}

fn admission_args() -> [&'static str; 15] {
    [
        "provider",
        "admit",
        "--provider",
        "codex",
        "--execution-mode",
        "codex_app_server",
        "--provider-version",
        VERSION,
        "--adapter-contract-version",
        "codex-app-server-v1",
        "--evidence",
        "evidence:selection-regression",
        "--actor",
        "operator:test",
        "--json",
    ]
}

fn rows(path: &Path) -> Vec<serde_json::Value> {
    if !path.is_file() {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn setup_mismatched_active_context(home: &TempHome, fake_bin: &Path) -> (String, String) {
    let project_a = home.base().join("project-a");
    let project_b = home.base().join("project-b");
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();

    succeeds(run(home, &project_a, fake_bin, &["init"], &[]));
    let project_a_id = project_id_for_path(home, &project_a);
    succeeds(run(
        home,
        &project_b,
        fake_bin,
        &["project", "add", project_b.to_str().unwrap()],
        &[],
    ));
    let project_b_id = project_id_for_path(home, &project_b);
    succeeds(run(
        home,
        home.base(),
        fake_bin,
        &[
            "space",
            "init",
            "--id",
            SPACE_ID,
            "--project-binding",
            &project_b_id,
        ],
        &[],
    ));
    succeeds(run(
        home,
        home.base(),
        fake_bin,
        &["project", "switch", &project_a_id],
        &[],
    ));
    (project_a_id, project_b_id)
}

#[test]
fn execution_space_admission_requires_flag_and_scopes_exact_selected_binding() {
    let home = TempHome::new("provider-admit-space-selection");
    let fake_bin = install_version_probe(&home);
    let (project_a, project_b) = setup_mismatched_active_context(&home, &fake_bin);
    let ledger = home
        .spaces_dir()
        .join(SPACE_ID)
        .join("provider_compatibility_admissions.jsonl");

    let omitted = run(&home, home.base(), &fake_bin, &admission_args(), &[]);
    assert!(!omitted.status.success());
    assert!(String::from_utf8_lossy(&omitted.stderr)
        .contains("requires an explicit global `--project <id|path>` flag"));
    assert!(rows(&ledger).is_empty(), "omitted flag wrote an admission");

    let from_env = run(
        &home,
        home.base(),
        &fake_bin,
        &admission_args(),
        &[("FIRM_PROJECT", &project_a)],
    );
    assert!(!from_env.status.success());
    assert!(rows(&ledger).is_empty(), "FIRM_PROJECT authorized a write");

    let mut invalid = vec!["--project", "missing-binding"];
    invalid.extend(admission_args());
    assert!(!run(&home, home.base(), &fake_bin, &invalid, &[])
        .status
        .success());
    assert!(
        rows(&ledger).is_empty(),
        "invalid binding wrote an admission"
    );

    let mut admit_a = vec!["--project", project_a.as_str()];
    admit_a.extend(admission_args());
    let admitted_a = ok(run(&home, home.base(), &fake_bin, &admit_a, &[]));
    assert_eq!(admitted_a["admission"]["project_id"], project_a);
    assert_eq!(
        admitted_a["admission"]["store_id"],
        format!("execution-space:{SPACE_ID}")
    );

    let provider_report = ok(run(
        &home,
        home.base(),
        &fake_bin,
        &[
            "--space",
            SPACE_ID,
            "--project",
            &project_a,
            "member",
            "providers",
        ],
        &[],
    ));
    let codex = provider_report
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["provider"] == "codex")
        .unwrap();
    assert_eq!(codex["operational_compatibility"]["allowed"], true);
    assert_eq!(
        codex["operational_compatibility"]["source"],
        "operational_admission"
    );

    let mut admit_b = vec!["--project", project_b.as_str()];
    admit_b.extend(admission_args());
    let admitted_b = ok(run(&home, home.base(), &fake_bin, &admit_b, &[]));
    assert_eq!(admitted_b["admission"]["project_id"], project_b);
    let stored = rows(&ledger);
    assert_eq!(stored.len(), 2);
    assert!(stored.iter().any(|row| row["project_id"] == project_a));
    assert!(stored.iter().any(|row| row["project_id"] == project_b));
}

#[test]
fn unambiguous_project_store_keeps_ambient_project_compatibility() {
    let home = TempHome::new("provider-admit-project-store");
    let fake_bin = install_version_probe(&home);
    let project = home.base().join("project-only");
    std::fs::create_dir_all(&project).unwrap();
    succeeds(run(
        &home,
        &project,
        &fake_bin,
        &["project", "add", "--switch"],
        &[],
    ));
    let project_id = project_id_for_path(&home, &project);

    let admission = ok(run(&home, &project, &fake_bin, &admission_args(), &[]));
    assert_eq!(admission["admission"]["project_id"], project_id);
    assert_eq!(
        admission["admission"]["store_id"],
        format!("project-store:{project_id}")
    );
    assert_eq!(
        rows(
            &home
                .projects_dir()
                .join(&project_id)
                .join("provider_compatibility_admissions.jsonl")
        )
        .len(),
        1
    );
}
