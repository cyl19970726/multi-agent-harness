use harness_core::agentfirm_api::PermissionCeiling;

pub fn compile_node_permission(ceiling: PermissionCeiling) -> (&'static str, &'static str) {
    match ceiling {
        PermissionCeiling::ReadOnly => ("read-only", "never"),
        PermissionCeiling::WorkspaceWrite => ("workspace-write", "never"),
        PermissionCeiling::FullAccess => ("danger-full-access", "never"),
    }
}
