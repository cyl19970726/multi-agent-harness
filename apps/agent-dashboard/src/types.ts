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
  success_criteria?: string[];
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
}

export interface Decision {
  id: string;
  task_id: string;
  decision?: string;
  rationale?: string;
  evidence_ids?: string[];
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
  evidence?: Evidence[];
  decisions?: Decision[];
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
