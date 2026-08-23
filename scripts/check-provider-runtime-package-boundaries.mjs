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
