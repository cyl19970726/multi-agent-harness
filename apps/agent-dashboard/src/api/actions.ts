// Maps dashboard write intents to the REAL harness HTTP routes.
//
// The backend (crates/harness-cli/src/main.rs `handle_http_action`) exposes:
//   POST /v1/messages                          { from, to, content, kind, task }
//   POST /v1/agents/{id}/deliver               { start_runtime?, dry_run?, ... }
//   POST /v1/agents/{id}/retry-delivery        { message_id, ... }
//   POST /v1/agents/{id}/reconcile-session     { session_id, status, ... }
//   POST /v1/agents/{id}/close                 {}
//   POST /v1/tasks/{id}/request-review         { from_agent_id?, to_agent_id?, content? }
//
// The agent id / task id belong in the URL PATH, never the body. The earlier
// UI posted /v1/actions/* with the id in the body, so every write 400'd. This
// module is the single seam translating each intent into the correct request.

export interface ActionDescriptor {
  method: "POST";
  path: string;
  body: Record<string, unknown>;
}

function encodeId(id: string): string {
  return encodeURIComponent(id);
}

/**
 * Queue a message to a member. `to` is the recipient member id; `from` is the
 * authoring agent (typically the team lead/owner). Both `from` and `content`
 * are required by the backend, so callers must supply them.
 */
export function messageMember(params: {
  from: string;
  to: string;
  content: string;
  kind?: string;
  task?: string;
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
  return { method: "POST", path: "/v1/messages", body };
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
