#!/usr/bin/env node

// RETIRED at contract layer 2026-08-05 (issue #323).
// Commitment/Payment code remains dormant; script preserved as
// historical evidence. Remove when code is fully decommissioned.

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "firm");
const token = "finance-cli-smoke-token";
const NOW = "2026-07-25T10:00:00+08:00";

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
    authority: { actor_type: "human", actor_id: "human-finance-owner" },
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
  execFileSync("cargo", ["build", "-q", "-p", "firm-cli"], { cwd: repoRoot, stdio: "inherit" });
  const root = await mkdtemp(join(tmpdir(), "company-os-finance-cli-smoke-"));
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
      id: "human-finance-owner",
      display_name: "Finance Owner",
      title: "Owner",
      status: "active",
      availability: "available",
      membership_refs: [],
      responsibility_summary: "Owns Finance CLI smoke.",
      permission_policy_refs: ["company_os.admin", "company.records.write", "company.approve", "finance.commitment.write", "finance.payment.write"],
      authority_policy_refs: ["company_os.admin", "page-finance-cli:commitment.append", "page-finance-cli:payment.append"],
      created_at: NOW,
      updated_at: NOW,
    },
  });
  await post(base, "/v1/company-os/actors", admin({
    actor_type: "agent",
    actor: {
      id: "agent-finance-governance",
      agent_member_ref: { kind: "agent_member", id: "agent-finance-governance" },
      status: "active",
      membership_refs: [],
      responsibility_summary: "Maintains money state.",
      permission_policy_refs: ["company.records.write", "finance.commitment.write", "finance.payment.write"],
      created_at: NOW,
      updated_at: NOW,
    },
  }));
  await post(base, "/v1/company-os/documents", admin({
    id: "document-finance-root",
    space_id: "company",
    parent_document_id: null,
    title: "Finance root",
    kind: "page",
    lifecycle_status: "active",
    block_ids: [],
    template_ref: null,
    permission_policy_refs: ["company.records.write"],
    reference_refs: [],
    created_by: { actor_type: "human", actor_id: "human-finance-owner" },
    updated_by: { actor_type: "human", actor_id: "human-finance-owner" },
    created_at: NOW,
    updated_at: NOW,
  }));

  server.kill();
  await new Promise((resolveWait) => server.once("exit", resolveWait));

  const module = run([
    "company", "docs", "module", "create",
    "--id", "module-finance-cli",
    "--root-document", "document-finance-root",
    "--name", "Finance CLI",
    "--purpose", "Acceptance module for Finance CLI.",
    "--record-type", "finance",
    "--default-view-id", "view-finance-cli",
    "--default-view-title", "Finance CLI fallback",
    "--authority", "human-finance-owner",
  ], env);
  check(module.ok === true && module.result?.module_id === "module-finance-cli", "module seed created through existing Docs CLI");

  const definition = run([
    "company", "docs", "page-definition", "create",
    "--id", "page-finance-cli",
    "--module", "module-finance-cli",
    "--fallback-view", "view-finance-cli",
    "--purpose", "Declare scoped Finance CLI commands.",
    "--package-id", "package-finance-cli",
    "--authority", "human-finance-owner",
    "--owner", "human-finance-owner",
    "--action", "approval.request",
    "--action", "approval.decide",
    "--action", "commitment.append",
    "--action", "payment.append",
  ], env);
  check(definition.ok === true && definition.result?.definition_id === "page-finance-cli", "page definition seed declares Finance actions");

  const proposed = run([
    "company", "finance", "propose-commitment",
    "--id", "commitment-cli-smoke",
    "--source-document", "document-finance-root",
    "--amount", "3000",
    "--currency", "CNY",
    "--submitted-by", "agent-finance-governance",
    "--accountable-owner", "human-finance-owner",
    "--authority", "human-finance-owner",
  ], env);
  check(proposed.ok === true && proposed.result?.record?.status === "proposed", "finance propose-commitment creates proposed Commitment through administrative import boundary");

  const approval = run([
    "company", "finance", "request-approval",
    "--definition", "page-finance-cli",
    "--id", "approval-finance-cli-smoke",
    "--commitment", "commitment-cli-smoke",
    "--requested-by", "agent-finance-governance",
    "--approver", "human-finance-owner",
    "--evidence", "evidence-fee-quote",
    "--action-summary", "Authorize commitment.append for Finance CLI smoke",
  ], env);
  check(approval.ok === true && approval.result?.record?.status === "requested", "finance request-approval creates requested Human approval");

  const pending = run([
    "company", "finance", "transition-commitment",
    "--definition", "page-finance-cli",
    "--commitment", "commitment-cli-smoke",
    "--status", "pending_approval",
    "--actor", "agent-finance-governance",
    "--approval", "approval-finance-cli-smoke",
    "--evidence", "evidence-fee-quote",
  ], env);
  check(pending.ok === true && pending.result?.record?.status === "pending_approval", "finance transition-commitment enters pending_approval with requested Human gate");

  const decided = run([
    "company", "finance", "decide-approval",
    "--definition", "page-finance-cli",
    "--approval", "approval-finance-cli-smoke",
    "--actor", "human-finance-owner",
    "--decision", "approved",
    "--note", "Approved for Finance CLI smoke.",
  ], env);
  check(decided.ok === true && decided.result?.record?.status === "approved", "finance decide-approval records Human approval decision");

  const approved = run([
    "company", "finance", "transition-commitment",
    "--definition", "page-finance-cli",
    "--commitment", "commitment-cli-smoke",
    "--status", "approved",
    "--actor", "agent-finance-governance",
    "--approval", "approval-finance-cli-smoke",
  ], env);
  check(approved.ok === true && approved.result?.record?.status === "approved", "finance transition-commitment moves pending_approval to approved");

  const paymentApproval = run([
    "company", "finance", "request-approval",
    "--definition", "page-finance-cli",
    "--id", "approval-payment-cli-smoke",
    "--commitment", "commitment-cli-smoke",
    "--policy-ref", "page-finance-cli:payment.append",
    "--requested-by", "agent-finance-governance",
    "--approver", "human-finance-owner",
    "--evidence", "evidence-payment-approval",
    "--action-summary", "Authorize payment.append for Finance CLI smoke",
  ], env);
  check(paymentApproval.ok === true && paymentApproval.result?.record?.status === "requested", "finance request-approval creates separate Payment approval");

  const paymentDecided = run([
    "company", "finance", "decide-approval",
    "--definition", "page-finance-cli",
    "--approval", "approval-payment-cli-smoke",
    "--actor", "human-finance-owner",
    "--decision", "approved",
    "--note", "Approved payment preparation for Finance CLI smoke.",
  ], env);
  check(paymentDecided.ok === true && paymentDecided.result?.record?.status === "approved", "finance decide-approval approves Payment action");

  const payment = run([
    "company", "finance", "record-payment",
    "--definition", "page-finance-cli",
    "--id", "payment-cli-smoke",
    "--commitment", "commitment-cli-smoke",
    "--actor", "agent-finance-governance",
    "--approval", "approval-payment-cli-smoke",
    "--evidence", "evidence-payment-prepared",
  ], env);
  check(payment.ok === true && payment.result?.record?.status === "prepared", "finance record-payment creates prepared Payment without implying settlement");

  const queried = run(["company", "finance", "query", "--commitment", "commitment-cli-smoke"], env);
  check(queried.ok === true && queried.result?.commitment?.status === "approved", "finance query returns native Commitment truth");

  const listed = run(["company", "finance", "list", "--commitment-status", "approved"], env);
  check(listed.ok === true && listed.result?.summary?.commitment_count === 1 && listed.result?.summary?.payment_count === 1, "finance list filters and keeps Payment separate from Commitment");
  check(JSON.stringify(listed.result?.boundaries).includes("approved_commitment_is_payment") && JSON.stringify(listed.result?.boundaries).includes("false"), "finance list declares Commitment is not Payment");

  await rm(root, { recursive: true, force: true });
  console.log(`\nCompany OS Finance CLI smoke: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
