#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFile(join(root, path), "utf8");
const [router, organization, works, orgSelectors, workSelectors, selection, warRoom, memberFocus, docs] = await Promise.all([
  read("src/company-os/CompanyOsRouter.tsx"),
  read("src/surfaces/AgentTeamOrganization.tsx"),
  read("src/surfaces/TeamWorks.tsx"),
  read("src/model/orgSelectors.ts"),
  read("src/model/teamWorksSelectors.ts"),
  read("src/app/selection.ts"),
  read("src/surfaces/TeamWarRoom.tsx"),
  read("src/surfaces/MemberRuns.tsx"),
  read("src/company-os/docs/BasicDocumentPage.tsx"),
]);

let passed = 0;
let failed = 0;
function check(condition, message) {
  if (condition) {
    passed += 1;
    console.log(`PASS ${message}`);
  } else {
    failed += 1;
    console.error(`FAIL ${message}`);
  }
}

check(router.includes('selection.orgView === "agent-teams"') && router.includes('selection.workView === "team-works"'), "OrgUnit/AgentTeam and WorkItem/Team Work kernels have distinct URL views");
check(router.includes("Execution snapshot") && router.includes("remain separate from the Company Store"), "execution snapshot truth is not labelled as Company Store truth");
check(selection.includes('params.get("orgTeam")') && selection.includes('params.get("teamWork")'), "recursive Team and Team Work focus round-trip through URL selection");

check(organization.includes("data-org-team-depth") && organization.includes("data-durable-status") && organization.includes("data-runtime-state"), "Organization exposes durable/runtime/topology acceptance probes");
check(organization.includes("No root Agent Team yet") && organization.includes("No direct Members") && organization.includes("No child Teams"), "Organization has honest root/member/child empty states");
check(organization.includes("Topology integrity findings") && orgSelectors.includes("would create a cycle") && orgSelectors.includes("not a direct member of parent team"), "Organization reports cycle, missing relation, and Host-parent integrity findings");
check(!organization.includes("canonicalFixture") && !organization.includes("/fixtures/") && !orgSelectors.includes("canonicalFixture"), "recursive Organization does not import or manufacture fixture truth");

check(works.includes('data-team-work-demand="unassigned"') || works.includes('data-team-work-demand={group.id}'), "Team Works renders demand classes as first-class groups");
check(works.includes("Awaiting Host acceptance") && works.includes("Delegated demand unavailable"), "Team Works uses Host acceptance wording and does not infer delegation");
check(workSelectors.includes("source_work_item_ref") && workSelectors.includes("parent_work_id") && workSelectors.includes("team_run_id"), "Team Works aggregation derives from implemented wire fields");
check(!works.includes("canonicalFixture") && !workSelectors.includes("canonicalFixture"), "Team Works aggregate contains no fixture fallback");

check(warRoom.includes("Organization") && warRoom.includes("data-team-child-count") && warRoom.includes("teamWorkId"), "War Room reuses the existing surface with Organization breadcrumb, child Teams, and deep-linked Work");
check(memberFocus.includes("Created Work") && memberFocus.includes("Child Work") && memberFocus.includes("created_by_actor") && memberFocus.includes("parent_work_id"), "Member Focus adds provenance-backed Work lineage without a second focus implementation");
check(docs.includes("Create Work from selection") && docs.includes("Link existing Work") && docs.includes("No linked Work"), "Docs-to-Work handoff slots and honest empty state are visible");
check(docs.includes('data-docs-create-work="unavailable"') && docs.includes("A Document revision never completes Work"), "unconnected Docs handoff transports stay unavailable and preserve lifecycle boundaries");

console.log(`\nrecursive-org-docs-works: ${passed} passed, ${failed} failed`);
if (failed) process.exit(1);
