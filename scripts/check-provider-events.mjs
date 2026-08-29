import Ajv2020 from "ajv/dist/2020.js";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = "schemas/provider-events";
const readJson = path => JSON.parse(readFileSync(path, "utf8"));
const recordSchema = readJson(join(root, "provider-native-event-record-v3.schema.json"));
const pageSchema = readJson(join(root, "persisted-session-page-v1.schema.json"));
const adapterSchema = readJson(join(root, "persisted-adapter-manifest-v1.schema.json"));
const manifest = readJson(join(root, "manifest.v1.json"));
const adapters = readJson(join(root, "persisted-adapters.v3.json"));
const ajv = new Ajv2020({ allErrors: true, strict: false });
for (const schema of [recordSchema, pageSchema, adapterSchema]) ajv.addSchema(schema);
const validateRecord = ajv.getSchema(recordSchema.$id);
const validatePage = ajv.getSchema(pageSchema.$id);
const validateAdapter = ajv.getSchema(adapterSchema.$id);
const failures = [];

for (const [kind, validator] of [
  ["valid", validateRecord],
  ["invalid", validateRecord],
]) {
  for (const file of readdirSync(join(root, `fixtures/v3/${kind}`)).sort()) {
    if (!file.endsWith(".json")) continue;
    const accepted = validator(readJson(join(root, `fixtures/v3/${kind}`, file)));
    if (kind === "valid" && !accepted) {
      failures.push(`${file}: expected valid v3 record: ${ajv.errorsText(validator.errors)}`);
    }
    if (kind === "invalid" && accepted) failures.push(`${file}: expected invalid v3 record`);
  }
}
for (const [kind, validator] of [["page", validatePage], ["adapter", validateAdapter]]) {
  for (const validity of ["valid", "invalid"]) {
    const directory = join(root, `fixtures/v3/${kind}/${validity}`);
    for (const file of readdirSync(directory).sort()) {
      const accepted = validator(readJson(join(directory, file)));
      if (validity === "valid" && !accepted) {
        failures.push(`${kind}/${file}: expected valid: ${ajv.errorsText(validator.errors)}`);
      }
      if (validity === "invalid" && accepted) failures.push(`${kind}/${file}: expected invalid`);
    }
  }
}
for (const adapter of adapters) {
  if (!validateAdapter(adapter)) {
    failures.push(`${adapter.provider}: invalid persisted adapter: ${ajv.errorsText(validateAdapter.errors)}`);
  }
}

const exactSet = (left, right, label) => {
  const a = [...new Set(left)].sort();
  const b = [...new Set(right)].sort();
  if (JSON.stringify(a) !== JSON.stringify(b)) failures.push(`${label}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`);
};
exactSet(adapters.map(({ provider }) => provider), manifest.providers, "provider set");
exactSet(recordSchema.properties.provider.enum, manifest.providers, "record providers");
exactSet(recordSchema.$defs.fragment.properties.semantic_kind.enum, manifest.persisted_semantic_kinds, "semantic kinds");

for (const forbidden of ["node_daemon_id", "node_daemon_generation", "runtime_command_id", "effect_certainty", "visibility"]) {
  if (Object.hasOwn(recordSchema.properties, forbidden) || Object.hasOwn(recordSchema.$defs.fragment.properties, forbidden)) {
    failures.push(`persisted transcript must not carry runtime field ${forbidden}`);
  }
}
for (const runtimeKind of ["interaction_required", "interaction_resolved", "runtime_started", "runtime_ready", "runtime_stopped", "transport_interrupted", "command_recovery_required"]) {
  if (manifest.persisted_semantic_kinds.includes(runtimeKind)) failures.push(`runtime kind leaked into persisted Session vocabulary: ${runtimeKind}`);
}

const projector = readFileSync("crates/firm-provider-events/src/persisted/projector.rs", "utf8");
for (const provider of manifest.providers) {
  if (!projector.includes(`fn project_${provider}(`)) failures.push(`missing persisted ${provider} projector`);
}
const retired = [
  "crates/firm-provider-events/src/decoder.rs",
  "crates/firm-provider-events/src/fold.rs",
  "crates/firm-provider-events/src/service.rs",
  join(root, "provider-native-event-record.schema.json"),
  join(root, "session-event-projection.schema.json"),
  join(root, "live-provider-activity.schema.json"),
  join(root, "live-provider-activity-event.schema.json"),
];
for (const path of retired) if (existsSync(path)) failures.push(`retired v2 surface still exists: ${path}`);

const dashboard = [
  "apps/agent-dashboard/src/model/roleViews.ts",
  "apps/agent-dashboard/src/surfaces/AgentConversationWorkspace.tsx",
].map(path => readFileSync(path, "utf8")).join("\n");
for (const retiredToken of ["session_event_projection", "live_provider_activity", "provider_native_event_record.v2"]) {
  if (dashboard.includes(retiredToken)) failures.push(`Dashboard still references retired v2 surface: ${retiredToken}`);
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`provider event contract PASS: ${adapters.length} persisted adapters, ${manifest.persisted_semantic_kinds.length} persisted v3 kinds, v2 overlay retired`);
