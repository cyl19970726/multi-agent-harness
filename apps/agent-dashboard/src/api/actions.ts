// Maps dashboard write intents to the REAL harness HTTP routes.
//
// The backend (crates/harness-cli/src/main.rs `handle_http_action`) exposes:
//   POST /v1/messages                          { from, to, content, kind, task, sender_kind }
//   POST /v1/teams                             { name, description, owner }
//   POST /v1/agents                            { name, role, provider?, skill[], team[], ... }
//   POST /v1/goals                             { title, objective, owner, success[], priority? }
//   POST /v1/agents/{id}/deliver               { start_runtime?, dry_run?, ... }
//   POST /v1/agents/{id}/retry-delivery        { message_id, ... }
//   POST /v1/agents/{id}/reconcile-session     { session_id, status, ... }
//   POST /v1/agents/{id}/close                 {}
//   POST /v1/tasks/{id}/request-review         { from_agent_id?, to_agent_id?, content? }
//   POST /v1/tasks/{id}/assign                  { assignee }
//
// The agent id / task id belong in the URL PATH, never the body. The earlier
// UI posted /v1/actions/* with the id in the body, so every write 400'd. This
// module is the single seam translating each intent into the correct request.

export interface ActionDescriptor {
  method: "POST";
  path: string;
  body: Record<string, unknown>;
}

/**
 * The synthetic identity the dashboard authors operator messages as. The
 * backend keys delivery off the recipient member, so `from` does not need to be
 * a real member id — `sender_kind=operator` marks the row as operator-authored
 * (vs an agent), and `from="operator"` keeps the conversation attributable to
 * the human driving the team rather than impersonating the Lead.
 */
export const OPERATOR_ID = "operator";

function encodeId(id: string): string {
  return encodeURIComponent(id);
}

/**
 * Queue a message to a member. `to` is the recipient member id; `from` is the
 * authoring identity. Both `from` and `content` are required by the backend.
 *
 * `senderKind` marks the message's identity class (additive Message.sender_kind,
 * WP-i): omit it (defaults agent-side) for an agent-authored message, or pass
 * `"operator"` for an operator/human-authored one. The dashboard composer
 * authors as the operator (`from=OPERATOR_ID`, `senderKind="operator"`), never
 * impersonating the Lead.
 */
export function messageMember(params: {
  from: string;
  to: string;
  content: string;
  kind?: string;
  task?: string;
  senderKind?: "agent" | "operator" | "system";
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    from: params.from,
    to: params.to,
    content: params.content,
    kind: params.kind ?? "message",
  };
  if (params.task) {
    body.task = params.task;
  }
  if (params.senderKind) {
    body.sender_kind = params.senderKind;
  }
  return { method: "POST", path: "/v1/messages", body };
}

/**
 * Author a message as the OPERATOR (the human driving the team). Sets
 * `from=OPERATOR_ID` + `sender_kind=operator` so the row is attributable to the
 * operator and renders distinctly from agent messages — it does NOT impersonate
 * the team Lead.
 */
export function operatorMessage(params: {
  to: string;
  content: string;
  kind?: string;
  task?: string;
}): ActionDescriptor {
  return messageMember({
    from: OPERATOR_ID,
    to: params.to,
    content: params.content,
    kind: params.kind,
    task: params.task,
    senderKind: "operator",
  });
}

/**
 * Create a new team. POST /v1/teams requires name, description and owner (the
 * Lead/owner agent id). Returns the created AgentTeam in the action result.
 */
export function createTeam(params: {
  name: string;
  description: string;
  owner: string;
}): ActionDescriptor {
  return {
    method: "POST",
    path: "/v1/teams",
    body: {
      name: params.name,
      description: params.description,
      owner: params.owner,
    },
  };
}

/**
 * Create a new Agent Member. POST /v1/agents requires name and role; provider
 * (codex|claude), description, skills and team membership are optional. Does NOT
 * start a runtime — that stays a separate action.
 */
export function createAgent(params: {
  name: string;
  role: string;
  provider?: string;
  model?: string;
  description?: string;
  skills?: string[];
  teamIds?: string[];
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    name: params.name,
    role: params.role,
  };
  if (params.provider) {
    body.provider = params.provider;
  }
  if (params.model) {
    body.model = params.model;
  }
  if (params.description) {
    body.description = params.description;
  }
  // The backend reads repeatable `--skill` / `--team` flags as string arrays
  // off the `skill` / `team` JSON keys.
  if (params.skills && params.skills.length) {
    body.skill = params.skills;
  }
  if (params.teamIds && params.teamIds.length) {
    body.team = params.teamIds;
  }
  return { method: "POST", path: "/v1/agents", body };
}

/**
 * Create a new Goal. POST /v1/goals requires title, objective and owner (the
 * Lead). Success criteria and priority are optional.
 */
export function createGoal(params: {
  title: string;
  objective: string;
  owner: string;
  success?: string[];
  priority?: string;
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    title: params.title,
    objective: params.objective,
    owner: params.owner,
  };
  if (params.success && params.success.length) {
    body.success = params.success;
  }
  if (params.priority) {
    body.priority = params.priority;
  }
  return { method: "POST", path: "/v1/goals", body };
}

/**
 * Deliver this member's queued messages. The backend keys delivery off the
 * agent id in the URL path; the body only carries optional delivery options.
 */
export function deliverQueued(
  agentId: string,
  options: { startRuntime?: boolean; dryRun?: boolean } = {},
): ActionDescriptor {
  const body: Record<string, unknown> = {};
  if (options.startRuntime != null) {
    body.start_runtime = options.startRuntime;
  }
  if (options.dryRun != null) {
    body.dry_run = options.dryRun;
  }
  return { method: "POST", path: `/v1/agents/${encodeId(agentId)}/deliver`, body };
}

/**
 * Retry a previously failed delivery for a member's specific message.
 */
export function retryDelivery(
  agentId: string,
  params: { messageId: string; sessionId?: string; reason?: string; force?: boolean },
): ActionDescriptor {
  const body: Record<string, unknown> = { message_id: params.messageId };
  if (params.sessionId) {
    body.session_id = params.sessionId;
  }
  if (params.reason) {
    body.reason = params.reason;
  }
  if (params.force != null) {
    body.force = params.force;
  }
  return {
    method: "POST",
    path: `/v1/agents/${encodeId(agentId)}/retry-delivery`,
    body,
  };
}

/**
 * Reconcile a stuck provider session for a member to a terminal state.
 */
export function reconcileSession(
  agentId: string,
  params: { sessionId: string; status?: string; terminalSource?: string; reason?: string },
): ActionDescriptor {
  const body: Record<string, unknown> = { session_id: params.sessionId };
  if (params.status) {
    body.status = params.status;
  }
  if (params.terminalSource) {
    body.terminal_source = params.terminalSource;
  }
  if (params.reason) {
    body.reason = params.reason;
  }
  return {
    method: "POST",
    path: `/v1/agents/${encodeId(agentId)}/reconcile-session`,
    body,
  };
}

/**
 * Close a member, tearing down its runtime.
 */
export function closeMember(agentId: string): ActionDescriptor {
  return { method: "POST", path: `/v1/agents/${encodeId(agentId)}/close`, body: {} };
}

/**
 * Assign a task to an agent. The backend keys assignment off the task id in the
 * URL path; the body carries the `assignee` agent id (POST /v1/tasks/{id}/assign).
 */
export function assignTask(taskId: string, assignee: string): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/tasks/${encodeId(taskId)}/assign`,
    body: { assignee },
  };
}

/**
 * Set a task's reviewer (the `@reviewer` gesture). POST /v1/tasks/{id}/reviewer
 * records `reviewer_agent_id` on the existing field WITHOUT a status change or a
 * queued message — naming a reviewer is not the same as handing the work off.
 * Review delivery is the separate `requestReview` hand-off.
 */
export function setReviewer(taskId: string, reviewer: string): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/tasks/${encodeId(taskId)}/reviewer`,
    body: { reviewer },
  };
}

/**
 * Request review of a task. `from` and `reviewer` default server-side to the
 * task's owner / reviewer when omitted, so an empty descriptor body is valid.
 */
export function requestReview(
  taskId: string,
  params: { from?: string; reviewer?: string; content?: string } = {},
): ActionDescriptor {
  const body: Record<string, unknown> = {};
  if (params.from) {
    body.from_agent_id = params.from;
  }
  if (params.reviewer) {
    body.to_agent_id = params.reviewer;
  }
  if (params.content) {
    body.content = params.content;
  }
  return {
    method: "POST",
    path: `/v1/tasks/${encodeId(taskId)}/request-review`,
    body,
  };
}
