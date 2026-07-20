import type {
  DashboardSnapshot,
  DelegationRun,
  LiveMemberActivity,
  MemberAction,
  MemberRun,
  Mission,
  TeamMessage,
  TeamMessageDelivery,
  TeamRun,
  TeamRunEvent,
  Wave,
} from "../types";

/**
 * Read-model selectors for the native Mission → Wave → AgentTeamRun hierarchy.
 *
 * These selectors intentionally do not project a MemberRun into a standing
 * AgentMember. A MemberRun is one participation in one TeamRun attempt, and
 * all ownership below is derived from assignment messages in that attempt.
 */

export type ActivityOrder = "asc" | "desc";

export interface TeamRunNeedsYou {
  approvals: TeamMessage[];
  waitingMembers: MemberRun[];
  blockedMembers: MemberRun[];
  unacknowledgedDeliveries: Array<{
    message: TeamMessage;
    delivery: TeamMessageDelivery;
  }>;
  total: number;
}

export interface AssignmentCorrelation {
  /** Stable locally-derived key even for assignments that predate correlation ids. */
  key: string;
  correlationId?: string;
  /** The assignment is the only ownership anchor. */
  assignment: TeamMessage;
  /** All messages in this attempt bearing the assignment's correlation id. */
  relatedMessages: TeamMessage[];
}

export interface MessageAssignmentLineage {
  message: TeamMessage;
  assignment?: TeamMessage;
  anchored: boolean;
}

export type StableTeamActivity =
  | {
      id: string;
      kind: "message";
      at?: string;
      atMs: number;
      seq?: number;
      sourceMemberRunId?: string;
      message: TeamMessage;
    }
  | {
      id: string;
      kind: "action";
      at?: string;
      atMs: number;
      seq?: number;
      sourceMemberRunId?: string;
      action: MemberAction;
    }
  | {
      id: string;
      kind: "event";
      at?: string;
      atMs: number;
      seq?: number;
      sourceMemberRunId?: string;
      event: TeamRunEvent;
    };

export interface TeamRunContext {
  run: TeamRun;
  mission?: Mission;
  wave?: Wave;
  /** All attempts attached to the parent Wave, in retry/history order. */
  attempts: TeamRun[];
  members: MemberRun[];
  memberById: Map<string, MemberRun>;
  messages: TeamMessage[];
  actions: MemberAction[];
  delegations: DelegationRun[];
  events: TeamRunEvent[];
  liveActivityByMember: Map<string, LiveMemberActivity>;
  needsYou: TeamRunNeedsYou;
  activity: StableTeamActivity[];
}

export interface MemberRunContext extends TeamRunContext {
  member: MemberRun;
  assignments: AssignmentCorrelation[];
  /** Messages which involve this member, oldest first. */
  messagesForMember: TeamMessage[];
  actionsForMember: MemberAction[];
  delegationsForMember: DelegationRun[];
  eventsForMember: TeamRunEvent[];
  activityForMember: StableTeamActivity[];
  liveActivity?: LiveMemberActivity;
}

/** Parse harness `unix-ms:<number>` timestamps as well as normal ISO values. */
export function parseTeamTimestamp(value?: string | null): number {
  if (!value) return 0;
  if (value.startsWith("unix-ms:")) {
    const epoch = Number(value.slice("unix-ms:".length));
    return Number.isFinite(epoch) ? epoch : 0;
  }
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

/** Native Mission lookup. */
export function selectMission(snapshot: DashboardSnapshot, missionId?: string): Mission | undefined {
  if (!missionId) return undefined;
  return (snapshot.missions ?? []).find((mission) => mission.id === missionId);
}

/** Ordered Waves are the Mission execution plan. */
export function selectOrderedWaves(snapshot: DashboardSnapshot, missionId?: string): Wave[] {
  if (!missionId) return [];
  return [...(snapshot.waves ?? [])]
    .filter((wave) => wave.mission_id === missionId)
    .sort((left, right) => left.index - right.index || stableIdCompare(left.id, right.id));
}

/**
 * AgentTeamRun attempts for one Wave. Explicit `executor_run_ids` order wins;
 * older snapshots without it fall back to creation time, then id.
 */
export function selectWaveAttempts(snapshot: DashboardSnapshot, wave: Wave | string | undefined): TeamRun[] {
  const resolvedWave = typeof wave === "string" ? (snapshot.waves ?? []).find((item) => item.id === wave) : wave;
  if (!resolvedWave) return [];
  const explicitOrder = new Map((resolvedWave.executor_run_ids ?? []).map((id, index) => [id, index]));
  return [...(snapshot.team_runs ?? [])]
    .filter(
      (run) =>
        run.wave_id === resolvedWave.id &&
        (!run.mission_id || run.mission_id === resolvedWave.mission_id),
    )
    .sort((left, right) => {
      const leftExplicit = explicitOrder.get(left.id);
      const rightExplicit = explicitOrder.get(right.id);
      if (leftExplicit != null || rightExplicit != null) {
        if (leftExplicit == null) return 1;
        if (rightExplicit == null) return -1;
        if (leftExplicit !== rightExplicit) return leftExplicit - rightExplicit;
      }
      return (
        parseTeamTimestamp(left.created_at) - parseTeamTimestamp(right.created_at) ||
        stableIdCompare(left.id, right.id)
      );
    });
}

/** Resolve the parent attempt without relying on an optional member_run_ids index. */
export function selectTeamRunForMemberRun(
  snapshot: DashboardSnapshot,
  memberRun: MemberRun | string | undefined,
): TeamRun | undefined {
  const resolvedMember =
    typeof memberRun === "string"
      ? (snapshot.member_runs ?? []).find((member) => member.id === memberRun)
      : memberRun;
  if (!resolvedMember) return undefined;
  return (snapshot.team_runs ?? []).find(
    (run) =>
      run.id === resolvedMember.team_run_id ||
      (run.member_run_ids ?? []).includes(resolvedMember.id),
  );
}

export function selectTeamRunContext(
  snapshot: DashboardSnapshot,
  teamRunId?: string,
): TeamRunContext | undefined {
  if (!teamRunId) return undefined;
  const run = (snapshot.team_runs ?? []).find((item) => item.id === teamRunId);
  if (!run) return undefined;

  const wave =
    (snapshot.waves ?? []).find((item) => item.id === run.wave_id) ??
    (snapshot.waves ?? []).find((item) => (item.executor_run_ids ?? []).includes(run.id));
  const mission = selectMission(snapshot, run.mission_id ?? wave?.mission_id);
  const members = (snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id === run.id || (run.member_run_ids ?? []).includes(member.id),
  );
  const memberById = new Map(members.map((member) => [member.id, member]));
  const messages = (snapshot.team_messages ?? []).filter((message) => message.team_run_id === run.id);
  const actions = (snapshot.member_actions ?? []).filter((action) => action.team_run_id === run.id);
  const delegations = (snapshot.delegation_runs ?? []).filter((delegation) => delegation.team_run_id === run.id);
  const events = (snapshot.team_run_events ?? []).filter((event) => event.team_run_id === run.id);
  const liveActivityByMember = new Map(
    Object.entries(snapshot.live_member_activity ?? {}).filter(([, activity]) => activity.team_run_id === run.id),
  );

  return {
    run,
    mission,
    wave,
    attempts: wave ? selectWaveAttempts(snapshot, wave) : [run],
    members,
    memberById,
    messages,
    actions,
    delegations,
    events,
    liveActivityByMember,
    needsYou: selectTeamRunNeedsYou(members, messages),
    activity: selectStableTeamActivity({ messages, actions, events }),
  };
}

export function selectMemberRunContext(
  snapshot: DashboardSnapshot,
  memberRunId?: string,
): MemberRunContext | undefined {
  if (!memberRunId) return undefined;
  const member = (snapshot.member_runs ?? []).find((item) => item.id === memberRunId);
  if (!member) return undefined;
  const run = selectTeamRunForMemberRun(snapshot, member);
  if (!run) return undefined;
  const team = selectTeamRunContext(snapshot, run.id);
  if (!team) return undefined;

  const messagesForMember = team.messages.filter(
    (message) =>
      message.from_member_id === member.id || (message.to_member_ids ?? []).includes(member.id),
  );
  const actionsForMember = team.actions.filter((action) => action.member_run_id === member.id);
  const delegationsForMember = team.delegations.filter(
    (delegation) => delegation.parent_member_run_id === member.id,
  );
  const eventsForMember = team.events.filter((event) => event.member_run_id === member.id);

  return {
    ...team,
    member,
    assignments: selectMemberAssignmentCorrelations(team.messages, member.id),
    messagesForMember: sortMessages(messagesForMember),
    actionsForMember: sortActions(actionsForMember),
    delegationsForMember: sortDelegations(delegationsForMember),
    eventsForMember: sortEvents(eventsForMember),
    activityForMember: selectStableTeamActivity({
      messages: messagesForMember,
      actions: actionsForMember,
      events: eventsForMember,
    }),
    liveActivity: team.liveActivityByMember.get(member.id),
  };
}

/** Assignment delivery proves the member's lane; correlation only supplies lineage. */
export function selectMemberAssignmentCorrelations(
  messages: TeamMessage[],
  memberRunId: string,
): AssignmentCorrelation[] {
  const assignments = sortMessages(
    messages.filter(
      (message) =>
        message.kind === "assignment" && (message.to_member_ids ?? []).includes(memberRunId),
    ),
  );
  return assignments.map((assignment) => {
    const correlationId = assignment.correlation_id ?? undefined;
    return {
      key: correlationId ? `correlation:${correlationId}` : `assignment:${assignment.id}`,
      correlationId,
      assignment,
      relatedMessages: correlationId
        ? sortMessages(messages.filter((message) => message.correlation_id === correlationId))
        : [assignment],
    };
  });
}

/** Locate a message's assignment anchor without ever treating a naked correlation as ownership. */
export function selectMessageAssignmentLineage(
  messages: TeamMessage[],
  message: TeamMessage,
): MessageAssignmentLineage {
  if (message.kind === "assignment") {
    return { message, assignment: message, anchored: true };
  }
  const assignment = message.correlation_id
    ? sortMessages(
        messages.filter(
          (candidate) =>
            candidate.kind === "assignment" && candidate.correlation_id === message.correlation_id,
        ),
      )[0]
    : undefined;
  return { message, assignment, anchored: Boolean(assignment) };
}

/** First-class operator signals for a single attempt. */
export function selectTeamRunNeedsYou(
  members: MemberRun[],
  messages: TeamMessage[],
): TeamRunNeedsYou {
  const approvals = sortMessages(
    messages.filter((message) => ["blocker", "review_request"].includes(message.kind ?? "")),
  );
  const waitingMembers = members.filter((member) => member.status === "waiting");
  const blockedMembers = members.filter(
    (member) => member.status === "blocked" || member.status === "failed",
  );
  const unacknowledgedDeliveries = messages.flatMap((message) =>
    (message.deliveries ?? [])
      .filter((delivery) => isUnacknowledgedDelivery(delivery.status))
      .map((delivery) => ({ message, delivery })),
  );
  return {
    approvals,
    waitingMembers,
    blockedMembers,
    unacknowledgedDeliveries,
    total: approvals.length + waitingMembers.length + blockedMembers.length + unacknowledgedDeliveries.length,
  };
}

/**
 * Resolve the durable message that explains one pressured member. A member's
 * own blocker is stronger evidence than a later request merely addressed to
 * that member; this prevents an unrelated review request from rewriting why
 * the operator is being interrupted.
 */
export function selectMemberPressureMessage(
  messages: TeamMessage[],
  member: MemberRun | undefined,
): TeamMessage | undefined {
  if (!member) return undefined;
  const ownBlockers = sortMessages(
    messages.filter(
      (message) => message.kind === "blocker" && message.from_member_id === member.id,
    ),
  );
  if (ownBlockers.length) return ownBlockers[ownBlockers.length - 1];

  const incomingReviewRequests = sortMessages(
    messages.filter(
      (message) =>
        message.kind === "review_request" && (message.to_member_ids ?? []).includes(member.id),
    ),
  );
  return incomingReviewRequests[incomingReviewRequests.length - 1];
}

/**
 * Durable, replayable team activity only. Live provider thinking is deliberately
 * excluded: it is display-only transient state, never an event or evidence.
 */
export function selectStableTeamActivity({
  messages = [],
  actions = [],
  events = [],
  order = "asc",
}: {
  messages?: TeamMessage[];
  actions?: MemberAction[];
  events?: TeamRunEvent[];
  order?: ActivityOrder;
}): StableTeamActivity[] {
  const items: StableTeamActivity[] = [
    ...messages.map((message) => ({
      id: `message:${message.id}`,
      kind: "message" as const,
      at: message.created_at,
      atMs: parseTeamTimestamp(message.created_at),
      sourceMemberRunId: message.from_member_id === "host" ? undefined : message.from_member_id,
      message,
    })),
    ...actions.map((action) => ({
      id: `action:${action.id}`,
      kind: "action" as const,
      at: action.started_at ?? action.completed_at ?? undefined,
      atMs: parseTeamTimestamp(action.started_at ?? action.completed_at),
      seq: action.seq,
      sourceMemberRunId: action.member_run_id ?? undefined,
      action,
    })),
    ...events.map((event) => ({
      id: `event:${event.id}`,
      kind: "event" as const,
      at: event.occurred_at,
      atMs: parseTeamTimestamp(event.occurred_at),
      seq: event.seq,
      sourceMemberRunId: event.member_run_id ?? undefined,
      event,
    })),
  ];
  const factor = order === "desc" ? -1 : 1;
  return items.sort((left, right) => factor * compareActivity(left, right));
}

function isUnacknowledgedDelivery(status?: string | null): boolean {
  const normalized = (status ?? "").toLowerCase();
  return normalized === "queued" || normalized === "delivered";
}

function compareActivity(left: StableTeamActivity, right: StableTeamActivity): number {
  if (left.atMs !== right.atMs) return left.atMs - right.atMs;
  const leftSeq = left.seq ?? Number.MAX_SAFE_INTEGER;
  const rightSeq = right.seq ?? Number.MAX_SAFE_INTEGER;
  if (leftSeq !== rightSeq) return leftSeq - rightSeq;
  const kindRank: Record<StableTeamActivity["kind"], number> = {
    message: 0,
    event: 1,
    action: 2,
  };
  if (kindRank[left.kind] !== kindRank[right.kind]) return kindRank[left.kind] - kindRank[right.kind];
  return stableIdCompare(left.id, right.id);
}

function sortMessages(messages: TeamMessage[]): TeamMessage[] {
  return [...messages].sort(
    (left, right) =>
      parseTeamTimestamp(left.created_at) - parseTeamTimestamp(right.created_at) ||
      stableIdCompare(left.id, right.id),
  );
}

function sortActions(actions: MemberAction[]): MemberAction[] {
  return [...actions].sort(
    (left, right) =>
      parseTeamTimestamp(left.started_at ?? left.completed_at) -
        parseTeamTimestamp(right.started_at ?? right.completed_at) ||
      (left.seq ?? Number.MAX_SAFE_INTEGER) - (right.seq ?? Number.MAX_SAFE_INTEGER) ||
      stableIdCompare(left.id, right.id),
  );
}

function sortDelegations(delegations: DelegationRun[]): DelegationRun[] {
  return [...delegations].sort(
    (left, right) =>
      parseTeamTimestamp(left.created_at) - parseTeamTimestamp(right.created_at) ||
      stableIdCompare(left.id, right.id),
  );
}

function sortEvents(events: TeamRunEvent[]): TeamRunEvent[] {
  return [...events].sort(
    (left, right) =>
      parseTeamTimestamp(left.occurred_at) - parseTeamTimestamp(right.occurred_at) ||
      (left.seq ?? Number.MAX_SAFE_INTEGER) - (right.seq ?? Number.MAX_SAFE_INTEGER) ||
      stableIdCompare(left.id, right.id),
  );
}

function stableIdCompare(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}
