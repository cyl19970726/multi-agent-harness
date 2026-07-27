#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
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
  execFileSync("cargo", ["build", "-q", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });
  const root = await mkdtemp(join(tmpdir(), "company-os-operator-cli-smoke-"));
  const env = { ...process.env, HARNESS_ROOT: join(root, "store"), HARNESS_COMPANY_OS_TOKEN: token };

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
    "--name", "Operations Agent",
    "--role", "Operations governance",
    "--responsibility", "Routes work and requests finance effects.",
    "--capability", "company.records.write",
    "--capability", "company.work.execute",
    "--capability", "finance.commitment.write",
    "--permission", "company.records.write",
    "--permission", "company.work.execute",
    "--permission", "finance.commitment.write",
    "--permission", "finance.payment.write",
    "--skill", "company-work-operator",
    "--skill", "company-finance-operator",
  ], env);
  check(agent.ok === true && agent.result?.actor?.id === "agent-ops", "org actor create-agent writes a Standing Agent");

  const unit = run([
    "company", "org", "unit", "create",
    "--authority", "human-admin",
    "--id", "unit-operations",
    "--name", "Operations",
    "--purpose", "Owns operating WorkItems and finance requests.",
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
    "--action", "work_item.append",
    "--action", "work_item.transition",
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
    "--acceptance-criterion", "Org, approval, finance and milestone commands are callable.",
  ], env);
  check(milestone.ok === true && milestone.result?.id === "milestone-operator-cli", "work milestone create writes native Milestone");

  const work = run([
    "company", "work", "create",
    "--definition", "page-operator-cli",
    "--id", "workitem-operator-cli",
    "--source-document", "document-ops-root",
    "--module", "module-operator-cli",
    "--milestone", "milestone-operator-cli",
    "--title", "Buy launch prize",
    "--objective", "Create a governed finance request from a WorkItem.",
    "--submitted-by", "agent-ops",
    "--accountable-owner", "agent-ops",
    "--assignee", "agent-ops",
    "--work-type", "procurement",
    "--actor", "agent-ops",
  ], env);
  check(work.ok === true && work.result?.record?.id === "workitem-operator-cli", "work create provides finance source WorkItem");

  await post(base, "/v1/company-os/relations", admin({
    id: "relation-work-commitment-cli",
    from_ref: { kind: "work_item", id: "workitem-operator-cli" },
    relation_type: "requests_financial_commitment",
    to_ref: { kind: "document", id: "document-ops-root" },
    provenance_ref: { kind: "document", id: "document-ops-root" },
    lifecycle_status: "active",
    created_by: { actor_type: "agent", actor_id: "agent-ops" },
    created_at: NOW,
  }));

  const commitment = run([
    "company", "finance", "commitment", "propose",
    "--definition", "page-operator-cli",
    "--id", "commitment-operator-cli",
    "--work-item", "workitem-operator-cli",
    "--source-document", "document-ops-root",
    "--submitted-by", "agent-ops",
    "--accountable-owner", "agent-ops",
    "--amount", "3000",
    "--currency", "CNY",
    "--relation", "relation-work-commitment-cli",
  ], env);
  check(commitment.ok === true && commitment.result?.record?.status === "proposed", "finance commitment propose writes proposed Commitment through Action API");

  const commitmentApproval = run([
    "company", "approval", "request",
    "--definition", "page-operator-cli",
    "--id", "approval-commitment-cli",
    "--subject-kind", "financial_record",
    "--subject", "commitment-operator-cli",
    "--summary", "Approve commitment.append for launch prize budget.",
    "--requested-by", "agent-ops",
    "--approval-policy-ref", "page-operator-cli:commitment.append",
    "--required-approver", "human-admin",
    "--evidence", "evidence-commitment-request",
  ], env);
  check(commitmentApproval.ok === true && commitmentApproval.result?.record?.status === "requested", "approval request creates requested human Approval");

  const pending = run([
    "company", "finance", "commitment", "transition",
    "--definition", "page-operator-cli",
    "--commitment", "commitment-operator-cli",
    "--status", "pending_approval",
    "--actor", "agent-ops",
    "--approval", "approval-commitment-cli",
    "--evidence", "evidence-commitment-request",
  ], env);
  check(pending.ok === true && pending.result?.record?.status === "pending_approval", "finance commitment transition enters pending_approval with requested Approval");

  const approvedCommitmentApproval = run([
    "company", "approval", "decide",
    "--definition", "page-operator-cli",
    "--approval", "approval-commitment-cli",
    "--actor", "human-admin",
    "--decision", "approved",
    "--note", "Approved for smoke test.",
    "--evidence", "evidence-commitment-decision",
  ], env);
  check(approvedCommitmentApproval.ok === true && approvedCommitmentApproval.result?.record?.status === "approved", "approval decide records human decision");

  const approvedCommitment = run([
    "company", "finance", "commitment", "transition",
    "--definition", "page-operator-cli",
    "--commitment", "commitment-operator-cli",
    "--status", "approved",
    "--actor", "agent-ops",
    "--approval", "approval-commitment-cli",
    "--evidence", "evidence-commitment-decision",
  ], env);
  check(approvedCommitment.ok === true && approvedCommitment.result?.record?.status === "approved", "finance commitment transition uses approved human Approval");

  const paymentApproval = run([
    "company", "approval", "request",
    "--definition", "page-operator-cli",
    "--id", "approval-payment-cli",
    "--subject-kind", "financial_record",
    "--subject", "commitment-operator-cli",
    "--summary", "Approve payment.append for launch prize purchase.",
    "--requested-by", "agent-ops",
    "--approval-policy-ref", "page-operator-cli:payment.append",
    "--required-approver", "human-admin",
    "--evidence", "evidence-payment-request",
  ], env);
  check(paymentApproval.ok === true && paymentApproval.result?.record?.status === "requested", "approval request can target payment policy");

  run([
    "company", "approval", "decide",
    "--definition", "page-operator-cli",
    "--approval", "approval-payment-cli",
    "--actor", "human-admin",
    "--decision", "approved",
    "--note", "Payment approved for smoke test.",
    "--evidence", "evidence-payment-decision",
  ], env);

  const payment = run([
    "company", "finance", "payment", "record",
    "--definition", "page-operator-cli",
    "--id", "payment-operator-cli",
    "--commitment", "commitment-operator-cli",
    "--source-document", "document-ops-root",
    "--submitted-by", "agent-ops",
    "--accountable-owner", "agent-ops",
    "--amount", "3000",
    "--currency", "CNY",
    "--approval", "approval-payment-cli",
    "--evidence", "evidence-payment-receipt",
  ], env);
  check(payment.ok === true && payment.result?.record?.status === "prepared", "finance payment record writes evidence-backed prepared Payment");

  const milestoneClosed = run([
    "company", "work", "milestone", "close",
    "--authority", "human-admin",
    "--milestone", "milestone-operator-cli",
    "--work-item", "workitem-operator-cli",
  ], env);
  check(milestoneClosed.ok === true && milestoneClosed.result?.status === "achieved", "work milestone close marks milestone achieved");

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
