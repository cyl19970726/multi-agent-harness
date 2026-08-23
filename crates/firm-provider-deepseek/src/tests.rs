use super::*;
use harness_core::agentfirm_api::PermissionCeiling;

#[test]
fn runner_package_and_contract_are_exactly_version_bound() {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/deepseek-member-runner/bin/deepseek-member-runner.mjs");
    verify_runner_harness_version(&runner).expect("reviewed DSH package pin");
    let contract: Value = serde_json::from_str(include_str!(
        "../../../apps/deepseek-member-runner/contract/runner-v1.json"
    ))
    .expect("runner contract");
    assert_eq!(contract["protocolVersion"], DEEPSEEK_NATIVE_PROTOCOL);
    assert_eq!(
        contract["commands"],
        serde_json::json!(["start", "deliver", "interrupt", "close"])
    );
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
