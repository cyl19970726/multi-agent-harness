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

async function loadTypescriptModule(path, prefix) {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), prefix));
  const input = await readFile(path, "utf8");
  const output = ts.transpileModule(input, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const target = join(directory, "module.mjs");
  await writeFile(target, output, "utf8");
  return { module: await import(pathToFileURL(target).href), directory };
}

function installLocation(search) {
  const calls = { push: [], replace: [] };
  globalThis.window = {
    location: { pathname: "/", search, hash: "" },
    history: {
      pushState: (_state, _title, url) => calls.push.push(String(url)),
      replaceState: (_state, _title, url) => calls.replace.push(String(url)),
    },
  };
  return calls;
}

async function main() {
  const [shell, router, selectionSource, api, app] = await Promise.all([
    readFile(join(dashboardRoot, "src/app/WorkbenchShell.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/app/selection.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/api.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/app/App.tsx"), "utf8"),
  ]);
  const navigation = shell.slice(shell.indexOf("const navigationGroups"), shell.indexOf("const navItems"));
  check(["PRIMARY", "OPERATIONS", "EXECUTION", "PLATFORM"].every((label) => navigation.includes(`label: "${label}"`)), "rail declares the four canonical navigation groups");
  check(["Home", "Docs", "Organization", "Work", "Approvals", "Finance", "Missions", "Workflows", "Agent Teams", "Providers", "Plugins", "Settings"].every((label) => navigation.includes(`label: "${label}"`)), "all twelve navigation destinations are present");
  check(!navigation.includes("Legacy") && !navigation.includes("Tasks") && !navigation.includes("Goals"), "legacy Goal and Task navigation is absent");
  check(shell.includes("CompanyOsRouter") && shell.includes("isCompanyOsSurface"), "Company OS surfaces mount in the real workbench shell");
  check(router.includes('"workboard"') && router.includes('"custom-page"') && !router.includes('"work-item-focus"'), "router owns the unified Work page and no Company WorkItem focus page");
  check(router.includes("<WorkOperatingPage source={resolved.value} />"), "Company Work routes to the unified read-only aggregate");
  check(!router.includes("onTransition") && !router.includes("onCreateCorrectiveWork"), "router exposes no retired Company WorkItem mutation transports");
  check(router.includes("onDecision") && router.includes("onRepairRelation") && router.includes('"X-Harness-Company-OS-Token"'), "independent Approval and Docs actions remain Store-live capability guarded");
  check(api.includes("...options.headers") && app.includes("selectedCompanyId") && app.includes("selectedSpaceId"), "browser actions preserve Company Store and Execution Space scope");
  check(!selectionSource.includes("workItemId") && !selectionSource.includes('"workItem"'), "selection contract has no Company WorkItem deep-link compatibility");

  const { module: selection, directory } = await loadTypescriptModule(join(dashboardRoot, "src/app/selection.ts"), "company-os-selection-");
  try {
    check(selection.defaultSelection.surface === "work", "Company Work remains the default surface");
    installLocation("?company=agent-company&space=agentos&project=star-harness&teamWork=work-native-1");
    const contextual = selection.selectionFromLocation(selection.defaultSelection);
    check(contextual.surface === "work" && contextual.teamWorkId === "work-native-1", "native TeamWork deep link preserves Company and Execution Space context");

    const cases = [
      ["?surface=docs&document=document-1", "docs", "documentId", "document-1"],
      ["?surface=work&teamWork=work-native-1", "work", "teamWorkId", "work-native-1"],
      ["?surface=organization&agent=standing-1", "organization", "standingAgentId", "standing-1"],
      ["?surface=approvals&approval=approval-1", "approvals", "approvalId", "approval-1"],
      ["?surface=docs&module=module-1", "docs", "moduleId", "module-1"],
    ];
    for (const [search, surface, key, value] of cases) {
      installLocation(search);
      const selected = selection.selectionFromLocation(selection.defaultSelection);
      check(selected.surface === surface && selected[key] === value, `${key} is URL-addressable on ${surface}`);
    }

    const calls = installLocation("?company=agent-company&space=agentos&project=star-harness");
    selection.syncSelectionToLocation({ surface: "work", teamWorkId: "work-native-1" });
    const pushed = calls.push.at(-1) ?? "";
    check(pushed.includes("teamWork=work-native-1") && pushed.includes("company=agent-company") && pushed.includes("space=agentos") && pushed.includes("project=star-harness"), "native TeamWork selection preserves external routing context");
    check(!pushed.includes("workItem"), "native TeamWork selection never serializes the retired workItem parameter");

    const canonical = installLocation("?surface=work");
    selection.syncSelectionToLocation({ surface: "work" });
    check(canonical.push.length === 0 && canonical.replace.at(-1) === "/", "explicit default Work route canonicalizes without a browser Back trap");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }

  const { module: sourceTruth, directory: sourceDirectory } = await loadTypescriptModule(join(dashboardRoot, "src/company-os/sourceTruth.ts"), "company-os-source-");
  try {
    const live = {
      snapshot_contract: "company-os-v1",
      projection_kind: "live_company_os",
      source: {
        kind: "harness_store",
        authoritative: true,
        project_id: "project-1",
        store_root: "/tmp/project-1",
        schema: "company-os/v1",
        revision: "fnv1a64:0123456789abcdef",
        projection: "latest_row_wins",
      },
    };
    check(sourceTruth.resolveCompanyOsData({ snapshotProjection: live, fallback: {} }).mode === "store-live", "complete server authority contract enables Store-live mode");
    check(sourceTruth.resolveCompanyOsData({ snapshotProjection: { ...live, source: { ...live.source, authoritative: false } }, fallback: {} }).mode !== "store-live", "incomplete authority fails closed");
  } finally {
    await rm(sourceDirectory, { recursive: true, force: true });
  }

  console.log(`\nCompany OS navigation checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
