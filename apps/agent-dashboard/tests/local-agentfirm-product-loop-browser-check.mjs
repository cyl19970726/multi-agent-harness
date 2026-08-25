#!/usr/bin/env node
import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import {dirname,join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {chromium} from "playwright";
import {createServer} from "vite";

const dashboardRoot=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const fixtureRoot=join(dashboardRoot,"fixtures/wave4-local-agentfirm-v1");
const fixtures=Object.fromEntries(await Promise.all(["global-work","team-workspace","host-console","member-workbench","agent-workspace","operator"].map(async name=>[name,JSON.parse(await readFile(join(fixtureRoot,`${name}.json`),"utf8"))])));
const work={work_id:"work-fixture-1",work_revision:3,team_id:"team-fixture-1",mission_id:"mission-fixture-1",accountable_team_id:"team-fixture-1",assignee_membership_id:null,assignee_kind:"member",assignee_ref:{kind:"member",membership_id:null,membership_state:null,agent_member_id:"member-fixture-1",display_name:"Fixture Member"},migration_state:"canonical",title:"Fixture Work",context_markdown:"RoleView fixture context.",completion_criteria_markdown:"Closed semantic action succeeds.",claim_mode:"host_assign",eligible_member_ids:["member-fixture-1"],prerequisite_work_ids:[],successor_work_ids:[],readiness:{state:"not_claimable",reason_codes:["fixture_state"],unsatisfied_prerequisite_work_ids:[],failed_or_cancelled_prerequisite_work_ids:[]},blocker_reason:null,result_summary:null,artifact_refs:[],check_refs:[],latest_event:null,owner_actor_ref:{kind:"agent_member",id:"member-fixture-1"},current_member_run_ref:"member-run-fixture-1",phase:"review",condition:"normal",resolution:null,priority:"high",module_refs:["integration-plan"],gate_summary:{required:1,passed:1,failed:0,pending:0,waived:0,stale:0},latest_report_ref:"report-1",latest_finding_refs:["finding-1"],latest_failure_ref:null,delivery_summary:{queued:0,claimed:0,provider_received:1,failed:0,expired:0,invalidated:0,recovery_class:"none"},runtime_summary:{state:"reviewing",generation:2,freshness:"current"},workspace_summary:{binding_id:"workspace-1",lifecycle:"ready",safety:"safe"},delegation_summary:{incoming:0,outgoing:0,attention:false},updated_at:"2026-08-10T00:00:00Z"};
fixtures["global-work"].data.items=[work];fixtures["global-work"].data.page.item_count=1;
fixtures["team-workspace"].data.team={team_id:"team-fixture-1",display_name:"Fixture Team",team_revision:1,mission_id:"mission-fixture-1",host_agent_id:"member-fixture-1",viewer_role:"host",node_id:"node-fixture-1",placement_generation:1,status:"active",latest_run:{id:"run-fixture-1",status:"running",previous_run_id:null,execution_node_id:"node-fixture-1",project_binding_id:"fixture-project",execution_root:"/fixture",created_at:"2026-08-10T00:00:00Z",completed_at:null}};fixtures["team-workspace"].data.works=[work];fixtures["team-workspace"].data.members=[{agent_member_ref:{kind:"agent_member",id:"member-fixture-1"},display_name:"Fixture Member",role:"worker",organization_status:"active",coordination_status:"active",provider:"codex",model:"gpt-5",native_session_health:"available",capacity:"available",current_member_run_ref:"member-run-fixture-1",runtime_state:"idle",runtime_generation:1,queued_work_count:0,active_work_count:0,review_work_count:1,blocked_work_count:0,latest_action:null}];
fixtures["host-console"].data.work_queues={ready:[],unassigned:[],blocked:[],review:[work],integration:[work]};
fixtures["host-console"].allowed_actions=[{kind:"create_work",target_ref:{kind:"team_run",id:"run-fixture-1"},required_version:0,disabled_reason:null}];
fixtures["member-workbench"].data.agent_member={id:"member-fixture-1"};fixtures["member-workbench"].data.member_run={id:"member-run-fixture-1"};fixtures["member-workbench"].data.my_works=[work];
fixtures.operator.data.node={node_id:"node-fixture-1",daemon_generation:4,status:"active"};fixtures.operator.data.build={build_sha:"fbc401646f66b69a0269622c489441cfe643b54f",protocol_version:"agentfirm.local.v1",schema_version:"agentfirm.role_views.v1"};fixtures.operator.data.delivery_backlog={depth:0,oldest_age_ms:null,recovery_required:false};

const vite=await createServer({configFile:join(dashboardRoot,"vite.config.ts"),server:{host:"127.0.0.1",port:0},logLevel:"silent"});await vite.listen();const base=`http://127.0.0.1:${vite.httpServer.address().port}`;const browser=await chromium.launch({headless:true});
try{
 for(const viewport of [{width:1440,height:900},{width:390,height:844}]){
  const page=await browser.newPage({viewport});
  page.on("pageerror",error=>console.error("browser page error:",error));
  page.on("console",message=>{if(message.type()==="error")console.error("browser console error:",message.text())});
  page.on("response",async response=>{if(response.status()>=500)console.error("browser response error:",response.status(),response.url(),await response.text())});
  await page.addInitScript(()=>{
    window.__AGENTFIRM_BOOTSTRAP__={capabilityToken:"fixture-token"};
    class LiveFixtureEventSource{
      constructor(url){this.url=url;this.timers=[]}
      addEventListener(kind,listener){if(kind==="snapshot")for(const delay of [0,100,500])this.timers.push(setTimeout(()=>listener(new MessageEvent("snapshot",{data:JSON.stringify({generated_at:"2026-08-10T00:00:00Z",execution_space_id:"fixture-space",stream_epoch:"fixture-live-1"})})),delay))}
      close(){for(const timer of this.timers)clearTimeout(timer)}
    }
    Object.defineProperty(window,"EventSource",{value:LiveFixtureEventSource,configurable:true});
  });
  let actionExecuted=false,capabilityMismatch=false;
  await page.route("**/v1/**",async route=>{
    const request=route.request();const url=new URL(request.url());let body={};
    if(request.method()==="POST"){
      assert.equal(url.pathname,"/v1/agentfirm/team-runs/run-fixture-1/works","browser may call only the closed semantic endpoint");
      assert.equal(request.headers()["x-agentfirm-token"],"fixture-token");
      assert.equal(request.headers()["if-match"],"0");
      assert.ok(request.headers()["idempotency-key"]);
      const intent=request.postDataJSON();assert.deepEqual(Object.keys(intent).sort(),["action","claim_mode","completion_criteria_markdown","context_markdown","priority","title","work_id"].sort());
      assert.equal(intent.action,"create_work");actionExecuted=true;
      return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify({ok:true,projection:{id:intent.work_id},event_id:"event-browser-1",resulting_version:1,store_sequence:8,replayed:false})});
    }
    if(url.pathname==="/v1/meta")body={schema_version:capabilityMismatch?"agentfirm.role_views.v999":"agentfirm.role_views.v1",protocol_version:"agentfirm-member-trust/1",action_manifest_version:"agentfirm.role_actions.v1",capability_auth:"x-agentfirm-token",build_sha:"fbc401646f66b69a0269622c489441cfe643b54f"};
    else if(url.pathname==="/v1/projects")body={projects:[{id:"fixture-project",name:"Fixture",is_current:true}]};
    else if(url.pathname==="/v1/spaces")body={spaces:[{id:"fixture-space",name:"Fixture",is_current:true}]};
    else if(url.pathname==="/v1/companies")body={companies:[]};
    else if(url.pathname==="/v1/snapshot"||url.pathname==="/v1/team-runs/run-fixture-1/snapshot")body={generated_at:"2026-08-10T00:00:00Z",teams:[{id:"team-fixture-1",name:"Fixture Team",mission_id:"mission-fixture-1",node_id:"node-fixture-1"}],team_runs:[{id:"run-fixture-1",agent_team_id:"team-fixture-1"}],execution_nodes:[{id:"node-fixture-1"}],company_os:{}};
    else if(url.pathname==="/v1/views/global-work")body=fixtures["global-work"];
    else if(url.pathname==="/v1/views/viewer-context")body={view_kind:"viewer_context",schema_version:"agentfirm.role_views.v1",source_execution_space_id:"fixture-space",source_store_identity:"local-product-loop-fixture-store",as_of_event_sequence:1,generated_at:"2026-08-10T00:00:00Z",freshness:"current",data:{viewer_actor_ref:{kind:"agent_member",id:"member-fixture-1"},teams:[{team_id:"team-fixture-1",display_name:"Fixture Team",viewer_role:"host",viewer_agent_member_id:"member-fixture-1",default_conversation:"host",latest_run_id:"run-fixture-1",team_run_ids:["run-fixture-1"],current_member_run_id:"member-run-fixture-1"}]},attention:[],allowed_actions:[]};
    else if(url.pathname.includes("team-workspace"))body=fixtures["team-workspace"];
    else if(url.pathname.includes("host-console"))body=fixtures["host-console"];
    else if(url.pathname.includes("agent-workspace"))body=fixtures["agent-workspace"];
    else if(url.pathname.includes("member-workbench"))body=fixtures["member-workbench"];
    else if(url.pathname.includes("operator"))body=fixtures.operator;
    else return route.fulfill({status:404,contentType:"application/json",body:JSON.stringify({error:{code:"UNEXPECTED_ROUTE",message:url.pathname}})});
    return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(body)});
  });
  await page.goto(`${base}/?space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Global Work"}).waitFor();assert.equal(await page.getByText("work-fixture-1").count()>0,true);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"Global Work view overflows");
  await page.goto(`${base}/?surface=team&team=team-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Fixture Team"}).waitFor();await page.getByRole("button",{name:"Host Console"}).click();await page.getByRole("heading",{name:"Host Console"}).waitFor();
  if(viewport.width===1440){await page.locator("summary").filter({hasText:"More authorized actions"}).click();await page.getByRole("button",{name:"create work"}).click();await page.getByLabel("Work ID").fill("work-browser-1");await page.getByLabel("Title").fill("Browser action");await page.getByLabel("Completion criteria").fill("Closed semantic POST observed");await page.getByRole("button",{name:"Execute action"}).click();await page.waitForTimeout(100);assert.equal(actionExecuted,true,"Dashboard did not execute the closed action");}
  await page.goto(`${base}/?surface=team&memberRun=member-run-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("button",{name:"Open Mira Chen configuration"}).waitFor();await page.getByRole("tab",{name:/Session/}).waitFor();
  await page.goto(`${base}/?surface=operator&node=node-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Nodes"}).waitFor();assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"Nodes view overflows");
  if(viewport.width===390){capabilityMismatch=false;await page.goto(`${base}/?surface=team&team=team-fixture-1&space=fixture-space&project=fixture-project`,{waitUntil:"networkidle"});await page.getByRole("heading",{name:"Fixture Team"}).waitFor();capabilityMismatch=true;await page.getByRole("button",{name:"Host Console"}).click();await page.getByText(/Unsupported AgentFirm capabilities/).waitFor();assert.equal(await page.getByRole("button",{name:"create work"}).count(),0,"capability mismatch must fail closed");}
  await page.close();
 }
 console.log("local AgentFirm product loop browser check: PASS");
}finally{await browser.close();await vite.close();}
