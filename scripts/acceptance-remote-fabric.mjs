import Ajv2020 from "ajv/dist/2020.js";
import { existsSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";

const root = "schemas/remote-fabric";
const fixtureRoot = join(root, "fixtures");
const schemas = [
  ["artifact-reference.schema.json", "artifact-reference"],
  ["company-node.schema.json", "company-node"],
  ["collaboration-business-reference.schema.json", "collaboration-business-reference"],
  ["delivery-intent-reference.schema.json", "delivery-intent-reference"],
  ["fabric-frame.schema.json", "fabric-frame"],
  ["node-enrollment.schema.json", "node-enrollment"],
  ["node-gateway-lease.schema.json", "node-gateway-lease"],
  ["node-hello.schema.json", "node-hello"],
  ["node-welcome.schema.json", "node-welcome"],
  ["route-attempt.schema.json", "route-attempt"],
  ["message-reference.schema.json", "message-reference"],
  ["routed-operation.schema.json", "routed-operation"],
  ["route-receipt.schema.json", "route-receipt"],
  ["artifact-manifest.schema.json", "artifact-manifest"],
  ["runtime-command-reference.schema.json", "runtime-command-reference"],
];
const failures = [];
let validCount = 0;
let invalidCount = 0;
const bundlePath = join(root, "schema-bundle.v1.json");
const bundleBytes = readFileSync(bundlePath);
const bundle = JSON.parse(bundleBytes.toString("utf8"));
const bundleDigest = createHash("sha256").update(bundleBytes).digest("hex");
const checkedSchemaNames = schemas.map(([name]) => name).sort();
if (bundle.schema_version !== "agentfirm.remote_fabric.v1" || bundle.protocol_version !== 1) {
  failures.push("schema bundle version does not match the frozen Rust contract");
}
if (JSON.stringify([...bundle.schemas].sort()) !== JSON.stringify(checkedSchemaNames)) {
  failures.push("schema bundle file inventory differs from the executable schema gate");
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

const ajv = new Ajv2020({ allErrors: true, strict: true });
ajv.addFormat("uuid", /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
ajv.addSchema(readJson("schemas/control-command-envelope.schema.json"));
const schemaDocuments = new Map(
  schemas.map(([schemaName]) => [schemaName, readJson(join(root, schemaName))]),
);
for (const document of schemaDocuments.values()) ajv.addSchema(document);

for (const [schemaName, fixturePrefix] of schemas) {
  const validate = ajv.getSchema(schemaDocuments.get(schemaName).$id);
  if (!validate) {
    failures.push(`${schemaName}: schema was not registered`);
    continue;
  }
  for (const kind of ["valid", "invalid"]) {
    const fixtures = readdirSync(join(fixtureRoot, kind))
      .filter((name) => name.startsWith(fixturePrefix) && name.endsWith(".json"))
      .sort();
    if (fixtures.length === 0) {
      failures.push(`${schemaName}: missing ${kind} fixture`);
      continue;
    }
    for (const fixture of fixtures) {
      const accepted = validate(readJson(join(fixtureRoot, kind, fixture)));
      if (kind === "valid") validCount += 1;
      else invalidCount += 1;
      if ((kind === "valid") !== accepted) {
        failures.push(
          `${fixture}: expected ${kind}, got ${accepted ? "valid" : JSON.stringify(validate.errors)}`,
        );
      }
    }
  }
}

const router = readFileSync("crates/firm-fabric/src/router.rs", "utf8");
const protocol = readFileSync("crates/firm-fabric/src/protocol.rs", "utf8");
const enrollment = readFileSync("crates/firm-fabric/src/enrollment.rs", "utf8");
const trustStore = readFileSync("crates/firm-store/src/trust_kernel.rs", "utf8");
const remoteIntegration = [
  readFileSync("crates/firm-cli/src/remote_fabric.rs", "utf8"),
  readFileSync("crates/firm-cli/src/fabric_runtime.rs", "utf8"),
].join("\n");
const requiredKinds = [
  "fabric.probe.v1",
  "fabric.reconcile_probe.v1",
  "runtime_command.reference.v1",
  "message.reference.v1",
  "delivery_intent.reference.v1",
  "artifact.reference.v1",
  "collaboration.business.v1",
];
for (const kind of requiredKinds) {
  if (!protocol.includes(`\"${kind}\"`)) {
    failures.push(`Rust operation registry missing ${kind}`);
  }
}
if (JSON.stringify([...bundle.operation_registry].sort()) !== JSON.stringify([...requiredKinds].sort())) {
  failures.push("schema bundle operation registry differs from the Rust registry");
}
for (const field of [
  "source_gateway_generation",
  "control_plane_generation",
  "body_digest",
  "idempotency_key",
]) {
  if (!protocol.includes(`pub ${field}:`)) {
    failures.push(`RoutedOperation trust field missing: ${field}`);
  }
}
for (const forbidden of ["work.accept.v1", "provider.start.v1", "message.send.v1"]) {
  if (router.includes(`\"${forbidden}\"`) || protocol.includes(`\"${forbidden}\"`)) {
    failures.push(`transport registry illegally owns business mutation ${forbidden}`);
  }
}
if (
  trustStore.includes("route_message_cross_node") ||
  trustStore.includes('"message_route_journal"')
) {
  failures.push("Wave4C Store still exposes a writable cross-node MessageRouteJournal authority");
}
if (
  !remoteIntegration.includes("persist_remote_message") ||
  remoteIntegration.includes("route_message_cross_node")
) {
  failures.push("Remote Message integration must persist canonical Message truth without dual route writes");
}
if (
  bundle.limits.enrollment_lifetime_max_ms !== 600000 ||
  !enrollment.includes("ENROLLMENT_LIFETIME_MAX_MS: u64 = 10 * 60 * 1000")
) {
  failures.push("enrollment lifetime differs between schema bundle and Rust authority");
}
if (
  bundle.limits.node_certificate_lifetime_max_ms !== 2592000000 ||
  !enrollment.includes("NODE_CERTIFICATE_LIFETIME_MAX_MS: u64 = 30 * 24 * 60 * 60 * 1000")
) {
  failures.push("Node certificate lifetime differs between schema bundle and Rust authority");
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

const tests = spawnSync(
  "cargo",
  ["test", "-p", "firm-fabric", "--", "--test-threads=1"],
  { stdio: "inherit", env: { ...process.env, CARGO_TERM_COLOR: "never" } },
);
if (tests.status !== 0) {
  process.exit(tests.status ?? 1);
}
for (const [packageName, args] of [
  [
    "firm-store",
    ["test", "-p", "firm-store", "remote_message_persists_before_delivery_and_replays_without_route_duplication"],
  ],
  [
    "firm-cli",
    ["test", "-p", "firm-cli", "--test", "node_gateway", "--", "--test-threads=1"],
  ],
]) {
  const integration = spawnSync("cargo", args, {
    stdio: "inherit",
    env: { ...process.env, CARGO_TERM_COLOR: "never" },
  });
  if (integration.status !== 0) {
    console.error(`Remote Fabric ${packageName} integration gate failed`);
    process.exit(integration.status ?? 1);
  }
}

const processEvidenceRoot = process.env.FABRIC_ACCEPTANCE_OUTPUT
  ? process.env.FABRIC_ACCEPTANCE_OUTPUT
  : mkdtempSync(join(tmpdir(), "agentfirm-remote-fabric-"));
if (
  !existsSync(processEvidenceRoot) ||
  readdirSync(processEvidenceRoot).length !== 0
) {
  console.error("FABRIC_ACCEPTANCE_OUTPUT must be an existing empty dedicated directory");
  process.exit(1);
}
const reviewedRevision = spawnSync("git", ["rev-parse", "HEAD"], {
  encoding: "utf8",
}).stdout.trim();
if (!/^[a-f0-9]{40}$/.test(reviewedRevision)) {
  console.error("cannot bind Remote Fabric evidence to the exact Git revision");
  process.exit(1);
}
const processAcceptance = spawnSync(
  "cargo",
  [
    "test",
    "-p",
    "firm-fabric",
    "--test",
    "remote_fabric_process",
    "remote_fabric_three_process_acceptance",
    "--",
    "--ignored",
    "--exact",
    "--nocapture",
  ],
  {
    stdio: "inherit",
    env: {
      ...process.env,
      CARGO_TERM_COLOR: "never",
      FABRIC_ACCEPTANCE_OUTPUT: processEvidenceRoot,
      FABRIC_ACCEPTANCE_REVISION: reviewedRevision,
    },
  },
);
if (processAcceptance.status !== 0) {
  process.exit(processAcceptance.status ?? 1);
}
const requiredEvidence = [
  "artifact-manifests.json",
  "attempts.json",
  "control-plane-leases.json",
  "fabric-acceptance.json",
  "gateway-leases.json",
  "nodes.json",
  "operations.json",
  "port-scan.json",
  "receipts.json",
  "reconcile.json",
];
const evidenceNames = readdirSync(processEvidenceRoot).sort();
if (JSON.stringify(evidenceNames) !== JSON.stringify(requiredEvidence.sort())) {
  console.error(`unexpected three-process evidence inventory: ${evidenceNames.join(", ")}`);
  process.exit(1);
}
if (evidenceNames.some((name) => /(?:key|secret|token|\.pem$)/i.test(name))) {
  console.error("three-process evidence retained secret runtime material");
  process.exit(1);
}
const processResult = readJson(join(processEvidenceRoot, "fabric-acceptance.json"));
const reconcileResult = readJson(join(processEvidenceRoot, "reconcile.json"));
const portScan = readJson(join(processEvidenceRoot, "port-scan.json"));
if (
  processResult.ok !== true ||
  processResult.processes !== 3 ||
  processResult.submitted_revision !== reviewedRevision ||
  processResult.gateway_generations.length !== 2 ||
  processResult.operation_ids.length !== 1 ||
  processResult.schema_bundle_digest !== bundleDigest ||
  processResult.effect !== "applied" ||
  reconcileResult.blind_replay !== false ||
  portScan.inspection !== "lsof-process-owned-tcp-listeners" ||
  !Array.isArray(portScan.control_plane_gateway_listeners) ||
  portScan.control_plane_gateway_listeners.length === 0 ||
  portScan.node_a_inbound_collaboration_listeners.length !== 0 ||
  portScan.node_b_inbound_collaboration_listeners.length !== 0
) {
  console.error("three-process evidence did not prove the frozen remote-fabric journey");
  process.exit(1);
}

console.log(
  `remote fabric accepted: ${validCount} valid schemas, ${invalidCount} hostile schemas, bundle ${bundleDigest}, durable routing/security/recovery and three-process mTLS/WSS journey PASS; evidence ${processEvidenceRoot}`,
);
