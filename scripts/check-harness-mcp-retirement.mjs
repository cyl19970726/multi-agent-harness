#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";

const failures = [];
const retiredPaths = [
  "crates/firm-cli/src/mcp.rs",
  "crates/firm-cli/src/mcp",
  "crates/firm-cli/tests/mcp_stdio.rs",
  "crates/firm-cli/tests/mcp_stdio",
  "docs/current/integration/host-agent-mcp.md",
];

for (const path of retiredPaths) {
  if (existsSync(path)) failures.push(`retired Harness MCP surface remains: ${path}`);
}

// ADR 0063 retired the plugin package that used to carry the .mcp.json
// registration; scripts/check-retired-paths.mjs keeps plugins/ absent.
const inspected = [
  "crates/firm-cli/src/main.rs",
  "skills/collaborate-as-agent-team-member/SKILL.md",
  "skills/collaborate-as-agent-team-member/references/host-loop.md",
  "skills/collaborate-as-agent-team-member/references/member-loop.md",
];
for (const path of inspected) {
  const text = readFileSync(path, "utf8");
  for (const forbidden of ["harness mcp", "HARNESS_BIN", '"mcpServers"', '"mcp" =>']) {
    if (text.includes(forbidden)) failures.push(`${path}: retired coordination token remains: ${forbidden}`);
  }
}

const installer = readFileSync("scripts/manage-star-harness-install.sh", "utf8");
for (const forbidden of [".mcp.json", "plugins/star-harness", "plugin marketplace"]) {
  if (installer.includes(forbidden)) {
    failures.push(`installer still references the retired plugin surface: ${forbidden}`);
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log("Harness coordination MCP retirement boundary passed");
