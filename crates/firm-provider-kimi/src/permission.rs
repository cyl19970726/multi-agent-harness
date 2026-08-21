use harness_core::agentfirm_api::PermissionCeiling;

use crate::{KimiError, KimiResult};

pub fn compile_acp_permission(
    ceiling: PermissionCeiling,
) -> KimiResult<(&'static str, &'static str)> {
    match ceiling {
        PermissionCeiling::FullAccess => Ok(("provider-native-full-access", "exact_allow")),
        PermissionCeiling::ReadOnly | PermissionCeiling::WorkspaceWrite => Err(KimiError::Usage(
            format!("Kimi ACP cannot prove the {ceiling:?} permission ceiling"),
        )),
    }
}
