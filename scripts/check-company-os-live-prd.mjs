import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const docsRoot = path.join(repositoryRoot, "docs", "company-os");
const htmlPath = path.join(docsRoot, "live-prd.html");
const cssPath = path.join(docsRoot, "live-prd.css");
const scriptPath = path.join(docsRoot, "live-prd.js");
const contractPath = path.join(repositoryRoot, "docs", "design", "company-os-v3", "live-prd-v1", "visual-contract.json");

const [html, css, script, contractSource] = await Promise.all([
  readFile(htmlPath, "utf8"),
  readFile(cssPath, "utf8"),
  readFile(scriptPath, "utf8"),
  readFile(contractPath, "utf8"),
]);
const contract = JSON.parse(contractSource);

for (const view of ["overview", "journey", "architecture"]) {
  assert.match(html, new RegExp(`data-view-panel=["']${view}["']`), `missing ${view} view`);
  assert.ok(contract.cases.some((item) => item.route.endsWith(`?view=${view}`)), `visual contract missing ${view}`);
}

for (const line of ["Company governance", "Brand & IP", "Content & media", "Product & development", "Finance & admin"]) {
  assert.ok(script.includes(line), `missing business line: ${line}`);
}

for (const requiredTruth of ["Actual", "Expected", "Planned"]) {
  assert.ok(html.includes(requiredTruth), `missing truth label: ${requiredTruth}`);
}

for (const invariant of [
  "This is not a Task Graph",
  "Commitment 不等于 Payment",
  "Chat 与 thinking 不是公司真相",
  "执行内部结构在本报告中不展开",
]) {
  assert.ok(html.includes(invariant) || script.includes(invariant), `missing boundary statement: ${invariant}`);
}

assert.ok(css.includes("@media (max-width: 980px)"), "missing tablet layout contract");
assert.ok(css.includes("@media (max-width: 720px)"), "missing mobile layout contract");
assert.ok(css.includes("prefers-reduced-motion"), "missing reduced-motion support");

const localAssets = new Set();
for (const source of [html, script]) {
  for (const match of source.matchAll(/(?:src=|data-image-src=)["']([^"']+)["']/g)) {
    if (!match[1].startsWith("http") && !match[1].startsWith("#") && !match[1].includes("${") && match[1]) localAssets.add(match[1]);
  }
  for (const match of source.matchAll(/asset\(["']([^"']+)["']\)/g)) localAssets.add(`../design/${match[1]}`);
}

for (const relativeAsset of localAssets) {
  await access(path.resolve(docsRoot, relativeAsset));
}

for (const item of contract.cases) {
  assert.equal(item.priority, "P0", `${item.id} must remain P0`);
  assert.equal(item.expected_approval.status, "approved", `${item.id} Expected is not approved`);
  await access(path.resolve(path.dirname(contractPath), item.expected));
  await access(path.resolve(path.dirname(contractPath), item.design_spec));
  await access(path.resolve(path.dirname(contractPath), item.asset_inventory));
}

console.log(`Company OS Live PRD contract check passed (${contract.cases.length} P0 views, ${localAssets.size} local assets).`);
