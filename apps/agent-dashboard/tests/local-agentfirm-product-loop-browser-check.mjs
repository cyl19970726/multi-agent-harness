#!/usr/bin/env node
import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import {dirname,join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {chromium} from "playwright";
import {createServer} from "vite";

const dashboardRoot=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const fixtureRoot=join(dashboardRoot,"fixtures/wave4-local-agentfirm-v1");
const fixtures=Object.fromEntries(await Promise.all(["company-work","team-workspace","host-console","member-workbench","operator"].map(async name=>[name,JSON.parse(await readFile(join(fixtureRoot,`${name}.json`),"utf8"))])));
const work={work_id:"work-fixture-1",work_revision:3,team_id:"team-fixture-1",mission_id:"mission-fixture-1",owner_actor_ref:{kind:"agent_member",id:"member-fixture-1"},current_member_run_ref:"member-run-fixture-1",phase:"review",condition:"normal",resolution:null,priority:"high",module_refs:["integration-plan"],gate_summary:{required:1,passed:1,failed:0,pending:0,waived:0,stale:0},latest_report_ref:"report-1",latest_finding_refs:["finding-1"],latest_failure_ref:null,delivery_summary:{queued:0,claimed:0,provider_received:1,failed:0,expired:0,invalidated:0,recovery_class:"none"},runtime_summary:{state:"reviewing",generation:2,freshness:"current"},workspace_summary:{binding_id:"workspace-1",lifecycle:"ready",safety:"safe"},delegation_summary:{incoming:0,outgoing:0,attention:false},updated_at:"2026-08-10T00:00:00Z"};
fixtures["company-work"].data.items=[work];fixtures["company-work"].data.page.item_count=1;
fixtures["team-workspace"].data.team={team_id:"team-fixture-1",team_revision:1,mission_id:"mission-fixture-1",node_id:"node-fixture-1",placement_generation:1,status:"active"};fixtures["team-workspace"].data.works=[work];fixtures["team-workspace"].data.members=[{agent_member_ref:{kind:"agent_member",id:"member-fixture-1"},capacity:"available",current_member_run_ref:"member-run-fixture-1"}];
fixtures["host-console"].data.work_queues={ready:[],unassigned:[],blocked:[],review:[work],integration:[work]};
fixtures["member-workbench"].data.agent_member={id:"member-fixture-1"};fixtures["member-workbench"].data.member_run={id:"member-run-fixture-1"};fixtures["member-workbench"].data.my_works=[work];
fixtures.operator.data.node={node_id:"node-fixture-1",daemon_generation:4,status:"active"};fixtures.operator.data.build={build_sha:"fbc401646f66b69a0269622c489441cfe643b54f",protocol_version:"agentfirm.local.v1",schema_version:"agentfirm.role_views.v1"};fixtures.operator.data.delivery_backlog={depth:0,oldest_age_ms:null,recovery_required:false};

const vite=await createServer({configFile:join(dashboardRoot,"vite.config.ts"),server:{host:"127.0.0.1",port:0},logLevel:"silent"});await vite.listen();const base=`http://127.0.0.1:${vite.httpServer.address().port}`;const browser=await chromium.launch({headless:true});
try{
 for(const viewport of [{width:1440,height:900},{width:390,height:844}]){
  const page=await browser.newPage({viewport});await page.addInitScript(()=>{window.__AGENTFIRM_BOOTSTRAP__={capabilityToken:"fixture-token"}});
  await page.route("**/v1/**",async route=>{const url=new URL(route.request().url());let body={};if(url.pathname==="/v1/projects")body={projects:[{id:"fixture-project",name:"Fixture",is_current:true}]};else if(url.pathname==="/v1/spaces")body={spaces:[{id:"fixture-space",name:"Fixture",is_current:true}]};else if(url.pathname==="/v1/companies")body={companies:[]};else if(url.pathname==="/v1/snapshot")body={generated_at:"2026-08-10T00:00:00Z",teams:[{id:"team-fixture-1",name:"Fixture Team",mission_id:"mission-fixture-1",node_id:"node-fixture-1"}],team_runs:[],execution_nodes:[{id:"node-fixture-1"}],company_os:{}};else if(url.pathname==="/v1/workflows")body={workflows:[]};else if(url.pathname==="/v1/views/company-work")body=fixtures["company-work"];else if(url.pathname.includes("team-workspace"))body=fixtures["team-workspace"];else if(url.pathname.includes("host-console"))body=fixtures["host-console"];else if(url.pathname.includes("member-workbench"))body=fixtures["member-workbench"];else if(url.pathname.includes("operator"))body=fixtures.operator;else if(url.pathname==="/v1/events")return route.fulfill({status:200,contentType:"text/event-stream",body:"event: projection_invalidation\ndata: {}\n\n"});return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(body)});});
  await page.goto(`${base}/?space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Company Work"}).waitFor();assert.equal(await page.getByText("work-fixture-1").count()>0,true);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"Company view overflows");
  await page.goto(`${base}/?surface=team&team=team-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"team-fixture-1"}).waitFor();await page.getByRole("button",{name:"Open Host Console"}).click();await page.getByRole("heading",{name:"Host Console"}).waitFor();
  await page.goto(`${base}/?surface=team&memberRun=member-run-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Member Workbench"}).waitFor();
  await page.goto(`${base}/?surface=operator&node=node-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Operator View"}).waitFor();assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"Operator view overflows");await page.close();
 }
 console.log("local AgentFirm product loop browser check: PASS");
}finally{await browser.close();await vite.close();}
