#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const activeFiles = [
  "src/model/roleViews.ts",
  "src/surfaces/TeamWorkspace.tsx",
  "src/surfaces/HostConsole.tsx",
  "src/surfaces/HostActivityComposer.tsx",
  "src/surfaces/AgentConversationWorkspace.tsx",
  "src/surfaces/RoleActionPanel.tsx",
  "src/surfaces/RoleViewPrimitives.tsx",
  "src/components/workbench/team/TeamCapacityStrip.tsx",
  "src/components/workbench/team/TeamWorksBoard.tsx",
  "src/components/workbench/team/TeamMembersCapacity.tsx",
  "src/components/workbench/team/TeamConversation.tsx",
  "src/components/workbench/team/TeamMessageComposer.tsx",
];
const sources = Object.fromEntries(await Promise.all(activeFiles.map(async (file) => [file, await readFile(join(root,file),"utf8")])));
const active = Object.values(sources).join("\n");
const workbenchShell = await readFile(join(root,"src/app/WorkbenchShell.tsx"),"utf8");
const styles = await readFile(join(root,"src/index.css"),"utf8");

for (const retired of ["TeamWarRoom", "teamSelectors", "/v1/snapshot", "api/actions"]) {
  assert.equal(active.includes(retired), false, `active RoleView composition imports retired ${retired}`);
}
assert.equal(/catch\s*\([^)]*\)\s*=>\s*\(\s*\{\s*ok\s*:\s*true/.test(active), false, "RoleView surface contains catch-all fake success");
assert.equal(/sender(_actor_ref|_runtime_id)?\s*:/.test(sources["src/model/roleViews.ts"].split("prepareRoleAction")[1].split("export function roleActionRoute")[0]), false, "browser action payload authors sender identity");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"], /"works" \| "activity" \| "members"/, "semantic Team tabs are missing");
assert.match(sources["src/components/workbench/team/TeamWorksBoard.tsx"], /data-testid="role-view-work-sheet"/, "responsive selected Work sheet is missing");
assert.match(sources["src/components/workbench/team/TeamWorksBoard.tsx"], /event\.key !== "Tab"/, "Work sheet focus trap is missing");
assert.match(sources["src/surfaces/HostConsole.tsx"], /view\.data\.team_supervisor/, "Host-only supervisor truth is not rendered");
assert.match(sources["src/surfaces/HostConsole.tsx"], /view\.data\.host_inbox/, "Host-only Lead Inbox is not rendered");
assert.match(sources["src/surfaces/HostConsole.tsx"], /Work authority/, "HostConsole does not separate Work authority from TeamRun controls");
assert.match(sources["src/surfaces/HostConsole.tsx"], /groupActionsByTarget\(memberRunActions\)/, "MemberRun controls are not grouped by exact target");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"], /team\.viewer_role === "host"/, "member Team views expose an unscoped Host Console entry");
assert.match(workbenchShell, /selection\.teamId \?/, "selected Team context is not retained while opening Agent Conversation");
const teamRouteSource=workbenchShell.split('case "team":')[1].split('case "operator":')[0];
assert.doesNotMatch(teamRouteSource,/model\.snapshot\.team_runs/,"active Team route still joins global snapshot TeamRun rows");
assert.doesNotMatch(teamRouteSource,/TeamWorkspace key=/,"snapshot generation remounts and destroys last-good TeamWorkspace state");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"],/<HostConsole\s+embedded/,"Host-only truth replaces instead of composing the shared War Room");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"],/<HostActivityComposer /,"Activity lacks an authenticated same-surface composer");
assert.doesNotMatch(sources["src/components/workbench/team/TeamWorksBoard.tsx"],/id: "assigned"/,"Assigned is incorrectly modeled as a canonical lifecycle lane");
assert.match(sources["src/components/workbench/team/TeamWorksBoard.tsx"],/label: "Active"/,"active Work phase is not explicit");
assert.match(sources["src/components/workbench/team/TeamWorksBoard.tsx"],/label: "Closed"/,"closed Work phase is not explicit");
assert.match(sources["src/surfaces/AgentConversationWorkspace.tsx"],/fetchNativeMemberActivity/,"Agent conversation does not read provider-native activity on demand");
assert.match(sources["src/surfaces/AgentConversationWorkspace.tsx"],/Host execution is not fabricated as a MemberRun/,"Host conversation fabricates MemberRun execution truth");
assert.match(sources["src/surfaces/AgentConversationWorkspace.tsx"],/Ordinary messages|Messages, runtime controls and Work transitions remain separate/,"conversation UI collapses Message and control authority");
assert.match(sources["src/surfaces/AgentConversationWorkspace.tsx"],/role="dialog" aria-modal="true"/,"responsive Agent/context sheets are missing");
assert.match(sources["src/surfaces/AgentConversationWorkspace.tsx"],/selection\.teamConversation === "host"/,"an explicit Host Member is mistaken for the Host conversation target");
assert.match(sources["src/surfaces/AgentConversationWorkspace.tsx"],/filter\(\(work\) => work\.current_member_run_ref === selectedMemberRunId\)/,"conversation does not partition all exact MemberRun-bound Works");
assert.doesNotMatch(sources["src/surfaces/AgentConversationWorkspace.tsx"],/Current execution Work/,"conversation guesses one current Work without a server projection");
assert.match(sources["src/surfaces/RoleActionPanel.tsx"],/aria-describedby=\{reason \? reasonId/,"disabled Role Actions do not expose visible accessible reasons");
assert.match(sources["src/components/workbench/team/TeamMembersCapacity.tsx"],/<Avatar /,"mature member portraits are not reused");
assert.match(sources["src/components/workbench/team/TeamMembersCapacity.tsx"],/org \{member\.organization_status\}/,"AgentMember organization status is not exposed separately");
assert.doesNotMatch(sources["src/components/workbench/team/TeamMembersCapacity.tsx"],/Workspace \/ capacity/,"Members roster claims unprojected Workspace truth");
assert.match(sources["src/components/workbench/team/TeamMessageComposer.tsx"], /prepareRoleAction/, "compact composer bypasses closed Role Actions");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"], /Last authoritative view|last authoritative view|last-good truth|Showing the last authoritative view/, "last-good refresh state is missing");
assert.match(sources["src/model/roleViews.ts"], /runtime_fabric:RuntimeFabricSummary/g, "RoleView runtime fabric types are missing");
assert.match(styles, /\.agent-team-surface :where\(button, a, input, textarea, select, summary\):focus-visible/, "Agent Team focus-visible contract is missing");
assert.match(styles, /prefers-reduced-motion: reduce[\s\S]*\.agent-team-surface \.animate-spin/, "Agent Team pending indicators ignore reduced motion");
assert.match(styles, /prefers-reduced-motion: no-preference[\s\S]*agent-team-sheet-enter/, "responsive sheet motion is not explicitly preference-gated");

console.log("Team RoleView War Room source check: PASS");
