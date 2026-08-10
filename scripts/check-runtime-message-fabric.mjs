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
  "provider-dispatch-envelope",
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
]) {
  if (!core.includes(token)) failures.push(`missing closed Rust contract: ${token}`);
}
for (const token of [
  "require_current_node_daemon_unlocked",
  "create_agent_session",
  "author_message",
  "claim_message_for_provider",
  "recipient identity has multiple current AgentSessions",
]) {
  if (!store.includes(token)) failures.push(`missing canonical Store authority: ${token}`);
}
if (!daemon.includes('"runtime" =>')) failures.push("NodeDaemon does not own RuntimeCommand admission");
if (!daemon.includes("runtime_command_via_socket")) failures.push("runtime command socket transport missing");
if (!server.includes('/v1/agentfirm/runtime-commands')) failures.push("authenticated HTTP runtime command route missing");
if (!server.includes("target_node_daemon_generation: lease.generation")) failures.push("server does not freeze current daemon generation");

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("runtime/message fabric governance: PASS");
