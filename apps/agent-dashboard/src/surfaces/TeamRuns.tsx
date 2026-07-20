import { useEffect, useState, type ComponentProps, type ReactNode } from "react";
import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  MessageSquare,
  Play,
  Send,
  ShieldAlert,
  ShieldCheck,
  Users,
  X,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  DocProperties,
  DocSection,
  DocumentSurface,
  EmptyState,
  MonoId,
  StatusDot,
  type StatusTone,
} from "@/components/workbench/atoms";
import { Select, TextArea } from "@/components/workbench/OperatorForms";

import { parseTs, type WorkbenchModel } from "../model/readModel";
import {
  sendTeamMessage,
  startTeamRun,
  transitionTeamRun,
  type ActionDescriptor,
} from "../api/actions";
import type {
  DelegationRun,
  LiveMemberActivity,
  MemberAction,
  MemberRun,
  TeamMessage,
  TeamRun,
  TeamRunEvent,
  Wave,
} from "../types";
import type { SelectionState } from "../app/selection";

interface TeamSurfaceProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  /** True only when the snapshot is the live source; gates write actions. */
  actionsEnabled?: boolean;
  /** POST a harness action then refresh the snapshot. */
  onAction?: (path: string, body?: unknown) => void;
  apiUrl?: string;
}

const ACTIONS_DISABLED_HINT = "Connect a live source to enable actions";

/** Dispatch an action descriptor through the snapshot-refreshing onAction prop. */
function dispatch(
  onAction: ((path: string, body?: unknown) => void) | undefined,
  descriptor: ActionDescriptor,
): void {
  onAction?.(descriptor.path, descriptor.body);
}

/* ------------------------------------------------------------------ */
/* Tones (all reused from the existing pill palette — no new colors)   */
/* ------------------------------------------------------------------ */

/** Team run status → pill tone. */
function teamRunTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "completed":
      return "good";
    case "failed":
      return "bad";
    case "waiting":
    case "reviewing":
      return "warn";
    case "planning":
      return "info";
    default:
      return "idle";
  }
}

/** Resolve Wave display truth exclusively through the run's native Wave join. */
function resolveRunWave(waves: Wave[], run: TeamRun): Wave | undefined {
  if (!run.wave_id) return undefined;
  return waves.find(
    (wave) => wave.id === run.wave_id && (!run.mission_id || wave.mission_id === run.mission_id),
  );
}

function runWaveLabel(run: TeamRun, wave?: Wave): string {
  if (!run.wave_id) return "unlinked";
  if (!wave) return "unknown Wave";
  return `Wave ${wave.index} · ${wave.title}`;
}

/** Member run status → pill tone. */
function memberRunTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "completed":
      return "good";
    case "failed":
    case "blocked":
      return "bad";
    case "waiting":
    case "reviewing":
      return "warn";
    case "queued":
    case "starting":
      return "info";
    default:
      return "idle";
  }
}

/** Team message kind → pill tone (blockers red, reviews amber, handoffs purple). */
function messageKindTone(kind?: string | null): StatusTone {
  switch ((kind ?? "").toLowerCase()) {
    case "blocker":
      return "bad";
    case "review_request":
      return "warn";
    case "review_result":
    case "answer":
      return "good";
    case "handoff":
    case "question":
      return "decision";
    case "progress":
      return "running";
    case "assignment":
    case "broadcast":
      return "info";
    default:
      return "idle";
  }
}

/**
 * Delivery status → pill tone. Anything not yet ACKNOWLEDGED (queued, or
 * delivered-but-unacked) reads amber — those are the operator's needs-you
 * signal; failed reads red; acknowledged green.
 */
function deliveryTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "acknowledged":
      return "good";
    case "failed":
      return "bad";
    case "queued":
    case "delivered":
      return "warn";
    default:
      return "idle";
  }
}

/** Member action status → pill tone. */
function memberActionTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "succeeded":
      return "good";
    case "failed":
    case "cancelled":
      return "bad";
    case "started":
      return "running";
    case "progress":
      return "info";
    default:
      return "idle";
  }
}

/** Team-run event operation → pill tone. */
function operationTone(operation?: string | null): StatusTone {
  switch ((operation ?? "").toLowerCase()) {
    case "completed":
      return "good";
    case "created":
      return "info";
    default:
      return "idle";
  }
}

/** Provider → a stable color dot (kimi blue, codex green, claude purple). */
function providerTone(provider?: string | null): StatusTone {
  switch ((provider ?? "").toLowerCase()) {
    case "kimi":
      return "info";
    case "codex":
      return "good";
    case "claude":
      return "decision";
    default:
      return "idle";
  }
}

/* ------------------------------------------------------------------ */
/* Small shared helpers                                                */
/* ------------------------------------------------------------------ */

function fmtTime(value?: string | null): string {
  if (!value) return "—";
  const parsed = parseTs(value);
  if (Number.isNaN(parsed)) return value;
  return new Date(parsed).toLocaleString(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Relative "just now / 3m ago / 2h ago / 4d ago" from epoch ms (NaN → "—"). */
function relativeFromMs(ms: number | null): string {
  if (ms == null || Number.isNaN(ms)) return "—";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 45) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

/** Relative age of a harness timestamp string. */
function relativeFromTs(value?: string | null): string {
  if (!value) return "—";
  const ms = parseTs(value);
  return relativeFromMs(Number.isNaN(ms) ? null : ms);
}

function groupBy<T>(items: T[], key: (item: T) => string | undefined | null): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const item of items) {
    const k = key(item) ?? "";
    const list = map.get(k);
    if (list) list.push(item);
    else map.set(k, [item]);
  }
  return map;
}

/** True when a delivery is still waiting on an ACK (the needs-you signal). */
function isUnacked(status?: string | null): boolean {
  const s = (status ?? "").toLowerCase();
  return s === "queued" || s === "delivered";
}

/** Kinds that require the OPERATOR to decide something (blocker / review ask). */
function isApprovalKind(kind?: string | null): boolean {
  const k = (kind ?? "").toLowerCase();
  return k === "blocker" || k === "review_request";
}

/**
 * The needs-you rollup for one run — the first-class operator signal, never
 * buried in the timeline:
 * - `approvals`: blocker / review_request messages + members parked in
 *   "waiting" (someone must decide before work can move).
 * - `unacked`: deliveries still queued / delivered-but-unacknowledged.
 * - `blockedMembers`: members in blocked/failed state.
 */
interface RunSignals {
  approvals: TeamMessage[];
  waitingMembers: MemberRun[];
  unacked: number;
  blockedMembers: MemberRun[];
  total: number;
}

function runSignals(members: MemberRun[], messages: TeamMessage[]): RunSignals {
  const approvals = messages.filter((m) => isApprovalKind(m.kind));
  const waitingMembers = members.filter((m) => m.status === "waiting");
  const unacked = messages.reduce(
    (count, m) => count + (m.deliveries ?? []).filter((d) => isUnacked(d.status)).length,
    0,
  );
  const blockedMembers = members.filter(
    (m) => m.status === "blocked" || m.status === "failed",
  );
  return {
    approvals,
    waitingMembers,
    unacked,
    blockedMembers,
    total: approvals.length + waitingMembers.length + unacked + blockedMembers.length,
  };
}

/** Most recent activity timestamp for a run (run update or any of its events). */
function lastActivityMs(run: TeamRun, events: TeamRunEvent[]): number {
  let ms = parseTs(run.updated_at) || parseTs(run.created_at) || 0;
  for (const event of events) {
    const t = parseTs(event.occurred_at);
    if (!Number.isNaN(t) && t > ms) ms = t;
  }
  return ms;
}

/** The latest recorded action of one member (by seq). */
function latestMemberAction(actions: MemberAction[], memberId: string): MemberAction | undefined {
  let latest: MemberAction | undefined;
  for (const action of actions) {
    if (action.member_run_id !== memberId) continue;
    if (!latest || (action.seq ?? 0) >= (latest.seq ?? 0)) latest = action;
  }
  return latest;
}

/** Resolve a message endpoint id ("host" / member run id / raw name) to a label. */
function endpointLabel(memberById: Map<string, MemberRun>, id?: string | null): string {
  if (!id) return "—";
  if (id === "host") return "host";
  const member = memberById.get(id);
  return member?.name ?? id;
}

/** Assignment messages addressed to one member, oldest first. */
function assignmentsForMember(messages: TeamMessage[], memberRunId: string): TeamMessage[] {
  return messages
    .filter(
      (message) =>
        message.kind === "assignment" && (message.to_member_ids ?? []).includes(memberRunId),
    )
    .sort((a, b) => {
      const ta = parseTs(a.created_at);
      const tb = parseTs(b.created_at);
      return (Number.isNaN(ta) ? 0 : ta) - (Number.isNaN(tb) ? 0 : tb);
    });
}

/** True only when both recipient sets describe the same ownership lane. */
function sameRecipients(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const expected = new Set(left);
  return expected.size === right.length && right.every((id) => expected.has(id));
}

/**
 * Show assignment lineage without treating a generated opaque correlation as
 * proof of ownership. A message is anchored only when an Assignment in this
 * attempt carries the same correlation.
 */
function MessageLineage({ message, messages }: { message: TeamMessage; messages: TeamMessage[] }) {
  const assignment =
    message.kind === "assignment"
      ? message
      : messages.find(
          (candidate) =>
            candidate.kind === "assignment" &&
            Boolean(message.correlation_id) &&
            candidate.correlation_id === message.correlation_id,
        );

  if (assignment) {
    return (
      <div className="flex flex-wrap items-center gap-1 text-[10px] text-muted-foreground">
        <Badge tone="info">{message.kind === "assignment" ? "assignment" : "assignment anchor"}</Badge>
        <MonoId>{assignment.id}</MonoId>
        {assignment.correlation_id && <MonoId>corr {assignment.correlation_id}</MonoId>}
        {message.causation_id && <MonoId>cause {message.causation_id}</MonoId>}
      </div>
    );
  }

  return (
    <div className="flex flex-wrap items-center gap-1 text-[10px] text-muted-foreground">
      <Badge tone="muted">unanchored</Badge>
      {message.correlation_id && <MonoId>corr {message.correlation_id}</MonoId>}
      {message.causation_id && <MonoId>cause {message.causation_id}</MonoId>}
    </div>
  );
}

/** Member-run statuses that mean the member is done working (terminal-ish). */
const MEMBER_TERMINAL_STATUSES = new Set(["completed", "failed", "stopped"]);

/** Primary action button that is honest about read-only mode. */
function ActionButton({
  enabled,
  children,
  ...props
}: ComponentProps<typeof Button> & { enabled?: boolean; children: ReactNode }) {
  if (enabled) {
    return <Button {...props}>{children}</Button>;
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/* span wrapper keeps the tooltip reachable while the button is disabled */}
        <span className="inline-flex">
          <Button {...props} disabled title={ACTIONS_DISABLED_HINT}>
            {children}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom">{ACTIONS_DISABLED_HINT}</TooltipContent>
    </Tooltip>
  );
}

/* ================================================================== */
/* LIST — the operator's team inbox                                    */
/* ================================================================== */

/**
 * Read-only index of unlinked compatibility runs, needs-you first. Native
 * AgentTeamRun attempts are entered from their Mission/Wave cards.
 */
export function TeamRunsList({ model, onSelectionChange, actionsEnabled }: TeamSurfaceProps) {
  const snapshot = model.snapshot;
  // Mission → Wave is the native entry point. This list intentionally retains
  // only unlinked historical/manual runs as a compatibility reader.
  const runs = (snapshot.team_runs ?? []).filter((run) => !run.mission_id && !run.wave_id);
  const allMembers = snapshot.member_runs ?? [];
  const allMessages = snapshot.team_messages ?? [];
  const allEvents = snapshot.team_run_events ?? [];
  const live = Boolean(actionsEnabled);

  const membersByRun = groupBy(allMembers, (m) => m.team_run_id);
  const messagesByRun = groupBy(allMessages, (m) => m.team_run_id);
  const eventsByRun = groupBy(allEvents, (e) => e.team_run_id);

  const decorated = runs
      .map((run) => ({
        run,
        members: membersByRun.get(run.id) ?? [],
        signals: runSignals(membersByRun.get(run.id) ?? [], messagesByRun.get(run.id) ?? []),
        lastMs: lastActivityMs(run, eventsByRun.get(run.id) ?? []),
      }))
      .sort((a, b) => b.signals.total - a.signals.total || b.lastMs - a.lastMs);

  const cols =
    "grid-cols-[minmax(0,2.2fr)_minmax(0,1fr)_minmax(0,0.9fr)_minmax(0,1.7fr)_minmax(0,0.8fr)]";

  return (
    <DocumentSurface className="max-w-[1180px]">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Users className="size-3.5" /> Compatibility reader
          </div>
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">Unlinked Team Runs</h1>
          <p className="text-sm text-muted-foreground">
            Historical or manual runs without a Mission/Wave link. Start new Agent Team work from Missions.
          </p>
        </div>
      </header>

      <DocSection label={`${runs.length} ${runs.length === 1 ? "team run" : "team runs"}`}>
        {runs.length === 0 ? (
          <EmptyState
            icon={Users}
            title="No team runs yet"
            description={
              live
                ? "Start native Agent Team work from a Mission Wave. This page remains only for deliberate compatibility runs."
                : "Connect to a running harness, then open Missions to create a native Agent Team Wave."
            }
          />
        ) : (
          <div className="overflow-hidden">
            <div
              className={cn(
                "grid gap-3 border-b border-border px-2 pb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
                cols,
              )}
            >
              <span>Objective</span>
              <span>Status</span>
              <span>Members</span>
              <span>Needs you</span>
              <span>Last activity</span>
            </div>
            <div>
          {decorated.map(({ run, members, signals, lastMs }) => {
                const status = run.status ?? "unknown";
                const providers = Array.from(
                  new Set(members.map((m) => m.provider).filter(Boolean)),
                ) as string[];
                return (
                  <button
                    key={run.id}
                    type="button"
                    onClick={() => onSelectionChange({ surface: "team", teamId: run.id })}
                    className={cn(
                      "grid w-full items-center gap-3 border-b border-border/60 px-2 py-2.5 text-left transition-colors last:border-b-0 hover:bg-accent/40",
                      cols,
                    )}
                  >
                    <span
                      className="flex min-w-0 items-center gap-2.5"
                    >
                      <StatusDot tone={teamRunTone(status)} pulse={status === "running"} />
                      <span className="min-w-0">
                        <span className="block truncate text-[13px] font-medium text-foreground">
                          {run.objective ?? run.id}
                        </span>
                        <span className="block truncate">
                          <MonoId>{run.id}</MonoId>
                        </span>
                      </span>
                    </span>
                    <span className="flex min-w-0 flex-wrap items-center gap-1">
                      <Badge tone={teamRunTone(status)}>{status}</Badge>
                      <Badge tone="muted">unlinked</Badge>
                    </span>
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span className="text-[12px] tabular-nums text-foreground">
                        {members.length}
                      </span>
                      <span className="flex items-center gap-1" title={providers.join(", ")}>
                        {providers.map((provider) => (
                          <StatusDot key={provider} tone={providerTone(provider)} />
                        ))}
                      </span>
                    </span>
                    <span className="flex min-w-0 flex-wrap items-center gap-1">
                      {signals.total === 0 ? (
                        <span className="text-[12px] text-muted-foreground">—</span>
                      ) : (
                        <>
                          {signals.approvals.length + signals.waitingMembers.length > 0 && (
                            <Badge tone="bad">
                              {signals.approvals.length + signals.waitingMembers.length}{" "}
                              {signals.approvals.length + signals.waitingMembers.length === 1
                                ? "approval"
                                : "approvals"}
                            </Badge>
                          )}
                          {signals.unacked > 0 && (
                            <Badge tone="warn">{signals.unacked} unacked</Badge>
                          )}
                          {signals.blockedMembers.length > 0 && (
                            <Badge tone="bad">{signals.blockedMembers.length} blocked</Badge>
                          )}
                        </>
                      )}
                    </span>
                    <span className="min-w-0 truncate text-[12px] text-muted-foreground">
                      {relativeFromMs(lastMs || null)}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </DocSection>
    </DocumentSurface>
  );
}

/* ================================================================== */
/* DETAIL — watch one run, act on what needs you                       */
/* ================================================================== */

type DetailTab = "overview" | "activity" | "messages" | "wave";

/**
 * One AgentTeamRun attempt: Mission/Wave context, attempt actions, the
 * NEEDS-YOU banner (pending decisions, always on top), the member strip, and
 * Overview/Activity/Messages/Wave-context tabs. Overview is the default
 * cockpit; Wave planning and gating remain on the Mission surface.
 */
export function TeamRunDetail({
  model,
  teamRunId,
  onSelectionChange,
  actionsEnabled,
  onAction,
}: TeamSurfaceProps & { teamRunId?: string }) {
  const [tab, setTab] = useState<DetailTab>("overview");
  const [drawerMemberId, setDrawerMemberId] = useState<string | null>(null);
  const [activityClock, setActivityClock] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setActivityClock(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const snapshot = model.snapshot;
  const allRuns = snapshot.team_runs ?? [];
  const run = allRuns.find((r) => r.id === teamRunId);
  const members = (snapshot.member_runs ?? []).filter((m) => m.team_run_id === teamRunId);
  const messages = (snapshot.team_messages ?? []).filter((m) => m.team_run_id === teamRunId);
  const actions = (snapshot.member_actions ?? []).filter((a) => a.team_run_id === teamRunId);
  const events = (snapshot.team_run_events ?? []).filter((e) => e.team_run_id === teamRunId);
  const delegations = (snapshot.delegation_runs ?? []).filter(
    (d) => d.team_run_id === teamRunId,
  );
  const memberById = new Map(members.map((m) => [m.id, m]));
  const drawerMember = drawerMemberId ? memberById.get(drawerMemberId) : undefined;
  const liveActivity = snapshot.live_member_activity ?? {};
  const activePreview = (memberId: string): LiveMemberActivity | undefined => {
    const activity = liveActivity[memberId];
    if (!activity || activity.team_run_id !== teamRunId) return undefined;
    const expires = parseTs(activity.expires_at);
    return !Number.isNaN(expires) && expires > activityClock ? activity : undefined;
  };

  if (!run) {
    return (
      <DocumentSurface className="max-w-[1180px]">
        <button
          type="button"
          onClick={() => onSelectionChange({ surface: "team", teamId: undefined })}
          className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground transition-colors hover:text-foreground"
        >
          <ChevronLeft className="size-3.5" /> Agent Teams
        </button>
        <EmptyState
          icon={Users}
          title="Team run not found"
          description="This run is not in the current snapshot — it may belong to another project."
        />
      </DocumentSurface>
    );
  }

  const status = run.status ?? "unknown";
  const wave = resolveRunWave(snapshot.waves ?? [], run);
  const signals = runSignals(members, messages);
  const live = Boolean(actionsEnabled);

  return (
    <DocumentSurface className="max-w-[1180px]">
      <div className="space-y-4">
        <button
          type="button"
          onClick={() => onSelectionChange(
            run.mission_id && run.wave_id
              ? { surface: "missions", missionId: run.mission_id, waveId: run.wave_id, teamId: undefined }
              : { surface: "team", teamId: undefined },
          )}
          className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground transition-colors hover:text-foreground"
        >
          <ChevronLeft className="size-3.5" /> Agent Teams
        </button>

        <header className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0 flex-1 space-y-1.5">
            <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
              <Users className="size-3.5" /> Team Run
            </div>
            <h1 className="text-xl font-semibold tracking-tight text-foreground">
              {run.objective ?? run.id}
            </h1>
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge tone={teamRunTone(status)}>{status}</Badge>
              <Badge tone="muted">{runWaveLabel(run, wave)}</Badge>
              {run.host_surface && <Badge tone="muted">{run.host_surface}</Badge>}
              <MonoId>{run.id}</MonoId>
              <span className="text-[11px] text-muted-foreground">
                created {fmtTime(run.created_at)}
              </span>
            </div>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button size="sm" variant="secondary" onClick={() => setTab("messages")}>
              <MessageSquare className="size-3.5" />
              New message
            </Button>
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="inline-flex">
                  <ActionButton
                    enabled={live}
                    size="sm"
                    disabled={status !== "planning"}
                    onClick={() => dispatch(onAction, startTeamRun(run.id))}
                  >
                    <Play className="size-3.5" />
                    Start orchestration
                  </ActionButton>
                </span>
              </TooltipTrigger>
              <TooltipContent side="bottom">
                Starts this attempt asynchronously. Member and Wave state stream back over SSE.
              </TooltipContent>
            </Tooltip>
            {(["planning", "waiting", "reviewing"] as string[]).includes(status) && (
              <ActionButton
                enabled={live}
                size="sm"
                variant="secondary"
                onClick={() => dispatch(onAction, transitionTeamRun(run.id, "cancelled"))}
              >
                <X className="size-3.5" /> Cancel attempt
              </ActionButton>
            )}
            {status === "reviewing" && (
              <ActionButton
                enabled={live}
                size="sm"
                variant="secondary"
                onClick={() => dispatch(onAction, transitionTeamRun(run.id, "completed"))}
              >
                <CheckCircle2 className="size-3.5" /> Mark completed
              </ActionButton>
            )}
          </div>
        </header>

        <AttemptContext run={run} onSelectionChange={onSelectionChange} />

        <NeedsYouBanner
          signals={signals}
          memberById={memberById}
          onViewMessages={() => setTab("messages")}
        />

        {/* Member strip: who is on this team and what each one is doing. A card
            opens the member drawer (actions / delegations / messages / raw). */}
        <div className="flex gap-2.5 overflow-x-auto pb-1">
          {members.map((member) => (
            <MemberStripCard
              key={member.id}
              member={member}
              latestAction={latestMemberAction(actions, member.id)}
              liveActivity={activePreview(member.id)}
              onClick={() => setDrawerMemberId(member.id)}
            />
          ))}
          {members.length === 0 && (
            <p className="text-[12px] text-muted-foreground">No members recorded for this run.</p>
          )}
        </div>

        <Tabs value={tab} onValueChange={(value) => setTab(value as DetailTab)}>
          <TabsList>
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="activity">Activity</TabsTrigger>
            <TabsTrigger value="messages">
              Messages
              {signals.unacked > 0 && <Badge tone="warn">{signals.unacked}</Badge>}
            </TabsTrigger>
            <TabsTrigger value="wave">Wave</TabsTrigger>
          </TabsList>
          <TabsContent value="overview">
            <OverviewTab
              run={run}
              wave={wave}
              members={members}
              messages={messages}
              actions={actions}
              signals={signals}
              onOpenMember={(id) => setDrawerMemberId(id)}
            />
          </TabsContent>
          <TabsContent value="activity">
            <ActivityTab events={events} actions={actions} memberById={memberById} />
          </TabsContent>
          <TabsContent value="messages">
            <MessagesTab
              run={run}
              members={members}
              messages={messages}
              memberById={memberById}
              actionsEnabled={live}
              onAction={onAction}
            />
          </TabsContent>
          <TabsContent value="wave">
            <WaveTab
              run={run}
              wave={wave}
              members={members}
              messages={messages}
              memberById={memberById}
              actionsEnabled={live}
              onAction={onAction}
              onSelectionChange={onSelectionChange}
            />
          </TabsContent>
        </Tabs>
      </div>

      {drawerMember && (
        <MemberDrawer
          member={drawerMember}
          actions={actions}
          delegations={delegations}
          messages={messages}
          memberById={memberById}
          onClose={() => setDrawerMemberId(null)}
        />
      )}
    </DocumentSurface>
  );
}

/* ------------------------------------------------------------------ */
/* Needs-you banner                                                    */
/* ------------------------------------------------------------------ */

/**
 * The soul of the detail page: everything awaiting an OPERATOR decision, pinned
 * above the fold so it can never be buried in the timeline — blocker /
 * review-request messages, members parked in waiting, members blocked/failed,
 * and unacknowledged deliveries. Red when something is truly stuck, amber when
 * only acks are outstanding.
 */
function NeedsYouBanner({
  signals,
  memberById,
  onViewMessages,
}: {
  signals: RunSignals;
  memberById: Map<string, MemberRun>;
  onViewMessages: () => void;
}) {
  if (signals.total === 0) return null;
  const urgent = signals.approvals.length + signals.waitingMembers.length + signals.blockedMembers.length > 0;
  return (
    <section
      className={cn(
        "rounded-lg border px-3.5 py-2.5 text-[12px]",
        urgent
          ? "border-status-bad/30 bg-status-bad/8"
          : "border-status-warn/30 bg-status-warn/8",
      )}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span
          className={cn(
            "inline-flex items-center gap-1.5 font-semibold",
            urgent ? "text-status-bad" : "text-status-warn",
          )}
        >
          <ShieldAlert className="size-3.5" />
          Needs you
        </span>
        <Button size="sm" variant="secondary" onClick={onViewMessages}>
          <MessageSquare className="size-3.5" />
          View in Messages
        </Button>
      </div>
      <ul className="mt-2 space-y-1">
        {signals.approvals.map((message) => (
          <li key={message.id} className="flex min-w-0 items-center gap-2">
            <Badge tone={messageKindTone(message.kind)}>{message.kind ?? "message"}</Badge>
            <span className="shrink-0 text-muted-foreground">
              {endpointLabel(memberById, message.from_member_id)}
            </span>
            <span className="min-w-0 truncate text-foreground">{message.body}</span>
          </li>
        ))}
        {signals.waitingMembers.map((member) => (
          <li key={member.id} className="flex min-w-0 items-center gap-2">
            <Badge tone="warn">waiting</Badge>
            <span className="min-w-0 truncate text-foreground">
              {member.name ?? member.id} is waiting for input
            </span>
          </li>
        ))}
        {signals.blockedMembers.map((member) => (
          <li key={member.id} className="flex min-w-0 items-center gap-2">
            <Badge tone="bad">{member.status}</Badge>
            <span className="min-w-0 truncate text-foreground">
              {member.name ?? member.id} is {member.status}
            </span>
          </li>
        ))}
        {signals.unacked > 0 && (
          <li className="flex min-w-0 items-center gap-2">
            <Badge tone="warn">unacked</Badge>
            <span className="min-w-0 truncate text-muted-foreground">
              {signals.unacked}{" "}
              {signals.unacked === 1 ? "delivery is" : "deliveries are"} still awaiting
              acknowledgment
            </span>
          </li>
        )}
      </ul>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Native context                                                      */
/* ------------------------------------------------------------------ */

/** A TeamRun is an executor attempt, not a Wave itself. */
function AttemptContext({
  run,
  onSelectionChange,
}: {
  run: TeamRun;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  if (!run.mission_id || !run.wave_id) {
    return (
      <p className="rounded-md border border-status-warn/30 bg-status-warn/8 px-3 py-2 text-[12px] text-muted-foreground">
        Compatibility run — this historical run is not linked to a native Mission/Wave.
      </p>
    );
  }
  const missionId = run.mission_id;
  const waveId = run.wave_id;
  return (
    <button
      type="button"
      onClick={() => onSelectionChange({ surface: "missions", missionId, waveId })}
      className="flex flex-wrap items-center gap-1.5 rounded-md border border-primary/25 bg-primary/8 px-3 py-2 text-left text-[12px] hover:border-primary/50"
    >
      <span className="font-semibold text-primary">Mission</span><MonoId>{run.mission_id}</MonoId>
      <ChevronRight className="size-3.5 text-muted-foreground" />
      <span className="font-semibold text-primary">Wave</span><MonoId>{run.wave_id}</MonoId>
      <ChevronRight className="size-3.5 text-muted-foreground" />
      <span className="font-semibold text-foreground">Attempt</span><MonoId>{run.id}</MonoId>
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Overview tab (default landing — attempt summary + cockpit table)    */
/* ------------------------------------------------------------------ */

/**
 * The cockpit: the attempt summary card (what this run is, what it needs) over
 * the member × work table — every member's assignment ownership, current action,
 * runtime heartbeat and execution status in one glance (the design report's
 * 驾驶舱 row, re-lit with the light theme). A row opens the member drawer.
 */
function OverviewTab({
  run,
  wave,
  members,
  messages,
  actions,
  signals,
  onOpenMember,
}: {
  run: TeamRun;
  wave?: Wave;
  members: MemberRun[];
  messages: TeamMessage[];
  actions: MemberAction[];
  signals: RunSignals;
  onOpenMember: (memberRunId: string) => void;
}) {
  const blockerCount = signals.approvals.length + signals.waitingMembers.length;
  const cols =
    "grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.4fr)_minmax(0,0.9fr)_minmax(0,0.7fr)_minmax(0,0.7fr)]";
  return (
    <div className="space-y-3">
      {/* Attempt summary card */}
      <section className="rounded-lg border border-border bg-card p-3.5">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Attempt objective
        </p>
        <p className="mt-1 text-[13px] leading-relaxed text-foreground">
          {run.objective ?? run.id}
        </p>
        <div className="mt-2.5 grid grid-cols-2 gap-2 sm:grid-cols-5">
          <div className="rounded-md border border-border bg-background/40 px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-muted-foreground">Status</div>
            <div className="mt-1">
              <Badge tone={teamRunTone(run.status)}>{run.status ?? "unknown"}</Badge>
            </div>
          </div>
          <div className="rounded-md border border-border bg-background/40 px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-muted-foreground">Wave</div>
            <div className="mt-0.5 text-sm font-semibold">{runWaveLabel(run, wave)}</div>
          </div>
          <div className="rounded-md border border-border bg-background/40 px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-muted-foreground">Budget</div>
            <div className="mt-0.5 text-lg font-semibold tabular-nums">
              {run.budget_limit_usd != null ? `$${run.budget_limit_usd}` : "—"}
            </div>
          </div>
          <div className="rounded-md border border-border bg-background/40 px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-muted-foreground">Unacked</div>
            <div
              className={cn(
                "mt-0.5 text-lg font-semibold tabular-nums",
                signals.unacked > 0 && "text-status-warn",
              )}
            >
              {signals.unacked}
            </div>
          </div>
          <div className="rounded-md border border-border bg-background/40 px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-muted-foreground">Blockers</div>
            <div
              className={cn(
                "mt-0.5 text-lg font-semibold tabular-nums",
                blockerCount > 0 && "text-status-bad",
              )}
            >
              {blockerCount}
            </div>
          </div>
        </div>
      </section>

      {/* Cockpit table */}
      <section className="overflow-hidden rounded-lg border border-border bg-card">
        <div className="border-b border-border px-3.5 py-2.5">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Cockpit — every member at a glance
          </p>
        </div>
        {members.length === 0 ? (
          <EmptyState icon={Users} title="No members" description="This run has no member slots." />
        ) : (
          <>
            <div
              className={cn(
                "grid gap-3 border-b border-border px-3.5 pb-2 pt-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
                cols,
              )}
            >
              <span>Member</span>
              <span>Assignment ownership</span>
              <span>Current action</span>
              <span>Runtime</span>
              <span>Status</span>
              <span>Last event</span>
            </div>
            {members.map((member) => {
              const latest = latestMemberAction(actions, member.id);
              const assignments = assignmentsForMember(messages, member.id);
              const assignment = assignments[assignments.length - 1];
              const heartbeatMs = member.last_event_at ? parseTs(member.last_event_at) : NaN;
              const alive = !Number.isNaN(heartbeatMs) && Date.now() - heartbeatMs < 120_000;
              return (
                <button
                  key={member.id}
                  type="button"
                  onClick={() => onOpenMember(member.id)}
                  className={cn(
                    "grid w-full items-center gap-3 border-b border-border/60 px-3.5 py-2.5 text-left transition-colors last:border-b-0 hover:bg-accent/40",
                    cols,
                  )}
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <StatusDot tone={providerTone(member.provider)} />
                    <span className="min-w-0">
                      <span className="block truncate text-[13px] font-medium text-foreground">
                        {member.name ?? member.id}
                      </span>
                      <span className="block truncate text-[11px] text-muted-foreground">
                        {member.provider ?? "provider"} · {member.role ?? "member"}
                      </span>
                    </span>
                  </span>
                  <span className="min-w-0 truncate">
                    {assignment ? (
                      <span className="flex min-w-0 flex-wrap items-center gap-1">
                        <Badge tone="info">assigned</Badge>
                        <MonoId>{assignment.correlation_id ?? assignment.id}</MonoId>
                      </span>
                    ) : member.owned_paths?.length ? (
                      <span className="text-[12px] text-muted-foreground">
                        Declared paths (no assignment): {member.owned_paths.join(", ")}
                      </span>
                    ) : (
                      <span className="text-[12px] text-muted-foreground">No assignment recorded</span>
                    )}
                  </span>
                  <span className="flex min-w-0 items-center gap-1.5">
                    {latest ? (
                      <>
                        <Badge tone={memberActionTone(latest.status)}>
                          {latest.action_type ?? "action"}
                        </Badge>
                        <span className="min-w-0 truncate text-[12px] text-foreground">
                          {latest.title ?? "—"}
                        </span>
                      </>
                    ) : (
                      <span className="text-[12px] text-muted-foreground">No actions yet</span>
                    )}
                  </span>
                  <span className="flex min-w-0 items-center gap-1.5 text-[12px]">
                    <StatusDot tone={alive ? "good" : "idle"} />
                    <span className="truncate text-muted-foreground">
                      {alive ? "ready" : "no heartbeat"}
                    </span>
                  </span>
                  <span className="min-w-0">
                    <Badge tone={memberRunTone(member.status)}>{member.status ?? "unknown"}</Badge>
                  </span>
                  <span className="min-w-0 truncate text-[12px] text-muted-foreground">
                    {relativeFromTs(member.last_event_at)}
                  </span>
                </button>
              );
            })}
          </>
        )}
      </section>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Member strip                                                        */
/* ------------------------------------------------------------------ */

/** One member card in the strip: identity, status, current action, heartbeat. */
function MemberStripCard({
  member,
  latestAction,
  liveActivity,
  onClick,
}: {
  member: MemberRun;
  latestAction?: MemberAction;
  liveActivity?: LiveMemberActivity;
  onClick: () => void;
}) {
  const status = member.status ?? "unknown";
  return (
    <button
      type="button"
      onClick={onClick}
      className="w-[220px] shrink-0 space-y-1.5 rounded-lg border border-border bg-card p-3 text-left transition-colors hover:border-input hover:bg-accent/40"
    >
      <span className="flex items-center gap-1.5">
        <StatusDot tone={providerTone(member.provider)} />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-foreground">
          {member.name ?? member.id}
        </span>
        <Badge tone={memberRunTone(status)}>{status}</Badge>
      </span>
      <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <span className="truncate">{member.role ?? "member"}</span>
        {member.provider && (
          <Badge tone="muted" className="shrink-0">
            {member.provider}
          </Badge>
        )}
      </span>
      {liveActivity ? (
        <span className="block rounded border border-status-info/25 bg-status-info/8 px-2 py-1 text-[11px] text-foreground/80">
          <span className="mr-1 font-semibold text-status-info">Thinking live</span>
          <span className="line-clamp-2">{liveActivity.preview}</span>
          <span className="mt-0.5 block text-[9px] uppercase tracking-wide text-muted-foreground">
            not saved
          </span>
        </span>
      ) : (
        <span className="block truncate text-[11px] text-foreground/80">
          {latestAction?.title ?? "No actions yet"}
        </span>
      )}
      <span className="block text-[10px] uppercase tracking-wider text-muted-foreground">
        {member.last_event_at ? `${relativeFromTs(member.last_event_at)}` : "no heartbeat"}
      </span>
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Activity tab (default landing — the merged watch feed)              */
/* ------------------------------------------------------------------ */

type ActivityItem =
  | { kind: "event"; at: number; event: TeamRunEvent }
  | { kind: "action"; at: number; action: MemberAction };

/**
 * The watch-first feed: team_run_events merged with member actions, newest
 * first. Each row shows when, who (host / member + provider dot), what type,
 * and the summary; member actions expand to summary + evidence refs.
 */
function ActivityTab({
  events,
  actions,
  memberById,
}: {
  events: TeamRunEvent[];
  actions: MemberAction[];
  memberById: Map<string, MemberRun>;
}) {
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const items: ActivityItem[] = [
    ...events.map((event): ActivityItem => {
      const at = parseTs(event.occurred_at);
      return { kind: "event", at: Number.isNaN(at) ? 0 : at, event };
    }),
    ...actions.map((action): ActivityItem => {
      const at = parseTs(action.started_at);
      return { kind: "action", at: Number.isNaN(at) ? 0 : at, action };
    }),
  ].sort((a, b) => b.at - a.at);

  if (items.length === 0) {
    return (
      <EmptyState
        icon={Users}
        title="No activity yet"
        description="Run events and member actions will appear here as the team works."
      />
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-border bg-card">
      {items.map((item) => {
        if (item.kind === "event") {
          const { event } = item;
          const source =
            event.source_kind === "member" && event.member_run_id
              ? endpointLabel(memberById, event.member_run_id)
              : (event.source_kind ?? "host");
          const member = event.member_run_id ? memberById.get(event.member_run_id) : undefined;
          return (
            <div
              key={event.id}
              className="flex items-start gap-3 border-b border-border/60 px-3.5 py-2.5 last:border-b-0"
            >
              <span className="mt-0.5 w-16 shrink-0 text-[11px] tabular-nums text-muted-foreground">
                {relativeFromTs(event.occurred_at)}
              </span>
              <span className="mt-1 flex shrink-0 items-center gap-1.5">
                <StatusDot tone={member ? providerTone(member.provider) : "idle"} />
                <span className="w-20 truncate text-[11px] text-muted-foreground">{source}</span>
              </span>
              <span className="shrink-0">
                <Badge tone={operationTone(event.operation)}>
                  {event.entity_type ?? "entity"} {event.operation ?? "updated"}
                </Badge>
              </span>
              <span className="min-w-0 flex-1 truncate pt-0.5 text-[12px] text-foreground">
                {event.summary ?? "—"}
              </span>
              <span className="shrink-0 pt-0.5 text-[10px] tabular-nums text-muted-foreground">
                #{event.seq ?? "—"}
              </span>
            </div>
          );
        }
        const { action } = item;
        const member = action.member_run_id ? memberById.get(action.member_run_id) : undefined;
        const expanded = expandedId === action.id;
        return (
          <div key={action.id} className="border-b border-border/60 last:border-b-0">
            <button
              type="button"
              onClick={() => setExpandedId(expanded ? null : action.id)}
              className="flex w-full items-start gap-3 px-3.5 py-2.5 text-left transition-colors hover:bg-accent/40"
            >
              <span className="mt-0.5 w-16 shrink-0 text-[11px] tabular-nums text-muted-foreground">
                {relativeFromTs(action.started_at)}
              </span>
              <span className="mt-1 flex shrink-0 items-center gap-1.5">
                <StatusDot tone={member ? providerTone(member.provider) : "idle"} />
                <span className="w-20 truncate text-[11px] text-muted-foreground">
                  {member?.name ?? action.member_run_id ?? "—"}
                </span>
              </span>
              <span className="shrink-0">
                <Badge tone={memberActionTone(action.status)}>
                  {action.action_type ?? "action"}
                </Badge>
              </span>
              <span className="min-w-0 flex-1 truncate pt-0.5 text-[12px] font-medium text-foreground">
                {action.title ?? "—"}
              </span>
              <ChevronRight
                className={cn(
                  "mt-1 size-3.5 shrink-0 text-muted-foreground transition-transform",
                  expanded && "rotate-90",
                )}
              />
            </button>
            {expanded && (
              <div className="space-y-1.5 border-t border-border/40 bg-background/40 px-3.5 py-2.5 pl-[9.5rem]">
                <p className="whitespace-pre-wrap text-[12px] text-muted-foreground">
                  {action.summary ?? "No summary."}
                </p>
                {(action.evidence_refs ?? []).length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {(action.evidence_refs ?? []).map((ref) => (
                      <Badge key={ref} tone="muted">
                        {ref}
                      </Badge>
                    ))}
                  </div>
                )}
                <p className="text-[11px] text-muted-foreground">
                  status {action.status ?? "—"} · seq {action.seq ?? "—"} · completed{" "}
                  {fmtTime(action.completed_at)}
                </p>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Messages tab (the handoff chain + composer)                         */
/* ------------------------------------------------------------------ */

const MESSAGE_KINDS = [
  "broadcast",
  "assignment",
  "progress",
  "question",
  "answer",
  "handoff",
  "blocker",
  "review_request",
  "review_result",
  "control",
];

/** The handoff chain, oldest first, with per-recipient delivery pills. */
function MessagesTab({
  run,
  members,
  messages,
  memberById,
  actionsEnabled,
  onAction,
}: {
  run: TeamRun;
  members: MemberRun[];
  messages: TeamMessage[];
  memberById: Map<string, MemberRun>;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  const sorted = messages.slice().sort((a, b) => {
    const ta = parseTs(a.created_at);
    const tb = parseTs(b.created_at);
    return (Number.isNaN(ta) ? 0 : ta) - (Number.isNaN(tb) ? 0 : tb);
  });
  return (
    <div className="space-y-3">
      {sorted.length === 0 ? (
        <EmptyState
          icon={MessageSquare}
          title="No messages yet"
          description="Assignments, questions and handoffs between the host and members appear here."
        />
      ) : (
        <div className="space-y-2">
          {sorted.map((message) => (
            <article
              key={message.id}
              className="space-y-1.5 rounded-lg border border-border bg-card px-3 py-2.5"
            >
              <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
                <Badge tone={messageKindTone(message.kind)}>{message.kind ?? "message"}</Badge>
                <span className="font-medium text-foreground">
                  {endpointLabel(memberById, message.from_member_id)}
                </span>
                <span className="text-muted-foreground">→</span>
                <span className="text-muted-foreground">
                  {(message.to_member_ids ?? [])
                    .map((id) => endpointLabel(memberById, id))
                    .join(", ") || "—"}
                </span>
                <span className="ml-auto text-muted-foreground">
                  {fmtTime(message.created_at)}
                </span>
              </div>
              <p className="whitespace-pre-wrap text-[13px] leading-relaxed text-foreground">
                {message.body}
              </p>
              <MessageLineage message={message} messages={messages} />
              {(message.deliveries ?? []).length > 0 && (
                <div className="flex flex-wrap items-center gap-1">
                  {(message.deliveries ?? []).map((delivery, index) => (
                    <Badge
                      key={`${delivery.member_id ?? "member"}-${index}`}
                      tone={deliveryTone(delivery.status)}
                      title={
                        delivery.updated_at
                          ? `updated ${fmtTime(delivery.updated_at)} · attempt ${delivery.attempt ?? 0}`
                          : undefined
                      }
                    >
                      {endpointLabel(memberById, delivery.member_id)}:{" "}
                      {delivery.status ?? "unknown"}
                    </Badge>
                  ))}
                </div>
              )}
            </article>
          ))}
        </div>
      )}

      <MessageComposer
        run={run}
        members={members}
        messages={messages}
        memberById={memberById}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
      />
    </div>
  );
}

/**
 * Operator-authored messages only. Member-originated rows are written by the
 * runtime/adapter; the console never impersonates a MemberRun. A selected
 * assignment is the sole way to reuse assignment ownership correlation.
 */
function MessageComposer({
  run,
  members,
  messages,
  memberById,
  actionsEnabled,
  onAction,
}: {
  run: TeamRun;
  members: MemberRun[];
  messages: TeamMessage[];
  memberById: Map<string, MemberRun>;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  const [kind, setKind] = useState("broadcast");
  const [to, setTo] = useState<string[]>([]);
  const [body, setBody] = useState("");
  const [assignmentAnchorId, setAssignmentAnchorId] = useState<string | null>(null);

  const assignments = messages.filter((message) => message.kind === "assignment");
  const selectedRecipients = to.slice().sort();
  const exactAssignments = assignments.filter(
    (assignment) =>
      assignment.from_member_id === "host" &&
      Boolean(assignment.correlation_id) &&
      sameRecipients((assignment.to_member_ids ?? []).slice().sort(), selectedRecipients),
  );
  const anchorCandidates = assignments.filter((assignment) =>
    Boolean(assignment.correlation_id) &&
    (assignment.to_member_ids ?? []).some((id) => to.includes(id)),
  );
  const selectedAnchor = assignments.find((assignment) => assignment.id === assignmentAnchorId);

  useEffect(() => {
    // An automatic anchor is safe only for one exact host assignment. Multiple
    // member lanes (including broadcasts) stay deliberately unanchored until
    // the operator chooses an assignment in the explicit control below.
    setAssignmentAnchorId(
      kind === "assignment" || exactAssignments.length !== 1 ? null : exactAssignments[0].id,
    );
  }, [kind, to.join("\u0000"), exactAssignments.map((assignment) => assignment.id).join("\u0000")]);

  const canSubmit = Boolean(actionsEnabled && body.trim() && to.length > 0);

  function toggleRecipient(id: string) {
    setTo((current) =>
      current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    );
  }

  function submit() {
    if (!canSubmit) return;
    dispatch(
      onAction,
      sendTeamMessage(run.id, {
        fromMemberId: "host",
        toMemberIds: to,
        kind,
        body: body.trim(),
        correlationId: kind === "assignment" ? undefined : selectedAnchor?.correlation_id ?? undefined,
        causationId: kind === "assignment" ? undefined : selectedAnchor?.id,
      }),
    );
    setBody("");
  }

  const recipientOptions = members.map((m) => m.id);
  return (
    <section className="space-y-2.5 rounded-lg border border-border bg-card p-3">
      <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        New message
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex h-8 items-center rounded-md border border-border bg-background/50 px-2 text-[11px] text-muted-foreground">
          From <strong className="ml-1 font-medium text-foreground">Host / operator</strong>
        </span>
        <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          Kind
          <Select
            aria-label="Kind"
            value={kind}
            onChange={(event) => setKind(event.target.value)}
            className="h-8 w-36"
          >
            {MESSAGE_KINDS.map((entry) => (
              <option key={entry} value={entry}>
                {entry}
              </option>
            ))}
          </Select>
        </label>
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] text-muted-foreground">To</span>
        {recipientOptions.map((id) => {
          const selected = to.includes(id);
          return (
            <button
              key={id}
              type="button"
              aria-pressed={selected}
              onClick={() => toggleRecipient(id)}
              className={cn(
                "rounded-md border px-2 py-1 text-[11px] transition-colors",
                selected
                  ? "border-primary/40 bg-primary/12 text-primary"
                  : "border-border bg-background/50 text-muted-foreground hover:text-foreground",
              )}
            >
              {endpointLabel(memberById, id)}
            </button>
          );
        })}
      </div>
      {kind !== "assignment" && (
        <label className="flex max-w-full flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
          Assignment anchor
          <Select
            aria-label="Assignment anchor"
            value={assignmentAnchorId ?? ""}
            onChange={(event) => setAssignmentAnchorId(event.target.value || null)}
            className="h-8 min-w-44 max-w-full"
          >
            <option value="">None — unanchored</option>
            {anchorCandidates.map((assignment) => (
              <option key={assignment.id} value={assignment.id}>
                {assignment.to_member_ids
                  ?.map((id) => endpointLabel(memberById, id))
                  .join(", ") ?? "member"}
                {" · "}{assignment.id}
              </option>
            ))}
          </Select>
          {exactAssignments.length === 1 && assignmentAnchorId === exactAssignments[0].id && (
            <span>auto-selected exact recipient match</span>
          )}
          {to.length > 1 && exactAssignments.length !== 1 && (
            <span>broadcasts stay unanchored until you choose an assignment</span>
          )}
        </label>
      )}
      <TextArea
        aria-label="Message body"
        value={body}
        onChange={(event) => setBody(event.target.value)}
        placeholder="Write a message to the team…"
        rows={3}
      />
      <div className="flex items-center justify-end gap-2">
        <ActionButton enabled={actionsEnabled} size="sm" onClick={submit} disabled={!canSubmit}>
          <Send className="size-3.5" />
          Send
        </ActionButton>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Wave tab (attempt contract & parent-gate context)                   */
/* ------------------------------------------------------------------ */

/**
 * Attempt contract and review: each member's assignment/owned paths, whether
 * this attempt can be marked completed, and the deviations the parent Wave
 * gate should consider. Completing an attempt never accepts the Wave.
 */
function WaveTab({
  run,
  wave,
  members,
  messages,
  memberById,
  actionsEnabled,
  onAction,
  onSelectionChange,
}: {
  run: TeamRun;
  wave?: Wave;
  members: MemberRun[];
  messages: TeamMessage[];
  memberById: Map<string, MemberRun>;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onSelectionChange: TeamSurfaceProps["onSelectionChange"];
}) {
  const assignments = messages.filter((m) => m.kind === "assignment");
  const assignmentFor = (memberId: string) =>
    assignments.find((m) => (m.to_member_ids ?? []).includes(memberId));
  // Deviations affecting attempt completion and the later parent Wave gate.
  const unackedHandoffs = messages.filter(
    (m) => m.kind === "handoff" && (m.deliveries ?? []).some((d) => isUnacked(d.status)),
  );
  const blockedMembers = members.filter(
    (m) => m.status === "blocked" || m.status === "failed",
  );
  const blockers = messages.filter((m) => m.kind === "blocker");
  const allMembersTerminal =
    members.length > 0 &&
    members.every((m) => MEMBER_TERMINAL_STATUSES.has((m.status ?? "").toLowerCase()));
  const gateReady = run.status === "reviewing" && allMembersTerminal;

  return (
    <div className="space-y-3">
      {/* Wave contract: what each member signed up for this wave. */}
      <section className="overflow-hidden rounded-lg border border-border bg-card">
        <div className="border-b border-border px-3.5 py-2.5">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {runWaveLabel(run, wave)} contract — {members.length}{" "}
            {members.length === 1 ? "member" : "members"}
          </p>
        </div>
        {members.length === 0 ? (
          <EmptyState icon={Users} title="No members" description="This run has no member slots." />
        ) : (
          members.map((member) => (
            <div
              key={member.id}
              className="flex items-start gap-3 border-b border-border/60 px-3.5 py-2.5 last:border-b-0"
            >
              <StatusDot tone={providerTone(member.provider)} className="mt-1.5" />
              <span className="min-w-0 flex-1">
                <span className="flex flex-wrap items-center gap-1.5">
                  <span className="text-[13px] font-medium text-foreground">
                    {member.name ?? member.id}
                  </span>
                  <Badge tone="muted">{member.role ?? "member"}</Badge>
                  <Badge tone="muted">{member.provider ?? "provider"}</Badge>
                  <Badge tone={memberRunTone(member.status)}>{member.status ?? "unknown"}</Badge>
                </span>
                <span className="mt-0.5 block truncate text-[12px] text-muted-foreground">
                  {assignmentFor(member.id)?.body ?? "No assignment message recorded."}
                </span>
                <span className="mt-1 flex flex-wrap items-center gap-1">
                  {member.owned_paths?.length ? (
                    member.owned_paths.map((path) => (
                      <Badge key={path} tone="muted">
                        {path}
                      </Badge>
                    ))
                  ) : (
                    <span className="text-[11px] text-muted-foreground">read-only</span>
                  )}
                </span>
              </span>
            </div>
          ))
        )}
      </section>

      {/* Attempt completion is only an input to the separate parent Wave gate. */}
      <section className="rounded-lg border border-border bg-card p-3.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Attempt completion
          </p>
          <Badge tone={teamRunTone(run.status)}>{run.status ?? "unknown"}</Badge>
        </div>

        {run.status === "completed" ? (
          <p className="mt-2 flex flex-wrap items-center gap-2 text-[12px] text-muted-foreground">
            <Badge tone="good">attempt completed</Badge>
            <span>completed {fmtTime(run.completed_at)}</span>
          </p>
        ) : (
          <div className="mt-2 space-y-2">
            {unackedHandoffs.length === 0 && blockedMembers.length === 0 ? (
              <p className="text-[12px] text-muted-foreground">
                No open deviations — no unacked handoffs, no blocked or failed members.
              </p>
            ) : (
              <ul className="space-y-1">
                {unackedHandoffs.map((message) => (
                  <li key={message.id} className="flex min-w-0 items-center gap-2 text-[12px]">
                    <Badge tone="warn">unacked handoff</Badge>
                    <span className="shrink-0 text-muted-foreground">
                      {endpointLabel(memberById, message.from_member_id)} →{" "}
                      {(message.to_member_ids ?? [])
                        .map((id) => endpointLabel(memberById, id))
                        .join(", ")}
                    </span>
                    <span className="min-w-0 truncate text-foreground">{message.body}</span>
                  </li>
                ))}
                {blockedMembers.map((member) => (
                  <li key={member.id} className="flex min-w-0 items-center gap-2 text-[12px]">
                    <Badge tone="bad">{member.status}</Badge>
                    <span className="min-w-0 truncate text-foreground">
                      {member.name ?? member.id} is {member.status}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <div className="flex items-center gap-2 pt-1">
              {run.status === "reviewing" ? (
                gateReady ? (
                  <ActionButton
                    enabled={actionsEnabled}
                    size="sm"
                    onClick={() => dispatch(onAction, transitionTeamRun(run.id, "completed"))}
                  >
                    <CheckCircle2 className="size-3.5" />
                    Mark attempt completed
                  </ActionButton>
                ) : (
                  <p className="text-[12px] text-muted-foreground">
                    Reviewing — every member must reach a terminal state before this attempt can
                    be completed.
                  </p>
                )
              ) : (
                <p className="text-[12px] text-muted-foreground">
                  Completion becomes available once the attempt reaches reviewing (start it with{" "}
                  <MonoId>harness team-run start --id {run.id}</MonoId>).
                </p>
              )}
            </div>
          </div>
        )}
        {run.mission_id && run.wave_id && (
          <div className="mt-3 border-t border-border/60 pt-3">
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() =>
                onSelectionChange({
                  surface: "missions",
                  missionId: run.mission_id ?? undefined,
                  waveId: run.wave_id ?? undefined,
                  teamId: undefined,
                })
              }
            >
              <ShieldCheck className="size-3.5" /> Open parent Wave gate
            </Button>
            <p className="mt-2 text-[11px] text-muted-foreground">
              The Host accepts, revises, or blocks the Wave separately after choosing a completed
              attempt.
            </p>
          </div>
        )}
      </section>

      {/* Deviation summary: input to parent Wave gate or a same-Wave retry. */}
      <section className="rounded-lg border border-border bg-card p-3.5">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Deviations — Wave gate / retry input
        </p>
        {blockers.length === 0 && blockedMembers.length === 0 ? (
          <p className="mt-2 text-[12px] text-muted-foreground">
            No blockers recorded for this attempt.
          </p>
        ) : (
          <ul className="mt-2 space-y-1">
            {blockers.map((message) => (
              <li key={message.id} className="flex min-w-0 items-center gap-2 text-[12px]">
                <Badge tone="bad">blocker</Badge>
                <span className="shrink-0 text-muted-foreground">
                  {endpointLabel(memberById, message.from_member_id)}
                </span>
                <span className="min-w-0 truncate text-foreground">{message.body}</span>
              </li>
            ))}
            {blockedMembers.map((member) => (
              <li key={member.id} className="flex min-w-0 items-center gap-2 text-[12px]">
                <Badge tone="bad">{member.status}</Badge>
                <span className="min-w-0 truncate text-foreground">
                  {member.name ?? member.id} ({member.role ?? "member"}) ended {member.status}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/* ================================================================== */
/* MEMBER DRAWER                                                       */
/* ================================================================== */

type DrawerTab = "actions" | "delegations" | "messages" | "raw";

/**
 * Right-hand drawer for one member of the run: identity card plus Actions /
 * Delegations / Messages / Raw tabs. Opened from the member strip; closes via
 * the X, the overlay, or Escape.
 */
function MemberDrawer({
  member,
  actions,
  delegations,
  messages,
  memberById,
  onClose,
}: {
  member: MemberRun;
  actions: MemberAction[];
  delegations: DelegationRun[];
  messages: TeamMessage[];
  memberById: Map<string, MemberRun>;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<DrawerTab>("actions");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const memberActions = actions
    .filter((a) => a.member_run_id === member.id)
    .sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0));
  const memberDelegations = delegations.filter((d) => d.parent_member_run_id === member.id);
  const memberMessages = messages
    .filter(
      (m) => m.from_member_id === member.id || (m.to_member_ids ?? []).includes(member.id),
    )
    .sort((a, b) => {
      const ta = parseTs(a.created_at);
      const tb = parseTs(b.created_at);
      return (Number.isNaN(ta) ? 0 : ta) - (Number.isNaN(tb) ? 0 : tb);
    });

  return (
    <div className="fixed inset-0 z-40" role="presentation">
      <div
        className="absolute inset-0 bg-background/60 backdrop-blur-sm"
        onClick={onClose}
        aria-hidden
      />
      <aside
        role="dialog"
        aria-modal="true"
        aria-label={`Member ${member.name ?? member.id}`}
        className="absolute inset-y-0 right-0 flex w-full max-w-md flex-col border-l border-border bg-card shadow-2xl"
      >
        <div className="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
          <div className="min-w-0 space-y-1">
            <div className="flex items-center gap-2">
              <StatusDot tone={providerTone(member.provider)} />
              <h2 className="truncate text-sm font-semibold tracking-tight text-foreground">
                {member.name ?? member.id}
              </h2>
              <Badge tone={memberRunTone(member.status)}>{member.status ?? "unknown"}</Badge>
            </div>
            <p className="text-[11px] text-muted-foreground">
              {member.role ?? "member"} · {member.provider ?? "provider"}
              {member.model ? ` · ${member.model}` : ""}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="border-b border-border px-4 py-3">
          <DocProperties
            items={[
              {
                label: "ACP session",
                value: member.acp_session_id ? <MonoId>{member.acp_session_id}</MonoId> : "—",
              },
              {
                label: "Worktree",
                value: member.worktree_ref ? <MonoId>{member.worktree_ref}</MonoId> : "—",
              },
              {
                label: "Owned paths",
                value: member.owned_paths?.length ? (
                  <span className="flex flex-wrap gap-1">
                    {member.owned_paths.map((path) => (
                      <Badge key={path} tone="muted">
                        {path}
                      </Badge>
                    ))}
                  </span>
                ) : (
                  "read-only"
                ),
              },
              { label: "Started", value: fmtTime(member.started_at) },
              { label: "Last event", value: relativeFromTs(member.last_event_at) },
              {
                label: "Finished",
                value: member.finished_at ? fmtTime(member.finished_at) : "—",
              },
            ]}
          />
        </div>

        <div className="border-b border-border px-4 py-2">
          <Tabs value={tab} onValueChange={(value) => setTab(value as DrawerTab)}>
            <TabsList>
              <TabsTrigger value="actions">Actions ({memberActions.length})</TabsTrigger>
              <TabsTrigger value="delegations">
                Delegations ({memberDelegations.length})
              </TabsTrigger>
              <TabsTrigger value="messages">Messages ({memberMessages.length})</TabsTrigger>
              <TabsTrigger value="raw">Raw</TabsTrigger>
            </TabsList>
          </Tabs>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {tab === "actions" &&
            (memberActions.length === 0 ? (
              <EmptyState
                icon={Users}
                title="No actions yet"
                description="This member's tool calls and progress notes will appear here."
              />
            ) : (
              <div className="space-y-2">
                {memberActions.map((action) => {
                  const expanded = expandedId === action.id;
                  return (
                    <div
                      key={action.id}
                      className="overflow-hidden rounded-lg border border-border bg-background/40"
                    >
                      <button
                        type="button"
                        onClick={() => setExpandedId(expanded ? null : action.id)}
                        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-accent/40"
                      >
                        <span className="w-8 shrink-0 text-[10px] tabular-nums text-muted-foreground">
                          #{action.seq ?? "—"}
                        </span>
                        <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground">
                          {action.title ?? action.action_type ?? "action"}
                        </span>
                        <Badge tone={memberActionTone(action.status)}>
                          {action.status ?? "unknown"}
                        </Badge>
                        <ChevronRight
                          className={cn(
                            "size-3.5 shrink-0 text-muted-foreground transition-transform",
                            expanded && "rotate-90",
                          )}
                        />
                      </button>
                      {expanded && (
                        <div className="space-y-1.5 border-t border-border/40 px-3 py-2.5">
                          <p className="whitespace-pre-wrap text-[12px] text-muted-foreground">
                            {action.summary ?? "No summary."}
                          </p>
                          {(action.evidence_refs ?? []).length > 0 && (
                            <div className="flex flex-wrap gap-1">
                              {(action.evidence_refs ?? []).map((ref) => (
                                <Badge key={ref} tone="muted">
                                  {ref}
                                </Badge>
                              ))}
                            </div>
                          )}
                          <p className="text-[11px] text-muted-foreground">
                            {fmtTime(action.started_at)} → {fmtTime(action.completed_at)}
                          </p>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            ))}

          {tab === "delegations" &&
            (memberDelegations.length === 0 ? (
              <EmptyState
                icon={Users}
                title="No delegations"
                description="Sub-agents this member spawned (captured or orchestrated) appear here."
              />
            ) : (
              <div className="space-y-2">
                {memberDelegations.map((delegation) => (
                  <div
                    key={delegation.id}
                    className="space-y-1.5 rounded-lg border border-border bg-background/40 px-3 py-2.5"
                  >
                    <div className="flex flex-wrap items-center gap-1.5">
                      {delegation.mode === "provider_native" ? (
                        <Badge tone="info">captured</Badge>
                      ) : (
                        <Badge tone="decision">orchestrated</Badge>
                      )}
                      <Badge tone="muted">{delegation.mode ?? "delegation"}</Badge>
                      {delegation.provider && <Badge tone="muted">{delegation.provider}</Badge>}
                      <Badge tone={teamRunTone(delegation.status)}>
                        {delegation.status ?? "unknown"}
                      </Badge>
                    </div>
                    <p className="text-[12px] text-foreground">{delegation.objective ?? "—"}</p>
                    <div className="space-y-0.5">
                      {delegation.provider_child_thread_id && (
                        <MonoId>thread {delegation.provider_child_thread_id}</MonoId>
                      )}
                      {delegation.workflow_run_id && (
                        <span className="block">
                          <MonoId>workflow {delegation.workflow_run_id}</MonoId>
                        </span>
                      )}
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      {fmtTime(delegation.created_at)}
                    </p>
                  </div>
                ))}
              </div>
            ))}

          {tab === "messages" &&
            (memberMessages.length === 0 ? (
              <EmptyState
                icon={MessageSquare}
                title="No messages"
                description="Messages from or to this member appear here."
              />
            ) : (
              <div className="space-y-2">
                {memberMessages.map((message) => (
                  <div
                    key={message.id}
                    className="space-y-1 rounded-lg border border-border bg-background/40 px-3 py-2"
                  >
                    <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
                      <Badge tone={messageKindTone(message.kind)}>
                        {message.kind ?? "message"}
                      </Badge>
                      <span className="text-muted-foreground">
                        {endpointLabel(memberById, message.from_member_id)} →{" "}
                        {(message.to_member_ids ?? [])
                          .map((id) => endpointLabel(memberById, id))
                          .join(", ")}
                      </span>
                      <span className="ml-auto text-muted-foreground">
                        {fmtTime(message.created_at)}
                      </span>
                    </div>
                    <p className="whitespace-pre-wrap text-[12px] text-foreground">
                      {message.body}
                    </p>
                    <MessageLineage message={message} messages={messages} />
                  </div>
                ))}
              </div>
            ))}

          {tab === "raw" && (
            <pre className="overflow-auto rounded-lg border border-border bg-background/40 p-3 font-mono text-[11px] leading-relaxed text-foreground">
              {JSON.stringify(member, null, 2)}
            </pre>
          )}
        </div>
      </aside>
    </div>
  );
}
