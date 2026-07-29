#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const token = "work-cli-smoke-token";
const NOW = "2026-07-25T09:00:00+08:00";

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

function admin(record) {
  return {
    mode: "administrative",
    authority: { actor_type: "human", actor_id: "human-work-owner" },
    record,
  };
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
  const root = await mkdtemp(join(tmpdir(), "company-os-work-cli-smoke-"));
  const env = { ...process.env, HARNESS_ROOT: join(root, "store"), HARNESS_COMPANY_OS_TOKEN: token };
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

  await post(base, "/v1/company-os/actors", {
    actor_type: "human",
    actor: {
      id: "human-work-owner",
      display_name: "Work Owner",
      title: "Owner",
      status: "active",
      availability: "available",
      membership_refs: [],
      responsibility_summary: "Owns Work CLI smoke.",
      permission_policy_refs: ["company_os.admin", "company.records.write", "company.work.execute"],
      authority_policy_refs: ["company_os.admin"],
      created_at: NOW,
      updated_at: NOW,
    },
  });
  await post(base, "/v1/company-os/actors", admin({
    actor_type: "agent",
    actor: {
      id: "agent-work-governance",
      display_name: "Work Governance Agent",
      role: "Work governance",
      status: "active",
      availability: "available",
      assignment_capacity: 4,
      exclusive_assignment_ref: null,
      home_org_unit_ref: null,
      membership_refs: [],
      responsibility_summary: "Routes WorkItems.",
      capability_refs: ["company.records.write", "company.work.execute"],
      permission_policy_refs: ["company.records.write", "company.work.execute"],
      system_prompt_ref: "document-prompt-agent-work-governance",
      tool_refs: ["tool-company-work"],
      skill_refs: ["company-work-operator"],
      accepted_work_types: ["governance", "operations"],
      escalation_policy_ref: "policy-work-owner-escalation",
      runtime_refs: [],
      native_session_refs: [],
      created_at: NOW,
      updated_at: NOW,
    },
  }));
  await post(base, "/v1/company-os/documents", admin({
    id: "document-work-root",
    space_id: "company",
    parent_document_id: null,
    title: "Work root",
    kind: "page",
    lifecycle_status: "active",
    block_ids: [],
    template_ref: null,
    permission_policy_refs: ["company.records.write"],
    reference_refs: [],
    created_by: { actor_type: "human", actor_id: "human-work-owner" },
    updated_by: { actor_type: "human", actor_id: "human-work-owner" },
    created_at: NOW,
    updated_at: NOW,
  }));
  server.kill();
  await new Promise((resolveWait) => server.once("exit", resolveWait));

  const module = run([
    "company", "docs", "module", "create",
    "--id", "module-work-cli",
    "--root-document", "document-work-root",
    "--name", "Work CLI",
    "--purpose", "Acceptance module for Work CLI.",
    "--record-type", "work",
    "--default-view-id", "view-work-cli",
    "--default-view-title", "Work CLI fallback",
    "--authority", "human-work-owner",
  ], env);
  check(module.ok === true && module.result?.module_id === "module-work-cli", "module seed created through existing Docs CLI");

  const definition = run([
    "company", "docs", "page-definition", "create",
    "--id", "page-work-cli",
    "--module", "module-work-cli",
    "--fallback-view", "view-work-cli",
    "--purpose", "Declare scoped Work CLI commands.",
    "--package-id", "package-work-cli",
    "--authority", "human-work-owner",
    "--owner", "human-work-owner",
    "--action", "work_item.append",
    "--action", "assignment.append",
    "--action", "work_item.transition",
  ], env);
  check(definition.ok === true && definition.result?.definition_id === "page-work-cli", "page definition seed declares Work actions");

  const created = run([
    "company", "work", "create",
    "--definition", "page-work-cli",
    "--id", "workitem-cli-smoke",
    "--source-document", "document-work-root",
    "--module", "module-work-cli",
    "--title", "Run Work CLI smoke",
    "--objective", "Prove native Work CLI can create and transition a WorkItem.",
    "--description", "Create, assign, transition, and close one native WorkItem while preserving detail fields.",
    "--acceptance-criterion", "The WorkItem keeps a human-readable description.",
    "--acceptance-criterion", "The WorkItem records acceptance and context references.",
    "--context-ref-json", '{"kind":"document","id":"document-work-root"}',
    "--submitted-by", "agent-work-governance",
    "--accountable-owner", "agent-work-governance",
    "--assignee", "agent-work-governance",
    "--work-type", "operations",
    "--priority", "high",
    "--actor", "agent-work-governance",
  ], env);
  check(created.ok === true && created.result?.record?.id === "workitem-cli-smoke", "work create dispatches work_item.append through Action API");

  const assigned = run([
    "company", "work", "assign",
    "--definition", "page-work-cli",
    "--id", "assignment-cli-smoke",
    "--work-item", "workitem-cli-smoke",
    "--assignee", "agent-work-governance",
    "--assigned-by", "agent-work-governance",
    "--role", "owner",
    "--correlation-id", "corr-work-cli-smoke",
  ], env);
  check(assigned.ok === true && assigned.result?.record?.id === "assignment-cli-smoke", "work assign dispatches assignment.append without rewriting WorkItem");

  const inProgress = run([
    "company", "work", "transition",
    "--definition", "page-work-cli",
    "--work-item", "workitem-cli-smoke",
    "--status", "in_progress",
    "--actor", "agent-work-governance",
  ], env);
  check(inProgress.ok === true && inProgress.result?.record?.status === "in_progress", "work transition moves submitted to in_progress");

  const inReview = run([
    "company", "work", "transition",
    "--definition", "page-work-cli",
    "--work-item", "workitem-cli-smoke",
    "--status", "in_review",
    "--actor", "agent-work-governance",
    "--result-document", "document-work-root",
    "--evidence", "evidence-work-cli-smoke",
    "--deliverable-ref-json", '{"kind":"evidence","id":"evidence-work-cli-smoke"}',
    "--outcome-summary", "Work CLI smoke produced durable evidence.",
  ], env);
  check(inReview.ok === true && inReview.result?.record?.status === "in_review", "work transition requires result/evidence/outcome before in_review");

  const closed = run([
    "company", "work", "close",
    "--definition", "page-work-cli",
    "--work-item", "workitem-cli-smoke",
    "--actor", "agent-work-governance",
  ], env);
  check(closed.ok === true && closed.result?.record?.status === "completed", "work close moves in_review to completed");

  const queried = run(["company", "work", "query", "--work-item", "workitem-cli-smoke"], env);
  check(queried.ok === true && queried.result?.assignments?.length === 1 && queried.result?.work_item?.status === "completed", "work query returns WorkItem plus native Assignment context");
  check(
    queried.result?.work_item?.description?.includes("Create, assign")
      && queried.result?.work_item?.acceptance_criteria?.length === 2
      && queried.result?.work_item?.context_refs?.[0]?.id === "document-work-root"
      && queried.result?.work_item?.deliverable_refs?.[0]?.id === "evidence-work-cli-smoke",
    "work query preserves WorkItem description, acceptance criteria, context refs, and deliverable refs",
  );

  const listed = run(["company", "work", "list", "--status", "completed", "--module", "module-work-cli"], env);
  check(listed.ok === true && listed.result?.summary?.completed === 1 && listed.result?.board?.completed?.includes("workitem-cli-smoke"), "work list returns filtered native Work projection");

  const boundaries = listed.boundaries ?? listed.result?.boundaries;
  check(JSON.stringify(boundaries).includes("goal_phase") && JSON.stringify(boundaries).includes("task_graph"), "work list declares no Project, Task Graph, or GoalPhase boundary");

  await rm(root, { recursive: true, force: true });
  console.log(`\nCompany OS Work CLI smoke: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
