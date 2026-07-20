#!/usr/bin/env node

/** Promote a passing store-live capture into the durable Company OS design
 * evidence set and generate an auditable baseline -> expected -> actual gallery.
 * A missing historical route is represented as data, never as a fake image. */

import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const designRoot = join(repoRoot, "docs/design/company-os-v1");
const actualRoot = join(designRoot, "actual");
const baselineSource = join(repoRoot, ".visual-evidence/company-os-v1/wave7-route-audit/capture-run.json");

function argument(name, fallback = "") {
  const index = process.argv.indexOf(name);
  return index === -1 ? fallback : process.argv[index + 1];
}

function html(value) {
  return String(value ?? "").replace(/[&<>\"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

async function sha256(path) {
  return `sha256:${createHash("sha256").update(await readFile(path)).digest("hex")}`;
}

async function main() {
  const suppliedCapture = argument("--capture-manifest");
  if (!suppliedCapture) throw new Error("--capture-manifest is required");
  const capturePath = resolve(suppliedCapture);
  const [capture, baseline, visual] = await Promise.all([
    readFile(capturePath, "utf8").then(JSON.parse),
    readFile(baselineSource, "utf8").then(JSON.parse),
    readFile(join(designRoot, "visual-contract.json"), "utf8").then(JSON.parse),
  ]);
  if (capture.status !== "passed" || capture.data_mode !== "live" || capture.data_source?.kind !== "harness-store-live") {
    throw new Error("only a passing store-live capture may be promoted to actual");
  }
  if (capture.results.length !== 26 || capture.results.some((result) => result.status !== "captured")) {
    throw new Error(`live capture must contain 26 clean captures; got ${capture.results.length}`);
  }

  await rm(actualRoot, { recursive: true, force: true });
  await mkdir(actualRoot, { recursive: true });
  const actualEntries = [];
  for (const result of capture.results) {
    const source = result.artifacts.implemented;
    const destination = join(actualRoot, basename(source));
    await cp(source, destination);
    actualEntries.push({
      case_id: result.case_id,
      page: result.page,
      viewport: result.viewport,
      route: result.route,
      file: relative(designRoot, destination),
      sha256: await sha256(destination),
      source_capture: relative(repoRoot, capturePath),
      data_mode: "store-live",
    });
  }

  const baselineEntries = baseline.results.map((result) => ({
    case_id: result.case_id,
    page: result.page,
    viewport: result.viewport,
    route: result.route,
    status: result.status,
    reason: result.reason,
    screenshot: null,
    evidence: relative(repoRoot, baselineSource),
  }));
  const baselineRecord = {
    contract: "company-os-v1-current-before-v1",
    run_id: baseline.run_id,
    status: "audited_missing_routes",
    source_manifest: relative(repoRoot, baselineSource),
    explanation: "The pre-implementation Dashboard exposed none of the twelve Company OS semantic route roots. No screenshot is substituted or fabricated.",
    results: baselineEntries,
  };
  await writeFile(join(designRoot, "current-before-missing-routes.json"), `${JSON.stringify(baselineRecord, null, 2)}\n`);

  const comparisons = [];
  for (const item of visual.cases) {
    const baselineEntry = baselineEntries.find((entry) => entry.case_id === item.id && entry.viewport === "desktop-1440x1000");
    const actual = actualEntries.find((entry) => entry.case_id === item.id && entry.viewport === "desktop-1440x1000");
    if (!baselineEntry || !actual) throw new Error(`incomplete desktop comparison for ${item.id}`);
    const expectedPath = join(designRoot, item.expected);
    comparisons.push({
      case_id: item.id,
      page: item.page,
      route: actual.route,
      baseline: baselineEntry,
      expected: { file: item.expected, sha256: await sha256(expectedPath), status: "design_reference" },
      actual,
    });
  }
  const comparisonManifest = {
    contract: "company-os-v1-visual-comparison-v1",
    source_capture: relative(repoRoot, capturePath),
    source_store_archive: relative(repoRoot, join(dirname(capturePath), "archived-harness-home")),
    source_data: capture.data_source,
    counts: { baseline_missing_routes: 12, expected_designs: 12, actual_live_desktop: 12, actual_live_responsive: 14, actual_total: 26 },
    truth: {
      baseline: "audited missing route; no image",
      expected: "human-reviewable generated design reference",
      actual: "real browser render from authoritative Harness Store projection",
    },
    comparisons,
    responsive_actual: actualEntries.filter((entry) => entry.viewport !== "desktop-1440x1000"),
  };
  await writeFile(join(designRoot, "comparison-manifest.json"), `${JSON.stringify(comparisonManifest, null, 2)}\n`);
  await writeFile(join(designRoot, "expected-vs-actual.html"), renderGallery(comparisonManifest));
  console.log(JSON.stringify({ status: "materialized", actual_count: actualEntries.length, gallery: join(designRoot, "expected-vs-actual.html") }, null, 2));
}

function renderGallery(manifest) {
  const rows = manifest.comparisons.map((entry) => `
    <section class="case">
      <header><div><span>${html(entry.page)}</span><h2>${html(entry.case_id)}</h2></div><code>${html(entry.route)}</code></header>
      <div class="triptych">
        <article class="missing"><h3>Current before</h3><div class="missing-card"><strong>Audited missing route</strong><p>${html(entry.baseline.status)}</p><small>No screenshot exists. Source: ${html(entry.baseline.evidence)}</small></div></article>
        <article><h3>Expected design</h3><a href="${html(entry.expected.file)}"><img loading="lazy" src="${html(entry.expected.file)}" alt="Expected ${html(entry.page)} design"></a></article>
        <article><h3>Actual · store-live</h3><a href="${html(entry.actual.file)}"><img loading="lazy" src="${html(entry.actual.file)}" alt="Actual live ${html(entry.page)} page"></a></article>
      </div>
    </section>`).join("");
  const responsive = manifest.responsive_actual.map((entry) => `
      <article><h3>${html(entry.page)} · ${html(entry.viewport)}</h3><a href="${html(entry.file)}"><img loading="lazy" src="${html(entry.file)}" alt="${html(entry.page)} ${html(entry.viewport)}"></a></article>`).join("");
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Company OS V1 · Current / Expected / Actual</title>
<style>
:root{color-scheme:light;font-family:Inter,ui-sans-serif,system-ui,sans-serif;color:#17221d;background:#f3f6f2}*{box-sizing:border-box}body{margin:0}.hero{padding:48px clamp(20px,5vw,72px) 34px;background:#10271e;color:#f4fff7}.hero p{max-width:860px;color:#bed4c6;line-height:1.65}.hero code{color:#a6e5ba}.case{margin:34px auto;max-width:1880px;padding:0 clamp(14px,2vw,32px)}header{display:flex;align-items:end;justify-content:space-between;gap:20px;margin-bottom:13px}header span{font-size:12px;text-transform:uppercase;letter-spacing:.12em;color:#64806f}h2{margin:4px 0 0;font-size:18px}header code{font-size:11px;color:#6d7c73}.triptych{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:14px}article{min-width:0;border:1px solid #d9e1da;border-radius:15px;background:#fff;padding:11px;box-shadow:0 8px 28px rgba(25,49,36,.06)}h3{margin:2px 2px 10px;font-size:12px;color:#547161}img{display:block;width:100%;height:auto;border-radius:9px;border:1px solid #edf1ee}.missing-card{aspect-ratio:1.44;display:grid;place-content:center;text-align:center;padding:24px;border:1px dashed #c3cec6;border-radius:9px;background:repeating-linear-gradient(-45deg,#f7f9f7,#f7f9f7 10px,#f1f4f1 10px,#f1f4f1 20px);color:#5b685f}.missing-card p{margin:8px 0}.missing-card small{max-width:340px;line-height:1.45}.responsive{max-width:1880px;margin:58px auto;padding:0 clamp(14px,2vw,32px) 70px}.responsive-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:14px}@media(max-width:1100px){.triptych{grid-template-columns:1fr}.responsive-grid{grid-template-columns:repeat(2,minmax(0,1fr))}}@media(max-width:620px){header{display:block}header code{display:block;margin-top:8px}.responsive-grid{grid-template-columns:1fr}}
</style></head><body><div class="hero"><h1>Company OS V1 · visual evidence chain</h1><p>Three distinct truths are kept separate: the audited pre-implementation state had no Company OS route to screenshot; Expected is the design target; Actual is captured from a real <code>harness serve</code> projection backed by the archived Store. No fixture is injected into the live browser.</p></div>${rows}<section class="responsive"><h2>Responsive actual · 14 live captures</h2><div class="responsive-grid">${responsive}</div></section></body></html>\n`;
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
