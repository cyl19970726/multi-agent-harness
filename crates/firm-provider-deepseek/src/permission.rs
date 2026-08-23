use harness_core::agentfirm_api::PermissionCeiling;

pub fn compile_harness_permission(ceiling: PermissionCeiling) -> (&'static str, &'static str) {
    match ceiling {
        PermissionCeiling::ReadOnly => ("read-only", "dsh-sandbox-policy"),
        PermissionCeiling::WorkspaceWrite => ("workspace-write", "dsh-sandbox-policy"),
        PermissionCeiling::FullAccess => ("danger-full-access", "dsh-sandbox-policy"),
    }
}
