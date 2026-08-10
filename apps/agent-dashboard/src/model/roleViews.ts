export const ROLE_VIEW_SCHEMA = "agentfirm.role_views.v1" as const;

export type RoleViewFreshness = "current" | "stale" | "unavailable" | "unknown";
export type RoleViewKind = "company_work" | "team_workspace" | "host_console" | "member_workbench" | "operator";

export interface ActorRef { kind: string; id: string }
export interface TargetRef { kind: string; id: string }
export interface AttentionItem {
  kind: string; severity: "info" | "warning" | "critical"; source_ref: TargetRef;
  reason_code: string; first_seen_at: string; last_seen_at: string; recommended_action: string;
}
export interface AllowedAction {
  kind: string; target_ref: TargetRef; required_version: number; disabled_reason: string | null;
}

export interface AgentFirmActionError {
  status?: number;
  code: string;
  message: string;
  retryable?: boolean;
  resource_kind?: string;
  resource_id?: string;
  current_version?: number | null;
}

export interface RoleActionExecutionResult { ok: boolean; error?: AgentFirmActionError }

export type RoleActionExecutor = (
  path: string,
  body?: unknown,
  options?: {headers?: Readonly<Record<string,string>>},
) => Promise<RoleActionExecutionResult>;

const TEAM_WORK_ACTIONS = {
  assign_work:"assign", rebind_work:"rebind", release_work:"release",
  cancel_work:"cancel", claim_work:"claim",
  start_work:"start", block_work:"block", unblock_work:"resume",
  submit_work:"submit",
} as const;

export type ExecutableRoleActionKind = "create_work" | "accept_work" | "reconcile_delivery" | keyof typeof TEAM_WORK_ACTIONS;
export type RoleActionFields = Readonly<Record<string,string>>;
export interface PreparedRoleAction {
  path: string;
  body: Record<string,unknown>;
  headers: Readonly<Record<string,string>>;
}

/** Build one closed semantic intent. The browser cannot express actor,
 * authority, event id, CAS, or idempotency inside the payload. */
export function prepareRoleAction(
  action:AllowedAction,
  context:{teamId?:string;teamRunId?:string;nodeId?:string},
  fields:RoleActionFields,
  confirmed:boolean,
):PreparedRoleAction|{error:string}{
  if(action.disabled_reason)return {error:action.disabled_reason};
  if(!Number.isSafeInteger(action.required_version)||Number(action.required_version)<0){
    return {error:"Server did not provide an exact CAS version."};
  }
  const run=context.teamRunId&&encodeURIComponent(context.teamRunId);
  const team=context.teamId&&encodeURIComponent(context.teamId);
  const node=context.nodeId&&encodeURIComponent(context.nodeId);
  const id=encodeURIComponent(action.target_ref.id);
  const operation=TEAM_WORK_ACTIONS[action.kind as keyof typeof TEAM_WORK_ACTIONS];
  const path=action.kind==="reconcile_delivery"&&node
    ?`/v1/agentfirm/nodes/${node}/work-deliveries/${id}/reconcile`
    :action.kind==="accept_work"&&team
    ?`/v1/agentfirm/teams/${team}/works/${id}/accept`
    :action.kind==="create_work"&&run
    ?`/v1/agentfirm/team-runs/${run}/works`
    :operation&&run?`/v1/agentfirm/team-runs/${run}/works/${id}/${operation}`:null;
  if(!path)return {error:"Dashboard semantic adapter does not implement this action."};
  const required=(name:string)=>{const value=fields[name]?.trim();if(!value)throw new Error(`${name.replace(/_/g," ")} is required.`);return value};
  let body:Record<string,unknown>;
  try{
    switch(action.kind as ExecutableRoleActionKind){
      case "create_work": body={action:"create_work",work_id:required("work_id"),title:required("title"),context_markdown:fields.context_markdown??"",completion_criteria_markdown:required("completion_criteria_markdown"),claim_mode:fields.claim_mode||"host_assign",priority:fields.priority||"normal"};break;
      case "accept_work": body={action:"accept_work"};break;
      case "reconcile_delivery": body={action:"reconcile_delivery",evidence_ref:required("evidence_ref")};break;
      case "assign_work": body={action:"assign_work",member_run_id:required("member_run_id")};break;
      case "rebind_work": body={action:"rebind_work",member_run_id:required("member_run_id")};break;
      case "cancel_work": body={action:"cancel_work",reason:required("reason")};break;
      case "block_work": body={action:"block_work",reason:required("reason")};break;
      case "unblock_work": body={action:"unblock_work",resolution:required("resolution")};break;
      case "submit_work": {
        const artifact_refs=(fields.artifact_refs??"").split(",").map(value=>value.trim()).filter(Boolean);
        const check_refs=(fields.check_refs??"").split(",").map(value=>value.trim()).filter(Boolean);
        if(artifact_refs.length+check_refs.length===0)throw new Error("At least one artifact or check/evidence ref is required.");
        body={action:"submit_work",result_summary:required("result_summary"),artifact_refs,check_refs,...(fields.base_revision?.trim()?{base_revision:fields.base_revision.trim()}:{}),candidate_revision:required("candidate_revision")};break;
      }
      case "release_work": body={action:"release_work"};break;
      case "claim_work": body={action:"claim_work"};break;
      case "start_work": body={action:"start_work"};break;
    }
  }catch(error){return {error:error instanceof Error?error.message:String(error)}}
  if(["accept_work","cancel_work","reconcile_delivery"].includes(action.kind)&&!confirmed)return {error:"Server-enforced confirmation is required."};
  return {path,body,headers:{"Idempotency-Key":crypto.randomUUID(),"If-Match":String(action.required_version),...(action.kind==="cancel_work"?{"X-AgentFirm-Confirm":"cancel"}:action.kind==="accept_work"?{"X-AgentFirm-Confirm":"accept"}:action.kind==="reconcile_delivery"?{"X-AgentFirm-Confirm":"reconcile_delivery"}:{})}};
}

/** Resolve only the semantic actions implemented by the closed adapter. */
export function roleActionRoute(action:AllowedAction,context:{teamId?:string;teamRunId?:string;nodeId?:string}):string|null{
  const id=encodeURIComponent(action.target_ref.id);
  const run=context.teamRunId&&encodeURIComponent(context.teamRunId);
  const team=context.teamId&&encodeURIComponent(context.teamId);
  const node=context.nodeId&&encodeURIComponent(context.nodeId);
  if(action.kind in TEAM_WORK_ACTIONS&&run)return `/v1/agentfirm/team-runs/${run}/works/${id}/${TEAM_WORK_ACTIONS[action.kind as keyof typeof TEAM_WORK_ACTIONS]}`;
  if(action.kind==="create_work"&&run)return `/v1/agentfirm/team-runs/${run}/works`;
  if(action.kind==="accept_work"&&team)return `/v1/agentfirm/teams/${team}/works/${id}/accept`;
  if(action.kind==="reconcile_delivery"&&node)return `/v1/agentfirm/nodes/${node}/work-deliveries/${id}/reconcile`;
  return null;
}
export interface RoleView<T> {
  view_kind: RoleViewKind; schema_version: typeof ROLE_VIEW_SCHEMA;
  source_execution_space_id: string; source_store_identity: string;
  as_of_event_sequence: number; generated_at: string; freshness: RoleViewFreshness;
  data: T; attention: AttentionItem[]; allowed_actions: AllowedAction[];
}

export interface WorkSummary {
  work_id: string; work_revision: number; team_id: string; mission_id: string;
  owner_actor_ref: ActorRef | null; current_member_run_ref: string | null;
  phase: string; condition: string; resolution: string | null; priority: string;
  module_refs: string[];
  gate_summary: { required: number; passed: number; failed: number; pending: number; waived: number; stale: number };
  latest_report_ref: string | null; latest_finding_refs: string[]; latest_failure_ref: string | null;
  delivery_summary: Record<string, number | string>; runtime_summary: Record<string, unknown>;
  workspace_summary: Record<string, unknown>; delegation_summary: Record<string, unknown>; updated_at: string;
}
export interface RoleRecordSummary {
  kind:string; id:string; work_id:string|null; member_run_id:string|null; requirement_id:string|null;
  status:string|null; version:number|null; actor_ref:ActorRef|null; summary:string|null;
  created_at:string|null; source_id:string|null; target_id:string|null; locator:string|null;
}
export interface MemberCapacitySummary {
  agent_member_ref:ActorRef; role:string; organization_status:string; current_member_run_ref:string|null;
  runtime_state:string|null; runtime_generation:number|null; capacity:"available"|"busy"|"paused"|"unknown";
}
export interface MessageSummary {
  message_id:string; work_id:string|null; sender:ActorRef; recipients:ActorRef[];
  response_intent:string; created_at:string; delivery_summary:string[];
}
export interface CompanyWorkIndexData {
  query: Record<string, string[]>; sort: Array<{field: string; direction: string}>; items: WorkSummary[];
  page: { as_of_event_sequence: number; item_count: number; next_cursor: string | null; snapshot_vector:Array<{execution_space_id:string;store_identity:string;trust_store_sequence:number;work_operation_count:number;team_row_count:number;team_run_row_count:number}> };
  facets: Record<string, string[]>;
}
export interface TeamWorkspaceData {
  team: {team_id:string; team_revision:number; mission_id:string; node_id:string; placement_generation:number|null; status:string};
  works: WorkSummary[]; members: MemberCapacitySummary[]; messages: MessageSummary[];
  reports: RoleRecordSummary[]; findings: RoleRecordSummary[]; failures: RoleRecordSummary[]; gate_requirements: RoleRecordSummary[];
  gate_evaluations: RoleRecordSummary[]; gate_waivers: RoleRecordSummary[]; workspace_attention: RoleRecordSummary[];
  delegation_provenance: RoleRecordSummary[]; page: {as_of_event_sequence:number;item_count:number;next_cursor:string|null};
}
export interface HostConsoleData {
  team_ref:string; mission_ref:string; work_queues:Record<string,WorkSummary[]>;
  member_capacity:MemberCapacitySummary[]; convergence_plans:RoleRecordSummary[]; reusable_findings:RoleRecordSummary[];
  workspace_conflicts:RoleRecordSummary[]; provider_capacity_attention:Array<{state:"not_modeled";reason:string}>; deliveries_requiring_reconcile:RoleRecordSummary[];
  gate_attention:RoleRecordSummary[]; daemon_summary:{node_id:string;lease_status:string|null;generation:number|null};
}
export interface MemberWorkbenchData {
  agent_member:{id:string;role:string;organization_status:string}; member_run:{id:string;agent_member_id:string;team_run_id:string;coordination_status:string;runtime_status:string;runtime_generation:number;native_session_health:string}; my_works:WorkSummary[];
  eligible_ready_pool:WorkSummary[]; unread_messages:MessageSummary[]; queued_deliveries:RoleRecordSummary[];
  workspace_binding:RoleRecordSummary|null; native_session_health:string; pending_provider_interactions:RoleRecordSummary[];
  report_history:RoleRecordSummary[]; finding_history:RoleRecordSummary[]; failure_history:RoleRecordSummary[]; gate_requirements:RoleRecordSummary[];
}
export interface OperatorViewData {
  node:{node_id:string;node_revision:number;daemon_generation:number|null;status:string}; build:{build_sha:string;protocol_version:string;schema_version:string};
  projects:RoleRecordSummary[]; team_supervisors:RoleRecordSummary[]; delivery_backlog:{depth:number;oldest_age_ms:number|null;recovery_required:boolean};
  runtime_recovery:RoleRecordSummary[]; provider_admission:RoleRecordSummary[]; workspace_safety:RoleRecordSummary[]; diagnostics:Array<{kind:string;state:string}>;
}

export async function fetchRoleView<T>(apiUrl:string,path:string,scope:{project?:string;space?:string;company?:string}={}):Promise<RoleView<T>>{
  const base=apiUrl.endsWith("/")?apiUrl:`${apiUrl}/`;
  const metaUrl=new URL("/v1/meta",base);
  if(scope.project)metaUrl.searchParams.set("project",scope.project);
  if(scope.space)metaUrl.searchParams.set("space",scope.space);
  const metaResponse=await fetch(metaUrl.toString(),{headers:{Accept:"application/json"}});
  const meta=await metaResponse.json().catch(()=>({}));
  if(!metaResponse.ok)throw new Error(`AgentFirm capability negotiation failed (${metaResponse.status})`);
  const mismatches=[
    meta.schema_version!==ROLE_VIEW_SCHEMA&&`schema ${String(meta.schema_version)}`,
    meta.protocol_version!=="agentfirm-member-trust/1"&&`protocol ${String(meta.protocol_version)}`,
    meta.action_manifest_version!=="agentfirm.role_actions.v1"&&`actions ${String(meta.action_manifest_version)}`,
    meta.capability_auth!=="x-agentfirm-token"&&`auth ${String(meta.capability_auth)}`,
    (typeof meta.build_sha!=="string"||!/^[0-9a-f]{40}$/.test(meta.build_sha))&&"invalid build SHA",
  ].filter(Boolean);
  if(mismatches.length)throw new Error(`Unsupported AgentFirm capabilities: ${mismatches.join(", ")}`);
  const url=new URL(path,apiUrl.endsWith("/")?apiUrl:`${apiUrl}/`);
  if(scope.project)url.searchParams.set("project",scope.project);
  if(scope.space)url.searchParams.set("space",scope.space);
  if(scope.company)url.searchParams.set("company",scope.company);
  const token=window.__AGENTFIRM_BOOTSTRAP__?.capabilityToken;
  const response=await fetch(url.toString(),{headers:{Accept:"application/json",...(token?{"X-AgentFirm-Token":token}:{})}});
  const body=await response.json().catch(()=>({}));
  if(!response.ok)throw new Error(body?.error?.message??`RoleView request failed (${response.status})`);
  if(body.schema_version!==ROLE_VIEW_SCHEMA)throw new Error(`Unsupported RoleView schema: ${String(body.schema_version)}`);
  const expectedKind=path.includes("company-work")?"company_work":path.includes("team-workspace")?"team_workspace":path.includes("host-console")?"host_console":path.includes("member-workbench")?"member_workbench":path.includes("operator")?"operator":null;
  if(!expectedKind||body.view_kind!==expectedKind)throw new Error(`RoleView kind mismatch: expected ${String(expectedKind)}, received ${String(body.view_kind)}`);
  if(body.source_execution_space_id!==scope.space&&expectedKind!=="company_work")throw new Error("RoleView execution-space identity mismatch");
  if(!Number.isSafeInteger(body.as_of_event_sequence)||!["current","stale","unavailable","unknown"].includes(body.freshness)||!Array.isArray(body.allowed_actions)||!Array.isArray(body.attention))throw new Error("Malformed RoleView envelope");
  for(const action of body.allowed_actions){if(!action||typeof action.kind!=="string"||!action.target_ref||typeof action.target_ref.id!=="string"||!Number.isSafeInteger(action.required_version)||action.required_version<0||!(action.disabled_reason===null||typeof action.disabled_reason==="string"))throw new Error("Malformed or non-CAS RoleView action")}
  return body as RoleView<T>;
}
