#!/usr/bin/env node

import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const closureRoot = resolve(repoRoot, "docs/design/company-os-v3/trademark-native-closure-v1");
const contract = JSON.parse(await readFile(resolve(closureRoot, "visual-contract.json"), "utf8"));
const review = await readFile(resolve(closureRoot, "review.html"), "utf8");
const readme = await readFile(resolve(closureRoot, "README.md"), "utf8");
const matrix = await readFile(resolve(closureRoot, "page-matrix.md"), "utf8");
const index = await readFile(resolve(repoRoot, "docs/design/company-os-v2/visual-index.md"), "utf8");
const truthMatrix = await readFile(resolve(repoRoot, "docs/company-os/implementation-truth-matrix.md"), "utf8");
const historicalReviews = ["review.html", "review-v2.1.html", "review-v2.2.html"];

const failures = [];
const check = (condition, message) => {
  if (condition) console.log(`  PASS  ${message}`);
  else failures.push(message);
};

check(contract.workstream === "trademark-native-closure-v1", "Review uses the current native trademark contract");
check(contract.cases.length === 3 && contract.cases.every((item) => item.priority === "P0"), "Contract contains exactly three P0 review cases");
check(review.includes('data-review-scope="current"') && review.includes(`data-contract-version="${contract.workstream}"`), "Review declares current scope and contract version");
check(readme.includes("](review.html)") && matrix.includes("](review.html)"), "README and page matrix link the canonical Review");
check(index.includes("trademark-native-closure-v1/review.html"), "Visual index points to the canonical current Review");
check(truthMatrix.includes("trademark-native-closure-v1/review.html"), "Product truth matrix points to the canonical current Review");

for (const file of historicalReviews) {
  const html = await readFile(resolve(repoRoot, "docs/design/company-os-v2", file), "utf8");
  check(html.includes('data-review-scope="historical"'), `${file}: declares historical scope`);
  check(html.includes("company-os-v3/trademark-native-closure-v1/review.html"), `${file}: links the current Review`);
}

for (const item of contract.cases) {
  check(item.expected_approval?.status === "approved", `${item.id}: Expected intent is approved`);
  check(item.gates?.product_truth?.status === "pass", `${item.id}: product-truth gate passes`);
  check(item.gates?.visual_fidelity?.status === "pass", `${item.id}: visual-fidelity gate passes`);
  check(item.review?.status === "pass_with_deviations" && item.review.defects?.length === 0, `${item.id}: review status and defects agree`);
  check(review.includes(`id="${item.id}"`), `${item.id}: Review contains the case`);
  for (const field of ["comparison", "overlay"]) {
    await access(resolve(closureRoot, item[field]));
    check(review.includes(item[field]), `${item.id}: Review references the contract ${field}`);
  }
}

check(review.includes("six native WorkItems") && review.includes("24 illustrative"), "Review preserves the Work Expected-versus-Actual count boundary");
check(review.includes("proposed") && review.includes("Active badge would contradict"), "Review preserves the proposed-versus-Active module boundary");
check(review.includes("zero Payment") && review.includes("Commitment"), "Review preserves the Commitment-versus-Payment boundary");

if (failures.length) {
  for (const failure of failures) console.error(`  FAIL  ${failure}`);
  console.error(`\nCompany OS Review consistency: ${failures.length} failed`);
  process.exit(1);
}

console.log("\nCompany OS Review consistency: all checks passed");
