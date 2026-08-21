//! Exact-session, read-only Claude CLI binding for headless Host turns.

use std::path::Path;
use std::time::Duration;

use harness_core::{LaunchPermission, LaunchSpec};

use crate::{run_claude_compatibility, ClaudeCompatibilityRun};

pub fn run_claude_host_turn(
    spec: &LaunchSpec,
    prompt: &str,
    system_prompt: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<ClaudeCompatibilityRun, String> {
    let expected_session = spec
        .resume
        .as_deref()
        .filter(|session| !session.trim().is_empty())
        .ok_or_else(|| "CLAUDE_HOST_EXACT_SESSION_REQUIRED".to_string())?;
    if spec.permission != LaunchPermission::ReadOnly {
        return Err("CLAUDE_HOST_READ_ONLY_REQUIRED".to_string());
    }
    let run = run_claude_compatibility(spec, prompt, system_prompt, cwd, timeout)?;
    if run.session_id.as_deref() != Some(expected_session) {
        return Err(format!(
            "CLAUDE_HOST_SESSION_DRIFT: expected {expected_session}, got {}",
            run.session_id.as_deref().unwrap_or("unavailable")
        ));
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_session_and_writable_permission_before_effect() {
        let spec = LaunchSpec {
            prompt_ref: None,
            message_content: "test".into(),
            model: None,
            effort: None,
            output_schema: None,
            permission: LaunchPermission::WorkspaceWrite,
            writable_roots: vec![],
            tools: vec![],
            workspace: None,
            mcp: None,
            skill_refs: vec![],
            resume: None,
            output: None,
        };
        let error =
            run_claude_host_turn(&spec, "test", "", Path::new("."), Duration::from_millis(1))
                .unwrap_err();
        assert_eq!(error, "CLAUDE_HOST_EXACT_SESSION_REQUIRED");
    }
}
