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
  "pub(crate) enum RuntimeAuthorization",
  "pub(crate) enum RecipientBinding",
  "pub(crate) enum WorkBinding",
  "pub(crate) enum ResponseIntent",
  "pub(crate) enum CorrelationBinding",
  "pub(crate) enum WakeBehavior",
  "fn render_command(&self",
  "fn render_projection(&self",
  "every_typed_field_drives_the_shared_projection",
  "action_collection_is_unique_complete_and_honest_about_runtime_choices",
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
  [
    "scripts/check-cross-layer-consistency.mjs",
    ["crates/firm-cli/src/collaboration"],
  ],
  [
    "crates/firm-cli/src/main_tests/general/turn_input_uses_stable_harness_envelope.rs",
    ["MEMBER_OPERATING_ACTIONS", "RecipientFromBoard"],
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

for (const typedProjection of [
  "self.action.label()",
  "self.actor_role.label()",
  "self.runtime_authorization.label()",
  "self.recipient.label()",
  "self.work_binding.label()",
  "self.response_intent.label()",
  "self.correlation.label()",
  "self.wake_behavior.label()",
  "self.route.subcommand()",
  "self.body_shape.placeholder()",
]) {
  if (!contract.includes(typedProjection)) {
    failures.push(
      `${contractPath}: typed field does not drive the shared projection: ${typedProjection}`,
    );
  }
}
for (const forbidden of ["fn generic_command", "match action {"]) {
  if (contract.includes(forbidden)) {
    failures.push(
      `${contractPath}: command rendering must not branch independently from the action spec: ${forbidden}`,
    );
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

const daemonRootPath = "crates/firm-cli/src/supervisor_daemon.rs";
const machineAuthorityPath =
  "crates/firm-cli/src/supervisor_daemon/machine_authority.rs";
const teamSupervisionPath =
  "crates/firm-cli/src/supervisor_daemon/team_supervision.rs";
const daemonRoot = read(daemonRootPath);
for (const moduleName of [
  "control_protocol",
  "machine_authority",
  "recovery",
  "shutdown",
  "team_supervision",
]) {
  if (!daemonRoot.includes(`mod ${moduleName};`)) {
    failures.push(
      `${daemonRootPath}: missing NodeDaemon responsibility module ${moduleName}`,
    );
  }
}

const daemonProductionPaths = sourcePaths.filter(
  (path) =>
    path === daemonRootPath ||
    (path.startsWith("crates/firm-cli/src/supervisor_daemon/") &&
      !path.endsWith("/tests.rs")),
);
const authorityWriterTokens = [
  ".acquire_node_daemon_lease(",
  ".renew_node_daemon_lease(",
  ".drain_node_daemon_lease(",
  ".release_node_daemon_lease(",
];
for (const path of daemonProductionPaths) {
  const content = read(path);
  for (const token of authorityWriterTokens) {
    if (path !== machineAuthorityPath && content.includes(token)) {
      failures.push(
        `${path}: NodeDaemon machine authority writer escaped ${machineAuthorityPath}: ${token}`,
      );
    }
  }
}
const machineAuthority = read(machineAuthorityPath);
for (const token of authorityWriterTokens) {
  if (!machineAuthority.includes(token)) {
    failures.push(`${machineAuthorityPath}: missing authority operation ${token}`);
  }
}

const teamLifecycleTokens = [
  "fn scan_and_adopt(",
  "fn start_supervising(",
  "TeamSupervisorRegistration::start(",
  "drive_prepared_team_run(",
  "fn reap_finished(",
];
for (const path of daemonProductionPaths) {
  const content = read(path);
  for (const token of teamLifecycleTokens) {
    if (path !== teamSupervisionPath && content.includes(token)) {
      failures.push(
        `${path}: Team supervisor lifecycle escaped ${teamSupervisionPath}: ${token}`,
      );
    }
  }
}
const teamSupervision = read(teamSupervisionPath);
for (const token of teamLifecycleTokens) {
  if (!teamSupervision.includes(token)) {
    failures.push(`${teamSupervisionPath}: missing lifecycle operation ${token}`);
  }
}

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
