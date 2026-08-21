use super::*;

#[test]
fn workflow_child_store_guard_isolates_nested_harness_by_default() {
    let session_dir =
        std::env::temp_dir().join(format!("harness-child-env-{}", generated_id("guard")));
    let mut cmd = Command::new("harness");
    cmd.env("HARNESS_PROJECT", "real-project");

    apply_workflow_child_store_guard(&mut cmd, &session_dir, false);

    let envs: BTreeMap<String, Option<String>> = cmd
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().to_string(),
                value.map(|v| v.to_string_lossy().to_string()),
            )
        })
        .collect();

    assert_eq!(
        envs.get(HARNESS_WORKFLOW_CHILD_STORE_ROOT_ENV)
            .cloned()
            .flatten(),
        Some(
            workflow_child_store_root(&session_dir)
                .to_string_lossy()
                .to_string()
        )
    );
    assert_eq!(
        envs.get("HARNESS_HOME").cloned().flatten(),
        Some(
            workflow_child_firm_home(&session_dir)
                .to_string_lossy()
                .to_string()
        )
    );
    assert_eq!(
        envs.get("HARNESS_WORKFLOW_STORE_GUARD")
            .and_then(|v| v.as_deref()),
        Some("isolated")
    );
    assert!(
        matches!(envs.get("HARNESS_PROJECT"), Some(None)),
        "project selector must be removed so the child store guard wins"
    );
}
