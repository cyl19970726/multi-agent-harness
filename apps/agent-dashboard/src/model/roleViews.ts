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
  kind: string; target_ref: TargetRef; required_version: number | null; disabled_reason: string | null;
}

export type RoleActionExecutor = (
  path: string,
  body?: unknown,
  options?: {headers?: Readonly<Record<string,string>>},
) => Promise<boolean>;

const TEAM_WORK_ACTIONS: Record<string,string> = {
  assign_work:"assign", rebind_work:"rebind", release_work:"release",
  request_changes:"request-changes", cancel_work:"cancel", claim_work:"claim",
  start_work:"start", block_work:"block", unblock_work:"resume",
  submit_work:"submit", revise_work:"start",
};

/** Resolve a server-authorized action onto an existing canonical 4A route.
 * This is deliberately a closed adapter: unknown actions fail closed and UI
 * payloads can never select actor identity. */
export function roleActionRoute(action:AllowedAction,context:{teamId?:string;teamRunId?:string;nodeId?:string}):string|null{
  const id=encodeURIComponent(action.target_ref.id);
  const run=context.teamRunId&&encodeURIComponent(context.teamRunId);
  const team=context.teamId&&encodeURIComponent(context.teamId);
  if(action.kind in TEAM_WORK_ACTIONS&&run)return `/v1/team-runs/${run}/works/${id}/${TEAM_WORK_ACTIONS[action.kind]}`;
  if(action.kind==="accept_work"&&team)return `/v1/teams/${team}/works/${id}/accept`;
  if(action.kind==="write_report"&&team)return `/v1/teams/${team}/works/${id}/reports`;
  if(action.kind==="write_finding"&&team)return `/v1/teams/${team}/works/${id}/findings`;
  if(action.kind==="write_failure"&&team)return `/v1/teams/${team}/works/${id}/failure-analyses`;
  if(action.kind==="create_work"&&run)return `/v1/team-runs/${run}/works`;
  if(["send_message","reply_message","request_decision"].includes(action.kind)&&run)return `/v1/team-runs/${run}/messages`;
  if(["close_member_run","reopen_member_run","retire_member_run"].includes(action.kind))return `/v1/member-runs/${id}/${action.kind.replace("_member_run","")}`;
  if(["provision_workspace","attach_workspace","archive_workspace","cleanup_workspace"].includes(action.kind))return `/v1/member-runs/${id}/workspace/${action.kind.replace("_workspace","")}`;
  if(action.kind==="evaluate_gate")return `/v1/gate-requirements/${id}/evaluate`;
  if(action.kind==="waive_gate")return `/v1/gate-requirements/${id}/waive`;
  if(action.kind==="revoke_waiver")return `/v1/gate-waivers/${id}/revoke`;
  if(action.kind==="reconcile_delivery"&&action.target_ref.kind==="work_delivery")return `/v1/work-deliveries/${id}/reconcile`;
  if(action.kind==="reconcile_delivery"&&action.target_ref.kind==="message_delivery")return `/v1/message-deliveries/${id}/reconcile`;
  // Diagnostics is the current Operator RoleView GET itself, not a mutation.
  // The caller refreshes that view instead of POSTing to a read endpoint.
  if(action.kind==="daemon_diagnostics")return null;
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
export interface CompanyWorkIndexData {
  query: Record<string, string[]>; sort: Array<{field: string; direction: string}>; items: WorkSummary[];
  page: { as_of_event_sequence: number; item_count: number; next_cursor: string | null };
  facets: Record<string, string[]>;
}
export interface TeamWorkspaceData {
  team: {team_id:string; team_revision:number; mission_id:string; node_id:string; placement_generation:number|null; status:string};
  works: WorkSummary[]; members: Array<Record<string, unknown>>; messages: Array<Record<string, unknown>>;
  reports: unknown[]; findings: unknown[]; failures: unknown[]; gate_requirements: unknown[];
  gate_evaluations: unknown[]; gate_waivers: unknown[]; workspace_attention: unknown[];
  delegation_provenance: unknown[]; page: {as_of_event_sequence:number;item_count:number;next_cursor:string|null};
}
export interface HostConsoleData {
  team_ref:string; mission_ref:string; work_queues:Record<string,WorkSummary[]>;
  member_capacity:Array<Record<string,unknown>>; convergence_plans:unknown[]; reusable_findings:unknown[];
  workspace_conflicts:unknown[]; provider_capacity_attention:unknown[]; deliveries_requiring_reconcile:unknown[];
  gate_attention:unknown[]; daemon_summary:Record<string,unknown>;
}
export interface MemberWorkbenchData {
  agent_member:Record<string,unknown>; member_run:Record<string,unknown>; my_works:WorkSummary[];
  eligible_ready_pool:WorkSummary[]; unread_messages:unknown[]; queued_deliveries:unknown[];
  workspace_binding:Record<string,unknown>|null; native_session_health:string; pending_provider_interactions:unknown[];
  report_history:unknown[]; finding_history:unknown[]; failure_history:unknown[]; gate_requirements:unknown[];
}
export interface OperatorViewData {
  node:Record<string,unknown>; build:{build_sha:string;protocol_version:string;schema_version:string};
  projects:unknown[]; team_supervisors:unknown[]; delivery_backlog:{depth:number;oldest_age_ms:number|null;recovery_required:boolean};
  runtime_recovery:unknown[]; provider_admission:unknown[]; workspace_safety:unknown[]; diagnostics:unknown[];
}

export async function fetchRoleView<T>(apiUrl:string,path:string,scope:{project?:string;space?:string;company?:string}={}):Promise<RoleView<T>>{
  const url=new URL(path,apiUrl.endsWith("/")?apiUrl:`${apiUrl}/`);
  if(scope.project)url.searchParams.set("project",scope.project);
  if(scope.space)url.searchParams.set("space",scope.space);
  if(scope.company)url.searchParams.set("company",scope.company);
  const token=window.__AGENTFIRM_BOOTSTRAP__?.capabilityToken;
  const response=await fetch(url.toString(),{headers:{Accept:"application/json",...(token?{"X-AgentFirm-Token":token}:{})}});
  const body=await response.json().catch(()=>({}));
  if(!response.ok)throw new Error(body?.error?.message??`RoleView request failed (${response.status})`);
  if(body.schema_version!==ROLE_VIEW_SCHEMA)throw new Error(`Unsupported RoleView schema: ${String(body.schema_version)}`);
  return body as RoleView<T>;
}
