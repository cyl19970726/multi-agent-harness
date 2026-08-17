// INACTIVE HISTORICAL (DOC-108 Stage B): this gate exercised the retired
// legacy CompanyOS surface and is removed from every pipeline. Kept as
// source-only history per the inactive-historical convention (file kept,
// removed from pipelines, named replacement) — see
// docs/current/operations/operations.md.
// Replacement: none — the Company operator surface is retired (DOC-108)

#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "firm");
const token = "company-os-operator-cli-smoke-token";
const NOW = "2026-07-27T09:00:00+08:00";

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

function admin(record) {
  return {
    mode: "administrative",
    authority: { actor_type: "human", actor_id: "human-admin" },
    record,
  };
}

async function main() {
  execFileSync("cargo", ["build", "-q", "-p", "firm-cli"], { cwd: repoRoot, stdio: "inherit" });
  const root = await mkdtemp(join(tmpdir(), "company-os-operator-cli-smoke-"));
  const executionEnv = { ...process.env, FIRM_HOME: join(root, "firm-home") };
  const env = { ...executionEnv, HARNESS_ROOT: join(root, "store"), HARNESS_COMPANY_OS_TOKEN: token };

  run(["space", "init", "--id", "operator-smoke-space"], executionEnv);
  run([
    "member-trust", "mutate",
    "--actor-kind", "human",
    "--actor-id", "human-admin",
    "--idempotency-key", "operator-smoke-create-member",
    "--expected-version", "0",
    "--json", JSON.stringify({
      command: "create_agent_member",
      member: {
        id: "member-ops",
        name: "Operations AgentMember",
        description: "Canonical execution identity for the operator smoke.",
        role: "operations governance",
        capabilities: ["company.records.write", "company.work.execute", "finance.commitment.write"],
        skill_refs: ["company-work-operator", "company-finance-operator"],
        provider_profile_ref: "codex-default",
        model_preference: null,
        workspace_policy: "managed-worktree",
        permission_ceiling: "workspace_write",
        organization_status: "active",
        version: 1,
        created_by: { kind: "human", id: "human-admin" },
        created_at: NOW,
        updated_at: NOW,
      },
    }),
  ], executionEnv);

  const socialReadiness = run(["company", "gateway", "social", "readiness"], env);
  check(socialReadiness.ok === true && socialReadiness.gateway === "social_content", "company gateway social readiness returns a social-content observation");
  check(socialReadiness.boundaries?.read_only === true && socialReadiness.boundaries?.store_side_effects === false && socialReadiness.boundaries?.publishing_side_effects === false, "social readiness command is read-only and cannot publish or write Store truth");
  check(["xiaohongshu", "douyin", "wechat_channels"].every((platform) => socialReadiness.platforms?.some((item) => item.platform === platform)), "social readiness reports Xiaohongshu, Douyin, and WeChat Channels slots");

  const human = run([
    "company", "org", "actor", "create-human",
    "--id", "human-admin",
    "--name", "Company Admin",
    "--title", "Human owner",
    "--responsibility", "Owns Company OS operator CLI smoke.",
    "--permission", "company_os.admin",
    "--permission", "company.records.write",
    "--permission", "company.work.execute",
    "--permission", "company.approve",
    "--permission", "finance.commitment.write",
    "--permission", "finance.payment.write",
    "--authority-policy", "company_os.admin",
    "--authority-policy", "company.approve",
  ], env);
  check(human.ok === true && human.result?.actor?.id === "human-admin", "org actor create-human bootstraps root Human");

  const agent = run([
    "company", "org", "actor", "create-agent",
    "--authority", "human-admin",
    "--id", "agent-ops",
    "--agent-member", "member-ops",
    "--execution-space", "operator-smoke-space",
    "--responsibility", "Routes work and requests finance effects.",
    "--permission", "company.records.write",
    "--permission", "company.work.execute",
    "--permission", "finance.commitment.write",
    "--permission", "finance.payment.write",
  ], env);
  check(agent.ok === true && agent.result?.actor?.id === "agent-ops", "org actor create-agent writes a Agent Membership");

  const unit = run([
    "company", "org", "unit", "create",
    "--authority", "human-admin",
    "--id", "unit-operations",
    "--name", "Operations",
    "--purpose", "Owns operating TeamWork and finance requests.",
    "--human-lead", "human-admin",
    "--agent-lead", "agent-ops",
    "--policy", "company.records.write",
  ], env);
  check(unit.ok === true && unit.result?.id === "unit-operations", "org unit create writes OrgUnit");

  const membership = run([
    "company", "org", "membership", "assign",
    "--authority", "human-admin",
    "--id", "membership-agent-ops",
    "--unit", "unit-operations",
    "--actor", "agent-ops",
    "--actor-kind", "agent",
    "--role", "lead",
    "--title", "Operations lead",
  ], env);
  check(membership.ok === true && membership.result?.id === "membership-agent-ops", "org membership assign links actor to OrgUnit");

  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(harness, ["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  server.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  server.stderr.on("data", (chunk) => logs.push(chunk.toString()));
  await waitFor(`${base}/health`);

  await post(base, "/v1/company-os/documents", admin({
    id: "document-ops-root",
    space_id: "company",
    parent_document_id: null,
    title: "Operations root",
    kind: "page",
    lifecycle_status: "active",
    block_ids: [],
    template_ref: null,
    permission_policy_refs: ["company.records.write"],
    reference_refs: [],
    created_by: { actor_type: "human", actor_id: "human-admin" },
    updated_by: { actor_type: "human", actor_id: "human-admin" },
    created_at: NOW,
    updated_at: NOW,
  }));

  const module = run([
    "company", "docs", "module", "create",
    "--id", "module-operator-cli",
    "--root-document", "document-ops-root",
    "--name", "Operator CLI",
    "--purpose", "Smoke module for org, work, approval and finance CLI.",
    "--record-type", "operator",
    "--default-view-id", "view-operator-cli",
    "--default-view-title", "Operator CLI fallback",
    "--authority", "human-admin",
  ], env);
  check(module.ok === true && module.result?.module_id === "module-operator-cli", "docs module create seeds module scope");

  const definition = run([
    "company", "docs", "page-definition", "create",
    "--id", "page-operator-cli",
    "--module", "module-operator-cli",
    "--fallback-view", "view-operator-cli",
    "--purpose", "Declare operator CLI action policies.",
    "--package-id", "package-operator-cli",
    "--authority", "human-admin",
    "--owner", "human-admin",
    "--action", "approval.request",
    "--action", "approval.decide",
    "--action", "commitment.propose",
    "--action", "commitment.append",
    "--action", "payment.append",
  ], env);
  check(definition.ok === true && definition.result?.definition_id === "page-operator-cli", "page definition declares approval and finance actions");

  const milestone = run([
    "company", "work", "milestone", "create",
    "--authority", "human-admin",
    "--id", "milestone-operator-cli",
    "--title", "Operator CLI Ready",
    "--outcome", "Operator CLI can route Company OS work and finance effects.",
    "--accountable-owner", "agent-ops",
    "--module", "module-operator-cli",
    "--source-document", "document-ops-root",
    "--work", "work-operator-cli",
    "--acceptance-criterion", "Org, approval, finance and milestone commands are callable.",
  ], env);
  check(milestone.ok === true && milestone.result?.id === "milestone-operator-cli", "work milestone create stores a native TeamWork reference without creating a Company task object");

  server.kill();
  await new Promise((resolveWait) => server.once("exit", resolveWait));
  await rm(root, { recursive: true, force: true });

  console.log(`\nCompany OS operator CLI smoke: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
