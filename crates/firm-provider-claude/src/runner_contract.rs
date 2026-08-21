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
        let frame = json!({
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
        });
        validate_runner_frame("commandPayloadSchemas", "command", "payload", &frame)?;
        Ok(frame)
    }
}

pub(crate) fn validate_runner_frame(
    schema_set: &str,
    name_field: &str,
    payload_field: &str,
    frame: &Value,
) -> CliResult<()> {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../apps/claude-member-runner/contract/runner-v1.json"
    ))
    .map_err(|error| {
        CliError::Usage(format!(
            "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: invalid embedded runner contract: {error}"
        ))
    })?;
    let name = frame
        .get(name_field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: frame lacks {name_field}"
            ))
        })?;
    let schema = contract
        .get(schema_set)
        .and_then(|schemas| schemas.get(name))
        .ok_or_else(|| {
            CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: contract lacks {schema_set} schema for {name}"
            ))
        })?;
    validate_schema(
        schema,
        frame.get(payload_field).unwrap_or(&Value::Null),
        &format!("{name}.{payload_field}"),
    )
}

fn validate_schema(schema: &Value, value: &Value, path: &str) -> CliResult<()> {
    let Some(schema_object) = schema.as_object() else {
        return Err(CliError::Usage(format!(
            "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: invalid schema at {path}"
        )));
    };
    if schema_object.is_empty() {
        return Ok(());
    }
    let types = match schema.get("type") {
        Some(Value::String(value)) => vec![value.as_str()],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !types
        .iter()
        .any(|expected| value_matches_type(value, expected))
    {
        return Err(CliError::Usage(format!(
            "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: {path} has wrong payload type"
        )));
    }
    if value.is_null() {
        return Ok(());
    }
    if let Some(items) = value.as_array() {
        let item_schema = schema.get("items").ok_or_else(|| {
            CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: array schema at {path} lacks items"
            ))
        })?;
        for (index, item) in items.iter().enumerate() {
            validate_schema(item_schema, item, &format!("{path}[{index}]"))?;
        }
        return Ok(());
    }
    if let Some(object) = value.as_object() {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: object schema at {path} lacks properties"
                ))
            })?;
        for required in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !object.contains_key(required) {
                return Err(CliError::Usage(format!(
                    "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: {path} lacks required {required}"
                )));
            }
        }
        for (key, item) in object {
            if let Some(property_schema) = properties.get(key) {
                validate_schema(property_schema, item, &format!("{path}.{key}"))?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(CliError::Usage(format!(
                    "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: {path} has unknown property {key}"
                )));
            }
        }
    }
    Ok(())
}

fn value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "null" => value.is_null(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
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
