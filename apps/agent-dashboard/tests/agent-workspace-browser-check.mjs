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
const reviewer={...member,agent_member_ref:{kind:"agent_member",id:"agent-ava"},display_name:"Ava Stone",role:"Security Reviewer",provider:"claude",current_member_run_ref:"member-run-ava",runtime_state:"idle",capacity:"available",active_work_count:0,queued_work_count:0,review_work_count:1};
const strategist={...member,agent_member_ref:{kind:"agent_member",id:"agent-lena"},display_name:"Lena Ortiz",role:"Product Strategist",provider:"codex",current_member_run_ref:"member-run-lena",runtime_state:"waiting",capacity:"busy",active_work_count:1,queued_work_count:0,review_work_count:0};
const roster=[{agent_member_ref:{kind:"agent_member",id:"agent-host"},display_name:"Marcus Allen",role:"Team Lead",organization_status:"active",coordination_status:"active",provider:"codex",model:null,native_session_health:"available",host_session_mode:"external_interactive",current_member_run_ref:null,runtime_state:"running",runtime_generation:null,capacity:"unknown",active_work_count:0,queued_work_count:0,review_work_count:0,blocked_work_count:0,latest_action:null,is_host:true},member,analyst,reviewer,strategist];
const baseWork={work_id:"work-agent-workspace-1",work_revision:4,team_id:team.team_id,mission_id:team.mission_id,title:"Restore authored conversation dominance",context_markdown:"Match the approved Agent Workspace composition.",completion_criteria_markdown:"Host and Member share one shell; provider-private events remain owner-bound.",claim_mode:"host_assign",eligible_member_ids:["agent-mira"],prerequisite_work_ids:[],successor_work_ids:[],readiness:{state:"not_claimable",reason_codes:["fixture_state"],unsatisfied_prerequisite_work_ids:[],failed_or_cancelled_prerequisite_work_ids:[]},blocker_reason:null,result_summary:null,artifact_refs:[],check_refs:[],latest_event:{id:"event-4",kind:"started",actor_ref:{kind:"agent_member",id:"agent-mira"},created_at:"2026-08-12T08:02:00Z"},owner_actor_ref:{kind:"agent_member",id:"agent-mira"},current_member_run_ref:"member-run-mira",phase:"active",condition:"normal",resolution:null,priority:"urgent",module_refs:[],gate_summary:{required:2,passed:1,failed:0,pending:1,waived:0,stale:0},latest_report_ref:null,latest_finding_refs:[],latest_failure_ref:null,delivery_summary:{queued:0,claimed:0,provider_received:1,failed:0,expired:0,invalidated:0,recovery_class:"none"},runtime_summary:{state:"running",generation:3,freshness:"current"},workspace_summary:{binding_id:"workspace-mira",lifecycle:"attached",safety:"safe"},delegation_summary:{incoming:0,outgoing:0,attention:false},updated_at:"2026-08-12T08:08:00Z"};
const works=[
  baseWork,
  {...baseWork,work_id:"work-agent-workspace-2",title:"Verify Host session privacy",work_revision:2,phase:"review",priority:"high",completion_criteria_markdown:"Host view renders public coordination only; private Session facts stay structurally absent.",latest_event:{...baseWork.latest_event,id:"event-5",kind:"submitted"}},
  {...baseWork,work_id:"work-agent-workspace-3",title:"Polish interaction and focus states",work_revision:1,phase:"open",priority:"normal",completion_criteria_markdown:"Every interactive row exposes a visible focus ring and hover affordance.",latest_event:{...baseWork.latest_event,id:"event-6",kind:"assigned"}},
  {...baseWork,work_id:"work-agent-workspace-4",title:"Calibrate type scale and record rhythm",work_revision:3,phase:"active",priority:"high",completion_criteria_markdown:"Body text and metadata separate by weight and contrast instead of tiny-size differences.",latest_event:{...baseWork.latest_event,id:"event-7",kind:"started"}},
  {...baseWork,work_id:"work-agent-workspace-5",title:"Converge the selection-aware context rail",work_revision:2,phase:"open",priority:"normal",completion_criteria_markdown:"Rail sections follow the current selection; empty sections reserve no decorative space.",latest_event:{...baseWork.latest_event,id:"event-8",kind:"assigned"}},
  {...baseWork,work_id:"work-agent-workspace-6",title:"Freeze exact-source desktop evidence",work_revision:5,phase:"review",priority:"urgent",completion_criteria_markdown:"Session, Messages and Work screenshots all bind to one exact revision in the final bundle.",latest_event:{...baseWork.latest_event,id:"event-9",kind:"submitted"}},
  {...baseWork,work_id:"work-agent-workspace-7",title:"Preserve compact viewport interaction",work_revision:1,phase:"open",priority:"low",completion_criteria_markdown:"Compact viewports keep roster navigation and the composer reachable without clipping.",latest_event:{...baseWork.latest_event,id:"event-10",kind:"created"}},
];
const messages=[
  {message_id:"message-1",work_id:baseWork.work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Keep the authored exchange primary. Compact tool and runtime facts underneath it.",kind:"message",correlation_id:"conversation-1",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:00:00Z",deliveries:[{id:"delivery-1",recipient_member_run_id:"member-run-mira",status:"provider_received",version:2,provider_receipt_id:"receipt-1",updated_at:"2026-08-12T08:00:02Z"}]},
  {message_id:"message-2",work_id:baseWork.work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"Implemented the owner-bound Session projection. The Host view cannot receive another Member's native events.",kind:"message",correlation_id:"conversation-1",causation_id:"message-1",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:04:00Z",deliveries:[]},
  {message_id:"message-3",work_id:works[1].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Please include exact-SHA Session, Messages and Work screenshots in the final bundle.",kind:"message",correlation_id:"conversation-2",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:06:00Z",deliveries:[{id:"delivery-3",recipient_member_run_id:"member-run-mira",status:"queued",version:1,provider_receipt_id:null,updated_at:"2026-08-12T08:06:00Z"}]},
  {message_id:"message-4",work_id:works[3].work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"Body text and metadata now separate by weight and contrast instead of tiny-size differences.",kind:"message",correlation_id:"conversation-3",causation_id:null,response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:08:00Z",deliveries:[]},
  {message_id:"message-5",work_id:works[4].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Keep the context rail limited to facts that change the selected Agent or Work decision.",kind:"message",correlation_id:"conversation-4",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:09:00Z",deliveries:[{id:"delivery-5",recipient_member_run_id:"member-run-mira",status:"provider_received",version:2,provider_receipt_id:"receipt-5",updated_at:"2026-08-12T08:09:02Z"}]},
  {message_id:"message-6",work_id:works[4].work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"The rail is now selection-aware; empty sections do not reserve decorative space.",kind:"message",correlation_id:"conversation-4",causation_id:"message-5",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:11:00Z",deliveries:[]},
  {message_id:"message-7",work_id:works[5].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Freeze the full desktop family only after Session, Messages, Work and configuration read as one product.",kind:"message",correlation_id:"conversation-5",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:12:00Z",deliveries:[{id:"delivery-7",recipient_member_run_id:"member-run-mira",status:"queued",version:1,provider_receipt_id:null,updated_at:"2026-08-12T08:12:00Z"}]},
  {message_id:"message-8",work_id:works[5].work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"The complete frame set is ready for exact-revision self-review; no page was submitted independently.",kind:"message",correlation_id:"conversation-5",causation_id:"message-7",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:14:00Z",deliveries:[]},
];
const actions=[
  {kind:"send_message",target_ref:{kind:"team_run",id:team.latest_run.id},required_version:5,disabled_reason:null},
  {kind:"rebind_work",target_ref:{kind:"work",id:baseWork.work_id},required_version:4,disabled_reason:null},
  {kind:"request_gate_evaluation",target_ref:{kind:"work",id:works[1].work_id},required_version:2,disabled_reason:null},
  {kind:"close_member_run",target_ref:{kind:"member_run",id:"member-run-mira"},required_version:3,disabled_reason:null},
];
const configuration={description:"Owns the frontend implementation and exact-source validation.",prompt_ref:null,prompt_projection:"not_modeled",skill_refs:["harness-frontend-product-design","frontend-visual-contract"],capabilities:["workspace_write","browser_acceptance","source_review"],tool_refs:[],tools_projection:"not_modeled_by_agent_member",provider_profile_ref:"codex-app-server-v1",model_preference:"gpt-5",workspace_policy:"isolated_worktree",permission_ceiling:"full_access",forbidden_actions:[],forbidden_actions_projection:"not_modeled",workspace_binding:{kind:"workspace_binding",id:"workspace-mira",work_id:baseWork.work_id,member_run_id:"member-run-mira",requirement_id:null,status:"attached",version:2,actor_ref:null,summary:null,created_at:"2026-08-12T07:00:00Z",source_id:null,target_id:null,locator:"/fixture/worktree"}};
const observation=(id,semanticKind,payload,at,position)=>({schema_version:"agentfirm.provider_observation.v1",observation_id:id,provider:"codex",adapter_version:"agentfirm.provider_event_adapter.v1",native_source_ref:`provider-source:fixture/${id}`,agent_identity_id:"agent-mira",agent_session_id:"session-mira-current",agent_session_generation:3,node_daemon_id:"node-fixture",node_daemon_generation:4,provider_thread_id:"thread-mira",provider_turn_id:"turn-mira-1",provider_event_id:id,ordering_position:position,causal_parent_id:null,correlation_id:null,runtime_command_id:null,occurred_at:at,observed_at:at,semantic_kind:semanticKind,lifecycle_phase:"terminal",completeness:"complete",effect_certainty:semanticKind.startsWith("tool_call_")?"applied":"none",visibility:"session_owner_private",redacted:true,truncated:false,source_content_fingerprint:`sha256:${id.padEnd(64,"0").slice(0,64)}`,payload});
const memberObservations=[
  observation("native-0","authored_response",{type:"authored_response",text:"I mapped the Agent Workspace read model to the approved composition and started with the privacy boundary."},"2026-08-12T07:58:00Z",0),
  observation("native-1","tool_call_completed",{type:"tool",tool_name:"Inspected canonical RoleView contracts",call_id:"call-1",display_detail:"Read the current projections without creating a second task, message, or provider-event store."},"2026-08-12T08:01:00Z",1),
  observation("native-2","tool_call_completed",{type:"tool",tool_name:"Validated owner-bound native Session",call_id:"call-2",display_detail:"Resolved the exact MemberRun and provider-native source; private data remains owner-bound."},"2026-08-12T08:03:00Z",2),
  observation("native-3","turn_completed",{type:"turn",outcome:"completed",display_summary:"Frontend build passed for the unified three-column shell."},"2026-08-12T08:07:00Z",3),
];
const memberProjection={schema_version:"agentfirm.provider_observation.v1",agent_session_id:"session-mira-current",agent_session_generation:3,source_snapshot_fingerprint:"sha256:fixture",episodes:[{episode_id:"episode-mira-1",provider_turn_id:"turn-mira-1",observations:memberObservations,terminal:true,incomplete:false}],truncated:false,availability:"available",unavailable_reason_code:null,disabled_reason:null};
const privateBase={projection_scope:"member_self_private",team,selected_agent:{agent_member_ref:member.agent_member_ref,display_name:member.display_name,role:member.role,organization_status:"active",is_host:false,current_member_run_ref:"member-run-mira",provider:"codex",execution_mode:"codex_app_server",runtime_status:"running",runtime_generation:3,host_session_mode:null},roster,session_event_projection:memberProjection,live_provider_activity:null,messages,works,configuration,context_summary:{current_work_id:baseWork.work_id,message_count:messages.length,unread_count:1,last_activity_at:"2026-08-12T08:07:00Z",authorization_count:actions.length}};
const memberView=envelope("agent_workspace",privateBase,actions);
memberView.data.team={team_id:team.team_id,display_name:team.display_name,team_revision:team.team_revision,mission_id:team.mission_id,host_agent_id:team.host_agent_id,viewer_role:team.viewer_role,status:team.status,latest_run_id:team.latest_run.id};
const hostMessages=messages;
const hostObservations=[observation("host-native-0","authored_response",{type:"authored_response",text:"I reviewed the current decision surface and sent the next bounded assignment."},"2026-08-12T08:05:00Z",0),observation("host-native-1","tool_call_completed",{type:"tool",tool_name:"Read Lead inbox",call_id:"host-call-1",display_detail:"Provider-native Host observation; no Member-private tool or reasoning data is present."},"2026-08-12T08:08:00Z",1)].map(item=>({...item,agent_identity_id:"agent-host",agent_session_id:"host-thread-current",agent_session_generation:1,provider_turn_id:"turn-host-1"}));
const hostView=envelope("agent_workspace",{...memberView.data,projection_scope:"host_self_private",selected_agent:{agent_member_ref:{kind:"agent_member",id:"agent-host"},display_name:"Marcus Allen",role:"Team Lead",organization_status:"active",is_host:true,current_member_run_ref:"member-run-host",provider:"codex",execution_mode:"host_native",runtime_status:"active",runtime_generation:1,host_session_mode:"external_interactive"},session_event_projection:{...memberProjection,agent_session_id:"host-thread-current",agent_session_generation:1,episodes:[{episode_id:"episode-host-1",provider_turn_id:"turn-host-1",observations:hostObservations,terminal:true,incomplete:false}]},live_provider_activity:null,messages:hostMessages,configuration:{...configuration,description:"Owns Team judgment and assignment authority."},context_summary:{current_work_id:works[1].work_id,message_count:hostMessages.length,unread_count:0,last_activity_at:"2026-08-12T08:08:00Z",authorization_count:actions.length}},actions);
const {session_event_projection:_privateHistory,live_provider_activity:_privateLive,...publicBase}=memberView.data;
const hostMemberPublic=envelope("agent_workspace",{...publicBase,projection_scope:"host_member_public",selected_agent:{...memberView.data.selected_agent,current_member_run_ref:null,provider:null,execution_mode:null,runtime_status:null,runtime_generation:null},configuration:{...configuration,prompt_ref:null,tool_refs:[],provider_profile_ref:null,model_preference:null,workspace_policy:null,permission_ceiling:null,forbidden_actions:[],workspace_binding:null},messages:messages.map(message=>({...message,deliveries:[]})),works:works.map(work=>({...work,current_member_run_ref:null,runtime_summary:{state:"not_projected",generation:null,freshness:"unknown"},workspace_summary:{binding_id:null,lifecycle:"not_projected",safety:"unknown"}}))},actions);
assert.equal("session_event_projection" in hostMemberPublic.data,false,"host_member_public fixture must structurally omit private history");
assert.equal("live_provider_activity" in hostMemberPublic.data,false,"host_member_public fixture must structurally omit private live activity");
const teamWorkspace=envelope("team_workspace",{team,pressure_summary:{active_turns:1,ready_members:1,total_members:2,ready_work:1,review_work:1,blocked_work:0},works,work_graph:{nodes:works,edges:[],ready_work_ids:[],attention_work_ids:[]},members:[member,analyst],messages,activity:[],activity_truncated:false,reports:[],findings:[],failures:[],gate_requirements:[],gate_evaluations:[],gate_waivers:[],workspace_attention:[],delegation_provenance:[],page:{as_of_event_sequence:72,item_count:works.length,next_cursor:null}});

const vite=await createServer({configFile:join(dashboardRoot,"vite.config.ts"),server:{host:"127.0.0.1",port:0},logLevel:"silent"});
await vite.listen();
const base=`http://127.0.0.1:${vite.httpServer.address().port}`;
const browser=await chromium.launch({headless:true});
try{
  const consoleErrors=[];
  const httpFailures=[];
  const unknownFixturePaths=[];
  let failMemberRefresh=false;
  let failHostMemberProjection=false;
  let hostMemberProjectionGate=null;
  let firstMemberWorkspaceRequestAt=null;
  const makePage=async token=>{const next=await browser.newPage({viewport:{width:1440,height:1000}});next.on("console",message=>{if(message.type()==="error")consoleErrors.push(message.text());});next.on("pageerror",error=>consoleErrors.push(error.message));next.on("response",response=>{if(response.status()>=400)httpFailures.push(`${response.status()} ${response.url()}`);});await next.addInitScript(({token,fixture,scope})=>{window.__AGENTFIRM_BOOTSTRAP__={capabilityToken:token};class QuietEventSource{timer;started=false;addEventListener(type,listener){if(!fixture||type!=="snapshot"||this.started)return;this.started=true;let revision=0;this.timer=setInterval(()=>{revision+=1;listener({data:JSON.stringify({generated_at:`2026-08-12T08:10:${String(revision).padStart(2,"0")}Z`,execution_space_id:"fixture-space"})});if(revision===80){clearInterval(this.timer);this.timer=undefined;}},15);}close(){if(this.timer)clearInterval(this.timer);}}Object.defineProperty(window,"EventSource",{value:QuietEventSource,configurable:true});if(fixture){const nativeFetch=window.fetch.bind(window);let memberStreamIssued=false;try{memberStreamIssued=sessionStorage.getItem("fixture-member-stream-issued")==="1";}catch{}window.fetch=async(input,init)=>{const request=input instanceof Request?input:null;const url=new URL(request?.url??String(input),location.href);if(url.pathname!=="/v1/events")return nativeFetch(input,init);const headers=new Headers(request?.headers);new Headers(init?.headers).forEach((value,key)=>headers.set(key,value));const capability=headers.get("X-AgentFirm-Token");if(!capability)throw new Error("private live stream must carry X-AgentFirm-Token");const member=capability==="fixture-member-token";const exactScope=member?scope.member:scope.host;const terminal={schema_version:"agentfirm.live_provider_activity_event.v1",reason:"terminal",scope:exactScope,activity:null};const encode=value=>new TextEncoder().encode(`event: live_provider_activity\ndata: ${JSON.stringify(value)}\n\n`);if(!member||memberStreamIssued)return new Response(encode(terminal),{headers:{"Content-Type":"text/event-stream"}});memberStreamIssued=true;try{sessionStorage.setItem("fixture-member-stream-issued","1");}catch{}let timer;const stream=new ReadableStream({start(controller){const now=Date.now();const activity={schema_version:"agentfirm.live_provider_activity.v1",durability:"volatile_process_memory",replayable:false,...exactScope,runtime_snapshot_locator:"runtime-snapshot-browser-live",expires_unix_ms:now+5000,items:[{runtime_event_locator:"runtime-event-browser-live",kind:"tool_started",provider:"codex",display_summary:"Codex tool started",emitted_unix_ms:now,expires_unix_ms:now+5000}]};controller.enqueue(encode({schema_version:"agentfirm.live_provider_activity_event.v1",reason:"updated",scope:exactScope,activity}));const staleScope={...exactScope,agent_session_id:"stale-session",agent_session_generation:exactScope.agent_session_generation+1};controller.enqueue(encode({schema_version:"agentfirm.live_provider_activity_event.v1",reason:"terminal",scope:staleScope,activity:null}));timer=setTimeout(()=>{controller.enqueue(encode(terminal));controller.close();},700);},cancel(){if(timer)clearTimeout(timer);}});return new Response(stream,{headers:{"Content-Type":"text/event-stream"}});};}},{token,fixture:!liveConfig,scope:{member:{execution_space_id:"fixture-space",project_id:"fixture-project",team_run_id:team.latest_run.id,member_run_id:"member-run-mira",member_run_generation:3,agent_session_id:"session-mira-current",agent_session_generation:3},host:{execution_space_id:"fixture-space",project_id:"fixture-project",team_run_id:team.latest_run.id,member_run_id:"member-run-host",member_run_generation:1,agent_session_id:"host-thread-current",agent_session_generation:1}}});if(!liveConfig)await next.route("**/v1/**",async route=>{
    const request=route.request(),url=new URL(request.url());
    const token=request.headers()["x-agentfirm-token"];
    if(request.method()==="POST")return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify({ok:true})});
    if(url.pathname==="/v1/events"){
      assert.ok(token,"private live stream must carry X-AgentFirm-Token");
      const scope=token==="fixture-host-token"?{execution_space_id:"fixture-space",project_id:"fixture-project",team_run_id:team.latest_run.id,member_run_id:"member-run-host",member_run_generation:1,agent_session_id:"host-thread-current",agent_session_generation:1}:{execution_space_id:"fixture-space",project_id:"fixture-project",team_run_id:team.latest_run.id,member_run_id:"member-run-mira",member_run_generation:3,agent_session_id:"session-mira-current",agent_session_generation:3};
      const terminal={schema_version:"agentfirm.live_provider_activity_event.v1",reason:"terminal",scope,activity:null};
      return route.fulfill({status:200,contentType:"text/event-stream",body:`event: live_provider_activity\ndata: ${JSON.stringify(terminal)}\n\n`});
    }
    let body;
    if(url.pathname==="/v1/meta")body={schema_version:"agentfirm.role_views.v1",protocol_version:"agentfirm-member-trust/1",action_manifest_version:"agentfirm.role_actions.v1",capability_auth:"x-agentfirm-token",build_sha:"b362bc1ba1ebbeff26eb9a4a08bf3c6982ec764d"};
    else if(url.pathname==="/v1/projects")body={projects:[{id:"fixture-project",is_current:true}]};
    else if(url.pathname==="/v1/spaces")body={spaces:[{id:"fixture-space",is_current:true}]};
    else if(url.pathname==="/v1/companies")body={companies:[]};
    else if(url.pathname==="/v1/snapshot"||url.pathname.includes("/snapshot"))body={generated_at:"2026-08-12T08:10:00Z",execution_space_id:"fixture-space",teams:[],team_runs:[],member_runs:[],execution_nodes:[],company_os:{}};
    else if(url.pathname.includes("team-workspace"))body=teamWorkspace;
    else if(url.pathname.includes("agent-workspace")){
      const memberSelected=url.searchParams.get("agent_id")==="agent-mira"||url.pathname.endsWith("member-run-mira");
      if(memberSelected&&token==="fixture-member-token"&&firstMemberWorkspaceRequestAt===null)firstMemberWorkspaceRequestAt=Date.now();
      if(memberSelected&&token==="fixture-member-token")await new Promise(resolve=>setTimeout(resolve,40));
      if(memberSelected&&token==="fixture-member-token"&&failMemberRefresh)return route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:{message:"fixture member refresh failed"}})});
      if(memberSelected&&token==="fixture-host-token"){
        if(hostMemberProjectionGate)await hostMemberProjectionGate;
        if(failHostMemberProjection)return route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:{message:"fixture public projection failed"}})});
      }
      body=memberSelected?(token==="fixture-member-token"?memberView:hostMemberPublic):hostView;
    }
    else { unknownFixturePaths.push(url.pathname); return route.fulfill({status:404,contentType:"application/json",body:JSON.stringify({error:{message:url.pathname}})}); }
    return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(body)});
  });return next;};
  const page=await makePage(liveConfig?.memberToken??"fixture-member-token");
  const open=async(target,url)=>{await target.goto(url,{waitUntil:"domcontentloaded"});await target.getByTestId("agent-workspace").waitFor();assert.equal(await target.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"desktop horizontal overflow");};
  const waitForStableWriteSurface=async target=>{await target.getByText("Authoritative Agent Workspace refresh is pending or failed. Composer and action writes are unavailable.",{exact:true}).waitFor({state:"detached"});};
  const routeState=liveConfig?{teamRun:liveConfig.teamRun,member:liveConfig.member,memberRun:liveConfig.memberRun,host:liveConfig.host,space:liveConfig.space,project:liveConfig.project}:{teamRun:team.latest_run.id,member:"agent-mira",memberRun:"member-run-mira",host:"agent-host",space:"fixture-space",project:"fixture-project"};
  await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
  if(!liveConfig){assert.ok(firstMemberWorkspaceRequestAt!==null,"owner Agent Workspace request never started");assert.ok(Date.now()-firstMemberWorkspaceRequestAt<900,"continuous snapshot churn cancelled the first owner Agent Workspace load");}
  if(!liveConfig){const liveSlot=page.locator(".aw-current-execution");await liveSlot.waitFor();await liveSlot.getByText("Codex tool started",{exact:true}).waitFor();await page.screenshot({path:join(evidenceDir,`member-live--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});await page.waitForTimeout(150);assert.equal(await liveSlot.count(),1,"stale Session terminal cleared the current exact Session activity");await liveSlot.waitFor({state:"detached",timeout:2000});await page.getByText(/live stream disconnected/).waitFor();}
  await page.getByRole("button",{name:/Open .* configuration/}).waitFor();
  assert.deepEqual(await page.locator(".agent-roster-name").evaluateAll(nodes=>nodes.filter(node=>node.scrollWidth>node.clientWidth).map(node=>node.textContent)),[],"Agent roster identity is visually clipped");
  assert.deepEqual(await page.locator(".agent-roster-meta").evaluateAll(nodes=>nodes.filter(node=>node.scrollHeight>node.clientHeight+1).map(node=>node.textContent)),[],"Agent roster role/runtime meta wraps beyond its single-line row");
  if(!liveConfig){const hostMeta=await page.locator(".agent-roster-row").first().locator(".agent-roster-meta").textContent();assert.ok(hostMeta?.includes("External · unmanaged"),"external-interactive Host roster row must not masquerade as Running");assert.ok(!hostMeta?.includes("Running"),"external-interactive Host roster row still shows Running");}
  assert.deepEqual(await page.locator(".aw-context-work-title, .aw-context-work-row-title, .aw-context-work-row-meta").evaluateAll(nodes=>nodes.filter(node=>node.scrollWidth>node.clientWidth).map(node=>node.textContent)),[],"Current or assigned Work is visually clipped");
  assert.deepEqual(await page.locator(".agent-team-composer span[title]").evaluateAll(nodes=>nodes.filter(node=>node.scrollWidth>node.clientWidth).map(node=>node.textContent)),[],"Composer recipient target is visually clipped");
  if(!liveConfig){await page.getByText(/Implemented the owner-bound Session projection/).waitFor();await page.locator(".aw-native-facts-trail .aw-stream-fact__trigger").first().waitFor();assert.ok(await page.locator(".aw-native-facts-trail .aw-stream-fact__trigger").count()>=3,"native observations are not presented as individual expandable event rows");}
  if(!liveConfig)assert.equal(await page.getByTestId("agent-workspace-identity").getByText("Native Session unavailable",{exact:true}).count(),0,"available historical Session projection is mislabeled as unavailable without an active current_session");
  if(!liveConfig){
    assert.equal(await page.getByRole("button",{name:/Open native Session response from Mira Chen/}).count(),1,"provider-native authored content is not presented as a primary readable Session record");
    assert.ok(await page.locator('.aw-stream-fact[data-family="tool"]').count()>=2,"canonical tool events do not use the shared Tool presentation family");
    assert.equal(await page.locator('.aw-stream-fact[data-family="tool"] .aw-stream-kind').first().getAttribute("data-tone"),"neutral","completed Tool activity should remain a quiet operational fact, not delivery-success chrome");
  }
  assert.equal(await page.locator(".aw-current-execution").count(),0,"Agent Workspace fabricated a current-execution preview without a private live projection");
  assert.equal(await page.locator('[data-testid="agent-workspace"]').evaluate(node=>node.textContent?.includes("Live · transient")??false),false,"legacy live member activity entered the private provider execution slot without exact Session generation scope");
  await waitForStableWriteSurface(page);
  const composerAlignment=await page.getByTestId("agent-workspace-composer").evaluate(composer=>{const bounds=composer.getBoundingClientRect();const send=[...composer.querySelectorAll('button')].find(node=>node.textContent?.trim()==="Send");const sendBounds=send?.getBoundingClientRect();const parent=send?.parentElement;const parentBounds=parent?.getBoundingClientRect();const style=send?getComputedStyle(send):null;return {composerRight:bounds.right,sendRight:sendBounds?.right??0,gap:sendBounds?bounds.right-sendBounds.right:null,parentClass:parent?.className,parentDisplay:parent?getComputedStyle(parent).display:null,parentLeft:parentBounds?.left,parentRight:parentBounds?.right,marginLeft:style?.marginLeft,position:style?.position};});
  assert.ok(composerAlignment.gap!=null&&composerAlignment.gap<=24,`Composer primary action does not close the command surface at the right edge: ${JSON.stringify(composerAlignment)}`);
  await page.screenshot({path:join(evidenceDir,`member-session--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  if(!liveConfig){
    await page.getByText("Validated owner-bound native Session",{exact:true}).waitFor();
    const eventRow=page.getByRole("button",{name:/Validated owner-bound native Session/});
    await eventRow.click();assert.equal(await eventRow.getAttribute("aria-expanded"),"true","clicking an individual event row expands it in place");
    await page.getByText("Native observation in focus",{exact:true}).waitFor();
    const authoredMessage=page.getByRole("button",{name:/Open authored Message from Marcus Allen/}).first();
    await authoredMessage.focus();await page.keyboard.press("Enter");await page.getByText("Message in focus",{exact:true}).waitFor();
    const nativeMessage=page.locator('.aw-stream-fact__trigger').first();
    await nativeMessage.focus();await page.keyboard.press("Space");await page.getByText("Native observation in focus",{exact:true}).waitFor();
    const toolRow=page.getByRole("button",{name:/Validated owner-bound native Session/});await toolRow.focus();await page.keyboard.press("Enter");assert.equal(await toolRow.getAttribute("aria-expanded"),"true","event row keyboard selection");
    await toolRow.locator('xpath=ancestor::div[@data-boundary-aligned="true"]').waitFor();
    assert.equal(await toolRow.locator('xpath=ancestor::article').getAttribute("data-selected"),"true","selected event does not retain a stable center-row state");
    assert.equal(await page.getByText("Native observation in focus",{exact:true}).count(),1,"selected event detail is not delegated to the context rail");
    await page.locator('textarea[aria-label="Message"]').fill("Trigger a benign background revalidate");
    await page.getByRole("button",{name:"Send",exact:true}).click();
    await page.getByText("Message recorded. Refreshing this Agent Workspace.",{exact:true}).waitFor();
    await page.waitForLoadState("networkidle");
    assert.equal(await page.getByText("Native observation in focus",{exact:true}).count(),1,"background revalidate dropped the selected native observation");
    assert.equal(await toolRow.locator('xpath=ancestor::article').getAttribute("data-selected"),"true","background revalidate dropped the selected stream row state");
    await page.screenshot({path:join(evidenceDir,`member-event-detail--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  }
  await page.getByRole("tab",{name:/Messages/}).click();await page.getByRole("button",{name:"inbox",exact:true}).waitFor();
  await waitForStableWriteSurface(page);
  const visibleTogether=await page.evaluate(()=>{
    const selectors=['[data-testid="agent-workspace-identity"]','[data-testid="agent-workspace-modebar"]','[role="tabpanel"][data-state="active"]','[data-testid="agent-workspace-composer"]'];
    return selectors.map(selector=>{const node=document.querySelector(selector);const rect=node?.getBoundingClientRect();return {selector,visible:Boolean(rect&&rect.top>=0&&rect.bottom<=window.innerHeight&&rect.width>0&&rect.height>0)};});
  });
  assert.deepEqual(visibleTogether.filter(item=>!item.visible),[],`Messages mode lost normative first viewport after event expansion: ${JSON.stringify(visibleTogether)}`);
  assert.equal(await page.locator('[role="tabpanel"][data-state="active"] [data-radix-scroll-area-viewport]').evaluate(node=>node.scrollTop),0,"Messages canvas did not reset to top after mode change");
  if(!liveConfig){
    const recordKind=page.locator(".aw-record-kind").first();
    const recordStyle=await recordKind.evaluate(node=>{const style=getComputedStyle(node);return {size:parseFloat(style.fontSize),weight:Number(style.fontWeight),color:style.color,clipped:node.scrollWidth>node.clientWidth};});
    const actorStyle=await page.locator(".agent-message-row b").first().evaluate(node=>{const style=getComputedStyle(node);return {size:parseFloat(style.fontSize),weight:Number(style.fontWeight),color:style.color};});
    assert.ok(recordStyle.size<actorStyle.size&&recordStyle.weight<actorStyle.weight,"Message route metadata competes with the authored actor");
    assert.equal(recordStyle.clipped,false,"Message route metadata is clipped");
  }
  await page.screenshot({path:join(evidenceDir,`member-messages--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  await page.getByRole("tab",{name:/Work/}).click();
  if(!liveConfig)await page.getByRole("button",{name:/Restore authored conversation dominance/}).waitFor();
  else await page.locator(".agent-work-row").first().waitFor();
  await waitForStableWriteSurface(page);
  if(!liveConfig)assert.equal(await page.locator('.agent-work-row[data-current="true"]').first().getByText("Restore authored conversation dominance",{exact:true}).count(),1,"Current Work emphasis does not follow context_summary.current_work_id");
  if(!liveConfig){
    await page.locator('.agent-work-row[data-current="true"]').first().click();
    await page.getByText("Work in focus",{exact:true}).waitFor();
    assert.ok(await page.locator('.aw-context-selection-inset').getByText(/rev 4 \(latest\)/).count()===1,"selected Work detail does not expose its canonical revision");
    assert.equal(await page.locator('.aw-context-selection-inset .aw-fact-row').filter({hasText:"Gates"}).filter({hasText:"1/2"}).count(),1,"selected Work detail does not expose gate progress");
  }
  if(!liveConfig){
    const clippedWorkRows=await page.locator('.agent-work-row').evaluateAll(rows=>rows.filter(row=>{const rect=row.getBoundingClientRect();return rect.top<window.innerHeight&&rect.bottom>window.innerHeight;}).map(row=>row.textContent?.trim().slice(0,80)));
    assert.deepEqual(clippedWorkRows,[],`Work first viewport ends on a partially obscured responsibility: ${JSON.stringify(clippedWorkRows)}`);
  }
  await page.screenshot({path:join(evidenceDir,`member-work--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  const profileOpener=page.getByRole("button",{name:/Open .* configuration/});
  await profileOpener.click();await page.getByRole("dialog").waitFor();
  assert.equal(await page.getByRole("dialog").getByText(/\b(?:none|null|not_model(?:ed)?|not modeled)\b/i).count(),0,"Configuration exposes raw empty-model tokens as primary UI");
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
    assert.equal(await page.locator(".aw-current-execution").count(),0,"reconnecting replayed volatile provider activity from the previous Session stream");
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
  if(liveConfig&&(await hostPage.locator(".agent-authored-turn").count())>0)await hostPage.locator(".agent-authored-turn").first().waitFor();
  await waitForStableWriteSurface(hostPage);
  if(!liveConfig){assert.equal(await hostPage.getByText("Validated owner-bound native Session",{exact:true}).count(),0,"Member-private provider event leaked into Host Session");assert.equal(await hostPage.getByText("Read Lead inbox",{exact:true}).count(),1,"Host-native event missing");}
  if(!liveConfig){
    await hostPage.getByText("Current decision",{exact:true}).waitFor();
    await hostPage.getByText("Team Inbox",{exact:true}).waitFor();
    await hostPage.getByText("Current Session",{exact:true}).waitFor();
    await hostPage.getByText("Decision actions",{exact:true}).waitFor();
    assert.equal(await hostPage.getByText(/agent-mira/).count(),0,"Host decision rail exposes a raw AgentMember id instead of the canonical display identity");
    assert.ok(await hostPage.locator('[data-testid="agent-workspace-identity"] .aw-header-chip',{hasText:"External · unmanaged"}).count()>=1,"external-interactive Host header is missing the unmanaged chip");
  }
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
  await hostPage.getByText("Privacy",{exact:true}).waitFor();
  if(liveConfig&&(await hostPage.locator(".agent-authored-turn").count())>0)await hostPage.locator(".agent-authored-turn").first().waitFor();
  await waitForStableWriteSurface(hostPage);
  assert.equal(await hostPage.getByText("Validated owner-bound native Session",{exact:true}).count(),0,"Host-selected Member leaked provider-private activity");
  assert.equal(await hostPage.locator(".aw-current-execution").count(),0,"Host-selected Member exposed or fabricated a private current-execution preview");
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
  const unexpectedConsoleErrors=consoleErrors.filter(message=>!message.includes("503 (Service Unavailable)")&&!message.includes("404"));
  const unexpectedHttpFailures=httpFailures.filter(failure=>!failure.startsWith("404 https://fonts.gstatic.com/"));
  assert.deepEqual(unexpectedConsoleErrors,[],`unexpected console errors: ${consoleErrors.join(" | ")}; HTTP failures: ${httpFailures.join(" | ")}; unknown fixture paths: ${unknownFixturePaths.join(", ")}`);
  assert.deepEqual(unexpectedHttpFailures.filter(failure=>!failure.startsWith("503 ")),[],`unexpected HTTP failures: ${httpFailures.join(" | ")}`);

  for(const viewport of [{width:900,height:1180},{width:390,height:844},{width:320,height:844}]){
    await page.setViewportSize(viewport);
    await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,`${viewport.width}px horizontal overflow`);
    if(viewport.width===390){
      await page.getByRole("button",{name:"Open Agent roster"}).click();
      await page.getByRole("dialog",{name:"Agent roster"}).waitFor();
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"390px roster sheet overflow");
      await page.getByRole("button",{name:"Close Agent roster"}).click();
      await page.getByRole("button",{name:"Open Agent context"}).click();
      await page.getByRole("dialog",{name:"Agent context"}).waitFor();
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"390px context sheet overflow");
      await page.getByRole("button",{name:"Close Agent context"}).click();
    }
  }
  const captures=[];
  for(const name of [...(!liveConfig?["member-live"]:[]),"member-session",...(!liveConfig?["member-event-detail"]:[]),"member-messages","member-work","member-configuration","host-session","host-member-public"]){
    const file=`${name}--1440x1000--${capturedSourceSha}.png`;
    captures.push({name,file,viewport:{width:1440,height:1000},sha256:createHash("sha256").update(await readFile(join(evidenceDir,file))).digest("hex")});
  }
  const liveMeta=liveConfig?await fetch(`${liveConfig.api}/v1/meta?space=${encodeURIComponent(liveConfig.space)}&project=${encodeURIComponent(liveConfig.project)}`).then(async response=>{assert.equal(response.ok,true,`live meta ${response.status}`);return response.json();}):null;
  if(liveMeta){
    assert.equal(liveMeta.build_sha,capturedSourceSha,"live server build SHA differs from frozen frontend SHA");
    assert.equal(liveMeta.git_rev,capturedSourceSha,"live server git revision differs from frozen frontend SHA");
    assert.equal(await page.getByText(/Stale build:/).count(),0,"Member live evidence displays a stale-build warning");
    assert.equal(await hostPage.getByText(/Stale build:/).count(),0,"Host live evidence displays a stale-build warning");
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
    runtime_revisions:{frontend:captureSourceRevision(capturedSourceSha),server_build:liveMeta?.build_sha??"fixture-server",server_git:liveMeta?.git_rev??"fixture-server"},
    canonical_objects:liveConfig?{team_run_id:liveConfig.teamRun,host_agent_member_id:liveConfig.host,member_agent_member_id:liveConfig.member,member_run_id:liveConfig.memberRun}:null,
    native_session_claim:liveConfig?{availability:liveMemberEvidence?.data?.session_event_projection?.agent_session_id?"available":"unavailable",native_session_id:liveMemberEvidence?.data?.session_event_projection?.agent_session_id??null,claim:liveMemberEvidence?.data?.session_event_projection?.agent_session_id?"owner-bound provider-native Session projection captured":"not claimed: no bound provider-native Session in the canonical store"}:"deterministic fixture only",
    captures,
  };
  await writeFile(join(evidenceDir,"evidence-manifest.json"),`${JSON.stringify(manifest,null,2)}\n`);
  console.log(`agent workspace browser check: PASS (${liveConfig?"store-live":"fixture"}; ${evidenceDir})`);
}finally{await browser.close();await vite.close();}

function captureSourceRevision(value){return value==="working-tree"?value:String(value);}
