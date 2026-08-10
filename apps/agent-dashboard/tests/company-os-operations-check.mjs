#!/usr/bin/env node

import { access, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const operations = resolve(root, "src/company-os/operations");
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

async function absent(path) {
  try {
    await access(path);
    return false;
  } catch {
    return true;
  }
}

async function main() {
  const [pages, adapterSource, types, workPage, router, approvalAction] = await Promise.all([
    readFile(resolve(operations, "pages.tsx"), "utf8"),
    readFile(resolve(operations, "fixture.ts"), "utf8"),
    readFile(resolve(operations, "types.ts"), "utf8"),
    readFile(resolve(root, "src/company-os/work/WorkOperatingPage.tsx"), "utf8"),
    readFile(resolve(root, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
    readFile(resolve(operations, "approvalAction.ts"), "utf8"),
  ]);

  check(await absent(resolve(operations, "workItemAction.ts")), "retired Company WorkItem action builder is physically absent");
  check(await absent(resolve(root, "src/company-os/work/projection.ts")), "retired Company WorkItem projection helper is physically absent");
  check(!pages.includes("WorkItem") && !types.includes("WorkItem") && !adapterSource.includes("work_items"), "operations production surface has no Company WorkItem bridge contract");
  check(!types.includes("AssignmentView") && !types.includes("WorkExecutionChain"), "duplicate Company Assignment and execution-chain view models are absent");
  check(router.includes("<WorkOperatingPage source={resolved.value} />"), "Company Work route renders the unified read-only operating page");
  check(workPage.includes("company_work_aggregate") && workPage.includes("data-company-work-authority") && workPage.includes("data-company-work-read-only"), "Company Work page exposes aggregation authority and read-only provenance");
  check(workPage.includes("phase") && workPage.includes("condition") && workPage.includes("resolution"), "Company Work page renders the independent TeamWork lifecycle axes");
  check(workPage.includes("records(aggregate.works)") && !workPage.includes("work_items"), "Company Work page reads native works without a fallback task ledger");
  check(approvalAction.includes('command_name: "approval.decide"'), "independent Human approval action remains available");
  check(types.includes("linkedWork") && types.includes("AgentMemberExecutionAssignment"), "operations view links raw TeamWork and explicit AgentMember execution participation");

  const ts = (await import("typescript")).default;
  const directory = await mkdtemp(resolve(tmpdir(), "company-os-operations-"));
  const target = resolve(directory, "fixture.mjs");
  await writeFile(target, ts.transpileModule(adapterSource, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  const adapter = await import(pathToFileURL(target).href);
  const source = {
    actors: [
      { id: "human-1", actor_type: "human", display_name: "Human Owner" },
      { id: "agent-membership-1", actor_type: "agent_membership", display_name: "Docs Agent", actor: { agent_member_ref: "member-1" } },
    ],
    organization: {
      org_units: [{ id: "unit-1", name: "Operations", parent_unit_id: null, human_lead_actor_ref: { actor_type: "human", actor_id: "human-1" } }],
      memberships: [{ id: "membership-1", org_unit_id: "unit-1", actor_ref: { actor_type: "agent_membership", actor_id: "agent-membership-1" }, membership_role: "member" }],
    },
    work: {
      authority: "team_work",
      read_only: true,
      works: [{ id: "work-1", team_id: "team-1", team_run_id: "run-1", title: "Repair docs", phase: "review", condition: "normal", resolution: null }],
    },
    membership_projections: [{
      id: "member-assignment-1",
      agent_member_id: "member-1",
      source_kind: "agent_team_work",
      work_id: "work-1",
      team_run_id: "run-1",
      member_run_id: "member-run-1",
      title: "Repair docs",
      role: "worker",
      status: "review",
      assigned_at: "2026-08-09T00:00:00Z",
      native_session: { provider: "codex" },
    }],
  };
  const adapted = adapter.adaptTrademarkOperationsProjection(source);
  check(adapted.linkedWork?.id === "work-1" && adapted.linkedWork.phase === "review", "adapter preserves the exact native Work id and phase");
  check(adapted.organization.rootUnitIds.join(",") === "unit-1" && adapted.organization.memberships[0].actorId === "agent-membership-1", "organization projection preserves explicit roots and memberships");
  check(adapted.membershipProjections[0].workId === "work-1" && adapted.membershipProjections[0].memberRunId === "member-run-1", "AgentMember participation preserves exact Work and MemberRun identities");
  check(!JSON.stringify(adapter.adaptTrademarkOperationsProjection({})).includes("Repair docs"), "empty projection remains empty without fixture fallback");

  await rm(directory, { recursive: true, force: true });
  console.log(`\nCompany OS operations checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
