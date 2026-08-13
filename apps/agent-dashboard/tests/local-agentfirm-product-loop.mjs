import assert from "node:assert/strict";import fs from "node:fs";
const shell=fs.readFileSync("apps/agent-dashboard/src/app/WorkbenchShell.tsx","utf8");
for(const surface of ["CompanyWorkIndex","TeamWorkspace","HostConsole","AgentConversationWorkspace","MemberWorkbench","OperatorView"])assert.ok(fs.existsSync(`apps/agent-dashboard/src/surfaces/${surface}.tsx`),`${surface} missing`);
for(const marker of ["<CompanyWorkIndex","<TeamWorkspace","<AgentConversationWorkspace","<OperatorView"])assert.ok(shell.includes(marker),`route missing ${marker}`);
assert.ok(!shell.includes("<MemberWorkbench"),"retired MemberWorkbench remains an active route instead of the unified Agent Workspace");
const model=fs.readFileSync("apps/agent-dashboard/src/model/roleViews.ts","utf8");assert.ok(model.includes("agentfirm.role_views.v1"));assert.ok(model.includes("Unsupported RoleView schema"));
const migration=JSON.parse(fs.readFileSync("schemas/role-views/surface-migration.v1.json","utf8"));const packageScripts=JSON.stringify(JSON.parse(fs.readFileSync("package.json","utf8")).scripts);for(const retired of migration.retired_tests){assert.ok(fs.existsSync(retired.path),`historical test missing without delete disposition: ${retired.path}`);assert.ok(!packageScripts.includes(retired.path.split("/").at(-1)),`retired test remains active: ${retired.path}`);assert.ok(retired.replacement,`retired test lacks canonical replacement: ${retired.path}`)}
const combined=["CompanyWorkIndex","TeamWorkspace","HostConsole","AgentConversationWorkspace","OperatorView","RoleViewPrimitives"].map(name=>fs.readFileSync(`apps/agent-dashboard/src/surfaces/${name}.tsx`,"utf8")).join("\n");
for(const state of ["loading","error","freshness","disabled"])assert.ok(combined.toLowerCase().includes(state),`UI state ${state} not represented`);
assert.doesNotMatch(combined,/chain.of.thought|sub.agent commands|internal checklist/i);
console.log("local AgentFirm product loop check: PASS");
