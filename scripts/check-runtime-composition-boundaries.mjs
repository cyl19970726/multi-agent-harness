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

const viewerContextApplicationPath =
  "crates/firm-application/src/viewer_context.rs";
const viewerContextApplication = read(viewerContextApplicationPath);
for (const required of [
  "pub struct RoleViewReadPrincipal",
  "pub struct ViewerContextFacts",
  "pub struct ViewerContextProjection",
  "pub fn validate_viewer_context_principal",
  "pub fn project_viewer_context",
  "ViewerContextQueryError",
]) {
  if (!viewerContextApplication.includes(required)) {
    failures.push(
      `${viewerContextApplicationPath}: missing typed ViewerContext owner ${required}`,
    );
  }
}
for (const forbidden of [
  "HarnessStore",
  "serde_json",
  "std::fs",
  "TcpStream",
  "/v1/",
  "SystemTime",
  "WorkDelivery",
  "firm_provider",
]) {
  if (viewerContextApplication.includes(forbidden)) {
    failures.push(
      `${viewerContextApplicationPath}: application query depends on forbidden adapter/runtime detail ${forbidden}`,
    );
  }
}
const viewerContextAdapterPath =
  "crates/firm-cli/src/role_views_api/viewer_surface.rs";
const viewerContextAdapter = read(viewerContextAdapterPath);
if (!viewerContextAdapter.includes("harness_application::project_viewer_context")) {
  failures.push(
    `${viewerContextAdapterPath}: adapter bypasses the typed ViewerContext projector`,
  );
}
for (const escapedPolicy of [
  "enum ViewerTeamRole",
  "fn viewer_team_role",
  "max_by_key(|member_run|",
]) {
  if (viewerContextAdapter.includes(escapedPolicy)) {
    failures.push(
      `${viewerContextAdapterPath}: ViewerContext policy escaped application owner: ${escapedPolicy}`,
    );
  }
}
const applicationCargo = read("crates/firm-application/Cargo.toml");
for (const forbidden of ["firm-store", "firm-cli", "firm-provider-", "harness_store"]) {
  if (applicationCargo.includes(forbidden)) {
    failures.push(
      `crates/firm-application/Cargo.toml: application boundary depends on forbidden ${forbidden}`,
    );
  }
}

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
  .filter((path) => path.endsWith(".rs"));

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

const isTestRustPath = (path) => {
  const segments = path.split("/");
  const basename = segments.at(-1) ?? "";
  return (
    segments.includes("tests") ||
    segments.includes("main_tests") ||
    basename === "tests.rs" ||
    basename === "main_tests.rs" ||
    basename.endsWith("_tests.rs")
  );
};
const productionRustPaths = sourcePaths.filter((path) => !isTestRustPath(path));
const testRustPaths = sourcePaths.filter(isTestRustPath);
if (productionRustPaths.length + testRustPaths.length !== sourcePaths.length) {
  failures.push("firm-cli Rust source classification is incomplete");
}
if (!productionRustPaths.includes(daemonRootPath)) {
  failures.push(`${daemonRootPath}: NodeDaemon root was misclassified as test-only`);
}
if (testRustPaths.length === 0) {
  failures.push("firm-cli test-only Rust source classification unexpectedly found zero files");
}
const authorityWriterTokens = [
  ".acquire_node_daemon_lease(",
  ".renew_node_daemon_lease(",
  ".drain_node_daemon_lease(",
  ".release_node_daemon_lease(",
];
for (const path of productionRustPaths) {
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
  "fn reap_finished(",
];
for (const path of productionRustPaths) {
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

const driveDefinitionPath =
  "crates/firm-cli/src/main_modules/member_orchestration.rs";
const countOccurrences = (content, token) => content.split(token).length - 1;
for (const path of productionRustPaths) {
  const count = countOccurrences(read(path), "drive_prepared_team_run(");
  const expected =
    path === driveDefinitionPath || path === teamSupervisionPath ? 1 : 0;
  if (count !== expected) {
    failures.push(
      `${path}: expected ${expected} drive_prepared_team_run definition/call occurrence(s), found ${count}`,
    );
  }
}
if (
  !read(driveDefinitionPath).includes(
    "pub(crate) fn drive_prepared_team_run(",
  )
) {
  failures.push(`${driveDefinitionPath}: missing canonical drive function definition`);
}

const forbiddenRenderedCommands = [
  "member message send --recipient-agent-id <stable-agent-identity>",
  "member message send --response-required --recipient-agent-id",
  "member message reply --recipient-agent-id",
  "member message request-decision --work-id",
];
for (const path of sourcePaths.filter((path) => path !== contractPath)) {
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
