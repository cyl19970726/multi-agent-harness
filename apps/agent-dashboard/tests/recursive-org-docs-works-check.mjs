#!/usr/bin/env node
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

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
  read("src/company-os/docs/DocsV2Surface.tsx"),
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

async function loadFlatSelectors() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "recursive-org-selectors-"));
  try {
    for (const name of ["orgSelectors", "teamWorksSelectors"]) {
      const source = await readFile(join(root, "src", "model", `${name}.ts`), "utf8");
      let output = ts.transpileModule(source, {
        compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
      }).outputText;
      output = output.replace('from "./orgSelectors"', 'from "./orgSelectors.mjs"');
      await writeFile(join(directory, `${name}.mjs`), output, "utf8");
    }
    const org = await import(pathToFileURL(join(directory, "orgSelectors.mjs")).href);
    const works = await import(pathToFileURL(join(directory, "teamWorksSelectors.mjs")).href);
    return { org, works };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

check(router.includes('selection.orgView === "agent-teams"') && router.includes('selection.workView === "team-works"'), "OrgUnit/AgentTeam and Work/Team Work kernels have distinct URL views");
check(router.includes("Execution snapshot") && router.includes("remain separate from the Company Store"), "execution snapshot truth is not labelled as Company Store truth");
check(selection.includes('params.get("orgTeam")') && selection.includes('params.get("teamWork")'), "flat Team and Team Work focus round-trip through URL selection");

check(organization.includes("data-org-team-depth") && organization.includes("data-durable-status") && organization.includes("data-runtime-state"), "Organization exposes durable/runtime/topology acceptance probes");
check(organization.includes("No Agent Team yet") && organization.includes("No direct Members") && organization.includes("No other Agent Teams match"), "Organization has honest flat Team/member/peer empty states");
check(organization.includes("Organization contract findings") && orgSelectors.includes("Host Agent") && !orgSelectors.includes("parent_team_id"), "Organization reports identity findings without recursive topology");
check(!organization.includes("canonicalFixture") && !organization.includes("/fixtures/") && !orgSelectors.includes("canonicalFixture"), "flat Organization does not import or manufacture fixture truth");

check(works.includes('data-team-work-demand="unassigned"') || works.includes('data-team-work-demand={group.id}'), "Team Works renders demand classes as first-class groups");
check(works.includes("Awaiting Host acceptance") && works.includes("WorkDelegation") && !works.includes("Delegated demand unavailable"), "Team Works uses Host acceptance wording and explicit WorkDelegation truth");
check(workSelectors.includes("parent_work_id") && workSelectors.includes("team_run_id") && workSelectors.includes("phase") && workSelectors.includes("condition"), "Team Works aggregation derives from implemented unified wire fields");
check(!works.includes("canonicalFixture") && !workSelectors.includes("canonicalFixture"), "Team Works aggregate contains no fixture fallback");

check(warRoom.includes("Organization") && !warRoom.includes("data-team-child-count") && warRoom.includes("teamWorkId"), "War Room reuses the existing surface with a flat Organization breadcrumb and deep-linked Work");
check(memberFocus.includes("Created Work") && memberFocus.includes("Child Work") && memberFocus.includes("created_by_actor") && memberFocus.includes("parent_work_id"), "Member Focus adds provenance-backed Work lineage without a second focus implementation");
// The Block-era Document Focus handoff slots were retired with BasicDocumentPage;
// document deep links now render store-live through DocsV2Surface, which stays a
// read-only projection renderer (no Work writes, no fixture fallback).
check(docs.includes("data-docs-v2-page") && docs.includes("data-docs-v2-surface") && docs.includes("fetchDocsV2Page") && docs.includes("data-docs-v2-error"), "Docs deep links render through the store-live DocsV2Surface contract with an explicit error state instead of a silent fallback");
check(docs.includes("data-docs-v2-legacy") && !docs.includes("adaptTrademarkDocsFixture") && !docs.includes("company-os-trademark-v1.json"), "DocsV2Surface labels legacy Block-era projections read-only and imports no fixture truth");

const selectors = await loadFlatSelectors();
const durableOnlyRoot = {
  company_os: {
    durable_agent_members: [{
      id: "foundation-lead", name: "Foundation Lead", description: "Root Lead",
      role: "lead", status: "active", created_at: "1", updated_at: "1",
    }],
  },
  members: [],
  teams: [{
    id: "foundation", name: "Company OS Foundation Builders", description: "Foundation",
    mission_id: "mission-foundation", host_agent_id: "foundation-lead",
    node_id: "node-local", status: "active", member_ids: ["foundation-lead"],
  }],
  missions: [{ id: "mission-foundation", title: "Foundation", objective: "Build foundation" }],
  execution_nodes: [{ id: "node-local", display_name: "Local node" }],
  team_runs: [{
    id: "run-foundation", agent_team_id: "foundation", execution_node_id: "node-local",
    project_binding_id: "binding-foundation", status: "running",
    host_actor: { kind: "host", id: "runtime-host", display_name: "Runtime Host" },
    created_at: "2",
  }],
  member_runs: [],
  works: [{
    id: "work-foundation", team_run_id: "run-foundation", title: "Read-model seam",
    context_markdown: "", completion_criteria_markdown: "", phase: "open", condition: "normal", resolution: null,
    owner_member_id: "foundation-lead", claim_mode: "host_assign", eligible_member_ids: [],
    prerequisite_work_ids: [], priority: "normal", created_by_actor: { kind: "host", id: "runtime-host" },
    artifact_refs: [], check_refs: [], version: 1, created_at: "1", updated_at: "2",
  }],
  work_delegations: [],
};
const orgModel = selectors.org.buildAgentTeamOrgModel(durableOnlyRoot);
const rootNode = orgModel.roots[0];
check(
  rootNode?.host?.id === "foundation-lead"
    && rootNode.host.identitySource === "durable"
    && rootNode.members[0]?.name === "Foundation Lead"
    && !rootNode.findings.some((finding) => finding.includes("not present")),
  "durable-only Host Agent resolves as Host/member without a dangling identity finding",
);

const convergingSnapshot = structuredClone(durableOnlyRoot);
convergingSnapshot.members = [{
  id: "foundation-lead", name: "Stale Runtime Label", status: "offline",
  runtime_status: "stopped",
}];
const convergingRoot = selectors.org.buildAgentTeamOrgModel(convergingSnapshot).roots[0];
check(
  convergingRoot?.host?.name === "Foundation Lead"
    && convergingRoot.host.status === "active"
    && convergingRoot.host.identitySource === "durable",
  "durable identity wins when a compatibility AgentMember row has the same id",
);

const worksModel = selectors.works.buildTeamWorksModel(durableOnlyRoot);
check(
  worksModel.rows[0]?.hostId === "foundation-lead"
    && worksModel.rows[0]?.hostLabel === "Foundation Lead"
    && worksModel.facets.hosts[0]?.id === "foundation-lead",
  "Team Works Host facet uses Team.host_agent_id durable authority over TeamRun.host_actor",
);

const delegatedSnapshot = structuredClone(durableOnlyRoot);
delegatedSnapshot.work_delegations = [{
  id: "delegation-1", source_team_id: "source-team", target_team_id: "foundation",
  source_work_ref: { team_run_id: "source-run", work_id: "source-work" },
  target_work_ref: { team_run_id: "run-foundation", work_id: "work-foundation" },
  status: "active", version: 1,
}];
const delegatedWorks = selectors.works.buildTeamWorksModel(delegatedSnapshot);
check(
  delegatedWorks.rows[0]?.demandClass === "delegated"
    && delegatedWorks.rows[0]?.delegationId === "delegation-1"
    && delegatedWorks.counts.delegated === 1,
  "explicit WorkDelegation classifies target Work without naming inference",
);

console.log(`\nflat-org-docs-works: ${passed} passed, ${failed} failed`);
if (failed) process.exit(1);
