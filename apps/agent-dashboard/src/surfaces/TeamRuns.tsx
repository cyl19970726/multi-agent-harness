import { useEffect, useRef, useState, type ComponentProps, type ReactNode } from "react";
import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  MessageSquare,
  Play,
  Plus,
  Send,
  ShieldAlert,
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
import {
  Dialog,
  DialogFooter,
  Field,
  parseList,
  Select,
  TextArea,
  TextInput,
} from "@/components/workbench/OperatorForms";

import { parseTs, type WorkbenchModel } from "../model/readModel";
import {
  createTeamRun,
  sendTeamMessage,
  startTeamRun,
  transitionTeamRun,
  type ActionDescriptor,
} from "../api/actions";
import type {
  DelegationRun,
  MemberAction,
  MemberRun,
  TeamMessage,
  TeamRun,
  TeamRunEvent,
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

/**
 * Build the wave lineage chain containing `runId`: walk BACK via
 * `previous_run_id` to the root, then FORWARD through children (a run's child
 * is the run whose `previous_run_id` points at it; first match wins per wave).
 * Cycle-safe via a seen-set. The chain is ordered wave 1 → N.
 */
function waveLineage(runs: TeamRun[], runId: string): TeamRun[] {
  const byId = new Map(runs.map((run) => [run.id, run]));
  const seen = new Set<string>();
  const back: TeamRun[] = [];
  let cursor = byId.get(runId);
  while (cursor && !seen.has(cursor.id)) {
    seen.add(cursor.id);
    back.push(cursor);
    cursor = cursor.previous_run_id ? byId.get(cursor.previous_run_id) : undefined;
  }
  back.reverse();
  cursor = byId.get(runId);
  while (cursor) {
    const child = runs.find(
      (run) => run.previous_run_id === cursor?.id && !seen.has(run.id),
    );
    if (!child) break;
    seen.add(child.id);
    back.push(child);
    cursor = child;
  }
  return back;
}

/**
 * Order a list of runs so a child wave renders right under its parent (a
 * simple DFS over the previous_run_id links, preserving the input order
 * within each sibling group). Returns items annotated with indent depth.
 */
function stitchLineage<T extends { run: TeamRun }>(items: T[]): (T & { depth: number })[] {
  const ids = new Set(items.map((item) => item.run.id));
  const childrenByParent = new Map<string, T[]>();
  const roots: T[] = [];
  for (const item of items) {
    const parent = item.run.previous_run_id;
    if (parent && ids.has(parent)) {
      childrenByParent.set(parent, [...(childrenByParent.get(parent) ?? []), item]);
    } else {
      roots.push(item);
    }
  }
  const out: (T & { depth: number })[] = [];
  const walk = (item: T, depth: number) => {
    out.push({ ...item, depth });
    for (const child of childrenByParent.get(item.run.id) ?? []) walk(child, depth + 1);
  };
  for (const root of roots) walk(root, 0);
  return out;
}

/**
 * After dispatching a team-run create, adopt the run id that appears in the
 * refreshed snapshot (the id is server-generated, so we diff against the set
 * of ids known at submit time) and navigate to its detail page.
 */
function useAdoptNewTeamRun(
  runs: TeamRun[],
  onSelectionChange: (selection: Partial<SelectionState>) => void,
): () => void {
  const knownRunIds = useRef<Set<string> | null>(null);
  useEffect(() => {
    const known = knownRunIds.current;
    if (!known) return;
    const created = runs.find((run) => !known.has(run.id));
    if (created) {
      knownRunIds.current = null;
      onSelectionChange({ surface: "team", teamId: created.id });
    }
  }, [runs, onSelectionChange]);
  return () => {
    knownRunIds.current = new Set(runs.map((run) => run.id));
  };
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
 * The Team surface list: every team run, needs-you first. Each row answers two
 * questions — what is this team doing (objective, status, wave, members) and
 * does it need ME (approvals / unacked deliveries / blocked members). Rows with
 * a non-zero needs-you signal sort to the top.
 */
export function TeamRunsList({ model, onSelectionChange, actionsEnabled, onAction }: TeamSurfaceProps) {
  const [newRunOpen, setNewRunOpen] = useState(false);
  const snapshot = model.snapshot;
  const runs = snapshot.team_runs ?? [];
  const allMembers = snapshot.member_runs ?? [];
  const allMessages = snapshot.team_messages ?? [];
  const allEvents = snapshot.team_run_events ?? [];
  const live = Boolean(actionsEnabled);

  const membersByRun = groupBy(allMembers, (m) => m.team_run_id);
  const messagesByRun = groupBy(allMessages, (m) => m.team_run_id);
  const eventsByRun = groupBy(allEvents, (e) => e.team_run_id);

  const decorated = stitchLineage(
    runs
      .map((run) => ({
        run,
        members: membersByRun.get(run.id) ?? [],
        signals: runSignals(membersByRun.get(run.id) ?? [], messagesByRun.get(run.id) ?? []),
        lastMs: lastActivityMs(run, eventsByRun.get(run.id) ?? []),
      }))
      .sort((a, b) => b.signals.total - a.signals.total || b.lastMs - a.lastMs),
  );

  // After a successful create the snapshot refreshes with a run id we could not
  // know at submit time; remember the ids we knew, then adopt the new one.
  const markPendingCreate = useAdoptNewTeamRun(runs, onSelectionChange);

  const cols =
    "grid-cols-[minmax(0,2.2fr)_minmax(0,1fr)_minmax(0,0.9fr)_minmax(0,1.7fr)_minmax(0,0.8fr)]";

  return (
    <DocumentSurface className="max-w-[1180px]">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Users className="size-3.5" /> Agent Teams
          </div>
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">Agent Teams</h1>
          <p className="text-sm text-muted-foreground">
            Watch agent teams work, inspect their members and messages, and step in when a run
            needs you.
          </p>
        </div>
        <ActionButton enabled={live} size="sm" onClick={() => setNewRunOpen(true)}>
          <Plus className="size-3.5" />
          New Team Run
        </ActionButton>
      </header>

      <DocSection label={`${runs.length} ${runs.length === 1 ? "team run" : "team runs"}`}>
        {runs.length === 0 ? (
          <EmptyState
            icon={Users}
            title="No team runs yet"
            description={
              live
                ? "Create a team run with New Team Run to watch an agent team work on one objective."
                : "Connect to a running harness, then create your first team run with New Team Run."
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
              {decorated.map(({ run, members, signals, lastMs, depth }) => {
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
                      style={depth > 0 ? { paddingLeft: depth * 18 } : undefined}
                    >
                      {depth > 0 && (
                        <span className="shrink-0 text-[11px] text-muted-foreground">↳</span>
                      )}
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
                      <Badge tone="muted">wave {run.wave_index ?? 1}</Badge>
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

      <NewTeamRunDialog
        open={newRunOpen}
        actionsEnabled={live}
        onAction={onAction}
        onSubmitting={markPendingCreate}
        onClose={() => setNewRunOpen(false)}
      />
    </DocumentSurface>
  );
}

/* ================================================================== */
/* DETAIL — watch one run, act on what needs you                       */
/* ================================================================== */

type DetailTab = "overview" | "activity" | "messages" | "wave";

/**
 * One team run: header (objective + status + actions) with the wave-lineage
 * stepper, the NEEDS-YOU banner (pending decisions, always on top), the member
 * strip (who is doing what, opening the member drawer), and the
 * Overview/Activity/Messages/Wave tabs. Overview — the cockpit — is the default
 * landing: one glance at every member, then drill into feeds when needed.
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
  const [nextWaveOpen, setNextWaveOpen] = useState(false);

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
  const markPendingCreate = useAdoptNewTeamRun(allRuns, onSelectionChange);

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
  const signals = runSignals(members, messages);
  const live = Boolean(actionsEnabled);
  const lineage = waveLineage(allRuns, run.id);
  // Re-plan hints for the next-wave dialog: this wave's deviations, in one glance.
  const blockers = messages.filter((m) => m.kind === "blocker");
  const rePlanHints: string[] = [
    ...signals.blockedMembers.map(
      (member) => `${member.name ?? member.id} ended ${member.status}`,
    ),
    ...blockers.map((message) => message.body ?? ""),
  ].filter(Boolean);

  return (
    <DocumentSurface className="max-w-[1180px]">
      <div className="space-y-4">
        <button
          type="button"
          onClick={() => onSelectionChange({ surface: "team", teamId: undefined })}
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
              <Badge tone="muted">wave {run.wave_index ?? 1}</Badge>
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
                    onClick={() => dispatch(onAction, startTeamRun(run.id))}
                  >
                    <Play className="size-3.5" />
                    Start orchestration
                  </ActionButton>
                </span>
              </TooltipTrigger>
              <TooltipContent side="bottom">
                Starts the wave loop. The backend answers 501 in v0 and points at the CLI
                (harness team-run start --id …).
              </TooltipContent>
            </Tooltip>
          </div>
        </header>

        <WaveStepper
          lineage={lineage}
          currentId={run.id}
          canStartNext={status === "completed"}
          onSelect={(id) => onSelectionChange({ surface: "team", teamId: id })}
          onStartNext={() => setNextWaveOpen(true)}
        />

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
              members={members}
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
              members={members}
              messages={messages}
              memberById={memberById}
              actionsEnabled={live}
              onAction={onAction}
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

      <NewTeamRunDialog
        open={nextWaveOpen}
        actionsEnabled={live}
        onAction={onAction}
        onSubmitting={markPendingCreate}
        onClose={() => setNextWaveOpen(false)}
        prefill={{
          objective: run.objective ?? "",
          waveIndex: (run.wave_index ?? 1) + 1,
          previousRunId: run.id,
          members: members.map((member) => ({
            name: member.name ?? "",
            role: member.role ?? "",
            provider: member.provider ?? "kimi",
            model: member.model ?? "",
            ownedPaths: (member.owned_paths ?? []).join(", "),
          })),
          hints: rePlanHints,
        }}
      />
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
/* Wave lineage stepper                                                */
/* ------------------------------------------------------------------ */

/**
 * The wave chain for this run's lineage (previous_run_id links): one small
 * node per wave (wave N + status pill), the current wave highlighted, each
 * node clickable. The trailing "+ Start next wave" button only unlocks once
 * the current wave's gate has passed (status completed) — you cannot re-plan
 * a wave that has not landed.
 */
function WaveStepper({
  lineage,
  currentId,
  canStartNext,
  onSelect,
  onStartNext,
}: {
  lineage: TeamRun[];
  currentId: string;
  canStartNext: boolean;
  onSelect: (runId: string) => void;
  onStartNext: () => void;
}) {
  return (
    <nav aria-label="Wave lineage" className="flex flex-wrap items-center gap-1.5">
      {lineage.map((node, index) => {
        const current = node.id === currentId;
        return (
          <span key={node.id} className="flex items-center gap-1.5">
            {index > 0 && <ChevronRight className="size-3.5 text-muted-foreground/60" />}
            <button
              type="button"
              onClick={() => onSelect(node.id)}
              aria-current={current ? "page" : undefined}
              className={cn(
                "flex items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] transition-colors",
                current
                  ? "border-primary/40 bg-primary/12 text-primary"
                  : "border-border bg-card text-muted-foreground hover:border-input hover:text-foreground",
              )}
            >
              <span className="font-semibold">wave {node.wave_index ?? 1}</span>
              <Badge tone={teamRunTone(node.status)}>{node.status ?? "unknown"}</Badge>
            </button>
          </span>
        );
      })}
      {canStartNext ? (
        <Button size="sm" variant="secondary" onClick={onStartNext}>
          <Plus className="size-3.5" />
          Start next wave
        </Button>
      ) : (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="inline-flex">
              <Button size="sm" variant="secondary" disabled title="Complete this wave's gate first">
                <Plus className="size-3.5" />
                Start next wave
              </Button>
            </span>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            Complete this wave&apos;s gate first — the next wave re-plans from a completed one.
          </TooltipContent>
        </Tooltip>
      )}
    </nav>
  );
}

/* ------------------------------------------------------------------ */
/* Overview tab (default landing — Goal summary + the cockpit table)   */
/* ------------------------------------------------------------------ */

/**
 * The cockpit: the Goal summary card (what this run is, what it needs) over
 * the member × work table — every member's current task, current action,
 * runtime heartbeat and execution status in one glance (the design report's
 * 驾驶舱 row, re-lit with the light theme). A row opens the member drawer.
 */
function OverviewTab({
  run,
  members,
  actions,
  signals,
  onOpenMember,
}: {
  run: TeamRun;
  members: MemberRun[];
  actions: MemberAction[];
  signals: RunSignals;
  onOpenMember: (memberRunId: string) => void;
}) {
  const blockerCount = signals.approvals.length + signals.waitingMembers.length;
  const cols =
    "grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.4fr)_minmax(0,0.9fr)_minmax(0,0.7fr)_minmax(0,0.7fr)]";
  return (
    <div className="space-y-3">
      {/* Goal summary card */}
      <section className="rounded-lg border border-border bg-card p-3.5">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Goal
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
            <div className="mt-0.5 text-lg font-semibold tabular-nums">{run.wave_index ?? 1}</div>
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
              <span>Current task</span>
              <span>Current action</span>
              <span>Runtime</span>
              <span>Status</span>
              <span>Last event</span>
            </div>
            {members.map((member) => {
              const latest = latestMemberAction(actions, member.id);
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
                    {member.current_task_id ? (
                      <MonoId>{member.current_task_id}</MonoId>
                    ) : (
                      <span className="text-[12px] text-muted-foreground">—</span>
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
  onClick,
}: {
  member: MemberRun;
  latestAction?: MemberAction;
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
      <span className="block truncate text-[11px] text-foreground/80">
        {latestAction?.title ?? "No actions yet"}
      </span>
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
        memberById={memberById}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
      />
    </div>
  );
}

/** The composer: from / kind / recipients / body → POST /v1/team-runs/{id}/messages. */
function MessageComposer({
  run,
  members,
  memberById,
  actionsEnabled,
  onAction,
}: {
  run: TeamRun;
  members: MemberRun[];
  memberById: Map<string, MemberRun>;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  const [from, setFrom] = useState("host");
  const [kind, setKind] = useState("broadcast");
  const [to, setTo] = useState<string[]>([]);
  const [body, setBody] = useState("");

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
        fromMemberId: from,
        toMemberIds: to,
        kind,
        body: body.trim(),
      }),
    );
    setBody("");
  }

  const recipientOptions = ["host", ...members.map((m) => m.id)];
  return (
    <section className="space-y-2.5 rounded-lg border border-border bg-card p-3">
      <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        New message
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          From
          <Select
            aria-label="From"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
            className="h-8 w-36"
          >
            {recipientOptions.map((id) => (
              <option key={id} value={id}>
                {endpointLabel(memberById, id)}
              </option>
            ))}
          </Select>
        </label>
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
/* Wave tab (wave plan & gate)                                         */
/* ------------------------------------------------------------------ */

/**
 * Wave plan & gate: the current wave's contract (each member's assignment and
 * owned paths), the INTEGRATION GATE (what must be true for this wave to land:
 * no unacked handoffs, no blocked/failed members — and the Complete-wave-gate
 * action once the run is reviewing), and the deviation summary that feeds the
 * next wave's re-plan.
 */
function WaveTab({
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
  const assignments = messages.filter((m) => m.kind === "assignment");
  const assignmentFor = (memberId: string) =>
    assignments.find((m) => (m.to_member_ids ?? []).includes(memberId));
  // Deviations blocking the gate: handoffs not yet acked + members that ended badly.
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
            Wave {run.wave_index ?? 1} contract — {members.length}{" "}
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

      {/* Integration gate: can this wave land? */}
      <section className="rounded-lg border border-border bg-card p-3.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Integration gate
          </p>
          <Badge tone={teamRunTone(run.status)}>{run.status ?? "unknown"}</Badge>
        </div>

        {run.status === "completed" ? (
          <p className="mt-2 flex flex-wrap items-center gap-2 text-[12px] text-muted-foreground">
            <Badge tone="good">gate passed</Badge>
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
                    Complete wave gate
                  </ActionButton>
                ) : (
                  <p className="text-[12px] text-muted-foreground">
                    Reviewing — waiting for every member to reach a terminal state before the
                    gate can pass.
                  </p>
                )
              ) : (
                <p className="text-[12px] text-muted-foreground">
                  The gate opens once the run reaches reviewing (drive the wave with{" "}
                  <MonoId>harness team-run start --id {run.id}</MonoId>).
                </p>
              )}
            </div>
          </div>
        )}
      </section>

      {/* Deviation summary: re-plan input for the next wave. */}
      <section className="rounded-lg border border-border bg-card p-3.5">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Deviations — input to the next wave&apos;s re-plan
        </p>
        {blockers.length === 0 && blockedMembers.length === 0 ? (
          <p className="mt-2 text-[12px] text-muted-foreground">
            No blockers recorded this wave — the next wave can re-plan from a clean base.
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

/* ================================================================== */
/* NEW TEAM RUN DIALOG (member configuration)                          */
/* ================================================================== */

interface MemberDraft {
  name: string;
  role: string;
  provider: string;
  model: string;
  ownedPaths: string;
}

function emptyMember(): MemberDraft {
  return { name: "", role: "", provider: "kimi", model: "", ownedPaths: "" };
}

/** "Start next wave" prefill: same objective + roster, wave+1, lineage link,
 * plus the previous wave's deviations as re-plan hints. */
interface NextWavePrefill {
  objective: string;
  waveIndex: number;
  previousRunId: string;
  members: MemberDraft[];
  hints: string[];
}

/**
 * NEW TEAM RUN (POST /v1/team-runs): objective + wave/budget + the member
 * roster editor. Every field carries a one-line hint so the operator can feel
 * what each setting does. With `prefill` set it becomes the START NEXT WAVE
 * dialog: fields seeded from the completed wave, a re-plan hint box on top,
 * and the create chained onto the previous run. On submit the caller watches
 * the refreshed snapshot for the new run id and navigates to its detail page.
 */
function NewTeamRunDialog({
  open,
  actionsEnabled,
  onAction,
  onSubmitting,
  onClose,
  prefill,
}: {
  open: boolean;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
  /** Called just before dispatch so the list can adopt the new run id. */
  onSubmitting: () => void;
  onClose: () => void;
  prefill?: NextWavePrefill;
}) {
  const [objective, setObjective] = useState("");
  const [waveIndex, setWaveIndex] = useState("1");
  const [budget, setBudget] = useState("");
  const [members, setMembers] = useState<MemberDraft[]>([emptyMember()]);

  useEffect(() => {
    if (open) {
      setObjective(prefill?.objective ?? "");
      setWaveIndex(String(prefill?.waveIndex ?? 1));
      setBudget("");
      setMembers(
        prefill?.members.length
          ? prefill.members.map((member) => ({ ...member }))
          : [emptyMember()],
      );
    }
  }, [open, prefill]);

  const membersValid =
    members.length > 0 && members.every((m) => m.name.trim() && m.role.trim());
  const canSubmit = Boolean(objective.trim()) && membersValid;

  function updateMember(index: number, patch: Partial<MemberDraft>) {
    setMembers((current) =>
      current.map((member, i) => (i === index ? { ...member, ...patch } : member)),
    );
  }

  function submit() {
    if (!canSubmit || !actionsEnabled) return;
    onSubmitting();
    dispatch(
      onAction,
      createTeamRun({
        objective: objective.trim(),
        waveIndex: Number(waveIndex) > 0 ? Number(waveIndex) : undefined,
        budgetLimitUsd: budget.trim() ? Number(budget.trim()) : undefined,
        previousRunId: prefill?.previousRunId,
        members: members.map((member) => ({
          name: member.name.trim(),
          role: member.role.trim(),
          provider: member.provider,
          model: member.model.trim() || undefined,
          ownedPaths: parseList(member.ownedPaths),
        })),
      }),
    );
    onClose();
  }

  return (
    <Dialog
      open={open}
      title={prefill ? `Start wave ${prefill.waveIndex}` : "New Team Run"}
      description={
        prefill
          ? `Re-plan from the completed wave and chain onto it. POST /v1/team-runs.`
          : "Create an agent team run. POST /v1/team-runs."
      }
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        {prefill && prefill.hints.length > 0 && (
          <div className="rounded-lg border border-status-warn/30 bg-status-warn/8 px-3 py-2.5">
            <p className="text-[10px] font-semibold uppercase tracking-wider text-status-warn">
              Re-plan hint — deviations from wave {prefill.waveIndex - 1}
            </p>
            <ul className="mt-1 space-y-0.5">
              {prefill.hints.map((hint, index) => (
                <li key={index} className="truncate text-[12px] text-foreground">
                  {hint}
                </li>
              ))}
            </ul>
            <p className="mt-1 text-[11px] text-muted-foreground">
              Adjust the objective or the roster below to answer them.
            </p>
          </div>
        )}
        <Field
          label="Objective"
          required
          hint="What the team should achieve. The host plans waves and assignments from this."
        >
          {(id) => (
            <TextArea
              id={id}
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              placeholder="e.g. Ship the onboarding revamp"
            />
          )}
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Wave" hint="Wave to start from. 1 = plan from the beginning.">
            {(id) => (
              <TextInput
                id={id}
                type="number"
                min={1}
                value={waveIndex}
                onChange={(event) => setWaveIndex(event.target.value)}
              />
            )}
          </Field>
          <Field
            label="Budget (USD)"
            hint="Optional USD cap for the whole run. Empty = no budget limit."
          >
            {(id) => (
              <TextInput
                id={id}
                type="number"
                min={0}
                step="0.01"
                value={budget}
                onChange={(event) => setBudget(event.target.value)}
                placeholder="no limit"
              />
            )}
          </Field>
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
              Members
            </p>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => setMembers((current) => [...current, emptyMember()])}
            >
              <Plus className="size-3.5" />
              Add member
            </Button>
          </div>
          {members.map((member, index) => (
            <div
              key={index}
              className="space-y-2.5 rounded-lg border border-border bg-background/40 p-2.5"
            >
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  Member {index + 1}
                </span>
                {members.length > 1 && (
                  <button
                    type="button"
                    aria-label={`Remove member ${index + 1}`}
                    onClick={() =>
                      setMembers((current) => current.filter((_, i) => i !== index))
                    }
                    className="grid size-6 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                  >
                    <X className="size-3.5" />
                  </button>
                )}
              </div>
              <div className="grid grid-cols-2 gap-2.5">
                <Field label="Name" required hint="Shown in the member strip and messages.">
                  {(id) => (
                    <TextInput
                      id={id}
                      value={member.name}
                      onChange={(event) => updateMember(index, { name: event.target.value })}
                      placeholder="e.g. lead"
                    />
                  )}
                </Field>
                <Field
                  label="Role"
                  required
                  hint="Responsibility (e.g. lead, engineer, reviewer)."
                >
                  {(id) => (
                    <TextInput
                      id={id}
                      value={member.role}
                      onChange={(event) => updateMember(index, { role: event.target.value })}
                      placeholder="e.g. engineer"
                    />
                  )}
                </Field>
                <Field
                  label="Provider"
                  hint="Runtime adapter. kimi is wired in v0; codex/claude are reserved."
                >
                  {(id) => (
                    <Select
                      id={id}
                      value={member.provider}
                      onChange={(event) => updateMember(index, { provider: event.target.value })}
                    >
                      <option value="kimi">kimi</option>
                      <option value="codex">codex — adapter not wired in v0</option>
                      <option value="claude">claude — adapter not wired in v0</option>
                    </Select>
                  )}
                </Field>
                <Field
                  label="Model"
                  hint="Optional model id (e.g. kimi-code/k3). Empty = provider default."
                >
                  {(id) => (
                    <TextInput
                      id={id}
                      value={member.model}
                      onChange={(event) => updateMember(index, { model: event.target.value })}
                      placeholder="provider default"
                    />
                  )}
                </Field>
              </div>
              <Field
                label="Owned paths"
                hint="Files the member may modify, comma separated. Empty = read-only."
              >
                {(id) => (
                  <TextInput
                    id={id}
                    value={member.ownedPaths}
                    onChange={(event) => updateMember(index, { ownedPaths: event.target.value })}
                    placeholder="e.g. src/, docs/"
                  />
                )}
              </Field>
            </div>
          ))}
        </div>

        <DialogFooter
          submitLabel={prefill ? `Create wave ${prefill.waveIndex}` : "Create team run"}
          actionsEnabled={actionsEnabled}
          canSubmit={canSubmit}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}
