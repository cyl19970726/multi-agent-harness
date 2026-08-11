import { readFileSync } from "node:fs";

function stripCfgItems(source, cfgName) {
  const lines = source.split("\n");
  const kept = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].trim() !== `#[cfg(${cfgName})]`) {
      kept.push(lines[index]);
      continue;
    }
    // Skip adjacent attributes and the complete attributed Rust item. This is
    // deliberately syntax-light, but brace-balanced: the governance rule is
    // about production-executable authority, not broad token censorship of
    // frozen historical tests/source.
    index += 1;
    while (index < lines.length && lines[index].trim().startsWith("#[")) index += 1;
    let depth = 0;
    let opened = false;
    for (; index < lines.length; index += 1) {
      const line = lines[index];
      for (const character of line) {
        if (character === "{") {
          depth += 1;
          opened = true;
        } else if (character === "}") {
          depth -= 1;
        }
      }
      if (opened && depth === 0) break;
      if (!opened && line.trim().endsWith(";")) break;
    }
  }
  return kept.join("\n");
}

function productionRust(path) {
  return stripCfgItems(stripCfgItems(readFileSync(path, "utf8"), "any()"), "test");
}

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
  const text = productionRust(path);
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

const retiredWave4AMessageTokens = [
  "trust_team_messages",
  "trust_message_deliveries",
  "create_trust_team_message_with_deliveries",
  "claim_trust_message_delivery",
  "receive_trust_message_delivery",
  "acknowledge_trust_message_delivery",
  "reconcile_trust_message_delivery",
  "retry_trust_message_delivery",
];
for (const path of [
  "crates/firm-store/src/trust_kernel.rs",
  "crates/firm-cli/src/main.rs",
  "crates/firm-cli/src/mcp.rs",
  "crates/firm-cli/src/supervisor_daemon.rs",
]) {
  const text = productionRust(path);
  for (const token of retiredWave4AMessageTokens) {
    if (text.includes(token)) failures.push(`${path} retains production-executable Wave4A message authority: ${token}`);
  }
}
for (const functionName of retiredWave4AMessageTokens.slice(0, 2).concat(retiredWave4AMessageTokens.slice(2))) {
  const pattern = new RegExp(`#\\[cfg\\(any\\(\\)\\)\\]\\s+pub fn ${functionName}\\b`);
  if (!pattern.test(store)) failures.push(`retired Store seam ${functionName} is not explicitly quarantined`);
}

if (!server.includes('"send" => {\n            return Err(CliError::Usage(\n                "RETIRED_WRITE_AUTHORITY: team-run send')) {
  failures.push("team-run send CLI is not a hard-retired writer");
}
for (const [command, marker] of [
  ["ack", "team-run ack cannot authenticate the recipient session"],
  ["reconcile-delivery", "team-run reconcile-delivery cannot supply NodeDaemon delivery authority"],
]) {
  if (!server.includes(`"${command}" => {`) || !server.includes(marker)) {
    failures.push(`team-run ${command} CLI is not a hard-retired writer`);
  }
}
const mcp = readFileSync("crates/firm-cli/src/mcp.rs", "utf8");
for (const [tool, marker] of [
  ["team_run_send_message", "cannot select a sender identity"],
  ["team_message_acknowledge", "cannot authenticate the recipient session"],
  ["team_run_reconcile_delivery", "cannot supply target NodeDaemon authority"],
]) {
  if (!mcp.includes(`RETIRED_WRITE_AUTHORITY: ${tool} ${marker}`)) {
    failures.push(`${tool} MCP tool is not a hard-retired writer`);
  }
}
for (const routeToken of [
  'path_only == "/v1/messages"',
  'path_only.ends_with("/messages")',
  'path_only.contains("/messages/")',
  'path_only.starts_with("/v1/message-deliveries/")',
  '"code": "RETIRED_WRITE_AUTHORITY"',
]) {
  if (!server.includes(routeToken)) failures.push(`retired HTTP message route inventory is incomplete: ${routeToken}`);
}

const legacyExport = readFileSync("crates/firm-cli/src/legacy_export.rs", "utf8");
const providerDispatchLedgerMatches = [
  ...legacyExport.matchAll(/provider_dispatch_events\.jsonl/g),
].length;
if (providerDispatchLedgerMatches !== 1 || !legacyExport.includes('ledger: "provider_dispatch_events.jsonl"')) {
  failures.push("provider_dispatch_events historical allowlist must be exactly one read-only legacy export entry");
}
for (const path of activeRuntimeSources) {
  if (productionRust(path).includes("provider_dispatch_events.jsonl")) {
    failures.push(`${path} retains current provider_dispatch ledger authority`);
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("runtime/message fabric governance: PASS");
