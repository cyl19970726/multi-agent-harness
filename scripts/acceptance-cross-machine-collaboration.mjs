import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const failures = [];
const read = (path) => readFileSync(path, "utf8");
const core = read("crates/firm-core/src/collaboration.rs");
const store = read("crates/firm-store/src/collaboration.rs");
const route = read("crates/firm-store/src/collaboration_fabric.rs");
const runtime = read("crates/firm-cli/src/fabric_runtime.rs");
const http = read("crates/firm-cli/src/main.rs");
const mcp = read("crates/firm-cli/src/mcp.rs");
const architecture = read("docs/current/architecture/cross-machine-team-collaboration.md");
const operations = read("docs/current/operations/cross-machine-collaboration.md");
const normalizedDocs = `${architecture}\n${operations}`.replace(/\s+/g, " ");

for (const token of [
  "SourceWorkAttestation",
  "TargetPlacementRef",
  "WorkDelegationV1",
  "DelegationDecision",
  "DelegationCancellationRequest",
  "RemoteFactPublication",
  "CrossNodeDeliveryProjection",
]) {
  if (!core.includes(token)) failures.push(`canonical collaboration type missing: ${token}`);
}
for (const token of [
  "queue_collaboration_proposal",
  "queue_collaboration_message",
  "queue_remote_fact_publication",
  "persist_remote_message",
  "RemoteFabricCollaborationPort",
  "RecoveryRequired",
]) {
  if (!`${runtime}\n${route}`.includes(token)) failures.push(`integrated collaboration seam missing: ${token}`);
}
for (const token of [
  "/v1/collaboration/delegations",
  "/publications",
  "Idempotency-Key is required",
  "If-Match exact Delegation revision is required",
]) {
  if (!`${runtime}\n${http}`.includes(token)) failures.push(`closed HTTP contract missing: ${token}`);
}
for (const forbidden of [
  "fn work_delegation_create_tool(",
  "fn work_delegation_cancel_tool(",
  'name: "work_delegation_create"',
  'name: "work_delegation_cancel"',
]) {
  if (mcp.includes(forbidden)) failures.push(`retired local collaboration MCP authority remains: ${forbidden}`);
}
for (const token of [
  "Target Work completion does not complete source Work",
  "one Mission owns one flat Team",
  "RecoveryRequired",
  "second process on one Mac",
]) {
  if (!normalizedDocs.includes(token)) failures.push(`current collaboration docs omit invariant: ${token}`);
}

const realEvidencePath = process.env.AGENTFIRM_TWO_MAC_EVIDENCE;
if (process.env.REQUIRE_REAL_TWO_MAC === "1" && !realEvidencePath) {
  failures.push("REQUIRE_REAL_TWO_MAC=1 requires AGENTFIRM_TWO_MAC_EVIDENCE");
}
if (realEvidencePath) {
  if (!existsSync(realEvidencePath)) {
    failures.push(`two-Mac evidence does not exist: ${realEvidencePath}`);
  } else {
    const evidence = JSON.parse(read(realEvidencePath));
    const required = [
      "build_sha",
      "company_id",
      "protocol_schema_digest",
      "control_plane_generation",
      "source_node_id",
      "target_node_id",
      "source_gateway_generation",
      "target_gateway_generation",
      "delegation_id",
      "proposal_operation_id",
      "target_work_revision",
      "publication_id",
      "terminal_receipt_id",
      "cleanup_verified",
    ];
    for (const field of required) {
      if (evidence[field] === undefined || evidence[field] === "") {
        failures.push(`two-Mac evidence missing ${field}`);
      }
    }
    if (evidence.source_node_id === evidence.target_node_id) {
      failures.push("two-Mac evidence uses one Node identity twice");
    }
    if (evidence.team_spans_nodes !== false || evidence.source_work_auto_completed !== false) {
      failures.push("two-Mac evidence violates Team placement or source Work independence");
    }
    if (evidence.cleanup_verified !== true || evidence.secret_material_recorded !== false) {
      failures.push("two-Mac evidence must prove cleanup and contain no secret material");
    }
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

for (const args of [
  ["test", "-p", "firm-store", "--test", "cross_machine_collaboration", "--", "--test-threads=1"],
  ["test", "-p", "firm-fabric", "--test", "fabric_contract", "--", "--test-threads=1"],
  ["test", "-p", "firm-cli", "--test", "mcp_stdio", "--", "--test-threads=1"],
]) {
  const result = spawnSync("cargo", args, {
    stdio: "inherit",
    env: { ...process.env, CARGO_TERM_COLOR: "never" },
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

console.log(
  `cross-machine collaboration acceptance passed${realEvidencePath ? ` with real evidence ${realEvidencePath}` : " (deterministic mode; real two-Mac evidence not asserted)"}`,
);
