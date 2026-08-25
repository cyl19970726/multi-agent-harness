import { execFileSync } from "node:child_process";
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

const runtimeContractRoot = "crates/firm-runtime-contract/src";
const runtimeContractLib = `${runtimeContractRoot}/lib.rs`;
const runtimeContractModules = new Map([
  ["cycle", [
    "struct CycleRuntimeObservation",
    "struct ControlTransportReceipt",
    "struct NativeCycleCorrelation",
    "struct QuiesceOutcome",
    "struct ExecutionCycleOutcome",
    "enum SteerProviderResult",
    "struct SteerRequest",
    "struct CycleControl",
    "enum LiveProviderActivityKind",
    "trait TeamRuntimeAdapter",
  ]],
  ["control", [
    "enum ProviderControlAction",
    "enum NativeControlPrimitive",
    "struct ProviderControlPlan",
    "trait ProviderNativeControl",
    "struct RuntimeDescription",
    "enum ControlIntent",
    "struct ControlRequest",
  ]],
  ["provider_capabilities", [
    "enum CapabilityStatus",
    "struct CapabilityBinding",
    "enum SemanticCapability",
    "struct AdmissionDecision",
    "struct CapabilityResolver",
    "struct MemberRunGeneration",
    "struct AgentSessionGeneration",
    "struct NodeDaemonGeneration",
    "struct TeamSupervisorGeneration",
    "struct RuntimeDriverGeneration",
    "struct RuntimeBindingFence",
  ]],
  ["receipt_and_terminal", [
    "struct ProviderTerminalFailure",
    "struct EffectReceipt",
    "struct RuntimeObservation",
    "struct EffectInspection",
    "struct ReconcileReceipt",
    "struct QuiesceReceipt",
    "enum QuiesceStep",
    "struct QuiesceReceiptBuilder",
    "struct ReleaseReceipt",
    "struct MemberRuntimeCloseReceipt",
  ]],
  ["collaboration_capability", [
    "enum CollaborationCapabilityScope",
    "enum CollaborationCapabilityMechanism",
    "enum CollaborationCapabilityExpiry",
    "struct CollaborationCapabilityBinding",
    "struct CollaborationCapabilitySecret",
    "struct CollaborationCapabilityEnvironment",
    "struct CollaborationCapabilityEnvelope",
    "enum CollaborationCapabilityError",
  ]],
  ["conformance", [
    "trait RuntimeAdapter",
    "fn preflight_effect",
    "struct CompositionLifecycle",
    "struct OneShotDisposer",
    "enum RuntimeContractError",
  ]],
]);
const runtimeContractSources = execFileSync(
  "git",
  ["ls-files", "-co", "--exclude-standard", runtimeContractRoot],
  { cwd: root },
)
  .toString("utf8")
  .trim()
  .split("\n")
  .filter((source) => source.endsWith(".rs"));
const isTestRustPath = (source) => {
  const segments = source.split("/");
  const basename = segments.at(-1) ?? "";
  return (
    segments.includes("tests")
    || basename === "tests.rs"
    || basename.endsWith("_tests.rs")
  );
};
const runtimeContractProductionSources = runtimeContractSources.filter(
  (source) => !isTestRustPath(source),
);
const runtimeContractLibText = read(runtimeContractLib);
const expectedRuntimeContractSources = new Set([
  runtimeContractLib,
  ...[...runtimeContractModules.keys()].map(
    (moduleName) => `${runtimeContractRoot}/${moduleName}.rs`,
  ),
]);
for (const source of runtimeContractProductionSources) {
  if (!expectedRuntimeContractSources.has(source)) {
    failures.push(
      `${source}: unclassified runtime-contract production module; add its responsibility and edges to the complete inventory`,
    );
  }
}
for (const source of expectedRuntimeContractSources) {
  if (!runtimeContractProductionSources.includes(source)) {
    failures.push(`${source}: missing inventoried runtime-contract production module`);
  }
}
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

const expectedPublicOwner = new Map();
for (const [moduleName, publicItems] of runtimeContractModules) {
  for (const publicItem of publicItems) {
    if (expectedPublicOwner.has(publicItem)) {
      failures.push(`${publicItem}: duplicated runtime-contract inventory entry`);
    }
    expectedPublicOwner.set(publicItem, moduleName);
  }
}
const actualPublicOwner = new Map();
for (const source of runtimeContractProductionSources) {
  const moduleName = path.basename(source, ".rs");
  const content = read(source);
  const directDefinitions = content.matchAll(
    /^pub (struct|enum|trait|fn|type|const|static|union|mod)\s+([A-Za-z_][A-Za-z0-9_]*)/gm,
  );
  const generatedDefinitions = content.matchAll(
    /^generation_type!\(([A-Za-z_][A-Za-z0-9_]*),/gm,
  );
  for (const match of directDefinitions) {
    const publicItem = `${match[1]} ${match[2]}`;
    const owners = actualPublicOwner.get(publicItem) ?? [];
    owners.push(moduleName);
    actualPublicOwner.set(publicItem, owners);
  }
  for (const match of generatedDefinitions) {
    const publicItem = `struct ${match[1]}`;
    const owners = actualPublicOwner.get(publicItem) ?? [];
    owners.push(moduleName);
    actualPublicOwner.set(publicItem, owners);
  }
}
for (const [publicItem, expectedOwner] of expectedPublicOwner) {
  const owners = actualPublicOwner.get(publicItem) ?? [];
  if (owners.length !== 1 || owners[0] !== expectedOwner) {
    failures.push(
      `${publicItem}: expected sole owner ${expectedOwner}, found ${owners.join(", ") || "none"}`,
    );
  }
}
for (const [publicItem, owners] of actualPublicOwner) {
  if (!expectedPublicOwner.has(publicItem)) {
    failures.push(
      `${publicItem}: unclassified public runtime-contract item in ${owners.join(", ")}`,
    );
  }
}

const runtimeContractManifest = read("crates/firm-runtime-contract/Cargo.toml");
const allowedContractDependencies = new Set([
  "harness_core",
  "serde",
  "serde_json",
  "sha2",
  "thiserror",
]);
let dependencySection = false;
for (const line of runtimeContractManifest.split("\n")) {
  const section = line.match(/^\[([^\]]+)\]$/);
  if (section) {
    dependencySection =
      section[1] === "dependencies" || section[1].endsWith(".dependencies");
    continue;
  }
  if (!dependencySection) continue;
  const dependency = line.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (dependency && !allowedContractDependencies.has(dependency)) {
    failures.push(
      `crates/firm-runtime-contract/Cargo.toml: dependency ${dependency} is outside the provider-neutral allowlist`,
    );
  }
}

const workspaceCrateRoots = fs
  .readdirSync(path.join(root, "crates"), { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => `crates/${entry.name}/Cargo.toml`)
  .filter((manifest) => fs.existsSync(path.join(root, manifest)))
  .map((manifest) => read(manifest).match(/^name\s*=\s*"([^"]+)"/m)?.[1])
  .filter(Boolean)
  .filter((packageName) => !["firm-core", "firm-runtime-contract"].includes(packageName))
  .map((packageName) => packageName.replaceAll("-", "_"));
for (const source of runtimeContractProductionSources) {
  const content = read(source);
  for (const crateRoot of workspaceCrateRoots) {
    if (content.includes(`${crateRoot}::`)) {
      failures.push(
        `${source}: provider-neutral contract references workspace implementation crate ${crateRoot}`,
      );
    }
  }
}

const privateItemOwner = new Map([
  ["validate_continuation_exact", "provider_capabilities"],
]);
const itemOwner = new Map([
  ...[...expectedPublicOwner].map(([item, owner]) => [item.split(" ")[1], owner]),
  ...privateItemOwner,
]);
const allowedModuleEdges = new Map([
  ["collaboration_capability", new Set()],
  ["conformance", new Set(["control", "provider_capabilities", "receipt_and_terminal"])],
  ["control", new Set(["conformance", "provider_capabilities"])],
  ["cycle", new Set(["conformance", "control", "provider_capabilities", "receipt_and_terminal"])],
  ["provider_capabilities", new Set(["conformance"])],
  ["receipt_and_terminal", new Set(["conformance"])],
]);
const observedModuleEdges = new Set();
for (const moduleName of runtimeContractModules.keys()) {
  const source = `${runtimeContractRoot}/${moduleName}.rs`;
  const content = read(source);
  const importedItems = [];
  for (const match of content.matchAll(/use crate::\{([\s\S]*?)\};/g)) {
    importedItems.push(
      ...match[1]
        .split(",")
        .map((item) => item.trim())
        .filter(Boolean),
    );
  }
  for (const match of content.matchAll(/use crate::([A-Za-z_][A-Za-z0-9_]*)\s*;/g)) {
    importedItems.push(match[1]);
  }
  for (const match of content.matchAll(/(?:crate|super)::([A-Za-z_][A-Za-z0-9_]*)/g)) {
    importedItems.push(match[1]);
  }
  for (const importedItem of new Set(importedItems)) {
    const targetModule = runtimeContractModules.has(importedItem)
      ? importedItem
      : itemOwner.get(importedItem);
    if (!targetModule) {
      failures.push(`${source}: unclassified crate import ${importedItem}`);
      continue;
    }
    if (targetModule === moduleName) continue;
    observedModuleEdges.add(`${moduleName}->${targetModule}`);
    if (!allowedModuleEdges.get(moduleName)?.has(targetModule)) {
      failures.push(
        `${source}: forbidden internal dependency ${moduleName} -> ${targetModule} via ${importedItem}`,
      );
    }
  }
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

console.log(
  `Provider runtime package boundaries: 5 providers, ${expectedPublicOwner.size} runtime-contract public items, ${runtimeContractProductionSources.length} recursive production sources, ${observedModuleEdges.size} allowed internal edges, neutral contracts, application policy, and CLI composition verified.`,
);
