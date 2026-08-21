use super::*;

#[test]
fn workflow_child_store_guard_respects_explicit_store_mutation_opt_in() {
    let session_dir =
        std::env::temp_dir().join(format!("harness-child-env-{}", generated_id("allow")));
    let mut cmd = Command::new("harness");
    cmd.env("HARNESS_PROJECT", "real-project");

    apply_workflow_child_store_guard(&mut cmd, &session_dir, true);

    let envs: BTreeMap<String, Option<String>> = cmd
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().to_string(),
                value.map(|v| v.to_string_lossy().to_string()),
            )
        })
        .collect();

    assert!(
        !envs.contains_key(HARNESS_WORKFLOW_CHILD_STORE_ROOT_ENV),
        "explicit opt-in must not inject the child store override"
    );
    assert_eq!(
        envs.get("HARNESS_PROJECT").and_then(|v| v.as_deref()),
        Some("real-project")
    );
    assert_eq!(
        envs.get("HARNESS_PARENT_WORKFLOW_SESSION_DIR")
            .cloned()
            .flatten(),
        Some(session_dir.to_string_lossy().to_string())
    );
}
