#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  lstatSync,
  readlinkSync,
  readdirSync,
  readFileSync,
  statSync,
} from "node:fs";
import { dirname, join, relative } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = join(
  ROOT,
  "specs/retirement/dynamic-workflow-bound-register.v1.json",
);
const COMPLETION_PATH = join(
  ROOT,
  "specs/retirement/dynamic-workflow-completion.v1.json",
);
const EXPECTED_SOURCE = {
  status: "accepted_design_evidence",
  product_effect:
    "none; Dynamic Workflow remains current until a separately reviewed implementation cutover",
  payload_sha256:
    "ca53ee4eef284435b84a4058ac032fa4499f8c19833102d46a2b90164ff12e3a",
  bundle_manifest_sha256:
    "aa059f32b1d8d2a39b28e16ec1a3d41b47b7cd965e3443fdb3b55482e28d7973",
  source_spec_body_sha256:
    "1888a2cca8362af474280aa06bc3d40ae890156a99b7c891866452a3ee18119c",
  source_register_compact_sha256:
    "75d6137cebf8d8a9571fb6e299d0459a73eecb8195768ad4fed38f9f2627046b",
};

function fail(message) {
  throw new Error(message);
}

function git(args, { allowNoMatches = false } = {}) {
  const result = spawnSync("git", args, {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.status === 0) return result.stdout;
  if (allowNoMatches && result.status === 1) return "";
  fail(
    `git ${args.join(" ")} failed (${result.status}): ${result.stderr.trim()}`,
  );
}

function sorted(values) {
  return [...values].sort((left, right) =>
    left < right ? -1 : left > right ? 1 : 0,
  );
}

function assertEqual(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(
      `${label} mismatch\nexpected: ${JSON.stringify(expected)}\nactual:   ${JSON.stringify(actual)}`,
    );
  }
}

function grepPaths(revision, args) {
  const prefix = `${revision}:`;
  return new Set(
    git(["grep", "-I", "-il", ...args, revision, "--"], {
      allowNoMatches: true,
    })
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        if (!line.startsWith(prefix)) {
          fail(`unexpected git grep output: ${line}`);
        }
        return line.slice(prefix.length);
      }),
  );
}

const manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
if (manifest.format !== "AF-DYNAMIC-WORKFLOW-RETIREMENT-MANIFEST-V1") {
  fail(`unsupported manifest format: ${manifest.format}`);
}

for (const [field, expected] of Object.entries(EXPECTED_SOURCE)) {
  if (manifest.provenance?.[field] !== expected) {
    fail(`provenance.${field} does not match the accepted source`);
  }
}

const register = manifest.register;
if (register?.format !== "AF-DYNAMIC-WORKFLOW-BOUND-REGISTER-V1") {
  fail(`unsupported register format: ${register?.format}`);
}

const compactHash = createHash("sha256")
  .update(JSON.stringify(register))
  .digest("hex");
assertEqual(
  compactHash,
  manifest.provenance.source_register_compact_sha256,
  "register compact SHA-256",
);

const proof = register.proof;
const revision = proof.bound_revision;
assertEqual(
  revision,
  manifest.provenance.bound_repository_revision,
  "bound revision",
);

if (git(["cat-file", "-t", revision]).trim() !== "commit") {
  fail(`bound revision is not a commit: ${revision}`);
}
git(["merge-base", "--is-ancestor", revision, "HEAD"]);

const treeLines = git(["ls-tree", "-r", revision])
  .split("\n")
  .filter(Boolean);
const tree = new Map();
for (const line of treeLines) {
  const match = line.match(/^[0-7]{6} blob ([0-9a-f]{40})\t(.+)$/);
  if (!match) fail(`unexpected ls-tree row: ${line}`);
  tree.set(match[2], match[1]);
}
assertEqual(tree.size, proof.tracked_paths, "tracked path count");

const pathName = new Set(
  [...tree.keys()].filter((path) => /workflow|\.star$/i.test(path)),
);
const contentCi = grepPaths(revision, ["-i", "workflow"]);
const semantic = grepPaths(revision, [
  "-E",
  "WorkflowRun|WorkflowStep|WorkflowPatch|WorkflowArtifactManifest|workflow_run|workflow_step|workflow_patch|workflow_artifact_manifest|firm[_-]workflow|run-script|run_script|compile_starlark|workflow\\(",
]);
const candidates = new Set([...pathName, ...contentCi, ...semantic]);

assertEqual(pathName.size, proof.path_name_count, "path-name candidate count");
assertEqual(contentCi.size, proof.content_ci_count, "content candidate count");
assertEqual(semantic.size, proof.semantic_count, "semantic candidate count");
assertEqual(candidates.size, proof.candidate_union_count, "candidate union count");

const requiredRowFields = [
  "path",
  "blob_oid",
  "matched_classes",
  "primary_disposition",
  "secondary_rows",
  "current_role",
  "read_write_behavior",
  "owner",
  "target",
  "slice",
  "prerequisite",
  "executable_check_evidence",
  "deletion_rule",
  "rollback_forward_repair_rule",
];
if (!Array.isArray(register.rows)) fail("register.rows must be an array");
assertEqual(register.rows.length, proof.row_count, "register row count");

const rowPaths = register.rows.map((row) => row.path);
assertEqual(rowPaths, sorted(rowPaths), "row path ordering");
assertEqual(new Set(rowPaths).size, rowPaths.length, "unique row path count");
assertEqual(rowPaths, sorted(candidates), "candidate coverage");

const primaryCounts = {};
for (const row of register.rows) {
  for (const field of requiredRowFields) {
    if (!Object.hasOwn(row, field)) {
      fail(`${row.path ?? "<unknown>"}: missing ${field}`);
    }
  }
  if (!tree.has(row.path)) fail(`${row.path}: stale path`);
  assertEqual(row.blob_oid, tree.get(row.path), `${row.path}: blob OID`);

  const matchedClasses = [];
  if (pathName.has(row.path)) matchedClasses.push("path_name");
  if (contentCi.has(row.path)) matchedClasses.push("content_ci");
  if (semantic.has(row.path)) matchedClasses.push("semantic");
  assertEqual(
    row.matched_classes,
    matchedClasses,
    `${row.path}: matched classes`,
  );

  if (!/^D(?:0[1-9]|1[0-9]|2[0-5])$/.test(row.primary_disposition)) {
    fail(`${row.path}: invalid primary disposition ${row.primary_disposition}`);
  }
  if (!Array.isArray(row.secondary_rows)) {
    fail(`${row.path}: secondary_rows must be an array`);
  }
  if (!row.owner || !row.target || !row.executable_check_evidence) {
    fail(`${row.path}: owner, target, and executable evidence are required`);
  }
  primaryCounts[row.primary_disposition] =
    (primaryCounts[row.primary_disposition] ?? 0) + 1;
}
assertEqual(primaryCounts, proof.primary_counts, "primary disposition totals");

for (const name of [
  "unmatched",
  "multiply_primary",
  "stale_rows",
  "ownerless",
]) {
  assertEqual(proof[name], [], `proof.${name}`);
}

const closure = proof.reference_edge_closure;
const edgeKeys = new Set();
for (const edge of closure.registered_reference_edges) {
  if (!Array.isArray(edge) || edge.length !== 2) {
    fail(`invalid reference edge: ${JSON.stringify(edge)}`);
  }
  const key = JSON.stringify(edge);
  if (edgeKeys.has(key)) fail(`duplicate reference edge: ${key}`);
  edgeKeys.add(key);
  for (const endpoint of edge) {
    if (!tree.has(endpoint)) fail(`reference endpoint is absent: ${endpoint}`);
    if (!candidates.has(endpoint)) {
      fail(`reference endpoint is outside the candidate register: ${endpoint}`);
    }
  }
}
for (const name of [
  "reference_edge_additions",
  "unregistered_reference_targets",
  "missing_reference_targets",
]) {
  assertEqual(closure[name], [], `reference_edge_closure.${name}`);
}
for (const [name, values] of Object.entries(proof.negative_gates)) {
  assertEqual(values, [], `negative_gates.${name}`);
}

console.log(
  `verified ${register.rows.length} Dynamic Workflow retirement rows at ${revision}`,
);
console.log(
  `candidate classes: ${pathName.size}/${contentCi.size}/${semantic.size}; union: ${candidates.size}`,
);
console.log(
  `source register SHA-256: ${manifest.provenance.source_register_compact_sha256}`,
);

// The accepted register is immutable design/provenance evidence. The
// completion policy below is a separate, current-HEAD gate: it proves the
// implementation did not merely preserve a good inventory while leaving the
// retired product surfaces reachable.
const completion = JSON.parse(readFileSync(COMPLETION_PATH, "utf8"));
if (completion.format !== "AF-DYNAMIC-WORKFLOW-RETIREMENT-COMPLETION-V1") {
  fail(`unsupported completion format: ${completion.format}`);
}
if (completion.task !== "DEV-56") fail("completion policy is not bound to DEV-56");

const startRevision = completion.retirement_task_start_revision;
if (git(["cat-file", "-t", startRevision]).trim() !== "commit") {
  fail(`retirement start revision is not a commit: ${startRevision}`);
}
git(["merge-base", "--is-ancestor", startRevision, "HEAD"]);

const startCandidates = new Set([
  ...grepPaths(startRevision, ["-i", "workflow"]),
  ...grepPaths(startRevision, [
    "-E",
    "WorkflowRun|WorkflowStep|WorkflowPatch|WorkflowArtifactManifest|workflow_run|workflow_step|workflow_patch|workflow_artifact_manifest|firm[_-]workflow|run-script|run_script|compile_starlark|workflow\\(",
  ]),
  ...git(["ls-tree", "-r", "--name-only", startRevision])
    .split("\n")
    .filter((path) => /workflow|\.star$/i.test(path)),
]);
assertEqual(
  startCandidates.size,
  completion.source_candidate_count,
  "retirement-start candidate count",
);
const registeredSourcePaths = new Set(register.rows.map((row) => row.path));
const extraStartCandidates = sorted(
  [...startCandidates].filter((path) => !registeredSourcePaths.has(path)),
);
assertEqual(
  extraStartCandidates,
  completion.source_governance_candidates,
  "retirement-start governance classification",
);
assertEqual(
  registeredSourcePaths.size,
  completion.source_register_rows,
  "retirement-start registered classification count",
);

function walkFiles(root) {
  const files = [];
  function walk(path) {
    for (const name of readdirSync(path)) {
      const child = join(path, name);
      if (statSync(child).isDirectory()) walk(child);
      else files.push(child);
    }
  }
  walk(root);
  return files.sort();
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function workingTreeBlob(path) {
  if (!lstatSync(path).isSymbolicLink()) {
    return git(["hash-object", "--", path]).trim();
  }
  const bytes = Buffer.from(readlinkSync(path));
  return createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

function treeSha256(root) {
  const hash = createHash("sha256");
  const files = walkFiles(root);
  for (const path of files) {
    hash.update(`${relative(root, path)}\0${sha256File(path)}\n`);
  }
  return { fileCount: files.length, digest: hash.digest("hex"), files };
}

const archivedSources = new Set();
for (const group of completion.historical_groups) {
  if (!group.reason) fail(`${group.archive_prefix}: historical reason is required`);
  const archiveRoot = join(ROOT, group.archive_prefix);
  if (!existsSync(archiveRoot)) fail(`${group.archive_prefix}: archive is absent`);
  const actual = treeSha256(archiveRoot);
  assertEqual(actual.fileCount, group.file_count, `${group.archive_prefix}: file count`);
  assertEqual(actual.digest, group.tree_sha256, `${group.archive_prefix}: tree SHA-256`);

  for (const path of actual.files) {
    if ((statSync(path).mode & 0o111) !== 0) {
      fail(`${relative(ROOT, path)}: archived file must not be executable`);
    }
    const sourcePath = `${group.source_prefix}${relative(archiveRoot, path)}`;
    const row = register.rows.find((candidate) => candidate.path === sourcePath);
    if (row) {
      const blob = workingTreeBlob(path);
      assertEqual(blob, row.blob_oid, `${sourcePath}: archived source blob`);
      archivedSources.add(sourcePath);
    }
  }
}
for (const historical of completion.historical_files) {
  if (!historical.reason) fail(`${historical.archive_path}: historical reason is required`);
  const path = join(ROOT, historical.archive_path);
  if (!existsSync(path)) fail(`${historical.archive_path}: archived file is absent`);
  assertEqual(sha256File(path), historical.sha256, `${historical.archive_path}: SHA-256`);
  const row = register.rows.find((candidate) => candidate.path === historical.source_path);
  if (!row) fail(`${historical.source_path}: historical file has no accepted register row`);
  assertEqual(
    workingTreeBlob(path),
    row.blob_oid,
    `${historical.source_path}: archived source blob`,
  );
  archivedSources.add(historical.source_path);
}

const startTree = new Map();
for (const line of git(["ls-tree", "-r", startRevision]).split("\n").filter(Boolean)) {
  const match = line.match(/^[0-7]{6} blob ([0-9a-f]{40})\t(.+)$/);
  if (!match) fail(`unexpected retirement-start tree row: ${line}`);
  startTree.set(match[2], match[1]);
}

// D20 is immutable ADR/history. D24 and D25 are deliberate false positives or
// unrelated operating Skills. Every other populated disposition required an
// implementation change, removal, or verified archive move during DEV-56.
const unchangedAllowed = new Set(["D20", "D24", "D25"]);
const completionCounts = {
  archived: 0,
  removed: 0,
  changed: 0,
  unchanged_allowed: 0,
  unchanged_incomplete: 0,
};
const incompleteRows = [];
for (const row of register.rows) {
  if (archivedSources.has(row.path)) {
    if (existsSync(join(ROOT, row.path))) fail(`${row.path}: active source survived archive move`);
    completionCounts.archived += 1;
    continue;
  }
  const currentPath = join(ROOT, row.path);
  if (!existsSync(currentPath)) {
    completionCounts.removed += 1;
    continue;
  }
  const currentBlob = workingTreeBlob(currentPath);
  if (currentBlob !== startTree.get(row.path)) {
    completionCounts.changed += 1;
    continue;
  }
  if (!unchangedAllowed.has(row.primary_disposition)) {
    completionCounts.unchanged_incomplete += 1;
    incompleteRows.push(`${row.path} (${row.primary_disposition})`);
    continue;
  }
  completionCounts.unchanged_allowed += 1;
}
assertEqual(
  Object.values(completionCounts).reduce((sum, count) => sum + count, 0),
  register.rows.length,
  "completion classification total",
);
if (incompleteRows.length > 0) {
  fail(
    `retirement rows remained byte-identical to the DEV-56 start revision:\n${incompleteRows.join("\n")}`,
  );
}

for (const path of completion.deleted_active_surfaces) {
  if (existsSync(join(ROOT, path))) fail(`${path}: retired active surface still exists`);
}
for (const path of [
  ".claude-plugin/plugin.json",
  "crates/firm-workflow",
  "crates/firm-cli/src/workflow.rs",
  "skills/star-workflow",
  "workflows",
  "evals",
  "examples/adapters/earning-engine",
  "scripts/multi-project-demo",
]) {
  if (existsSync(join(ROOT, path))) fail(`${path}: retired active path still exists`);
}

const forbiddenSignatures =
  /firm[_-]workflow|WorkflowRun|WorkflowStep|WorkflowPatch|WorkflowArtifactManifest|workflow_runs|workflow_steps|workflow_patches|workflow_artifact_manifests|run-script|run_script|compile_starlark|\/v1\/workflows|Commands?::Workflow/g;
const activeRoots = [
  "Cargo.toml",
  "Cargo.lock",
  "crates/firm-cli/Cargo.toml",
  "crates/firm-cli/src/main.rs",
  "crates/firm-cli/src/store_resolution.rs",
  "crates/firm-cli/src/sse.rs",
  "crates/firm-cli/src/mcp.rs",
  "apps/agent-dashboard/src",
  "schemas",
  "plugins",
];
const activeFiles = [];
for (const source of activeRoots) {
  const path = join(ROOT, source);
  if (!existsSync(path)) continue;
  if (statSync(path).isDirectory()) activeFiles.push(...walkFiles(path));
  else activeFiles.push(path);
}
const forbiddenHits = [];
for (const path of activeFiles) {
  const rel = relative(ROOT, path);
  if (rel.includes("/migrations/") || rel.endsWith("retired-dynamic-workflow.json")) continue;
  const text = readFileSync(path, "utf8");
  forbiddenSignatures.lastIndex = 0;
  for (const match of text.matchAll(forbiddenSignatures)) {
    forbiddenHits.push(`${rel}:${match[0]}`);
  }
}
assertEqual(forbiddenHits, [], "active forbidden Dynamic Workflow signatures");

// Core decode types and Store readers remain solely for lossless historical
// export/verification. They are not active capability, but every old Store
// writer must terminate at the explicit fail-closed boundary.
const coreText = readFileSync(join(ROOT, "crates/firm-core/src/lib.rs"), "utf8");
if (!coreText.includes("Retired Dynamic Workflow historical decode objects")) {
  fail("firm-core historical Workflow decode types are not marked retired");
}
const storeText = readFileSync(join(ROOT, "crates/firm-store/src/lib.rs"), "utf8");
for (const writer of [
  "append_workflow_run",
  "append_workflow_step",
  "append_workflow_patch",
  "append_workflow_artifact_manifest",
]) {
  const start = storeText.indexOf(`pub fn ${writer}`);
  if (start < 0) fail(`${writer}: historical compatibility writer seam is absent`);
  const body = storeText.slice(start, storeText.indexOf("\n    }", start) + 6);
  if (!body.includes("reject_dynamic_workflow_write") || body.includes("append_jsonl")) {
    fail(`${writer}: writer does not fail closed`);
  }
}

const packageJson = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
for (const scriptName of ["check", "check:fast"]) {
  if (!packageJson.scripts?.[scriptName]?.includes("check:dynamic-workflow-retirement")) {
    fail(`${scriptName}: retirement completion gate is not reachable`);
  }
}
assertEqual(
  packageJson.scripts?.["check:dynamic-workflow-retirement"],
  "node scripts/check-dynamic-workflow-retirement-manifest.mjs",
  "completion gate command",
);
const packageText = JSON.stringify(packageJson);
for (const retired of ["acceptance-workflow-starlark", "eval-workflows.mjs"]) {
  if (packageText.includes(retired)) fail(`package.json still references ${retired}`);
}
const ciText = readFileSync(join(ROOT, ".github/workflows/ci.yml"), "utf8");
if (!ciText.includes("pnpm check:fast") || !ciText.includes("pnpm check")) {
  fail("CI does not reach both fast and full package gates");
}
if (!ciText.includes("workflow_dispatch:")) {
  fail("ordinary GitHub workflow_dispatch was removed by mistake");
}

const forbiddenDistributionHits = git(
  [
    "grep",
    "-I",
    "-n",
    "-E",
    "skills/star-workflow|--skill star-workflow|acceptance:workflow-starlark|scripts/eval-workflows",
    "--",
    ".claude-plugin",
    ".github",
    "package.json",
    "plugins",
    "skills",
  ],
  { allowNoMatches: true },
)
  .split("\n")
  .filter(Boolean);
assertEqual(forbiddenDistributionHits, [], "active distribution references");

console.log(
  `verified DEV-56 completion: ${completion.source_candidate_count} source candidates classified; ` +
    `${JSON.stringify(completionCounts)}`,
);
console.log(
  `verified ${completion.historical_groups.length + completion.historical_files.length} historical allowlist groups/files and zero active forbidden signatures`,
);
