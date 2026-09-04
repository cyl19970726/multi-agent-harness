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
const baseWork={work_id:"work-agent-workspace-1",work_revision:4,team_id:team.team_id,mission_id:team.mission_id,title:"Restore authored conversation dominance",context_markdown:"Match the approved Agent Workspace composition.",completion_criteria_markdown:"Host and Member share one shell; provider-native events remain Team-scoped.",claim_mode:"host_assign",eligible_member_ids:["agent-mira"],prerequisite_work_ids:[],successor_work_ids:[],readiness:{state:"not_claimable",reason_codes:["fixture_state"],unsatisfied_prerequisite_work_ids:[],failed_or_cancelled_prerequisite_work_ids:[]},blocker_reason:null,result_summary:null,artifact_refs:[],check_refs:[],latest_event:{id:"event-4",kind:"started",actor_ref:{kind:"agent_member",id:"agent-mira"},created_at:"2026-08-12T08:02:00Z"},owner_actor_ref:{kind:"agent_member",id:"agent-mira"},current_member_run_ref:"member-run-mira",phase:"active",condition:"normal",resolution:null,priority:"urgent",module_refs:[],gate_summary:{required:2,passed:1,failed:0,pending:1,waived:0,stale:0},latest_report_ref:null,latest_finding_refs:[],latest_failure_ref:null,delivery_summary:{queued:0,claimed:0,provider_received:1,failed:0,expired:0,invalidated:0,recovery_class:"none"},runtime_summary:{state:"running",generation:3,freshness:"current"},workspace_summary:{binding_id:"workspace-mira",lifecycle:"attached",safety:"safe"},delegation_summary:{incoming:0,outgoing:0,attention:false},updated_at:"2026-08-12T08:08:00Z"};
const works=[
  baseWork,
  {...baseWork,work_id:"work-agent-workspace-2",title:"Verify Team session access",work_revision:2,phase:"review",priority:"high",completion_criteria_markdown:"Host view renders the selected Team Member's complete provider-native Session.",latest_event:{...baseWork.latest_event,id:"event-5",kind:"submitted"}},
  {...baseWork,work_id:"work-agent-workspace-3",title:"Polish interaction and focus states",work_revision:1,phase:"open",priority:"normal",completion_criteria_markdown:"Every interactive row exposes a visible focus ring and hover affordance.",latest_event:{...baseWork.latest_event,id:"event-6",kind:"assigned"}},
  {...baseWork,work_id:"work-agent-workspace-4",title:"Calibrate type scale and record rhythm",work_revision:3,phase:"active",priority:"high",completion_criteria_markdown:"Body text and metadata separate by weight and contrast instead of tiny-size differences.",latest_event:{...baseWork.latest_event,id:"event-7",kind:"started"}},
  {...baseWork,work_id:"work-agent-workspace-5",title:"Converge the selection-aware context rail",work_revision:2,phase:"open",priority:"normal",completion_criteria_markdown:"Rail sections follow the current selection; empty sections reserve no decorative space.",latest_event:{...baseWork.latest_event,id:"event-8",kind:"assigned"}},
  {...baseWork,work_id:"work-agent-workspace-6",title:"Freeze exact-source desktop evidence",work_revision:5,phase:"review",priority:"urgent",completion_criteria_markdown:"Session, Messages and Work screenshots all bind to one exact revision in the final bundle.",latest_event:{...baseWork.latest_event,id:"event-9",kind:"submitted"}},
  {...baseWork,work_id:"work-agent-workspace-7",title:"Preserve compact viewport interaction",work_revision:1,phase:"open",priority:"low",completion_criteria_markdown:"Compact viewports keep roster navigation and the composer reachable without clipping.",latest_event:{...baseWork.latest_event,id:"event-10",kind:"created"}},
];
const messages=[
  {message_id:"message-1",work_id:baseWork.work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Keep the authored exchange primary. Compact tool and runtime facts underneath it.",kind:"message",correlation_id:"conversation-1",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:00:00Z",deliveries:[{id:"delivery-1",recipient_member_run_id:"member-run-mira",status:"provider_received",version:2,provider_receipt_id:"receipt-1",updated_at:"2026-08-12T08:00:02Z"}]},
  {message_id:"message-2",work_id:baseWork.work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"Implemented the Team-scoped Session projection. The Host can inspect this Member's native events.",kind:"message",correlation_id:"conversation-1",causation_id:"message-1",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:04:00Z",deliveries:[]},
  {message_id:"message-3",work_id:works[1].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Please include exact-SHA Session, Messages and Work screenshots in the final bundle.",kind:"message",correlation_id:"conversation-2",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:06:00Z",deliveries:[{id:"delivery-3",recipient_member_run_id:"member-run-mira",status:"queued",version:1,provider_receipt_id:null,updated_at:"2026-08-12T08:06:00Z"}]},
  {message_id:"message-4",work_id:works[3].work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"Body text and metadata now separate by weight and contrast instead of tiny-size differences.",kind:"message",correlation_id:"conversation-3",causation_id:null,response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:08:00Z",deliveries:[]},
  {message_id:"message-5",work_id:works[4].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Keep the context rail limited to facts that change the selected Agent or Work decision.",kind:"message",correlation_id:"conversation-4",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:09:00Z",deliveries:[{id:"delivery-5",recipient_member_run_id:"member-run-mira",status:"provider_received",version:2,provider_receipt_id:"receipt-5",updated_at:"2026-08-12T08:09:02Z"}]},
  {message_id:"message-6",work_id:works[4].work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"The rail is now selection-aware; empty sections do not reserve decorative space.",kind:"message",correlation_id:"conversation-4",causation_id:"message-5",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:11:00Z",deliveries:[]},
  {message_id:"message-7",work_id:works[5].work_id,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Freeze the full desktop family only after Session, Messages, Work and configuration read as one product.",kind:"message",correlation_id:"conversation-5",causation_id:null,response_intent:"response_required",reply_eligible:true,created_at:"2026-08-12T08:12:00Z",deliveries:[{id:"delivery-7",recipient_member_run_id:"member-run-mira",status:"queued",version:1,provider_receipt_id:null,updated_at:"2026-08-12T08:12:00Z"}]},
  {message_id:"message-8",work_id:works[5].work_id,sender:{kind:"agent_member",id:"agent-mira"},recipients:[{kind:"agent_member",id:"agent-host"}],body:"The complete frame set is ready for exact-revision self-review; no page was submitted independently.",kind:"message",correlation_id:"conversation-5",causation_id:"message-7",response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:14:00Z",deliveries:[]},
  {message_id:"message-9",work_id:"work-outside-view",sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"Keep this linked context visible even when the Work record is outside the selected projection.",kind:"message",correlation_id:"conversation-6",causation_id:null,response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:15:00Z",deliveries:[]},
  {message_id:"message-10",work_id:null,sender:{kind:"agent_member",id:"agent-host"},recipients:[{kind:"agent_member",id:"agent-mira"}],body:"This is genuinely unlinked coordination.",kind:"message",correlation_id:"conversation-7",causation_id:null,response_intent:"informational",reply_eligible:false,created_at:"2026-08-12T08:16:00Z",deliveries:[]},
];
const actions=[
  {kind:"send_message",target_ref:{kind:"team_run",id:team.latest_run.id},required_version:5,disabled_reason:null},
  {kind:"assign_work",target_ref:{kind:"work",id:baseWork.work_id},required_version:4,disabled_reason:null},
  {kind:"request_gate_evaluation",target_ref:{kind:"work",id:works[1].work_id},required_version:2,disabled_reason:null},
  {kind:"close_member_run",target_ref:{kind:"member_run",id:"member-run-mira"},required_version:3,disabled_reason:null},
];
const configuration={description:"Owns the frontend implementation and exact-source validation.",prompt_ref:null,prompt_projection:"not_modeled",skill_refs:["harness-frontend-product-design","frontend-visual-contract"],capabilities:["workspace_write","browser_acceptance","source_review"],tool_refs:[],tools_projection:"not_modeled_by_agent_member",provider_profile_ref:"codex-app-server-v1",model_preference:"gpt-5",workspace_policy:"isolated_worktree",permission_ceiling:"full_access",forbidden_actions:[],forbidden_actions_projection:"not_modeled",workspace_binding:{kind:"workspace_binding",id:"workspace-mira",work_id:baseWork.work_id,member_run_id:"member-run-mira",requirement_id:null,status:"attached",version:2,actor_ref:null,summary:null,created_at:"2026-08-12T07:00:00Z",source_id:null,target_id:null,locator:"/fixture/worktree"}};
const sourceGeneration="source-generation:fixture-a";
const observation=(id,semanticKind,payload,at,position,nativeEvent={type:semanticKind,payload})=>({schema_version:"agentfirm.provider_native_event_record.v3",record_id:`native-row:sha256:${createHash("sha256").update(`${sourceGeneration}\0row-locator:${id}\0`).digest("hex")}`,provider:"codex",adapter_version:"agentfirm.persisted_provider_event_adapter.v3",native_source_ref:"provider-source:fixture-a",source_generation:sourceGeneration,row_locator:`row-locator:${id}`,ordering_key:{kind:"complete_row_end_offset",value:position},agent_member_id:"agent-mira",agent_session_id:"session-mira-current",agent_session_generation:3,provider_thread_id:"thread-mira",provider_turn_id:"turn-mira-1",provider_event_id:id,occurred_at:at,observed_at:at,native_event:nativeEvent,source_content_fingerprint:`sha256:${createHash("sha256").update(id).digest("hex")}`,fragments:[{fragment_id:`${id}:fragment-0`,fragment_index:0,semantic_kind:semanticKind,lifecycle_phase:payload.outcome==="requested"?"requested":"terminal",completeness:"complete",content_availability:"available",payload}]});
const memberObservations=[
  observation("native-0","assistant_response",{type:"assistant_response",text:"I mapped the Agent Workspace read model to the approved composition and started with the privacy boundary."},"2026-08-12T07:58:00Z",10),
  observation("native-1","tool_call_requested",{type:"tool",tool_name:"Read",call_id:"call-1",parent_call_id:"turn-mira-1",operation_category:"read",primary_target:"AGENTS.md",arguments:{availability:"available",json_pointer:"/item/arguments"},outcome:"requested"},"2026-08-12T08:01:00Z",11,{type:"item.started",item:{type:"tool_call",name:"Read",arguments:{path:"AGENTS.md",line_start:1,line_end:120}}}),
  observation("native-2","tool_call_completed",{type:"tool",tool_name:"Read",call_id:"call-1",parent_call_id:"turn-mira-1",operation_category:"read",primary_target:"AGENTS.md",result:{availability:"available",json_pointer:"/item/result"},outcome:"completed"},"2026-08-12T08:03:00Z",12,{type:"item.completed",item:{type:"tool_call",name:"Read",result:{lines:120,summary:"Canonical operating rules loaded."}}}),
  observation("native-3","tool_call_failed",{type:"tool",tool_name:null,tool_name_unavailable_reason:"related_record_missing",call_id:"orphan-call",operation_category:"command",primary_target:"unknown command",error:{availability:"available",json_pointer:"/error"},outcome:"failed"},"2026-08-12T08:05:00Z",13,{type:"item.completed",call_id:"orphan-call",error:{code:"ENOENT",message:"Requested command was not found."}}),
  observation("native-4","turn_completed",{type:"turn",outcome:"completed",display_summary:"Frontend build passed for the unified three-column shell."},"2026-08-12T08:07:00Z",14),
  observation("native-5","tool_call_completed",{type:"tool",tool_name:null,tool_name_unavailable_reason:"related_record_missing",call_id:null,operation_category:"other",primary_target:null,result:{availability:"available",json_pointer:"/message/content/0/text"},outcome:"completed",display_detail:"Provider omitted the exact tool-call discriminator; this result remains independent."},"2026-08-12T08:08:00Z",15,{type:"message",message:{role:"toolResult",content:[{type:"text",text:"expected-tool-error\nCommand exited with code 7"}]}}),
].map((event,index)=>({...event,occurred_at:null,observed_at:["2026-08-12T08:00:00Z","2026-08-12T08:01:00Z","2026-08-12T09:02:00Z","2026-08-12T08:03:00Z","2026-08-12T08:03:30Z","2026-08-12T08:08:00Z"][index]}));
memberObservations[0].occurred_at="1";
const memberProjection={schema_version:"agentfirm.native_session_read.v1",available:true,native_source_ref:"provider-source:fixture-a",source_generation:sourceGeneration,snapshot_watermark:{kind:"complete_row_end_offset",value:15},records:memberObservations,has_more:true,next_before:{source_generation:sourceGeneration,ordering_key:{kind:"complete_row_end_offset",value:10}},incomplete_tail:false,source_reset:false};
const memberCurrentSession={agent_session_id:"session-mira-current",agent_session_generation:3,lifecycle:"active",runtime_residency:"attached",activity:"idle",provider:"codex",effective_permission_ceiling:"full_access",workspace_cwd:"/fixture/worktree",native_session_ref:{native_session_id:"thread-mira",provider:"codex",execution_mode:"codex_app_server"},native_session_open_target:null};
const runtimeTruth={work:{work_id:baseWork.work_id,phase:"active",condition:"normal",updated_at:"2026-08-12T08:00:00Z"},coordination:{state:"active",member_run_id:"member-run-mira",runtime_generation:3,runtime_status:"blocked"},harness_control:{state:"recovery_required",reason_code:"PROVIDER_IDLE_TIMEOUT",occurred_at:"2026-08-12T08:04:00Z",last_command:{id:"runtime-command-mira-1",command:"start_cycle",status:"recovery_required",updated_at:"2026-08-12T08:04:00Z",failure_code:"PROVIDER_IDLE_TIMEOUT"},next_action:"Resolve the exact RuntimeCommand from evidence; do not replay blindly."},provider_native_activity:{state:"observed",last_observed_at:"2026-08-12T09:02:00Z",observed_after_control_loss:true},explanation:"Harness control is recovery_required (PROVIDER_IDLE_TIMEOUT). Provider-native activity was observed afterward; it does not prove recovery or Work completion."};
const privateBase={projection_scope:"team_session_read",team:{...team,viewer_role:"member"},selected_agent:{agent_member_ref:member.agent_member_ref,display_name:member.display_name,role:member.role,organization_status:"active",is_host:false,current_member_run_ref:"member-run-mira",provider:"codex",execution_mode:"codex_app_server",runtime_status:"running",runtime_generation:3,host_session_mode:null},roster,current_session:memberCurrentSession,persisted_session_projection:memberProjection,runtime_truth:runtimeTruth,messages,works,configuration,context_summary:{current_work_id:baseWork.work_id,message_count:messages.length,unread_count:1,last_activity_at:"2026-08-12T08:07:00Z",authorization_count:actions.length}};
const memberView=envelope("agent_workspace",privateBase,actions);
memberView.data.team={team_id:team.team_id,display_name:team.display_name,team_revision:team.team_revision,mission_id:team.mission_id,host_agent_id:team.host_agent_id,viewer_role:"member",status:team.status,latest_run_id:team.latest_run.id};
const olderWithoutProviderTimestamp={...observation("native-earlier","assistant_response",{type:"assistant_response",text:"Earlier exact provider-native event loaded from the same native Session."},"2026-08-12T10:00:00Z",1),occurred_at:null};
const olderMemberView=envelope("agent_workspace",{...memberView.data,persisted_session_projection:{...memberProjection,records:[olderWithoutProviderTimestamp],has_more:false,next_before:null,snapshot_watermark:{kind:"complete_row_end_offset",value:1}}},actions);
const otherProjectSourceGeneration="source-generation:fixture-b";
const otherProjectObservation={...observation("native-other-project","assistant_response",{type:"assistant_response",text:"Exact native event from the second Project only."},"2026-08-12T11:00:00Z",20),source_generation:otherProjectSourceGeneration,native_source_ref:"provider-source:fixture-b",record_id:`native-row:sha256:${createHash("sha256").update(`${otherProjectSourceGeneration}\0row-locator:native-other-project\0`).digest("hex")}`};
const otherProjectMemberView=envelope("agent_workspace",{...memberView.data,persisted_session_projection:{...memberProjection,native_source_ref:"provider-source:fixture-b",source_generation:otherProjectSourceGeneration,records:[otherProjectObservation],has_more:false,next_before:null,snapshot_watermark:{kind:"complete_row_end_offset",value:20}}},actions);
const resetSourceGeneration="source-generation:fixture-reset";
const resetObservation={...observation("native-after-reset","assistant_response",{type:"assistant_response",text:"Authoritative native row after provider source reset."},null,30),source_generation:resetSourceGeneration,native_source_ref:"provider-source:fixture-reset",record_id:`native-row:sha256:${createHash("sha256").update(`${resetSourceGeneration}\0row-locator:native-after-reset\0`).digest("hex")}`,observed_at:"2026-08-12T12:00:00Z"};
const resetProjection={...memberProjection,native_source_ref:"provider-source:fixture-reset",source_generation:resetSourceGeneration,records:[resetObservation],has_more:false,next_before:null,snapshot_watermark:{kind:"complete_row_end_offset",value:30},source_reset:true};
const otherAgentView=envelope("agent_workspace",{...otherProjectMemberView.data,selected_agent:{...otherProjectMemberView.data.selected_agent,agent_member_ref:analyst.agent_member_ref,display_name:analyst.display_name,role:analyst.role,current_member_run_ref:analyst.current_member_run_ref,provider:analyst.provider}},actions);
const hostMessages=messages;
const hostObservations=[observation("host-native-0","assistant_response",{type:"assistant_response",text:"I reviewed the current decision surface and sent the next bounded assignment."},"2026-08-12T08:05:00Z",1),observation("host-native-1","tool_call_completed",{type:"tool",tool_name:"Read Lead inbox",call_id:"host-call-1",operation_category:"read",primary_target:"Team inbox",outcome:"completed"},"2026-08-12T08:08:00Z",2)].map(item=>({...item,agent_member_id:"agent-host",agent_session_id:"host-thread-current",agent_session_generation:1,provider_turn_id:"turn-host-1"}));
const hostView=envelope("agent_workspace",{...memberView.data,projection_scope:"team_session_read",selected_agent:{agent_member_ref:{kind:"agent_member",id:"agent-host"},display_name:"Marcus Allen",role:"Team Lead",organization_status:"active",is_host:true,current_member_run_ref:"member-run-host",provider:"codex",execution_mode:"host_native",runtime_status:"active",runtime_generation:1,host_session_mode:"external_interactive"},current_session:{...memberCurrentSession,agent_session_id:"host-thread-current",agent_session_generation:1,native_session_ref:{native_session_id:"host-thread-current",provider:"codex",execution_mode:"host_native"}},persisted_session_projection:{...memberProjection,records:hostObservations,has_more:false,next_before:null},messages:hostMessages,configuration:{...configuration,description:"Owns Team judgment and assignment authority."},context_summary:{current_work_id:works[1].work_id,message_count:hostMessages.length,unread_count:0,last_activity_at:"2026-08-12T08:08:00Z",authorization_count:actions.length}},actions);
const hostMemberTeamRead=envelope("agent_workspace",{...memberView.data,team:{...memberView.data.team,viewer_role:"host"},projection_scope:"team_session_read"},actions);
const hostOtherAgentRead=envelope("agent_workspace",{...otherAgentView.data,team:{...otherAgentView.data.team,viewer_role:"host"},projection_scope:"team_session_read"},actions);
const operatorMemberRead=envelope("agent_workspace",{...memberView.data,team:{...memberView.data.team,viewer_role:"operator"},projection_scope:"team_session_read"},[]);
assert.equal(hostMemberTeamRead.data.persisted_session_projection.records[0].agent_session_id,"session-mira-current","exact Host must receive selected Team Member native history");
const teamWorkspace=envelope("team_workspace",{team,pressure_summary:{active_turns:1,ready_members:1,total_members:2,ready_work:1,review_work:1,blocked_work:0},works,work_graph:{nodes:works,edges:[],ready_work_ids:[],attention_work_ids:[]},members:[member,analyst],messages,activity:[],activity_truncated:false,reports:[],findings:[],failures:[],gate_requirements:[],gate_evaluations:[],gate_waivers:[],workspace_attention:[],delegation_provenance:[],page:{as_of_event_sequence:72,item_count:works.length,next_cursor:null}});
const viewerContext=(token)=>{const host=token==="fixture-host-token";const teams=[{team_id:team.team_id,display_name:team.display_name,viewer_role:host?"host":"member",viewer_agent_member_id:host?"agent-host":"agent-mira",default_conversation:host?"host":"agent-mira",latest_run_id:team.latest_run.id,team_run_ids:[team.latest_run.id],current_member_run_id:host?"member-run-host":"member-run-mira"}];if(token==="fixture-multi-token")teams.push({team_id:"team-second-authorized",display_name:"Second Authorized Team",viewer_role:"member",viewer_agent_member_id:"agent-mira",default_conversation:"agent-mira",latest_run_id:null,team_run_ids:[],current_member_run_id:null});return envelope("viewer_context",{viewer_actor_ref:{kind:"agent_member",id:host?"agent-host":"agent-mira"},teams});};

const vite=await createServer({configFile:join(dashboardRoot,"vite.config.ts"),server:{host:"127.0.0.1",port:0},logLevel:"silent"});
await vite.listen();
const base=`http://127.0.0.1:${vite.httpServer.address().port}`;
const browser=await chromium.launch({headless:true});
try{
  const consoleErrors=[];
  const httpFailures=[];
  const unknownFixturePaths=[];
  const postedActions=[];
  const agentWorkspaceRequests=[];
  let failMemberRefresh=false;
  let memberRefreshRetryGate=null;
  let observeMemberRefreshRetry=null;
  let failHostMemberProjection=false;
  let hostMemberProjectionGate=null;
  let firstMemberWorkspaceRequestAt=null;
  let delayedOlderPageGate=null;
  let failDelayedOlderPage=false;
  let delayedSourceResetGate=null;
  let failNextMessagePost=false;
  const makePage=async token=>{const next=await browser.newPage({viewport:{width:1440,height:1000}});next.on("console",message=>{if(message.type()==="error")consoleErrors.push(message.text());});next.on("pageerror",error=>consoleErrors.push(error.message));next.on("response",response=>{if(response.status()>=400)httpFailures.push(`${response.status()} ${response.url()}`);});await next.addInitScript(({token,fixture})=>{window.__AGENTFIRM_BOOTSTRAP__={capabilityToken:token};class QuietEventSource{timer;started=false;addEventListener(type,listener){if(!fixture||type!=="snapshot"||this.started)return;this.started=true;let revision=0;this.timer=setInterval(()=>{revision+=1;listener({data:JSON.stringify({generated_at:`2026-08-12T08:10:${String(revision).padStart(2,"0")}Z`,execution_space_id:"fixture-space"})});if(revision===80){clearInterval(this.timer);this.timer=undefined;}},15);}close(){if(this.timer)clearInterval(this.timer);}}Object.defineProperty(window,"EventSource",{value:QuietEventSource,configurable:true});},{token,fixture:!liveConfig});if(!liveConfig)await next.route("**/v1/**",async route=>{
    const request=route.request(),url=new URL(request.url());
    const token=request.headers()["x-agentfirm-token"];
    if(request.method()==="POST"){
      postedActions.push({path:url.pathname,body:request.postDataJSON(),headers:request.headers()});
      if(failNextMessagePost&&url.pathname.endsWith("/messages/send")){failNextMessagePost=false;return route.fulfill({status:409,contentType:"application/json",body:JSON.stringify({error:{code:"VERSION_CONFLICT",message:"Authoritative Team revision changed."}})});}
      return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify({ok:true})});
    }
    if(url.pathname==="/v1/events"){
      const selectedAgent=url.searchParams.get("agent_id");
      const sourceResetGate=delayedSourceResetGate;
      if(sourceResetGate&&selectedAgent==="agent-mira")await sourceResetGate;
      const sourceReset=Boolean(sourceResetGate&&selectedAgent==="agent-mira");
      const projection=sourceReset?resetProjection:url.searchParams.get("project")==="fixture-project-b"||selectedAgent==="agent-noah"?otherProjectMemberView.data.persisted_session_projection:selectedAgent==="agent-host"||token==="fixture-host-token"?hostView.data.persisted_session_projection:memberView.data.persisted_session_projection;
      return route.fulfill({status:200,contentType:"text/event-stream",body:`event: ${sourceReset?"native_session_source_reset":"native_session_snapshot"}\ndata: ${JSON.stringify(projection)}\n\n`});
    }
    let body;
    if(url.pathname==="/v1/meta")body={schema_version:"agentfirm.role_views.v1",protocol_version:"agentfirm-member-trust/1",action_manifest_version:"agentfirm.role_actions.v1",capability_auth:"x-agentfirm-token",build_sha:"b362bc1ba1ebbeff26eb9a4a08bf3c6982ec764d"};
    else if(url.pathname==="/v1/projects")body={projects:[{id:"fixture-project",is_current:true},{id:"fixture-project-b",is_current:false}]};
    else if(url.pathname==="/v1/spaces")body={spaces:[{id:"fixture-space",is_current:true}]};
    else if(url.pathname==="/v1/companies")body={companies:[]};
    else if(url.pathname==="/v1/snapshot"||url.pathname.includes("/snapshot"))body={generated_at:"2026-08-12T08:10:00Z",execution_space_id:"fixture-space",teams:[],team_runs:[],member_runs:[],execution_nodes:[],company_os:{}};
    else if(url.pathname==="/v1/views/viewer-context")body=viewerContext(token);
    else if(url.pathname.includes("team-workspace"))body=teamWorkspace;
    else if(url.pathname.includes("agent-workspace")){
      agentWorkspaceRequests.push(url.pathname);
      const memberSelected=url.searchParams.get("agent_id")==="agent-mira"||url.pathname.endsWith("member-run-mira");
      const otherAgentSelected=url.searchParams.get("agent_id")==="agent-noah";
      if(memberSelected&&token==="fixture-member-token"&&firstMemberWorkspaceRequestAt===null)firstMemberWorkspaceRequestAt=Date.now();
      if(memberSelected&&token==="fixture-member-token")await new Promise(resolve=>setTimeout(resolve,40));
      if(memberSelected&&token==="fixture-member-token"&&failMemberRefresh)return route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:{message:"fixture member refresh failed"}})});
      if(memberSelected&&token==="fixture-member-token"&&memberRefreshRetryGate){observeMemberRefreshRetry?.();await memberRefreshRetryGate;}
      if(memberSelected&&token==="fixture-host-token"){
        if(hostMemberProjectionGate)await hostMemberProjectionGate;
        if(failHostMemberProjection)return route.abort("failed");
      }
      if(memberSelected&&url.searchParams.get("session_before")==="10"&&delayedOlderPageGate){await delayedOlderPageGate;if(failDelayedOlderPage)return route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:{message:"stale Project pagination failed"}})});}
      body=otherAgentSelected?(token==="fixture-host-token"?hostOtherAgentRead:otherAgentView):memberSelected?(url.searchParams.get("project")==="fixture-project-b"?otherProjectMemberView:url.searchParams.get("session_before")==="10"?olderMemberView:(token==="fixture-member-token"?memberView:token==="fixture-host-token"?hostMemberTeamRead:operatorMemberRead)):hostView;
    }
    else { unknownFixturePaths.push(url.pathname); return route.fulfill({status:404,contentType:"application/json",body:JSON.stringify({error:{message:url.pathname}})}); }
    return route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(body)});
  });return next;};
  const page=await makePage(liveConfig?.memberToken??"fixture-member-token");
  const open=async(target,url)=>{await target.goto(url,{waitUntil:"domcontentloaded"});await target.getByTestId("agent-workspace").waitFor();assert.equal(await target.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"desktop horizontal overflow");};
  const waitForStableWriteSurface=async target=>{await target.getByText("Authoritative Agent Workspace refresh is pending or failed. Action writes are unavailable.",{exact:true}).waitFor({state:"detached"});};
  const executeFixtureAction=async target=>{
    const secondary=target.locator(".aw-secondary-actions");
    if(await secondary.count()&&await secondary.getAttribute("open")===null)await secondary.locator("summary").click();
    await target.getByLabel("Composer action").selectOption({label:"Assign work"});
    await target.getByRole("button",{name:"assign work",exact:true}).click();
    await target.getByLabel("TeamMembership ID").fill("membership-mira");
    await target.getByRole("button",{name:"Execute action",exact:true}).click();
  };
  const routeState=liveConfig?{teamRun:liveConfig.teamRun,member:liveConfig.member,memberRun:liveConfig.memberRun,host:liveConfig.host,space:liveConfig.space,project:liveConfig.project}:{teamRun:team.latest_run.id,member:"agent-mira",memberRun:"member-run-mira",host:"agent-host",space:"fixture-space",project:"fixture-project"};
  await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
  if(!liveConfig){assert.ok(firstMemberWorkspaceRequestAt!==null,"owner Agent Workspace request never started");assert.ok(Date.now()-firstMemberWorkspaceRequestAt<900,"continuous snapshot churn cancelled the first owner Agent Workspace load");}
  if(!liveConfig)await page.getByText(/persisted stream disconnected/).waitFor();
  await page.getByRole("button",{name:/Open .* configuration/}).waitFor();
  assert.deepEqual(await page.locator(".agent-roster-name").evaluateAll(nodes=>nodes.filter(node=>node.scrollWidth>node.clientWidth).map(node=>node.textContent)),[],"Agent roster identity is visually clipped");
  assert.deepEqual(await page.locator(".agent-roster-meta").evaluateAll(nodes=>nodes.filter(node=>node.scrollHeight>node.clientHeight+1).map(node=>node.textContent)),[],"Agent roster role/runtime meta wraps beyond its single-line row");
  if(!liveConfig){const hostMeta=await page.locator(".agent-roster-row").first().locator(".agent-roster-meta").textContent();assert.ok(hostMeta?.includes("External · unmanaged"),"external-interactive Host roster row must not masquerade as Running");assert.ok(!hostMeta?.includes("Running"),"external-interactive Host roster row still shows Running");}
  assert.deepEqual(await page.locator(".aw-context-work-title, .aw-context-work-row-title, .aw-context-work-row-meta").evaluateAll(nodes=>nodes.filter(node=>node.scrollWidth>node.clientWidth).map(node=>node.textContent)),[],"Current or assigned Work is visually clipped");
  await page.getByText(/no Host-authored Message authority/).waitFor();
  assert.equal(await page.locator('textarea[aria-label="Message"]').count(),0,"Member self-view borrowed Host Message authority");
  if(!liveConfig){await page.getByText(/Implemented the Team-scoped Session projection/).waitFor();await page.locator(".aw-native-facts-trail .aw-stream-fact__trigger").first().waitFor();assert.ok(await page.locator(".aw-native-facts-trail .aw-stream-fact__trigger").count()>=3,"native observations are not presented as individual expandable event rows");}
  if(!liveConfig){
    assert.equal(await page.locator(".aw-runtime-truth").count(),1,"four-axis runtime truth is missing");
    await page.getByText("Harness Message · coordination",{exact:true}).waitFor();
    await page.getByText("Work link · context only",{exact:true}).waitFor();
    await page.getByText("Provider-native · execution evidence",{exact:true}).waitFor();
    await page.getByText("Harness Recovery required",{exact:true}).waitFor();
    await page.getByText("Native Observed",{exact:true}).waitFor();
    const selectedRosterMeta=await page.locator('.agent-roster-row[data-selected="true"] .agent-roster-meta').textContent();
    assert.ok(selectedRosterMeta?.includes("Harness Recovery required · native Observed"),"selected roster row must use the same Harness/native axes as the header and context rail");
    assert.equal(await page.locator('[data-session-row-kind="control_boundary"]').count(),1,"loss-of-control boundary is missing from the persisted timeline");
    const boundaryRow=page.locator('[data-session-row-kind="control_boundary"]');
    const spanningToolRow=page.getByLabel("Tool call Read").locator('xpath=ancestor::div[@data-session-row-kind="provider"]');
    assert.ok(Number(await boundaryRow.getAttribute("data-index"))<Number(await spanningToolRow.getAttribute("data-index")),"the Harness control boundary must precede a Tool episode whose exact terminal occurrence was observed after control loss");
    await page.getByText(/do not prove recovery or Work completion/).first().waitFor();
  }
  if(!liveConfig)assert.equal(await page.getByTestId("agent-workspace-identity").getByText("Native Session unavailable",{exact:true}).count(),0,"available historical Session projection is mislabeled as unavailable without an active current_session");
  if(!liveConfig){
    assert.equal(await page.getByRole("button",{name:/Open native Session response from Mira Chen/}).count(),1,"provider-native authored content is not presented as a primary readable Session record");
    assert.ok(await page.locator('.aw-stream-fact[data-family="tool"]').count()>=2,"canonical tool events do not use the shared Tool presentation family");
    assert.equal(await page.locator('.aw-stream-fact[data-family="tool"] .aw-stream-kind').first().getAttribute("data-tone"),"neutral","completed Tool activity should remain a quiet operational fact, not delivery-success chrome");
  }
  assert.equal(await page.locator(".aw-current-execution").count(),0,"Agent Workspace fabricated a current-execution preview without an exact Team Session live projection");
  assert.equal(await page.locator('[data-testid="agent-workspace"]').evaluate(node=>node.textContent?.includes("Live · transient")??false),false,"legacy live member activity entered the Team Session execution slot without exact Session generation scope");
  await waitForStableWriteSurface(page);
  assert.equal(await page.getByTestId("agent-workspace-composer").getAttribute("data-composer-kind"),"action","canonical Work/runtime controls did not remain separate from the read-only Message boundary");
  await page.screenshot({path:join(evidenceDir,`member-session--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  if(!liveConfig){
    const eventRow=page.locator('[data-tool-call-id="call-1"] .aw-stream-fact__trigger');
    await eventRow.click();assert.equal(await eventRow.getAttribute("aria-expanded"),"true","clicking an individual event row expands it in place");
    await page.getByText(/Canonical operating rules loaded\./).first().waitFor();
    assert.equal(await page.getByText("Read",{exact:true}).count()>0,true,"collapsed tool name is unavailable");
    assert.equal(await page.getByText(/AGENTS\.md/).count()>0,true,"collapsed primary target is unavailable");
    assert.equal(await page.getByText("Arguments",{exact:true}).count()>0,true,"structured arguments are unavailable");
    assert.equal(await page.getByText("Original provider-native records (2)",{exact:true}).count()>0,true,"raw evidence trail is unavailable");
    assert.equal(await page.locator('[data-tool-call-id="call-1"]').count(),1,"exact call id did not collapse request and completion into one episode");
    assert.equal(await page.locator('[data-tool-call-id="orphan-call"]').count(),1,"unpaired terminal tool evidence was dropped or guessed into another call");
    await eventRow.click();assert.equal(await eventRow.getAttribute("aria-expanded"),"false","tool episode does not collapse in place");
    const discriminatorlessResult=page.locator('[data-unpaired-tool-result="true"] .aw-stream-fact__trigger');
    assert.equal(await discriminatorlessResult.count(),1,"provider tool result without a call id was not projected as a standalone structured episode");
    await discriminatorlessResult.click();
    await page.getByText(/expected-tool-error/).first().waitFor();
    await page.getByLabel("Harness Messages and persisted provider-native records in their honest partial order").getByText("Provider omitted pairing discriminator",{exact:true}).waitFor();
    assert.equal(await page.locator('[data-tool-call-id="orphan-call"] .aw-stream-fact__trigger').getByText("Unknown tool",{exact:true}).count(),1,"unpaired tool evidence is not labeled honestly");
    const authoredMessage=page.getByRole("button",{name:/Open authored Message from Marcus Allen/}).first();
    await authoredMessage.click();const sessionDock=page.locator(".agent-dock-shell");await sessionDock.waitFor({state:"attached"});assert.ok((await sessionDock.boundingBox())?.width,"Session Message opened a zero-width dock");
    await page.getByText("Delivery",{exact:true}).waitFor();
    await page.getByText("Work context",{exact:true}).waitFor();
    await page.getByText(/does not mutate Work, prove a Result, or grant acceptance/).waitFor();
    const toolRow=page.locator('[data-tool-call-id="call-1"] .aw-stream-fact__trigger');await toolRow.focus();await page.keyboard.press("Space");assert.equal(await toolRow.getAttribute("aria-expanded"),"true","tool episode keyboard selection");
    assert.equal(await toolRow.locator('xpath=ancestor::article').getAttribute("data-selected"),"true","selected event does not retain a stable center-row state");
    await executeFixtureAction(page);
    await page.getByText("Completed assign_work. Refetching canonical RoleView.",{exact:true}).waitFor();
    await page.waitForLoadState("networkidle");
    assert.equal(await toolRow.locator('xpath=ancestor::article').getAttribute("data-selected"),"true","background revalidate dropped the selected stream row state");
    await page.screenshot({path:join(evidenceDir,`member-event-detail--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  }
  await page.getByRole("button",{name:"Open Messages dock",exact:true}).click();
  const workspaceDock=page.locator(".agent-dock-shell");await workspaceDock.waitFor();
  await workspaceDock.getByRole("button",{name:"all",exact:true}).click();
  if(!liveConfig){assert.ok(await workspaceDock.getByText(/Work context/).count()>0,"Messages dock does not label Work as context-only");assert.ok(await workspaceDock.getByText("Delivery",{exact:true}).count()>0,"Messages dock omits Harness delivery state");}
  await waitForStableWriteSurface(page);
  const visibleTogether=await page.evaluate(()=>{
    const selectors=['[data-testid="agent-workspace-identity"]','[data-testid="agent-workspace-sessionbar"]','[data-testid="agent-workspace-composer"]'];
    return selectors.map(selector=>{const node=document.querySelector(selector);const rect=node?.getBoundingClientRect();return {selector,visible:Boolean(rect&&rect.top>=0&&rect.bottom<=window.innerHeight&&rect.width>0&&rect.height>0)};});
  });
  assert.deepEqual(visibleTogether.filter(item=>!item.visible),[],`Messages dock obscured the stable Session shell: ${JSON.stringify(visibleTogether)}`);
  assert.equal(await page.getByTestId("agent-workspace-sessionbar").count(),1,"opening Messages replaced the Session center");
  await page.screenshot({path:join(evidenceDir,`member-messages--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  await workspaceDock.getByRole("tab",{name:/Work/}).click();
  if(!liveConfig)await workspaceDock.getByRole("button",{name:/Restore authored conversation dominance/}).waitFor();
  else await workspaceDock.locator('[data-testid="work-dock-list"] button').first().waitFor();
  await waitForStableWriteSurface(page);
  if(!liveConfig){
    await workspaceDock.getByRole("button",{name:/Restore authored conversation dominance/}).click();
    await workspaceDock.getByText(/revision 4/).waitFor();
    await workspaceDock.getByText("Result",{exact:true}).waitFor();
    await workspaceDock.getByRole("heading",{name:"Review",exact:true}).waitFor();
  }
  await page.screenshot({path:join(evidenceDir,`member-work--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  await workspaceDock.getByRole("button",{name:"Close Work and Messages dock"}).click();
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
    await page.getByLabel("Composer action").waitFor({state:"attached"});
    assert.equal(await page.locator(".aw-current-execution").count(),0,"reconnecting replayed volatile provider activity from the previous Session stream");
    failMemberRefresh=true;
    await executeFixtureAction(page);
    await page.getByRole("alert").filter({hasText:"Refresh failed; writes are disabled"}).waitFor();
    assert.equal(await page.locator('textarea[aria-label="Message"]').count(),0,"refresh failure retained a writable message composer");
    assert.equal(await page.getByLabel("Composer action").count(),0,"refresh failure retained a writable action selector");
    failMemberRefresh=false;
    let releaseMemberRefreshRetry;
    let markMemberRefreshRetryStarted;
    memberRefreshRetryGate=new Promise(resolve=>{releaseMemberRefreshRetry=resolve;});
    const memberRefreshRetryStarted=new Promise(resolve=>{markMemberRefreshRetryStarted=resolve;});
    observeMemberRefreshRetry=markMemberRefreshRetryStarted;
    await page.getByRole("button",{name:"Retry authenticated view",exact:true}).click();
    await memberRefreshRetryStarted;
    assert.equal(await page.getByRole("alert").filter({hasText:"Refresh failed; writes are disabled"}).count(),1,"pending retry cleared the authoritative refresh failure before success");
    assert.equal(await page.locator('textarea[aria-label="Message"]').count(),0,"pending retry restored a writable message composer before success");
    assert.equal(await page.getByLabel("Composer action").count(),0,"pending retry restored a writable action selector before success");
    releaseMemberRefreshRetry();memberRefreshRetryGate=null;observeMemberRefreshRetry=null;
    await page.getByRole("alert").filter({hasText:"Refresh failed; writes are disabled"}).waitFor({state:"detached"});
    await page.getByLabel("Composer action").waitFor({state:"attached"});
    await page.getByTestId("agent-workspace-identity").getByText("Session session-mira-current · gen 3",{exact:true}).waitFor();
  }
  const hostPage=await makePage(liveConfig?.hostToken??"fixture-host-token");
  await open(hostPage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.host}&space=${routeState.space}&project=${routeState.project}`);
  if(liveConfig&&(await hostPage.locator(".agent-authored-turn").count())>0)await hostPage.locator(".agent-authored-turn").first().waitFor();
  await waitForStableWriteSurface(hostPage);
  assert.equal(await hostPage.locator('textarea[aria-label="Message"]').count(),0,"selected Host self-view exposed a Host-to-self composer");
  if(!liveConfig){assert.equal(await hostPage.getByText("AGENTS.md",{exact:true}).count(),0,"Host Session incorrectly showed a non-selected Member event");const hostToolEvent=hostPage.locator('[data-tool-call-id="host-call-1"] .aw-stream-fact__trigger');await hostToolEvent.click();assert.equal(await hostToolEvent.getAttribute("aria-expanded"),"true","Host tool event did not expand inline");}
  if(!liveConfig){
    await hostPage.getByRole("button",{name:"Open Work dock",exact:true}).click();
    await hostPage.locator(".agent-dock-shell").waitFor();
    await hostPage.getByText("Current outcome",{exact:true}).waitFor();
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
    assert.equal(await hostPage.getByTestId("agent-workspace").count(),0,"old Host Session view remained while the selected Team Session was pending");
    assert.equal(await hostPage.locator('textarea[aria-label="Message"]').count(),0,"identity switch retained a writable composer while projection was pending");
    assert.equal(await hostPage.getByLabel("Composer action").count(),0,"identity switch retained an action selector while projection was pending");
    releasePublicProjection();hostMemberProjectionGate=null;
  }
  await hostPage.getByTestId("agent-workspace-identity").getByText(/Session .*gen/).waitFor();
  if(liveConfig&&(await hostPage.locator(".agent-authored-turn").count())>0)await hostPage.locator(".agent-authored-turn").first().waitFor();
  await waitForStableWriteSurface(hostPage);
  const hostComposer=hostPage.getByTestId("agent-workspace-composer");
  assert.equal(await hostComposer.getAttribute("data-composer-kind"),"message","exact Host did not receive the selected Member Message composer");
  const hostMessage=hostComposer.locator('textarea[aria-label="Message"]');
  await hostMessage.waitFor();
  if(!liveConfig){
    await hostComposer.getByRole("button",{name:"Open slash commands",exact:true}).click();
    await hostPage.screenshot({path:join(evidenceDir,`host-member-composer--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
    await hostMessage.press("Enter");
    await hostMessage.fill("/work Restore authored");
    await hostMessage.press("Enter");
    await hostComposer.getByText("Restore authored conversation dominance",{exact:true}).waitFor();
    await hostMessage.fill("Please verify the canonical composer boundary.");
    await hostMessage.press("Shift+Enter");
    await hostMessage.type("Keep Work as context only.");
    await hostMessage.press("Enter");
    await hostComposer.getByText("Message recorded. Refreshing this Agent Workspace.",{exact:true}).waitFor();
    const sent=postedActions.at(-1);
    assert.equal(sent.path,`/v1/agentfirm/team-runs/${team.latest_run.id}/messages/send`,"Host composer used a non-canonical Message route");
    assert.deepEqual(sent.body.recipient_ids,["agent-mira"],"Host composer did not freeze the selected AgentMember recipient");
    assert.equal(sent.body.work_id,baseWork.work_id,"/work did not bind the exact selected context");
    assert.equal(sent.body.body,"Please verify the canonical composer boundary.\nKeep Work as context only.","Host composer changed authored Message text or multiline keyboard input");
    assert.equal(sent.body.response_required,false,"ordinary composer fabricated response-required intent");
    assert.match(sent.headers["idempotency-key"],/^[0-9a-f-]{36}$/,"Host composer omitted a stable idempotency key");
    await hostMessage.fill("Keep this draft after the typed rejection.");
    failNextMessagePost=true;
    await hostComposer.getByRole("button",{name:"Send message",exact:true}).click();
    await hostComposer.getByText(/VERSION_CONFLICT: Authoritative Team revision changed/).waitFor();
    assert.equal(await hostMessage.inputValue(),"Keep this draft after the typed rejection.","typed rejection cleared the browser-local draft");
    const failedAttempt=postedActions.at(-1);
    await hostPage.screenshot({path:join(evidenceDir,`host-member-composer-error--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
    await hostPage.getByRole("button",{name:/Noah Park/}).first().click();
    const noahComposer=hostPage.getByTestId("agent-workspace-composer");
    await noahComposer.getByText(/Host →/).waitFor();
    assert.equal(await noahComposer.locator('textarea[aria-label="Message"]').inputValue(),"","selected Member inherited another Member's draft");
    await noahComposer.locator('textarea[aria-label="Message"]').fill("Noah-specific draft.");
    await hostPage.getByRole("button",{name:/Mira Chen/}).first().click();
    await hostPage.getByTestId("agent-workspace-identity").getByText("Mira Chen",{exact:true}).waitFor();
    const restoredComposer=hostPage.getByTestId("agent-workspace-composer");
    assert.equal(await restoredComposer.locator('textarea[aria-label="Message"]').inputValue(),"Keep this draft after the typed rejection.","returning to a Member did not restore its own draft");
    await restoredComposer.getByRole("button",{name:"Send message",exact:true}).click();
    await restoredComposer.getByText("Message recorded. Refreshing this Agent Workspace.",{exact:true}).waitFor();
    const retry=postedActions.at(-1);
    assert.equal(retry.headers["idempotency-key"],failedAttempt.headers["idempotency-key"],"typed retry did not preserve the original Message idempotency key");
    assert.equal(retry.body.body,failedAttempt.body.body,"typed retry changed the retained authored draft");
    assert.equal(await restoredComposer.locator('textarea[aria-label="Message"]').inputValue(),"","successful retry did not clear the browser-local draft");
  }
  const hostMemberEvents=hostPage.locator(".aw-native-facts-trail .aw-stream-fact__trigger");
  assert.ok(await hostMemberEvents.count()>=3,"Host-selected Team Member native activity is missing");
  const hostMemberTool=hostPage.locator('[data-tool-call-id="call-1"] .aw-stream-fact__trigger');await hostMemberTool.click();await hostPage.getByText(/Canonical operating rules loaded\./).first().waitFor();
  await hostPage.screenshot({path:join(evidenceDir,`host-member-team-session--1440x1000--${capturedSourceSha}.png`),animations:"disabled"});
  if(!liveConfig){
    await open(hostPage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.host}&space=${routeState.space}&project=${routeState.project}`);
    failHostMemberProjection=true;
    await hostPage.getByRole("button",{name:/Mira Chen/}).first().click();
    const selectedMemberUrl=hostPage.url();
    await hostPage.getByRole("alert").filter({hasText:"Failed to fetch"}).waitFor();
    assert.equal(await hostPage.getByText("Read Lead inbox",{exact:true}).count(),0,"failed identity switch restored the old Host Session activity");
    assert.equal(await hostPage.locator('textarea[aria-label="Message"]').count(),0,"failed identity switch retained a writable composer");
    assert.equal(await hostPage.getByLabel("Composer action").count(),0,"failed identity switch retained an action selector");
    failHostMemberProjection=false;
    await hostPage.getByRole("button",{name:"Retry authenticated view",exact:true}).click();
    await hostPage.getByTestId("agent-workspace-identity").getByText("Mira Chen",{exact:true}).waitFor();
    await hostPage.getByTestId("agent-workspace-identity").getByText("Session session-mira-current · gen 3",{exact:true}).waitFor();
    const recoveredMemberEvents=hostPage.locator(".aw-native-facts-trail .aw-stream-fact__trigger");
    assert.ok(await recoveredMemberEvents.count()>=3,"same-page retry did not recover the selected Member native activity");
    const recoveredMemberTool=hostPage.locator('[data-tool-call-id="call-1"] .aw-stream-fact__trigger');await recoveredMemberTool.click();await hostPage.getByText(/Canonical operating rules loaded\./).first().waitFor();
    assert.equal(hostPage.url(),selectedMemberUrl,"same-page retry changed the exact selected Member route");
  }
  if(!liveConfig){
    const routePage=await makePage("fixture-member-token");
    const staleTeam="team-from-an-old-bookmark";
    const staleUrl=`${base}/?surface=team&team=${staleTeam}&conversation=host&memberRun=stale-member-run&space=fixture-space&project=fixture-project`;
    const before=agentWorkspaceRequests.length;
    await routePage.goto(staleUrl,{waitUntil:"domcontentloaded"});
    await routePage.getByTestId("agent-workspace").waitFor();
    await routePage.waitForURL((url)=>url.searchParams.get("team")===team.team_id&&url.searchParams.get("conversation")==="agent-mira"&&url.searchParams.get("memberRun")==="member-run-mira");
    assert.equal(agentWorkspaceRequests.slice(before).some((path)=>path.endsWith(staleTeam)),false,"stale bookmark reached a private AgentWorkspace before authenticated Team convergence");
    assert.equal(await routePage.getByText(/exact selected AgentMember|exact Host authority/).count(),0,"stale bookmark exposed a raw authority failure instead of converging");
    await routePage.reload({waitUntil:"domcontentloaded"});
    await routePage.getByTestId("agent-workspace").waitFor();
    assert.equal(new URL(routePage.url()).searchParams.get("team"),team.team_id,"reload restored the stale Team route");
    await routePage.evaluate((url)=>{history.pushState(null,"",url);dispatchEvent(new PopStateEvent("popstate"));},staleUrl);
    await routePage.waitForURL((url)=>url.searchParams.get("team")===team.team_id&&url.searchParams.get("conversation")==="agent-mira");
    await routePage.getByRole("button",{name:"Open Messages dock",exact:true}).click();
    await routePage.locator(".agent-dock-shell").waitFor();
    assert.equal(new URL(routePage.url()).searchParams.get("agentMode"),null,"local Dock inspection polluted the canonical Agent route");
    assert.equal(await routePage.getByTestId("agent-workspace-sessionbar").count(),1,"opening the Dock replaced the Session center");
    const chooserPage=await makePage("fixture-multi-token");
    await chooserPage.goto(staleUrl,{waitUntil:"domcontentloaded"});
    await chooserPage.getByRole("heading",{name:"Choose the Agent Team to open"}).waitFor();
    assert.equal(new URL(chooserPage.url()).searchParams.get("team"),staleTeam,"multiple authorized Teams must not be resolved by an arbitrary automatic choice");
    await chooserPage.getByRole("button",{name:/Product Systems Team/}).click();
    await chooserPage.getByTestId("authenticated-team-workspace").waitFor();
    await chooserPage.waitForURL((url)=>url.searchParams.get("team")===team.team_id&&url.searchParams.get("conversation")===null);

    const localOperatorPage=await makePage(null);
    await open(localOperatorPage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
    await localOperatorPage.getByText(/Local Operator access is read-only/).waitFor();
    assert.equal(await localOperatorPage.locator('textarea[aria-label="Message"]').count(),0,"local Operator borrowed Host Message authority");
    await localOperatorPage.getByText("Native records without comparable provider timestamps remain in provider source order; their position relative to Harness Messages is not a recorded chronology.",{exact:true}).waitFor();
    assert.deepEqual((await localOperatorPage.locator("[data-session-row-kind]").evaluateAll(nodes=>nodes.map(node=>node.getAttribute("data-session-row-kind")).filter(kind=>kind==="provider"))).slice(0,4),["provider","provider","provider","provider"],"the control boundary or per-read observed_at disturbed provider source order");
    await localOperatorPage.getByRole("button",{name:"Load earlier native Session events",exact:true}).click();
    await localOperatorPage.getByText("Earlier exact provider-native event loaded from the same native Session.",{exact:true}).waitFor();
    assert.deepEqual(await localOperatorPage.locator("[data-native-ordering-position]").evaluateAll(nodes=>nodes.map(node=>Number(node.getAttribute("data-native-ordering-position")))),[1,10,11,13,14,15],"provider-native rows without occurred_at were reordered by per-read observed_at after loading an earlier page");
    assert.equal(await localOperatorPage.getByRole("button",{name:"Load earlier native Session events",exact:true}).count(),0,"terminal native Session page kept offering an invalid earlier cursor");
    await localOperatorPage.waitForTimeout(900);
    assert.deepEqual(await localOperatorPage.locator("[data-native-ordering-position]").evaluateAll(nodes=>nodes.map(node=>Number(node.getAttribute("data-native-ordering-position")))),[1,10,11,13,14,15],"ambient RoleView refresh discarded an already loaded provider-native history page");
    const localOperatorEvent=localOperatorPage.locator('[data-tool-call-id="call-1"] .aw-stream-fact__trigger');await localOperatorEvent.click();await localOperatorPage.getByText(/Canonical operating rules loaded\./).first().waitFor();
    assert.equal(await localOperatorPage.getByText("Persisted provider-native Session",{exact:true}).count(),1,"same-machine local Operator could not open the selected persisted Session without a token");
    const projectBUrl=new URL(localOperatorPage.url());projectBUrl.searchParams.set("project","fixture-project-b");
    await localOperatorPage.goto(projectBUrl.toString(),{waitUntil:"domcontentloaded"});
    await localOperatorPage.getByTestId("agent-workspace").waitFor();
    await localOperatorPage.locator('[data-native-ordering-position="20"]').waitFor();
    assert.match(await localOperatorPage.getByTestId("agent-workspace").innerText(),/Exact native event from the second Project only\./,"new Project native history did not render");
    assert.deepEqual(await localOperatorPage.locator("[data-native-ordering-position]").evaluateAll(nodes=>nodes.map(node=>Number(node.getAttribute("data-native-ordering-position")))),[20],"same Session id/generation leaked native history across exact Project identity");
    assert.equal(await localOperatorPage.getByText("Earlier exact provider-native event loaded from the same native Session.",{exact:true}).count(),0,"old Project native history survived an exact request identity change");
    await localOperatorPage.close();

    let releaseOlderPage;
    delayedOlderPageGate=new Promise(resolve=>{releaseOlderPage=resolve;});
    const paginationRacePage=await makePage(null);
    await open(paginationRacePage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=fixture-project`);
    await paginationRacePage.getByRole("button",{name:"Load earlier native Session events",exact:true}).click();
    await paginationRacePage.getByRole("button",{name:"Loading provider-native events…",exact:true}).waitFor();
    await paginationRacePage.getByRole("button",{name:/Noah Park/}).click();
    await paginationRacePage.locator('[data-native-ordering-position="20"]').waitFor();
    releaseOlderPage();
    delayedOlderPageGate=null;
    await paginationRacePage.waitForTimeout(200);
    assert.deepEqual(await paginationRacePage.locator("[data-native-ordering-position]").evaluateAll(nodes=>nodes.map(node=>Number(node.getAttribute("data-native-ordering-position")))),[20],"late pagination response crossed the exact request identity fence");
    await paginationRacePage.close();

    let releaseSourceReset;
    delayedOlderPageGate=new Promise(resolve=>{releaseOlderPage=resolve;});
    delayedSourceResetGate=new Promise(resolve=>{releaseSourceReset=resolve;});
    const sourceResetRacePage=await makePage(null);
    await open(sourceResetRacePage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=fixture-project`);
    await sourceResetRacePage.getByRole("button",{name:"Load earlier native Session events",exact:true}).click();
    await sourceResetRacePage.getByRole("button",{name:"Loading provider-native events…",exact:true}).waitFor();
    releaseSourceReset();
    await sourceResetRacePage.locator('[data-native-ordering-position="30"]').waitFor();
    delayedSourceResetGate=null;
    releaseOlderPage();
    delayedOlderPageGate=null;
    await sourceResetRacePage.waitForTimeout(200);
    assert.deepEqual(await sourceResetRacePage.locator("[data-native-ordering-position]").evaluateAll(nodes=>nodes.map(node=>Number(node.getAttribute("data-native-ordering-position")))),[30],"late older-page response replaced the authoritative reset source generation");
    assert.match(await sourceResetRacePage.getByTestId("agent-workspace").innerText(),/Authoritative native row after provider source reset\./,"source-reset head did not remain authoritative after stale pagination settled");
    await sourceResetRacePage.close();

    failDelayedOlderPage=true;
    delayedOlderPageGate=new Promise(resolve=>{releaseOlderPage=resolve;});
    const paginationFailurePage=await makePage(null);
    await open(paginationFailurePage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=fixture-project`);
    await paginationFailurePage.getByRole("button",{name:"Load earlier native Session events",exact:true}).click();
    await paginationFailurePage.getByRole("button",{name:"Loading provider-native events…",exact:true}).waitFor();
    await paginationFailurePage.getByRole("button",{name:/Noah Park/}).click();
    await paginationFailurePage.locator('[data-native-ordering-position="20"]').waitFor();
    releaseOlderPage();
    delayedOlderPageGate=null;
    await paginationFailurePage.waitForTimeout(200);
    failDelayedOlderPage=false;
    assert.equal(await paginationFailurePage.getByText(/Refresh failed; writes are disabled/).count(),0,"late old-identity pagination failure disabled the new exact identity");
    assert.deepEqual(await paginationFailurePage.locator("[data-native-ordering-position]").evaluateAll(nodes=>nodes.map(node=>Number(node.getAttribute("data-native-ordering-position")))),[20],"late failed pagination response altered the new exact identity");
    await paginationFailurePage.close();
  }
  const expectedFailureErrors=consoleErrors.filter(message=>message.includes("503 (Service Unavailable)"));
  if(!liveConfig)assert.equal(expectedFailureErrors.length,2,`fixture should exercise exactly two expected 503 projection failures: ${expectedFailureErrors.join(" | ")}`);
  const expectedFetchErrors=consoleErrors.filter(message=>message.includes("net::ERR_FAILED"));
  if(!liveConfig)assert.equal(expectedFetchErrors.length,1,`fixture should exercise exactly one initial Failed-to-fetch recovery: ${expectedFetchErrors.join(" | ")}`);
  const expectedConflictErrors=consoleErrors.filter(message=>message.includes("409 (Conflict)"));
  if(!liveConfig)assert.equal(expectedConflictErrors.length,1,`fixture should exercise exactly one typed Message rejection: ${expectedConflictErrors.join(" | ")}`);
  const browserTeardownErrors=["net::ERR_CONNECTION_CLOSED","net::ERR_CONNECTION_RESET"];
  const unexpectedConsoleErrors=consoleErrors.filter(message=>!message.includes("503 (Service Unavailable)")&&!message.includes("409 (Conflict)")&&!message.includes("net::ERR_FAILED")&&!message.includes("404")&&!browserTeardownErrors.some(error=>message.includes(error)));
  const unexpectedHttpFailures=httpFailures.filter(failure=>!failure.startsWith("404 https://fonts.gstatic.com/"));
  assert.deepEqual(unexpectedConsoleErrors,[],`unexpected console errors: ${consoleErrors.join(" | ")}; HTTP failures: ${httpFailures.join(" | ")}; unknown fixture paths: ${unknownFixturePaths.join(", ")}`);
  assert.deepEqual(unexpectedHttpFailures.filter(failure=>!failure.startsWith("503 ")&&!failure.startsWith("409 ")),[],`unexpected HTTP failures: ${httpFailures.join(" | ")}`);

  for(const viewport of [{width:900,height:1180},{width:390,height:844},{width:320,height:844}]){
    await page.setViewportSize(viewport);
    await open(page,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,`${viewport.width}px horizontal overflow`);
    if(viewport.width===390){
      await page.getByRole("button",{name:"Open Agent roster"}).click();
      await page.getByRole("dialog",{name:"Agent roster"}).waitFor();
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"390px roster sheet overflow");
      await page.getByRole("button",{name:"Close Agent roster"}).click();
      await page.getByRole("button",{name:"Open Work dock",exact:true}).click();
      const compactDock=page.getByRole("complementary",{name:"Work and Messages dock"});await compactDock.waitFor();
      assert.equal(await compactDock.evaluate(node=>getComputedStyle(node).position),"fixed","390px Work dock is not an overlay");
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"390px Work dock overflow");
      await compactDock.getByRole("button",{name:"Close Work and Messages dock"}).click();
    }
    if(!liveConfig&&viewport.width<=390){
      await hostPage.setViewportSize(viewport);
      await open(hostPage,`${base}/?surface=team&team=${routeState.teamRun}&conversation=${routeState.member}&memberRun=${routeState.memberRun}&space=${routeState.space}&project=${routeState.project}`);
      const compactComposer=hostPage.getByTestId("agent-workspace-composer");
      await compactComposer.locator('textarea[aria-label="Message"]').waitFor();
      assert.equal(await hostPage.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,`${viewport.width}px Host Message composer overflow`);
      if(viewport.width===390){
        await compactComposer.getByRole("button",{name:"Open slash commands",exact:true}).click();
        await compactComposer.getByRole("option",{name:/\/work/}).waitFor();
        assert.equal(await hostPage.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth),true,"390px slash palette overflow");
        await hostPage.screenshot({path:join(evidenceDir,`host-member-composer--390x844--${capturedSourceSha}.png`),animations:"disabled"});
        await compactComposer.locator('textarea[aria-label="Message"]').press("Escape");
      }
    }
  }
  const captures=[];
  for(const name of ["member-session",...(!liveConfig?["member-event-detail"]:[]),"member-messages","member-work","member-configuration","host-session","host-member-team-session"]){
    const file=`${name}--1440x1000--${capturedSourceSha}.png`;
    captures.push({name,file,viewport:{width:1440,height:1000},sha256:createHash("sha256").update(await readFile(join(evidenceDir,file))).digest("hex")});
  }
  if(!liveConfig){const file=`host-member-composer--390x844--${capturedSourceSha}.png`;captures.push({name:"host-member-composer-compact",file,viewport:{width:390,height:844},sha256:createHash("sha256").update(await readFile(join(evidenceDir,file))).digest("hex")});}
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
    native_session_claim:liveConfig?{availability:liveMemberEvidence?.data?.persisted_session_projection?.available?"available":"unavailable",native_session_id:liveMemberEvidence?.data?.current_session?.agent_session_id??null,claim:liveMemberEvidence?.data?.persisted_session_projection?.available?"Team-scoped persisted provider-native Session projection captured":"not claimed: no bound provider-native Session in the canonical store"}:"deterministic fixture only",
    captures,
  };
  await writeFile(join(evidenceDir,"evidence-manifest.json"),`${JSON.stringify(manifest,null,2)}\n`);
  console.log(`agent workspace browser check: PASS (${liveConfig?"store-live":"fixture"}; ${evidenceDir})`);
}finally{await browser.close();await vite.close();}

function captureSourceRevision(value){return value==="working-tree"?value:String(value);}
