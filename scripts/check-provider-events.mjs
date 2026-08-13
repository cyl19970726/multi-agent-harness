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
const liveSchema = readJson(join(root, "live-provider-activity.schema.json"));
const liveEventSchema = readJson(join(root, "live-provider-activity-event.schema.json"));
const ajv = new Ajv2020({ allErrors: true, strict: false });
for (const schema of [observationSchema, adapterSchema, sessionSchema, teamSchema, liveSchema, liveEventSchema]) ajv.addSchema(schema);
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
const publicRule = observationSchema.allOf.find(
  (rule) => rule.if?.properties?.visibility?.const === "team_public",
);
if (!publicRule) failures.push("observation schema lacks Team public allowlist rule");
const publicKinds = publicRule?.then?.properties?.semantic_kind?.enum ?? [];
exactSet(publicKinds, manifest.team_public_allowlist, "Team public allowlist");

const decoder = readFileSync("crates/firm-provider-events/src/decoder.rs", "utf8");
for (const provider of manifest.providers) {
  if (!decoder.includes(`fn decode_${provider}(`)) failures.push(`missing ${provider} decoder`);
}
const model = readFileSync("crates/firm-provider-events/src/model.rs", "utf8");
const runtime = readFileSync("crates/firm-cli/src/main.rs", "utf8");
const piRuntime = readFileSync("crates/firm-cli/src/pi_rpc.rs", "utf8");
const architecture = readFileSync("docs/current/architecture/provider-event-projection.md", "utf8");
for (const kind of manifest.semantic_kinds) {
  const rustName = kind.split("_").map((part) => part[0].toUpperCase() + part.slice(1)).join("");
  if (!model.includes(`    ${rustName},`)) failures.push(`missing Rust SemanticKind::${rustName}`);
}
for (const required of ["exact AgentIdentity owner", "TeamRuntimeActivity", "RuntimeCommand"]) {
  if (!architecture.includes(required)) failures.push(`architecture contract missing ${required}`);
}
if (!runtime.includes('Some("item/reasoning/summaryTextDelta")')) failures.push("Codex live projection does not consume provider-declared summaryTextDelta");
if (runtime.includes('item/reasoning/textDelta')) failures.push("Codex raw reasoning textDelta must not enter the live projection");
for (const required of ["Kimi is thinking", "Kimi is waiting for interaction", "Thinking blocks are provider-private", "emit_live_provider_terminal"]) {
  if (!runtime.includes(required)) failures.push(`runtime privacy contract missing ${required}`);
}
if (runtime.includes("tool started · {title}")) failures.push("unreviewed provider tool titles must not enter live display summaries");
if (piRuntime.includes('event.get("args")') || piRuntime.includes('format!("Tool: {}", other)')) failures.push("Pi live projection must omit tool arguments, paths, and unknown names");
const validObservation = readJson(join(root, "fixtures/valid/codex-authored.json"));
const sessionEnvelope = {
  schema_version: "agentfirm.provider_observation.v1",
  agent_session_id: "session-1",
  agent_session_generation: 7,
  source_snapshot_fingerprint: `sha256:${"a".repeat(64)}`,
  episodes: [{ episode_id: "turn-1", provider_turn_id: "turn-1", observations: [validObservation], terminal: false, incomplete: false }],
  truncated: false,
  disabled_reason: null,
};
if (!ajv.getSchema(sessionSchema.$id)(sessionEnvelope)) failures.push("generated Session projection violates schema");
const liveEnvelope = {
  schema_version: "agentfirm.live_provider_activity.v1",
  durability: "volatile_process_memory",
  replayable: false,
  execution_space_id: "space-1",
  project_id: "project-1",
  team_run_id: "team-run-1",
  member_run_id: "member-run-1",
  agent_session_id: "session-1",
  agent_session_generation: 7,
  runtime_snapshot_locator: "runtime-snapshot-1",
  expires_unix_ms: 2,
  items: [{runtime_event_locator:"runtime-event-1",kind:"thinking",provider:"codex",display_summary:"Working",emitted_unix_ms:1,expires_unix_ms:2}],
};
if (!ajv.getSchema(liveSchema.$id)(liveEnvelope)) failures.push("generated live activity violates schema");
const liveEventEnvelope = {
  schema_version: "agentfirm.live_provider_activity_event.v1",
  reason: "updated",
  scope: {execution_space_id:"space-1",project_id:"project-1",team_run_id:"team-run-1",member_run_id:"member-run-1",agent_session_id:"session-1",agent_session_generation:7},
  activity: liveEnvelope,
};
if (!ajv.getSchema(liveEventSchema.$id)(liveEventEnvelope)) failures.push("generated live activity event violates schema");
const publicObservation = readJson(join(root, "fixtures/valid/runtime-ready-public.json"));
const { observation_id, agent_identity_id, semantic_kind, lifecycle_phase, completeness, effect_certainty, occurred_at, payload } = publicObservation;
if (!ajv.getSchema(teamSchema.$id)({ observation_id, agent_identity_id, semantic_kind, lifecycle_phase, completeness, effect_certainty, occurred_at, payload })) failures.push("generated Team activity violates schema");

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log(`provider event contract PASS: ${adapters.length} adapters, ${manifest.semantic_kinds.length} semantic kinds`);
