#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = join(
  ROOT,
  "specs/retirement/dynamic-workflow-bound-register.v1.json",
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
