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

for (const retired of ["TeamWarRoom", "teamSelectors", "/v1/snapshot", "api/actions"]) {
  assert.equal(active.includes(retired), false, `active RoleView composition imports retired ${retired}`);
}
assert.equal(/catch\s*\([^)]*\)\s*=>\s*\(\s*\{\s*ok\s*:\s*true/.test(active), false, "RoleView surface contains catch-all fake success");
assert.equal(/sender(_actor_ref|_runtime_id)?\s*:/.test(sources["src/model/roleViews.ts"].split("prepareRoleAction")[1].split("export function roleActionRoute")[0]), false, "browser action payload authors sender identity");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"], /"works" \| "activity" \| "members"/, "semantic Team tabs are missing");
assert.match(sources["src/components/workbench/team/TeamWorksBoard.tsx"], /data-testid="role-view-work-sheet"/, "responsive selected Work sheet is missing");
assert.match(sources["src/surfaces/HostConsole.tsx"], /view\.data\.team_supervisor/, "Host-only supervisor truth is not rendered");
assert.match(sources["src/surfaces/HostConsole.tsx"], /view\.data\.host_inbox/, "Host-only Lead Inbox is not rendered");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"], /team\.viewer_role === "host"/, "member Team views expose an unscoped Host Console entry");
assert.match(workbenchShell, /selection\.teamId && selection\.teamMode/, "selected Team member context is routed into exact-self MemberWorkbench");
assert.match(sources["src/components/workbench/team/TeamMessageComposer.tsx"], /prepareRoleAction/, "compact composer bypasses closed Role Actions");
assert.match(sources["src/surfaces/TeamWorkspace.tsx"], /Last authoritative view|last authoritative view|last-good truth|Showing the last authoritative view/, "last-good refresh state is missing");
assert.match(sources["src/model/roleViews.ts"], /runtime_fabric:RuntimeFabricSummary/g, "RoleView runtime fabric types are missing");

console.log("Team RoleView War Room source check: PASS");
