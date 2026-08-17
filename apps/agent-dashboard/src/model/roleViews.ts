export const ROLE_VIEW_SCHEMA = "agentfirm.role_views.v1" as const;

export type RoleViewFreshness = "current" | "stale" | "unavailable" | "unknown";
export type RoleViewKind = "global_work" | "team_workspace" | "host_console" | "agent_workspace" | "member_workbench" | "operator" | "team_inbox";

export interface ActorRef { kind: string; id: string; /** Server-resolved durable display label; the raw id remains the secondary display. */ display_name?: string|null }
export interface TargetRef { kind: string; id: string }
export interface AttentionItem {
  kind: string; severity: "info" | "warning" | "critical"; source_ref: TargetRef;
  reason_code: string; first_seen_at: string; last_seen_at: string; recommended_action: string;
}
export interface AllowedAction {
  kind: string; target_ref: TargetRef; required_version: number; disabled_reason: string | null;
  authority_generation?: number;
  intent_binding?: {provider:string;execution_mode:string;eligibility:"eligible"|"disabled";eligibility_fingerprint:string;project_binding_id:string;source_store_identity:string;registration_identity:string;registration_revision:number};
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

export type ExecutableRoleActionKind = "create_work" | "accept_work" | "reconcile_delivery" | keyof typeof TEAM_WORK_ACTIONS
  | "request_changes" | "revise_work" | "send_message" | "reply_message" | "request_decision"
  | "interrupt_member_run" | "close_member_run" | "reopen_member_run" | "retire_member_run" | "resume_native_session"
  | "provision_workspace" | "attach_workspace" | "archive_workspace" | "cleanup_workspace"
  | "write_report" | "write_finding" | "write_failure" | "request_gate_evaluation"
  | "evaluate_gate" | "waive_gate" | "revoke_waiver" | "reconcile_message_delivery" | "resolve_runtime_recovery"
  | "start_daemon" | "stop_daemon" | "admit_provider" | "diagnose";
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
  const memberRunActions:Record<string,string>={interrupt_member_run:"interrupt",close_member_run:"close",reopen_member_run:"reopen",retire_member_run:"retire",resume_native_session:"resume-native-session"};
  const workspaceActions:Record<string,string>={provision_workspace:"provision",attach_workspace:"attach",archive_workspace:"archive",cleanup_workspace:"cleanup"};
  const workRecordActions:Record<string,string>={request_changes:"request-changes",revise_work:"revise",write_report:"reports",write_finding:"findings",write_failure:"failure-analyses",request_gate_evaluation:"gate-requirements"};
  const messageActions:Record<string,string>={send_message:"send",reply_message:"reply",request_decision:"request-decision"};
  const path=action.kind==="reconcile_delivery"&&node
    ?`/v1/agentfirm/nodes/${node}/work-deliveries/${id}/reconcile`
    :action.kind==="reconcile_message_delivery"&&node?`/v1/agentfirm/nodes/${node}/message-deliveries/${id}/reconcile`
    :action.kind==="resolve_runtime_recovery"&&node?`/v1/agentfirm/nodes/${node}/runtime-commands/${id}/resolve`
    :["start_daemon","stop_daemon","admit_provider"].includes(action.kind)&&node?`/v1/agentfirm/nodes/${node}/${action.kind==="start_daemon"?"daemon-start":action.kind==="stop_daemon"?"daemon-stop":"provider-admission"}`
    :action.kind==="diagnose"&&node?`/v1/views/operator/${node}`
    :action.kind==="accept_work"&&team
    ?`/v1/agentfirm/teams/${team}/works/${id}/accept`
    :action.kind==="create_work"&&run
    ?`/v1/agentfirm/team-runs/${run}/works`
    :operation&&run?`/v1/agentfirm/team-runs/${run}/works/${id}/${operation}`
    :workRecordActions[action.kind]&&team?`/v1/agentfirm/teams/${team}/works/${id}/${workRecordActions[action.kind]}`
    :messageActions[action.kind]&&run?`/v1/agentfirm/team-runs/${run}/messages/${messageActions[action.kind]}`
    :memberRunActions[action.kind]?`/v1/agentfirm/member-runs/${id}/${memberRunActions[action.kind]}`
    :workspaceActions[action.kind]?`/v1/agentfirm/member-runs/${id}/workspace/${workspaceActions[action.kind]}`
    :action.kind==="evaluate_gate"?`/v1/agentfirm/gate-requirements/${id}/evaluate`
    :action.kind==="waive_gate"?`/v1/agentfirm/gate-requirements/${id}/waive`
    :action.kind==="revoke_waiver"?`/v1/agentfirm/gate-waivers/${id}/revoke`:null;
  if(!path)return {error:"Dashboard semantic adapter does not implement this action."};
  const required=(name:string)=>{const value=fields[name]?.trim();if(!value)throw new Error(`${name.replace(/_/g," ")} is required.`);return value};
  let body:Record<string,unknown>;
  try{
    switch(action.kind as ExecutableRoleActionKind){
      case "create_work": body={action:"create_work",work_id:required("work_id"),title:required("title"),context_markdown:fields.context_markdown??"",completion_criteria_markdown:required("completion_criteria_markdown"),claim_mode:fields.claim_mode||"host_assign",priority:fields.priority||"normal"};break;
      case "accept_work": body={action:"accept_work"};break;
      case "reconcile_delivery": body={action:"reconcile_delivery",evidence_ref:required("evidence_ref")};break;
      case "reconcile_message_delivery": body={action:"reconcile_message_delivery",outcome:fields.outcome||"retry_safe_failure",evidence_ref:required("evidence_ref")};break;
      case "resolve_runtime_recovery": body={action:"resolve_runtime_recovery",resolution:fields.resolution||"keep_recovery_required",evidence_ref:required("evidence_ref")};break;
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
      case "revise_work": { const artifact_refs=(fields.artifact_refs??"").split(",").map(v=>v.trim()).filter(Boolean);const check_refs=(fields.check_refs??"").split(",").map(v=>v.trim()).filter(Boolean);if(!artifact_refs.length&&!check_refs.length)throw new Error("At least one evidence ref is required.");body={action:"revise_work",result_summary:required("result_summary"),artifact_refs,check_refs,candidate_revision:required("candidate_revision"),...(fields.base_revision?.trim()?{base_revision:fields.base_revision.trim()}:{})};break; }
      case "request_changes": body={action:"request_changes",reason:required("reason")};break;
      case "send_message": body={action:"send_message",recipient_ids:required("recipient_ids").split(",").map(v=>v.trim()).filter(Boolean),body:required("body"),response_required:fields.response_required==="true",...(fields.work_id?.trim()?{work_id:fields.work_id.trim()}:{}),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean)};break;
      case "reply_message": body={action:"reply_message",recipient_ids:required("recipient_ids").split(",").map(v=>v.trim()).filter(Boolean),body:required("body"),correlation_id:required("correlation_id"),causation_id:required("causation_id"),response_required:fields.response_required==="true",...(fields.work_id?.trim()?{work_id:fields.work_id.trim()}:{}),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean)};break;
      case "request_decision": body={action:"request_decision",body:required("body"),...(fields.work_id?.trim()?{work_id:fields.work_id.trim()}:{}),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean)};break;
      case "interrupt_member_run": body={action:"interrupt_member_run",reason:required("reason")};break;
      case "close_member_run": body={action:"close_member_run"};break; case "reopen_member_run": body={action:"reopen_member_run"};break; case "retire_member_run": body={action:"retire_member_run"};break; case "resume_native_session": body={action:"resume_native_session"};break;
      case "provision_workspace": body={action:"provision_workspace",project_binding_id:required("project_binding_id"),mode:fields.mode||"worktree",ownership:fields.ownership||"managed",canonical_root:required("canonical_root"),...(fields.work_id?.trim()?{work_id:fields.work_id.trim()}:{})};break;
      case "attach_workspace": body={action:"attach_workspace"};break; case "archive_workspace": body={action:"archive_workspace"};break; case "cleanup_workspace": body={action:"cleanup_workspace"};break;
      case "write_report": body={action:"write_report",summary:required("summary"),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean)};break;
      case "write_finding": body={action:"write_finding",kind:fields.kind||"discovery",summary:required("summary"),detail_markdown:required("detail_markdown"),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean),confidence:fields.confidence||"medium"};break;
      case "write_failure": body={action:"write_failure",observed_failure:required("observed_failure"),impact:required("impact"),primary_cause_status:fields.primary_cause_status||"suspected",primary_cause:fields.primary_cause||undefined,retry_safety:fields.retry_safety||"unknown",recommended_host_decision:required("recommended_host_decision"),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean),confidence:fields.confidence||"medium"};break;
      case "request_gate_evaluation": body={action:"request_gate_evaluation",gate_type:required("gate_type"),gate_contract_version:required("gate_contract_version"),evaluator_ref:{kind:fields.evaluator_kind||"agent_member",id:required("evaluator_id")},evaluator_version:required("evaluator_version"),resolved_config:{},required:true};break;
      case "evaluate_gate": body={action:"evaluate_gate",verdict:fields.verdict||"passed",summary:required("summary"),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean)};break;
      case "waive_gate": body={action:"waive_gate",reason:required("reason"),evidence_refs:(fields.evidence_refs??"").split(",").map(v=>v.trim()).filter(Boolean)};break; case "revoke_waiver": body={action:"revoke_waiver"};break;
      case "start_daemon": if(!Number.isSafeInteger(action.authority_generation))throw new Error("Exact daemon authority generation is required.");body={action:"daemon_start",daemon_generation:action.authority_generation};break; case "stop_daemon": if(!Number.isSafeInteger(action.authority_generation))throw new Error("Exact daemon authority generation is required.");body={action:"daemon_stop",daemon_generation:action.authority_generation};break; case "admit_provider": {const binding=action.intent_binding;if(!binding||binding.eligibility!=="eligible"||!binding.provider||!binding.execution_mode||!binding.eligibility_fingerprint||!binding.project_binding_id||!binding.source_store_identity||!binding.registration_identity||!Number.isSafeInteger(binding.registration_revision)||binding.registration_revision<1)throw new Error("Server did not provide an eligible provider admission binding.");body={action:"admit_provider",provider:binding.provider,execution_mode:binding.execution_mode,eligibility_fingerprint:binding.eligibility_fingerprint};break;} case "diagnose": body={action:"diagnose"};break;
      case "release_work": body={action:"release_work"};break;
      case "claim_work": body={action:"claim_work"};break;
      case "start_work": body={action:"start_work"};break;
    }
  }catch(error){return {error:error instanceof Error?error.message:String(error)}}
  const confirmation:Record<string,string>={cancel_work:"cancel",accept_work:"accept",reconcile_delivery:"reconcile_delivery",reconcile_message_delivery:"reconcile_message_delivery",resolve_runtime_recovery:"resolve_runtime_recovery",close_member_run:"close_member_run",retire_member_run:"retire_member_run",cleanup_workspace:"cleanup_workspace",waive_gate:"waive_gate",revoke_waiver:"revoke_waiver",start_daemon:"daemon-start",stop_daemon:"daemon-stop"};
  if(confirmation[action.kind]&&!confirmed)return {error:"Server-enforced confirmation is required."};
  return {path,body,headers:{"Idempotency-Key":crypto.randomUUID(),"If-Match":String(action.required_version),...(confirmation[action.kind]?{"X-AgentFirm-Confirm":confirmation[action.kind]}:{})}};
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
  if(action.kind==="reconcile_message_delivery"&&node)return `/v1/agentfirm/nodes/${node}/message-deliveries/${id}/reconcile`;
  if(action.kind==="resolve_runtime_recovery"&&node)return `/v1/agentfirm/nodes/${node}/runtime-commands/${id}/resolve`;
  if(action.kind==="diagnose"&&node)return `/v1/views/operator/${node}`;
  if(["start_daemon","stop_daemon","admit_provider"].includes(action.kind)&&node)return `/v1/agentfirm/nodes/${node}/${action.kind==="start_daemon"?"daemon-start":action.kind==="stop_daemon"?"daemon-stop":"provider-admission"}`;
  const workRecords:Record<string,string>={request_changes:"request-changes",revise_work:"revise",write_report:"reports",write_finding:"findings",write_failure:"failure-analyses",request_gate_evaluation:"gate-requirements"};
  if(workRecords[action.kind]&&team)return `/v1/agentfirm/teams/${team}/works/${id}/${workRecords[action.kind]}`;
  const messages:Record<string,string>={send_message:"send",reply_message:"reply",request_decision:"request-decision"};
  if(messages[action.kind]&&run)return `/v1/agentfirm/team-runs/${run}/messages/${messages[action.kind]}`;
  const memberRuns:Record<string,string>={interrupt_member_run:"interrupt",close_member_run:"close",reopen_member_run:"reopen",retire_member_run:"retire",resume_native_session:"resume-native-session"};
  if(memberRuns[action.kind])return `/v1/agentfirm/member-runs/${id}/${memberRuns[action.kind]}`;
  const workspaces:Record<string,string>={provision_workspace:"provision",attach_workspace:"attach",archive_workspace:"archive",cleanup_workspace:"cleanup"};
  if(workspaces[action.kind])return `/v1/agentfirm/member-runs/${id}/workspace/${workspaces[action.kind]}`;
  if(action.kind==="evaluate_gate")return `/v1/agentfirm/gate-requirements/${id}/evaluate`;
  if(action.kind==="waive_gate")return `/v1/agentfirm/gate-requirements/${id}/waive`;
  if(action.kind==="revoke_waiver")return `/v1/agentfirm/gate-waivers/${id}/revoke`;
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
  accountable_team_id: string | null; assignee_membership_id: string | null;
  assignee_kind: "host" | "member" | "unassigned";
  assignee_ref: { kind: string; membership_id: string | null; membership_state: string | null; agent_member_id: string | null; display_name: string | null };
  migration_state: "canonical" | "legacy_team_run_scoped";
  title: string; context_markdown: string; completion_criteria_markdown: string;
  claim_mode: string; eligible_member_ids: string[]; prerequisite_work_ids: string[];
  parent_work_id: string | null; blocker_reason: string | null; result_summary: string | null;
  artifact_refs: string[]; check_refs: string[];
  latest_event: {id:string; kind:string; actor_ref:ActorRef|null; created_at:string} | null;
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
  display_name:string; provider:string|null; model:string|null; coordination_status:string|null;
  runtime_state:string|null; runtime_generation:number|null; native_session_health:string|null;
  capacity:"available"|"busy"|"paused"|"unknown";
  /** Adapter review state of the run's provider tuple; separate from runtime availability. */
  provider_compatibility?:"current"|"review_required"|"incompatible"|"unavailable"|"unknown"|null;
  provider_compatibility_note?:string|null;
  provider_version?:string|null;
  /** Exact core runtime binding admission; separate from source/version compatibility. */
  provider_capability_admission?:"active"|"review_required"|"unavailable"|"unknown"|null;
  provider_capability_note?:string|null;
  queued_work_count:number; active_work_count:number; review_work_count:number; blocked_work_count:number;
  latest_action:RoleRecordSummary|null;
}
export interface MessageDeliverySummary {
  id:string; recipient_member_run_id:string; status:string; version:number; provider_receipt_id:string|null; updated_at:string|null;
  /** Canonical recipient identity and its server-resolved label (DEV-25). */
  recipient_identity_id?: string|null; recipient_display_name?: string|null;
}
export type MessageDeliveryState = "unsettled"|"queued"|"delivered"|"acknowledged"|"failed";
export interface MessageSummary {
  message_id:string; work_id:string|null; sender:ActorRef; recipients:ActorRef[];
  body:string; kind:string; correlation_id:string; causation_id:string|null;
  response_intent:string; reply_eligible:boolean; created_at:string;
  /** Server-computed aggregate over the canonical per-recipient deliveries (DEV-25). */
  delivery_state?: MessageDeliveryState;
  deliveries:MessageDeliverySummary[];
}
export interface TeamActivitySummary {
  source:string; id:string; work_id:string|null; actor_ref:ActorRef|null; status:string|null; summary:string|null; created_at:string;
  /** Parent Message id for `message_delivery` rows (DEV-25). */
  message_id?: string|null;
}
export interface RuntimeFabricSummary {
  agent_identities:RoleRecordSummary[]; agent_sessions:RoleRecordSummary[]; team_memberships:RoleRecordSummary[];
  work_execution_bindings:RoleRecordSummary[]; messages:RoleRecordSummary[]; message_deliveries:RoleRecordSummary[];
}
export interface CollaborationActorRef { kind:"human"|"agent_member"|"external"|"service"; id:string }
export interface CollaborationRemoteWorkRef {
  schema_version:string; execution_space_id:string; node_id:string; team_id:string;
  team_revision:number; placement_generation:1; work_id:string; work_revision:number;
  work_event_id:string; digest:string;
}
export interface CollaborationTargetPlacementRef { team_id:string; team_revision:number; node_id:string; placement_generation:1 }
export interface CollaborationInboundPolicySnapshot {
  policy_id:string; policy_revision:number; policy_digest:string; mode:"host_approval_required"|"auto_accept";
  allowed_outcome_classes:string[]; max_active_delegations:number;
}
export interface CollaborationDelegationProjection {
  id:string; company_id:string; source_work_attestation_id:string; source_work_ref:CollaborationRemoteWorkRef;
  source_owner_ref:CollaborationActorRef; source_team_id:string; source_node_id:string;
  target_placement:CollaborationTargetPlacementRef; target_host_ref:CollaborationActorRef;
  requested_outcome:string; outcome_class:string; acceptance_contract:string;
  inbound_policy_snapshot:CollaborationInboundPolicySnapshot; target_work_ref?:CollaborationRemoteWorkRef|null;
  state:"proposed"|"awaiting_target_decision"|"provisioning_target_work"|"active"|"result_available"|"cancellation_requested"|"terminal";
  terminal_outcome?:"completed"|"rejected"|"cancelled"|"failed"|null; revision:number;
  operation_id:string; idempotency_key:string; created_by:CollaborationActorRef; created_at:string; updated_at:string;
}
export interface CollaborationCancellationProjection {
  id:string; delegation_id:string; expected_delegation_revision:number; requested_by:CollaborationActorRef;
  reason:string; state:"pending"|"accepted"|"rejected"; revision:number; created_at:string; updated_at:string;
  target_host_decision_ref?:string|null;
}
export interface CollaborationProjectionSummary {
  company_id?:string; team_id?:string; state:"observed"|"unavailable"; reason?:string;
  as_of_store_sequence?:number; delegation_count?:number; attention_count?:number; publication_count?:number;
  delegations?:CollaborationDelegationProjection[]; pending_cancellations?:CollaborationCancellationProjection[];
}
export interface TeamPressureSummary {active_turns:number;ready_members:number;total_members:number;ready_work:number;review_work:number;blocked_work:number}
export interface LatestTeamRunSummary {
  id:string; status:string; created_at:string|null; completed_at:string|null; execution_node_id:string|null;
  execution_root:string|null; project_binding_id:string|null; previous_run_id:string|null;
}
export interface GlobalWorkIndexData {
  query: Record<string, string[]>; sort: Array<{field: string; direction: string}>; items: WorkSummary[];
  page: { as_of_event_sequence: number; item_count: number; next_cursor: string | null; snapshot_vector:Array<{execution_space_id:string;store_identity:string;trust_store_sequence:number;work_operation_count:number;team_row_count:number;team_run_row_count:number}> };
  pending_migration_work_ids: string[];
  facets: Record<string, string[]>; runtime_fabric:RuntimeFabricSummary;
}
export interface TeamWorkspaceData {
  team: {team_id:string; display_name:string; team_revision:number; mission_id:string; host_agent_id:string; viewer_role:string; node_id:string; placement_generation:number|null; status:string; latest_run:LatestTeamRunSummary|null};
  works: WorkSummary[]; members: MemberCapacitySummary[]; messages: MessageSummary[]; activity:TeamActivitySummary[]; activity_truncated:boolean; pressure_summary:TeamPressureSummary;
  reports: RoleRecordSummary[]; findings: RoleRecordSummary[]; failures: RoleRecordSummary[]; gate_requirements: RoleRecordSummary[];
  gate_evaluations: RoleRecordSummary[]; gate_waivers: RoleRecordSummary[]; workspace_attention: RoleRecordSummary[];
  delegation_provenance: RoleRecordSummary[]; collaboration:CollaborationProjectionSummary; page: {as_of_event_sequence:number;item_count:number;next_cursor:string|null}; runtime_fabric:RuntimeFabricSummary;
}
/** One Team-subject canonical delivery in the shared Team Inbox (DOC-106). */
export interface TeamInboxItem {
  delivery_id: string; delivery_version: number; delivery_status: string; attempt: number;
  claim_id: string | null; claimed_node_daemon_generation: number | null;
  resolved_team_membership_id: string | null; recipient_agent_member_id: string | null;
  subscription_id: string; subscription_revision: number;
  message_id: string; created_at: string; updated_at: string;
  message: {
    kind: string; body: string; body_digest?: string | null; content_fingerprint?: string | null;
    sender_actor_ref: ActorRef; sender_agent_member_id?: string | null; sender_session_id?: string | null;
    source_team_id?: string | null; source_execution_space_id?: string | null; source_node_id?: string | null;
    collaboration_scope?: string | null; correlation_id: string; causation_id?: string | null;
    work_id?: string | null; response_intent?: string | null; created_at: string;
  } | null;
}
export interface TeamInboxData {
  team: { team_id: string; display_name: string; team_revision: number; node_id: string; status: string };
  subscription: Record<string, unknown> | null;
  items: TeamInboxItem[];
  page: { as_of_event_sequence: number; item_count: number; next_cursor: string | null };
}
export interface MissionContextSummary {id:string; title:string; objective:string; context:string; desired_outcome:string|null; status:string; outcome_summary:string|null; created_at:string; updated_at:string; completed_at:string|null; log:Array<{id:string;revision:number;kind:string;body:string;actor:string;created_at:string}>}
export interface TeamSupervisorSummary {team_run_id:string; supervisor_id:string; generation:number; current:boolean; heartbeat_unix_ms:number; expires_unix_ms:number; owner_locator:string; node_daemon_generation:number; status:string}
export interface HostConsoleData {
  team_ref:string; mission_ref:string; all_works:WorkSummary[]; work_queues:Record<string,WorkSummary[]>;
  member_capacity:MemberCapacitySummary[]; convergence_plans:RoleRecordSummary[]; reusable_findings:RoleRecordSummary[];
  workspace_conflicts:RoleRecordSummary[]; provider_capacity_attention:Array<{state:"not_modeled";reason:string}>; deliveries_requiring_reconcile:RoleRecordSummary[];
  gate_attention:RoleRecordSummary[]; daemon_summary:{node_id:string;lease_status:string|null;generation:number|null};
  mission_context:MissionContextSummary|null; team_supervisor:TeamSupervisorSummary|null; host_inbox:MessageSummary[];
  member_runtime:MemberCapacitySummary[]; runtime_recovery:RoleRecordSummary[]; pressure_summary:TeamPressureSummary; collaboration:CollaborationProjectionSummary; runtime_fabric:RuntimeFabricSummary;
}
export type ProviderObservationSemanticKind =
  | "authored_response" | "reasoning_summary"
  | "tool_call_requested" | "tool_call_started" | "tool_call_completed" | "tool_call_failed"
  | "artifact_created" | "usage_reported" | "interaction_required" | "interaction_resolved"
  | "runtime_started" | "runtime_ready" | "runtime_stopped" | "transport_interrupted"
  | "turn_completed" | "turn_failed" | "turn_cancelled"
  | "command_recovery_required" | "malformed_or_incomplete";
export type ProviderObservationPayload =
  | {type:"authored_response";text:string}
  | {type:"reasoning_summary";summary:string}
  | {type:"tool";tool_name:string;call_id?:string|null;display_detail?:string|null}
  | {type:"artifact";display_name:string;media_type?:string|null;content_digest?:string|null}
  | {type:"usage";input_tokens?:number|null;output_tokens?:number|null;total_tokens?:number|null}
  | {type:"interaction";reason_code:string;prompt:string}
  | {type:"runtime";state:string}
  | {type:"transport";reason_code:string}
  | {type:"turn";outcome:string;display_summary?:string|null}
  | {type:"recovery";reason_code:string}
  | {type:"malformed";reason_code:string};
export interface ProviderObservation {
  schema_version:"agentfirm.provider_observation.v1"; observation_id:string; provider:"codex"|"claude"|"kimi"|"pi";
  adapter_version:"agentfirm.provider_event_adapter.v1"; native_source_ref:string; agent_identity_id:string; agent_session_id:string;
  agent_session_generation:number; node_daemon_id:string; node_daemon_generation:number; provider_thread_id?:string|null;
  provider_turn_id?:string|null; provider_event_id?:string|null; ordering_position:number; causal_parent_id?:string|null;
  correlation_id?:string|null; runtime_command_id?:string|null; occurred_at:string|null; observed_at:string;
  semantic_kind:ProviderObservationSemanticKind; lifecycle_phase:"requested"|"started"|"progress"|"terminal"|"recovery";
  completeness:"partial"|"complete"|"incomplete"|"recovery_required"; effect_certainty:"none"|"not_applied"|"applied"|"unknown";
  visibility:"session_owner_private"|"team_public"|"operator_only"; redacted:boolean; truncated:boolean;
  source_content_fingerprint:string; payload:ProviderObservationPayload;
}
export interface SessionEventProjection {
  schema_version:"agentfirm.provider_observation.v1"; agent_session_id:string|null; agent_session_generation:number|null;
  source_snapshot_fingerprint:string|null; episodes:Array<{episode_id:string;provider_turn_id:string|null;observations:ProviderObservation[];terminal:boolean;incomplete:boolean}>;
  truncated:boolean; disabled_reason:string|null;
}
export interface LiveProviderActivityItem {
  runtime_event_locator:string; kind:"thinking"|"response_streaming"|"tool_started"|"tool_completed"|"tool_failed"|"interaction_waiting";
  provider:"codex"|"claude"|"kimi"|"pi"; display_summary:string; emitted_unix_ms:number; expires_unix_ms:number;
}
export interface LiveProviderActivity {
  schema_version:"agentfirm.live_provider_activity.v1"; durability:"volatile_process_memory"; replayable:false;
  execution_space_id:string; project_id:string; team_run_id:string; member_run_id:string; agent_session_id:string;
  member_run_generation:number; agent_session_generation:number; runtime_snapshot_locator:string; expires_unix_ms:number; items:LiveProviderActivityItem[];
}
export interface LiveProviderActivityEvent {
  schema_version:"agentfirm.live_provider_activity_event.v1"; reason:"updated"|"terminal";
  scope:{execution_space_id:string;project_id:string;team_run_id:string;member_run_id:string;member_run_generation:number;agent_session_id:string;agent_session_generation:number};
  activity:LiveProviderActivity|null;
}
export type HostSessionMode = "harness_managed"|"external_interactive"|"unbound";
export interface AgentWorkspaceRosterItem extends Partial<MemberCapacitySummary> {
  agent_member_ref:ActorRef; display_name:string; role:string; is_host?:boolean;
  /** Host rows only: how the Host provider session is owned (DEV-24). */
  host_session_mode?:HostSessionMode;
}
interface AgentWorkspaceSelectedAgent {
  agent_member_ref:ActorRef;display_name:string;role:string;organization_status:string;is_host:boolean;current_member_run_ref:string|null;provider:string|null;execution_mode:string|null;runtime_status:string|null;runtime_generation:number|null;
  /** Present (non-null) only when the selected agent is the Team Host (DEV-24). */
  host_session_mode?:HostSessionMode|null;
}
interface AgentWorkspaceConfiguration {
  description:string|null;prompt_ref:string|null;prompt_projection:string;skill_refs:string[];capabilities:string[];tool_refs:string[];tools_projection:string;provider_profile_ref:string|null;model_preference:string|null;workspace_policy:string|null;permission_ceiling:string|null;forbidden_actions:string[];forbidden_actions_projection:string;workspace_binding:RoleRecordSummary|null;
}
interface AgentWorkspaceDataBase {
  team:{team_id:string;display_name:string;team_revision:number;mission_id:string;host_agent_id:string;viewer_role:"host"|"member";status:string;latest_run_id:string|null};
  selected_agent:AgentWorkspaceSelectedAgent;
  roster:AgentWorkspaceRosterItem[];
  messages:MessageSummary[];
  works:WorkSummary[];
  configuration:AgentWorkspaceConfiguration;
  context_summary:{current_work_id:string|null;message_count:number;unread_count:number;last_activity_at:string|null;authorization_count:number};
}
export type AgentWorkspacePrivateData=AgentWorkspaceDataBase&{
  projection_scope:"member_self_private"|"host_self_private";
  session_event_projection:SessionEventProjection;
  live_provider_activity:LiveProviderActivity|null;
};
export type AgentWorkspaceHostMemberPublicData=AgentWorkspaceDataBase&{
  projection_scope:"host_member_public";
  selected_agent:AgentWorkspaceSelectedAgent&{current_member_run_ref:null;provider:null;execution_mode:null;runtime_status:null;runtime_generation:null};
  session_event_projection?:never;
  live_provider_activity?:never;
  configuration:AgentWorkspaceConfiguration&{prompt_ref:null;tool_refs:[];provider_profile_ref:null;model_preference:null;workspace_policy:null;permission_ceiling:null;forbidden_actions:[];workspace_binding:null};
};
export type AgentWorkspaceData=AgentWorkspacePrivateData|AgentWorkspaceHostMemberPublicData;
export interface MemberWorkbenchData {
  agent_member:{id:string;role:string;organization_status:string}; member_run:{id:string;agent_member_id:string;team_run_id:string;coordination_status:string;runtime_status:string;runtime_generation:number;native_session_health:string}; my_works:WorkSummary[];
  eligible_ready_pool:WorkSummary[]; unread_messages:MessageSummary[]; queued_deliveries:RoleRecordSummary[];
  workspace_binding:RoleRecordSummary|null; native_session_health:string;
  report_history:RoleRecordSummary[]; finding_history:RoleRecordSummary[]; failure_history:RoleRecordSummary[]; gate_requirements:RoleRecordSummary[];
  collaboration:CollaborationProjectionSummary; runtime_fabric:RuntimeFabricSummary;
}
export interface OperatorViewData {
  node:{node_id:string;node_revision:number;daemon_generation:number|null;status:string}; build:{build_sha:string;protocol_version:string;schema_version:string};
  projects:RoleRecordSummary[]; team_supervisors:RoleRecordSummary[]; delivery_backlog:{depth:number;oldest_age_ms:number|null;recovery_required:boolean};
  runtime_recovery:RoleRecordSummary[]; provider_admission:RoleRecordSummary[]; workspace_safety:RoleRecordSummary[]; diagnostics:Array<{kind:string;state:string}>;
  remote_fabric:null|{
    company_id:string;node_id:string;state:"observed"|"offline"|"unknown"|"unavailable";reason?:string|null;
    gateway_session?:{company_id:string;node_id:string;gateway_generation:number;node_daemon_id:string;node_daemon_generation:number;control_plane_generation:number}|null;
    outbox_depth?:number;oldest_outbox_age_ms?:number;inbox_depth?:number;recovery_required?:string[];store_revision?:number;
    control_plane_online?:boolean|null;
    collaboration?:CollaborationProjectionSummary;
    control_plane_metrics?:null|{node_id:string;administrative_status:string;connection_status:string;gateway_generation:number|null;control_plane_generation:number|null;certificate_expires_at_unix_ms:number|null;queued_operations:number;oldest_queued_age_ms:number;gateway_lease_age_ms:number|null;recovery_required_operations:string[];last_assigned_route_seq:number;last_persisted_route_seq:number;reconcile_lag:number};
  };
  runtime_fabric:RuntimeFabricSummary;
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
  const expectedKind=path.includes("global-work")?"global_work":path.includes("team-workspace")?"team_workspace":path.includes("team-inbox")?"team_inbox":path.includes("host-console")?"host_console":path.includes("agent-workspace")?"agent_workspace":path.includes("member-workbench")?"member_workbench":path.includes("operator")?"operator":null;
  if(!expectedKind||body.view_kind!==expectedKind)throw new Error(`RoleView kind mismatch: expected ${String(expectedKind)}, received ${String(body.view_kind)}`);
  if(body.source_execution_space_id!==scope.space&&expectedKind!=="global_work")throw new Error("RoleView execution-space identity mismatch");
  if(!Number.isSafeInteger(body.as_of_event_sequence)||!["current","stale","unavailable","unknown"].includes(body.freshness)||!Array.isArray(body.allowed_actions)||!Array.isArray(body.attention))throw new Error("Malformed RoleView envelope");
  for(const action of body.allowed_actions){if(!action||typeof action.kind!=="string"||!action.target_ref||typeof action.target_ref.id!=="string"||!Number.isSafeInteger(action.required_version)||action.required_version<0||!(action.disabled_reason===null||typeof action.disabled_reason==="string"))throw new Error("Malformed or non-CAS RoleView action")}
  return body as RoleView<T>;
}
