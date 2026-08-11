import Ajv2020 from "ajv/dist/2020.js";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, join } from "node:path";

const root = "schemas/collaboration";
const fixtures = join(root, "fixtures");
const failures = [];
let validCount = 0;
let invalidCount = 0;

function json(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function files(path) {
  if (!existsSync(path)) return [];
  return readdirSync(path)
    .map((name) => join(path, name))
    .filter((file) => statSync(file).isFile() && file.endsWith(".json"))
    .sort();
}

const schemas = files(root)
  .filter((file) => file.endsWith(".schema.json"))
  .map((file) => ({ file, schema: json(file) }));
const ajv = new Ajv2020({ allErrors: true, strict: false });
for (const { schema } of schemas) ajv.addSchema(schema);

for (const { file, schema } of schemas) {
  const name = basename(file, ".schema.json");
  const valid = files(join(fixtures, name, "valid"));
  const invalid = files(join(fixtures, name, "invalid"));
  if (valid.length === 0) failures.push(`${file}: missing valid fixture`);
  if (invalid.length === 0) failures.push(`${file}: missing invalid fixture`);
  const validate = ajv.getSchema(schema.$id);
  for (const fixture of valid) {
    validCount += 1;
    if (!validate(json(fixture))) {
      failures.push(`${fixture}: expected valid: ${ajv.errorsText(validate.errors)}`);
    }
  }
  for (const fixture of invalid) {
    invalidCount += 1;
    if (validate(json(fixture))) failures.push(`${fixture}: expected invalid`);
  }
}

const core = readFileSync("crates/firm-core/src/collaboration.rs", "utf8");
for (const token of [
  "TargetPlacementRef",
  "RemoteWorkRef",
  "WorkDelegationV1",
  "DelegationCancellationRequest",
  "RemoteFactSnapshot",
  "CrossNodeDeliveryProjection",
  "RoutedBusinessKind",
]) {
  if (!core.includes(`pub struct ${token}`) && !core.includes(`pub enum ${token}`)) {
    failures.push(`firm-core collaboration contract missing ${token}`);
  }
}

const registryKinds = [
  "DelegationPropose",
  "DelegationDecide",
  "TargetWorkCreate",
  "DelegationCancelRequest",
  "DelegationCancelDecide",
  "TeamMessageDeliver",
  "RemoteFactPublish",
  "ArtifactGrant",
];
for (const kind of registryKinds) {
  if (!core.includes(kind)) failures.push(`routed business registry missing ${kind}`);
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`validated collaboration foundation: ${validCount} valid, ${invalidCount} invalid fixtures, 8 routed kinds`);
