import { existsSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const failures = [];
const read = (path) => readFileSync(path, "utf8");
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const readJsonLines = (path) =>
  read(path)
    .split("\n")
    .filter((line) => line.trim())
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`${path}:${index + 1} is not valid JSON: ${error.message}`);
      }
    });
const lastState = (rows, label) => {
  const state = rows.at(-1)?.state;
  if (!state || typeof state !== "object") throw new Error(`${label} has no final durable state`);
  return state;
};
const objects = (value) => {
  const found = [];
  const visit = (item) => {
    if (!item || typeof item !== "object") return;
    found.push(item);
    if (Array.isArray(item)) item.forEach(visit);
    else Object.values(item).forEach(visit);
  };
  visit(value);
  return found;
};
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
      "native_fact_work_revision",
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
    if (evidence.evidence_schema_version !== "agentfirm.wave6-two-mac-evidence.v2") {
      failures.push("two-Mac evidence must use the recomputable v2 evidence schema");
    } else {
      try {
        const exactHead = spawnSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" });
        if (exactHead.status !== 0) throw new Error("cannot resolve exact repository HEAD");
        const head = exactHead.stdout.trim();
        if (evidence.submitted_revision_binding !== "git_head_at_validation") {
          throw new Error("evidence must bind the submitted revision to the validator's exact git HEAD");
        }
        const ancestor = spawnSync(
          "git",
          ["merge-base", "--is-ancestor", evidence.base_revision, head],
          { encoding: "utf8" },
        );
        if (ancestor.status !== 0) throw new Error("evidence base is not an ancestor of its submitted revision");
        const buildAncestor = spawnSync(
          "git",
          ["merge-base", "--is-ancestor", evidence.build_sha, head],
          { encoding: "utf8" },
        );
        if (buildAncestor.status !== 0) throw new Error("tested build is not an ancestor of the submitted revision");
        if (evidence.build_sha !== head) {
          const changed = spawnSync(
            "git",
            ["diff", "--name-only", `${evidence.build_sha}..${head}`],
            { encoding: "utf8" },
          );
          if (changed.status !== 0) throw new Error("cannot prove the post-test revision delta");
          const changedPaths = changed.stdout.trim().split("\n").filter(Boolean);
          if (
            changedPaths.length === 0 ||
            changedPaths.some((path) => !path.startsWith("docs/current/operations/evidence/"))
          ) {
            throw new Error("post-test revision changed production or non-evidence files");
          }
        }

        const manifestRoot = dirname(resolve(realEvidencePath));
        const material = {};
        for (const name of [
          "central_collaboration_ledger",
          "control_plane_fabric_journal",
          "source_node_fabric_journal",
          "target_node_fabric_journal",
          "source_trust_ledger",
          "target_trust_ledger",
          "source_work_ledger",
          "target_work_ledger",
          "source_provider_transcript",
          "target_provider_transcript",
          "artifact",
        ]) {
          const descriptor = evidence.files?.[name];
          if (!descriptor?.path || !descriptor?.sha256) throw new Error(`evidence files.${name} is incomplete`);
          const absolute = resolve(manifestRoot, descriptor.path);
          if (!existsSync(absolute)) throw new Error(`evidence material is missing: ${name}`);
          const bytes = readFileSync(absolute);
          const digest = sha256(bytes);
          if (digest !== descriptor.sha256) throw new Error(`${name} digest mismatch`);
          material[name] = { absolute, bytes };
        }

        const collaborationRows = readJsonLines(material.central_collaboration_ledger.absolute);
        const delegationRows = collaborationRows.filter(
          (row) => row.aggregate_kind === "work_delegation_v1" && row.aggregate_id === evidence.delegation_id,
        );
        const delegation = delegationRows.at(-1)?.resulting_projection;
        if (!delegation) throw new Error("central ledger has no exact Delegation projection");
        if (delegation.company_id !== evidence.company_id) throw new Error("Delegation Company mismatch");
        if (delegation.source_node_id !== evidence.source_node_id) throw new Error("Delegation source Node mismatch");
        if (delegation.target_placement?.node_id !== evidence.target_node_id) throw new Error("Delegation target Node mismatch");
        if (delegation.target_work_ref?.work_revision !== evidence.target_work_revision) {
          throw new Error("target Work revision is not derived from the central Delegation");
        }
        if (String(delegation.state).toLowerCase() !== String(evidence.delegation_state).toLowerCase()) {
          throw new Error("Delegation terminal state mismatch");
        }
        const publication = collaborationRows.find(
          (row) => row.aggregate_kind === "remote_fact_publication" && row.aggregate_id === evidence.publication_id,
        )?.resulting_projection;
        if (!publication || publication.delegation_id !== evidence.delegation_id) {
          throw new Error("central ledger has no exact remote publication for the Delegation");
        }
        if (JSON.stringify(publication.fact_work_ref) !== JSON.stringify(delegation.target_work_ref)) {
          throw new Error("publication relationship Work ref is not the exact frozen Delegation target Work ref");
        }
        if (
          publication.native_fact_work_ref?.work_id !== delegation.target_work_ref?.work_id ||
          publication.native_fact_work_ref?.team_id !== delegation.target_work_ref?.team_id ||
          publication.native_fact_work_ref?.node_id !== delegation.target_work_ref?.node_id ||
          publication.native_fact_work_ref?.work_revision !== evidence.native_fact_work_revision
        ) {
          throw new Error("publication native fact Work ref is not the exact current target Work revision");
        }

        const fabric = lastState(
          readJsonLines(material.control_plane_fabric_journal.absolute),
          "Control Plane Fabric journal",
        );
        if (fabric.authority_company_id !== evidence.company_id) throw new Error("Fabric Company mismatch");
        const sourceNode = fabric.nodes?.[evidence.source_node_id];
        const targetNode = fabric.nodes?.[evidence.target_node_id];
        if (!sourceNode || !targetNode || evidence.source_node_id === evidence.target_node_id) {
          throw new Error("Fabric does not contain two distinct registered Nodes");
        }
        const sourceLease = fabric.gateway_leases?.[evidence.source_node_id];
        const targetLease = fabric.gateway_leases?.[evidence.target_node_id];
        if (sourceLease?.gateway_generation !== evidence.source_gateway_generation) throw new Error("source Gateway generation mismatch");
        if (targetLease?.gateway_generation !== evidence.target_gateway_generation) throw new Error("target Gateway generation mismatch");
        const terminalReceipt = fabric.receipts?.[evidence.terminal_receipt_id];
        if (!terminalReceipt || terminalReceipt.kind !== "operation_applied") {
          throw new Error("terminal OperationApplied receipt is absent from the route journal");
        }
        const artifactManifest = fabric.artifacts?.[evidence.artifact_id];
        if (!artifactManifest || artifactManifest.sha256 !== evidence.artifact_sha256) {
          throw new Error("Fabric artifact manifest does not match the claimed artifact digest");
        }
        if (sha256(material.artifact.bytes) !== evidence.artifact_sha256) {
          throw new Error("artifact bytes do not match the canonical artifact manifest");
        }

        for (const [side, expectedNode, expectedGateway] of [
          ["source", evidence.source_node_id, evidence.source_gateway_generation],
          ["target", evidence.target_node_id, evidence.target_gateway_generation],
        ]) {
          const local = lastState(
            readJsonLines(material[`${side}_node_fabric_journal`].absolute),
            `${side} Node Fabric journal`,
          );
          if (local.authority_company_id !== evidence.company_id || local.authority_node_id !== expectedNode) {
            throw new Error(`${side} Node-local authority mismatch`);
          }
          if (local.active_session?.gateway_generation !== expectedGateway) {
            throw new Error(`${side} Node-local Gateway generation mismatch`);
          }
        }

        const sourceTrustObjects = objects(readJsonLines(material.source_trust_ledger.absolute));
        const targetTrustObjects = objects(readJsonLines(material.target_trust_ledger.absolute));
        const sourceWorkObjects = objects(readJsonLines(material.source_work_ledger.absolute));
        const targetWorkObjects = objects(readJsonLines(material.target_work_ledger.absolute));
        const sourceMessage = sourceTrustObjects.find((value) => value.id === evidence.message_id);
        const targetMessage = targetTrustObjects.find((value) => value.id === evidence.message_id);
        const delivery = targetTrustObjects.find((value) => value.id === evidence.message_delivery_id);
        if (!sourceMessage || !targetMessage || !delivery) {
          throw new Error("Message authoring, target replica, or per-recipient Delivery is not recomputable from trust ledgers");
        }
        if (sourceMessage.body_digest !== targetMessage.body_digest) {
          throw new Error("source and target immutable Message bytes disagree");
        }
        const nativeTargetWork = targetWorkObjects.find(
          (value) =>
            value.id === publication.native_fact_work_ref.work_id &&
            value.version === publication.native_fact_work_ref.work_revision,
        );
        const activeBinding = targetTrustObjects.find(
          (value) =>
            value.work_id === publication.native_fact_work_ref.work_id &&
            value.work_revision === publication.native_fact_work_ref.work_revision &&
            value.status === "active" &&
            value.agent_identity_id === publication.created_by.id,
        );
        const sourceWork = sourceWorkObjects.find(
          (value) =>
            value.id === delegation.source_work_ref.work_id &&
            value.version === delegation.source_work_ref.work_revision,
        );
        if (!nativeTargetWork || !activeBinding || !sourceWork) {
          throw new Error("source Work, current target Work, or exact active target WorkExecutionBinding is absent");
        }

        for (const [side, marker] of [
          ["source", evidence.source_provider_marker],
          ["target", evidence.target_provider_marker],
        ]) {
          const transcript = JSON.parse(material[`${side}_provider_transcript`].bytes.toString("utf8"));
          if (
            transcript.node_id !== evidence[`${side}_node_id`] ||
            transcript.marker !== marker ||
            transcript.provider !== "codex" ||
            transcript.secret_material_recorded !== false
          ) {
            throw new Error(`${side} provider transcript is not the exact secret-free Codex proof`);
          }
        }
      } catch (error) {
        failures.push(`two-Mac evidence is not independently recomputable: ${error.message}`);
      }
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
