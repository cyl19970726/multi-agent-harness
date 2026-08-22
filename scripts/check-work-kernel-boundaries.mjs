#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));

function trackedAndUntrackedFiles() {
  return execFileSync(
    "git",
    ["ls-files", "-co", "--exclude-standard", "-z"],
    { cwd: root },
  )
    .toString("utf8")
    .split("\0")
    .filter(Boolean);
}

function read(path) {
  return readFileSync(resolve(root, path), "utf8");
}

const historicalVocabularyAllowlist = new Map([
  ["collab-skill-workspace/iteration-1/eval-2/old_skill/outputs/answer.md", "frozen Skill evaluation output"],
  ["collab-skill-workspace/iteration-1/review.html", "frozen Skill evaluation report"],
  ["collab-skill-workspace/skill-snapshot/SKILL.md", "frozen pre-cutover Skill snapshot"],
  ["docs/current/operations/evidence/dev-7-wave6-two-mac-v2/source-work.jsonl", "immutable acceptance evidence"],
  ["docs/current/operations/evidence/dev-7-wave6-two-mac-v2/target-work.jsonl", "immutable acceptance evidence"],
  ["docs/current/operations/evidence/dev-7-wave6-two-mac-v3/source-work.jsonl", "immutable acceptance evidence"],
  ["docs/current/operations/evidence/dev-7-wave6-two-mac-v3/target-work.jsonl", "immutable acceptance evidence"],
  ["docs/decisions/0050-agent-team-work-board-and-message-boundary.md", "amended historical ADR body preserved by ADR 0058"],
  ["docs/decisions/0058-work-dependency-dag-and-kernel-boundary.md", "supersession rationale names the retired vocabulary"],
  ["specs/nested-agent-team-organization/design.md", "superseded design evidence"],
  ["specs/nested-agent-team-organization/requirements.md", "superseded requirements evidence"],
  ["specs/nested-agent-team-organization/tasks.md", "superseded task evidence"],
  ["specs/organization-company-work/design.md", "superseded Company OS design evidence"],
  ["specs/organization-company-work/exploration.md", "superseded Company OS research evidence"],
  ["scripts/check-work-kernel-boundaries.mjs", "the gate must name the forbidden vocabulary"],
]);

const textExtensions = new Set([
  ".json", ".jsonl", ".md", ".html", ".js", ".mjs", ".rs", ".ts", ".tsx", ".toml", ".yaml", ".yml",
]);
const forbiddenWorkContainment = [
  { label: "retired Work containment field", pattern: /parent_work_id/i },
  { label: "legacy decode projection outside its exact core seam", pattern: /legacy_containment_ref/i },
  { label: "retired child-Work vocabulary", pattern: /\bchild[\s_-]+work\b/i },
];
const failures = [];
const repositoryFiles = trackedAndUntrackedFiles();
const exactLegacyDecodeSeam = new Set([
  "crates/firm-core/src/work.rs",
  "crates/firm-core/src/lib_tests/work_contracts.rs",
]);

for (const [path, reason] of historicalVocabularyAllowlist) {
  if (!reason.trim()) failures.push(`${path}: allowlist reason is empty`);
  if (!existsSync(resolve(root, path))) {
    failures.push(`${path}: allowlisted path is absent`);
    continue;
  }
  if (path === "scripts/check-work-kernel-boundaries.mjs") continue;
  const content = read(path);
  if (!forbiddenWorkContainment.some(({ pattern }) => pattern.test(content))) {
    failures.push(`${path}: stale allowlist row contains no retired Work vocabulary`);
  }
}

for (const path of repositoryFiles) {
  if (!textExtensions.has(extname(path))) continue;
  if (historicalVocabularyAllowlist.has(path)) continue;
  if (!existsSync(resolve(root, path))) continue;
  const content = read(path);
  for (const { label, pattern } of forbiddenWorkContainment) {
    const seamAllowsPattern = exactLegacyDecodeSeam.has(path) &&
      label !== "retired child-Work vocabulary";
    if (seamAllowsPattern) continue;
    if (pattern.test(content)) failures.push(`${path}: ${label}`);
  }
}

const legacyDecodeDeclaration = read("crates/firm-core/src/work.rs");
for (const required of [
  '#[serde(default, rename = "parent_work_id", skip_serializing)]',
  "pub legacy_containment_ref: Option<String>",
]) {
  if (!legacyDecodeDeclaration.includes(required)) {
    failures.push(`crates/firm-core/src/work.rs: exact read-only legacy decode seam is missing ${required}`);
  }
}
const legacyDecodeTest = read("crates/firm-core/src/lib_tests/work_contracts.rs");
for (const required of [
  '"parent_work_id": "historical-parent"',
  'value.get("parent_work_id").is_none()',
  'value.get("legacy_containment_ref").is_none()',
]) {
  if (!legacyDecodeTest.includes(required)) {
    failures.push(`crates/firm-core/src/lib_tests/work_contracts.rs: compatibility assertion is missing ${required}`);
  }
}

const coreManifest = read("crates/firm-core/Cargo.toml");
for (const forbidden of [
  "firm-store",
  "firm-application",
  "firm-cli",
  "firm-fabric",
  "firm-runtime-contract",
  "firm-runtime-host",
  "firm-runtime-supervisor",
]) {
  if (coreManifest.includes(forbidden)) {
    failures.push(`crates/firm-core/Cargo.toml: core must not depend on ${forbidden}`);
  }
}

const storeManifest = read("crates/firm-store/Cargo.toml");
for (const forbidden of [
  "firm-cli",
  "firm-runtime-host",
  "firm-runtime-supervisor",
  "firm-provider-",
]) {
  if (storeManifest.includes(forbidden)) {
    failures.push(`crates/firm-store/Cargo.toml: store must not depend on ${forbidden}`);
  }
}
if (!storeManifest.includes("firm-core")) {
  failures.push("crates/firm-store/Cargo.toml: store must depend inward on firm-core");
}

const applicationManifest = read("crates/firm-application/Cargo.toml");
if (!applicationManifest.includes("firm-core")) {
  failures.push("crates/firm-application/Cargo.toml: application must depend inward on firm-core");
}
for (const forbidden of [
  "firm-store",
  "firm-cli",
  "firm-fabric",
  "firm-runtime-contract",
  "firm-runtime-host",
  "firm-runtime-supervisor",
  "firm-provider-",
]) {
  if (applicationManifest.includes(forbidden)) {
    failures.push(`crates/firm-application/Cargo.toml: application must not depend on ${forbidden}`);
  }
}

for (const required of ["firm-core", "firm-application"]) {
  if (!storeManifest.includes(required)) {
    failures.push(`crates/firm-store/Cargo.toml: store must depend on ${required}`);
  }
}

const cliManifest = read("crates/firm-cli/Cargo.toml");
for (const required of ["firm-application", "firm-store"]) {
  if (!cliManifest.includes(required)) {
    failures.push(`crates/firm-cli/Cargo.toml: CLI composition root must consume ${required}`);
  }
}

const applicationServicePath = "crates/firm-application/src/work_service.rs";
if (!existsSync(resolve(root, applicationServicePath))) {
  failures.push(`${applicationServicePath}: Work application service is missing`);
} else {
  const applicationService = read(applicationServicePath);
  for (const required of ["pub trait WorkPersistence", "pub struct WorkApplication"]) {
    if (!applicationService.includes(required)) {
      failures.push(`${applicationServicePath}: missing ${required}`);
    }
  }
}
const storeApplicationPath = "crates/firm-store/src/store_work_application.rs";
if (!existsSync(resolve(root, storeApplicationPath))) {
  failures.push(`${storeApplicationPath}: WorkPersistence adapter is missing`);
} else if (!read(storeApplicationPath).includes("impl WorkPersistence for HarnessStore")) {
  failures.push(`${storeApplicationPath}: HarnessStore must implement WorkPersistence`);
}

const maintainedRoots = ["apps/", "crates/", "docs/current/", "plugins/star-harness/skills/", "schemas/", "scripts/", "skills/"];
const lineCheckedExtensions = new Set([".js", ".mjs", ".md", ".rs", ".ts", ".tsx"]);
for (const path of repositoryFiles) {
  if (!maintainedRoots.some((prefix) => path.startsWith(prefix))) continue;
  if (!lineCheckedExtensions.has(extname(path))) continue;
  if (path.includes("/fixtures/") || path.includes("/operations/evidence/")) continue;
  if (!existsSync(resolve(root, path))) continue;
  if (!statSync(resolve(root, path)).isFile()) continue;
  const lines = read(path).split(/\r?\n/).length;
  if (lines > 1500) failures.push(`${path}: ${lines} lines exceeds the maintained-file limit of 1500`);
}

if (failures.length) {
  console.error("Work kernel/package boundary check failed:");
  for (const failure of failures.sort()) console.error(`- ${failure}`);
  process.exit(1);
}

console.log(
  `Work kernel/package boundaries verified; ${historicalVocabularyAllowlist.size - 1} exact historical/rationale paths allowlisted`,
);
