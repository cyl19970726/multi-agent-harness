import { readFileSync } from "node:fs";

const requirements = new Map([
  [
    "AGENTS.md",
    ["provider's native", "streams into Harness ledgers", "Resume must use the provider-native session id"],
  ],
  [
    "docs/decisions/0032-provider-native-session-is-execution-truth.md",
    ["sole source of truth", "NativeSessionRef", "Harness does not persist", "Migration order"],
  ],
  [
    "docs/data-model.md",
    ["provider-native store owns transcript", "ephemeral read projection"],
  ],
  [
    "docs/company-os/execution-foundation.md",
    ["sole truth for", "does not keep a second provider event history"],
  ],
  [
    "docs/integration/native-session-storage.md",
    ["NativeSessionAdapter", "Write boundary", "Resume flow", "Provider matrix"],
  ],
  [
    "docs/dashboard/pages/member-run-focus.md",
    ["NativeActivityProjection", "does not silently fall back to a mirrored history"],
  ],
  [
    "docs/dashboard/pages/team-run-war-room.md",
    ["joined read model, not a transcript database"],
  ],
]);

const failures = [];

for (const [path, phrases] of requirements) {
  const text = readFileSync(path, "utf8");
  for (const phrase of phrases) {
    if (!text.includes(phrase)) failures.push(`${path}: missing ${JSON.stringify(phrase)}`);
  }
}

const registry = JSON.parse(readFileSync("docs/registry.json", "utf8"));
const registered = new Set(registry.documents.map((document) => document.path));
for (const path of [
  "docs/decisions/0032-provider-native-session-is-execution-truth.md",
  "docs/integration/native-session-storage.md",
]) {
  if (!registered.has(path)) failures.push(`docs/registry.json: missing ${path}`);
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`validated provider-native session boundary across ${requirements.size} canonical documents`);
