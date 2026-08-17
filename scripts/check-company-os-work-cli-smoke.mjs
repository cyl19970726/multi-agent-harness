#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "firm");
let passed = 0;
let failed = 0;

function check(condition, message) {
  if (condition) {
    console.log(`  PASS  ${message}`);
    passed += 1;
  } else {
    console.log(`  FAIL  ${message}`);
    failed += 1;
  }
}

function run(args, env) {
  return JSON.parse(execFileSync(harness, args, { cwd: repoRoot, env, encoding: "utf8" }));
}

async function main() {
  execFileSync("cargo", ["build", "-q", "-p", "firm-cli"], { cwd: repoRoot, stdio: "inherit" });
  const root = await mkdtemp(join(tmpdir(), "global-work-cli-smoke-"));
  const project = join(root, "project");
  const env = { ...process.env, FIRM_HOME: join(root, "home") };
  execFileSync("mkdir", ["-p", project]);
  execFileSync(harness, ["init"], { cwd: project, env, stdio: "ignore" });

  const listed = run(["work", "list"], env);
  check(listed.ok === true, "global work list succeeds without a second aggregate ledger");
  check(listed.result?.view_kind === "global_work", "Global Work view kind is global_work");
  check(Array.isArray(listed.result?.data?.items) && listed.result.data.items.length === 0, "empty truth stays empty without fallback rows");
  check(Array.isArray(listed.result?.data?.pending_migration_work_ids), "Global Work exposes an honest pending-migration list");
  check(listed.boundaries?.global_work_creates_second_object === false, "CLI declares no duplicate Global Work object");
  check(String(listed.boundaries?.mutation_route ?? "").includes("team-run work"), "mutations route to the native Work command");

  const filtered = run([
    "work", "list",
    "--phase", "active",
    "--condition", "blocked",
    "--team-id", "team-example",
    "--assignee-kind", "unassigned",
  ], env);
  check(filtered.result?.data?.query?.phase?.[0] === "active", "phase filter reaches the Global Work projection");
  check(filtered.result?.data?.query?.condition?.[0] === "blocked", "condition filter reaches the Global Work projection");
  check(filtered.result?.data?.query?.team_id?.[0] === "team-example", "Team filter reaches the Global Work projection");
  check(filtered.result?.data?.query?.assignee_kind?.[0] === "unassigned", "assignee_kind filter reaches the Global Work projection");

  let legacyRejected = false;
  try {
    execFileSync(harness, ["company", "work", "list"], { cwd: project, env, stdio: "pipe" });
  } catch (error) {
    legacyRejected = String(error.stderr ?? "").includes("harness work list");
  }
  check(legacyRejected, "retired Company Work CLI names fail closed toward `harness work list`");

  let legacyMutationRejected = false;
  try {
    execFileSync(harness, ["company", "work", "create"], { cwd: project, env, stdio: "pipe" });
  } catch (error) {
    legacyMutationRejected = String(error.stderr ?? "").includes("read-only")
      || String(error.stderr ?? "").includes("harness work list")
      || String(error.stderr ?? "").includes("team-run work")
      || String(error.stderr ?? "").includes("unknown company work command");
  }
  check(legacyMutationRejected, "legacy Company Work mutation commands are rejected");

  await rm(root, { recursive: true, force: true });
  console.log(`\nGlobal Work CLI smoke: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
