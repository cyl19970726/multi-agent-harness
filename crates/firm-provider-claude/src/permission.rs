use harness_core::agentfirm_api::PermissionCeiling;

pub fn compile_agent_sdk_permission(ceiling: PermissionCeiling) -> (&'static str, &'static str) {
    match ceiling {
        PermissionCeiling::ReadOnly => ("plan", "default"),
        PermissionCeiling::WorkspaceWrite => ("acceptEdits", "default"),
        PermissionCeiling::FullAccess => ("unrestricted", "bypassPermissions"),
    }
}
