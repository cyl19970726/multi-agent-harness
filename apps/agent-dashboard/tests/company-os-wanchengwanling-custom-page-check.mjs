#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
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

async function main() {
  const [selection, router, host, commandCenter] = await Promise.all([
    readFile(resolve(root, "src/app/selection.ts"), "utf8"),
    readFile(resolve(root, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
    readFile(resolve(root, "src/company-os/page-packages/CustomPageHost.tsx"), "utf8"),
    readFile(resolve(root, "src/company-os/page-packages/wanchengwanling/WanchengwanlingCommandCenter.tsx"), "utf8"),
  ]);

  // Behavioral custom page URL contract: ?page= parses into a Docs
  // custom-page selection, selection sync writes the same canonical form
  // with workbench context preserved, and an already-canonical location is
  // not rewritten (no duplicate Back/Forward entry).
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(resolve(tmpdir(), "wcw-custom-page-"));
  const selectionTarget = resolve(directory, "selection.mjs");
  await writeFile(selectionTarget, ts.transpileModule(selection, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText, "utf8");
  try {
    const selectionModule = await import(pathToFileURL(selectionTarget).href);
    const installLocation = (search) => {
      let pushedUrl = "";
      globalThis.window = {
        location: { pathname: "/", search, hash: "" },
        history: { pushState: (_state, _title, url) => { pushedUrl = String(url); } },
      };
      return () => pushedUrl;
    };

    installLocation("?page=page-wcw-command-center");
    const parsed = selectionModule.selectionFromLocation(selectionModule.defaultSelection);
    check(parsed.surface === "docs" && parsed.customPageId === "page-wcw-command-center", "custom pages are URL-addressable through the page query parameter");

    const pushedPage = installLocation("?company=agent-company&api=http%3A%2F%2Flocalhost%3A8787");
    selectionModule.syncSelectionToLocation({ surface: "docs", customPageId: "page-wcw-command-center" });
    const pageUrl = pushedPage();
    check(pageUrl.includes("surface=docs") && pageUrl.includes("page=page-wcw-command-center") && pageUrl.includes("company=agent-company") && pageUrl.includes("api="), "selection sync writes the canonical surface=docs&page custom page URL while preserving Company Store and API context");

    const pushedCanonicalPage = installLocation("?surface=docs&page=page-wcw-command-center");
    selectionModule.syncSelectionToLocation({ surface: "docs", customPageId: "page-wcw-command-center" });
    check(pushedCanonicalPage() === "", "an already-canonical custom page URL is not rewritten, so browser Back/Forward stays clean");
  } finally {
    delete globalThis.window;
    await rm(directory, { recursive: true, force: true });
  }
  check(router.includes('"custom-page"') && router.includes("<CustomPageHost pageId={selection.customPageId} source={resolved.value} />"), "Company OS router mounts custom pages against the resolved Store projection");
  check(host.includes("page-wcw-command-center") && host.includes("WanchengwanlingCommandCenter") && host.includes("standardFallbackHref"), "CustomPageHost has a real Command Center renderer and a standard module fallback");
  check(commandCenter.includes('data-company-os-custom-page={pageId}') && commandCenter.includes('data-wcw-command-center="store-live"'), "Command Center exposes deterministic runtime markers for visual and browser acceptance");
  check(commandCenter.includes("custom_page_definitions") && commandCenter.includes("custom_page_packages") && commandCenter.includes("implemented_package") && commandCenter.includes("metadata_only"), "Command Center distinguishes implemented packages from metadata-only custom page declarations");
  check(commandCenter.includes("compareVersion") && commandCenter.includes(".filter((entry) => text(entry.definition_id) === pageId)") && commandCenter.includes(".sort((left, right) => compareVersion"), "Command Center uses the latest package candidate for the selected CustomPageDefinition");
  check(commandCenter.includes("record-wcw-bracelet-physical-nfc") && commandCenter.includes("record-wcw-site-jieyang-ancient-city") && commandCenter.includes("work-wcw-custom-command-center"), "Command Center reads native Wanchengwanling TypedRecords and WorkItems instead of page-local constants");
  check(commandCenter.includes("preserveCompanyOsWorkbenchContext") && commandCenter.includes("?surface=docs&document=") && commandCenter.includes("?surface=work&workItem="), "Command Center links back to standard Store-live Docs and Work routes while preserving api/project");
  check(commandCenter.includes("Primary operating docs") && commandCenter.includes("document-wcw-project-home") && commandCenter.includes("document-wcw-business-model") && commandCenter.includes("Default Document renderer"), "Command Center exposes 00/01 as primary Store-backed Docs with default Document fallback");
  check(commandCenter.includes("moduleByDocument") && commandCenter.includes("Open Module") && commandCenter.includes("data-wcw-primary-docs"), "Command Center links primary Docs to their standard Module fallback routes");
  check(!commandCenter.includes("Trademark") && !commandCenter.includes("CN-2026") && !commandCenter.includes("Brand A"), "Command Center package contains no trademark prototype copy");

  console.log(`\nWanchengwanling custom page checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
