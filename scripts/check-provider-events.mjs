import Ajv2020 from "ajv/dist/2020.js";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = "schemas/provider-events";
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const observationSchema = readJson(join(root, "provider-observation.schema.json"));
const adapterSchema = readJson(join(root, "adapter-manifest.schema.json"));
const manifest = readJson(join(root, "manifest.v1.json"));
const adapters = readJson(join(root, "adapters.v1.json"));
const ajv = new Ajv2020({ allErrors: true, strict: false });
const validateObservation = ajv.compile(observationSchema);
const validateAdapter = ajv.compile(adapterSchema);
const failures = [];

for (const file of readdirSync(join(root, "fixtures/valid")).sort()) {
  const data = readJson(join(root, "fixtures/valid", file));
  if (!validateObservation(data)) {
    failures.push(`${file}: expected valid: ${ajv.errorsText(validateObservation.errors)}`);
  }
}
for (const file of readdirSync(join(root, "fixtures/invalid")).sort()) {
  const data = readJson(join(root, "fixtures/invalid", file));
  if (validateObservation(data)) failures.push(`${file}: expected invalid`);
}
for (const adapter of adapters) {
  if (!validateAdapter(adapter)) {
    failures.push(`${adapter.provider}: invalid adapter manifest: ${ajv.errorsText(validateAdapter.errors)}`);
  }
}

const exactSet = (left, right, label) => {
  const a = [...new Set(left)].sort();
  const b = [...new Set(right)].sort();
  if (JSON.stringify(a) !== JSON.stringify(b)) {
    failures.push(`${label}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`);
  }
};
exactSet(adapters.map(({ provider }) => provider), manifest.providers, "provider set");
exactSet(observationSchema.properties.provider.enum, manifest.providers, "observation providers");
exactSet(adapterSchema.properties.provider.enum, manifest.providers, "adapter providers");
exactSet(observationSchema.properties.semantic_kind.enum, manifest.semantic_kinds, "semantic kinds");
const publicKinds = observationSchema.allOf[1].then.properties.semantic_kind.enum;
exactSet(publicKinds, manifest.team_public_allowlist, "Team public allowlist");

const decoder = readFileSync("crates/firm-provider-events/src/decoder.rs", "utf8");
for (const provider of manifest.providers) {
  if (!decoder.includes(`fn decode_${provider}(`)) failures.push(`missing ${provider} decoder`);
}
const model = readFileSync("crates/firm-provider-events/src/model.rs", "utf8");
for (const kind of manifest.semantic_kinds) {
  const rustName = kind.split("_").map((part) => part[0].toUpperCase() + part.slice(1)).join("");
  if (!model.includes(`    ${rustName},`)) failures.push(`missing Rust SemanticKind::${rustName}`);
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`provider event contract PASS: ${adapters.length} adapters, ${manifest.semantic_kinds.length} semantic kinds`);
