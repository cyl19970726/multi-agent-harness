#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";

const failures = [];
const retiredPaths = [
  "crates/firm-cli/src/mcp.rs",
  "crates/firm-cli/src/mcp",
  "crates/firm-cli/tests/mcp_stdio.rs",
  "crates/firm-cli/tests/mcp_stdio",
  "plugins/star-harness/.mcp.json",
  "docs/current/integration/host-agent-mcp.md",
];

for (const path of retiredPaths) {
  if (existsSync(path)) failures.push(`retired Harness MCP surface remains: ${path}`);
}

const inspected = [
  "crates/firm-cli/src/main.rs",
  "skills/collaborate-as-agent-team-member/SKILL.md",
  "skills/collaborate-as-agent-team-member/references/host-loop.md",
  "skills/collaborate-as-agent-team-member/references/member-loop.md",
  "plugins/star-harness/.codex-plugin/plugin.json",
  "plugins/star-harness/.claude-plugin/plugin.json",
  "plugins/star-harness/kimi.plugin.json",
];
for (const path of inspected) {
  const text = readFileSync(path, "utf8");
  for (const forbidden of ["harness mcp", "HARNESS_BIN", '"mcpServers"', '"mcp" =>']) {
    if (text.includes(forbidden)) failures.push(`${path}: retired coordination token remains: ${forbidden}`);
  }
}

const installer = readFileSync("scripts/manage-star-harness-install.sh", "utf8");
if (installer.includes('plugins/star-harness/.mcp.json')) {
  failures.push("installer still publishes the retired Harness MCP registration");
}
if (!installer.includes('rm -f "${KIMI_MANAGED_DIR}/.mcp.json"')) {
  failures.push("Kimi in-place upgrade does not remove the retired Harness MCP registration");
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log("Harness coordination MCP retirement boundary passed");
