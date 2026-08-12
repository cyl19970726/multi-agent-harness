#!/usr/bin/env node
import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {mkdir,readFile,writeFile} from "node:fs/promises";
import {dirname,join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {chromium} from "playwright";
import {createServer} from "vite";

const dashboardRoot=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const evidenceDir=resolve(process.env.AGENT_WORKSPACE_EVIDENCE_DIR??join(dashboardRoot,"..","..",".visual-evidence","agent-workspace","working"));
await mkdir(evidenceDir,{recursive:true});
const capturedSourceSha=process.env.FIRM_BUILD_GIT_REV??"working-tree";
if(capturedSourceSha!=="working-tree")assert.match(capturedSourceSha,/^[0-9a-f]{40}$/);
const liveConfig=process.env.AGENT_WORKSPACE_LIVE_API?{
  api:process.env.AGENT_WORKSPACE_LIVE_API,
  hostToken:process.env.AGENT_WORKSPACE_LIVE_HOST_TOKEN,
  memberToken:process.env.AGENT_WORKSPACE_LIVE_MEMBER_TOKEN,
  space:process.env.AGENT_WORKSPACE_LIVE_SPACE,
  project:process.env.AGENT_WORKSPACE_LIVE_PROJECT,
  teamRun:process.env.AGENT_WORKSPACE_LIVE_TEAM_RUN,
  member:process.env.AGENT_WORKSPACE_LIVE_MEMBER,
  memberRun:process.env.AGENT_WORKSPACE_LIVE_MEMBER_RUN,
  host:process.env.AGENT_WORKSPACE_LIVE_HOST,
}:null;
if(liveConfig){for(const [key,value] of Object.entries(liveConfig))assert.ok(value,`missing live Agent Workspace setting: ${key}`);process.env.HARNESS_CAPTURE_API_PROXY=liveConfig.api;}
const runtimeFabric={agent_identities:[],agent_sessions:[],team_memberships:[],work_execution_bindings:[],messages:[],message_deliveries:[]};
const envelope=(kind,data,actions=[])=>({view_kind:kind,schema_version:"agentfirm.role_views.v1",source_execution_space_id:"fixture-space",source_store_identity:"fixture-store",as_of_event_sequence:72,generated_at:"2026-08-12T08:10:00Z",freshness:"current",data:kind==="agent_workspace"?data:{...data,runtime_fabric:runtimeFabric},attention:[],allowed_actions:actions});
const team={team_id:"team-agent-workspace",display_name:"Product Systems Team",team_revision:5,mission_id:"mission-agent-workspace",host_agent_id:"agent-host",viewer_role:"host",node_id:"node-fixture",placement_generation:4,status:"active",latest_run:{id:"run-agent-workspace",status:"active",previous_run_id:null,execution_node_id:"node-fixture",project_binding_id:"fixture-project",execution_root:"/fixture",created_at:"2026-08-12T06:00:00Z",completed_at:null}};
const member={agent_member_ref:{kind:"agent_member",id:"agent-mira"},display_name:"Mira Chen",role:"Implementation Engineer",organization_status:"active",coordination_status:"active",provider:"codex",model:"gpt-5",native_session_health:"available",current_member_run_ref:"member-run-mira",runtime_state:"running",runtime_generation:3,capacity:"busy",active_work_count:1,queued_work_count:1,review_work_count:1,blocked_work_count:0,latest_action:null};
const analyst={...member,agent_member_ref:{kind:"agent_member",id:"agent-noah"},display_name:"Noah Park",role:"Research Verifier",provider:"kimi",current_member_run_ref:"member-run-noah",runtime_state:"idle",capacity:"available",active_work_count:0,review_work_count:0};
const roster=[{agent_member_ref:{kind:"agent_member",id:"agent-host"},display_name:"Host Agent",role:"Team Lead",organization_status:"active",coordination_status:"active",provider:"codex",model:null,native_session_health:"available",current_member_run_ref:null,runtime_state:"running",runtime_generation:null,capacity:"unknown",active_work_count:0,queued_work_count:0,review_work_count:0,blocked_work_count:0,latest_action:null,is_host:true},member,analyst];
const baseWork={work_id:"work-agent-workspace-1",work_revision:4,team_id:team.team_id,mission_id:team.mission_id,title:"Restore authored conversation dominance",context_markdown:"Match the approved Agent Workspace composition.",completion_criteria_markdown:"Host and Member share one shell; provider-private events remain owner-bound.",claim_mode:"host_assign",eligible_member_ids:["agent-mira"],prerequisite_work_ids:[],parent_work_id:null,blocker_reason:null,result_summary:null,artifact_refs:[],check_refs:[],latest_event:{id:"event-4",kind:"started",actor_ref:{kind:"agent_member",id:"agent-mira"},created_at:"2026-08-12T08:02:00Z"},owner_actor_ref:{kind:"agent_member",id:"agent-mira"},current_member_run_ref:"member-run-mira",phase:"active",condition:"normal",resolution:null,priority:"urgent",module_refs:[],gate_summary:{required:2,passed:1,failed:0,pending:1,waived:0,stale:0},latest_report_ref:null,latest_finding_refs:[],latest_failure_ref:null,delivery_summary:{queued:0,claimed:0,provider_received:1,failed:0,expired:0,invalidated:0,recovery_class:"none"},runtime_summary:{state:"running",generation:3,freshness:"current"},workspace_summary:{binding_id:"workspace-mira",lifecycle:"attached",safety:"safe"},delegation_summary:{incoming:0,outgoing:0,attention:false},updated_at:"2026-08-12T08:08:00Z"};
const works=[baseWork,{...baseWork,work_id:"work-agent-workspace-2",title:"Verify Host session privacy",work_revision:2,phase:"review",priority:"high",latest_event:{...baseWork.latest_event,id:"event-5",kind:"submitted"}},{...baseWork,work_id:"work-agent-workspace-3",title:"Polish interaction and focus states",work_revision:1,phase:"open",priority:"normal",latest_event:{...baseWork.latest_event,id:"event-6",kind:"assigned"}}];
const messages=[
  {message_id:"message-1",work_id:baseWork.work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Keep the authored exchange primary. Compact tool and runtime facts underneath it.",kind:"message",correlation_id:"conversation-1",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:00:00Z",deliveries:[{id:"delivery-1",recipient_member_run_id:"member-run-mira",status:"provider_received",version:2,provider_receipt_id:"receipt-1",updated_at:"2026-08-12T08:00:02Z"}]},
  {message_id:"message-2",work_id:baseWork.work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"Implemented the owner-bound Session projection. The Host view cannot receive another Member's native events.",kind:"message",correlation_id:"conversation-1",causation_id:"message-1",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:04:00Z",deliveries:[]},
  {message_id:"message-3",work_id:works[1].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Please include exact-SHA Session, Messages and Work screenshots in the final bundle.",kind:"message",correlation_id:"conversation-2",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:06:00Z",deliveries:[{id:"delivery-3",recipient_member_run_id:"member-run-mira",status:"queued",version:1,provider_receipt_id:null,updated_at:"2026-08-12T08:06:00Z"}]},
];
const actions=[
  {kind:"send_message",target_ref:{kind:"team_run",id:team.latest_run.id},required_version:5,disabled_reason:null},
  {kind:"rebind_work",target_ref:{kind:"work",id:baseWork.work_id},required_version:4,disabled_reason:null},
  {kind:"request_gate_evaluation",target_ref:{kind:"work",id:works[1].work_id},required_version:2,disabled_reason:null},
  {kind:"close_member_run",target_ref:{kind:"member_run",id:"member-run-mira"},required_version:3,disabled_reason:null},
];
const configuration={description:"Owns the frontend implementation and exact-source validation.",prompt_ref:null,prompt_projection:"not_modeled",skill_refs:["harness-frontend-product-design","frontend-visual-contract"],capabilities:["workspace_write","browser_acceptance","source_review"],tool_refs:[],tools_projection:"not_modeled_by_agent_member",provider_profile_ref:"codex-app-server-v1",model_preference:"gpt-5",workspace_policy:"isolated_worktree",permission_ceiling:"full_access",forbidden_actions:[],forbidden_actions_projection:"not_modeled",workspace_binding:{kind:"workspace_binding",id:"workspace-mira",work_id:baseWork.work_id,member_run_id:"member-run-mira",requirement_id:null,status:"attached",version:2,actor_ref:null,summary:null,created_at:"2026-08-12T07:00:00Z",source_id:null,target_id:null,locator:"/fixture/worktree"}};
const memberActivity=[
  {event_id:"native-0",kind:"message",status:"completed",title:"Mira",summary:"I mapped the Agent Workspace read model to the approved composition and started with the privacy boundary.",occurred_at:"2026-08-12T07:58:00Z"},
  {event_id:"native-1",kind:"tool",status:"completed",title:"Inspected canonical RoleView contracts",summary:"Read the current TeamWorkspace, HostConsole and MemberWorkbench projections without creating a second task or message model.",occurred_at:"2026-08-12T08:01:00Z"},
  {event_id:"native-2",kind:"tool",status:"completed",title:"Validated owner-bound native Session",summary:"The server resolved the selected MemberRun before reading the provider-native store. Private reasoning and raw tool output remain excluded.",occurred_at:"2026-08-12T08:03:00Z"},
  {event_id:"native-3",kind:"event",status:"completed",title:"Frontend build passed",summary:"TypeScript and Vite completed for the unified three-column shell.",occurred_at:"2026-08-12T08:07:00Z"},
];
const memberView=envelope("agent_workspace",{team,selected_agent:{agent_member_ref:member.agent_member_ref,display_name:member.display_name,role:member.role,organization_status:"active",is_host:false,current_member_run_ref:"member-run-mira",provider:"codex",execution_mode:"codex_app_server",runtime_status:"running"},roster,sessions:[{session_id:"session-mira-current",member_run_id:"member-run-mira",team_run_id:team.latest_run.id,provider:"codex",execution_mode:"codex_app_server",coordination_status:"active",runtime_status:"running",runtime_generation:3,started_at:"2026-08-12T07:30:00Z",last_active_at:"2026-08-12T08:07:00Z",ended_at:null},{session_id:"session-mira-previous",member_run_id:"member-run-mira-old",team_run_id:"run-old",provider:"codex",execution_mode:"codex_app_server",coordination_status:"closed",runtime_status:"stopped",runtime_generation:2,started_at:"2026-08-11T07:30:00Z",last_active_at:"2026-08-11T10:00:00Z",ended_at:"2026-08-11T10:01:00Z"}],selected_session_id:"session-mira-current",session_activity:{native_session_id:"session-mira-current",provider:"codex",execution_mode:"codex_app_server",availability:"available",items:memberActivity,truncated:false,disabled_reason:null},messages,works,configuration,context_summary:{current_work_id:baseWork.work_id,message_count:messages.length,unread_count:1,last_activity_at:"2026-08-12T08:07:00Z",authorization_count:actions.length}},actions);
memberView.data.projection_scope="member_self_private";
memberView.data.team={team_id:team.team_id,display_name:team.display_name,team_revision:team.team_revision,mission_id:team.mission_id,host_agent_id:team.host_agent_id,viewer_role:team.viewer_role,status:team.status,latest_run_id:team.latest_run.id};
const hostMessages=[messages[0],messages[2]];
const hostView=envelope("agent_workspace",{...memberView.data,selected_agent:{agent_member_ref:{kind:"agent_member",id:"agent-host"},display_name:"Host Agent",role:"Team Lead",organization_status:"active",is_host:true,current_member_run_ref:null,provider:"codex",execution_mode:"host_native",runtime_status:"active"},selected_session_id:"host-thread-current",sessions:[{session_id:"host-thread-current",member_run_id:null,team_run_id:team.latest_run.id,provider:"codex",execution_mode:"host_native",coordination_status:"active",runtime_status:"active",runtime_generation:null,started_at:"2026-08-12T06:00:00Z",last_active_at:"2026-08-12T08:09:00Z",ended_at:null}],session_activity:{native_session_id:"host-thread-current",provider:"codex",execution_mode:"host_native",availability:"available",items:[{event_id:"host-native-0",kind:"message",status:"completed",title:"Host Agent",summary:"I reviewed the current decision surface and sent the next bounded assignment.",occurred_at:"2026-08-12T08:05:00Z"},{event_id:"host-native-1",kind:"tool",status:"completed",title:"Read Lead inbox",summary:"Provider-native Host event; no Member tool or thinking event is present.",occurred_at:"2026-08-12T08:08:00Z"}],truncated:false,disabled_reason:null},messages:hostMessages,configuration:{...configuration,description:"Owns Team judgment and assignment authority."},context_summary:{current_work_id:works[1].work_id,message_count:hostMessages.length,unread_count:0,last_activity_at:"2026-08-12T08:08:00Z",authorization_count:actions.length}},actions);
hostView.data.projection_scope="host_self_private";
const hostMemberPublic=envelope("agent_workspace",{...memberView.data,projection_scope:"host_member_public",selected_agent:{...memberView.data.selected_agent,current_member_run_ref:null,provider:null,execution_mode:null,runtime_status:null},sessions:[],selected_session_id:null,session_activity:{native_session_id:null,provider:null,execution_mode:null,availability:"unavailable",items:[],truncated:false,disabled_reason:"Provider-private Session events are visible only to the exact selected Agent identity."},configuration:{...configuration,tool_refs:[],provider_profile_ref:null,model_preference:null,workspace_policy:null,permission_ceiling:null,workspace_binding:null},messages:messages.map(message=>({...message,deliveries:[]})),works:works.map(work=>({...work,current_member_run_ref:null,runtime_summary:{state:"not_projected",generation:null,freshness:"unknown"},workspace_summary:{binding_id:null,lifecycle:"not_projected",safety:"unknown"}}))},actions);
const teamWorkspace=envelope("team_workspace",{team,pressure_summary:{active_turns:1,ready_members:1,total_members:2,ready_work:1,review_work:1,blocked_work:0},works,members:[member,analyst],messages,activity:[],activity_truncated:false,reports:[],findings:[],failures:[],gate_requirements:[],gate_evaluations:[],gate_waivers:[],workspace_attention:[],delegation_provenance:[],page:{as_of_event_sequence:72,item_count:works.length,next_cursor:null}});

const vite=await createServer({configFile:join(dashboardRoot,"vite.config.ts"),server:{host:"127.0.0.1",port:0},logLevel:"silent"});
await vite.listen();
const base=`http://127.0.0.1:${vite.httpServer.address().port}`;
const browser=await chromium.launch({headless:true});
try{
  const consoleErrors=[];
  let failMemberRefresh=false;
  let failHostMemberProjection=false;
  let hostMemberProjectionGate=null;
  const makePage=async token=>{const next=await browser.newPage({viewport:{width:1440,height:1000}});next.on("console",message=>{if(message.type()==="error")consoleErrors.push(message.text());});next.on("pageerror",error=>consoleErrors.push(error.message));await next.addInitScript(({token,fixture})=>{window.__AGENTFIRM_BOOTSTRAP__={capabilityToken:token};class QuietEventSource{addEventListener(type,listener){if(fixture&&type==="snapshot")queueMicrotask(()=>listener({data:JSON.stringify({generated_at:"2026-08-12T08:10:00Z",execution_space_id:"fixture-space"})}));}close(){}}Object.defineProperty(window,"EventSource",{value:QuietEventSource,configurable:true});},{token,fixture:!liveConfig});if(!liveConfig)await next.route("**/v1/**",async route=>{
    const request=route.request(),url=new URL(request.url());
    const token=request.headers()["x-agentfirm-token"];
    if(request.method()==="POST")return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify({ok:true})});
    let body;
    if(url.pathname==="/v1/meta")body={schema_version:"agentfirm.role_views.v1",protocol_version:"agentfirm-member-trust/1",action_manifest_version:"agentfirm.role_actions.v1",capability_auth:"x-agentfirm-token",build_sha:"b362bc1ba1ebbeff26eb9a4a08bf3c6982ec764d"};
    else if(url.pathname==="/v1/projects")body={projects:[{id:"fixture-project",is_current:true}]};
    else if(url.pathname==="/v1/spaces")body={spaces:[{id:"fixture-space",is_current:true}]};
    else if(url.pathname==="/v1/companies")body={companies:[]};
    else if(url.pathname==="/v1/workflows")body={workflows:[]};
    else if(url.pathname==="/v1/snapshot"||url.pathname.includes("/snapshot"))body={generated_at:"2026-08-12T08:10:00Z",execution_space_id:"fixture-space",teams:[],team_runs:[],member_runs:[],execution_nodes:[],company_os:{}};
    else if(url.pathname.includes("team-workspace"))body=teamWorkspace;
    else if(url.pathname.includes("agent-workspace")){
      const memberSelected=url.searchParams.get("agent_id")==="agent-mira"||url.pathname.endsWith("member-run-mira");
      if(memberSelected&&token==="fixture-member-token"&&failMemberRefresh)return route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:{message:"fixture member refresh failed"}})});
      if(memberSelected&&token==="fixture-host-token"){
        if(hostMemberProjectionGate)await hostMemberProjectionGate;
        if(failHostMemberProjection)return route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:{message:"fixture public projection failed"}})});
      }
      body=memberSelected?(token==="fixture-member-token"?memberView:hostMemberPublic):hostView;
    }
    else return route.fulfill({status:404,contentType:"application/json",body:JSON.stringify({error:{message:url.pathname}})});
    return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(body)});
  });return next;};
  const page=await makePage(liveConfig?.memberToken??"fixture-member-token");
  const open=async(target,url)=>{await target.goto(url,{waitUntil:"networkidle"});await target.getByTestId("agent-workspace").waitFor();assert.equal(await target.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"desktop horizontal overflow");};
  const routeState=liveConfig?{teamRun:liveConfig.teamRun,member:liveConfig.member,memberRun:liveConfig.memberRun,host:liveConfig.host,space:liveConfig.space,project:liveConfig.project}:{teamRun:team.latest_run.id,member:"agent-mira",memberRun:"member-run-mira",host:"agent-host",space:"fixture-space",project:"fixture-project"};
  await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
  await page.getByRole("button",{name:/Open .* configuration/}).waitFor();
  if(!liveConfig){await page.getByText(/Implemented the owner-bound Session projection/).waitFor();await page.getByText("Validated owner-bound native Session",{exact:true}).waitFor();}
  await page.screenshot({path:join(evidenceDir,`member-session--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  if(!liveConfig){
    const authoredMessage=page.getByRole("button",{name:/Open authored Message from Host Agent/}).first();
    await authoredMessage.focus();await page.keyboard.press("Enter");await page.getByText("Selected Message",{exact:true}).waitFor();
    const providerReply=page.getByRole("button",{name:/Open provider-authored reply from Mira Chen/}).first();
    await providerReply.focus();await page.keyboard.press("Space");await page.getByText("Selected Event",{exact:true}).waitFor();
    const toolRow=page.getByRole("button",{name:/Validated owner-bound native Session/});await toolRow.focus();await page.keyboard.press("Enter");assert.equal(await toolRow.getAttribute("aria-expanded"),"true","event row keyboard expansion");
  }
  await page.getByRole("tab",{name:/Messages/}).click();await page.getByRole("button",{name:"inbox",exact:true}).waitFor();
  await page.screenshot({path:join(evidenceDir,`member-messages--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  await page.getByRole("tab",{name:/Work/}).click();
  if(!liveConfig)await page.getByRole("button",{name:/Restore authored conversation dominance/}).waitFor();
  else await page.getByRole("button",{name:/Implement unified Host and Member Agent Workspace/}).waitFor();
  await page.screenshot({path:join(evidenceDir,`member-work--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  const profileOpener=page.getByRole("button",{name:/Open .* configuration/});
  await profileOpener.click();await page.getByRole("dialog").waitFor();
  const profileClose=page.getByRole("button",{name:"Close Agent configuration"});
  await profileClose.waitFor();assert.equal(await profileClose.evaluate(node=>node===document.activeElement),true,"Profile dialog moves focus inside");
  await page.keyboard.press("Tab");assert.equal(await profileClose.evaluate(node=>node===document.activeElement),true,"Profile dialog traps forward Tab");
  await page.keyboard.press("Shift+Tab");assert.equal(await profileClose.evaluate(node=>node===document.activeElement),true,"Profile dialog traps reverse Tab");
  await page.screenshot({path:join(evidenceDir,`member-configuration--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  await page.keyboard.press("Escape");await page.getByRole("dialog").waitFor({state:"detached"});
  assert.equal(await profileOpener.evaluate(node=>node===document.activeElement),true,"Profile dialog restores opener focus after Escape");
  if(!liveConfig){
    await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
    await page.locator('textarea[aria-label="Message"]').waitFor();
    failMemberRefresh=true;
    await page.locator('textarea[aria-label="Message"]').fill("Trigger authoritative refresh failure");
    await page.getByRole("button",{name:"Send",exact:true}).click();
    await page.getByRole("alert").filter({hasText:"Refresh failed; writes are disabled"}).waitFor();
    assert.equal(await page.locator('textarea[aria-label="Message"]').count(),0,"refresh failure retained a writable message composer");
    assert.equal(await page.getByLabel("Composer action").count(),0,"refresh failure retained a writable action selector");
    failMemberRefresh=false;
  }
  const hostPage=await makePage(liveConfig?.hostToken??"fixture-host-token");
  await open(hostPage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.host}&space=${routeState.space}&project=${routeState.project}`);
  if(liveConfig)await hostPage.locator(".agent-authored-turn").first().waitFor();
  if(!liveConfig){assert.equal(await hostPage.getByText("Validated owner-bound native Session",{exact:true}).count(),0,"Member-private provider event leaked into Host Session");assert.equal(await hostPage.getByText("Read Lead inbox",{exact:true}).count(),1,"Host-native event missing");}
  await hostPage.screenshot({path:join(evidenceDir,`host-session--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  if(liveConfig)await hostPage.locator(".agent-roster-row").filter({hasNotText:"Host"}).first().click();
  else{
    let releasePublicProjection;
    hostMemberProjectionGate=new Promise(resolve=>{releasePublicProjection=resolve;});
    await hostPage.getByRole("button",{name:/Mira Chen/}).first().click();
    await hostPage.getByText("Read Lead inbox",{exact:true}).waitFor({state:"detached"});
    assert.equal(await hostPage.getByTestId("agent-workspace").count(),0,"old Host-private view remained while public projection was pending");
    assert.equal(await hostPage.locator('textarea[aria-label="Message"]').count(),0,"identity switch retained a writable composer while projection was pending");
    assert.equal(await hostPage.getByLabel("Composer action").count(),0,"identity switch retained an action selector while projection was pending");
    releasePublicProjection();hostMemberProjectionGate=null;
  }
  await hostPage.getByText(/Public Agent context/).waitFor();
  if(liveConfig)await hostPage.locator(".agent-authored-turn").first().waitFor();
  assert.equal(await hostPage.getByText("Validated owner-bound native Session",{exact:true}).count(),0,"Host-selected Member leaked provider-private activity");
  await hostPage.screenshot({path:join(evidenceDir,`host-member-public--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  if(!liveConfig){
    await open(hostPage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.host}&space=${routeState.space}&project=${routeState.project}`);
    failHostMemberProjection=true;
    await hostPage.getByRole("button",{name:/Mira Chen/}).first().click();
    await hostPage.getByRole("alert").filter({hasText:"Agent Workspace"}).waitFor();
    assert.equal(await hostPage.getByText("Read Lead inbox",{exact:true}).count(),0,"failed identity switch restored old Host-private activity");
    assert.equal(await hostPage.locator('textarea[aria-label="Message"]').count(),0,"failed identity switch retained a writable composer");
    assert.equal(await hostPage.getByLabel("Composer action").count(),0,"failed identity switch retained an action selector");
    failHostMemberProjection=false;
  }
  const expectedFailureErrors=consoleErrors.filter(message=>message.includes("503 (Service Unavailable)"));
  if(!liveConfig)assert.equal(expectedFailureErrors.length,2,"fixture should exercise exactly two expected 503 projection failures");
  assert.deepEqual(consoleErrors.filter(message=>!message.includes("503 (Service Unavailable)")),[],`unexpected console errors: ${consoleErrors.join(" | ")}`);

  for(const viewport of [{width:900,height:1180},{width:390,height:844},{width:320,height:844}]){
    await page.setViewportSize(viewport);
    await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,`${viewport.width}px horizontal overflow`);
  }
  const captures=[];
  for(const name of ["member-session","member-messages","member-work","member-configuration","host-session","host-member-public"]){
    const file=`${name}--1440x1000--${capturedSourceSha}.png`;
    captures.push({name,file,viewport:{width:1440,height:1000},sha256:createHash("sha256").update(await readFile(join(evidenceDir,file))).digest("hex")});
  }
  const liveMemberEvidence=liveConfig?await fetch(`${liveConfig.api}/v1/views/agent-workspace/${encodeURIComponent(liveConfig.teamRun)}?space=${encodeURIComponent(liveConfig.space)}&project=${encodeURIComponent(liveConfig.project)}&agent_id=${encodeURIComponent(liveConfig.member)}`,{headers:{"X-AgentFirm-Token":liveConfig.memberToken}}).then(async response=>{assert.equal(response.ok,true,`live evidence RoleView ${response.status}`);return response.json();}):null;
  const manifest={
    evidence_kind:liveConfig?"canonical_store_live":"automated_contract_fixture",
    captured_source_sha:capturedSourceSha,
    captured_at:new Date().toISOString(),
    execution_space_id:liveConfig?.space??"fixture-space",
    project_binding_id:liveConfig?.project??"fixture-project",
    source_store_identity:liveMemberEvidence?.source_store_identity??"fixture-store",
    as_of_event_sequence:liveMemberEvidence?.as_of_event_sequence??72,
    canonical_objects:liveConfig?{team_run_id:liveConfig.teamRun,host_agent_member_id:liveConfig.host,member_agent_member_id:liveConfig.member,member_run_id:liveConfig.memberRun}:null,
    native_session_claim:liveConfig?{availability:liveMemberEvidence?.data?.session_activity?.availability??"unknown",native_session_id:liveMemberEvidence?.data?.selected_session_id??null,claim:liveMemberEvidence?.data?.selected_session_id?"owner-bound native Session captured":"not claimed: no bound provider-native Session in the canonical store"}:"deterministic fixture only",
    captures,
  };
  await writeFile(join(evidenceDir,"evidence-manifest.json"),`${JSON.stringify(manifest,null,2)}\n`);
  console.log(`agent workspace browser check: PASS (${liveConfig?"store-live":"fixture"}; ${evidenceDir})`);
}finally{await browser.close();await vite.close();}
