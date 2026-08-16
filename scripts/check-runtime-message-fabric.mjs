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
  return stripCfgItems(
    stripCfgItems(
      stripCfgItems(readFileSync(path, "utf8"), "any()"),
      "test",
    ),
    'any(test, feature = "test-support")',
  );
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
const providerAdapter = readFileSync("crates/firm-cli/src/provider_adapter.rs", "utf8");
const providerRuntimeSources = {
  CodexDeferredNativeControl: productionRust("crates/firm-cli/src/codex_team_runtime.rs"),
  ClaudeControlFlags: productionRust("crates/firm-cli/src/claude_team_runtime.rs"),
  KimiNeutralNativeControl: productionRust("crates/firm-cli/src/kimi_team_runtime.rs"),
};

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
for (const token of [
  "RuntimeStartSessionIntent",
  "runtime_control_actor_is_authorized",
  "caller-selected StartSession Node does not match the server-resolved local machine Node",
  "provider_availability(provider_kind)",
]) {
  if (!server.includes(token)) failures.push(`runtime HTTP authority is missing: ${token}`);
}
for (const token of [
  "ProviderControlAction",
  "NativeControlPrimitive",
  "control_plan",
  "ProviderNativeControl",
  "execute_team_control",
  "settle_team_control",
  "PiNativeControl",
  "provider_availability",
  "PROVIDER_CONTROL_FAILED",
]) {
  if (!providerAdapter.includes(token)) failures.push(`provider conformance is not executable: ${token}`);
}
for (const [control, source] of Object.entries(providerRuntimeSources)) {
  if (!source.includes(`impl ProviderNativeControl for ${control}`)
      && !source.includes(`impl crate::provider_adapter::ProviderNativeControl for ${control}`)) {
    failures.push(`provider conformance is not executable: ${control}`);
  }
}
if (providerAdapter.includes("execute_control_plan")) {
  failures.push("provider conformance must use concrete adapters, not an injected control closure");
}
for (const leakedNativeControl of [
  'PromptControl::Cancel',
  'PromptControl::TerminateRuntime',
  '"command": "interrupt"',
]) {
  if (server.includes(leakedNativeControl)) {
    failures.push(`provider-native control leaked outside the closed adapter seam: ${leakedNativeControl}`);
  }
}
if (!store.includes("AgentSession RuntimeCommand requires exact self or exact machine NodeDaemon/Operator authority; Team Host authority is Team-scoped only")) {
  failures.push("Store does not enforce machine-scoped AgentSession control authority under lock");
}
if (!store.includes("AgentSession stop requires explicit release, rebind, or quiesce of active WorkExecutionBindings first")) {
  failures.push("Store does not fence StopSession from active WorkExecutionBindings");
}
if (!store.includes("StartSession cannot widen the frozen AgentIdentity permission ceiling")) {
  failures.push("Store does not enforce the AgentIdentity permission ceiling");
}
if (!store.includes("resolve_runtime_command_recovery")) {
  failures.push("Operator RecoveryRequired resolution authority is missing");
}

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
const legacyStore = productionRust("crates/firm-store/src/lib.rs");
if (legacyStore.match(/pub fn append_team_message_checked[\s\S]{0,450}RETIRED_RUNTIME_WRITER/g)?.length !== 1) {
  failures.push("retired manual TeamMessage Store seam is not a single fail-closed entry point");
}
if (/fn record_provider_interaction_response\s*\(/.test(legacyStore)) {
  failures.push("retired provider-interaction response writer remains production-executable");
}
if (/append_jsonl(?:_unlocked)?\s*\(\s*"team_messages\.jsonl"/.test(legacyStore)) {
  failures.push("production Store retains a team_messages.jsonl mutator; the ledger is Legacy archive-only");
}
if (/read_jsonl\s*::<\s*TeamMessageProjection\s*>\s*\(\s*"team_messages\.jsonl"/.test(legacyStore)) {
  failures.push("production Store retains an implicit team_messages.jsonl reader; expose history only through an explicit Legacy archive seam");
}
if (/pub fn team_messages\s*\(/.test(legacyStore)) {
  failures.push("Store exposes ambiguous team_messages(); the historical reader must be named legacy_team_messages()");
}
if (!/pub fn legacy_team_messages\s*\(/.test(legacyStore)) {
  failures.push("Store is missing the explicitly named read-only legacy_team_messages() archive seam");
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
const memberTrustTransport = productionRust("crates/firm-cli/src/agentfirm_api.rs");
if (memberTrustTransport.includes("CreateMemberRun")
    || memberTrustTransport.includes('path.ends_with("/member-runs")')) {
  failures.push("HTTP Member Trust transport still exposes standalone MemberRun creation");
}
for (const lifecycleToken of [
  "transition_current_team_member_lifecycle",
  "CurrentTeamMemberLifecycleTransition::Close",
  "CurrentTeamMemberLifecycleTransition::Reopen",
  "CurrentTeamMemberLifecycleTransition::Retire",
  "CurrentTeamMemberLifecycleTransition::ResumeNativeSession",
]) {
  if (!memberTrustTransport.includes(lifecycleToken)) {
    failures.push(`current Member lifecycle transport bypasses combined TeamRun authority: ${lifecycleToken}`);
  }
}
for (const retiredLifecycleCall of [
  ".transition_trust_member_run(",
  ".resume_trust_native_session(",
]) {
  if (memberTrustTransport.includes(retiredLifecycleCall)) {
    failures.push(`current Member lifecycle transport retains canonical-only mutation: ${retiredLifecycleCall}`);
  }
}
const trustKernelProduction = productionRust("crates/firm-store/src/trust_kernel.rs");
const runtimeStoreProduction = productionRust("crates/firm-store/src/lib.rs");
const firmStoreCargo = readFileSync("crates/firm-store/Cargo.toml", "utf8");
const firmCliCargo = readFileSync("crates/firm-cli/Cargo.toml", "utf8");
const firmCliProductionDependencies = firmCliCargo.split("[dev-dependencies]")[0];
if (/default\s*=.*test-support/.test(firmStoreCargo)
    || firmCliProductionDependencies.includes('features = ["test-support"]')) {
  failures.push("firm-store test-support reconstruction seam is enabled by a production/default feature");
}
for (const productionOnlyTestSeam of [
  "legacy_import_create_trust_member_run_projection",
  "transition_trust_member_run",
  "resume_trust_native_session",
]) {
  if (trustKernelProduction.includes(`fn ${productionOnlyTestSeam}(`)) {
    failures.push(`firm-store production surface still compiles retired/test-only lifecycle writer: ${productionOnlyTestSeam}`);
  }
}
if (runtimeStoreProduction.includes("fn legacy_import_append_member_run_projection(")) {
  failures.push("firm-store production surface still compiles the Legacy raw MemberRun reconstruction writer");
}
if (!mcp.includes("const MCP_MEMBER_TRUST_COMMANDS")
    || mcp.includes('"create_member_run"')
    || !mcp.includes("MemberRun creation is available only through team_run_create or team_run_add_member")) {
  failures.push("MCP Member Trust inventory does not fail-close standalone MemberRun creation");
}
for (const removedTool of [
  "team_run_send_message",
  "team_message_acknowledge",
  "team_run_reconcile_delivery",
]) {
  if (mcp.includes(`\"name\": \"${removedTool}\"`) || mcp.includes(`\"${removedTool}\" =>`)) {
    failures.push(`${removedTool} remains advertised or dispatchable instead of failing closed as an unknown MCP tool`);
  }
}
for (const retiredReader of [
  "latest_team_messages_in_append_order",
  "has_actionable_delivered_manual_ack",
]) {
  if (productionRust("crates/firm-cli/src/mcp.rs").includes(retiredReader)) {
    failures.push(`MCP current status/inbox retains legacy TeamMessageProjection reader: ${retiredReader}`);
  }
}
for (const canonicalReader of ["fabric_messages", "fabric_message_deliveries", "message_summary"]) {
  if (!mcp.includes(canonicalReader)) {
    failures.push(`MCP current status lacks canonical Message-fabric visibility: ${canonicalReader}`);
  }
}
const mcpSpaceEnumerationCount = [...mcp.matchAll(/canonical_execution_space_ids/g)].length;
if (mcpSpaceEnumerationCount !== 0
    || !mcp.includes("fn mcp_team_run_execution_space_id")
    || !mcp.includes("current_team_run_execution_space(run)")) {
  failures.push("MCP must delegate exact TeamRun scope resolution to the locked Store authority without enumerating physical Spaces");
}
for (const currentProjection of [
  "fn tool_team_run_board_summary",
  "fn tool_team_run_status",
  "fn tool_team_run_events",
]) {
  const start = mcp.indexOf(currentProjection);
  const end = mcp.indexOf("\nfn ", start + currentProjection.length);
  const body = start >= 0 ? mcp.slice(start, end > start ? end : undefined) : "";
  if (!body.includes("require_current_team_run(store")) {
    failures.push(`${currentProjection} bypasses strict whole-TeamRun completeness`);
  }
}
const summaryStart = mcp.indexOf("fn canonical_message_summary_for_run");
const summaryEnd = mcp.indexOf("fn mcp_team_run_execution_space_id", summaryStart);
const summaryBody = summaryStart >= 0 && summaryEnd > summaryStart
  ? mcp.slice(summaryStart, summaryEnd)
  : "";
if (!summaryBody.includes("execution_space_id: &str")
    || summaryBody.includes("canonical_execution_space_ids")
    || !summaryBody.includes("fabric_messages(execution_space_id)")
    || !summaryBody.includes("fabric_message_deliveries(execution_space_id)")) {
  failures.push("MCP TeamRun message summary is not bound to one resolved canonical Execution Space");
}
for (const authorityToken of ["current_team_run_execution_space(run)", "EXECUTION_SPACE_SCOPE_MISMATCH"]) {
  if (!mcp.includes(authorityToken)) {
    failures.push(`MCP TeamRun Execution Space resolver is missing: ${authorityToken}`);
  }
}
const roleViewTransport = productionRust("crates/firm-cli/src/role_views_api.rs");
for (const roleViewScopeToken of [
  "current_team_run_execution_space(&run)",
  "resolved_space == space_id",
  'member_run["coordination_status"] == "active"',
  'Some("disconnected" | "failed" | "stopped")',
]) {
  if (!roleViewTransport.includes(roleViewScopeToken)) {
    failures.push(`RoleView current TeamRun projection bypasses strict exact-space authority: ${roleViewScopeToken}`);
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
const legacyTeamMessageLedgerMatches = [
  ...legacyExport.matchAll(/team_messages\.jsonl/g),
].length;
if (legacyTeamMessageLedgerMatches !== 1 || !legacyExport.includes('ledger: "team_messages.jsonl"')) {
  failures.push("team_messages historical allowlist must be exactly one explicit read-only Legacy export entry");
}
for (const path of activeRuntimeSources) {
  if (productionRust(path).includes("provider_dispatch_events.jsonl")) {
    failures.push(`${path} retains current provider_dispatch ledger authority`);
  }
}

// No current lineage, status, detail, inbox, or replay projection may consult
// the retired append-only TeamMessageProjection ledger. Explicit Legacy export
// is the sole historical read path. Disabled tests and frozen historical source
// are stripped before this production audit.
for (const path of [
  "crates/firm-cli/src/main.rs",
  "crates/firm-cli/src/mcp.rs",
  "crates/firm-cli/src/supervisor_daemon.rs",
  "crates/firm-cli/src/fabric_runtime.rs",
  "crates/firm-cli/src/role_actions_api.rs",
  "crates/firm-cli/src/role_views_api.rs",
]) {
  const text = productionRust(path);
  for (const retiredReader of [
    "latest_team_messages_in_append_order",
    "store.legacy_team_messages()",
    "store.legacy_team_messages()?",
    'read_jsonl::<TeamMessageProjection>("team_messages.jsonl")',
  ]) {
    if (text.includes(retiredReader)) {
      failures.push(`${path} retains a current team_messages.jsonl lineage/status/detail/replay reader: ${retiredReader}`);
    }
  }
}

const runtimeDoc = readFileSync("docs/current/architecture/agent-runtime.md", "utf8");
const rootRules = readFileSync("AGENTS.md", "utf8");
for (const token of [
  "AgentIdentity",
  "AgentSession",
  "TeamMembership",
  "WorkExecutionBinding",
  "MessageSubscription",
  "CanonicalMessageDelivery",
  "RuntimeCommand",
  "ProviderInvocation",
  "RecoveryRequired",
]) {
  if (!runtimeDoc.includes(token)) failures.push(`canonical runtime doc missing ${token}`);
}
if (!runtimeDoc.includes("Team `close-member` closes only that")) {
  failures.push("canonical runtime doc does not separate Team close from machine Session stop");
}
for (const token of [
  "AgentIdentity -> AgentSession",
  "Work -> WorkExecutionBinding",
  "Message -> MessageSubscription",
  "NodeDaemon -> durable RuntimeCommand",
]) {
  if (!rootRules.includes(token)) failures.push(`AGENTS.md runtime model drifted: ${token}`);
}
for (const path of [
  "skills/collaborate-as-agent-team-member/SKILL.md",
  "skills/orchestrate-mission-waves/SKILL.md",
  "plugins/star-harness/skills/collaborate-as-agent-team-member/SKILL.md",
  "plugins/star-harness/skills/orchestrate-mission-waves/SKILL.md",
]) {
  const text = readFileSync(path, "utf8");
  if (text.includes("team-run send") || text.includes("team-run ack")) {
    failures.push(`${path} advertises a retired caller-selected message authority`);
  }
  if (!text.includes("server-built")) {
    failures.push(`${path} does not require server-built identity/runtime authority`);
  }
}

const legacyCliSendTests = [
  "crates/firm-cli/tests/team_run_api.rs",
  "crates/firm-cli/tests/pi_team_member.rs",
  "crates/firm-cli/tests/claude_agent_sdk_member.rs",
];
for (const path of legacyCliSendTests) {
  const executable = stripCfgItems(readFileSync(path, "utf8"), "any()");
  if (executable.includes('"team-run", "send"')) {
    failures.push(`${path} retains an executable positive legacy CLI-send test seam`);
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("runtime/message fabric governance: PASS");
