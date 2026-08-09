#!/usr/bin/env node

/**
 * Seed Wanchengwanling's Company OS Docs structure.
 *
 * This builds the business-project Docs substrate, not repository markdown:
 * Documents, BusinessModules, CustomPageDefinition metadata, and core
 * TypedRecords for the MVP commercial model. Default mode uses an isolated
 * temporary Store so the result is acceptance-safe and repeatable.
 */

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const token = "wanchengwanling-docs-v0-token";
const NOW = "2026-07-26T13:00:00+08:00";

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

function slug(value) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
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

async function postBootstrapActor(base, record) {
  const response = await fetch(`${base}/v1/company-os/actors`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-harness-company-os-token": token,
    },
    body: JSON.stringify(record),
  });
  const data = await response.json();
  if (response.ok && data.ok !== false) return data.result ?? data;
  if (response.status === 403) return post(base, "/v1/company-os/actors", admin(record));
  throw new Error(`/v1/company-os/actors failed: HTTP ${response.status} ${JSON.stringify(data)}`);
}

async function get(base, path) {
  const response = await fetch(`${base}${path}`, { headers: { accept: "application/json" } });
  const data = await response.json();
  if (!response.ok) throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  return data.result ?? data;
}

const docs = [
  ["00", "project-home", "00 Project Home / 商业总览"],
  ["01", "business-model", "01 Business Model / 商业模式"],
  ["02", "bracelet-product", "02 Bracelet & Product / 手环与产品售卖"],
  ["03", "route-ar-experience", "03 Route & AR Experience / 景点路线与 AR 体验"],
  ["04", "merchant-network", "04 Merchant Network / 商家网络"],
  ["05", "rewards-procurement-inventory", "05 Rewards, Procurement & Inventory / 奖品、采购与库存"],
  ["06", "content-growth", "06 Content Growth / 自媒体内容增长"],
  ["07", "creator-outreach", "07 Creator Outreach / 博主合作"],
  ["08", "launch-readiness", "08 Launch Readiness / 上线准备"],
  ["09", "ip-product-design", "09 IP & Product Design / IP 与产品设计"],
  ["10", "software-product-sources", "10 Software Product Sources / GitHub PRD 映射"],
];

const modules = [
  {
    key: "project-home",
    name: "Wanchengwanling Project Home",
    purpose: "Human-readable operating overview for the Wanchengwanling business project.",
    recordTypes: ["project_overview", "mvp_scope", "operating_principle"],
    customPages: [
      ["command-center", "Wanchengwanling Command Center", "WanchengwanlingCommandCenter"],
    ],
  },
  {
    key: "business-model",
    name: "Wanchengwanling Business Model",
    purpose: "Commercial model, value proposition, revenue/cost logic, and replication strategy.",
    recordTypes: ["revenue_model", "cost_model", "user_value_proposition", "replication_model"],
    customPages: [
      ["business-model-canvas", "Business Model Canvas", "WanchengwanlingBusinessModelCanvas"],
      ["new-site-blueprint", "Replication Kit / New Site Blueprint", "WanchengwanlingNewSiteBlueprint"],
    ],
  },
  {
    key: "bracelet-product",
    name: "Bracelet & Product Sales",
    purpose: "Bracelet products, pricing, sales channels, consignment rules, and entitlement packaging.",
    recordTypes: ["bracelet_product", "pricing_rule", "consignment_rule", "sales_channel"],
  },
  {
    key: "route-ar-experience",
    name: "Route & AR Experience",
    purpose: "Site, scenic spots, AR check-in rules, and 8/12-step reward eligibility.",
    recordTypes: ["site", "spot", "ar_rule", "reward_eligibility_rule", "lottery_eligibility_rule"],
  },
  {
    key: "merchant-network",
    name: "Merchant Network",
    purpose: "Merchant profiles, capability tags, contact history, and go-live readiness.",
    recordTypes: ["merchant", "merchant_capability", "contact_log", "go_live_checklist"],
    customPages: [
      ["merchant-network-console", "Merchant Network Console", "WanchengwanlingMerchantNetworkConsole"],
    ],
  },
  {
    key: "rewards-procurement-inventory",
    name: "Rewards, Procurement & Inventory",
    purpose: "Rewards, prize pools, purchase orders, logistics, inventory allocation, and redemption points.",
    recordTypes: ["reward", "prize_pool", "purchase_order", "shipment", "inventory_allocation", "redemption_point"],
  },
  {
    key: "content-growth",
    name: "Content Growth",
    purpose: "Self-media channel strategy, content campaigns, publishing records, and growth metrics.",
    recordTypes: ["channel_account", "content_campaign", "post_draft", "publish_record", "metric_observation"],
  },
  {
    key: "creator-outreach",
    name: "Creator Outreach",
    purpose: "Blogger/KOL leads, outreach, proposals, deliverables, and collaboration metrics.",
    recordTypes: ["creator_lead", "outreach_record", "collaboration_proposal", "deliverable", "creator_metric"],
  },
  {
    key: "launch-readiness",
    name: "Launch Readiness",
    purpose: "MVP readiness gates across software, AR, merchant, inventory, content, finance, and field operations.",
    recordTypes: ["launch_milestone", "risk", "acceptance_evidence", "readiness_gate"],
    customPages: [
      ["launch-readiness-dashboard", "Launch Readiness Dashboard", "WanchengwanlingLaunchReadinessDashboard"],
    ],
  },
  {
    key: "ip-product-design",
    name: "IP & Product Design",
    purpose: "IP characters, visual/product assets, AR assets, SKU design, reviews, manufacturing specs, and usage.",
    recordTypes: ["ip_character", "visual_asset", "product_design_asset", "ar_asset", "sku_design", "design_review", "manufacturing_spec", "asset_usage"],
  },
  {
    key: "software-product-sources",
    name: "Software Product Sources",
    purpose: "GitHub-hosted PRD, architecture, ADR, design, and delivery-reference mapping.",
    recordTypes: ["external_project", "product_doc_source", "product_doc_snapshot", "source_sync_run", "delivery_ref"],
  },
];

const typedRecords = [
  ["project-home", "project_overview", "project-overview-mvp", "Wanchengwanling MVP", {
    statement: "AR cultural-tourism MVP built around bracelet purchase, AR check-in, physical cultural products, merchant participation, and content growth.",
    stage: "mvp",
    primary_loop: "buy bracelet -> AR check-in -> 8 spot magnet redemption -> 12 spot lottery -> merchant redemption/benefits -> content sharing",
  }],
  ["business-model", "user_value_proposition", "uvp-tourist-ar-cultural-tour", "Tourist value proposition", {
    customer: "tourist",
    value: "A more shareable and rewarding ancient-city route through AR scenes, bracelet identity, physical souvenirs, prize lottery, and merchant benefits.",
  }],
  ["business-model", "revenue_model", "revenue-bracelet-first", "Bracelet-first revenue model", {
    primary_revenue: ["physical_nfc_bracelet", "virtual_bracelet"],
    future_revenue: ["merchant cooperation", "site/city launch service", "joint operation revenue share", "cultural product sales"],
  }],
  ["business-model", "cost_model", "cost-mvp-operating", "MVP operating costs", {
    categories: ["bracelet manufacturing", "AR content", "magnet production", "polaroid grand prizes", "food coupon procurement", "merchant operations", "content production", "logistics"],
  }],
  ["business-model", "replication_model", "replication-configurable-site-kit", "Configurable site launch kit", {
    thesis: "The mini program is modular and can be quickly customized for a new scenic area, city, or commercial district through configuration, new spots, AR assets, merchants, rewards, and launch operations.",
  }],
  ["bracelet-product", "bracelet_product", "bracelet-physical-nfc", "Physical NFC bracelet", {
    price_cny: 30,
    channel: "merchant_consignment",
    merchant_share_cny: 10,
    company_share_cny: 20,
    entitlement: ["AR check-in", "8 spot magnet redemption eligibility", "12 spot lottery eligibility", "merchant benefits where configured"],
  }],
  ["bracelet-product", "bracelet_product", "bracelet-virtual", "Virtual bracelet", {
    price_cny: 20,
    channel: "mini_program",
    entitlement: ["AR check-in", "8 spot magnet redemption eligibility", "12 spot lottery eligibility", "merchant benefits where configured"],
  }],
  ["bracelet-product", "consignment_rule", "consignment-physical-bracelet-30-10-20", "Physical bracelet consignment split", {
    sale_price_cny: 30,
    merchant_share_cny: 10,
    company_share_cny: 20,
    settlement_owner: "Finance",
  }],
  ["route-ar-experience", "site", "site-jieyang-ancient-city", "Jieyang Ancient City", {
    mvp_spot_count: 12,
    magnet_unlock_checkins: 8,
    lottery_unlock_checkins: 12,
  }],
  ["route-ar-experience", "reward_eligibility_rule", "rule-8-checkins-ar-magnet", "8 check-ins unlock AR magnet", {
    required_checkin_count: 8,
    reward_ref: "reward-ar-magnet",
    purpose: "Keep the core reward achievable for most visitors.",
  }],
  ["route-ar-experience", "lottery_eligibility_rule", "rule-12-checkins-lottery", "12 check-ins unlock lottery", {
    required_checkin_count: 12,
    prize_pool_ref: "prize-pool-mvp-lottery",
    purpose: "Increase route completion, stay time, merchant exposure, and social sharing.",
  }],
  ["merchant-network", "merchant_capability", "merchant-capabilities-mvp", "MVP merchant capability tags", {
    capabilities: [
      "sells_physical_bracelet",
      "magnet_consignment_or_redemption",
      "prize_supplier",
      "prize_redemption_point",
      "bracelet_benefit_partner",
      "displayed_in_mini_program_shop_list",
    ],
    note: "Capabilities are non-exclusive; one merchant may hold several roles.",
  }],
  ["rewards-procurement-inventory", "reward", "reward-ar-magnet", "AR fridge magnet", {
    unlock_rule_ref: "rule-8-checkins-ar-magnet",
    product_design_module_ref: "module-wcw-ip-product-design",
    redemption_mode: "merchant_or_platform_configured_redemption",
  }],
  ["rewards-procurement-inventory", "prize_pool", "prize-pool-mvp-lottery", "MVP lottery prize pool", {
    unlock_rule_ref: "rule-12-checkins-lottery",
    prizes: [
      { name: "Polaroid camera", quantity: 2 },
      { name: "Local food coupons", examples: ["green bean cake", "low-ticket Chaozhou/Shantou specialty snacks"] },
    ],
  }],
  ["ip-product-design", "product_design_asset", "asset-ar-magnet-design", "AR magnet design asset", {
    product_ref: "reward-ar-magnet",
    asset_kinds: ["artwork", "marker_image", "AR trigger", "packaging"],
    status: "planned",
  }],
  ["ip-product-design", "product_design_asset", "asset-bracelet-design", "Bracelet design asset", {
    product_refs: ["bracelet-physical-nfc", "bracelet-virtual"],
    asset_kinds: ["wristband visual", "NFC placement", "packaging", "instruction card"],
    status: "planned",
  }],
  ["ip-product-design", "ip_character", "ip-main-character", "Main IP character", {
    usage: ["AR animation", "bracelet packaging", "magnet artwork", "content assets"],
    status: "planned",
  }],
  ["launch-readiness", "readiness_gate", "gate-mvp-launch-readiness", "MVP launch readiness gate", {
    gates: ["software PRD mapped", "AR spots accepted", "bracelets ready", "magnet reward configured", "lottery prize pool ready", "merchant list ready", "content launch plan ready"],
  }],
  ["software-product-sources", "external_project", "external-project-wanchengwanling", "cyl19970726/wanchengwanling dev", {
    repo: "cyl19970726/wanchengwanling",
    branch: "dev",
    role: "software_product_source",
  }],
];

async function main() {
  execFileSync("cargo", ["build", "-p", "firm-cli"], { cwd: repoRoot, stdio: "inherit" });

  const root = await mkdtemp(join(tmpdir(), "company-os-wcw-docs-v0-"));
  const explicitStoreRoot = argument("--store", "");
  const projectSelector = argument("--project", "");
  const useProject = !explicitStoreRoot && Boolean(projectSelector);
  const storeRoot = explicitStoreRoot || join(root, "store");
  const harnessArgs = (args) => useProject ? ["--project", projectSelector, ...args] : args;
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
    const preflight = await get(base, "/v1/company-os/snapshot");
    const existingDocs = (preflight.documents ?? []).filter((entry) => entry.space_id === "wanchengwanling");
    const existingModules = (preflight.business_modules ?? []).filter((entry) => entry.id.startsWith("module-wcw-"));
    const existingTyped = preflight.typed_records ?? [];
    const existingRelations = preflight.relations ?? [];
    const existingPageDefinitions = preflight.custom_page_definitions ?? [];
    const requiredCustomPageIds = [
      "page-wcw-command-center",
      "page-wcw-business-model-canvas",
      "page-wcw-new-site-blueprint",
      "page-wcw-merchant-network-console",
      "page-wcw-launch-readiness-dashboard",
    ];
    const completeExisting =
      existingDocs.length >= 12 &&
      existingModules.length >= modules.length &&
      requiredCustomPageIds.every((id) => existingPageDefinitions.some((entry) => entry.id === id)) &&
      typedRecords.every(([moduleKey, , recordKey]) =>
        existingRelations.some((relation) =>
          relation.lifecycle_status !== "archived" &&
          relation.relation_type === "source_for" &&
          relation.from_ref?.id === `document-wcw-${moduleKey}` &&
          relation.to_ref?.id === `record-wcw-${recordKey}`)) &&
      existingTyped.some((entry) => entry.id === "record-wcw-bracelet-physical-nfc" && entry.fields?.price_cny === 30 && entry.fields?.merchant_share_cny === 10) &&
      existingTyped.some((entry) => entry.id === "record-wcw-rule-8-checkins-ar-magnet" && entry.fields?.required_checkin_count === 8) &&
      existingTyped.some((entry) => entry.id === "record-wcw-rule-12-checkins-lottery" && entry.fields?.required_checkin_count === 12);
    if (completeExisting) {
      console.log(JSON.stringify({
        status: "already_exists",
        store_root: preflight.source?.store_root ?? storeRoot,
        project: useProject ? projectSelector : null,
        document_space: "wanchengwanling",
        document_count: existingDocs.length,
        module_count: existingModules.length,
        note: "Wanchengwanling Docs substrate already exists; no append was attempted.",
      }, null, 2));
      return;
    }
    if (existingDocs.length || existingModules.length) {
      throw new Error(`partial Wanchengwanling Docs rows already exist; refusing to append over partial state: ${JSON.stringify({
        document_count: existingDocs.length,
        module_count: existingModules.length,
      })}`);
    }

    await postBootstrapActor(base, {
      actor_type: "human",
      actor: {
        id: "human-wcw-owner",
        display_name: "Wanchengwanling Owner",
        title: "Project owner",
        status: "active",
        availability: "available",
        membership_refs: [],
        responsibility_summary: "Owns Wanchengwanling business, product, launch, and finance decisions.",
        permission_policy_refs: ["company_os.admin", "company.records.write"],
        authority_policy_refs: ["company_os.admin"],
        created_at: NOW,
        updated_at: NOW,
      },
    });
    await post(base, "/v1/company-os/actors", admin({
      actor_type: "agent",
      actor: {
        id: "agent-wcw-docs-governance",
        display_name: "Wanchengwanling Docs Governance Agent",
        role: "Docs governance",
        status: "active",
        availability: "available",
        assignment_capacity: 4,
        exclusive_assignment_ref: null,
        home_org_unit_ref: null,
        membership_refs: [],
        responsibility_summary: "Maintains Wanchengwanling Company OS Docs structure and source mappings.",
        capability_refs: ["company.records.write"],
        permission_policy_refs: ["company.records.write"],
        system_prompt_ref: "document-prompt-wcw-docs-governance",
        tool_refs: ["tool-company-records"],
        skill_refs: ["company-docs-operator"],
        accepted_work_types: ["docs_governance"],
        escalation_policy_ref: "policy-wcw-owner-escalation",
        runtime_refs: [],
        native_session_refs: [],
        created_at: NOW,
        updated_at: NOW,
      },
    }));

    await post(base, "/v1/company-os/documents", admin({
      id: "document-wcw-root",
      space_id: "wanchengwanling",
      parent_document_id: null,
      title: "Wanchengwanling / 万城万灵",
      kind: "page",
      lifecycle_status: "active",
      block_ids: [],
      template_ref: null,
      permission_policy_refs: ["company.records.write"],
      reference_refs: [],
      created_by: actorRef("human", "human-wcw-owner"),
      updated_by: actorRef("human", "human-wcw-owner"),
      created_at: NOW,
      updated_at: NOW,
    }));

    for (const [, key, title] of docs) {
      await post(base, "/v1/company-os/documents", admin({
        id: `document-wcw-${key}`,
        space_id: "wanchengwanling",
        parent_document_id: "document-wcw-root",
        title,
        kind: "page",
        lifecycle_status: "active",
        block_ids: [],
        template_ref: null,
        permission_policy_refs: ["company.records.write"],
        reference_refs: [],
        created_by: actorRef("human", "human-wcw-owner"),
        updated_by: actorRef("human", "human-wcw-owner"),
        created_at: NOW,
        updated_at: NOW,
      }));
    }

    const cliEnv = { ...env, HARNESS_ROOT: storeRoot, HARNESS_COMPANY_OS_TOKEN: token };
    if (useProject) delete cliEnv.HARNESS_ROOT;
    const run = (args) => JSON.parse(execFileSync(harness, harnessArgs(args), { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    const moduleByKey = new Map();
    const definitionByKey = new Map();

    for (const module of modules) {
      const moduleId = `module-wcw-${module.key}`;
      const viewId = `view-wcw-${module.key}`;
      const definitionId = `page-wcw-${module.key}`;
      const moduleResult = run([
        "company", "docs", "module", "create",
        "--id", moduleId,
        "--root-document", `document-wcw-${module.key}`,
        "--name", module.name,
        "--purpose", module.purpose,
        ...module.recordTypes.flatMap((type) => ["--record-type", type]),
        "--default-view-id", viewId,
        "--default-view-title", `${module.name} records`,
        "--authority", "human-wcw-owner",
      ]);
      if (moduleResult.ok !== true) throw new Error(`module create failed for ${module.key}: ${JSON.stringify(moduleResult)}`);
      const definitionResult = run([
        "company", "docs", "page-definition", "create",
        "--id", definitionId,
        "--module", moduleId,
        "--fallback-view", viewId,
        "--purpose", `${module.name} standard module surface.`,
        "--package-id", `package-wcw-${module.key}`,
        "--fixture-ref", `wanchengwanling-${module.key}-v0`,
        "--visual-contract-ref", `docs/design/company-os/wanchengwanling/${module.key}`,
        "--authority", "human-wcw-owner",
        "--owner", "human-wcw-owner",
        "--component", "StructuredDocumentView",
      ]);
      if (definitionResult.ok !== true) throw new Error(`page-definition failed for ${module.key}: ${JSON.stringify(definitionResult)}`);
      moduleByKey.set(module.key, moduleId);
      definitionByKey.set(module.key, definitionId);

      for (const [customKey, title, component] of module.customPages ?? []) {
        const customDefinition = run([
          "company", "docs", "page", "scaffold",
          "--id", `page-wcw-${customKey}`,
          "--module", moduleId,
          "--fallback-view", viewId,
          "--title", title,
          "--authority", "human-wcw-owner",
          "--artifact-ref", `apps/agent-dashboard/src/company-os/page-packages/wanchengwanling/${component}.tsx`,
          "--visual-contract-ref", `docs/design/company-os/wanchengwanling/${customKey}`,
        ]);
        if (customDefinition.ok !== true) throw new Error(`custom page scaffold failed for ${customKey}: ${JSON.stringify(customDefinition)}`);
      }
    }

    for (const [moduleKey, recordType, recordKey, title, fields] of typedRecords) {
      const record = run([
        "company", "docs", "typed-record", "append",
        "--definition", definitionByKey.get(moduleKey),
        "--module", moduleByKey.get(moduleKey),
        "--source-document", `document-wcw-${moduleKey}`,
        "--id", `record-wcw-${recordKey}`,
        "--record-type", recordType,
        "--title", title,
        "--fields-json", JSON.stringify(fields),
        "--status", "active",
        "--actor", "agent-wcw-docs-governance",
      ]);
      if (record.ok !== true) throw new Error(`typed-record append failed for ${recordKey}: ${JSON.stringify(record)}`);
      const relation = run([
        "company", "docs", "relation", "link",
        "--definition", definitionByKey.get(moduleKey),
        "--from-document", `document-wcw-${moduleKey}`,
        "--to-record", `record-wcw-${recordKey}`,
        "--relation-id", `relation-wcw-${moduleKey}-${recordKey}`,
        "--actor", "agent-wcw-docs-governance",
      ]);
      if (relation.ok !== true) throw new Error(`source_for relation append failed for ${recordKey}: ${JSON.stringify(relation)}`);
    }

    const snapshot = await get(base, "/v1/company-os/snapshot");
    const documents = snapshot.documents ?? [];
    const businessModules = snapshot.business_modules ?? [];
    const typed = snapshot.typed_records ?? [];
    const relations = snapshot.relations ?? [];
    const pageDefinitions = snapshot.custom_page_definitions ?? [];
    const customPageIds = [
      "page-wcw-command-center",
      "page-wcw-business-model-canvas",
      "page-wcw-new-site-blueprint",
      "page-wcw-merchant-network-console",
      "page-wcw-launch-readiness-dashboard",
    ];

    const missingCustomPages = customPageIds.filter((id) => !pageDefinitions.some((entry) => entry.id === id));
    if (documents.filter((entry) => entry.space_id === "wanchengwanling").length !== 12) {
      throw new Error(`expected 12 Wanchengwanling documents including root: ${documents.filter((entry) => entry.space_id === "wanchengwanling").length}`);
    }
    if (businessModules.filter((entry) => entry.id.startsWith("module-wcw-")).length !== modules.length) {
      throw new Error("missing Wanchengwanling business modules");
    }
    if (missingCustomPages.length) {
      throw new Error(`missing custom page definitions: ${missingCustomPages.join(", ")}`);
    }
    if (!typed.some((entry) => entry.id === "record-wcw-bracelet-physical-nfc" && entry.fields?.price_cny === 30 && entry.fields?.merchant_share_cny === 10)) {
      throw new Error("missing physical bracelet pricing record");
    }
    if (!typed.some((entry) => entry.id === "record-wcw-rule-8-checkins-ar-magnet" && entry.fields?.required_checkin_count === 8)) {
      throw new Error("missing 8-checkin magnet eligibility record");
    }
    if (!typed.some((entry) => entry.id === "record-wcw-rule-12-checkins-lottery" && entry.fields?.required_checkin_count === 12)) {
      throw new Error("missing 12-checkin lottery eligibility record");
    }
    const missingSourceRelations = typedRecords
      .map(([, , recordKey]) => `record-wcw-${recordKey}`)
      .filter((recordId) => !relations.some((relation) =>
        relation.lifecycle_status !== "archived" &&
        relation.relation_type === "source_for" &&
        relation.to_ref?.id === recordId));
    if (missingSourceRelations.length) {
      throw new Error(`missing Document source_for relations: ${missingSourceRelations.join(", ")}`);
    }
    if ((snapshot.approvals ?? []).length || (snapshot.financial_records ?? []).length) {
      throw new Error("Docs seed created Approval or Finance side effects");
    }

    console.log(JSON.stringify({
      status: "passed",
      store_root: snapshot.source?.store_root ?? storeRoot,
      project: useProject ? projectSelector : null,
      document_space: "wanchengwanling",
      document_count: documents.filter((entry) => entry.space_id === "wanchengwanling").length,
      module_count: businessModules.filter((entry) => entry.id.startsWith("module-wcw-")).length,
      typed_record_count: typed.filter((entry) => entry.id.startsWith("record-wcw-")).length,
      custom_pages: customPageIds,
      key_rules: {
        physical_bracelet_price_cny: 30,
        physical_bracelet_merchant_share_cny: 10,
        physical_bracelet_company_share_cny: 20,
        virtual_bracelet_price_cny: 20,
        magnet_unlock_checkins: 8,
        lottery_unlock_checkins: 12,
      },
      side_effects: {
        approvals: (snapshot.approvals ?? []).length,
        financial_records: (snapshot.financial_records ?? []).length,
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
