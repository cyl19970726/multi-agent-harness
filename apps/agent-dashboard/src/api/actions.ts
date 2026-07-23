// Maps dashboard write intents to the REAL harness HTTP routes.
//
// The backend (crates/harness-cli/src/main.rs `handle_http_action`) exposes:
//   POST /v1/messages                          { from, to, content, kind, task, sender_kind }
//   POST /v1/teams                             { name, description, owner }
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
  executionMode?: "codex_exec" | "codex_app_server" | "kimi_acp" | "claude_cli";
  /** Optional member-specific workspace override validated against project_root. */
  worktreeRef?: string;
  /** Paths the member may modify; empty/omitted means read-only. */
  ownedPaths?: string[];
}

/**
 * Create a new Agent Team run with its member roster (POST /v1/team-runs). The
 * response carries the refreshed snapshot, which App's runAction adopts; the
 * new run then appears at the top of the Team list.
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
      if (member.executionMode) {
        spec.execution_mode = member.executionMode;
      }
      if (member.worktreeRef) {
        spec.worktree_ref = member.worktreeRef;
      }
      if (member.ownedPaths && member.ownedPaths.length) {
        spec.owned_paths = member.ownedPaths;
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

/** Add an ordered native Wave to a Mission (POST /v1/waves). */
export function createWave(params: {
  missionId: string;
  title: string;
  objective: string;
  executorKind?: "agent_team" | "dynamic_workflow" | "host";
  index?: number;
  exitCriteria?: string;
  planNote?: string;
  context?: string;
}): ActionDescriptor {
  const body: Record<string, unknown> = {
    mission_id: params.missionId,
    title: params.title,
    objective: params.objective,
    executor_kind: params.executorKind ?? "host",
  };
  if (params.index != null) body.index = params.index;
  if (params.exitCriteria) body.exit_criteria = params.exitCriteria;
  if (params.planNote) body.plan_note = params.planNote;
  if (params.context) body.context = params.context;
  return { method: "POST", path: "/v1/waves", body };
}

export function updateMissionContext(missionId: string, context: string): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/missions/${encodeId(missionId)}/context`,
    body: { context },
  };
}

export function linkMissionTeam(missionId: string, teamId: string): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/missions/${encodeId(missionId)}/link-team`,
    body: { team_id: teamId },
  };
}

export function createMissionTeam(params: {
  missionId: string;
  name: string;
  description: string;
  owner?: string;
  memberIds?: string[];
}): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/missions/${encodeId(params.missionId)}/teams`,
    body: {
      name: params.name,
      description: params.description,
      owner: params.owner ?? "host",
      member: params.memberIds ?? [],
    },
  };
}

export function updateWaveContext(
  waveId: string,
  context: string,
  updatedBy = "host",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/waves/${encodeId(waveId)}/context`,
    body: { context, updated_by: updatedBy },
  };
}

export function advanceWave(params: {
  waveId: string;
  outcome: string;
  advancedBy?: string;
  artifactRefs?: string[];
}): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/waves/${encodeId(params.waveId)}/advance`,
    body: {
      outcome: params.outcome,
      advanced_by: params.advancedBy ?? "host",
      artifact_refs: params.artifactRefs ?? [],
    },
  };
}

/** Record a Wave gate result without rewriting its attempt history. */
export function gateWave(params: {
  waveId: string;
  status: "accepted" | "revise" | "blocked";
  runId?: string;
  acceptedBy?: string;
  note?: string;
  outcome?: string;
  artifactRefs?: string[];
}): ActionDescriptor {
  const body: Record<string, unknown> = { status: params.status };
  if (params.runId) body.run_id = params.runId;
  if (params.acceptedBy) body.accepted_by = params.acceptedBy;
  if (params.note) body.note = params.note;
  if (params.outcome) body.outcome = params.outcome;
  if (params.artifactRefs?.length) body.artifact_refs = params.artifactRefs;
  return { method: "POST", path: `/v1/waves/${encodeId(params.waveId)}/gate`, body };
}

/**
 * Send a message on a team run's handoff chain (POST /v1/team-runs/{id}/messages).
 * `fromMemberId` is "host" or a member run id; `toMemberIds` lists recipients.
 */
export function sendTeamMessage(
  teamRunId: string,
  params: {
    fromMemberId: string;
    toMemberIds: string[];
    kind: string;
    body: string;
    /**
     * Reuse an existing assignment's correlation only when the operator has
     * explicitly selected that assignment as this message's ownership anchor.
     */
    correlationId?: string;
    /** The assignment message that caused this anchored follow-up. */
    causationId?: string;
    originWaveId?: string;
  },
): ActionDescriptor {
  const body: Record<string, unknown> = {
    from_member_id: params.fromMemberId,
    to_member_ids: params.toMemberIds,
    kind: params.kind,
    body: params.body,
  };
  if (params.correlationId) {
    body.correlation_id = params.correlationId;
  }
  if (params.causationId) {
    body.causation_id = params.causationId;
  }
  if (params.originWaveId) {
    body.origin_wave_id = params.originWaveId;
  }
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/messages`,
    body,
  };
}

/** Acknowledge one delivered TeamMessage recipient row. */
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

/** Resolve a provider-originated question, approval, or plan review and resume
 * the same provider turn when its execution mode supports that contract. */
export function resolvePendingInteraction(
  teamRunId: string,
  interactionId: string,
  optionId: string,
  resolvedBy: "host" | "lead" | "operator" | "human" | "policy" = "host",
): ActionDescriptor {
  return {
    method: "POST",
    path: `/v1/team-runs/${encodeId(teamRunId)}/interactions/${encodeId(interactionId)}/resolve`,
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
