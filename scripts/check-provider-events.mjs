import Ajv2020 from "ajv/dist/2020.js";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = "schemas/provider-events";
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const recordSchema = readJson(join(root, "provider-native-event-record.schema.json"));
const adapterSchema = readJson(join(root, "adapter-manifest.schema.json"));
const manifest = readJson(join(root, "manifest.v1.json"));
const adapters = readJson(join(root, "adapters.v1.json"));
const sessionSchema = readJson(join(root, "session-event-projection.schema.json"));
const teamSchema = readJson(join(root, "team-runtime-activity.schema.json"));
const liveSchema = readJson(join(root, "live-provider-activity.schema.json"));
const liveEventSchema = readJson(join(root, "live-provider-activity-event.schema.json"));
const ajv = new Ajv2020({ allErrors: true, strict: false });
for (const schema of [recordSchema, adapterSchema, sessionSchema, teamSchema, liveSchema, liveEventSchema]) ajv.addSchema(schema);
const validateRecord = ajv.getSchema(recordSchema.$id);
const validateAdapter = ajv.getSchema(adapterSchema.$id);
const failures = [];

for (const file of readdirSync(join(root, "fixtures/valid")).sort()) {
  const data = readJson(join(root, "fixtures/valid", file));
  if (!validateRecord(data)) {
    failures.push(`${file}: expected valid: ${ajv.errorsText(validateRecord.errors)}`);
  }
}
for (const file of readdirSync(join(root, "fixtures/invalid")).sort()) {
  const data = readJson(join(root, "fixtures/invalid", file));
  if (validateRecord(data)) failures.push(`${file}: expected invalid`);
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
exactSet(recordSchema.properties.provider.enum, manifest.providers, "record providers");
exactSet(adapterSchema.properties.provider.enum, manifest.providers, "adapter providers");
exactSet(recordSchema.$defs.fragment.properties.semantic_kind.enum, manifest.semantic_kinds, "semantic kinds");

const decoder = readFileSync("crates/firm-provider-events/src/decoder.rs", "utf8");
for (const provider of manifest.providers) {
  if (!decoder.includes(`fn decode_${provider}(`)) failures.push(`missing ${provider} decoder`);
}
const model = readFileSync("crates/firm-provider-events/src/model.rs", "utf8");
const runtime = [
  "crates/firm-cli/src/main.rs",
  "crates/firm-provider-codex/src/team_runtime.rs",
  "crates/firm-provider-claude/src/lib.rs",
  "crates/firm-provider-kimi/src/team_runtime.rs",
  "crates/firm-cli/src/main_modules/member_work_coordination.rs",
].map((path) => readFileSync(path, "utf8")).join("\n");
const piRuntime = readFileSync("crates/firm-provider-pi/src/team_runtime.rs", "utf8");
const architecture = readFileSync("docs/current/architecture/provider-event-projection.md", "utf8");
for (const kind of manifest.semantic_kinds) {
  const rustName = kind.split("_").map((part) => part[0].toUpperCase() + part.slice(1)).join("");
  if (!model.includes(`    ${rustName},`)) failures.push(`missing Rust SemanticKind::${rustName}`);
}
// Native-session reads are same-machine local-Operator only; provider login
// and remote RoleView credentials never become transcript grants. Mutation
// remains RuntimeCommand-bound.
for (const required of ["same-machine loopback", "Remote RoleView credentials", "TeamRuntimeActivity", "RuntimeCommand"]) {
  if (!architecture.includes(required)) failures.push(`architecture contract missing ${required}`);
}
for (const required of ["native_event", "LiveProviderTurnGuard"]) {
  if (!runtime.includes(required)) failures.push(`runtime native-event contract missing ${required}`);
}
const validRecord = readJson(join(root, "fixtures/valid/codex-authored.json"));
for (const [label, mutate] of [
  ["payload fields are required", record => { delete record.fragments[0].payload.text; }],
  ["semantic kind and payload are paired", record => { record.fragments[0].semantic_kind = "reasoning"; }],
  ["payload fields are closed", record => { record.fragments[0].payload.unreviewed = true; }],
]) {
  const invalid = structuredClone(validRecord);
  mutate(invalid);
  if (validateRecord(invalid)) failures.push(`record schema is not closed: ${label}`);
}
const sessionEnvelope = {
  schema_version: "agentfirm.provider_native_event_record.v2",
  agent_session_id: "session-1",
  agent_session_generation: 7,
  source_snapshot_fingerprint: `sha256:${"a".repeat(64)}`,
  episodes: [{ episode_id: "turn-1", provider_turn_id: "turn-1", records: [validRecord], terminal: false, incomplete: false }],
  truncated: false,
  availability: "available",
  unavailable_reason_code: null,
  disabled_reason: null,
};
if (!ajv.getSchema(sessionSchema.$id)(sessionEnvelope)) failures.push("generated Session projection violates schema");
const liveEnvelope = {
  schema_version: "agentfirm.live_provider_activity.v2",
  durability: "volatile_process_memory",
  replayable: false,
  execution_space_id: "space-1",
  project_id: "project-1",
  team_run_id: "team-run-1",
  member_run_id: "member-run-1",
  member_run_generation: 3,
  agent_session_id: "session-1",
  agent_session_generation: 7,
  agent_member_id: "member-1",
  node_daemon_id: "daemon-1",
  node_daemon_generation: 4,
  runtime_snapshot_locator: "runtime-snapshot-1",
  expires_unix_ms: 2,
  items: [{runtime_event_locator:"runtime-event-1",record:validRecord,emitted_unix_ms:1,expires_unix_ms:2}],
};
if (!ajv.getSchema(liveSchema.$id)(liveEnvelope)) failures.push("generated live activity violates schema");
const liveEventEnvelope = {
  schema_version: "agentfirm.live_provider_activity_event.v2",
  reason: "updated",
  scope: {execution_space_id:"space-1",project_id:"project-1",team_run_id:"team-run-1",member_run_id:"member-run-1",member_run_generation:3,agent_session_id:"session-1",agent_session_generation:7,agent_member_id:"member-1",node_daemon_id:"daemon-1",node_daemon_generation:4},
  activity: liveEnvelope,
};
if (!ajv.getSchema(liveEventSchema.$id)(liveEventEnvelope)) failures.push("generated live activity event violates schema");
const publicRecord = readJson(join(root, "fixtures/valid/runtime-ready-public.json"));
const publicFragment = publicRecord.fragments[0];
const { fragment_id, semantic_kind, lifecycle_phase, completeness, effect_certainty, payload } = publicFragment;
const { record_id, agent_member_id, occurred_at } = publicRecord;
if (!ajv.getSchema(teamSchema.$id)({ record_id, fragment_id, agent_member_id, semantic_kind, lifecycle_phase, completeness, effect_certainty, occurred_at, payload })) failures.push("generated Team activity violates schema");

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`provider event contract PASS: ${adapters.length} adapters, ${manifest.semantic_kinds.length} semantic kinds`);
