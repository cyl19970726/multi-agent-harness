use super::*;

impl ClaudeTeamRuntimeConfig {
    pub(crate) fn start_frame(&self) -> CliResult<Value> {
        let contract: Value = serde_json::from_str(include_str!(
            "../../../apps/claude-member-runner/contract/runner-v1.json"
        ))
        .map_err(|error| {
            CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: invalid embedded runner contract: {error}"
            ))
        })?;
        let protocol_version = required_contract_field(&contract, "protocolVersion")?;
        let protocol_fingerprint = required_contract_field(&contract, "fingerprint")?;
        Ok(json!({
            "command": "start",
            "payload": {
                "protocolVersion": protocol_version,
                "protocolFingerprint": protocol_fingerprint,
                "teamRunId": self.team_run_id,
                "memberRunId": self.member_run_id,
                "memberName": self.member_name,
                "roleLabel": self.role_label,
                "cwd": self.cwd.to_string_lossy(),
                "ownedPaths": self.owned_paths,
                "model": self.model,
                "effort": self.effort,
                "permissionMode": self.permission_mode,
                "allowedTools": self.allowed_tools,
                "disallowedTools": self.disallowed_tools,
                "settingSources": self.setting_sources,
                "resumeSessionId": self.resume_session_id,
            }
        }))
    }
}

fn required_contract_field<'a>(contract: &'a Value, field: &str) -> CliResult<&'a str> {
    contract
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: runner contract lacks {field}"
            ))
        })
}
