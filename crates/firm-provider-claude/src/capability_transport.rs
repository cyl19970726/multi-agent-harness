use harness_runtime_contract::CollaborationCapabilityMechanism;

pub const COLLABORATION_CAPABILITY_MECHANISM: CollaborationCapabilityMechanism =
    CollaborationCapabilityMechanism::ClaudeSdkToolEnvironment;

pub fn collaboration_agent_tool_environment(
    envelope: &harness_runtime_contract::CollaborationCapabilityEnvelope,
) -> Result<
    harness_runtime_contract::CollaborationCapabilityEnvironment,
    harness_runtime_contract::CollaborationCapabilityError,
> {
    envelope.agent_tool_environment(COLLABORATION_CAPABILITY_MECHANISM)
}

pub(crate) fn apply_collaboration_environment(
    command: &mut std::process::Command,
    environment: &harness_runtime_contract::CollaborationCapabilityEnvironment,
) {
    command.envs(
        environment
            .as_pairs()
            .iter()
            .map(|(key, value)| (key, value)),
    );
}
