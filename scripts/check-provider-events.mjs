import Ajv2020 from "ajv/dist/2020.js";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = "schemas/provider-events";
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const observationSchema = readJson(join(root, "provider-observation.schema.json"));
const adapterSchema = readJson(join(root, "adapter-manifest.schema.json"));
const manifest = readJson(join(root, "manifest.v1.json"));
const adapters = readJson(join(root, "adapters.v1.json"));
const sessionSchema = readJson(join(root, "session-event-projection.schema.json"));
const teamSchema = readJson(join(root, "team-runtime-activity.schema.json"));
const ajv = new Ajv2020({ allErrors: true, strict: false });
for (const schema of [observationSchema, adapterSchema, sessionSchema, teamSchema]) ajv.addSchema(schema);
const validateObservation = ajv.getSchema(observationSchema.$id);
const validateAdapter = ajv.getSchema(adapterSchema.$id);
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
const typescript = readFileSync("apps/agent-dashboard/src/model/providerEvents.ts", "utf8");
const architecture = readFileSync("docs/current/architecture/provider-event-projection.md", "utf8");
for (const kind of manifest.semantic_kinds) {
  const rustName = kind.split("_").map((part) => part[0].toUpperCase() + part.slice(1)).join("");
  if (!model.includes(`    ${rustName},`)) failures.push(`missing Rust SemanticKind::${rustName}`);
  if (!typescript.includes(`"${kind}"`)) failures.push(`missing TypeScript semantic kind ${kind}`);
}
for (const forbidden of ["raw_transcript", "tool_input", "tool_output", "environment_variables"]) {
  if (typescript.includes(forbidden)) failures.push(`browser contract exposes forbidden ${forbidden}`);
}
for (const required of ["exact AgentIdentity owner", "TeamRuntimeActivity", "RuntimeCommand"]) {
  if (!architecture.includes(required)) failures.push(`architecture contract missing ${required}`);
}
for (const provider of manifest.providers) {
  if (!typescript.includes(`"${provider}"`)) failures.push(`missing TypeScript provider ${provider}`);
}

const validObservation = readJson(join(root, "fixtures/valid/codex-authored.json"));
const sessionEnvelope = {
  schema_version: "agentfirm.provider_observation.v1",
  agent_session_id: "session-1",
  agent_session_generation: 7,
  cursor: `sha256:${"a".repeat(64)}`,
  episodes: [{ episode_id: "turn-1", provider_turn_id: "turn-1", observations: [validObservation], terminal: false, incomplete: false }],
  truncated: false,
  disabled_reason: null,
};
if (!ajv.getSchema(sessionSchema.$id)(sessionEnvelope)) failures.push("generated Session projection violates schema");
const publicObservation = readJson(join(root, "fixtures/valid/runtime-ready-public.json"));
const { observation_id, agent_identity_id, semantic_kind, lifecycle_phase, completeness, effect_certainty, occurred_at, payload } = publicObservation;
if (!ajv.getSchema(teamSchema.$id)({ observation_id, agent_identity_id, semantic_kind, lifecycle_phase, completeness, effect_certainty, occurred_at, payload })) failures.push("generated Team activity violates schema");

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`provider event contract PASS: ${adapters.length} adapters, ${manifest.semantic_kinds.length} semantic kinds`);
