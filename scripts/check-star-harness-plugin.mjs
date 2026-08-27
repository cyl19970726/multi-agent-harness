#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pluginRoot = join(repoRoot, "plugins", "star-harness");
const errors = [];

function continuedCommands(markdown, prefix) {
  const lines = markdown.split(/\r?\n/);
  const commands = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (!lines[index].trimStart().startsWith(prefix)) continue;
    const commandLines = [lines[index].trim()];
    while (commandLines.at(-1).endsWith("\\") && index + 1 < lines.length) {
      index += 1;
      commandLines.push(lines[index].trim());
    }
    commands.push(commandLines.join(" "));
  }
  return commands;
}

function checkWorkCreateExamples() {
  const skillPath = join(
    repoRoot,
    "skills",
    "collaborate-as-agent-team-member",
    "SKILL.md",
  );
  const markdown = readFileSync(skillPath, "utf8");
  const commands = [
    ...continuedCommands(markdown, "firm team-run work create"),
    ...continuedCommands(markdown, '"$FIRM_BIN" team-run work create'),
  ];
  if (commands.length === 0) {
    errors.push(`${skillPath}: must contain executable team-run work create examples`);
    return;
  }
  if (!markdown.includes("work assign") || !markdown.includes("--membership-id")) {
    errors.push(`${skillPath}: Host assignment must use canonical work assign --membership-id`);
  }
  for (const command of commands) {
    for (const flag of ["--team-run-id", "--title", "--completion-criteria"]) {
      if (!command.includes(flag)) {
        errors.push(`${skillPath}: work create example is missing required ${flag}: ${command}`);
      }
    }
    if (command.includes("--owner-member-run-id")) {
      errors.push(`${skillPath}: create-time runtime ownership is retired; use work assign --membership-id: ${command}`);
    }
    if (command.includes("code-review:") && !command.includes("reviewer=")) {
      errors.push(`${skillPath}: code-review example must name its reviewer: ${command}`);
    }
    if (command.includes("--worktree")) {
      for (const flag of ["--context", "--claim-mode host_assign"] ) {
        if (!command.includes(flag)) {
          errors.push(`${skillPath}: code worktree example is missing ${flag}: ${command}`);
        }
      }
    }
  }
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    errors.push(`${path}: ${error.message}`);
    return {};
  }
}

const codex = readJson(join(pluginRoot, ".codex-plugin", "plugin.json"));
const claude = readJson(join(pluginRoot, ".claude-plugin", "plugin.json"));
const kimi = readJson(join(pluginRoot, "kimi.plugin.json"));
const marketplace = readJson(join(repoRoot, ".claude-plugin", "marketplace.json"));

if (
  codex.name !== "star-harness" ||
  claude.name !== "star-harness" ||
  kimi.name !== "star-harness"
) {
  errors.push("Codex, Claude, and Kimi manifests must all name star-harness");
}
if (codex.version !== claude.version || codex.version !== kimi.version) {
  errors.push("Codex, Claude, and Kimi manifest versions must match");
}
if (Object.hasOwn(claude, "hooks")) {
  errors.push(
    "Claude manifest must not redeclare default hooks/hooks.json; Claude auto-discovers it",
  );
}
if (existsSync(join(pluginRoot, ".mcp.json"))) {
  errors.push("Star Harness must not register a Harness MCP server; Agent coordination is CLI-only");
}
for (const [label, manifest] of [
  ["Codex", codex],
  ["Claude", claude],
  ["Kimi", kimi],
]) {
  if (Object.hasOwn(manifest, "mcpServers")) {
    errors.push(`${label} manifest must not register Harness MCP servers`);
  }
}
const kimiHookEvents = new Set(
  Array.isArray(kimi.hooks) ? kimi.hooks.map((hook) => hook.event) : [],
);
for (const event of ["SessionStart", "UserPromptSubmit", "Stop"]) {
  if (!kimiHookEvents.has(event)) {
    errors.push(`Kimi manifest must register the ${event} hook`);
  }
}
const marketplacePlugin = marketplace.plugins?.find(
  (plugin) => plugin.name === "star-harness",
);
if (!marketplacePlugin) {
  errors.push("repository marketplace must publish star-harness");
} else {
  if (marketplacePlugin.source !== "./plugins/star-harness") {
    errors.push("star-harness marketplace source must be ./plugins/star-harness");
  }
  if (marketplacePlugin.version !== codex.version) {
    errors.push("star-harness marketplace and manifest versions must match");
  }
}
for (const path of [
  join(pluginRoot, "hooks", "hooks.json"),
  join(pluginRoot, "scripts", "star-harness-hook.sh"),
]) {
  if (!existsSync(path)) errors.push(`missing lifecycle integration: ${path}`);
}
// mission-new.md and new-run.md were removed with the DOC-108 Mission
// retirement: every write they prescribed now fails closed.
for (const name of [
  "team-start.md",
  "team-status.md",
  "status.md",
  "dashboard.md",
]) {
  if (!existsSync(join(pluginRoot, "commands", name))) {
    errors.push(`missing command: ${name}`);
  }
}
for (const retired of ["kimi-agent-team", "harness-telemetry"]) {
  if (existsSync(join(repoRoot, "plugins", retired))) {
    errors.push(`retired plugin directory still exists: plugins/${retired}`);
  }
}
for (const skill of ["collaborate-as-agent-team-member"]) {
  if (!existsSync(join(pluginRoot, "skills", skill, "SKILL.md"))) {
    errors.push(`missing generated plugin skill: ${skill}`);
  }
}
checkWorkCreateExamples();

if (errors.length) {
  for (const error of errors) console.error(error);
  process.exit(1);
}
console.log("Star Harness unified plugin contract is valid");
