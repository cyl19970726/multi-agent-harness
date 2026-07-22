#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const dashboardRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
let passed = 0;
let failed = 0;

function check(condition, message) {
  if (condition) {
    console.log(`  PASS  ${message}`);
    passed += 1;
  } else {
    console.error(`  FAIL  ${message}`);
    failed += 1;
  }
}

async function loadSelection() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "company-os-navigation-"));
  const input = await readFile(join(dashboardRoot, "src/app/selection.ts"), "utf8");
  const output = ts.transpileModule(input, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const target = join(directory, "selection.mjs");
  await writeFile(target, output, "utf8");
  return { module: await import(pathToFileURL(target).href), directory };
}

async function loadSourceTruth() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "company-os-source-truth-"));
  const input = await readFile(join(dashboardRoot, "src/company-os/sourceTruth.ts"), "utf8");
  const output = ts.transpileModule(input, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const target = join(directory, "sourceTruth.mjs");
  await writeFile(target, output, "utf8");
  return { module: await import(pathToFileURL(target).href), directory };
}

function installLocation(search) {
  let pushedUrl = "";
  globalThis.window = {
    location: { pathname: "/", search, hash: "" },
    history: { pushState: (_state, _title, url) => { pushedUrl = String(url); } },
  };
  return () => pushedUrl;
}

async function main() {
  const [shell, router, api, app] = await Promise.all([
    readFile(join(dashboardRoot, "src/app/WorkbenchShell.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/api.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/app/App.tsx"), "utf8"),
  ]);
  const navStart = shell.indexOf("const navigationGroups");
  const navEnd = shell.indexOf("const navItems", navStart);
  const navigation = shell.slice(navStart, navEnd);

  check(["PRIMARY", "OPERATIONS", "EXECUTION", "PLATFORM"].every((label) => navigation.includes(`label: \"${label}\"`)), "rail declares the four canonical navigation groups");
  check(["Home", "Docs", "Organization", "Work", "Approvals", "Finance", "Missions", "Workflows", "Agent Teams", "Providers", "Plugins", "Settings"].every((label) => navigation.includes(`label: \"${label}\"`)), "all twelve navigation destinations are present");
  check(!navigation.includes("Legacy") && !navigation.includes("Tasks") && !navigation.includes("Goals"), "old Goal/Task compatibility navigation is absent from the product rail");
  check(shell.includes("whitespace-nowrap\">{item.label}") && shell.includes("mobilePrimaryItems"), "primary labels are not truncated on desktop or mobile navigation");
  check(shell.includes("CompanyOsRouter") && shell.includes("isCompanyOsSurface"), "Company OS surfaces are mounted in the real Workbench shell");
  check(shell.includes('{companyContext ? "Company context" : "Active context"}') && shell.includes("Docs holds context, Organization holds authority, Work holds commitments, and Finance records monetary effects."), "Company OS navigation keeps four-system context separate from Mission and Wave execution context");

  const pages = ["home", "docs-workspace", "document-focus", "workboard", "work-item-focus", "finance", "agents-organization", "standing-agent-focus", "governance-proposal", "approval-focus", "business-module-focus", "human-member-focus"];
  check(pages.every((page) => router.includes(`\"${page}\"`)), "router owns all twelve core page contracts");
  check(router.includes('data-company-os-prototype={isLive ? "false" : "true"}') && router.includes("fixed fixture fallback") && router.includes("not claiming live Company OS persistence"), "fixture fallback is visibly and structurally labelled as prototype data");
  check(router.includes("Live · Store-backed Company OS") && router.includes('data-company-os-data-mode="store-live"'), "authoritative store projections have a distinct live truth label");
  check(router.includes("adaptCompanyOsDocsProjection(resolved.value,") && router.includes("adaptTrademarkOperationsProjection(resolved.value,"), "fixture and store-live routes pass the resolved projection directly into both presentation adapters");
  check((router.match(/actionsEnabled && resolved\.mode === "store-live"/g) ?? []).length >= 2 && router.includes("onTransition") && router.includes("onDecision") && router.includes('"X-Harness-Company-OS-Token"'), "WorkItem and Approval Action transports are enabled only for Store-live truth and send the session capability in the dedicated header");
  check(api.includes("...options.headers") && api.includes("payload.detail || payload.error") && app.includes("postAction(apiUrl, path, body, selectedProjectId, options)"), "browser Action requests carry scoped headers, preserve server denial detail, and refresh through the existing mutation path");

  const { module: sourceTruth, directory: sourceTruthDirectory } = await loadSourceTruth();
  try {
    const fallback = { fixture_id: "fallback" };
    const authoritative = {
      snapshot_contract: "company-os-v1",
      projection_kind: "live_company_os",
      source: {
        kind: "harness_store",
        authoritative: true,
        project_id: "company-project",
        store_root: "/tmp/company-project",
        schema: "company-os/v1",
        revision: "fnv1a64:0123456789abcdef",
        projection: "latest_row_wins",
      },
    };
    check(sourceTruth.resolveCompanyOsData({ snapshotProjection: authoritative, fallback }).mode === "store-live", "only the complete server authority contract enables store-live mode");
    check(sourceTruth.resolveCompanyOsData({ injected: { fixture_id: "capture" }, snapshotProjection: authoritative, fallback }).mode === "capture-fixture", "capture injection remains Prototype even when a live projection is available");
    const invalidAuthorityCases = [
      { ...authoritative, snapshot_contract: "company-os-v0" },
      { ...authoritative, projection_kind: "fixture" },
      { ...authoritative, source: { ...authoritative.source, kind: "fixture" } },
      { ...authoritative, source: { ...authoritative.source, authoritative: false } },
      { ...authoritative, source: { ...authoritative.source, project_id: "" } },
      { ...authoritative, source: { ...authoritative.source, store_root: "relative/store" } },
      { ...authoritative, source: { ...authoritative.source, schema: "company-os/v0" } },
      { ...authoritative, source: { ...authoritative.source, revision: "latest" } },
      { ...authoritative, source: { ...authoritative.source, projection: "event_stream" } },
    ];
    check(invalidAuthorityCases.every((candidate) => sourceTruth.resolveCompanyOsData({ snapshotProjection: candidate, fallback }).mode === "snapshot-prototype"), "every server authority field is required; each invalid variant fails closed to Prototype");
    check(sourceTruth.resolveCompanyOsData({ snapshotProjection: { projection_kind: "live_company_os" }, fallback }).mode === "snapshot-prototype", "projection presence or a live-looking kind alone cannot claim authority");
    check(sourceTruth.resolveCompanyOsData({ fallback }).mode === "prototype-fixture", "missing snapshot data uses the fixed Prototype fallback");
  } finally {
    await rm(sourceTruthDirectory, { recursive: true, force: true });
  }

  const { module: selection, directory } = await loadSelection();
  try {
    check(selection.defaultSelection.surface === "home", "Home is the default product surface");

    const routeCases = [
      ["?surface=docs&document=document-brand-a-content-operating-plan", "docs", "documentId", "document-brand-a-content-operating-plan"],
      ["?surface=work&workItem=workitem-trademark-filing-brand-a", "work", "workItemId", "workitem-trademark-filing-brand-a"],
      ["?surface=organization&agent=actor-agent-document-architecture", "organization", "standingAgentId", "actor-agent-document-architecture"],
      ["?surface=organization&person=actor-human-brand-owner", "organization", "personId", "actor-human-brand-owner"],
      ["?surface=organization&proposal=governance-proposal-trademark-management", "organization", "proposalId", "governance-proposal-trademark-management"],
      ["?surface=approvals&approval=approval-trademark-filing-fee-cn-2026-018", "approvals", "approvalId", "approval-trademark-filing-fee-cn-2026-018"],
      ["?surface=docs&module=module-trademark-management", "docs", "moduleId", "module-trademark-management"],
    ];
    for (const [search, surface, key, value] of routeCases) {
      installLocation(search);
      const result = selection.selectionFromLocation(selection.defaultSelection);
      check(result.surface === surface && result[key] === value, `${key} is explicitly URL-addressable on ${surface}`);
    }

    const pushed = installLocation("?api=http%3A%2F%2Flocalhost%3A8787");
    selection.syncSelectionToLocation({ surface: "organization", standingAgentId: "actor-agent-document-architecture" });
    check(pushed().includes("surface=organization") && pushed().includes("agent=actor-agent-document-architecture") && pushed().includes("api="), "selection sync preserves unrelated URL configuration, writes organization identity, and creates a Back/Forward history entry");
  } finally {
    delete globalThis.window;
    await rm(directory, { recursive: true, force: true });
  }

  console.log(`\nCompany OS navigation checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
