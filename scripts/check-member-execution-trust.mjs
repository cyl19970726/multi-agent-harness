import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";

const failures = [];

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function assert(condition, message) {
  if (!condition) failures.push(message);
}

const requiredSchemas = [
  "schemas/agent-member.schema.json",
  "schemas/member-run.schema.json",
  "schemas/native-session-ref.schema.json",
  "schemas/canonical-mutation-event.schema.json",
  "schemas/canonical-operation.schema.json",
  "schemas/team-message.schema.json",
  "schemas/message-delivery.schema.json",
  "schemas/work-delivery.schema.json",
  "schemas/member-workspace-binding.schema.json",
  "schemas/member-run-event.schema.json",
  "schemas/work-report.schema.json",
  "schemas/work-finding.schema.json",
  "schemas/failure-analysis.schema.json",
  "schemas/work-module-definition.schema.json",
  "schemas/work-module-binding.schema.json",
  "schemas/gate-requirement.schema.json",
  "schemas/gate-evaluation.schema.json",
  "schemas/gate-waiver.schema.json",
  "schemas/trust-error.schema.json",
  "schemas/agent-identity.schema.json",
  "schemas/agent-session.schema.json",
  "schemas/team-membership.schema.json",
  "schemas/work-execution-binding.schema.json",
  "schemas/message.schema.json",
  "schemas/message-subscription.schema.json",
  "schemas/canonical-message-delivery.schema.json",
  "schemas/canonical-work-delivery.schema.json",
  "schemas/runtime-command-record.schema.json",
];
for (const path of requiredSchemas) {
  assert(existsSync(path), `${path}: canonical schema is required`);
}

const retiredPaths = [
  "schemas/durable-agent-member.schema.json",
  "schemas/agent-runtime.schema.json",
  "schemas/agent-event.schema.json",
  "schemas/team-member-close-request.schema.json",
  "schemas/work-gate-evaluation.schema.json",
  "schemas/fixtures/durable-agent-member",
  "schemas/fixtures/agent-runtime",
  "schemas/fixtures/agent-event",
  "schemas/fixtures/team-member-close-request",
  "schemas/fixtures/work-gate-evaluation",
];
function pathContainsFiles(path) {
  if (!existsSync(path)) return false;
  if (!statSync(path).isDirectory()) return true;
  return readdirSync(path).some((entry) => pathContainsFiles(join(path, entry)));
}
for (const path of retiredPaths) {
  assert(
    !pathContainsFiles(path),
    `${path}: retired member-execution schema surface must contain no files`,
  );
}

const agentMember = readJson("schemas/agent-member.schema.json");
const canonicalAgentMemberProperties = [
  "capabilities",
  "created_at",
  "created_by",
  "description",
  "id",
  "model_preference",
  "name",
  "organization_status",
  "permission_ceiling",
  "provider_profile_ref",
  "role",
  "skill_refs",
  "updated_at",
  "version",
  "workspace_policy",
];
assert(
  JSON.stringify(Object.keys(agentMember.properties ?? {}).sort()) ===
    JSON.stringify(canonicalAgentMemberProperties),
  "agent-member.schema.json: properties must be the exact durable identity field set",
);
for (const leakedField of [
  "team_ids",
  "current_task_id",
  "current_proposal_id",
  "provider_runtime_id",
  "native_session",
  "provider_thread_id",
  "control_endpoint",
  "worktree_ref",
  "runtime_workspace_roots",
  "last_seen_at",
]) {
  assert(
    !Object.hasOwn(agentMember.properties ?? {}, leakedField),
    `agent-member.schema.json: runtime/derived field ${leakedField} must be absent`,
  );
}

const memberRun = readJson("schemas/member-run.schema.json");
assert(
  (memberRun.required ?? []).includes("agent_member_id"),
  "member-run.schema.json: agent_member_id must be required",
);
assert(
  memberRun.properties?.agent_member_id?.type === "string",
  "member-run.schema.json: agent_member_id must be a non-null string",
);
assert(
  (memberRun.properties?.agent_member_id?.minLength ?? 0) >= 1,
  "member-run.schema.json: agent_member_id must reject empty strings",
);

const nativeSession = readJson("schemas/native-session-ref.schema.json");
const nativeSessionProperties = Object.keys(nativeSession.properties ?? {}).sort();
const allowedNativeSessionProperties = [
  "adapter_contract_version",
  "availability",
  "execution_mode",
  "last_verified_at",
  "native_locator_kind",
  "native_session_id",
  "parent_native_session_id",
  "provider",
  "provider_version",
  "supports_resume",
].sort();
assert(
  JSON.stringify(nativeSessionProperties) === JSON.stringify(allowedNativeSessionProperties),
  "native-session-ref.schema.json: only frozen locator/compatibility metadata is allowed",
);
assert(
  JSON.stringify(memberRun.$defs?.nativeSessionRef?.properties ?? {}) ===
    JSON.stringify(nativeSession.properties ?? {}),
  "member-run.schema.json: embedded NativeSessionRef properties must match the canonical schema",
);
assert(
  JSON.stringify(memberRun.$defs?.nativeSessionRef?.required ?? []) ===
    JSON.stringify(nativeSession.required ?? []),
  "member-run.schema.json: embedded NativeSessionRef required fields must match the canonical schema",
);
for (const mirroredActivity of [
  "transcript",
  "messages",
  "turns",
  "tool_calls",
  "events",
  "stdout",
  "stderr",
  "command_output",
]) {
  assert(
    !Object.hasOwn(nativeSession.properties ?? {}, mirroredActivity),
    `native-session-ref.schema.json: provider activity mirror ${mirroredActivity} must be absent`,
  );
}

const mutationEvent = readJson("schemas/canonical-mutation-event.schema.json");
for (const field of [
  "aggregate_kind",
  "aggregate_id",
  "sequence",
  "store_sequence",
  "expected_version",
  "resulting_version",
  "idempotency_key",
  "canonical_request_fingerprint",
]) {
  assert(
    (mutationEvent.required ?? []).includes(field),
    `canonical-mutation-event.schema.json: ${field} must be required`,
  );
}

const canonicalOperation = readJson("schemas/canonical-operation.schema.json");
assert(
  (canonicalOperation.required ?? []).includes("event") &&
    (canonicalOperation.required ?? []).includes("resulting_projection"),
  "canonical-operation.schema.json: event and resulting_projection must be crash-atomic",
);
for (const field of ["immutable_side_records", "initial_outbox_records"]) {
  assert(
    canonicalOperation.properties?.[field]?.type === "array",
    `canonical-operation.schema.json: ${field} must be an in-row array`,
  );
}
assert(
  JSON.stringify(canonicalOperation.$defs?.canonicalMutationEvent?.properties ?? {}) ===
    JSON.stringify(mutationEvent.properties ?? {}),
  "canonical-operation.schema.json: embedded event properties must match CanonicalMutationEvent",
);
assert(
  JSON.stringify(canonicalOperation.$defs?.canonicalMutationEvent?.required ?? []) ===
    JSON.stringify(mutationEvent.required ?? []),
  "canonical-operation.schema.json: embedded event required fields must match CanonicalMutationEvent",
);
const memberRunEvent = readJson("schemas/member-run-event.schema.json");
assert(
  memberRunEvent.properties?.aggregate_kind?.const === "member_run",
  "member-run-event.schema.json: aggregate_kind must be fixed to member_run",
);
assert(
  existsSync("schemas/work-event.schema.json"),
  "schemas/work-event.schema.json: canonical WorkEvent contract must be preserved",
);

const sourceRoots = [
  "AGENTS.md",
  ".agents/skills",
  "crates",
  "apps",
  "scripts",
  "skills",
  "schemas",
  "docs/current",
  "docs/mental",
  "docs/registry.json",
];
const sourceExtensions = new Set([".rs", ".ts", ".tsx", ".js", ".mjs", ".json", ".md"]);
const thisScript = "scripts/check-member-execution-trust.mjs";
// DOC-108: the legacy export machinery is the single sanctioned place that
// names retired ledgers — to archive them, never to serve them.
const exportModules = new Set([
  "crates/firm-cli/src/legacy_company_os.rs",
  "crates/firm-cli/tests/legacy_company_os.rs",
]);
const activeFiles = [];

function collect(path) {
  if (!existsSync(path)) return;
  const stat = statSync(path);
  if (stat.isDirectory()) {
    if (["target", "node_modules"].includes(path.split("/").at(-1))) return;
    for (const entry of readdirSync(path).sort()) collect(join(path, entry));
    return;
  }
  const normalized = relative(process.cwd(), path);
  if (normalized === thisScript || exportModules.has(normalized) || !sourceExtensions.has(extname(path))) return;
  activeFiles.push(normalized);
}
for (const root of sourceRoots) collect(root);

const retiredPatterns = [
  ["DurableAgentMember", /\bDurableAgentMember(?:Status)?\b/g],
  ["runtime-heavy AgentMember status", /\bAgentMemberStatus\b/g],
  ["runtime-heavy AgentMember provider config", /\bAgentProviderConfig\b/g],
  ["StandingAgent", /\bStandingAgent\b/g],
  ["StandingAgent execution join", /execution_agent_member_ref/g],
  ["AgentRuntime", /\bAgentRuntime(?:Status|Health)?\b/g],
  ["AgentEvent", /\bAgentEvent\b/g],
  [
    "optional MemberRun identity",
    /\bstruct\s+MemberRun\b[^}]*\bagent_member_id\s*:\s*Option\s*</gs,
  ],
  [
    "optional TypeScript MemberRun identity",
    /\b(?:interface|type)\s+MemberRun\b[^}]*\bagent_member_id\s*\?\s*:/gs,
  ],
  ["fallback MemberRun identity", /stable_member_identity/g],
  ["durable member legacy ledger", /durable_agent_members\.jsonl/g],
  ["runtime-heavy member registry ledger", /["']members\.jsonl["']/g],
  ["StandingAgent legacy ledger", /company_os_standing_agents\.jsonl/g],
  ["agent runtime legacy ledger", /agent_runtimes\.jsonl/g],
  ["agent event legacy ledger", /agent_events\.jsonl/g],
  ["legacy GateSpec model", /\bGateSpec\b/g],
  ["legacy Work gates", /\bWork\.gates\b|\bwork\.gates\b/g],
  ["legacy Work workspace", /\bWork\.workspace\b|\bwork\.workspace\b/g],
  ["legacy Work review tool", /team_run_work_review/g],
  ["legacy gate checker command", /work check-gates|check-gates/g],
  ["legacy MemberRun workspace field", /\bworktree_ref\b|\bworkspace_snapshot\b/g],
  ["legacy TeamMessage routing field", /\borigin_wave_id\b|\bfrom_member_id\b|\bto_member_ids\b/g],
  ["legacy authored TeamMessage kind", /["']kind["']\s*:\s*["'](?:plan_request|plan_proposal|plan_feedback|plan_approval|broadcast|progress|handoff|blocker|review_request|review_result)["']/g],
];

for (const path of activeFiles) {
  const text = readFileSync(path, "utf8");
  for (const [label, pattern] of retiredPatterns) {
    pattern.lastIndex = 0;
    if (pattern.test(text)) failures.push(`${path}: retired ${label} remains in active code`);
  }
}

const coreRoot = readFileSync("crates/firm-core/src/lib.rs", "utf8");
for (const legacyRootDeclaration of [
  /pub struct MemberRun\b/,
  /pub struct TeamMessage\b/,
  /pub struct MessageDelivery\b/,
  /pub struct WorkDelivery\b/,
  /pub struct Message\b/,
  /pub enum TeamMessageKind\b/,
  /pub enum MessageKind\b/,
]) {
  assert(
    !legacyRootDeclaration.test(coreRoot),
    `crates/firm-core/src/lib.rs: legacy root execution contract ${legacyRootDeclaration} must not be authoritative`,
  );
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(
  `validated canonical member-execution trust schemas and zero legacy matches across ${activeFiles.length} active files`,
);
