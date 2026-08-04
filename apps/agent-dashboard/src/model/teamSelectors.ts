import type {
  DashboardSnapshot,
  DelegationRun,
  LiveMemberActivity,
  MemberAction,
  MemberRun,
  Mission,
  PendingInteraction,
  TeamMessage,
  TeamMessageDelivery,
  TeamRun,
  TeamRunEvent,
  Wave,
  Work,
  WorkDelivery,
  WorkEvent,
} from "../types";

/**
 * Read-model selectors for Mission-linked independent Agent Teams, TeamRuns,
 * versioned Host-plan Waves, and provider-native MemberRun bindings.
 *
 * These selectors intentionally do not project a MemberRun into a standing
 * AgentMember. A MemberRun is one participation in one TeamRun attempt, and
 * all ownership below is derived from the TeamRun's shared Works board.
 */

export type ActivityOrder = "asc" | "desc";

export interface TeamRunNeedsYou {
  /** @deprecated Conversation messages are not responsibility or review truth. */
  approvals: TeamMessage[];
  waitingMembers: MemberRun[];
  blockedMembers: MemberRun[];
  /**
   * Non-terminal Works found on a terminal TeamRun. New transitions reject
   * this state, but historical/imported data must remain visibly anomalous.
   */
  unfinishedWorks: Work[];
  blockedWorks: Work[];
  reviewWorks: Work[];
  pendingInteractions: PendingInteraction[];
  /** Delivery failures that prevent an assigned Work from reaching its owner. */
  workDeliveryPressure: WorkDelivery[];
  /** @deprecated Ordinary message delivery is conversation state, not operator work. */
  unacknowledgedDeliveries: Array<{
    message: TeamMessage;
    delivery: TeamMessageDelivery;
  }>;
  total: number;
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
    }
  | {
      id: string;
      kind: "work_event";
      at?: string;
      atMs: number;
      seq?: number;
      sourceMemberRunId?: string;
      workEvent: WorkEvent;
    }
  | {
      id: string;
      kind: "work_delivery";
      at?: string;
      atMs: number;
      seq?: number;
      sourceMemberRunId?: string;
      workDelivery: WorkDelivery;
    };

export interface TeamRunContext {
  run: TeamRun;
  mission?: Mission;
  wave?: Wave;
  /** Direct-Wave compatibility attempts, or this Mission-scoped run alone. */
  attempts: TeamRun[];
  members: MemberRun[];
  memberById: Map<string, MemberRun>;
  messages: TeamMessage[];
  actions: MemberAction[];
  interactions: PendingInteraction[];
  delegations: DelegationRun[];
  events: TeamRunEvent[];
  works: Work[];
  workEvents: WorkEvent[];
  workDeliveries: WorkDelivery[];
  liveActivityByMember: Map<string, LiveMemberActivity>;
  needsYou: TeamRunNeedsYou;
  activity: StableTeamActivity[];
}

export interface MemberRunContext extends TeamRunContext {
  member: MemberRun;
  /** Durable ownership by AgentMember/slot identity, across runtime generations. */
  ownedWorks: Work[];
  /** Works currently bound to this concrete MemberRun generation. */
  activeWorks: Work[];
  /** One truthful focus derived from the active binding and Work state. */
  currentWork?: Work;
  /** Other non-terminal Works owned and actively bound to this MemberRun. */
  queuedOwnedWorks: Work[];
  /** Ready, unowned team-claim Works this active member may claim. */
  eligibleReadyWorks: Work[];
  /** Compatibility alias for `eligibleReadyWorks`. */
  eligibleWorks: Work[];
  /** Messages which involve this member, oldest first. */
  messagesForMember: TeamMessage[];
  actionsForMember: MemberAction[];
  delegationsForMember: DelegationRun[];
  eventsForMember: TeamRunEvent[];
  activityForMember: StableTeamActivity[];
  liveActivity?: LiveMemberActivity;
}

export type TeamWorksOwnerFilter = "all" | "unassigned" | `member:${string}`;
export type TeamWorksAttentionFilter = "all" | "review" | "blocked";

/**
 * Resolve one Work owner without treating a runtime generation as the durable
 * identity. The active MemberRun wins for navigation; otherwise the stable
 * AgentMember/slot identity resolves to the best matching generation.
 */
export function selectWorkOwnerMember(work: Work, members: MemberRun[]): MemberRun | undefined {
  const active = members.find((member) => member.id === work.active_member_run_id);
  if (active) return active;
  if (!work.owner_member_id) return undefined;
  return members.find((member) => stableMemberIdentity(member) === work.owner_member_id);
}

/** Shared Works-board filter. Status remains visible as five Kanban lanes. */
export function selectFilteredTeamWorks(
  works: Work[],
  members: MemberRun[],
  ownerFilter: TeamWorksOwnerFilter = "all",
  attentionFilter: TeamWorksAttentionFilter = "all",
): Work[] {
  const memberId = ownerFilter.startsWith("member:")
    ? ownerFilter.slice("member:".length)
    : undefined;
  const selectedMember = memberId ? members.find((member) => member.id === memberId) : undefined;
  return works.filter((work) => {
    const ownerMatches = ownerFilter === "all"
      || (ownerFilter === "unassigned" && !work.owner_member_id && !work.active_member_run_id)
      || Boolean(selectedMember && selectWorkOwnerMember(work, members)?.id === selectedMember.id);
    const attentionMatches = attentionFilter === "all"
      || (attentionFilter === "review" && work.status === "review")
      || (attentionFilter === "blocked" && work.status === "blocked");
    return ownerMatches && attentionMatches;
  });
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

/** Ordered Waves are versioned Host plan and judgment records for a Mission. */
export function selectOrderedWaves(snapshot: DashboardSnapshot, missionId?: string): Wave[] {
  if (!missionId) return [];
  return [...(snapshot.waves ?? [])]
    .filter((wave) => wave.mission_id === missionId)
    .sort((left, right) => left.index - right.index || stableIdCompare(left.id, right.id));
}

/**
 * Compatibility lookup for AgentTeamRun attempts directly owned by one Wave.
 * New Mission-scoped Teams are intentionally absent: their lifecycle can span
 * Waves, while the shared Works board carries durable execution ownership.
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
  const interactions = (snapshot.pending_interactions ?? []).filter((interaction) => interaction.team_run_id === run.id);
  const delegations = (snapshot.delegation_runs ?? []).filter((delegation) => delegation.team_run_id === run.id);
  const events = (snapshot.team_run_events ?? []).filter((event) => event.team_run_id === run.id);
  const works = (snapshot.works ?? []).filter((work) => work.team_run_id === run.id);
  const workEvents = (snapshot.work_events ?? []).filter((event) => event.team_run_id === run.id);
  const workDeliveries = (snapshot.work_deliveries ?? []).filter((delivery) => delivery.team_run_id === run.id);
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
    interactions,
    delegations,
    events,
    works,
    workEvents,
    workDeliveries,
    liveActivityByMember,
    needsYou: selectTeamRunNeedsYou(
      members,
      messages,
      interactions,
      run.status,
      works,
      workDeliveries,
    ),
    activity: selectStableTeamActivity({
      messages,
      actions,
      events,
      workEvents,
      workDeliveries,
    }),
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
  const stableMemberId = stableMemberIdentity(member);
  const ownedWorks = team.works.filter((work) => work.owner_member_id === stableMemberId);
  const activeWorks = ownedWorks.filter((work) => work.active_member_run_id === member.id);
  const orderedActiveWorks = sortMemberWorks(activeWorks);
  const currentWork = orderedActiveWorks[0];
  const eligibleReadyWorks = selectEligibleReadyWorks(team.works, team.members, member);
  const memberWorkIds = new Set(ownedWorks.map((work) => work.id));
  const workEventsForMember = team.workEvents.filter(
    (event) =>
      memberWorkIds.has(event.work_id)
      || (event.performed_by_actor.kind === "member_run" && event.performed_by_actor.id === member.id),
  );
  const workDeliveriesForMember = team.workDeliveries.filter(
    (delivery) => delivery.recipient_member_run_id === member.id,
  );

  return {
    ...team,
    member,
    ownedWorks,
    activeWorks,
    currentWork,
    queuedOwnedWorks: orderedActiveWorks.filter((work) => work.id !== currentWork?.id),
    eligibleReadyWorks,
    eligibleWorks: eligibleReadyWorks,
    messagesForMember: sortMessages(messagesForMember),
    actionsForMember: sortActions(actionsForMember),
    delegationsForMember: sortDelegations(delegationsForMember),
    eventsForMember: sortEvents(eventsForMember),
    activityForMember: selectStableTeamActivity({
      messages: messagesForMember,
      actions: actionsForMember,
      events: eventsForMember,
      workEvents: workEventsForMember,
      workDeliveries: workDeliveriesForMember,
    }),
    liveActivity: team.liveActivityByMember.get(member.id),
  };
}

/** First-class operator signals for a single attempt. */
export function selectTeamRunNeedsYou(
  members: MemberRun[],
  messages: TeamMessage[],
  interactions: PendingInteraction[] = [],
  runStatus?: string | null,
  works: Work[] = [],
  workDeliveries: WorkDelivery[] = [],
): TeamRunNeedsYou {
  // Work, not conversational language, is the durable review/blocker queue.
  // Keep the message argument for source compatibility, but never infer
  // responsibility from `kind=blocker|review_request`.
  void messages;
  const terminalRun = runStatus === "completed" || runStatus === "cancelled";
  const approvals: TeamMessage[] = [];
  const waitingMembers: MemberRun[] = [];
  const unfinishedWorks = terminalRun
    ? sortWorks(works.filter((work) => !["done", "cancelled"].includes(work.status)))
    : [];
  const blockedWorks = sortWorks(works.filter((work) => work.status === "blocked"));
  const reviewWorks = sortWorks(works.filter((work) => work.status === "review"));
  // A member can be blocked without owning any Work: the provider-capacity
  // start gate runs before the adapter claims anything, so such a member never
  // appears in `blockedWorks`. Deriving blocked members from Work alone made
  // that member invisible, so the durable MemberRun status is unioned in.
  const blockedMemberIds = new Set(
    blockedWorks.flatMap((work) => work.active_member_run_id ? [work.active_member_run_id] : []),
  );
  const blockedMembers = members.filter(
    (member) => blockedMemberIds.has(member.id) || member.status === "blocked",
  );
  // Terminal attempts are historical for conversation/interaction pressure,
  // but never erase contradictory Work truth. A failed delivery remains
  // pressure there only while its Work is still unfinished.
  const pendingInteractions = terminalRun
    ? []
    : interactions.filter((interaction) => interaction.status === "pending");
  const unfinishedWorkIds = new Set(unfinishedWorks.map((work) => work.id));
  const workDeliveryPressure = workDeliveries.filter((delivery) =>
    delivery.status === "failed"
    && (!terminalRun || unfinishedWorkIds.has(delivery.work_id)),
  );
  const unacknowledgedDeliveries: TeamRunNeedsYou["unacknowledgedDeliveries"] = [];
  return {
    approvals,
    waitingMembers,
    blockedMembers,
    unfinishedWorks,
    blockedWorks,
    reviewWorks,
    pendingInteractions,
    workDeliveryPressure,
    unacknowledgedDeliveries,
    total: (terminalRun ? unfinishedWorks.length : blockedWorks.length + reviewWorks.length)
      + pendingInteractions.length
      + workDeliveryPressure.length,
  };
}

/**
 * Durable, replayable team activity only. Live provider thinking is deliberately
 * excluded: it is display-only transient state, never an event or evidence.
 */
export function selectStableTeamActivity({
  messages = [],
  actions = [],
  events = [],
  workEvents = [],
  workDeliveries = [],
  order = "asc",
}: {
  messages?: TeamMessage[];
  actions?: MemberAction[];
  events?: TeamRunEvent[];
  workEvents?: WorkEvent[];
  workDeliveries?: WorkDelivery[];
  order?: ActivityOrder;
}): StableTeamActivity[] {
  const visibleWorkEventKinds = new Set([
    "assigned",
    "claimed",
    "started",
    "released",
    "blocked",
    "resumed",
    "submitted",
    "changes_requested",
    "accepted",
    "cancelled",
    "rebound",
  ]);
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
    ...workEvents
      .filter((event) => visibleWorkEventKinds.has(event.kind) || isResponsibilityUpdate(event))
      .map((workEvent) => ({
        id: `work-event:${workEvent.id}`,
        kind: "work_event" as const,
        at: workEvent.created_at,
        atMs: parseTeamTimestamp(workEvent.created_at),
        seq: workEvent.sequence,
        sourceMemberRunId: workEvent.performed_by_actor.kind === "member_run"
          ? workEvent.performed_by_actor.id
          : undefined,
        workEvent,
      })),
    ...workDeliveries
      .filter((delivery) => delivery.status === "failed")
      .map((workDelivery) => ({
        id: `work-delivery:${workDelivery.id}`,
        kind: "work_delivery" as const,
        at: workDelivery.updated_at,
        atMs: parseTeamTimestamp(workDelivery.updated_at),
        sourceMemberRunId: workDelivery.recipient_member_run_id,
        workDelivery,
      })),
  ];
  const factor = order === "desc" ? -1 : 1;
  return items.sort((left, right) => factor * compareActivity(left, right));
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
    work_event: 3,
    work_delivery: 4,
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

function stableMemberIdentity(member: MemberRun): string {
  return member.agent_member_id ?? member.slot_id ?? member.id;
}

/** Whether a concrete MemberRun generation may receive or claim new Work. */
export function canMemberAcceptWork(member: MemberRun): boolean {
  if (member.coordination_status !== "active") return false;
  return !new Set(["stopped", "failed", "closed"]).has((member.status ?? "").toLowerCase());
}

/**
 * One factual capacity tile.
 *
 * `total` is optional on purpose. A ratio is rendered only where a real
 * denominator exists in the store; there is no synthetic utilisation figure and
 * no invented limit. See `selectTeamCapacity`.
 */
export interface TeamCapacityTile {
  id: "active-turns" | "ready-members" | "queued-works" | "needs-review" | "blocked";
  label: string;
  value: number;
  total?: number;
  detail?: string;
  tone: "running" | "info" | "warn" | "bad" | "idle";
}

/**
 * Factual Team capacity derived only from durable MemberRun and Work rows.
 *
 * Deliberately absent: an `Active turns` denominator. The concurrency ceiling
 * is a runtime-only start parameter and is not persisted on AgentTeamRun, so
 * rendering "3 of N" would invent a runtime fact. `Ready members` does carry a
 * denominator because the roster size is durable.
 */
export function selectTeamCapacity(members: MemberRun[], works: Work[]): TeamCapacityTile[] {
  const activeTurns = members.filter((member) => member.status === "running").length;
  const readyMembers = members.filter(canMemberAcceptWork).length;
  const queuedWorks = works.filter((work) => work.status === "open").length;
  const reviewWorks = works.filter((work) => work.status === "review").length;
  const blockedWorks = works.filter((work) => work.status === "blocked");
  const blockedWorkMemberIds = new Set(
    blockedWorks.flatMap((work) => work.active_member_run_id ? [work.active_member_run_id] : []),
  );
  const blockedMembersWithoutWork = members.filter(
    (member) => member.status === "blocked" && !blockedWorkMemberIds.has(member.id),
  ).length;
  return [
    { id: "active-turns", label: "Active turns", value: activeTurns, tone: activeTurns ? "running" : "idle" },
    { id: "ready-members", label: "Ready members", value: readyMembers, total: members.length, tone: readyMembers ? "info" : "warn" },
    { id: "queued-works", label: "Queued Works", value: queuedWorks, tone: queuedWorks ? "info" : "idle" },
    { id: "needs-review", label: "Needs review", value: reviewWorks, tone: reviewWorks ? "warn" : "idle" },
    {
      id: "blocked",
      label: "Blocked",
      value: blockedWorks.length,
      detail: blockedMembersWithoutWork
        ? `${blockedMembersWithoutWork} member${blockedMembersWithoutWork === 1 ? "" : "s"} without Work`
        : undefined,
      tone: blockedWorks.length || blockedMembersWithoutWork ? "bad" : "idle",
    },
  ];
}

function isWorkReady(work: Work, works: Work[]): boolean {
  if (work.status !== "open") return false;
  const statusById = new Map(works.map((candidate) => [candidate.id, candidate.status]));
  return (work.prerequisite_work_ids ?? []).every((id) => statusById.get(id) === "done");
}

function selectEligibleReadyWorks(works: Work[], members: MemberRun[], member: MemberRun): Work[] {
  if (!canMemberAcceptWork(member) || !members.some((candidate) => candidate.id === member.id)) {
    return [];
  }
  const memberIdentity = stableMemberIdentity(member);
  return sortWorks(works.filter((work) =>
    work.claim_mode === "team_claim"
    && !work.owner_member_id
    && !work.active_member_run_id
    && isWorkReady(work, works)
    && (!(work.eligible_member_ids?.length)
      || work.eligible_member_ids.includes(memberIdentity)),
  ));
}

/**
 * `updated` is intentionally not blanket-visible: title/body edits are board
 * detail noise. Keep it in Activity only when its payload changes ownership,
 * runtime binding, claim eligibility, or dependency responsibility. Payloads
 * may nest a patch/diff, so inspect object keys recursively.
 */
function isResponsibilityUpdate(event: WorkEvent): boolean {
  if (event.kind !== "updated") return false;
  const responsibilityKeys = new Set([
    "owner_member_id",
    "active_member_run_id",
    "claim_mode",
    "eligible_member_ids",
    "prerequisite_work_ids",
  ]);
  const visits = new Set<unknown>();
  const containsKey = (value: unknown): boolean => {
    if (!value || typeof value !== "object" || visits.has(value)) return false;
    visits.add(value);
    if (Array.isArray(value)) return value.some(containsKey);
    return Object.entries(value as Record<string, unknown>).some(
      ([key, nested]) => responsibilityKeys.has(key) || containsKey(nested),
    );
  };
  return containsKey(event.payload);
}

function sortMemberWorks(works: Work[]): Work[] {
  const statusRank: Record<string, number> = {
    in_progress: 0,
    blocked: 1,
    review: 2,
    open: 3,
    done: 4,
    cancelled: 5,
  };
  return [...works]
    .filter((work) => !["done", "cancelled"].includes(work.status))
    .sort((left, right) =>
      (statusRank[left.status] ?? Number.MAX_SAFE_INTEGER)
      - (statusRank[right.status] ?? Number.MAX_SAFE_INTEGER)
      || priorityRank(left.priority) - priorityRank(right.priority)
      || parseTeamTimestamp(left.updated_at) - parseTeamTimestamp(right.updated_at)
      || stableIdCompare(left.id, right.id),
    );
}

function sortWorks(works: Work[]): Work[] {
  return [...works].sort((left, right) =>
    priorityRank(left.priority) - priorityRank(right.priority)
    || parseTeamTimestamp(left.updated_at) - parseTeamTimestamp(right.updated_at)
    || stableIdCompare(left.id, right.id),
  );
}

function priorityRank(priority?: string): number {
  const rank: Record<string, number> = { urgent: 0, high: 1, normal: 2, low: 3 };
  return rank[priority ?? "normal"] ?? 2;
}

function stableIdCompare(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}
