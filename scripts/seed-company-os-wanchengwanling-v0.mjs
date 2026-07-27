#!/usr/bin/env node

/**
 * Seed the first Wanchengwanling external-product-source slice into Company OS.
 *
 * Default mode is isolated and deterministic: it creates a temporary Store plus
 * a fixture Git repo. Pass `--repo-path <local-wanchengwanling-worktree>` to sync
 * a real local dev worktree instead. The script writes only through Company OS
 * API/CLI surfaces and keeps GitHub/webhook as transport, not authority.
 */

import { execFileSync, spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const token = "wanchengwanling-source-seed-token";
const NOW = "2026-07-26T12:00:00+08:00";

function argument(name, fallback = "") {
  const index = process.argv.indexOf(name);
  return index === -1 ? fallback : process.argv[index + 1];
}

function flag(name) {
  return process.argv.includes(name);
}

function actorRef(actorType, actorId) {
  return { actor_type: actorType, actor_id: actorId };
}

function admin(record) {
  return {
    mode: "administrative",
    authority: actorRef("human", "human-wanchengwanling-owner"),
    record,
  };
}

function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => (error ? reject(error) : resolvePort(address.port)));
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
    headers: {
      "content-type": "application/json",
      "x-harness-company-os-token": token,
    },
    body: JSON.stringify(body),
  });
  const data = await response.json();
  if (!response.ok || data.ok === false) {
    throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  }
  return data.result ?? data;
}

async function get(base, path) {
  const response = await fetch(`${base}${path}`, { headers: { accept: "application/json" } });
  const data = await response.json();
  if (!response.ok) throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  return data.result ?? data;
}

async function makeFixtureRepo(root) {
  const fixtureRepo = join(root, "wanchengwanling-fixture-repo");
  await mkdir(join(fixtureRepo, "docs", "prd"), { recursive: true });
  await mkdir(join(fixtureRepo, "docs", "architecture"), { recursive: true });
  await writeFile(join(fixtureRepo, "docs", "prd", "README.md"), `# 业务模块 PRD 总览

## 产品总览

万城万灵是 AR 文旅项目：游客绑定手环、完成打卡、触发 AR、兑换奖品、参与商家和内容运营。

## 模块索引

- identity-bind
- route-checkin-passport
- redeem-staff
- shops
- lottery
`);
  await writeFile(join(fixtureRepo, "docs", "prd", "checkin-passport.md"), `# 路线打卡与 AR 护照

## AR check-in

游客在景点完成真实设备验收后的 AR 互动，结果回写产品打卡记录。
`);
  await writeFile(join(fixtureRepo, "docs", "architecture", "data-model.md"), `# Data Model

## Shops and rewards

Approved shops, rewards, prizes, magnets, stock allocations, and redemption records belong to the software product.
`);
  execFileSync("git", ["init"], { cwd: fixtureRepo, stdio: "ignore" });
  execFileSync("git", ["add", "."], { cwd: fixtureRepo, stdio: "ignore" });
  execFileSync("git", ["-c", "user.name=Company OS", "-c", "user.email=company-os@example.invalid", "commit", "-m", "seed wanchengwanling product docs"], { cwd: fixtureRepo, stdio: "ignore" });
  return fixtureRepo;
}

async function main() {
  execFileSync("cargo", ["build", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });

  const root = await mkdtemp(join(tmpdir(), "company-os-wanchengwanling-v0-"));
  const explicitStoreRoot = argument("--store", "");
  const projectSelector = argument("--project", "");
  const useProject = !explicitStoreRoot && Boolean(projectSelector);
  const storeRoot = explicitStoreRoot || join(root, "store");
  const harnessArgs = (args) => useProject ? ["--project", projectSelector, ...args] : args;
  const repoPath = argument("--repo-path", "") || await makeFixtureRepo(root);
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const env = {
    ...process.env,
    HARNESS_COMPANY_OS_TOKEN: token,
    ...(useProject ? {} : { HARNESS_ROOT: storeRoot }),
  };
  const server = spawn(harness, harnessArgs(["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"]), {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  server.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  server.stderr.on("data", (chunk) => logs.push(chunk.toString()));

  try {
    await waitFor(`${base}/health`);
    await post(base, "/v1/company-os/actors", {
      actor_type: "human",
      actor: {
        id: "human-wanchengwanling-owner",
        display_name: "Wanchengwanling Owner",
        title: "Project owner",
        status: "active",
        availability: "available",
        membership_refs: [],
        responsibility_summary: "Owns Wanchengwanling commercial and product operating decisions.",
        permission_policy_refs: ["company_os.admin", "company.records.write"],
        authority_policy_refs: ["company_os.admin"],
        created_at: NOW,
        updated_at: NOW,
      },
    });
    await post(base, "/v1/company-os/actors", admin({
      actor_type: "agent",
      actor: {
        id: "agent-wanchengwanling-docs-governance",
        display_name: "Wanchengwanling Docs Governance Agent",
        role: "Docs governance",
        status: "active",
        availability: "available",
        assignment_capacity: 4,
        exclusive_assignment_ref: null,
        home_org_unit_ref: null,
        membership_refs: [],
        responsibility_summary: "Maps Wanchengwanling software product sources into Company OS Docs and routes drift for review.",
        capability_refs: ["company.records.write"],
        permission_policy_refs: ["company.records.write"],
        system_prompt_ref: "document-prompt-wanchengwanling-docs-governance",
        tool_refs: ["tool-company-records"],
        skill_refs: ["company-docs-operator"],
        accepted_work_types: ["docs_governance"],
        escalation_policy_ref: "policy-wanchengwanling-owner-escalation",
        runtime_refs: [],
        native_session_refs: [],
        created_at: NOW,
        updated_at: NOW,
      },
    }));
    await post(base, "/v1/company-os/documents", admin({
      id: "document-wanchengwanling-root",
      space_id: "wanchengwanling",
      parent_document_id: null,
      title: "Wanchengwanling Company OS",
      kind: "page",
      lifecycle_status: "active",
      block_ids: [],
      template_ref: null,
      permission_policy_refs: ["company.records.write"],
      reference_refs: [],
      created_by: actorRef("human", "human-wanchengwanling-owner"),
      updated_by: actorRef("human", "human-wanchengwanling-owner"),
      created_at: NOW,
      updated_at: NOW,
    }));

    const cliEnv = { ...env, HARNESS_COMPANY_OS_TOKEN: token };
    if (!useProject) cliEnv.HARNESS_ROOT = storeRoot;
    const run = (args) => JSON.parse(execFileSync(harness, harnessArgs(args), { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));

    const module = run([
      "company", "docs", "module", "create",
      "--id", "module-wanchengwanling-product-source",
      "--root-document", "document-wanchengwanling-root",
      "--name", "Wanchengwanling Product & Software Delivery",
      "--purpose", "Map GitHub-hosted software PRDs, architecture, ADRs, and delivery evidence into Company OS Docs without moving commercial operations into GitHub.",
      "--record-type", "external_project",
      "--record-type", "product_doc_source",
      "--record-type", "product_doc_snapshot",
      "--record-type", "source_sync_run",
      "--default-view-id", "view-wanchengwanling-product-sources",
      "--default-view-title", "Wanchengwanling product sources",
      "--authority", "human-wanchengwanling-owner",
    ]);
    if (module.ok !== true) throw new Error(`module create failed: ${JSON.stringify(module)}`);

    const definition = run([
      "company", "docs", "page-definition", "create",
      "--id", "page-wanchengwanling-product-source",
      "--module", "module-wanchengwanling-product-source",
      "--fallback-view", "view-wanchengwanling-product-sources",
      "--purpose", "Governed Wanchengwanling product-source mapping page.",
      "--package-id", "package-wanchengwanling-product-source",
      "--fixture-ref", "wanchengwanling-product-source-v0",
      "--visual-contract-ref", "docs/design/company-os/wanchengwanling-product-source-v0",
      "--authority", "human-wanchengwanling-owner",
      "--owner", "human-wanchengwanling-owner",
      "--component", "ProductSourceMapping",
    ]);
    if (definition.ok !== true) throw new Error(`page-definition create failed: ${JSON.stringify(definition)}`);

    const sourceSync = run([
      "company", "docs", "source", "sync",
      "--definition", "page-wanchengwanling-product-source",
      "--module", "module-wanchengwanling-product-source",
      "--source-document", "document-wanchengwanling-root",
      "--actor", "agent-wanchengwanling-docs-governance",
      "--repo-path", repoPath,
      "--repo", "cyl19970726/wanchengwanling",
      "--branch", argument("--branch", "dev"),
      "--project-id", "wanchengwanling",
      "--path", "docs/prd",
      "--path", "docs/architecture",
    ]);
    if (sourceSync.ok !== true) throw new Error(`source sync failed: ${JSON.stringify(sourceSync)}`);

    const snapshot = await get(base, "/v1/company-os/snapshot");
    const records = snapshot.typed_records ?? [];
    const productDocSnapshots = records.filter((record) => record.record_type === "product_doc_snapshot" && record.fields?.project_id === "wanchengwanling");
    const sourceSyncRuns = records.filter((record) => record.record_type === "source_sync_run" && record.fields?.project_id === "wanchengwanling");
    if (!productDocSnapshots.length || !sourceSyncRuns.length) {
      throw new Error(`seed did not create product source records: ${JSON.stringify({ productDocSnapshots, sourceSyncRuns })}`);
    }

    console.log(JSON.stringify({
      status: "passed",
      store_root: snapshot.source?.store_root ?? storeRoot,
      project: useProject ? projectSelector : null,
      repo_path: repoPath,
      module_id: "module-wanchengwanling-product-source",
      definition_id: "page-wanchengwanling-product-source",
      records_written: sourceSync.records_written,
      product_doc_snapshot_count: productDocSnapshots.length,
      synced_paths: productDocSnapshots.map((record) => record.fields.path).sort(),
      side_effects: {
        work_items: (snapshot.work_items ?? []).length,
        approvals: (snapshot.approvals ?? []).length,
        financial_records: (snapshot.financial_records ?? []).length,
      },
      boundaries: sourceSync.boundaries,
    }, null, 2));
  } finally {
    server.kill("SIGTERM");
    await new Promise((resolveStop) => server.once("exit", resolveStop));
    if (!flag("--keep")) {
      await rm(root, { recursive: true, force: true });
    }
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
