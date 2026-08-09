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
  const root = await mkdtemp(join(tmpdir(), "company-os-work-cli-smoke-"));
  const project = join(root, "project");
  const env = { ...process.env, FIRM_HOME: join(root, "home") };
  execFileSync("mkdir", ["-p", project]);
  execFileSync(harness, ["init"], { cwd: project, env, stdio: "ignore" });

  const listed = run(["company", "work", "list"], env);
  check(listed.ok === true, "company work list succeeds without a second Company task ledger");
  check(listed.result?.authority === "team_work", "Company Work declares TeamWork authority");
  check(listed.result?.read_only === true, "Company Work projection is read-only");
  check(Array.isArray(listed.result?.works) && listed.result.works.length === 0, "empty truth stays empty without fallback rows");
  check(listed.boundaries?.company_work_creates_second_object === false, "CLI declares no duplicate Company Work object");
  check(listed.boundaries?.mutation_route === "team-run work", "mutations route to the native Team Work command");

  const filtered = run([
    "company", "work", "list",
    "--phase", "active",
    "--condition", "blocked",
    "--team-id", "team-example",
  ], env);
  check(filtered.result?.query?.phases?.[0] === "active", "phase filter reaches the unified projection");
  check(filtered.result?.query?.conditions?.[0] === "blocked", "condition filter reaches the unified projection");
  check(filtered.result?.query?.team_ids?.[0] === "team-example", "Team filter reaches the unified projection");

  let legacyRejected = false;
  try {
    execFileSync(harness, ["company", "work", "create"], { cwd: project, env, stdio: "pipe" });
  } catch (error) {
    legacyRejected = String(error.stderr ?? "").includes("read-only")
      || String(error.stderr ?? "").includes("team-run work");
  }
  check(legacyRejected, "legacy Company Work mutation commands are rejected");

  await rm(root, { recursive: true, force: true });
  console.log(`\nUnified Company Work CLI smoke: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
