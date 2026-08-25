import { readFileSync } from "node:fs";

const requirements = new Map([
  [
    "AGENTS.md",
    ["provider's native", "streams into Harness ledgers", "Resume must use the provider-native session id"],
  ],
  [
    "docs/decisions/0032-provider-native-session-is-execution-truth.md",
    ["sole source of truth", "NativeSessionRef", "Harness does not persist", "Migration outcome"],
  ],
  [
    "docs/current/architecture/data-model.md",
    ["provider-native store owns transcript", "ephemeral read projection"],
  ],
  // execution-foundation.md was retired by DOC-108; its native-session
  // invariant now lives in the retained adapter contract below.
  [
    "docs/current/integration/native-session-storage.md",
    [
      "NativeSessionAdapter",
      "Write boundary",
      "Resume flow",
      "Provider matrix",
      "sole truth for",
      "does not keep a second provider event history",
      "joined read model, not a transcript database",
    ],
  ],
  // member-run-focus.md and team-run-war-room.md were retired with the
  // TeamRun-era surfaces (DEV-38/DOC-107); the Member contract moved to the
  // successor focus page, and the war-room surface no longer exists.
  [
    "docs/current/dashboard/pages/agent-member-focus.md",
    [
      "team_session_read",
      "same-machine local Operator",
      "mirrored history",
    ],
  ],
]);

const failures = [];

for (const [path, phrases] of requirements) {
  const text = readFileSync(path, "utf8");
  for (const phrase of phrases) {
    if (!text.includes(phrase)) failures.push(`${path}: missing ${JSON.stringify(phrase)}`);
  }
}

for (const retiredPath of [
  "schemas/provider-session.schema.json",
  "schemas/fixtures/provider-session/valid/codex.json",
  "schemas/fixtures/provider-session/invalid/missing-provider.json",
]) {
  try {
    readFileSync(retiredPath, "utf8");
    failures.push(`${retiredPath}: retired provider-session mirror must be absent`);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

const retiredProductionSymbols = new Map([
  ["crates/firm-core/src/lib.rs", ["pub struct ProviderSession", "provider_session_id:", "acp_session_id:"]],
  ["crates/firm-store/src/lib.rs", ["append_provider_session", "pub fn provider_sessions"]],
  ["apps/agent-dashboard/src/types.ts", ["provider_sessions:", "ProviderSession"]],
  ["crates/firm-cli/src/sse.rs", ["provider_sessions"]],
]);

for (const [path, symbols] of retiredProductionSymbols) {
  const text = readFileSync(path, "utf8");
  for (const symbol of symbols) {
    if (text.includes(symbol)) {
      failures.push(`${path}: retired provider-session mirror symbol remains: ${symbol}`);
    }
  }
}

const registry = JSON.parse(readFileSync("docs/registry.json", "utf8"));
const registered = new Set(registry.documents.map((document) => document.path));
for (const path of [
  "docs/decisions/0032-provider-native-session-is-execution-truth.md",
  "docs/current/integration/native-session-storage.md",
]) {
  if (!registered.has(path)) failures.push(`docs/registry.json: missing ${path}`);
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`validated provider-native session boundary across ${requirements.size} canonical documents`);
