#!/usr/bin/env node

/**
 * Seed Wanchengwanling's first four-system Company OS bootstrap.
 *
 * This is an acceptance/fixture script, not the final product entrypoint. It
 * composes the existing Docs substrate seed with native Organization, Work,
 * Assignment, Approval, and Finance records so the project can be inspected as
 * a real Company OS commercial workspace.
 */

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const docsSeed = join(repoRoot, "scripts", "seed-company-os-wanchengwanling-docs-v0.mjs");
const token = "wanchengwanling-four-system-v1-token";
const NOW = "2026-07-26T15:00:00+08:00";
const FUTURE = "2026-12-31T23:59:59+08:00";

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

function entityRef(kind, id) {
  return { kind, id };
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

function action(id, commandName, definitionId, subjectRef, requestedBy, record, options = {}) {
  const policyRef = options.policyRef ?? `${definitionId}:${commandName}`;
  return {
    id,
    command_name: commandName,
    subject_ref: subjectRef,
    requested_by: requestedBy,
    payload: {
      definition_id: definitionId,
      record,
    },
    required_permission: options.requiredPermission ?? "company.records.write",
    policy_ref: policyRef,
    risk_tier: options.riskTier ?? "r1",
    requires_human_approval: options.requiresHumanApproval ?? false,
    approval_refs: options.approvalRefs ?? [],
    status: "requested",
    audit_event_refs: [options.auditEvent ?? `${id}:policy-authorized`],
    requested_at: options.requestedAt ?? NOW,
    completed_at: null,
  };
}

function standingAgent(id, displayName, role, {
  responsibility,
  skills = [],
  capabilities = ["company.records.write", "company.work.execute"],
  permissions = ["company.records.write", "company.work.execute"],
  maintainedDocuments = [],
  acceptedWorkTypes = ["operations", "general"],
  capacity = 4,
} = {}) {
  return {
    actor_type: "agent",
    actor: {
      id,
      display_name: displayName,
      role,
      status: "active",
      availability: "available",
      assignment_capacity: capacity,
      exclusive_assignment_ref: null,
      membership_refs: [],
      responsibility_summary: responsibility ?? role,
      capability_refs: capabilities,
      system_prompt_ref: `document-prompt-${id}`,
      tool_refs: ["tool-company-os-cli", "tool-company-os-api"],
      skill_refs: skills,
      maintained_document_refs: maintainedDocuments,
      accepted_work_type_refs: acceptedWorkTypes,
      escalation_policy_ref: "policy-wcw-owner-escalation",
      permission_policy_refs: permissions,
      runtime_refs: [],
      native_session_refs: [],
      created_at: NOW,
      updated_at: NOW,
    },
  };
}

function membership(id, orgUnitId, actor, role, title) {
  return {
    id,
    organization_id: "org-wanchengwanling",
    org_unit_id: orgUnitId,
    actor_ref: actor,
    membership_role: role,
    title_or_function: title,
    status: "active",
    starts_at: NOW,
    ends_at: null,
    authority_policy_refs: role === "lead" ? ["company.lead"] : [],
    created_by_actor_ref: actorRef("human", "human-wcw-owner"),
    created_at: NOW,
  };
}

async function main() {
  execFileSync("cargo", ["build", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });

  const root = await mkdtemp(join(tmpdir(), "company-os-wcw-four-system-v1-"));
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

  const preflightOutput = execFileSync(harness, harnessArgs(["dashboard", "snapshot"]), {
    cwd: repoRoot,
    env,
    encoding: "utf8",
  });
  const preflight = JSON.parse(preflightOutput);
  const preflightDocuments = preflight.company_os?.documents ?? preflight.documents ?? [];
  const preflightWorkItems = preflight.company_os?.work_items ?? preflight.work_items ?? [];
  const existingWcwDocuments = preflightDocuments.filter((entry) => entry.space_id === "wanchengwanling");
  const existingWcwWorkItems = preflightWorkItems.filter((entry) => entry.id?.startsWith?.("work-wcw-"));
  if (existingWcwDocuments.length >= 12 && existingWcwWorkItems.length >= 8) {
    console.log(JSON.stringify({
      status: "already_exists",
      store_root: preflight.company_os?.source?.store_root ?? preflight.source?.store_root ?? storeRoot,
      project: useProject ? projectSelector : null,
      document_space: "wanchengwanling",
      counts: {
        documents: existingWcwDocuments.length,
        work_items: existingWcwWorkItems.length,
      },
      note: "Wanchengwanling four-system bootstrap already exists; no append was attempted.",
    }, null, 2));
    return;
  }
  if (existingWcwDocuments.length || existingWcwWorkItems.length) {
    throw new Error(`partial Wanchengwanling Company OS rows already exist; refusing to append over partial state: ${JSON.stringify({
      documents: existingWcwDocuments.length,
      work_items: existingWcwWorkItems.length,
    })}`);
  }

  execFileSync("node", [
    docsSeed,
    ...(useProject ? ["--project", projectSelector] : ["--store", storeRoot]),
  ], {
    cwd: repoRoot,
    env: { ...process.env },
    stdio: flag("--verbose") ? "inherit" : "ignore",
  });

  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const cliEnv = { ...env };
  const server = spawn(harness, harnessArgs(["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"]), {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  server.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  server.stderr.on("data", (chunk) => logs.push(chunk.toString()));

  const run = (args) => {
    const output = execFileSync(harness, harnessArgs(args), {
      cwd: repoRoot,
      env: cliEnv,
      encoding: "utf8",
    });
    const parsed = JSON.parse(output);
    if (parsed.ok !== true) {
      throw new Error(`${args.join(" ")} failed: ${output}`);
    }
    return parsed;
  };

  try {
    await waitFor(`${base}/health`);

    await post(base, "/v1/company-os/actors", admin({
      actor_type: "human",
      actor: {
        id: "human-wcw-owner",
        display_name: "Wanchengwanling Owner",
        title: "Project owner",
        status: "active",
        availability: "available",
        membership_refs: [],
        responsibility_summary: "Owns Wanchengwanling business, product, launch, organization, approval, and finance decisions.",
        permission_policy_refs: [
          "company_os.admin",
          "company.records.write",
          "company.work.execute",
          "company.approve",
          "finance.commitment.write",
          "finance.payment.write",
        ],
        authority_policy_refs: [
          "company_os.admin",
          "company.approve",
          "page-wcw-bracelet-finance-control:commitment.append",
          "page-wcw-bracelet-finance-control:approval.decide",
        ],
        created_at: NOW,
        updated_at: NOW,
      },
    }));

    const actors = [
      standingAgent("agent-wcw-lead", "Wanchengwanling Lead Agent", "Lead Agent", {
        responsibility: "Coordinates Wanchengwanling governance Agents, business Agents, WorkItems, and escalation to the Human Owner.",
        skills: ["company-business-project-bootstrap", "company-work-operator"],
        capabilities: ["company.records.write", "company.work.execute"],
        permissions: ["company.records.write", "company.work.execute"],
        acceptedWorkTypes: ["governance", "operations", "general"],
        maintainedDocuments: ["document-wcw-project-home"],
      }),
      standingAgent("agent-wcw-work-governance", "Wanchengwanling Work Governance Agent", "Work Governance", {
        responsibility: "Creates, classifies, routes, and monitors Wanchengwanling WorkItems and Milestones.",
        skills: ["company-work-operator"],
        acceptedWorkTypes: ["governance", "operations", "procurement", "content", "development", "design"],
        maintainedDocuments: ["document-wcw-launch-readiness"],
      }),
      standingAgent("agent-wcw-finance-governance", "Wanchengwanling Finance Governance Agent", "Finance Governance", {
        responsibility: "Prepares budget, commitment, and payment requests while preserving the no-payment-without-evidence boundary.",
        skills: ["company-finance-operator"],
        capabilities: ["company.records.write", "company.work.execute", "finance.commitment.write", "finance.payment.write"],
        permissions: ["company.records.write", "company.work.execute", "finance.commitment.write", "finance.payment.write"],
        acceptedWorkTypes: ["finance", "procurement"],
        maintainedDocuments: ["document-wcw-bracelet-product", "document-wcw-rewards-procurement-inventory"],
      }),
      standingAgent("agent-wcw-org-governance", "Wanchengwanling Org / HR Governance Agent", "Org / HR Governance", {
        responsibility: "Maintains the Agent organization shape and proposes business Agents when recurring capability gaps appear.",
        skills: ["company-org-operator"],
        acceptedWorkTypes: ["governance"],
        maintainedDocuments: ["document-wcw-project-home"],
      }),
      standingAgent("agent-wcw-merchant-ops", "Wanchengwanling Merchant Ops Agent", "Merchant Operations", {
        responsibility: "Onboards bracelet sellers, prize redemption points, and merchant-benefit partners.",
        skills: ["company-work-operator"],
        acceptedWorkTypes: ["operations", "human_action"],
        maintainedDocuments: ["document-wcw-merchant-network"],
      }),
      standingAgent("agent-wcw-procurement", "Wanchengwanling Procurement Agent", "Rewards and Inventory Procurement", {
        responsibility: "Sources bracelets, magnets, prizes, food coupons, logistics, and inventory evidence.",
        skills: ["company-work-operator", "company-finance-operator"],
        acceptedWorkTypes: ["procurement", "finance"],
        maintainedDocuments: ["document-wcw-rewards-procurement-inventory"],
      }),
      standingAgent("agent-wcw-content-growth", "Wanchengwanling Content Growth Agent", "Content Growth", {
        responsibility: "Plans self-media publishing and collects post-level growth metrics.",
        skills: ["company-work-operator"],
        acceptedWorkTypes: ["content"],
        maintainedDocuments: ["document-wcw-content-growth"],
      }),
      standingAgent("agent-wcw-creator-outreach", "Wanchengwanling Creator Outreach Agent", "Creator Outreach", {
        responsibility: "Builds creator leads, outreach records, collaboration proposals, and deliverable evidence.",
        skills: ["company-work-operator"],
        acceptedWorkTypes: ["content", "operations"],
        maintainedDocuments: ["document-wcw-creator-outreach"],
      }),
      standingAgent("agent-wcw-development", "Wanchengwanling Development Agent", "Software Product Development", {
        responsibility: "Tracks the software PRD source mapping and product delivery WorkItems.",
        skills: ["company-work-operator", "company-docs-operator"],
        acceptedWorkTypes: ["development"],
        maintainedDocuments: ["document-wcw-software-product-sources"],
      }),
      standingAgent("agent-wcw-ip-design", "Wanchengwanling IP / Product Design Agent", "IP and Product Design", {
        responsibility: "Maintains bracelet, magnet, IP character, AR asset, SKU, and manufacturing design records.",
        skills: ["company-work-operator", "company-docs-operator"],
        acceptedWorkTypes: ["design"],
        maintainedDocuments: ["document-wcw-ip-product-design"],
      }),
    ];

    for (const actor of actors) {
      await post(base, "/v1/company-os/actors", admin(actor));
    }

    await post(base, "/v1/company-os/actors", admin({
      actor_type: "external",
      actor: {
        id: "external-wcw-merchant-sample",
        display_name_or_organization: "Sample Ancient City Merchant",
        engagement_scope: "Potential bracelet seller, prize redemption point, and bracelet-benefit partner.",
        sponsor_actor_ref: actorRef("agent", "agent-wcw-merchant-ops"),
        access_expires_at: FUTURE,
        confidentiality_or_contract_refs: ["contract-template-wcw-merchant-cooperation"],
        membership_refs: [],
        restricted_permission_refs: ["merchant.profile.read"],
        status: "active",
        created_at: NOW,
        updated_at: NOW,
      },
    }));

    await post(base, "/v1/company-os/org-units", admin({
      id: "orgunit-wcw-root",
      organization_id: "org-wanchengwanling",
      name: "Wanchengwanling Company OS",
      purpose: "Root operating organization for the Wanchengwanling commercial MVP.",
      parent_unit_id: null,
      status: "active",
      human_lead_actor_ref: actorRef("human", "human-wcw-owner"),
      agent_lead_actor_ref: actorRef("agent", "agent-wcw-lead"),
      policy_refs: ["policy-wcw-owner-escalation", "policy-wcw-governance-led"],
      document_space_ref: "wanchengwanling",
      created_at: NOW,
      updated_at: NOW,
    }));
    await post(base, "/v1/company-os/org-units", admin({
      id: "orgunit-wcw-governance",
      organization_id: "org-wanchengwanling",
      name: "Governance Layer",
      purpose: "Docs, Work, Finance, and Org/HR governance Agents reporting to Lead.",
      parent_unit_id: "orgunit-wcw-root",
      status: "active",
      human_lead_actor_ref: actorRef("human", "human-wcw-owner"),
      agent_lead_actor_ref: actorRef("agent", "agent-wcw-lead"),
      policy_refs: ["policy-wcw-governance-led"],
      document_space_ref: "wanchengwanling",
      created_at: NOW,
      updated_at: NOW,
    }));
    await post(base, "/v1/company-os/org-units", admin({
      id: "orgunit-wcw-business-agents",
      organization_id: "org-wanchengwanling",
      name: "Business Agents",
      purpose: "Business-capability Agents managed by Org/HR and coordinated through Docs, Work, and Finance records.",
      parent_unit_id: "orgunit-wcw-root",
      status: "active",
      human_lead_actor_ref: null,
      agent_lead_actor_ref: actorRef("agent", "agent-wcw-org-governance"),
      policy_refs: ["policy-wcw-business-agent-lifecycle"],
      document_space_ref: "wanchengwanling",
      created_at: NOW,
      updated_at: NOW,
    }));

    const memberships = [
      membership("membership-wcw-owner-root", "orgunit-wcw-root", actorRef("human", "human-wcw-owner"), "lead", "Human Owner"),
      membership("membership-wcw-lead-root", "orgunit-wcw-root", actorRef("agent", "agent-wcw-lead"), "lead", "Lead Agent"),
      membership("membership-wcw-docs-gov", "orgunit-wcw-governance", actorRef("agent", "agent-wcw-docs-governance"), "member", "Docs Governance"),
      membership("membership-wcw-work-gov", "orgunit-wcw-governance", actorRef("agent", "agent-wcw-work-governance"), "member", "Work Governance"),
      membership("membership-wcw-finance-gov", "orgunit-wcw-governance", actorRef("agent", "agent-wcw-finance-governance"), "member", "Finance Governance"),
      membership("membership-wcw-org-gov", "orgunit-wcw-governance", actorRef("agent", "agent-wcw-org-governance"), "member", "Org / HR Governance"),
      membership("membership-wcw-merchant-ops", "orgunit-wcw-business-agents", actorRef("agent", "agent-wcw-merchant-ops"), "member", "Merchant Operations"),
      membership("membership-wcw-procurement", "orgunit-wcw-business-agents", actorRef("agent", "agent-wcw-procurement"), "member", "Procurement"),
      membership("membership-wcw-content-growth", "orgunit-wcw-business-agents", actorRef("agent", "agent-wcw-content-growth"), "member", "Content Growth"),
      membership("membership-wcw-creator-outreach", "orgunit-wcw-business-agents", actorRef("agent", "agent-wcw-creator-outreach"), "member", "Creator Outreach"),
      membership("membership-wcw-development", "orgunit-wcw-business-agents", actorRef("agent", "agent-wcw-development"), "member", "Development"),
      membership("membership-wcw-ip-design", "orgunit-wcw-business-agents", actorRef("agent", "agent-wcw-ip-design"), "member", "IP / Product Design"),
      membership("membership-wcw-sample-merchant", "orgunit-wcw-business-agents", actorRef("external", "external-wcw-merchant-sample"), "external_partner", "Merchant sample"),
    ];
    for (const item of memberships) {
      await post(base, "/v1/company-os/memberships", admin(item));
    }

    const workActions = [
      "typed_record.append",
      "view.append",
      "relation.append",
      "work_item.append",
      "assignment.append",
      "work_item.transition",
    ];
    const financeActions = [
      ...workActions,
      "approval.request",
      "approval.decide",
      "commitment.propose",
      "commitment.append",
      "payment.append",
    ];
    const controlDefinitions = [
      ["page-wcw-bracelet-finance-control", "module-wcw-bracelet-product", "view-wcw-bracelet-product", "Bracelet finance and settlement control", financeActions],
      ["page-wcw-route-work-control", "module-wcw-route-ar-experience", "view-wcw-route-ar-experience", "Route and AR Work control", workActions],
      ["page-wcw-merchant-work-control", "module-wcw-merchant-network", "view-wcw-merchant-network", "Merchant Work control", workActions],
      ["page-wcw-procurement-finance-control", "module-wcw-rewards-procurement-inventory", "view-wcw-rewards-procurement-inventory", "Rewards procurement and Finance control", financeActions],
      ["page-wcw-content-work-control", "module-wcw-content-growth", "view-wcw-content-growth", "Content Growth Work control", workActions],
      ["page-wcw-creator-work-control", "module-wcw-creator-outreach", "view-wcw-creator-outreach", "Creator Outreach Work control", workActions],
      ["page-wcw-design-work-control", "module-wcw-ip-product-design", "view-wcw-ip-product-design", "IP and Product Design Work control", workActions],
      ["page-wcw-software-work-control", "module-wcw-software-product-sources", "view-wcw-software-product-sources", "Software PRD source Work control", workActions],
    ];
    for (const [id, moduleId, viewId, purpose, actions] of controlDefinitions) {
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
        ...actions.flatMap((name) => ["--action", name]),
      ]);
    }

    const milestones = [
      {
        id: "milestone-wcw-mvp-launch",
        title: "MVP launch readiness",
        outcome: "The MVP can launch with configured bracelet sales, AR route, merchants, rewards, content, and software readiness evidence.",
        status: "active",
        accountable_owner: actorRef("agent", "agent-wcw-lead"),
        source_document_ref: "document-wcw-launch-readiness",
        business_module_ref: "module-wcw-launch-readiness",
        target_at: "2026-08-31T18:00:00+08:00",
        acceptance_criteria: [
          "Bracelet offer and settlement rules are documented",
          "8-checkin magnet and 12-checkin lottery rules are configured",
          "Merchant list and redemption responsibilities are clear",
          "Reward procurement has WorkItems and Finance gates",
          "Software PRD source mapping is current",
        ],
        work_item_refs: [],
        created_at: NOW,
        updated_at: NOW,
        achieved_at: null,
      },
      {
        id: "milestone-wcw-first-site-replication-kit",
        title: "First replication kit",
        outcome: "The operating model can be copied to the next city, scenic area, or commercial district.",
        status: "planned",
        accountable_owner: actorRef("agent", "agent-wcw-lead"),
        source_document_ref: "document-wcw-business-model",
        business_module_ref: "module-wcw-business-model",
        target_at: null,
        acceptance_criteria: [
          "Configurable spot/merchant/reward template exists",
          "Launch WorkItem template set exists",
          "Finance and Org assumptions are explicit",
        ],
        work_item_refs: [],
        created_at: NOW,
        updated_at: NOW,
        achieved_at: null,
      },
    ];
    for (const milestone of milestones) {
      await post(base, "/v1/company-os/milestones", admin(milestone));
    }

    const workItems = [
      {
        definition: "page-wcw-bracelet-finance-control",
        id: "work-wcw-configure-bracelet-settlement",
        sourceDocument: "document-wcw-bracelet-product",
        module: "module-wcw-bracelet-product",
        title: "Configure physical bracelet consignment settlement",
        objective: "Turn the ¥30 physical bracelet sale price and ¥10/¥20 split into governed Docs, Work, and Finance records without implying payment.",
        workType: "finance",
        assignee: "agent-wcw-finance-governance",
        accountableOwner: "agent-wcw-finance-governance",
        sourceRecord: "record-wcw-consignment-physical-bracelet-30-10-20",
        priority: "high",
      },
      {
        definition: "page-wcw-merchant-work-control",
        id: "work-wcw-onboard-mvp-merchants",
        sourceDocument: "document-wcw-merchant-network",
        module: "module-wcw-merchant-network",
        title: "Onboard MVP merchant network",
        objective: "Confirm merchants for bracelet sales, magnet/prize redemption, and bracelet benefits with capability tags and contact evidence.",
        workType: "operations",
        assignee: "agent-wcw-merchant-ops",
        accountableOwner: "agent-wcw-merchant-ops",
        sourceRecord: "record-wcw-merchant-capabilities-mvp",
        priority: "high",
      },
      {
        definition: "page-wcw-procurement-finance-control",
        id: "work-wcw-procure-mvp-rewards",
        sourceDocument: "document-wcw-rewards-procurement-inventory",
        module: "module-wcw-rewards-procurement-inventory",
        title: "Source MVP rewards and inventory",
        objective: "Source AR magnets, two Polaroid prizes, local food coupons, logistics, and inventory evidence before launch.",
        workType: "procurement",
        assignee: "agent-wcw-procurement",
        accountableOwner: "agent-wcw-procurement",
        sourceRecord: "record-wcw-prize-pool-mvp-lottery",
        priority: "high",
      },
      {
        definition: "page-wcw-route-work-control",
        id: "work-wcw-validate-12-spot-route",
        sourceDocument: "document-wcw-route-ar-experience",
        module: "module-wcw-route-ar-experience",
        title: "Validate 12-spot AR route and reward thresholds",
        objective: "Verify the configured 12 scenic spots, the 8-checkin magnet rule, and the 12-checkin lottery rule against MVP route requirements.",
        workType: "development",
        assignee: "agent-wcw-development",
        accountableOwner: "agent-wcw-development",
        sourceRecord: "record-wcw-site-jieyang-ancient-city",
        priority: "high",
      },
      {
        definition: "page-wcw-design-work-control",
        id: "work-wcw-finalize-ip-product-assets",
        sourceDocument: "document-wcw-ip-product-design",
        module: "module-wcw-ip-product-design",
        title: "Finalize IP, bracelet, and AR magnet design assets",
        objective: "Prepare first-pass IP character, bracelet, magnet, AR trigger, packaging, and manufacturing-spec records for review.",
        workType: "design",
        assignee: "agent-wcw-ip-design",
        accountableOwner: "agent-wcw-ip-design",
        sourceRecord: "record-wcw-asset-ar-magnet-design",
        priority: "medium",
      },
      {
        definition: "page-wcw-content-work-control",
        id: "work-wcw-content-launch-plan",
        sourceDocument: "document-wcw-content-growth",
        module: "module-wcw-content-growth",
        title: "Create MVP self-media launch content plan",
        objective: "Define first content campaign, publish cadence, post templates, and metrics needed to generate AR route sharing.",
        workType: "content",
        assignee: "agent-wcw-content-growth",
        accountableOwner: "agent-wcw-content-growth",
        sourceRecord: null,
        priority: "medium",
      },
      {
        definition: "page-wcw-creator-work-control",
        id: "work-wcw-creator-outreach-pilot",
        sourceDocument: "document-wcw-creator-outreach",
        module: "module-wcw-creator-outreach",
        title: "Create first creator outreach pilot list",
        objective: "Build the first blogger/KOL lead list and outreach script for MVP launch amplification.",
        workType: "content",
        assignee: "agent-wcw-creator-outreach",
        accountableOwner: "agent-wcw-creator-outreach",
        sourceRecord: null,
        priority: "medium",
      },
      {
        definition: "page-wcw-software-work-control",
        id: "work-wcw-sync-dev-prd-source",
        sourceDocument: "document-wcw-software-product-sources",
        module: "module-wcw-software-product-sources",
        title: "Keep wanchengwanling dev PRD mapped into Company OS",
        objective: "Sync GitHub software PRD/source documents as observed software product truth without overwriting commercial truth.",
        workType: "development",
        assignee: "agent-wcw-development",
        accountableOwner: "agent-wcw-development",
        sourceRecord: "record-wcw-external-project-wanchengwanling",
        priority: "medium",
      },
    ];

    for (const item of workItems) {
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
        "--accountable-owner", item.accountableOwner,
        "--accountable-owner-kind", "agent",
        "--assignee", item.assignee,
        "--assignee-kind", "agent",
        "--work-type", item.workType,
        "--milestone", "milestone-wcw-mvp-launch",
        "--priority", item.priority,
        "--risk-level", item.workType === "finance" || item.workType === "procurement" ? "medium" : "low",
        ...(item.sourceRecord ? ["--source-record", item.sourceRecord] : []),
      ]);
      run([
        "company", "work", "assign",
        "--definition", item.definition,
        "--id", `assignment-${item.id}`,
        "--work-item", item.id,
        "--assignee", item.assignee,
        "--assignee-kind", "agent",
        "--assigned-by", "agent-wcw-work-governance",
        "--assigned-by-kind", "agent",
        "--delivery-state", "delivered",
        "--delivery-evidence", `evidence-${item.id}-assignment-delivered`,
        "--scope", item.objective,
        "--correlation-id", `corr-${item.id}`,
      ]);
    }

    const launchMilestone = { ...milestones[0], work_item_refs: workItems.map((item) => item.id), updated_at: "2026-07-26T15:10:00+08:00" };
    await post(base, "/v1/company-os/milestones", admin(launchMilestone));

    const relationRecord = {
      id: "relation-wcw-consignment-settlement-work",
      from_ref: entityRef("typed_record", "record-wcw-consignment-physical-bracelet-30-10-20"),
      relation_type: "implemented_by",
      to_ref: entityRef("work_item", "work-wcw-configure-bracelet-settlement"),
      provenance_ref: entityRef("document", "document-wcw-bracelet-product"),
      lifecycle_status: "active",
      created_by: actorRef("agent", "agent-wcw-work-governance"),
      created_at: "2026-07-26T15:12:00+08:00",
    };
    await post(base, "/v1/company-os/actions/dispatch", action(
      "action-wcw-link-consignment-settlement-work",
      "relation.append",
      "page-wcw-bracelet-finance-control",
      entityRef("typed_record", "record-wcw-consignment-physical-bracelet-30-10-20"),
      actorRef("agent", "agent-wcw-work-governance"),
      relationRecord,
      { auditEvent: "audit-wcw-link-consignment-settlement-work" },
    ));

    const commitmentBase = {
      id: "commitment-wcw-merchant-share-unit-reserve",
      amount: { amount: "10", currency: "CNY" },
      status: "proposed",
      source_document_id: "document-wcw-bracelet-product",
      submitted_by: actorRef("agent", "agent-wcw-finance-governance"),
      accountable_owner: actorRef("human", "human-wcw-owner"),
      relation_ids: ["relation-wcw-consignment-settlement-work"],
      evidence_refs: ["record-wcw-consignment-physical-bracelet-30-10-20"],
      approval_refs: [],
      audit_event_ids: ["audit-wcw-propose-merchant-share-unit-reserve"],
      due_at: null,
      created_at: "2026-07-26T15:14:00+08:00",
      updated_at: "2026-07-26T15:14:00+08:00",
    };
    await post(base, "/v1/company-os/actions/dispatch", action(
      "action-wcw-propose-merchant-share-unit-reserve",
      "commitment.propose",
      "page-wcw-bracelet-finance-control",
      entityRef("work_item", "work-wcw-configure-bracelet-settlement"),
      actorRef("agent", "agent-wcw-finance-governance"),
      commitmentBase,
      {
        requiredPermission: "finance.commitment.write",
        riskTier: "r2",
        auditEvent: "audit-wcw-propose-merchant-share-unit-reserve",
      },
    ));

    const approvalRequested = {
      id: "approval-wcw-merchant-share-unit-reserve",
      subject_ref: entityRef("financial_record", "commitment-wcw-merchant-share-unit-reserve"),
      action_summary: "Authorize commitment.append for the ¥10 merchant-share unit reserve",
      requested_by: actorRef("agent", "agent-wcw-finance-governance"),
      required_approver_refs: [actorRef("human", "human-wcw-owner")],
      required_actor_type: "human",
      policy_ref: "page-wcw-bracelet-finance-control:commitment.append",
      status: "requested",
      decided_by: [],
      decision_note: null,
      evidence_refs: ["record-wcw-consignment-physical-bracelet-30-10-20"],
      requested_at: "2026-07-26T15:16:00+08:00",
      decided_at: null,
      expires_at: FUTURE,
    };
    await post(base, "/v1/company-os/actions/dispatch", action(
      "action-wcw-request-merchant-share-unit-approval",
      "approval.request",
      "page-wcw-bracelet-finance-control",
      entityRef("financial_record", "commitment-wcw-merchant-share-unit-reserve"),
      actorRef("agent", "agent-wcw-finance-governance"),
      approvalRequested,
      {
        policyRef: "page-wcw-bracelet-finance-control:approval.request",
        auditEvent: "audit-wcw-request-merchant-share-unit-approval",
        requestedAt: "2026-07-26T15:16:00+08:00",
      },
    ));

    const queuedCommitment = {
      ...commitmentBase,
      status: "pending_approval",
      approval_refs: ["approval-wcw-merchant-share-unit-reserve"],
      audit_event_ids: ["audit-wcw-merchant-share-unit-enter-queue"],
      updated_at: "2026-07-26T15:18:00+08:00",
    };
    await post(base, "/v1/company-os/actions/dispatch", action(
      "action-wcw-merchant-share-unit-enter-queue",
      "commitment.append",
      "page-wcw-bracelet-finance-control",
      entityRef("financial_record", "commitment-wcw-merchant-share-unit-reserve"),
      actorRef("agent", "agent-wcw-finance-governance"),
      queuedCommitment,
      {
        requiredPermission: "finance.commitment.write",
        riskTier: "r3",
        requiresHumanApproval: true,
        approvalRefs: ["approval-wcw-merchant-share-unit-reserve"],
        auditEvent: "audit-wcw-merchant-share-unit-enter-queue",
        requestedAt: "2026-07-26T15:18:00+08:00",
      },
    ));

    const approvalApproved = {
      ...approvalRequested,
      status: "approved",
      decided_by: [actorRef("human", "human-wcw-owner")],
      decision_note: "Approve the unit merchant-share reserve for the physical bracelet consignment model. This does not approve any payment.",
      decided_at: "2026-07-26T15:20:00+08:00",
    };
    await post(base, "/v1/company-os/actions/dispatch", action(
      "action-wcw-decide-merchant-share-unit-approval",
      "approval.decide",
      "page-wcw-bracelet-finance-control",
      entityRef("approval", "approval-wcw-merchant-share-unit-reserve"),
      actorRef("human", "human-wcw-owner"),
      approvalApproved,
      {
        policyRef: "page-wcw-bracelet-finance-control:approval.decide",
        requiredPermission: "company.approve",
        riskTier: "r2",
        auditEvent: "audit-wcw-decide-merchant-share-unit-approval",
        requestedAt: "2026-07-26T15:20:00+08:00",
      },
    ));

    const approvedCommitment = {
      ...commitmentBase,
      status: "approved",
      approval_refs: ["approval-wcw-merchant-share-unit-reserve"],
      audit_event_ids: ["audit-wcw-merchant-share-unit-approved"],
      updated_at: "2026-07-26T15:22:00+08:00",
    };
    await post(base, "/v1/company-os/actions/dispatch", action(
      "action-wcw-merchant-share-unit-approve",
      "commitment.append",
      "page-wcw-bracelet-finance-control",
      entityRef("financial_record", "commitment-wcw-merchant-share-unit-reserve"),
      actorRef("agent", "agent-wcw-finance-governance"),
      approvedCommitment,
      {
        requiredPermission: "finance.commitment.write",
        riskTier: "r3",
        requiresHumanApproval: true,
        approvalRefs: ["approval-wcw-merchant-share-unit-reserve"],
        auditEvent: "audit-wcw-merchant-share-unit-approved",
        requestedAt: "2026-07-26T15:22:00+08:00",
      },
    ));

    const snapshot = await get(base, "/v1/company-os/snapshot");
    const wcwDocuments = (snapshot.documents ?? []).filter((entry) => entry.space_id === "wanchengwanling");
    const wcwModules = (snapshot.business_modules ?? []).filter((entry) => entry.id.startsWith("module-wcw-"));
    const wcwWorkItems = (snapshot.work_items ?? []).filter((entry) => entry.id.startsWith("work-wcw-"));
    const wcwAssignments = (snapshot.assignments ?? []).filter((entry) => entry.id.startsWith("assignment-work-wcw-"));
    const wcwOrgUnits = (snapshot.organization?.org_units ?? []).filter((entry) => entry.id.startsWith("orgunit-wcw-"));
    const wcwMemberships = (snapshot.organization?.memberships ?? []).filter((entry) => entry.id.startsWith("membership-wcw-"));
    const wcwActors = (snapshot.actors ?? []).filter((entry) => entry.id.includes("wcw") || entry.id === "human-wcw-owner");
    const commitment = (snapshot.commitments ?? []).find((entry) => entry.id === "commitment-wcw-merchant-share-unit-reserve");
    const approval = (snapshot.approvals ?? []).find((entry) => entry.id === "approval-wcw-merchant-share-unit-reserve");
    const relation = (snapshot.relations ?? []).find((entry) => entry.id === "relation-wcw-consignment-settlement-work");
    const payments = snapshot.payments ?? [];

    const expectedWorkIds = workItems.map((item) => item.id).sort();
    const actualWorkIds = wcwWorkItems.map((item) => item.id).sort();
    if (wcwDocuments.length !== 12) throw new Error(`expected 12 Wanchengwanling documents, got ${wcwDocuments.length}`);
    if (wcwModules.length !== 11) throw new Error(`expected 11 Wanchengwanling modules, got ${wcwModules.length}`);
    if (JSON.stringify(actualWorkIds) !== JSON.stringify(expectedWorkIds)) {
      throw new Error(`work item mismatch: ${JSON.stringify({ expectedWorkIds, actualWorkIds })}`);
    }
    if (wcwAssignments.length !== workItems.length) throw new Error(`expected ${workItems.length} assignments, got ${wcwAssignments.length}`);
    if (wcwOrgUnits.length !== 3) throw new Error(`expected 3 org units, got ${wcwOrgUnits.length}`);
    if (wcwMemberships.length !== memberships.length) throw new Error(`expected ${memberships.length} memberships, got ${wcwMemberships.length}`);
    if (!commitment || commitment.status !== "approved" || commitment.amount?.amount !== "10" || commitment.amount?.currency !== "CNY") {
      throw new Error(`merchant-share unit Commitment is not approved as expected: ${JSON.stringify(commitment)}`);
    }
    if (!approval || approval.status !== "approved") {
      throw new Error(`merchant-share Approval is not approved as expected: ${JSON.stringify(approval)}`);
    }
    if (!relation) throw new Error("missing consignment settlement WorkItem relation");
    if (payments.length !== 0) throw new Error(`expected no Payments, got ${payments.length}`);
    if (!wcwActors.some((entry) => entry.id === "agent-wcw-lead")) throw new Error("missing Lead Agent");
    if (!wcwActors.some((entry) => entry.id === "agent-wcw-finance-governance")) throw new Error("missing Finance Governance Agent");

    const workProjection = await post(base, "/v1/company-os/work-query", {
      milestone_refs: ["milestone-wcw-mvp-launch"],
    });
    if (workProjection.summary?.total !== workItems.length) {
      throw new Error(`work-query total mismatch: ${JSON.stringify(workProjection.summary)}`);
    }

    console.log(JSON.stringify({
      status: "passed",
      store_root: storeRoot,
      document_space: "wanchengwanling",
      counts: {
        documents: wcwDocuments.length,
        business_modules: wcwModules.length,
        actors: wcwActors.length,
        org_units: wcwOrgUnits.length,
        memberships: wcwMemberships.length,
        milestones: (snapshot.milestones ?? []).filter((entry) => entry.id.startsWith("milestone-wcw-")).length,
        work_items: wcwWorkItems.length,
        assignments: wcwAssignments.length,
        approvals: (snapshot.approvals ?? []).filter((entry) => entry.id.includes("wcw")).length,
        commitments: (snapshot.commitments ?? []).filter((entry) => entry.id.includes("wcw")).length,
        payments: payments.length,
      },
      organization: {
        lead_agent: "agent-wcw-lead",
        governance_agents: [
          "agent-wcw-docs-governance",
          "agent-wcw-work-governance",
          "agent-wcw-finance-governance",
          "agent-wcw-org-governance",
        ],
        business_agents: [
          "agent-wcw-merchant-ops",
          "agent-wcw-procurement",
          "agent-wcw-content-growth",
          "agent-wcw-creator-outreach",
          "agent-wcw-development",
          "agent-wcw-ip-design",
        ],
      },
      work: {
        milestone: "milestone-wcw-mvp-launch",
        work_item_ids: actualWorkIds,
        projection_summary: workProjection.summary,
      },
      finance: {
        approved_commitment: commitment.id,
        amount: commitment.amount,
        approval: approval.id,
        no_payment_inferred: payments.length === 0,
        unknown_procurement_amounts_remain_planned: [
          "Polaroid purchase price",
          "food coupon procurement amount",
          "magnet production quote",
          "bracelet manufacturing quote",
        ],
      },
      implementation_status: {
        docs_cli: "implemented",
        work_cli: "implemented",
        org_dedicated_cli: "planned; seeded through existing Store/API administrative surface",
        finance_dedicated_cli: "planned; commitment/approval seeded through governed Action dispatcher",
        custom_pages: "definition-only; actual UI implementation remains separate",
      },
      boundaries: {
        project_object: false,
        task_graph: false,
        goal_phase: false,
        payment_inferred_from_commitment: false,
        github_prd_overwrites_commercial_truth: false,
        seed_is_product_entrypoint: false,
      },
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
