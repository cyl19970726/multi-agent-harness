#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const token = "org-cli-smoke-token";
const NOW = "2026-07-25T11:00:00+08:00";

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

function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolvePort(address.port));
    });
  });
}

async function waitFor(url) {
  const deadline = Date.now() + 30_000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
      lastError = new Error(`HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  }
  throw new Error(`server did not become ready: ${lastError?.message ?? "timeout"}`);
}

async function post(base, path, body) {
  const response = await fetch(`${base}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-harness-company-os-token": token },
    body: JSON.stringify(body),
  });
  const data = await response.json();
  if (!response.ok || data.ok === false) {
    throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  }
  return data.result ?? data;
}

async function main() {
  execFileSync("cargo", ["build", "-q", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });
  const root = await mkdtemp(join(tmpdir(), "company-os-org-cli-smoke-"));
  const env = { ...process.env, HARNESS_ROOT: join(root, "store"), HARNESS_COMPANY_OS_TOKEN: token };
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(harness, ["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  await waitFor(`${base}/health`);

  await post(base, "/v1/company-os/actors", {
    actor_type: "human",
    actor: {
      id: "human-org-owner",
      display_name: "Org Owner",
      title: "Owner",
      status: "active",
      availability: "available",
      membership_refs: [],
      responsibility_summary: "Owns Organization CLI smoke.",
      permission_policy_refs: ["company_os.admin", "company.records.write"],
      authority_policy_refs: ["company_os.admin"],
      created_at: NOW,
      updated_at: NOW,
    },
  });

  server.kill();
  await new Promise((resolveWait) => server.once("exit", resolveWait));

  const human = run([
    "company", "org", "create-human",
    "--id", "human-brand-owner",
    "--display-name", "Brand Owner",
    "--title", "Founder",
    "--responsibility", "Accountable human for brand and IP decisions.",
    "--permission", "company.records.write",
    "--authority-policy", "policy-brand-approval",
    "--authority", "human-org-owner",
  ], env);
  check(human.ok === true && human.result?.record?.actor?.id === "human-brand-owner", "org create-human appends a Human through administrative boundary");

  const unit = run([
    "company", "org", "create-unit",
    "--id", "org-brand-ip",
    "--organization", "company",
    "--name", "Brand & IP",
    "--purpose", "Own brand, trademark, and IP operations.",
    "--human-lead", "human-brand-owner",
    "--policy", "policy-brand-ip",
    "--document-space", "space-brand-ip",
    "--authority", "human-org-owner",
  ], env);
  check(unit.ok === true && unit.result?.record?.id === "org-brand-ip", "org create-unit appends an OrgUnit with explicit lead and document space");

  const agent = run([
    "company", "org", "create-agent",
    "--id", "agent-trademark",
    "--display-name", "Trademark Agent",
    "--role", "Trademark operations",
    "--responsibility", "Handles trademark preparation and filing work.",
    "--capability", "trademark.search",
    "--permission", "company.records.write",
    "--skill", "company-docs-operator",
    "--skill", "company-work-operator",
    "--accepted-work-type", "work-type-legal-filing",
    "--maintained-document", "document-trademark-root",
    "--authority", "human-org-owner",
  ], env);
  check(agent.ok === true && agent.result?.record?.actor?.id === "agent-trademark", "org create-agent appends a durable Standing Agent");

  const membership = run([
    "company", "org", "add-membership",
    "--id", "membership-trademark-agent-brand-ip",
    "--unit", "org-brand-ip",
    "--actor", "agent-trademark",
    "--actor-kind", "agent",
    "--role", "member",
    "--title", "Trademark operator",
    "--authority-policy", "policy-brand-ip-member",
    "--authority", "human-org-owner",
  ], env);
  check(membership.ok === true && membership.result?.record?.actor_ref?.actor_id === "agent-trademark", "org add-membership links actor to OrgUnit");

  const permission = run([
    "company", "org", "update-permissions",
    "--actor", "agent-trademark",
    "--actor-kind", "agent",
    "--permission", "trademark.records.write",
    "--capability", "trademark.filing.prepare",
    "--authority", "human-org-owner",
  ], env);
  check(permission.ok === true && permission.result?.record?.actor?.permission_policy_refs?.includes("trademark.records.write"), "org update-permissions appends capability and permission refs");

  const paused = run([
    "company", "org", "transition-actor",
    "--actor", "agent-trademark",
    "--actor-kind", "agent",
    "--status", "paused",
    "--availability", "paused",
    "--authority", "human-org-owner",
  ], env);
  check(paused.ok === true && paused.result?.record?.actor?.status === "paused", "org transition-actor updates declared actor status without runtime side effects");

  const queried = run(["company", "org", "query", "--actor", "agent-trademark", "--actor-kind", "agent"], env);
  check(queried.ok === true && queried.result?.actor?.actor?.status === "paused", "org query returns latest actor truth");
  check(queried.result?.related_memberships?.some((row) => row.id === "membership-trademark-agent-brand-ip"), "org query includes related memberships");

  const listed = run(["company", "org", "list", "--unit", "org-brand-ip"], env);
  check(listed.ok === true && listed.result?.summary?.actor_count === 1, "org list filters actors by OrgUnit membership");
  check(listed.result?.boundaries?.standing_agent_is_not_agent_team_member_run === true, "org list declares Standing Agent is not Agent Team MemberRun");
  check(listed.result?.boundaries?.runtime_health_does_not_grant_authority === true, "org list declares runtime health does not grant authority");

  await rm(root, { recursive: true, force: true });
  console.log(`\nCompany OS Org CLI smoke: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
