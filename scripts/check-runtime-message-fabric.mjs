import { readFileSync } from "node:fs";

const requiredSchemas = [
  "agent-identity",
  "agent-session",
  "team-membership",
  "work-execution-binding",
  "message",
  "message-subscription",
  "subscription-cursor",
  "canonical-message-delivery",
  "message-route-journal",
  "control-command-envelope",
  "runtime-command-record",
  "canonical-work-delivery",
  "provider-invocation",
];

const failures = [];
for (const name of requiredSchemas) {
  const path = `schemas/${name}.schema.json`;
  const schema = JSON.parse(readFileSync(path, "utf8"));
  if (schema.additionalProperties !== false) failures.push(`${path} must fail closed`);
}

const core = readFileSync("crates/firm-core/src/agentfirm_api.rs", "utf8");
const store = readFileSync("crates/firm-store/src/trust_kernel.rs", "utf8");
const daemon = readFileSync("crates/firm-cli/src/supervisor_daemon.rs", "utf8");
const server = readFileSync("crates/firm-cli/src/main.rs", "utf8");

for (const token of [
  "pub struct AgentIdentity",
  "pub struct AgentSession",
  "pub struct TeamMembership",
  "pub struct WorkExecutionBinding",
  "pub struct MessageSubscription",
  "pub struct SubscriptionCursor",
  "pub struct CanonicalMessageDelivery",
  "pub struct ControlCommandEnvelope",
  "pub struct RuntimeCommandRecord",
  "pub struct ProviderInvocation",
]) {
  if (!core.includes(token)) failures.push(`missing closed Rust contract: ${token}`);
}
for (const token of [
  "require_current_node_daemon_unlocked",
  "create_agent_session",
  "author_message",
  "claim_message_for_provider",
  "claim_work_for_provider",
  "prepare_runtime_command",
  "settle_runtime_command",
  "recipient identity has multiple current AgentSessions",
]) {
  if (!store.includes(token)) failures.push(`missing canonical Store authority: ${token}`);
}
if (!daemon.includes('"runtime" =>')) failures.push("NodeDaemon does not own RuntimeCommand admission");
if (!daemon.includes("runtime_command_via_socket")) failures.push("runtime command socket transport missing");
if (!server.includes('/v1/agentfirm/runtime-commands')) failures.push("authenticated HTTP runtime command route missing");
if (!server.includes("target_node_daemon_generation: lease.generation")) failures.push("server does not freeze current daemon generation");

const activeRuntimeSources = [
  "crates/firm-cli/src/main.rs",
  "crates/firm-cli/src/mcp.rs",
  "crates/firm-cli/src/supervisor_daemon.rs",
  "crates/firm-store/src/trust_kernel.rs",
  "crates/firm-core/src/agentfirm_api.rs",
];
for (const path of activeRuntimeSources) {
  const text = readFileSync(path, "utf8");
  if (text.includes("ProviderDispatchEnvelope")) {
    failures.push(`${path} retains the retired ProviderDispatchEnvelope contract`);
  }
}
if (server.includes("claim_round_triggering_messages_for") || server.includes("claim_next_work_for")) {
  failures.push("provider loops retain legacy TeamRun mailbox/work claim entry points");
}
for (const provider of ["codex", "claude", "kimi", "pi"]) {
  if (!server.includes(`\"provider\": \"${provider}\"`)) {
    failures.push(`missing durable RuntimeCommand settlement evidence for ${provider}`);
  }
}
for (const token of [
  "prepare_provider_process_effect",
  "prepare_provider_effect",
  "require_provider_session_authority",
  "RUNTIME_COMMAND_RECOVERY_REQUIRED",
  "RETIRED_RUNTIME_WRITER",
  "RETIRED_RUNTIME_READER",
]) {
  if (!server.includes(token)) failures.push(`missing executable hard-cutover fence: ${token}`);
}
if (readFileSync("crates/firm-store/src/lib.rs", "utf8").match(/pub fn append_team_message[\s\S]{0,450}RETIRED_RUNTIME_WRITER/g)?.length !== 2) {
  failures.push("retired Team message Store writer entry points are not both hard rejected");
}
try {
  readFileSync("schemas/provider-dispatch-envelope.schema.json", "utf8");
  failures.push("retired provider-dispatch-envelope schema still exists");
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("runtime/message fabric governance: PASS");
