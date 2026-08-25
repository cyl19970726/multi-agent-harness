#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const contractPath =
  "crates/firm-cli/src/collaboration/member_operating_contract.rs";
const read = (path) => readFileSync(resolve(root, path), "utf8");
const failures = [];

const contract = read(contractPath);
for (const required of [
  "pub(crate) struct MemberOperatingContract",
  "pub(crate) const MEMBER_OPERATING_ACTIONS",
  "pub(crate) enum ActorRole",
  "pub(crate) enum RecipientBinding",
  "pub(crate) enum WorkBinding",
  "pub(crate) enum ResponseIntent",
  "pub(crate) enum CorrelationBinding",
  "pub(crate) enum WakeBehavior",
]) {
  if (!contract.includes(required)) {
    failures.push(`${contractPath}: missing typed contract member ${required}`);
  }
}

const requiredConsumers = new Map([
  [
    "crates/firm-cli/src/main_modules/provider_interactions.rs",
    ["MemberOperatingContract::new", "render_incoming_message_reply_command"],
  ],
  [
    "crates/firm-cli/src/main_modules/cli_utilities.rs",
    ["render_member_message_cli_help"],
  ],
  [
    "crates/firm-cli/src/main_modules/user_commands.rs",
    ["member_message_subcommand_usage"],
  ],
]);
for (const [path, tokens] of requiredConsumers) {
  const content = read(path);
  for (const token of tokens) {
    if (!content.includes(token)) {
      failures.push(`${path}: does not consume canonical ${token}`);
    }
  }
}

const sourcePaths = execFileSync(
  "git",
  ["ls-files", "-co", "--exclude-standard", "crates/firm-cli/src"],
  { cwd: root },
)
  .toString("utf8")
  .trim()
  .split("\n")
  .filter((path) => path.endsWith(".rs") && path !== contractPath);
const forbiddenRenderedCommands = [
  "member message send --recipient-agent-id <stable-agent-identity>",
  "member message send --response-required --recipient-agent-id",
  "member message reply --recipient-agent-id",
  "member message request-decision --work-id",
];
for (const path of sourcePaths) {
  const content = read(path);
  for (const command of forbiddenRenderedCommands) {
    if (content.includes(command)) {
      failures.push(
        `${path}: rendered Member operating command escaped the typed contract: ${command}`,
      );
    }
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log("Runtime composition boundaries are valid.");
