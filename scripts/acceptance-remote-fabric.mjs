import Ajv2020 from "ajv/dist/2020.js";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const root = "schemas/remote-fabric";
const fixtureRoot = join(root, "fixtures");
const schemas = [
  ["company-node.schema.json", "company-node"],
  ["node-enrollment.schema.json", "node-enrollment"],
  ["node-gateway-lease.schema.json", "node-gateway-lease"],
  ["routed-operation.schema.json", "routed-operation"],
  ["route-receipt.schema.json", "route-receipt"],
  ["artifact-manifest.schema.json", "artifact-manifest"],
];
const failures = [];
let validCount = 0;
let invalidCount = 0;
const bundle = readJson(join(root, "schema-bundle.v1.json"));
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

for (const [schemaName, fixturePrefix] of schemas) {
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  const validate = ajv.compile(readJson(join(root, schemaName)));
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
const requiredKinds = [
  "fabric.probe.v1",
  "fabric.reconcile_probe.v1",
  "runtime_command.reference.v1",
  "message.reference.v1",
  "delivery_intent.reference.v1",
  "artifact.reference.v1",
];
for (const kind of requiredKinds) {
  if (!router.includes(`\"${kind}\"`)) {
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
  if (router.includes(`\"${forbidden}\"`)) {
    failures.push(`transport registry illegally owns business mutation ${forbidden}`);
  }
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

const bundleDigest = createHash("sha256")
  .update(JSON.stringify(bundle))
  .digest("hex");
console.log(
  `remote fabric foundation accepted: ${validCount} valid schemas, ${invalidCount} hostile schemas, bundle ${bundleDigest}, durable routing/security/recovery tests PASS`,
);
