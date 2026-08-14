// Maps dashboard write intents to the REAL harness HTTP routes.
//
// The backend (crates/harness-cli/src/main.rs `handle_http_action`) exposes:
//   POST /v1/messages                          { from, to, content, kind, task, sender_kind }
//   POST /v1/teams                             { name, description, lead_agent_id }
//   POST /v1/agents                            { name, role, provider?, skill[], team[], ... }
//   POST /v1/agents/{id}/deliver               { start_runtime?, dry_run?, ... }
//   POST /v1/agents/{id}/retry-delivery        { message_id, ... }
//   POST /v1/agents/{id}/reconcile-delivery    { delivery_id, status, ... }
//   POST /v1/agents/{id}/close                 {}
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
 * Create the one flat Team for a Mission on an immutable Node placement.
 * Returns the created AgentTeam in the action result.
 */
export function createTeam(params: {
  name: string;
  description: string;
  missionId: string;
  hostAgentId: string;
  nodeId: string;
  memberIds?: string[];
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    name: params.name,
    description: params.description,
    mission_id: params.missionId,
    host_agent_id: params.hostAgentId,
    node_id: params.nodeId,
  };
  if (params.memberIds && params.memberIds.length) {
    body.member = params.memberIds;
  }
  return {
    method: "POST",
    path: "/v1/teams",
    body,
  };
}

/**
 * Create a new Agent Member. POST /v1/agents requires name and role; provider
 * (kimi|codex|claude|pi), description, skills and team membership are
 * optional. Does NOT start a runtime — that stays a separate action.
 */
export function createAgent(params: {
  name: string;
  role: string;
  provider?: string;
  model?: string;
  description?: string;
  skills?: string[];
  teamIds?: string[];
  profile?: string;
  permissionProfile?: string;
  approvalPolicy?: string;
  sandboxPolicy?: string;
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
  if (params.profile) {
    body.profile = params.profile;
  }
  if (params.permissionProfile) {
    body.permission_profile = params.permissionProfile;
  }
  if (params.approvalPolicy) {
    body.approval_policy = params.approvalPolicy;
  }
  if (params.sandboxPolicy) {
    body.sandbox_policy = params.sandboxPolicy;
  }
  return { method: "POST", path: "/v1/agents", body };
}

/**
 * Append one append-only Mission Log entry (POST /v1/missions/{id}/log),
 * the ADR 0051 replacement for retired Wave writes. Kinds mirror the CLI:
 * judgment | replan | recovery | closeout_evidence.
 */
export function appendMissionLog(params: {
  missionId: string;
  kind: "judgment" | "replan" | "recovery" | "closeout_evidence";
  body: string;
  actor?: string;
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    kind: params.kind,
    body: params.body,
  };
  if (params.actor) {
    body.actor = params.actor;
  }
  return {
    method: "POST",
    path: `/v1/missions/${encodeId(params.missionId)}/log`,
    body,
  };
}

/**
 * Add one member to an existing TeamRun (POST /v1/team-runs/{id}/members).
 * A live Supervisor picks the queued MemberRun up; without one the member
 * waits until the run is (re)started or the member is reopened.
 */
export function addTeamMember(params: {
  teamRunId: string;
  name: string;
  role: string;
  provider: string;
  model?: string;
  executionMode?: string;
  resumeNativeSessionId?: string;
  initialWork?: string;
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    name: params.name,
    role: params.role,
    provider: params.provider,
  };
  if (params.model) body.model = params.model;
  if (params.executionMode) body.execution_mode = params.executionMode;
  if (params.resumeNativeSessionId) body.resume_native_session_id = params.resumeNativeSessionId;
  if (params.initialWork) body.initial_work = params.initialWork;
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(params.teamRunId)}/members`,
    body,
  };
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
 * Reconcile a stuck Harness delivery attempt to a terminal state.
 */
export function reconcileDelivery(
  agentId: string,
  params: { deliveryId: string; status?: string; terminalSource?: string; reason?: string },
): ActionDescriptor {
  const body: Record<string, unknown> = { delivery_id: params.deliveryId };
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
    path: `/v1/agents/${encodeId(agentId)}/reconcile-delivery`,
    body,
  };
}

/**
 * Close a member, tearing down its runtime.
 */
export function closeMember(agentId: string): ActionDescriptor {
  return { method: "POST", path: `/v1/agents/${encodeId(agentId)}/close`, body: {} };
}

/* ------------------------------------------------------------------ */
/* Agent Team runs (POST /v1/team-runs…, team-console)                 */
/* ------------------------------------------------------------------ */

/** One member slot of a {@link createTeamRun} request. */
export interface TeamRunMemberSpec {
  name: string;
  role: string;
  provider: string;
  model?: string;
  effort?: string;
  serviceTier?: string;
  executionMode?: "codex_app_server" | "kimi_acp" | "claude_agent_sdk" | "pi_rpc";
  /** Optional member-specific workspace override validated against project_root. */
  worktreeRef?: string;
  /** Paths the member may modify; empty/omitted means read-only. */
  ownedPaths?: string[];
  /** Optional first Work. Omit to create an idle, addressable member. */
  initialWork?: string;
}

/**
 * Create a new Agent Team run with its member roster (POST /v1/team-runs). The
 * the Dashboard performs a bounded mutation then refreshes the snapshot; the
 * new run appears when that read completes.
 */
export function createTeamRun(params: {
  objective: string;
  budgetLimitUsd?: number;
  /** Retry lineage: an earlier attempt of this same native Wave. */
  previousRunId?: string;
  /** Stable AgentTeam definition; primary Mission-scoped runs omit waveId. */
  agentTeamId?: string;
  missionId?: string;
  waveId?: string;
  /** Optional TeamRun workspace; defaults to the selected registered project_root. */
  executionRoot?: string;
  members: TeamRunMemberSpec[];
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    objective: params.objective,
    members: params.members.map((member) => {
      const spec: Record<string, unknown> = {
        name: member.name,
        role: member.role,
        provider: member.provider,
      };
      if (member.model) {
        spec.model = member.model;
      }
      if (member.effort) {
        spec.effort = member.effort;
      }
      if (member.serviceTier) {
        spec.service_tier = member.serviceTier;
      }
      if (member.executionMode) {
        spec.execution_mode = member.executionMode;
      }
      if (member.worktreeRef) {
        spec.provider_cwd_hint = member.worktreeRef;
      }
      if (member.ownedPaths && member.ownedPaths.length) {
        spec.owned_paths = member.ownedPaths;
      }
      if (member.initialWork) {
        spec.initial_work = member.initialWork;
      }
      return spec;
    }),
  };
  if (params.budgetLimitUsd != null) {
    body.budget_limit_usd = params.budgetLimitUsd;
  }
  if (params.previousRunId) {
    body.previous_run_id = params.previousRunId;
  }
  if (params.agentTeamId) {
    body.agent_team_id = params.agentTeamId;
  }
  if (params.missionId) {
    body.mission_id = params.missionId;
  }
  if (params.waveId) {
    body.wave_id = params.waveId;
  }
  if (params.executionRoot) {
    body.execution_root = params.executionRoot;
  }
  return { method: "POST", path: "/v1/team-runs", body };
}

/** Create native Mission intent (POST /v1/missions). */
export function createMission(params: {
  title: string;
  objective: string;
  desiredOutcome?: string;
  context?: string;
}): ActionDescriptor {
  const body: Record<string, unknown> = { title: params.title, objective: params.objective };
  if (params.desiredOutcome) body.desired_outcome = params.desiredOutcome;
  if (params.context) body.context = params.context;
  return { method: "POST", path: "/v1/missions", body };
}

/** Explicitly complete a Mission after every ordered Wave is accepted. */
export function closeMission(params: {
  missionId: string;
  outcome: string;
  completedBy?: string;
}): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/missions/${encodeId(params.missionId)}/close`,
    body: {
      outcome: params.outcome,
      completed_by: params.completedBy ?? "host",
    },
  };
}

export function updateMissionContext(missionId: string, context: string): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/missions/${encodeId(missionId)}/context`,
    body: { context },
  };
}

/**
 * Acknowledge one HostAttention from the console (POST
 * /v1/host-attentions/{id}/ack). Transport intake only — the server walks the
 * attention lifecycle and never mutates the underlying Work.
 */
export function acknowledgeHostAttention(attentionId: string): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/host-attentions/${encodeId(attentionId)}/ack`,
    body: { acknowledged_by: "operator" },
  };
}

/**
 * Send a message on a team run's handoff chain (POST /v1/team-runs/{id}/messages).
 * `fromMemberId` is "host" or a member run id; `toMemberIds` lists recipients.
 */
export function sendTeamMessage(
  teamRunId: string,
  params: {
    fromMemberId: string;
    /** Authenticated actor class. Dashboard-authored messages are Operator,
     * never the Host or a MemberRun. */
    senderKind?: "host" | "member_run" | "agent_member" | "operator" | "service";
    senderId?: string;
    senderName?: string;
    toMemberIds: string[];
    kind: string;
    body: string;
    /** Optional Work discussed by this conversation message. */
    workId?: string;
    /** Whether this conversation should wake an idle recipient into a turn. */
    responseIntent?: "informational" | "response_required";
    /**
     * Reuse an existing conversation correlation when replying.
     */
    correlationId?: string;
    /** The direct message that caused this reply. */
    causationId?: string;
    originWaveId?: string;
  },
): ActionDescriptor {
  const body: Record<string, unknown> = {
    sender_runtime_id: params.fromMemberId,
    sender_kind: params.senderKind ?? (params.fromMemberId === "host" ? "host" : "member_run"),
    sender_id: params.senderId ?? params.fromMemberId,
    recipient_runtime_ids: params.toMemberIds,
    kind: params.kind,
    body: params.body,
  };
  if (params.senderName) {
    body.sender_name = params.senderName;
  }
  if (params.workId) {
    body.work_id = params.workId;
  }
  if (params.responseIntent) {
    body.response_intent = params.responseIntent;
  }
  if (params.correlationId) {
    body.correlation_id = params.correlationId;
  }
  if (params.causationId) {
    body.causation_id = params.causationId;
  }
  if (params.originWaveId) {
    body.source_plan_ref = params.originWaveId;
  }
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/messages`,
    body,
  };
}

export function createTeamWork(
  teamRunId: string,
  params: {
    title: string;
    contextMarkdown?: string;
    completionCriteriaMarkdown: string;
    activeMemberRunId?: string;
    claimMode?: "host_assign" | "team_claim";
    eligibleMemberIds?: string[];
    priority?: "low" | "normal" | "high" | "urgent";
    causedByMessageId?: string;
  },
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/works`,
    body: {
      title: params.title,
      context_markdown: params.contextMarkdown ?? "",
      completion_criteria_markdown: params.completionCriteriaMarkdown,
      owner_member_run_id: params.activeMemberRunId,
      claim_mode: params.claimMode ?? (params.activeMemberRunId ? "host_assign" : "team_claim"),
      eligible_member_ids: params.eligibleMemberIds ?? [],
      priority: params.priority ?? "normal",
      caused_by_message_id: params.causedByMessageId,
    },
  };
}

export function assignTeamWork(
  teamRunId: string,
  workId: string,
  memberRunId: string,
  expectedVersion: number,
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/works/${encodeId(workId)}/assign`,
    body: { member_run_id: memberRunId, expected_version: expectedVersion },
  };
}

export function reviewTeamWork(
  teamRunId: string,
  workId: string,
  expectedVersion: number,
  decision: "accept" | "request-changes",
  note?: string,
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/works/${encodeId(workId)}/${decision}`,
    body: decision === "accept"
      ? { expected_version: expectedVersion, summary: note ?? "Accepted by Host" }
      : { expected_version: expectedVersion, reason: note ?? "Host requested changes" },
  };
}

/** Acknowledge one delivered TeamMessageProjection recipient row. */
export function acknowledgeTeamMessage(
  teamRunId: string,
  messageId: string,
  memberId: string,
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/messages/${encodeId(messageId)}/ack`,
    body: { member_id: memberId },
  };
}

/** Answer a provider-originated correlated Message and resume the same provider
 * turn when its execution mode supports that contract. */
export function answerProviderMessage(
  teamRunId: string,
  messageId: string,
  optionId: string,
  resolvedBy: "host" | "lead" | "operator" | "human" | "policy" = "host",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/messages/${encodeId(messageId)}/answer`,
    body: { option_id: optionId, resolved_by: resolvedBy },
  };
}

/** Inject input into the currently active provider turn. This is only valid
 * when the MemberRun's mode advertises live steer (currently codex_app_server). */
export function steerTeamMember(
  teamRunId: string,
  memberRunId: string,
  content: string,
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/members/${encodeId(memberRunId)}/steer`,
    body: { content, requested_by: "operator" },
  };
}

/** Cooperatively interrupt the active provider turn. */
export function interruptTeamMember(
  teamRunId: string,
  memberRunId: string,
  reason = "Operator requested interruption",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/members/${encodeId(memberRunId)}/interrupt`,
    body: { reason, requested_by: "operator" },
  };
}

/** Close a Team Member runtime while retaining its resumable identity/history. */
export function closeTeamMember(
  teamRunId: string,
  memberRunId: string,
  reason = "Host closed member runtime",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/members/${encodeId(memberRunId)}/close`,
    body: { reason, requested_by: "host" },
  };
}

/** Reopen the same MemberRun; the server resumes its native session when managed. */
export function reopenTeamMember(
  teamRunId: string,
  memberRunId: string,
  reason = "Host reopened member runtime",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/members/${encodeId(memberRunId)}/reopen`,
    body: { reason, reopened_by: "host" },
  };
}

/**
 * Resume the recorded provider-native session (POST
 * /v1/team-runs/{id}/members/{m}/resume). Refuses active members — their
 * continuation is a message or steer — and otherwise reuses the reopen
 * machinery with the same capability gates.
 */
export function resumeTeamMember(
  teamRunId: string,
  memberRunId: string,
  reason = "Host resumed member native session",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/members/${encodeId(memberRunId)}/resume`,
    body: { reason, resumed_by: "operator" },
  };
}

/**
 * Start a team run's orchestration loop (POST /v1/team-runs/{id}/start). The
 * server reserves the attempt synchronously, then executes providers in the
 * background while durable and volatile updates arrive over SSE.
 */
export function startTeamRun(teamRunId: string): ActionDescriptor {
  return { method: "POST", path: `/v1/team-runs/${encodeId(teamRunId)}/start`, body: {} };
}

/**
 * Drive an attempt lifecycle (POST /v1/team-runs/{id}/transition). The native
 * Wave gate is separate: it accepts, revises, or blocks a completed attempt.
 * The backend only allows `reviewing → completed` (attempt completion) and
 * `planning|waiting|reviewing → cancelled`; running cancellation is rejected
 * until provider execution has a cooperative interruption path.
 */
export function transitionTeamRun(
  teamRunId: string,
  status: "completed" | "cancelled",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/transition`,
    body: { status },
  };
}
