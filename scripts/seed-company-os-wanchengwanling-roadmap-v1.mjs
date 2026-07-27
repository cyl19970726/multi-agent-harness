#!/usr/bin/env node

/**
 * Seed Wanchengwanling's Company OS completion roadmap.
 *
 * This adds the unfinished goals required to make the project operable through
 * Company OS: CLI/API, skills, custom pages, GitHub sync, SQL read/search,
 * real launch operating data, and replication templates.
 */

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const fourSystemSeed = join(repoRoot, "scripts", "seed-company-os-wanchengwanling-four-system-v1.mjs");
const token = "wanchengwanling-roadmap-v1-token";
const NOW = "2026-07-27T10:30:00+08:00";

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
    authority: actorRef("human", "human-wcw-owner"),
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

async function get(base, path) {
  const response = await fetch(`${base}${path}`, { headers: { accept: "application/json" } });
  const data = await response.json();
  if (!response.ok) throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  return data.result ?? data;
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

const workActions = [
  "document.append",
  "block.append",
  "typed_record.append",
  "view.append",
  "relation.append",
  "work_item.append",
  "assignment.append",
  "work_item.transition",
];

const additionalControlDefinitions = [
  ["page-wcw-project-home-work-control", "module-wcw-project-home", "view-wcw-project-home", "Project Home Work control"],
  ["page-wcw-launch-work-control", "module-wcw-launch-readiness", "view-wcw-launch-readiness", "Launch Readiness Work control"],
  ["page-wcw-business-model-work-control", "module-wcw-business-model", "view-wcw-business-model", "Business Model and Replication Work control"],
];

const operatingSurfaceMilestone = {
  id: "milestone-wcw-company-os-operating-surface",
  title: "Company OS operating surface completion",
  outcome: "Wanchengwanling can be operated from storage-backed CLI/skills, custom pages, GitHub source observations, standard views, and acceptance evidence.",
  status: "active",
  accountable_owner: actorRef("agent", "agent-wcw-lead"),
  source_document_ref: "document-wcw-project-home",
  business_module_ref: "module-wcw-project-home",
  target_at: "2026-08-15T18:00:00+08:00",
  acceptance_criteria: [
    "All governance modules have Agent-friendly CLI/skill paths or explicit planned gaps",
    "Core custom pages have expected images, Store-live actual screenshots, and expected-vs-actual comparisons",
    "GitHub dev source sync is repeatable and reviewable without overwriting commercial truth",
    "SQL read/search is introduced only as a rebuildable derived read layer",
    "Launch operators can inspect Docs, Work, Org, Finance, source snapshots, and evidence from the Store",
  ],
  work_item_refs: [],
  created_at: NOW,
  updated_at: NOW,
  achieved_at: null,
};

const operatingSurfaceWorkItems = [
  {
    id: "work-wcw-company-os-bootstrap-cli",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Ship Company OS business-project bootstrap CLI",
    objective: "Replace manual Wanchengwanling seed usage with a stable CLI/API path that registers a project, creates the DocumentSpace/module map, and reports exact Store evidence.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-development",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-company-os-skill-install-path",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Stabilize Company OS skill install and operator handoff",
    objective: "Ensure Docs, Work, Finance, Org, Module, Page, and Business Bootstrap skills install from repository commands and describe the Agent-operated path consistently.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-development",
    priority: "high",
    risk: "low",
  },
  {
    id: "work-wcw-source-sync-dev-branch-policy",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Make dev branch source sync branch-aware",
    objective: "Record both intended source branch and observed local checkout state, and surface mismatch as a review finding before claiming dev PRD truth.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-finance-cli-v1",
    definition: "page-wcw-procurement-finance-control",
    sourceDocument: "document-wcw-rewards-procurement-inventory",
    module: "module-wcw-rewards-procurement-inventory",
    title: "Implement Finance operator CLI v1",
    objective: "Add dedicated Finance CLI/API for budget, commitment, approval, invoice, payment, refund, and monetary evidence while preserving Commitment versus Payment separation.",
    workType: "finance",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-finance-governance",
    priority: "high",
    risk: "high",
  },
  {
    id: "work-wcw-org-cli-v1",
    definition: "page-wcw-project-home-work-control",
    sourceDocument: "document-wcw-project-home",
    module: "module-wcw-project-home",
    title: "Implement Organization operator CLI v1",
    objective: "Add dedicated Organization CLI/API for humans, Standing Agents, OrgUnits, memberships, permissions, skills, prompts, lifecycle proposal, and governance review.",
    workType: "governance",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-org-governance",
    priority: "high",
    risk: "high",
  },
  {
    id: "work-wcw-work-intake-from-docs",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement Docs-to-Work intake command",
    objective: "Let Docs Governance or Lead Agent create typed WorkItems from a Document/TypedRecord template with milestone, work type, owner, assignee, and source/result provenance.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-work-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-docs-custom-page-lifecycle-cli",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Complete custom page lifecycle CLI",
    objective: "Move code-declared custom pages from definition-only metadata to scaffold, verify, publish, Store-live capture, expected-vs-actual review, and fallback View acceptance.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-command-center",
    definition: "page-wcw-project-home-work-control",
    sourceDocument: "document-wcw-project-home",
    module: "module-wcw-project-home",
    title: "Implement Wanchengwanling Command Center custom page",
    objective: "Build a Store-live command center showing launch state, module health, work blockers, finance gates, source sync state, and next actions with fallback standard views.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-lead",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-work-board",
    definition: "page-wcw-launch-work-control",
    sourceDocument: "document-wcw-launch-readiness",
    module: "module-wcw-launch-readiness",
    title: "Implement Work Board custom page",
    objective: "Show all WorkItems by milestone, work type, business line, status, owner, assignee, approval/finance state, and evidence coverage.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-work-governance",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-launch-readiness",
    definition: "page-wcw-launch-work-control",
    sourceDocument: "document-wcw-launch-readiness",
    module: "module-wcw-launch-readiness",
    title: "Implement Launch Readiness custom page",
    objective: "Combine software, route, merchant, inventory, content, creator, finance, and approval readiness into one human-readable launch gate view.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-lead",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-merchant-console",
    definition: "page-wcw-merchant-work-control",
    sourceDocument: "document-wcw-merchant-network",
    module: "module-wcw-merchant-network",
    title: "Implement Merchant Network Console custom page",
    objective: "Show merchant capabilities, contact state, onboarding WorkItems, staff readiness, redemption responsibilities, and go-live evidence.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-merchant-ops",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-procurement-finance",
    definition: "page-wcw-procurement-finance-control",
    sourceDocument: "document-wcw-rewards-procurement-inventory",
    module: "module-wcw-rewards-procurement-inventory",
    title: "Implement Procurement and Finance Console custom page",
    objective: "Show reward SKUs, suppliers, quotes, commitments, approvals, payments, logistics, QC, inventory allocation, and evidence without inferring Payment.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-finance-governance",
    priority: "high",
    risk: "high",
  },
  {
    id: "work-wcw-custom-route-ar-console",
    definition: "page-wcw-route-work-control",
    sourceDocument: "document-wcw-route-ar-experience",
    module: "module-wcw-route-ar-experience",
    title: "Implement Route and AR Experience Console custom page",
    objective: "Show 12 configured spots, 8-checkin magnet threshold, 12-checkin lottery threshold, AR asset state, device signoff, blockers, and evidence.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-development",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-content-creator",
    definition: "page-wcw-content-work-control",
    sourceDocument: "document-wcw-content-growth",
    module: "module-wcw-content-growth",
    title: "Implement Content and Creator operating pages",
    objective: "Build content calendar and creator outreach views with briefs, publication state, deliverables, metrics, and finance links for paid/gifted collaborations.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-content-growth",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-ip-design-board",
    definition: "page-wcw-design-work-control",
    sourceDocument: "document-wcw-ip-product-design",
    module: "module-wcw-ip-product-design",
    title: "Implement IP and Product Design Asset Board custom page",
    objective: "Show IP character, bracelet, AR magnet, packaging, manufacturing specs, AR triggers, design approvals, and asset readiness.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-ip-design",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-custom-software-source-map",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement Software Source Map custom page",
    objective: "Show mapped repo paths, commits, source classes, headings, drift review state, DeliveryRefs, and linked WorkItems.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-github-source-webhook",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement GitHub source webhook path",
    objective: "Verify GitHub events, append SourceChangeEvent, run SourceSyncRun, and create Docs Governance review WorkItems for material PRD drift.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-github-delivery-refs",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement GitHub DeliveryRef linkage",
    objective: "Link issues, PRs, commits, CI, release tags, preview links, and device signoff artifacts to explicit WorkItems without letting GitHub own task truth.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-work-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-prd-drift-review-queue",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement PRD drift review queue",
    objective: "Surface unmapped, deleted, finance-impacting, privacy/security-impacting, or launch-impacting source changes as Docs Governance review WorkItems.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-sql-read-model-v1",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement SQL-derived read model v1",
    objective: "Build rebuildable SQL projections for Company OS Docs, Work, Org, Finance, relations, source snapshots, and page queries while JSONL ledgers remain canonical writes.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-development",
    priority: "medium",
    risk: "high",
  },
  {
    id: "work-wcw-global-search-v1",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Implement Company OS global search v1",
    objective: "Support search across Documents, Blocks, TypedRecords, WorkItems, actors, finance records, evidence refs, source snapshots, and relations from derived indexes.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-page-query-performance",
    definition: "page-wcw-software-work-control",
    sourceDocument: "document-wcw-software-product-sources",
    module: "module-wcw-software-product-sources",
    title: "Stabilize custom page query performance",
    objective: "Define scoped queries, loading/empty/error/permission states, and rebuildable projection contracts for custom pages before broad use.",
    workType: "development",
    assignee: "agent-wcw-development",
    owner: "agent-wcw-docs-governance",
    priority: "medium",
    risk: "medium",
  },
];

const launchDataWorkItems = [
  {
    id: "work-wcw-real-merchant-list",
    definition: "page-wcw-merchant-work-control",
    sourceDocument: "document-wcw-merchant-network",
    module: "module-wcw-merchant-network",
    title: "Populate real MVP merchant list and capability tags",
    objective: "Create merchant records for bracelet sellers, magnet redemption points, prize redemption partners, bracelet-benefit merchants, and purchased-supply merchants.",
    workType: "operations",
    assignee: "agent-wcw-merchant-ops",
    owner: "agent-wcw-merchant-ops",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-real-reward-quotes",
    definition: "page-wcw-procurement-finance-control",
    sourceDocument: "document-wcw-rewards-procurement-inventory",
    module: "module-wcw-rewards-procurement-inventory",
    title: "Collect real reward procurement quotes",
    objective: "Record quotes for AR magnets, Polaroid grand prizes, food coupons, bracelet manufacturing, and packaging before creating commitments.",
    workType: "procurement",
    assignee: "agent-wcw-procurement",
    owner: "agent-wcw-finance-governance",
    priority: "high",
    risk: "high",
  },
  {
    id: "work-wcw-real-inventory-logistics",
    definition: "page-wcw-procurement-finance-control",
    sourceDocument: "document-wcw-rewards-procurement-inventory",
    module: "module-wcw-rewards-procurement-inventory",
    title: "Create real inventory, logistics, receipt, and QC records",
    objective: "Track Pinduoduo magnet orders, supplier shipments, receipt evidence, quality checks, and shop allocation records.",
    workType: "procurement",
    assignee: "agent-wcw-procurement",
    owner: "agent-wcw-procurement",
    priority: "high",
    risk: "medium",
  },
  {
    id: "work-wcw-real-content-calendar",
    definition: "page-wcw-content-work-control",
    sourceDocument: "document-wcw-content-growth",
    module: "module-wcw-content-growth",
    title: "Create real MVP content calendar",
    objective: "Build the launch content calendar with briefs, assets, publish channels, expected metrics, and review cadence.",
    workType: "content",
    assignee: "agent-wcw-content-growth",
    owner: "agent-wcw-content-growth",
    priority: "medium",
    risk: "low",
  },
  {
    id: "work-wcw-real-creator-leads",
    definition: "page-wcw-creator-work-control",
    sourceDocument: "document-wcw-creator-outreach",
    module: "module-wcw-creator-outreach",
    title: "Create real creator outreach lead list",
    objective: "Record target bloggers/KOLs, contact scripts, terms, deliverables, evidence, finance implications, and response status.",
    workType: "content",
    assignee: "agent-wcw-creator-outreach",
    owner: "agent-wcw-creator-outreach",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-real-ip-asset-package",
    definition: "page-wcw-design-work-control",
    sourceDocument: "document-wcw-ip-product-design",
    module: "module-wcw-ip-product-design",
    title: "Package real IP, bracelet, magnet, and AR asset records",
    objective: "Create durable asset records for IP design, bracelet appearance, NFC spec, AR magnet design, packaging, AR triggers, source files, and review state.",
    workType: "design",
    assignee: "agent-wcw-ip-design",
    owner: "agent-wcw-ip-design",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-real-launch-runbook",
    definition: "page-wcw-launch-work-control",
    sourceDocument: "document-wcw-launch-readiness",
    module: "module-wcw-launch-readiness",
    title: "Write real launch runbook and go/no-go checklist",
    objective: "Create the launch runbook that combines software release, AR route, merchants, inventory, staff, content, creators, finance, and risk gates.",
    workType: "operations",
    assignee: "agent-wcw-lead",
    owner: "agent-wcw-lead",
    priority: "high",
    risk: "medium",
  },
];

const replicationWorkItems = [
  {
    id: "work-wcw-replication-site-template",
    definition: "page-wcw-business-model-work-control",
    sourceDocument: "document-wcw-business-model",
    module: "module-wcw-business-model",
    title: "Design next-site configuration template",
    objective: "Define the configurable model for a new city, scenic area, or commercial district: spot count, reward thresholds, merchants, assets, and launch gates.",
    workType: "governance",
    assignee: "agent-wcw-lead",
    owner: "agent-wcw-lead",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-replication-merchant-template",
    definition: "page-wcw-business-model-work-control",
    sourceDocument: "document-wcw-business-model",
    module: "module-wcw-business-model",
    title: "Design merchant network replication template",
    objective: "Create reusable merchant capability, onboarding, agreement, staff, redemption, and go-live templates for new locations.",
    workType: "governance",
    assignee: "agent-wcw-merchant-ops",
    owner: "agent-wcw-merchant-ops",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-replication-reward-template",
    definition: "page-wcw-business-model-work-control",
    sourceDocument: "document-wcw-business-model",
    module: "module-wcw-business-model",
    title: "Design reward and inventory replication template",
    objective: "Create reusable prize, procurement, supplier, logistics, QC, allocation, finance, and evidence templates for future locations.",
    workType: "governance",
    assignee: "agent-wcw-procurement",
    owner: "agent-wcw-finance-governance",
    priority: "medium",
    risk: "medium",
  },
  {
    id: "work-wcw-replication-launch-template",
    definition: "page-wcw-business-model-work-control",
    sourceDocument: "document-wcw-business-model",
    module: "module-wcw-business-model",
    title: "Design launch readiness replication template",
    objective: "Create a reusable milestone and WorkItem template set for a future city/scenic-area launch.",
    workType: "governance",
    assignee: "agent-wcw-lead",
    owner: "agent-wcw-work-governance",
    priority: "medium",
    risk: "medium",
  },
];

async function main() {
  execFileSync("cargo", ["build", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });

  const root = await mkdtemp(join(tmpdir(), "company-os-wcw-roadmap-v1-"));
  const explicitStoreRoot = argument("--store", "");
  const projectSelector = argument("--project", "");
  const useProject = !explicitStoreRoot && Boolean(projectSelector);
  const storeRoot = explicitStoreRoot || join(root, "store");
  const harnessArgs = (args) => useProject ? ["--project", projectSelector, ...args] : args;
  const env = {
    ...process.env,
    HARNESS_COMPANY_OS_TOKEN: token,
    ...(useProject ? {} : { HARNESS_ROOT: storeRoot }),
  };

  execFileSync("node", [
    fourSystemSeed,
    ...(useProject ? ["--project", projectSelector] : ["--store", storeRoot]),
  ], {
    cwd: repoRoot,
    env: { ...process.env },
    stdio: flag("--verbose") ? "inherit" : "ignore",
  });

  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(harness, harnessArgs(["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"]), {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  server.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  server.stderr.on("data", (chunk) => logs.push(chunk.toString()));

  const run = (args) => JSON.parse(execFileSync(harness, harnessArgs(args), {
    cwd: repoRoot,
    env,
    encoding: "utf8",
  }));

  try {
    await waitFor(`${base}/health`);
    let snapshot = await get(base, "/v1/company-os/snapshot");
    const existingDefinitionIds = new Set((snapshot.custom_page_definitions ?? []).map((entry) => entry.id));
    const controlDefinitionsCreated = [];
    for (const [id, moduleId, viewId, purpose] of additionalControlDefinitions) {
      if (existingDefinitionIds.has(id)) continue;
      run([
        "company", "docs", "page-definition", "create",
        "--id", id,
        "--module", moduleId,
        "--fallback-view", viewId,
        "--purpose", purpose,
        "--package-id", `package-${id}`,
        "--fixture-ref", `${id}:fixture`,
        "--visual-contract-ref", `docs/design/company-os/wanchengwanling/${id}`,
        "--authority", "human-wcw-owner",
        "--owner", "human-wcw-owner",
        "--component", "GovernedWorkControl",
        ...workActions.flatMap((name) => ["--action", name]),
      ]);
      controlDefinitionsCreated.push(id);
    }

    snapshot = await get(base, "/v1/company-os/snapshot");
    const existingMilestoneIds = new Set((snapshot.milestones ?? []).map((entry) => entry.id));
    if (!existingMilestoneIds.has(operatingSurfaceMilestone.id)) {
      await post(base, "/v1/company-os/milestones", admin(operatingSurfaceMilestone));
    }

    snapshot = await get(base, "/v1/company-os/snapshot");
    const existingWorkIds = new Set((snapshot.work_items ?? []).map((entry) => entry.id));
    const workItemsCreated = [];
    const allRoadmapItems = [
      ...operatingSurfaceWorkItems.map((item) => ({ ...item, milestone: operatingSurfaceMilestone.id })),
      ...launchDataWorkItems.map((item) => ({ ...item, milestone: "milestone-wcw-mvp-launch" })),
      ...replicationWorkItems.map((item) => ({ ...item, milestone: "milestone-wcw-first-site-replication-kit" })),
    ];

    for (const item of allRoadmapItems) {
      if (existingWorkIds.has(item.id)) continue;
      run([
        "company", "work", "create",
        "--definition", item.definition,
        "--id", item.id,
        "--source-document", item.sourceDocument,
        "--module", item.module,
        "--title", item.title,
        "--objective", item.objective,
        "--submitted-by", "agent-wcw-work-governance",
        "--submitted-by-kind", "agent",
        "--requested-by", "human-wcw-owner",
        "--requested-by-kind", "human",
        "--accountable-owner", item.owner,
        "--accountable-owner-kind", "agent",
        "--assignee", item.assignee,
        "--assignee-kind", "agent",
        "--work-type", item.workType,
        "--milestone", item.milestone,
        "--priority", item.priority,
        "--risk-level", item.risk,
      ]);
      workItemsCreated.push(item.id);
      existingWorkIds.add(item.id);
    }

    snapshot = await get(base, "/v1/company-os/snapshot");
    const workItems = snapshot.work_items ?? [];
    const milestoneRefs = new Map();
    for (const workItem of workItems) {
      if (!workItem.milestone_ref?.startsWith?.("milestone-wcw-")) continue;
      const refs = milestoneRefs.get(workItem.milestone_ref) ?? [];
      refs.push(workItem.id);
      milestoneRefs.set(workItem.milestone_ref, refs);
    }
    const milestonesById = new Map((snapshot.milestones ?? []).map((entry) => [entry.id, entry]));
    for (const [milestoneId, refs] of milestoneRefs) {
      const milestone = milestonesById.get(milestoneId);
      if (!milestone) continue;
      const currentRefs = JSON.stringify([...(milestone.work_item_refs ?? [])].sort());
      const nextRefs = JSON.stringify([...refs].sort());
      if (currentRefs === nextRefs) continue;
      await post(base, "/v1/company-os/milestones", admin({
        ...milestone,
        work_item_refs: refs,
        updated_at: NOW,
      }));
    }

    const finalSnapshot = await get(base, "/v1/company-os/snapshot");
    const wcwWorkItems = (finalSnapshot.work_items ?? []).filter((entry) => entry.id?.startsWith?.("work-wcw-"));
    const roadmapIds = new Set(allRoadmapItems.map((item) => item.id));
    const roadmapWorkItems = wcwWorkItems.filter((entry) => roadmapIds.has(entry.id));
    const byMilestone = {};
    const byWorkType = {};
    for (const item of wcwWorkItems) {
      byMilestone[item.milestone_ref ?? "none"] = (byMilestone[item.milestone_ref ?? "none"] ?? 0) + 1;
      byWorkType[item.work_type ?? "none"] = (byWorkType[item.work_type ?? "none"] ?? 0) + 1;
    }

    console.log(JSON.stringify({
      status: "passed",
      store_root: finalSnapshot.source?.store_root ?? storeRoot,
      project: useProject ? projectSelector : null,
      document_space: "wanchengwanling",
      control_definitions_created: controlDefinitionsCreated,
      work_items_created: workItemsCreated,
      roadmap_work_item_count: roadmapWorkItems.length,
      total_wanchengwanling_work_items: wcwWorkItems.length,
      by_milestone: byMilestone,
      by_work_type: byWorkType,
      key_milestone: operatingSurfaceMilestone.id,
      boundaries: {
        project_object: false,
        task_graph: false,
        goal_phase: false,
        custom_pages_are_storage_backed: true,
        cli_only: false,
        payment_inferred_from_commitment: false,
      },
    }, null, 2));
  } finally {
    server.kill();
    if (!flag("--keep-temp") && !explicitStoreRoot && !useProject) {
      await rm(root, { recursive: true, force: true });
    }
    if (logs.length && flag("--verbose-logs")) {
      console.error(logs.join(""));
    }
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
