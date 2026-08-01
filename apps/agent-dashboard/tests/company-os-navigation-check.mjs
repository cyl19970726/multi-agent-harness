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
  const calls = { push: [], replace: [] };
  globalThis.window = {
    location: { pathname: "/", search, hash: "" },
    history: {
      pushState: (_state, _title, url) => { calls.push.push(String(url)); },
      replaceState: (_state, _title, url) => { calls.replace.push(String(url)); },
    },
  };
  return {
    pushed: () => calls.push.at(-1) ?? "",
    replaced: () => calls.replace.at(-1) ?? "",
    pushCount: () => calls.push.length,
    replaceCount: () => calls.replace.length,
  };
}

async function main() {
  const [shell, router, components, api, app] = await Promise.all([
    readFile(join(dashboardRoot, "src/app/WorkbenchShell.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/company-os/operations/components.tsx"), "utf8"),
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

  const pages = ["home", "docs-workspace", "document-health", "document-focus", "workboard", "work-item-focus", "finance", "agents-organization", "standing-agent-focus", "governance-proposal", "approval-focus", "business-module-focus", "human-member-focus"];
  check([...pages, "custom-page"].every((page) => router.includes(`\"${page}\"`)), "router owns all core page contracts plus the custom page runtime entry");
  check(router.includes("<CustomPageHost") && router.includes("selection.customPageId"), "router mounts Store-backed custom pages through the generic CustomPageHost");
  check(router.includes('data-company-os-prototype={isLive ? "false" : "true"}') && router.includes("fixed fixture fallback") && router.includes("not claiming live Company OS persistence"), "fixture fallback is visibly and structurally labelled as prototype data");
  check(router.includes("store-live-loading") && router.includes("prototype fixture data is suppressed") && shell.includes("livePending={livePending}"), "Company OS suppresses prototype fixtures while Store-live data is loading");
  check(router.includes("Live · Store-backed Company OS") && router.includes('data-company-os-data-mode="store-live"'), "authoritative store projections have a distinct live truth label");
  check(router.includes("adaptCompanyOsDocsProjection(resolved.value,") && router.includes("adaptTrademarkOperationsProjection(resolved.value"), "fixture and store-live routes pass the resolved projection directly into both presentation adapters");
  const routeViewportClass = router.match(/<DataTruthBanner resolved=\{resolved\} \/>\s*<div className="([^"]+)">\{children\}<\/div>/)?.[1] ?? "";
  const routeViewportTokens = new Set(routeViewportClass.split(/\s+/));
  check(
    ["h-full", "min-h-0", "min-w-0", "flex-1", "overflow-hidden"].every((token) => routeViewportTokens.has(token))
      && components.includes('className="company-workbench h-full overflow-y-auto bg-background"')
      && shell.includes('<div className="flex h-full min-h-0 flex-1">{surface}</div>'),
    "full-bleed Company OS routes bound their child viewport while PageFrame owns vertical document scrolling",
  );
  check((router.match(/actionsEnabled && resolved\.mode === "store-live"/g) ?? []).length >= 4 && router.includes("onTransition") && router.includes("onDecision") && router.includes("onCreateCorrectiveWork") && router.includes("onDocsAction") && router.includes('"X-Harness-Company-OS-Token"'), "WorkItem, Approval, Docs Health, and Document authoring transports are enabled only for Store-live truth and send the session capability in the dedicated header");
  check(api.includes("...options.headers") && api.includes("payload.detail || payload.error") && app.includes("postAction(apiUrl, path, body, selectedProjectId, selectedCompanyId, options, selectedSpaceId)"), "browser Action requests carry Company, Execution Space, and Project Binding scope, preserve server denial detail, and refresh through the existing mutation path");
  check(api.includes("fetchCompanies") && api.includes("/v1/companies") && shell.includes("CompanyPicker") && app.includes("selectedCompanyId"), "dashboard exposes a Company Store selector separate from the Project selector");

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
    check(selection.defaultSelection.surface === "work", "Work overview is the default Company OS operating board (N6)");

    installLocation("");
    const bare = selection.selectionFromLocation(selection.defaultSelection);
    check(bare.surface === "work" && !bare.workItemId && !bare.documentId, "a bare URL lands on the Work overview operating board without inventing record focus");

    installLocation("?surface=home");
    const explicitHome = selection.selectionFromLocation(selection.defaultSelection);
    check(explicitHome.surface === "home", "Home remains explicitly addressable after Work became the default surface");

    installLocation("?company=agent-company&space=agentos&project=star-harness&workItem=work-wcw-agentos-work-overview-ui");
    const contextual = selection.selectionFromLocation(selection.defaultSelection);
    check(contextual.surface === "work" && contextual.workItemId === "work-wcw-agentos-work-overview-ui", "a deep link carrying Company Store, Execution Space, and Project Binding context keeps the selected WorkItem on the Work surface");

    const routeCases = [
      ["?surface=docs&document=document-brand-a-content-operating-plan", "docs", "documentId", "document-brand-a-content-operating-plan"],
      ["?surface=work&workItem=workitem-trademark-filing-brand-a", "work", "workItemId", "workitem-trademark-filing-brand-a"],
      ["?surface=organization&agent=actor-agent-document-architecture", "organization", "standingAgentId", "actor-agent-document-architecture"],
      ["?surface=organization&person=actor-human-brand-owner", "organization", "personId", "actor-human-brand-owner"],
      ["?surface=organization&proposal=governance-proposal-trademark-management", "organization", "proposalId", "governance-proposal-trademark-management"],
      ["?surface=approvals&approval=approval-trademark-filing-fee-cn-2026-018", "approvals", "approvalId", "approval-trademark-filing-fee-cn-2026-018"],
      ["?surface=docs&module=module-trademark-management", "docs", "moduleId", "module-trademark-management"],
      ["?surface=docs&page=page-wcw-command-center", "docs", "customPageId", "page-wcw-command-center"],
      ["?surface=docs&health=structure", "docs", "docsHealth", "structure"],
    ];
    for (const [search, surface, key, value] of routeCases) {
      installLocation(search);
      const result = selection.selectionFromLocation(selection.defaultSelection);
      check(result.surface === surface && result[key] === value, `${key} is explicitly URL-addressable on ${surface}`);
    }

    const pushed = installLocation("?api=http%3A%2F%2Flocalhost%3A8787");
    selection.syncSelectionToLocation({ surface: "organization", standingAgentId: "actor-agent-document-architecture" });
    check(pushed.pushed().includes("surface=organization") && pushed.pushed().includes("agent=actor-agent-document-architecture") && pushed.pushed().includes("api=") && pushed.replaceCount() === 0, "selection sync preserves unrelated URL configuration, writes organization identity, and creates a Back/Forward history entry");

    const pushedContext = installLocation("?company=agent-company&space=agentos&project=star-harness&api=http%3A%2F%2Flocalhost%3A8787");
    selection.syncSelectionToLocation({ surface: "work", workItemId: "work-wcw-agentos-work-overview-ui" });
    const contextUrl = pushedContext.pushed();
    check(contextUrl.includes("surface=work") && contextUrl.includes("workItem=work-wcw-agentos-work-overview-ui") && contextUrl.includes("company=agent-company") && contextUrl.includes("space=agentos") && contextUrl.includes("project=star-harness") && contextUrl.includes("api="), "a selected WorkItem keeps the canonical surface=work&workItem URL with Company Store, Execution Space, Project Binding, and API context preserved (regression: bc0e025 dropped surface=work)");

    const pushedHome = installLocation("");
    selection.syncSelectionToLocation({ surface: "home" });
    check(pushedHome.pushed().includes("surface=home"), "an explicit Home selection stays URL-addressable");

    const pushedDefault = installLocation("");
    selection.syncSelectionToLocation({ surface: selection.defaultSelection.surface });
    check(pushedDefault.pushCount() === 0 && pushedDefault.replaceCount() === 0, "the default Work surface is omitted from the URL so a bare link round-trips to the Work overview");

    // Back-trap regression (work-wcw-work-surface-url-back-trap-v1): loading
    // an explicit default-surface URL must canonicalize without pushState, or
    // browser Back returns to ?surface=work which canonicalizes again.
    const trap = installLocation("?surface=work");
    selection.syncSelectionToLocation({ surface: "work" });
    check(trap.pushCount() === 0, "loading bare ?surface=work adds no history entry (no Back trap)");
    check(trap.replaced() === "/", "?surface=work canonicalizes in place to the bare default form via replaceState");

    const trapContext = installLocation("?surface=work&company=agent-company&api=http%3A%2F%2Flocalhost%3A8787");
    selection.syncSelectionToLocation({ surface: "work" });
    check(trapContext.pushCount() === 0 && trapContext.replaced().includes("company=agent-company") && trapContext.replaced().includes("api=") && !trapContext.replaced().includes("surface="), "?surface=work with Company Store and API context canonicalizes in place, preserving context without a history entry");

    const pushedNav = installLocation("?surface=home");
    selection.syncSelectionToLocation({ surface: "work" });
    check(pushedNav.pushed() === "/" && pushedNav.replaceCount() === 0, "user navigation from Home to the default Work surface still pushStates the bare entry");

    // Back/Forward contract: each history entry must parse back to the exact
    // selection it captured, so leaving and returning to a WorkItem focus
    // restores it instead of collapsing onto the default surface.
    const pushedFocus = installLocation("");
    selection.syncSelectionToLocation({ surface: "work", workItemId: "work-wcw-agentos-work-overview-ui" });
    const focusUrl = pushedFocus.pushed();
    check(focusUrl.includes("surface=work") && focusUrl.includes("workItem=work-wcw-agentos-work-overview-ui"), "selecting a WorkItem pushes the canonical surface=work&workItem history entry");
    const focusSearch = focusUrl.slice(focusUrl.indexOf("?"));
    const pushedDeselect = installLocation(focusSearch);
    selection.syncSelectionToLocation({ surface: "work" });
    check(pushedDeselect.pushed() === "/", "deselecting the WorkItem pushes the bare default-surface entry");
    installLocation(focusSearch);
    const restored = selection.selectionFromLocation(selection.defaultSelection);
    check(restored.surface === "work" && restored.workItemId === "work-wcw-agentos-work-overview-ui", "browser Back to the pushed WorkItem entry restores the Work surface and selected WorkItem");

    const pushedCanonical = installLocation("?surface=work&workItem=work-wcw-agentos-work-overview-ui&company=agent-company&space=agentos&project=star-harness");
    selection.syncSelectionToLocation({ surface: "work", workItemId: "work-wcw-agentos-work-overview-ui" });
    check(pushedCanonical.pushCount() === 0 && pushedCanonical.replaceCount() === 0, "sync is a no-op on an already-canonical WorkItem URL, so loading a deep link does not push a duplicate reordered history entry");
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
