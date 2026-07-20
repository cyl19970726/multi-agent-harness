#!/usr/bin/env node

import { createHash } from "node:crypto";
import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  indexFixture,
  loadCompanyOsFixture,
  resolveContractRoute,
} from "../fixtures/company-os-trademark-v1/fixture.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const designRoot = resolve(repoRoot, "docs/design/company-os-v1");

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

function byId(items, id) {
  return items.find((item) => item.id === id);
}

function collectTimestamps(value, path = "$", result = []) {
  if (Array.isArray(value)) {
    value.forEach((item, index) => collectTimestamps(item, `${path}[${index}]`, result));
  } else if (value && typeof value === "object") {
    for (const [key, item] of Object.entries(value)) {
      const childPath = `${path}.${key}`;
      if (typeof item === "string" && (key.endsWith("_at") || key === "expires_at")) {
        result.push([childPath, item]);
      } else {
        collectTimestamps(item, childPath, result);
      }
    }
  }
  return result;
}

async function sha256(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

async function main() {
  const { fixture, manifest, sourceSha256 } = await loadCompanyOsFixture();
  const visual = JSON.parse(await readFile(resolve(designRoot, "visual-contract.json"), "utf8"));
  const records = indexFixture(fixture);

  check(sourceSha256 === manifest.authoritative_sha256, "browser fixture is pinned to the authoritative design fixture");
  check(fixture.time_contract.fixture_as_of === manifest.capture_now, "browser clock equals the canonical fixture clock");
  check(collectTimestamps(fixture).every(([, value]) => value.startsWith("2026-07")), "all fixture timestamps stay inside July 2026");

  const sourceDocument = byId(fixture.documents, "document-trademark-application-cn-2026-018");
  const workItem = byId(fixture.work_items, "workitem-trademark-filing-brand-a");
  const assignment = byId(fixture.assignments, "assignment-trademark-agent-trademark-filing-brand-a");
  const approval = byId(fixture.approvals, "approval-trademark-filing-fee-cn-2026-018");
  const commitment = byId(fixture.financial_records, "financial-commitment-trademark-filing-fee-cn-2026-018");

  check(sourceDocument?.title === "Trademark application CN-2026-018", "Document is the canonical origin of the trademark filing");
  check(workItem?.source_document_ref === sourceDocument?.id, "Document creates the linked WorkItem");
  check(assignment?.work_item_ref === workItem?.id && assignment?.assignee_ref === "actor-agent-trademark", "Assignment gives the WorkItem to Trademark Agent");
  check(workItem?.submitted_by_ref === "actor-agent-trademark" && workItem?.status === "waiting_for_approval", "Agent submission moves the WorkItem to human approval");
  check(workItem?.evidence_refs?.length === 2 && workItem.evidence_refs.every((id) => records.has(id)), "Agent submission carries resolvable evidence");
  check(approval?.status === "requested" && approval?.required_approver_refs?.length === 1 && approval.required_approver_refs[0] === "actor-human-brand-owner", "Approval is pending with Brand Owner as the required Human approver");
  check(commitment?.type === "commitment" && commitment?.status === "pending_approval" && commitment?.amount === 3000 && commitment?.currency === "CNY", "Finance contains one ¥3,000 pending Commitment");
  check(commitment?.work_item_ref === workItem?.id && commitment?.source_document_ref === sourceDocument?.id && commitment?.approval_refs?.includes(approval?.id), "Commitment remains linked to Document, WorkItem, and Approval");
  check(workItem?.result_document_ref === sourceDocument?.id && byId(fixture.typed_records, "trademark-application-cn-2026-018")?.updated_at === workItem?.updated_at, "submitted outcome writes back to the originating document/record truth");
  check(fixture.financial_records.every((record) => record.type !== "payment"), "no Payment FinancialRecord exists before approval and settlement");
  check(fixture.negative_assertions?.payment_financial_records?.length === 0 && fixture.negative_assertions?.settlement_evidence?.length === 0, "no settlement evidence is present or implied");

  check(visual.shared_fixture === fixture.fixture_id, "all visual cases use the canonical shared fixture");
  check(visual.cases.length === 12 && new Set(visual.cases.map((item) => item.page)).size === 12, "visual contract contains twelve distinct core pages");
  check(Object.keys(fixture.page_slices).length === 12, "fixture provides one fact slice for every core page");

  const pageNames = new Set(visual.cases.map((item) => item.page));
  check(Object.keys(fixture.page_slices).every((page) => pageNames.has(page)), "every fixture page slice is represented by a visual case");
  check(visual.cases.every((item) => item.viewport.width >= 1440 && item.viewport.height >= 1000), "every core page has a desktop capture contract of at least 1440x1000");

  for (const item of visual.cases) {
    const route = resolveContractRoute(item.route, manifest.route_tokens);
    check(!route.includes("<") && route.startsWith("/?surface="), `${item.page} resolves to a concrete deterministic route`);
    for (const ref of fixture.page_slices[item.page].required_refs) {
      check(records.has(ref), `${item.page} required reference resolves: ${ref}`);
    }
    const expectedPath = resolve(designRoot, item.expected);
    await access(expectedPath);
    if (item.expected_hash) {
      check(`sha256:${await sha256(expectedPath)}` === item.expected_hash, `${item.page} expected design hash matches its manifest`);
    }
  }

  check(manifest.responsive_pages.every((page) => pageNames.has(page)), "all tablet/mobile focus pages are part of the twelve-page contract");
  check(manifest.viewports["tablet-900x1180"]?.width === 900 && manifest.viewports["mobile-390x844"]?.width === 390, "tablet and mobile evidence viewports are pinned");

  console.log(`\nCompany OS trademark acceptance: ${passed} passed, ${failed} failed`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
