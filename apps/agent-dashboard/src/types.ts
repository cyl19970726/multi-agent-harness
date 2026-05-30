export type TaskStatus = "planned" | "assigned" | "running" | "blocked" | "review" | "done" | "archived";
export type DeliveryStatus = "queued" | "delivered" | "acknowledged" | "failed";
export type MessageKind = "message" | "task" | "report";
export type ProviderSessionStatus = "queued" | "running" | "succeeded" | "failed" | "canceled" | "stale";

export interface Goal {
  id: string;
  title?: string;
  objective?: string;
  owner_agent_id?: string;
  status?: string;
  priority?: string;
  success_criteria?: string[];
  created_at?: string;
  updated_at?: string;
  vision_id?: string | null;
  goal_design_id?: string | null;
  closed_by_decision_id?: string | null;
}

export interface Task {
  id: string;
  goal_id?: string | null;
  parent_task_id?: string | null;
  title?: string;
  objective?: string;
  owner_agent_id?: string;
  assignee_agent_id?: string | null;
  reviewer_agent_id?: string | null;
  status: TaskStatus;
  depends_on_task_ids?: string[];
  workspace_ref?: string | null;
  branch_ref?: string | null;
  pr_ref?: string | null;
  owned_paths?: string[];
  acceptance_criteria?: string[];
  created_at?: string;
  updated_at?: string;
  phase?: string | null;
  scope_refs?: string[];
  requires_human_approval?: boolean;
  verdict_decision_id?: string | null;
}

export interface AgentMember {
  id: string;
  name?: string;
  description?: string;
  role?: string;
  provider?: string;
  status?: string;
  runtime_status?: string | null;
  runtime_id?: string | null;
  runtime_pid?: number | null;
  runtime_alive?: boolean;
  runtime_health?: Record<string, unknown> | null;
  control_endpoint?: string | null;
  provider_thread_id?: string | null;
  provider_agent_path?: string | null;
  provider_agent_nickname?: string | null;
  provider_agent_role?: string | null;
  current_task_id?: string | null;
  current_proposal_id?: string | null;
  prompt_ref?: string | null;
  skill_refs?: string[];
  queued_count?: number;
  inbox_count?: number;
  team_ids?: string[];
  provider_child_thread_count?: number;
}

export interface AgentTeam {
  id: string;
  name?: string;
  description?: string;
  owner_agent_id?: string;
  status?: "active" | "closed" | "archived";
  member_ids?: string[];
}

export interface Message {
  id: string;
  task_id?: string | null;
  from_agent_id?: string;
  to_agent_id?: string | null;
  channel?: string | null;
  kind: MessageKind;
  delivery_status: DeliveryStatus;
  content?: string;
  evidence_ids?: string[];
  created_at?: string;
  delivery?: MessageDelivery | null;
}

export interface MessageDelivery {
  provider_session_id?: string | null;
  provider_request_id?: string | null;
  provider_thread_id?: string | null;
  provider_turn_id?: string | null;
  terminal_source?: string | null;
  delivered_at?: string | null;
  last_error?: string | null;
}

export interface ProviderSession {
  id: string;
  provider?: string;
  agent_member_id?: string;
  task_id?: string | null;
  workspace_ref?: string | null;
  provider_thread_id?: string | null;
  provider_turn_id?: string | null;
  terminal_source?: string | null;
  status?: ProviderSessionStatus | string;
  command?: string;
  args?: string[];
  prompt_ref?: string | null;
  prompt_summary?: string | null;
  provider_session_ref?: string | null;
  exit_code?: number | null;
  stdout_ref?: string | null;
  stderr_ref?: string | null;
  jsonl_ref?: string | null;
  transcript_ref?: string | null;
  last_message_ref?: string | null;
  started_at?: string;
  ended_at?: string | null;
  evidence_ids?: string[];
}

export interface AgentEvent {
  id: string;
  agent_member_id?: string;
  provider_runtime_id?: string | null;
  task_id?: string | null;
  event_type?: string;
  summary?: string;
  payload_ref?: string | null;
  created_at?: string;
}

export interface Proposal {
  id: string;
  task_id: string;
  agent_member_id?: string;
  title?: string;
  summary?: string;
  status?: string;
  changed_paths?: string[];
  evidence_ids?: string[];
}

export interface ProviderChildThread {
  id: string;
  provider?: string;
  agent_member_id?: string;
  provider_runtime_id?: string | null;
  task_id?: string | null;
  parent_provider_thread_id?: string | null;
  provider_thread_id?: string;
  provider_agent_path?: string | null;
  provider_agent_nickname?: string | null;
  provider_agent_role?: string | null;
  status?: string;
  last_message_ref?: string | null;
  created_at?: string;
  updated_at?: string;
}

export interface Evidence {
  id: string;
  task_id?: string | null;
  source_type?: string;
  source_ref?: string;
  summary?: string;
  evidence_kind?: string | null;
  goal_id?: string | null;
}

export interface Decision {
  id: string;
  task_id: string;
  decision?: string;
  rationale?: string;
  evidence_ids?: string[];
  decision_kind?: string | null;
  goal_id?: string | null;
  is_waiver?: boolean;
  follow_up_task_id?: string | null;
}

export interface Review {
  id: string;
  task_id?: string | null;
  goal_id?: string | null;
  reviewer_agent_id?: string;
  review_kind?: string;
  /** Open enum: pass/fail/blocked/needs_changes, or an adapter-supplied value. */
  verdict?: string;
  summary?: string;
  blockers?: string[];
  residual_risk?: string | null;
  missing_validation?: string[];
  evidence_ids?: string[];
  created_at?: string;
}

export interface AutonomousProposal {
  id: string;
  kind?: string;
  source_type?: string;
  source_ref?: string;
  summary?: string;
  task_id?: string | null;
  goal_id?: string | null;
  created_at?: string;
  message_id?: string | null;
  from_agent_id?: string | null;
  to_agent_id?: string | null;
  linked_evidence_ids?: string[];
  disposition?: "pending" | "accepted" | "rejected" | "deferred" | "request_evidence" | "decided" | string;
  decision_id?: string | null;
  decision_rationale?: string | null;
  follow_up_task_ids?: string[];
  follow_up_goal_ids?: string[];
}

export interface GoalLearningStatus {
  goal_id: string;
  ok?: boolean;
  warnings?: string[];
  task_ids?: string[];
  goal_design?: unknown[];
  goal_evaluation?: unknown[];
  goal_cases?: { source_ref?: string; id?: string }[];
  follow_up_tasks?: Task[];
  member_reports?: unknown[];
  decisions?: unknown[];
  event_order?: Record<string, unknown>;
}

export interface DashboardSnapshot {
  generated_at?: string;
  goals?: Goal[];
  teams?: AgentTeam[];
  members?: AgentMember[];
  kanban?: Record<TaskStatus, string[]>;
  tasks?: Task[];
  messages?: Message[];
  events?: AgentEvent[];
  proposals?: Proposal[];
  autonomous_proposals?: AutonomousProposal[];
  evidence?: Evidence[];
  decisions?: Decision[];
  reviews?: Review[];
  provider_sessions?: ProviderSession[];
  provider_child_threads?: ProviderChildThread[];
  goal_learning_status?: GoalLearningStatus[];
}

export interface WorkflowWarning {
  id: string;
  kind: string;
  severity: "high" | "medium" | "low";
  goalId?: string;
  taskId?: string;
  memberId?: string;
  proposalId?: string;
  decisionId?: string;
  sessionId?: string;
  evidenceId?: string;
  summary: string;
}

export type DashboardAction = (path: string, body?: unknown) => Promise<void>;
