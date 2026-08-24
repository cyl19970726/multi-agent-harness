use harness_runtime_contract::CollaborationCapabilityMechanism;

pub const COLLABORATION_CAPABILITY_MECHANISM: CollaborationCapabilityMechanism =
    CollaborationCapabilityMechanism::DirectAgentToolEnvironment;

pub fn collaboration_agent_tool_environment(
    envelope: &harness_runtime_contract::CollaborationCapabilityEnvelope,
) -> Result<
    harness_runtime_contract::CollaborationCapabilityEnvironment,
    harness_runtime_contract::CollaborationCapabilityError,
> {
    envelope.agent_tool_environment(COLLABORATION_CAPABILITY_MECHANISM)
}
