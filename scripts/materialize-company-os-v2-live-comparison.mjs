#!/usr/bin/env node

import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const designRoot = join(repoRoot, "docs/design/company-os-v2");
const durableRoot = join(designRoot, "store-live-actual");

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>\"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

async function sha256(path) {
  return `sha256:${createHash("sha256").update(await readFile(path)).digest("hex")}`;
}

async function main() {
  const captureArg = argument("--capture-manifest");
  if (!captureArg) throw new Error("--capture-manifest is required");
  const capturePath = resolve(captureArg);
  const [capture, fixtureComparison, visual] = await Promise.all([
    readFile(capturePath, "utf8").then(JSON.parse),
    readFile(join(designRoot, "comparison-manifest-v2.2.json"), "utf8").then(JSON.parse),
    readFile(join(designRoot, "visual-contract.json"), "utf8").then(JSON.parse),
  ]);
  if (capture.status !== "passed" || capture.data_mode !== "store-live" || capture.results?.length !== 6) {
    throw new Error("materialization requires one passing six-page Store-live V2.2 capture");
  }
  const revision = visual.design_revisions.find((entry) => entry.version === "2.2");
  if (!revision) throw new Error("V2.2 visual revision is missing");
  await rm(durableRoot, { recursive: true, force: true });
  await mkdir(durableRoot, { recursive: true });

  const comparisons = [];
  for (const result of capture.results) {
    const prior = fixtureComparison.comparisons.find((entry) => entry.case_id === result.id);
    const expectedContract = revision.cases.find((entry) => entry.case_id === result.id);
    if (!prior || !expectedContract) throw new Error(`missing comparison contract for ${result.id}`);
    const source = join(repoRoot, result.file);
    const destination = join(durableRoot, basename(source));
    await cp(source, destination);
    comparisons.push({
      case_id: result.id,
      route: result.route,
      current_before: prior.baseline,
      expected: prior.expected,
      fixture_actual: prior.actual,
      store_live_actual: {
        file: relative(designRoot, destination),
        sha256: await sha256(destination),
        truth: "V2.2 browser implementation from authority-labelled Harness Store projection",
      },
    });
  }

  const manifest = {
    contract: "company-os-v2-2-store-live-comparison-v1",
    status: "human_review_pending",
    source_capture: relative(repoRoot, capturePath),
    source_store_archive: relative(repoRoot, join(dirname(capturePath), "archived-harness-home")),
    source_snapshot: relative(repoRoot, join(dirname(capturePath), "live-company-os-snapshot.json")),
    project_id: capture.data_source.project_id,
    store_source: capture.data_source.source,
    counts: { current_before: 6, expected: 6, fixture_actual: 6, store_live_actual: 6 },
    truth: {
      current_before: "V1 browser baseline",
      expected: "V2.2 generated visual direction",
      fixture_actual: "V2.2 deterministic-fixture browser render",
      store_live_actual: "V2.2 browser render from archived authority-labelled Store projection",
    },
    comparisons,
  };
  await writeFile(join(designRoot, "store-live-comparison-manifest-v2.2.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(join(designRoot, "expected-vs-store-live-v2.2.html"), gallery(manifest));
  console.log(JSON.stringify({ status: "materialized", pages: comparisons.length, gallery: join(designRoot, "expected-vs-store-live-v2.2.html") }, null, 2));
}

function gallery(manifest) {
  const rows = manifest.comparisons.map((entry) => `<section><header><strong>${escapeHtml(entry.case_id)}</strong><code>${escapeHtml(entry.route)}</code></header><div class="evidence"><article><h2>Before · V1 Actual</h2><a href="${escapeHtml(entry.current_before.file)}"><img src="${escapeHtml(entry.current_before.file)}" alt="V1 Actual"></a></article><article><h2>Expected · V2.2</h2><a href="${escapeHtml(entry.expected.file)}"><img src="${escapeHtml(entry.expected.file)}" alt="V2.2 Expected"></a></article><article><h2>Fixture Actual · V2.2</h2><a href="${escapeHtml(entry.fixture_actual.file)}"><img src="${escapeHtml(entry.fixture_actual.file)}" alt="V2.2 fixture Actual"></a></article><article class="live"><h2>Store-live Actual · V2.2</h2><a href="${escapeHtml(entry.store_live_actual.file)}"><img src="${escapeHtml(entry.store_live_actual.file)}" alt="V2.2 Store-live Actual"></a></article></div></section>`).join("");
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Company OS V2.2 · Store-live evidence</title><style>:root{font-family:Geist,Inter,system-ui,sans-serif;color:#2c2926;background:#f7f3ed}*{box-sizing:border-box}body{margin:0}.hero{padding:44px clamp(20px,5vw,72px);background:#2d2925;color:#fff9ef}.hero p{max-width:920px;color:#d8cec2;line-height:1.65}section{max-width:2100px;margin:34px auto;padding:0 clamp(14px,2vw,32px)}header{margin-bottom:12px}header code{display:block;margin-top:5px;color:#84786c}.evidence{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:12px}article{min-width:0;border:1px solid #ddd3c7;border-radius:15px;background:#fffdf9;padding:9px;box-shadow:0 12px 38px rgba(67,50,33,.06)}article.live{border-color:#9fc7ac;box-shadow:0 12px 38px rgba(38,109,65,.1)}h2{margin:3px 3px 9px;font:500 14px Georgia,serif}img{display:block;width:100%;height:auto;border:1px solid #ece5dc;border-radius:9px}@media(max-width:1300px){.evidence{grid-template-columns:repeat(2,minmax(0,1fr))}}@media(max-width:720px){.evidence{grid-template-columns:1fr}}</style></head><body><div class="hero"><h1>Company OS V2.2 · Store-live evidence chain</h1><p>Four truth classes remain separate: the V1 baseline, generated V2.2 direction, deterministic-fixture implementation render, and a real browser render sourced from an archived authority-labelled Harness Store projection.</p></div>${rows}</body></html>\n`;
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
