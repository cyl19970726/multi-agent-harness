#!/usr/bin/env node

import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const designRoot = join(repoRoot, "docs/design/company-os-v2");
const actualRoot = join(designRoot, "actual");

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
  const suppliedCapture = argument("--capture-manifest");
  if (!suppliedCapture) throw new Error("--capture-manifest is required");
  const capturePath = resolve(suppliedCapture);
  const [capture, visual] = await Promise.all([
    readFile(capturePath, "utf8").then(JSON.parse),
    readFile(join(designRoot, "visual-contract.json"), "utf8").then(JSON.parse),
  ]);
  if (capture.status !== "passed" || capture.data_mode !== "deterministic-fixture" || capture.results?.length !== 6) {
    throw new Error("V2 materialization requires one passing six-page deterministic fixture capture");
  }
  const revision = visual.design_revisions.find((entry) => entry.version === "2.2");
  if (!revision) throw new Error("V2.2 design revision is missing");
  await rm(actualRoot, { recursive: true, force: true });
  await mkdir(actualRoot, { recursive: true });
  const comparisons = [];
  for (const result of capture.results) {
    const design = revision.cases.find((entry) => entry.case_id === result.id);
    const contractCase = visual.cases.find((entry) => entry.id === result.id);
    if (!design || !contractCase) throw new Error(`missing visual contract entry for ${result.id}`);
    const source = join(repoRoot, result.file);
    const destination = join(actualRoot, basename(source));
    await cp(source, destination);
    const expected = join(designRoot, design.expected);
    const baseline = resolve(designRoot, contractCase.baseline);
    comparisons.push({
      case_id: result.id,
      route: result.route,
      baseline: { file: relative(designRoot, baseline), sha256: await sha256(baseline), truth: "V1 browser actual" },
      expected: { file: design.expected, sha256: await sha256(expected), truth: "V2.2 generated design direction" },
      actual: { file: relative(designRoot, destination), sha256: await sha256(destination), truth: "V2.2 browser implementation with deterministic fixture" },
    });
  }
  const manifest = {
    contract: "company-os-v2-2-visual-comparison-v1",
    source_capture: relative(repoRoot, capturePath),
    data_mode: capture.data_mode,
    fixture: capture.fixture,
    status: "implementation_review_pending",
    counts: { current_before: 6, expected: 6, actual: 6 },
    truth: {
      current_before: "V1 browser implementation baseline",
      expected: "V2.2 generated visual target; visual direction only",
      actual: "real browser render of this branch using deterministic Company OS fixture; not Store-live evidence",
    },
    comparisons,
  };
  await writeFile(join(designRoot, "comparison-manifest-v2.2.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(join(designRoot, "expected-vs-actual-v2.2.html"), gallery(manifest));
  console.log(JSON.stringify({ status: "materialized", gallery: join(designRoot, "expected-vs-actual-v2.2.html"), actual_count: 6 }, null, 2));
}

function gallery(manifest) {
  const rows = manifest.comparisons.map((entry) => `<section><header><div><span>${escapeHtml(entry.case_id)}</span><code>${escapeHtml(entry.route)}</code></div></header><div class="triptych"><article><h2>Current before · V1 Actual</h2><a href="${escapeHtml(entry.baseline.file)}"><img src="${escapeHtml(entry.baseline.file)}" alt="V1 baseline"></a></article><article><h2>Expected · V2.2 direction</h2><a href="${escapeHtml(entry.expected.file)}"><img src="${escapeHtml(entry.expected.file)}" alt="V2.2 expected"></a></article><article><h2>Actual · V2.2 implementation</h2><a href="${escapeHtml(entry.actual.file)}"><img src="${escapeHtml(entry.actual.file)}" alt="V2.2 actual"></a></article></div></section>`).join("");
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Company OS V2.2 · Current / Expected / Actual</title><style>:root{font-family:Geist,Inter,system-ui,sans-serif;color:#2c2926;background:#f7f3ed}*{box-sizing:border-box}body{margin:0}.hero{padding:46px clamp(20px,5vw,72px);background:#2d2925;color:#fff9ef}.hero p{max-width:900px;color:#d8cec2;line-height:1.65}section{max-width:1900px;margin:36px auto;padding:0 clamp(14px,2vw,32px)}header{margin-bottom:12px}header span{font-size:15px;font-weight:700}header code{display:block;margin-top:5px;color:#84786c}.triptych{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:14px}article{min-width:0;border:1px solid #ddd3c7;border-radius:16px;background:#fffdf9;padding:10px;box-shadow:0 12px 38px rgba(67,50,33,.07)}h2{margin:3px 3px 10px;font:500 15px Georgia,serif}img{display:block;width:100%;height:auto;border:1px solid #ece5dc;border-radius:10px}@media(max-width:1050px){.triptych{grid-template-columns:1fr}}</style></head><body><div class="hero"><h1>Company OS V2.2 · visual evidence chain</h1><p>Current Before, Expected, and Actual are deliberately separate. Actual is a browser render backed by the deterministic Company OS fixture and is design implementation evidence—not Store-live business evidence.</p></div>${rows}</body></html>\n`;
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
