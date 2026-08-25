import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");
const failures = [];
const requireText = (relative, pattern, message) => {
  const text = read(relative);
  if (!pattern.test(text)) failures.push(`${relative}: ${message}`);
};
const rejectText = (relative, pattern, message) => {
  const text = read(relative);
  if (pattern.test(text)) failures.push(`${relative}: ${message}`);
};

const providerCrates = ["codex", "claude", "kimi", "pi", "deepseek"];
for (const provider of providerCrates) {
  const base = `crates/firm-provider-${provider}`;
  if (!fs.existsSync(path.join(root, base, "src"))) {
    failures.push(`${base}: missing production provider package`);
    continue;
  }
  rejectText(
    `${base}/Cargo.toml`,
    /firm-(?:cli|store|application)\s*=/,
    "provider packages must not depend on CLI, durable stores, or application orchestration",
  );
}

rejectText(
  "crates/firm-runtime-contract/Cargo.toml",
  /firm-provider-|firm-cli|firm-store|firm-application/,
  "runtime contract must remain provider- and composition-neutral",
);

const runtimeContractRoot = "crates/firm-runtime-contract/src";
const runtimeContractLib = `${runtimeContractRoot}/lib.rs`;
const runtimeContractModules = new Map([
  ["cycle", [
    "pub struct CycleRuntimeObservation",
    "pub struct ExecutionCycleOutcome",
    "pub struct CycleControl",
    "pub trait TeamRuntimeAdapter",
  ]],
  ["control", [
    "pub enum ProviderControlAction",
    "pub enum NativeControlPrimitive",
    "pub struct ProviderControlPlan",
    "pub trait ProviderNativeControl",
    "pub enum ControlIntent",
    "pub struct ControlRequest",
  ]],
  ["provider_capabilities", [
    "pub enum CapabilityStatus",
    "pub struct CapabilityBinding",
    "pub enum SemanticCapability",
    "pub struct CapabilityResolver",
    "pub struct RuntimeBindingFence",
  ]],
  ["receipt_and_terminal", [
    "pub struct ProviderTerminalFailure",
    "pub struct EffectReceipt",
    "pub struct RuntimeObservation",
    "pub struct QuiesceReceipt",
    "pub struct ReleaseReceipt",
    "pub struct MemberRuntimeCloseReceipt",
  ]],
  ["collaboration_capability", [
    "pub struct CollaborationCapabilityEnvelope",
    "pub struct CollaborationCapabilityBinding",
  ]],
  ["conformance", [
    "pub trait RuntimeAdapter",
    "pub fn preflight_effect",
    "pub struct CompositionLifecycle",
    "pub struct OneShotDisposer",
    "pub enum RuntimeContractError",
  ]],
]);
const runtimeContractSources = fs
  .readdirSync(path.join(root, runtimeContractRoot), { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
  .map((entry) => `${runtimeContractRoot}/${entry.name}`);
const runtimeContractProductionSources = runtimeContractSources.filter(
  (source) => !source.endsWith("/tests.rs"),
);
const runtimeContractLibText = read(runtimeContractLib);
for (const moduleName of runtimeContractModules.keys()) {
  const modulePath = `${runtimeContractRoot}/${moduleName}.rs`;
  if (!runtimeContractProductionSources.includes(modulePath)) {
    failures.push(`${modulePath}: missing runtime-contract responsibility module`);
    continue;
  }
  for (const rootToken of [`mod ${moduleName};`, `pub use ${moduleName}::*;`]) {
    if (!runtimeContractLibText.includes(rootToken)) {
      failures.push(`${runtimeContractLib}: missing stable module surface ${rootToken}`);
    }
  }
}
if (/^pub (?:struct|enum|trait|fn) /m.test(runtimeContractLibText)) {
  failures.push(
    `${runtimeContractLib}: crate root must remain a module/re-export surface`,
  );
}
for (const [moduleName, ownedDefinitions] of runtimeContractModules) {
  const ownerPath = `${runtimeContractRoot}/${moduleName}.rs`;
  for (const definition of ownedDefinitions) {
    const owners = runtimeContractProductionSources.filter((source) =>
      read(source).includes(definition),
    );
    if (owners.length !== 1 || owners[0] !== ownerPath) {
      failures.push(
        `${definition}: expected sole owner ${ownerPath}, found ${owners.join(", ") || "none"}`,
      );
    }
  }
}
for (const source of runtimeContractProductionSources) {
  rejectText(
    source,
    /(?:firm|harness)_(?:provider|store|application|cli)/,
    "provider-neutral runtime contract must not import provider, Store, application, or CLI implementation",
  );
}
rejectText(
  "crates/firm-runtime-supervisor/Cargo.toml",
  /firm-provider-|firm-cli|firm-store/,
  "supervisor must depend only on neutral contracts",
);
requireText(
  "crates/firm-application/src/lib.rs",
  /mod team_runtime_policy;/,
  "application package must own Team round policy instead of leaving it in CLI",
);
requireText(
  "crates/firm-application/src/provider_catalog.rs",
  /PROVIDERS: \[ProviderDescriptor; 5\]/,
  "canonical provider catalog must remain closed over the five production providers",
);
requireText(
  "crates/firm-provider-claude/src/runner_contract.rs",
  /runner-v1\.json/,
  "Claude Rust binding must consume the shared versioned runner contract",
);
requireText(
  "apps/claude-member-runner/src/protocol.mjs",
  /runner-v1\.json/,
  "Claude Node runner must consume the same versioned contract before SDK loading",
);
requireText(
  "crates/firm-provider-deepseek/src/runner_contract.rs",
  /runner-v1\.json/,
  "DeepSeek Harness Rust binding must consume the shared versioned runner contract",
);
requireText(
  "apps/deepseek-member-runner/src/member-runner.mjs",
  /runner-v1\.json/,
  "DeepSeek Harness runner must consume the same versioned contract before plugin loading",
);

const capabilityContract = "crates/firm-runtime-contract/src/collaboration_capability.rs";
for (const required of [
  "pub struct CollaborationCapabilityEnvelope",
  "pub struct CollaborationCapabilitySecret",
  "pub struct CollaborationCapabilityBinding",
  "pub member_run_generation: u64",
  "pub struct CollaborationCapabilityEnvironment",
  "pub enum CollaborationCapabilityMechanism",
  "LiveSupervisorRegistration",
]) {
  requireText(
    capabilityContract,
    new RegExp(required.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    `provider-neutral collaboration capability contract is missing ${required}`,
  );
}
rejectText(
  capabilityContract,
  /derive\([^)]*Serialize[^)]*\)\s*\n(?:pub\s+)?struct CollaborationCapability(?:Secret|Envelope)/,
  "collaboration capability secret/envelope must never be serializable",
);
rejectText(
  "crates/firm-cli/src/main_modules/runtime_effects.rs",
  /pub\(super\) role_action_token: String/,
  "live provider registry must retain only a non-secret capability fingerprint",
);
requireText(
  "crates/firm-cli/src/main_modules/supervisor_control.rs",
  /latest_node_daemon_lease/,
  "every Role Action must revalidate the current NodeDaemon lease",
);
for (const provider of providerCrates) {
  const transport = `crates/firm-provider-${provider}/src/capability_transport.rs`;
  requireText(
    transport,
    /COLLABORATION_CAPABILITY_MECHANISM/,
    `${provider} must declare its reviewed agent-tool capability mechanism`,
  );
  requireText(
    transport,
    /collaboration_agent_tool_environment/,
    `${provider} must compile the capability at its owned agent-tool boundary`,
  );
}
for (const provider of ["claude", "deepseek"]) {
  const providerLib = `crates/firm-provider-${provider}/src/lib.rs`;
  requireText(
    providerLib,
    /pub environment: harness_runtime_contract::CollaborationCapabilityEnvironment/,
    `${provider} spawn config must retain the protected collaboration environment wrapper`,
  );
  rejectText(
    providerLib,
    /#\[derive\(Debug, Clone\)\]\s*pub struct (?:Claude|DeepSeek)TeamRuntimeConfig/,
    `${provider} spawn config carrying the bearer environment must not be Clone`,
  );
}
const providerRunners = read("crates/firm-cli/src/main_modules/provider_runners.rs")
  + read("crates/firm-cli/src/main_modules/pi_runner_state.rs");
for (const provider of providerCrates) {
  if (!providerRunners.includes(`harness_provider_${provider}::collaboration_agent_tool_environment`)) {
    failures.push(`provider composition: ${provider} bypasses its owned collaboration capability boundary`);
  }
}
requireText(
  "crates/firm-provider-codex/src/lib.rs",
  /mcp_servers\.harness\.enabled=false/,
  "managed Codex must keep Harness MCP mutations disabled",
);
requireText(
  "apps/deepseek-member-runner/src/member-role-action-env.mjs",
  /execution\.agent\s*\?/,
  "DeepSeek shellEnv must expose the capability only to agent executions",
);

for (const retiredCliNativeFile of [
  "crates/firm-cli/src/main_modules/provider_ephemeral.rs",
  "crates/firm-cli/src/main_modules/resident.rs",
]) {
  if (fs.existsSync(path.join(root, retiredCliNativeFile))) {
    failures.push(`${retiredCliNativeFile}: native provider implementation belongs in a provider package`);
  }
}

const mcpTools = read("crates/firm-cli/src/mcp/tool_definitions.rs");
for (const mode of ["codex_app_server", "claude_agent_sdk", "kimi_acp", "pi_rpc", "deepseek_sdk"]) {
  if (!mcpTools.includes(`\"${mode}\"`)) {
    failures.push(`crates/firm-cli/src/mcp/tool_definitions.rs: missing current Team mode ${mode}`);
  }
}

if (failures.length) {
  console.error("Provider runtime package boundary check failed:\n");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log("Provider runtime package boundaries: 5 providers, neutral contracts, application policy, and CLI composition verified.");
