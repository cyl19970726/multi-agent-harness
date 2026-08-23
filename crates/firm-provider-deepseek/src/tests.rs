use super::*;
use harness_core::agentfirm_api::PermissionCeiling;
use std::fs;

fn reviewed_runner_fixture(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "deepseek-runner-composition-{label}-{}-{}",
        std::process::id(),
        now_string().replace(':', "-")
    ));
    fs::create_dir_all(root.join("bin")).expect("runner bin");
    fs::write(root.join("bin/deepseek-member-runner.mjs"), "// fixture").expect("runner fixture");
    fs::write(
        root.join("package.json"),
        include_str!("../../../apps/deepseek-member-runner/package.json"),
    )
    .expect("package fixture");
    fs::write(
        root.join("cordis.yml"),
        include_str!("../../../apps/deepseek-member-runner/cordis.yml"),
    )
    .expect("composition fixture");
    let reviewed = embedded_reviewed_provider().expect("reviewed provider");
    for (name, version) in reviewed["dependencies"]
        .as_object()
        .expect("reviewed dependencies")
    {
        let package_dir = root.join("node_modules").join(name);
        fs::create_dir_all(&package_dir).expect("dependency fixture");
        fs::write(
            package_dir.join("package.json"),
            serde_json::to_vec(&json!({"name": name, "version": version})).unwrap(),
        )
        .expect("dependency package fixture");
    }
    root
}

#[test]
fn runner_package_and_contract_are_exactly_version_bound() {
    let root = reviewed_runner_fixture("exact");
    let runner = root.join("bin/deepseek-member-runner.mjs");
    verify_runner_harness_composition(&runner).expect("reviewed DSH composition");
    let contract: Value = serde_json::from_str(include_str!(
        "../../../apps/deepseek-member-runner/contract/runner-v1.json"
    ))
    .expect("runner contract");
    assert_eq!(contract["protocolVersion"], DEEPSEEK_NATIVE_PROTOCOL);
    assert_eq!(
        contract["commands"],
        serde_json::json!(["start", "deliver", "interrupt", "close"])
    );
    assert_eq!(
        contract["reviewedProvider"]["sourceRevision"],
        REVIEWED_DEEPSEEK_SOURCE_REVISION
    );
    assert_eq!(
        contract["reviewedProvider"]["compositionFingerprint"],
        REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn dependency_and_cordis_drift_fail_before_runner_spawn() {
    let dependency_root = reviewed_runner_fixture("dependency-drift");
    let dependency_runner = dependency_root.join("bin/deepseek-member-runner.mjs");
    fs::write(
        dependency_root.join("node_modules/@deepseek-ai/dsh-sandbox-policy/package.json"),
        r#"{"name":"@deepseek-ai/dsh-sandbox-policy","version":"0.1.1-rc.3"}"#,
    )
    .expect("drift dependency");
    let dependency_error = verify_runner_harness_composition(&dependency_runner)
        .expect_err("security plugin drift must fail closed")
        .to_string();
    assert!(dependency_error.contains("DEEPSEEK_HARNESS_DEPENDENCY_UNREVIEWED"));
    fs::remove_dir_all(dependency_root).expect("remove dependency fixture");

    let composition_root = reviewed_runner_fixture("composition-drift");
    let composition_runner = composition_root.join("bin/deepseek-member-runner.mjs");
    fs::write(
        composition_root.join("cordis.yml"),
        "- id: unreviewed-plugin\n",
    )
    .expect("drift composition");
    let composition_error = verify_runner_harness_composition(&composition_runner)
        .expect_err("Cordis drift must fail closed")
        .to_string();
    assert!(composition_error.contains("DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED"));
    fs::remove_dir_all(composition_root).expect("remove composition fixture");
}

#[test]
fn session_binding_revalidates_source_and_composition_identity() {
    let exact = json!({
        "providerVersion": REVIEWED_DEEPSEEK_HARNESS_VERSION,
        "sourceRevision": REVIEWED_DEEPSEEK_SOURCE_REVISION,
        "compositionFingerprint": REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT
    });
    assert_eq!(
        verify_session_bound_provider_identity(&exact).expect("exact provider identity"),
        REVIEWED_DEEPSEEK_HARNESS_VERSION
    );

    let mut source_drift = exact.clone();
    source_drift["sourceRevision"] = json!("unreviewed-source");
    assert!(verify_session_bound_provider_identity(&source_drift)
        .expect_err("source drift")
        .to_string()
        .contains("DEEPSEEK_HARNESS_SOURCE_REVISION_UNREVIEWED"));

    let mut composition_drift = exact;
    composition_drift["compositionFingerprint"] = json!("sha256:unreviewed");
    assert!(verify_session_bound_provider_identity(&composition_drift)
        .expect_err("composition drift")
        .to_string()
        .contains("DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED"));
}

#[test]
fn permission_ceiling_compiles_into_the_shared_dsh_policy() {
    assert_eq!(
        compile_harness_permission(PermissionCeiling::ReadOnly),
        ("read-only", "dsh-sandbox-policy")
    );
    assert_eq!(
        compile_harness_permission(PermissionCeiling::WorkspaceWrite),
        ("workspace-write", "dsh-sandbox-policy")
    );
    assert_eq!(
        compile_harness_permission(PermissionCeiling::FullAccess),
        ("danger-full-access", "dsh-sandbox-policy")
    );
}
