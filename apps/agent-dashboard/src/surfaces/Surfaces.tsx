import {
  Activity,
  AlertTriangle,
  Bot,
  Bug,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  ClipboardList,
  Clock,
  Crown,
  ExternalLink,
  FileCheck2,
  FileText,
  Gavel,
  GitBranch,
  Inbox,
  Link2,
  ListChecks,
  MessageSquare,
  RefreshCw,
  Scale,
  Send,
  ShieldAlert,
  ShieldCheck,
  Target,
  Terminal,
  User,
  UserPlus,
  Workflow,
  Wrench,
  X,
  Zap,
} from "lucide-react";

import { useEffect, useRef, useState, type ComponentProps, type ReactNode } from "react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  AgentSparkline,
  CollapsibleBlock,
  DocProperties,
  DocSection,
  DocumentSurface,
  EmptyState,
  MonoId,
  Section,
  StatusDot,
  SurfaceHeader,
  TimelineRow,
  toneText,
  type StatusTone,
} from "@/components/workbench/atoms";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import { fetchDoc, normalizeBaseUrl } from "../api";
import {
  Dialog,
  DialogFooter,
  Field,
  parseList,
  Select,
  TextArea,
  TextInput,
} from "@/components/workbench/OperatorForms";
import {
  gapSeverityTone,
  gapStatusTone,
  goalTone,
  memberTone,
  reviewVerdictTone,
  severityTone,
  taskTone,
  timelineTone,
} from "@/components/workbench/tones";

import {
  displayGoalStatus,
  formatDuration,
  gapIsResolved,
  memberName,
  parseTs,
  taskTitle,
  tasksBlockedBy,
  taskGitMetadata,
  type TimelineItem,
  type WorkbenchModel,
} from "../model/readModel";
import {
  assignTask,
  closeMember,
  createAgent,
  deliverQueued,
  operatorMessage,
  reconcileSession,
  requestReview,
  retryDelivery,
  setReviewer,
  type ActionDescriptor,
} from "../api/actions";
import type {
  AgentMember,
  AgentProviderConfig,
  AgentStats,
  DeliveryStatus,
  Gap,
  Goal,
  GoalDesign,
  GoalEvaluation,
  Message,
  ProviderChildThread,
  ProviderSession,
  Review,
  RuntimeHealth,
  Task,
  Vision,
  WorkflowWarning,
} from "../types";
import type { AgentTab, SelectionState } from "../app/selection";

interface SurfaceProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  /** True only when the snapshot is the live source; gates write actions. */
  actionsEnabled?: boolean;
  /** POST a harness action then refresh the snapshot. */
  onAction?: (path: string, body?: unknown) => void;
  /** Live harness base URL; used to fetch doc bodies (GET /v1/docs). */
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

/**
 * Tone the member by DELIVERY health, not mere presence. A live process whose
 * delivery probe failed/unknown must not read as healthy/green. Falls back to
 * the coarse runtime/status tone when no health object is present.
 */
function deliveryHealthTone(member: AgentMember): StatusTone {
  const health = member.runtime_health;
  if (health) {
    const probe = (health.delivery_probe ?? "").toLowerCase();
    if (probe.startsWith("pass")) return "good";
    if (probe.startsWith("fail")) return "bad";
    // Process alive but delivery not yet (or never) confirmed → amber, not green.
    if (health.process_alive) return "warn";
    return "bad";
  }
  return memberTone(member.runtime_status ?? member.status);
}

/** Tone for a message delivery_status chip. */
function deliveryStatusTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "delivered":
    case "acknowledged":
      return "good";
    case "failed":
      return "bad";
    case "queued":
      return "warn";
    default:
      return "idle";
  }
}

/**
 * Primary action button that is honest about read-only mode: when actions are
 * disabled it renders visibly disabled with an explanatory tooltip instead of
 * silently doing nothing.
 */
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

/* ------------------------------------------------------------------ */
/* Shared building blocks                                              */
/* ------------------------------------------------------------------ */

function fmtTime(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function shortBranch(value: string): string {
  if (value.startsWith("http")) {
    const parts = value.split("/");
    return `#${parts.slice(-1)[0]}`;
  }
  const parts = value.split("/");
  return parts.length > 2 ? `…/${parts.slice(-1)[0]}` : value;
}

function ProofStat({
  label,
  value,
  tone,
  caption,
}: {
  label: string;
  value: number | string;
  tone: StatusTone;
  caption?: string;
}) {
  return (
    <div className="px-3 py-1 text-center">
      <div className={cn("text-lg font-semibold tabular-nums", toneText[tone])}>
        {value}
      </div>
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      {caption && (
        <div className="mt-0.5 text-[10px] text-muted-foreground/70">{caption}</div>
      )}
    </div>
  );
}

/** Verifiable-criteria checklist (used by Task acceptance + Goal success). */
function CriteriaList({
  items,
  empty,
}: {
  items?: string[];
  empty: string;
}) {
  if (!items?.length) {
    return <EmptyState title={empty} />;
  }
  return (
    <ul className="space-y-2 p-4">
      {items.map((item, index) => (
        <li key={index} className="flex items-start gap-2.5 text-[13px]">
          <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-status-good" />
          <span className="text-foreground/90">{item}</span>
        </li>
      ))}
    </ul>
  );
}

function PathList({ paths }: { paths?: string[] }) {
  if (!paths?.length) return <span className="text-muted-foreground">—</span>;
  return (
    <span className="flex flex-col gap-0.5">
      {paths.map((path) => (
        <MonoId key={path}>{path}</MonoId>
      ))}
    </span>
  );
}

/** depends_on / blocks chips that link to the related task. */
function DependencyChips({
  ids,
  tasks,
  empty,
  onSelect,
}: {
  ids: string[];
  tasks: Task[];
  empty: string;
  onSelect: (id: string) => void;
}) {
  if (!ids.length) {
    return <p className="px-1 text-xs text-muted-foreground">{empty}</p>;
  }
  return (
    <div className="flex flex-wrap gap-1.5">
      {ids.map((id) => {
        const t = tasks.find((task) => task.id === id);
        return (
          <button
            key={id}
            type="button"
            onClick={() => onSelect(id)}
            className="inline-flex items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 py-1 text-[11px] transition-colors hover:border-input hover:bg-accent/40"
          >
            <StatusDot tone={taskTone(t?.status)} />
            <span className="max-w-44 truncate">{t?.title ?? id}</span>
          </button>
        );
      })}
    </div>
  );
}

/** Dependency readiness for a TaskCard, derived from the task graph. */
type Readiness = { ready: boolean; waiting: number };

/** A ready 🟢 / waiting ⏳(N) chip — derived, distinct from status=blocked. */
function ReadinessChip({ readiness }: { readiness?: Readiness }) {
  if (!readiness) return null;
  if (readiness.waiting > 0) {
    return (
      <span className="inline-flex items-center gap-1 rounded bg-status-warn/12 px-1.5 py-0.5 text-[10px] font-medium text-status-warn">
        <Clock className="size-2.5" />
        waiting ({readiness.waiting})
      </span>
    );
  }
  if (readiness.ready) {
    return (
      <span className="inline-flex items-center gap-1 rounded bg-status-good/12 px-1.5 py-0.5 text-[10px] font-medium text-status-good">
        <CheckCircle2 className="size-2.5" />
        ready
      </span>
    );
  }
  return null;
}

function TaskCard({
  task,
  onClick,
  readiness,
  goalLabel,
}: {
  task: Task;
  onClick: () => void;
  readiness?: Readiness;
  goalLabel?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group block w-full rounded-md border border-border bg-background/40 p-2.5 text-left transition-colors hover:border-input hover:bg-accent/40"
    >
      <div className="mb-1 flex items-center gap-2">
        <MonoId>{task.id}</MonoId>
        {goalLabel && (
          <span className="inline-flex items-center gap-1 truncate text-[10px] text-muted-foreground">
            <Target className="size-2.5" />
            <span className="max-w-28 truncate">{goalLabel}</span>
          </span>
        )}
        <span className="ml-auto">
          <ReadinessChip readiness={readiness} />
        </span>
      </div>
      <div className="flex items-start justify-between gap-2">
        <span className="line-clamp-2 text-[13px] font-medium leading-snug">
          {task.title ?? task.id}
        </span>
        <Badge tone={taskTone(task.status)}>{task.status}</Badge>
      </div>
      <div className="mt-1.5 flex items-center gap-3 text-[11px] text-muted-foreground">
        {task.assignee_agent_id && (
          <span className="inline-flex items-center gap-1">
            <Bot className="size-3" />
            {task.assignee_agent_id.replace(/^agent-/, "")}
          </span>
        )}
        {task.branch_ref && (
          <span className="inline-flex items-center gap-1">
            <GitBranch className="size-3" />
            <MonoId>{shortBranch(task.branch_ref)}</MonoId>
          </span>
        )}
      </div>
    </button>
  );
}

/** Build a readiness lookup for a task list from the model's task graph. */
function readinessFor(
  task: Task,
  graph: WorkbenchModel["taskGraph"],
): Readiness {
  return {
    ready: graph.ready.has(task.id),
    waiting: graph.waiting.get(task.id)?.length ?? 0,
  };
}

function QueueList({
  items,
  empty,
  onSelect,
}: {
  items: WorkbenchModel["decisionQueue"];
  empty: string;
  onSelect: (objectRef?: string) => void;
}) {
  if (!items.length) {
    return <EmptyState icon={Gavel} title={empty} />;
  }
  return (
    <div className="max-h-[22rem] overflow-y-auto">
      {items.map((item) => (
        <TimelineRow
          key={item.id}
          kind={item.kind}
          title={item.title}
          meta={item.meta}
          body={item.body}
          tone={timelineTone(item.kind, item.severity)}
          onClick={() => onSelect(item.objectRef)}
        />
      ))}
    </div>
  );
}

function WarningList({
  warnings,
  onSelect,
}: {
  warnings: WorkflowWarning[];
  onSelect: (warning: WorkflowWarning) => void;
}) {
  if (!warnings.length) {
    return (
      <EmptyState
        icon={ShieldCheck}
        title="No active warnings"
        description="Every workflow invariant currently holds."
      />
    );
  }
  return (
    <div className="max-h-[22rem] overflow-y-auto">
      {warnings.map((warning) => (
        <button
          key={warning.id}
          type="button"
          onClick={() => onSelect(warning)}
          className="flex w-full items-start gap-3 border-b border-border/60 px-3.5 py-2.5 text-left transition-colors last:border-0 hover:bg-accent/40"
        >
          <StatusDot tone={severityTone(warning.severity)} className="mt-1" />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2">
              <MonoId>{warning.kind}</MonoId>
              <Badge tone={severityTone(warning.severity)}>{warning.severity}</Badge>
            </span>
            <span className="mt-0.5 block line-clamp-2 text-xs text-muted-foreground">
              {warning.summary}
            </span>
          </span>
        </button>
      ))}
    </div>
  );
}

function GoalCard({
  goal,
  model,
  onSelect,
}: {
  goal: Goal;
  model: WorkbenchModel;
  onSelect: () => void;
}) {
  const tasks = model.tasks.filter((task) => task.goal_id === goal.id);
  return (
    <button
      type="button"
      onClick={onSelect}
      className="block w-full rounded-lg border border-border bg-background/40 p-3 text-left transition-colors hover:border-input hover:bg-accent/40"
    >
      <div className="flex items-start justify-between gap-2">
        <span className="line-clamp-2 text-[13px] font-medium leading-snug">
          {goal.title ?? goal.id}
        </span>
        <Badge tone={goalTone(goal.status)}>{goal.status ?? "active"}</Badge>
      </div>
      <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{goal.objective}</p>
      <div className="mt-2 inline-flex items-center gap-1 text-[11px] text-muted-foreground">
        <ClipboardList className="size-3" /> {tasks.length} tasks
      </div>
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Agents area (list)                                                  */
/* ------------------------------------------------------------------ */

/** A small provider badge: codex / claude (or any provider) as a muted chip. */
function ProviderBadge({ provider }: { provider?: string | null }) {
  if (!provider) return <span className="text-muted-foreground">—</span>;
  return <Badge tone="muted">{provider}</Badge>;
}

/**
 * The Agents area: a clean Notion-style list of every agent in the snapshot
 * (`snapshot.members`), de-centered from any Team. Columns: name, provider,
 * status/runtime, current task. A "New agent" affordance opens a small dialog
 * (POST /v1/agents). Selecting a row opens the agent detail page (`?agent=<id>`).
 * Whitespace-led, muted labels, no tile chrome.
 */
type AgentFilter = "all" | "online" | "working" | "idle" | "offline" | "unstable";
type AgentSort = "recent" | "name" | "runs";

/**
 * Classify an agent into ONE Multica status bucket. Online (alive) splits into a
 * clean partition — unstable XOR working XOR idle — so the filter chips never
 * double-count: an alive agent with a failing/unknown delivery probe is
 * "unstable" even while running (the health problem is what matters most), so it
 * is excluded from "working". Offline (not alive) is separate. Buckets:
 *   offline  = !runtime_alive
 *   unstable = alive && delivery health warn/bad
 *   working  = alive && healthy && (running || has current task)
 *   idle     = alive && healthy && not working
 * Online = alive = unstable + working + idle; All = online + offline.
 */
type AgentBucket = "offline" | "unstable" | "working" | "idle";
function agentBucket(agent: AgentMember): AgentBucket {
  if (!agent.runtime_alive) return "offline";
  const tone = deliveryHealthTone(agent);
  if (tone === "warn" || tone === "bad") return "unstable";
  const status = agent.runtime_status ?? agent.status;
  if (status === "running" || agent.current_task_id) return "working";
  return "idle";
}
function agentMatchesFilter(agent: AgentMember, filter: AgentFilter): boolean {
  if (filter === "all") return true;
  const bucket = agentBucket(agent);
  if (filter === "online") return bucket !== "offline";
  return bucket === filter;
}

export function AgentsList({ model, onSelectionChange, actionsEnabled, onAction }: SurfaceProps) {
  const [newAgentOpen, setNewAgentOpen] = useState(false);
  const [filter, setFilter] = useState<AgentFilter>("all");
  const [sort, setSort] = useState<AgentSort>("recent");
  const agents = model.members;
  const live = Boolean(actionsEnabled);

  const counts: Record<AgentFilter, number> = {
    all: agents.length,
    online: agents.filter((a) => agentMatchesFilter(a, "online")).length,
    working: agents.filter((a) => agentMatchesFilter(a, "working")).length,
    idle: agents.filter((a) => agentMatchesFilter(a, "idle")).length,
    offline: agents.filter((a) => agentMatchesFilter(a, "offline")).length,
    unstable: agents.filter((a) => agentMatchesFilter(a, "unstable")).length,
  };
  const filtered = agents
    .filter((a) => agentMatchesFilter(a, filter))
    .slice()
    .sort((a, b) => {
      if (sort === "name") return (a.name ?? a.id).localeCompare(b.name ?? b.id);
      const sa = model.statsByMember[a.id];
      const sb = model.statsByMember[b.id];
      if (sort === "runs") return (sb?.runCount30d ?? 0) - (sa?.runCount30d ?? 0);
      return (sb?.lastActiveMs ?? 0) - (sa?.lastActiveMs ?? 0); // recent
    });

  const FILTERS: { key: AgentFilter; label: string }[] = [
    { key: "all", label: "All" },
    { key: "online", label: "Online" },
    { key: "working", label: "Working" },
    { key: "idle", label: "Idle" },
    { key: "offline", label: "Offline" },
    { key: "unstable", label: "Unstable" },
  ];
  const cols =
    "grid-cols-[minmax(0,2fr)_minmax(0,0.8fr)_minmax(0,1fr)_minmax(0,1fr)_64px_minmax(0,0.6fr)_minmax(0,1.6fr)]";

  return (
    <DocumentSurface className="max-w-[1180px]">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Bot className="size-3.5" /> Agents
          </div>
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">Agents</h1>
          <p className="text-sm text-muted-foreground">
            Every agent in the workspace. Open one to message it, inspect its runtime, and assign work.
          </p>
        </div>
        <OperatorActionButton enabled={live} onClick={() => setNewAgentOpen(true)}>
          <UserPlus className="size-3.5" />
          New agent
        </OperatorActionButton>
      </header>

      {agents.length > 0 && (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-1">
            {FILTERS.map((entry) => (
              <button
                key={entry.key}
                type="button"
                onClick={() => setFilter(entry.key)}
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] transition-colors",
                  filter === entry.key
                    ? "border-primary/40 bg-primary/12 text-primary"
                    : "border-border bg-background/50 text-muted-foreground hover:text-foreground",
                )}
              >
                {entry.label}
                <span className="font-mono text-[10px] opacity-70">{counts[entry.key]}</span>
              </button>
            ))}
          </div>
          <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            Sort
            <Select
              aria-label="Sort agents"
              value={sort}
              onChange={(event) => setSort(event.target.value as AgentSort)}
              className="h-8 w-[7.5rem]"
            >
              <option value="recent">Recent</option>
              <option value="name">Name</option>
              <option value="runs">Runs</option>
            </Select>
          </label>
        </div>
      )}

      <DocSection label={`${filtered.length} ${filtered.length === 1 ? "agent" : "agents"}`}>
        {agents.length === 0 ? (
          <EmptyState
            icon={Bot}
            title="No agents yet"
            description={
              live
                ? "Create an agent with New agent to start delegating work."
                : "Connect to a running harness with Load live, then create your first agent with New agent."
            }
          />
        ) : filtered.length === 0 ? (
          <EmptyState
            icon={Bot}
            title="No agents match this filter"
            description="Clear the filter to see every agent."
          />
        ) : (
          <div className="overflow-hidden">
            <div
              className={cn(
                "grid gap-3 border-b border-border px-2 pb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
                cols,
              )}
            >
              <span>Name</span>
              <span>Provider</span>
              <span>Status</span>
              <span className="hidden lg:block">Workload</span>
              <span className="hidden lg:block">7-day</span>
              <span>Runs</span>
              <span>Current task</span>
            </div>
            <div>
              {filtered.map((agent) => {
                const status = agent.runtime_status ?? agent.status ?? "unknown";
                const stats = model.statsByMember[agent.id];
                const queued = agent.queued_count ?? 0;
                const inbox = agent.inbox_count ?? 0;
                return (
                  <button
                    key={agent.id}
                    type="button"
                    onClick={() =>
                      onSelectionChange({ surface: "agents", memberId: agent.id, agentTab: "conversation" })
                    }
                    className={cn(
                      "grid w-full items-center gap-3 border-b border-border/60 px-2 py-2.5 text-left transition-colors last:border-b-0 hover:bg-accent/40",
                      cols,
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-2.5">
                      <Avatar name={agent.name ?? agent.id} tone={deliveryHealthTone(agent)} />
                      <span className="min-w-0">
                        <span className="block truncate text-[13px] font-medium text-foreground">
                          {agent.name ?? agent.id}
                        </span>
                        <span className="block truncate text-[11px] text-muted-foreground">
                          {agent.role ?? "Member"}
                        </span>
                      </span>
                    </span>
                    <span className="min-w-0">
                      <ProviderBadge provider={agent.provider} />
                    </span>
                    <span className="flex min-w-0 items-center gap-1.5 text-[12px] text-foreground">
                      <StatusDot tone={memberTone(status)} pulse={status === "running"} />
                      <span className="truncate">{status}</span>
                    </span>
                    <span className="hidden min-w-0 text-[12px] text-muted-foreground lg:block">
                      {queued || inbox ? `${queued}q · ${inbox}in` : "—"}
                    </span>
                    <span className="hidden lg:block" title={stats ? `${stats.runCount30d} runs / 7d window` : undefined}>
                      <AgentSparkline data={stats?.activity7d ?? [0, 0, 0, 0, 0, 0, 0]} />
                    </span>
                    <span
                      className="min-w-0 text-[12px] tabular-nums text-muted-foreground"
                      title={
                        stats
                          ? `${stats.succeeded} ok / ${stats.failed} failed${stats.successRate != null ? ` (${Math.round(stats.successRate * 100)}%)` : ""}`
                          : undefined
                      }
                    >
                      {stats?.runCount30d ?? 0}
                    </span>
                    <span className="min-w-0 truncate text-[12px] text-muted-foreground">
                      {agent.current_task_id ? taskTitle(model.tasks, agent.current_task_id) : "—"}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </DocSection>

      <NewAgentForm
        open={newAgentOpen}
        actionsEnabled={live}
        onAction={onAction}
        onClose={() => setNewAgentOpen(false)}
      />
    </DocumentSurface>
  );
}

/* ------------------------------------------------------------------ */
/* Operator forms: create an agent with ZERO CLI                       */
/* ------------------------------------------------------------------ */

/** Header action button that stays honest about read-only mode. */
function OperatorActionButton({
  enabled,
  children,
  variant = "default",
  onClick,
}: {
  enabled: boolean;
  children: ReactNode;
  variant?: ComponentProps<typeof Button>["variant"];
  onClick: () => void;
}) {
  if (enabled) {
    return (
      <Button size="sm" variant={variant} onClick={onClick}>
        {children}
      </Button>
    );
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex">
          <Button size="sm" variant={variant} disabled title={ACTIONS_DISABLED_HINT}>
            {children}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom">{ACTIONS_DISABLED_HINT}</TooltipContent>
    </Tooltip>
  );
}

/**
 * NEW AGENT (POST /v1/agents). Requires name + role; provider (codex|claude),
 * model, description and skills are optional. The new agent appears in the
 * Agents list on the next snapshot. De-centered: an agent does not require a
 * team, so this form never asks for one.
 */
function NewAgentForm({
  open,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [role, setRole] = useState("");
  const [provider, setProvider] = useState("");
  const [modelName, setModelName] = useState("");
  const [description, setDescription] = useState("");
  const [skills, setSkills] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setRole("");
      setProvider("");
      setModelName("");
      setDescription("");
      setSkills("");
    }
  }, [open]);

  const canSubmit = Boolean(name.trim() && role.trim());
  function submit() {
    if (!canSubmit || !actionsEnabled) return;
    dispatch(
      onAction,
      createAgent({
        name: name.trim(),
        role: role.trim(),
        provider: provider || undefined,
        model: modelName.trim() || undefined,
        description: description.trim() || undefined,
        skills: parseList(skills),
      }),
    );
    onClose();
  }

  return (
    <Dialog
      open={open}
      title="New agent"
      description="Create an agent. POST /v1/agents."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Name" required>
          {(id) => (
            <TextInput
              id={id}
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="e.g. Backend Engineer"
            />
          )}
        </Field>
        <Field label="Role" required hint="e.g. lead, engineer, reviewer.">
          {(id) => (
            <TextInput
              id={id}
              value={role}
              onChange={(event) => setRole(event.target.value)}
              placeholder="e.g. engineer"
            />
          )}
        </Field>
        <Field label="Provider" hint="Defaults to codex when left as Default.">
          {(id) => (
            <Select id={id} value={provider} onChange={(event) => setProvider(event.target.value)}>
              <option value="">Default (codex)</option>
              <option value="codex">codex</option>
              <option value="claude">claude</option>
            </Select>
          )}
        </Field>
        <Field label="Model" hint="Optional provider model id (e.g. gpt-5-codex, claude-opus).">
          {(id) => (
            <TextInput
              id={id}
              value={modelName}
              onChange={(event) => setModelName(event.target.value)}
              placeholder="provider default"
            />
          )}
        </Field>
        <Field label="Description">
          {(id) => (
            <TextArea
              id={id}
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="What this member does."
            />
          )}
        </Field>
        <Field label="Skills" hint="Comma or newline separated skill refs (optional).">
          {(id) => (
            <TextInput
              id={id}
              value={skills}
              onChange={(event) => setSkills(event.target.value)}
              placeholder="e.g. rust, code-review"
            />
          )}
        </Field>
        <DialogFooter
          submitLabel="Create agent"
          actionsEnabled={actionsEnabled}
          canSubmit={canSubmit}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

/* ------------------------------------------------------------------ */
/* Vision overview                                                    */
/* ------------------------------------------------------------------ */

/**
 * Right-side slide-over that renders a project doc (a Vision `source_ref` or a
 * mounted doc). Fetches `GET /v1/docs?path=…` from the live source and renders
 * markdown; offline (no live source) it shows an honest fallback with the path.
 */
function DocSheet({
  apiUrl,
  path,
  onClose,
}: {
  apiUrl?: string;
  path: string;
  onClose: () => void;
}) {
  const [state, setState] = useState<
    { status: "loading" } | { status: "ok"; content: string } | { status: "error"; detail: string }
  >({ status: "loading" });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    let cancelled = false;
    if (!apiUrl) {
      setState({ status: "error", detail: "No live source — connect the harness to render docs." });
      return;
    }
    setState({ status: "loading" });
    fetchDoc(apiUrl, path)
      .then((doc) => {
        if (!cancelled) setState({ status: "ok", content: doc.content });
      })
      .catch((error: unknown) => {
        if (!cancelled)
          setState({ status: "error", detail: error instanceof Error ? error.message : String(error) });
      });
    return () => {
      cancelled = true;
    };
  }, [apiUrl, path]);

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      <button
        type="button"
        aria-label="Close document panel"
        className="absolute inset-0 bg-foreground/20 backdrop-blur-[1px]"
        onClick={onClose}
      />
      <aside
        role="dialog"
        aria-label="Document"
        className="relative flex h-full w-full max-w-[680px] flex-col border-l border-border bg-background shadow-xl"
      >
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
          <FileText className="size-4 text-muted-foreground" />
          <MonoId>{path}</MonoId>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="ml-auto grid size-8 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          {state.status === "loading" && (
            <p className="text-[13px] text-muted-foreground">Loading {path}…</p>
          )}
          {state.status === "error" && (
            <EmptyState icon={FileText} title="Cannot render doc" description={state.detail} />
          )}
          {state.status === "ok" && <Markdown source={state.content} />}
        </div>
      </aside>
    </div>
  );
}

export function VisionOverview({ model, onSelectionChange, apiUrl }: SurfaceProps) {
  const [docPath, setDocPath] = useState<string | null>(null);
  const groups: { id: string; title: string; goals: Goal[] }[] = [
    { id: "active", title: "Active", goals: model.activeGoals },
    { id: "complete", title: "Completed", goals: model.completeGoals },
    { id: "blocked", title: "Blocked", goals: model.blockedGoals },
    { id: "proposed", title: "Proposed", goals: model.proposedGoals },
  ];
  const proposals = model.snapshot.autonomous_proposals ?? [];
  const visions = model.visions;
  // Goals linked to each vision via Goal.vision_id, for the goal↔vision link.
  const goalsByVision = new Map<string, Goal[]>();
  for (const goal of model.goals) {
    if (goal.vision_id == null) continue;
    goalsByVision.set(goal.vision_id, [...(goalsByVision.get(goal.vision_id) ?? []), goal]);
  }
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Vision overview"
        title="Workbench self-hosting vision"
        description="Track whether active, completed, blocked and proposed goals are moving the harness toward a reusable self-hosting workflow."
        actions={
          <Button
            size="sm"
            variant="secondary"
            onClick={() => onSelectionChange({ surface: "tasks" })}
          >
            <Workflow className="size-3.5" />
            Open tasks
          </Button>
        }
      />

      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <ProofStat label="Active" value={model.activeGoals.length} tone="running" caption="not complete" />
        <ProofStat label="Completed" value={model.completeGoals.length} tone="good" caption="decision + eval" />
        <ProofStat
          label="Blocked"
          value={model.blockedGoals.length}
          tone={model.blockedGoals.length ? "bad" : "good"}
          caption="needs lead action"
        />
        <ProofStat label="Proposed" value={model.proposedGoals.length} tone="decision" caption="awaiting accept" />
      </div>

      <Section kicker="Durable product vision" title="Visions" className="rise">
        <div className="space-y-2 p-3">
          {visions.length ? (
            visions.map((vision) => (
              <VisionRow
                key={vision.id}
                vision={vision}
                goals={goalsByVision.get(vision.id) ?? []}
                onSelectGoal={(goalId) => onSelectionChange({ goalId, surface: "goal" })}
                onOpenDoc={setDocPath}
              />
            ))
          ) : (
            <EmptyState
              icon={Target}
              title="No visions recorded"
              description="A Vision is the durable product direction a goal is scheduled against."
            />
          )}
        </div>
      </Section>

      <div className="grid gap-4 xl:grid-cols-[1fr_20rem]">
        <Section kicker="Completion proven by decision + evaluation" title="Goal collection" className="rise">
          <div className="grid gap-3 p-3 sm:grid-cols-2">
            {groups.map((group) => (
              <div key={group.id}>
                <p className="mb-1.5 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                  <StatusDot tone={goalTone(group.id)} /> {group.title}
                  <span className="font-mono text-muted-foreground/60">
                    {group.goals.length}
                  </span>
                </p>
                <div className="space-y-2">
                  {group.goals.length ? (
                    group.goals.map((goal) => (
                      <GoalCard
                        key={goal.id}
                        goal={goal}
                        model={model}
                        onSelect={() =>
                          onSelectionChange({ goalId: goal.id, surface: "goal" })
                        }
                      />
                    ))
                  ) : (
                    <p className="rounded-md border border-dashed border-border px-3 py-4 text-center text-[11px] text-muted-foreground">
                      None
                    </p>
                  )}
                </div>
              </div>
            ))}
          </div>
        </Section>

        <Section kicker="Distance-to-vision" title="Next-round proposals" className="rise">
          <div className="space-y-2 p-3">
            {proposals.length ? (
              proposals.slice(0, 5).map((proposal) => (
                <div
                  key={proposal.id}
                  className="rounded-md border border-border bg-background/40 p-3"
                >
                  <div className="flex items-center gap-2">
                    <Badge tone="decision">{proposal.disposition ?? "pending"}</Badge>
                    <MonoId>{proposal.source_type ?? "observer"}</MonoId>
                  </div>
                  <p className="mt-1.5 text-[13px] font-medium leading-snug">
                    {proposal.summary ?? "Proposed next step"}
                  </p>
                  <div className="mt-2 flex gap-1.5">
                    <Badge tone={proposal.linked_evidence_ids?.length ? "good" : "warn"}>
                      {proposal.linked_evidence_ids?.length ?? 0} evidence
                    </Badge>
                    <Badge tone="info">
                      {proposal.follow_up_task_ids?.length ?? 0} follow-ups
                    </Badge>
                  </div>
                </div>
              ))
            ) : (
              <EmptyState
                icon={Target}
                title="No next proposals"
                description="Observer proposals appear here when linked to evidence or evaluation."
              />
            )}
          </div>
        </Section>
      </div>

      {docPath && (
        <DocSheet apiUrl={apiUrl} path={docPath} onClose={() => setDocPath(null)} />
      )}
    </div>
  );
}

/** A Vision with the goals scheduled against it (goal↔vision link). */
function VisionRow({
  vision,
  goals,
  onSelectGoal,
  onOpenDoc,
}: {
  vision: Vision;
  goals: Goal[];
  onSelectGoal: (goalId: string) => void;
  onOpenDoc: (path: string) => void;
}) {
  return (
    <div className="rounded-md border border-border bg-background/40 p-3">
      <div className="flex items-center gap-2">
        <Target className="size-3.5 text-primary" />
        <MonoId>{vision.id}</MonoId>
        <Badge tone={goals.length ? "good" : "muted"}>{goals.length} goals</Badge>
      </div>
      <p className="mt-1.5 text-[13px] leading-snug text-foreground/90">
        {vision.summary ?? "No summary recorded"}
      </p>
      {goals.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {goals.map((goal) => (
            <button
              key={goal.id}
              type="button"
              onClick={() => onSelectGoal(goal.id)}
              className="rounded-md border border-border bg-muted/40 px-2 py-0.5 text-[11px] text-foreground/90 transition-colors hover:bg-muted"
            >
              {goal.title ?? goal.id}
            </button>
          ))}
        </div>
      )}
      {vision.source_refs && vision.source_refs.length > 0 && (
        <div className="mt-2 flex flex-col items-start gap-1">
          <p className="text-[10px] uppercase tracking-wider text-muted-foreground">Narrative</p>
          {vision.source_refs.map((ref) => (
            <button
              key={ref}
              type="button"
              onClick={() => onOpenDoc(ref)}
              className="inline-flex items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 py-1 text-[11px] transition-colors hover:border-input hover:bg-accent/40"
            >
              <FileText className="size-3 text-muted-foreground" />
              <MonoId>{ref}</MonoId>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Goal document                                                      */
/* ------------------------------------------------------------------ */

/**
 * A bounded section that is collapsed by default — used to push the
 * design/evaluation/closeout depth below the fold so the Goal page reads like a
 * clean Notion document, not a proof wall (ADR 0019).
 */
function CollapsibleSection({
  kicker,
  title,
  badge,
  defaultOpen = false,
  children,
}: {
  kicker: string;
  title: string;
  badge?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  return (
    <details className="rise group rounded-lg border border-border bg-card" open={defaultOpen}>
      <summary className="flex cursor-pointer list-none items-center gap-2.5 px-4 py-3">
        <ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" />
        <div className="min-w-0">
          <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{kicker}</div>
          <div className="text-[13px] font-semibold">{title}</div>
        </div>
        {badge && <span className="ml-auto">{badge}</span>}
      </summary>
      <div className="border-t border-border">{children}</div>
    </details>
  );
}

/** Compact per-status task counts + a jump to the goal-filtered Work board. */
function GoalTasksJump({
  model,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const goal = model.selectedGoal;
  const tasks = model.goalTasks;
  const done = tasks.filter((task) => task.status === "done").length;
  const counts = TASK_COLUMNS.map((status) => ({
    status,
    n: tasks.filter((task) => task.status === status).length,
  })).filter((entry) => entry.n > 0);
  return (
    <div className="p-4">
      <div className="flex flex-wrap items-center gap-3">
        <div className="text-2xl font-semibold tabular-nums">
          {done}
          <span className="text-base font-normal text-muted-foreground">/{tasks.length}</span>
        </div>
        <span className="text-xs text-muted-foreground">tasks done</span>
        <Button
          size="sm"
          className="ml-auto"
          disabled={!goal}
          onClick={() =>
            goal &&
            onSelectionChange({ surface: "tasks", boardScope: "tasks", boardGoal: goal.id })
          }
        >
          <Workflow className="size-3.5" />
          View tasks ({tasks.length})
        </Button>
      </div>
      {counts.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {counts.map((entry) => (
            <span
              key={entry.status}
              className="inline-flex items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 py-1 text-[11px]"
            >
              <StatusDot tone={taskTone(entry.status)} />
              <span className="capitalize">{entry.status}</span>
              <span className="font-mono text-muted-foreground">{entry.n}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

export function GoalDocument({ model, onSelectionChange }: SurfaceProps) {
  const goal = model.selectedGoal;
  if (!goal) {
    return (
      <EmptyState
        icon={ClipboardList}
        title="No goal selected"
        description="Pick a goal from the Vision overview."
      />
    );
  }

  const learning = (model.snapshot.goal_learning_status ?? []).find(
    (item) => item.goal_id === goal.id,
  );
  const goalDecision = model.decisions.find((d) =>
    model.goalTasks.some((t) => t.id === d.task_id),
  );
  const goalProposals = (model.snapshot.autonomous_proposals ?? []).filter(
    (p) => p.goal_id === goal.id,
  );
  // Dual-read: a graduated GoalDesign/GoalEvaluation object OR a legacy
  // Evidence row satisfies the closeout invariant.
  const design = model.goalDesignsForGoal[0];
  const evaluation = model.goalEvaluationsForGoal[0];
  const hasEvaluation =
    Boolean(evaluation) || (learning?.goal_evaluation?.length ?? 0) > 0;
  const hasDesign =
    Boolean(design) || (learning?.goal_design?.length ?? 0) > 0;
  const hasDecision = Boolean(goalDecision);
  // Closeout gate (§3.7): the CLI computes readiness; the UI mirrors it. A goal may
  // close only with a closeout Decision + GoalEvaluation, or a valid waiver.
  const hasCloseoutDecision = learning?.has_closeout_decision ?? false;
  const hasCloseoutWaiver = learning?.has_closeout_waiver ?? false;
  const mayClose = learning?.may_close ?? false;
  const closeoutBlockers = learning?.closeout_blockers ?? [];
  const blockedTasks = model.goalTasks.filter((t) => t.status === "blocked");

  const learningChips = [
    { label: "Goal design", n: model.goalDesignsForGoal.length + learningCount(learning?.goal_design) },
    { label: "Evaluation", n: model.goalEvaluationsForGoal.length + learningCount(learning?.goal_evaluation) },
    { label: "Goal cases", n: model.goalCasesForGoal.length + learningCount(learning?.goal_cases) },
    { label: "Reports", n: learningCount(learning?.member_reports) },
    { label: "Follow-ups", n: learningCount(learning?.follow_up_tasks) },
    { label: "Blocked", n: blockedTasks.length },
  ];

  return (
    <DocumentSurface>
      <header className="space-y-3">
        <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          <Target className="size-3.5" /> Goal
        </div>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">
            {goal.title ?? goal.id}
          </h1>
          <div className="flex shrink-0 items-center gap-1.5 pt-1">
            {goal.priority && <Badge tone="info">{goal.priority}</Badge>}
            <Badge tone={goalTone(goal.status)}>{displayGoalStatus(goal)}</Badge>
          </div>
        </div>
        <DocProperties
          items={[
            { label: "Owner", value: memberName(model.members, goal.owner_agent_id) },
            { label: "Team", value: model.selectedTeam?.name ?? "—" },
            { label: "Vision", value: model.visionForGoal?.summary ?? "—" },
            { label: "Created", value: fmtTime(goal.created_at) },
            { label: "Updated", value: fmtTime(goal.updated_at) },
          ]}
        />
      </header>

      <DocSection label="Objective">
        <p className="text-[15px] leading-relaxed text-foreground/90">
          {goal.objective ?? "No objective recorded."}
        </p>
      </DocSection>

      <DocSection label="Success criteria">
        <CriteriaList items={goal.success_criteria} empty="No success criteria recorded" />
      </DocSection>

      <DocSection label="Tasks">
        <div className="rounded-lg border border-border bg-card">
          <GoalTasksJump model={model} onSelectionChange={onSelectionChange} />
        </div>
      </DocSection>

      <CollapsibleSection kicker="Executable thesis" title="Goal design">
        <GoalDesignSection design={design} />
      </CollapsibleSection>

      <CollapsibleSection kicker="Retrospective" title="Goal evaluation">
        <GoalEvaluationSection evaluation={evaluation} />
      </CollapsibleSection>

      <CollapsibleSection
        kicker="Closeout invariant"
        title="Closeout & decision"
        badge={<Badge tone={mayClose ? "good" : "warn"}>{mayClose ? "may close" : "blocked"}</Badge>}
      >
        <div className="space-y-3 p-4">
          <p className="text-xs text-muted-foreground">
            A goal is complete only after a Leader decision and a GoalEvaluation —
            never just because its tasks are done.
          </p>
          <ProofRow ok={hasDesign} label="GoalDesign" detail={hasDesign ? "recorded" : "missing"} />
          <ProofRow ok={hasDecision} label="Leader decision" detail={goalDecision?.decision ?? "missing"} />
          <ProofRow ok={hasEvaluation} label="GoalEvaluation" detail={hasEvaluation ? "recorded" : "missing"} />
          <ProofRow
            ok={hasCloseoutDecision}
            label="Closeout decision"
            detail={hasCloseoutDecision ? "recorded (kind=closeout, evidence)" : "missing"}
          />
          <ProofRow
            ok={mayClose}
            label="May close"
            detail={
              mayClose
                ? hasCloseoutWaiver
                  ? "yes (via waiver)"
                  : "yes (decision + evaluation)"
                : closeoutBlockers.length
                  ? closeoutBlockers.join("; ")
                  : "blocked"
            }
          />
        </div>
      </CollapsibleSection>

      <DocSection label="Learning">
        <div className="flex flex-wrap gap-1.5">
          {learningChips.map((chip) => (
            <span
              key={chip.label}
              className="inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11px]"
            >
              {chip.label}
              <span className="font-mono text-muted-foreground">{chip.n}</span>
            </span>
          ))}
        </div>
      </DocSection>

      {goalProposals.length > 0 && (
        <DocSection label="Next-round proposals">
          <div className="space-y-2">
            {goalProposals.slice(0, 4).map((proposal) => (
              <div key={proposal.id} className="rounded-lg border border-border bg-card p-3">
                <div className="flex items-center gap-2">
                  <Badge tone="decision">{proposal.disposition ?? "pending"}</Badge>
                  <MonoId>{proposal.source_type ?? "observer"}</MonoId>
                </div>
                <p className="mt-1.5 text-[13px] text-foreground/90">
                  {proposal.summary ?? "Proposed next step"}
                </p>
              </div>
            ))}
          </div>
        </DocSection>
      )}
    </DocumentSurface>
  );
}

function learningCount(value?: unknown[]): number {
  return value?.length ?? 0;
}

/** A labeled bullet list used by the GoalDesign / GoalEvaluation sections. */
function LabeledList({
  label,
  items,
  tone = "info",
}: {
  label: string;
  items?: string[];
  tone?: StatusTone;
}) {
  if (!items?.length) return null;
  return (
    <div>
      <p className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        <StatusDot tone={tone} /> {label}
      </p>
      <ul className="space-y-1">
        {items.map((item, index) => (
          <li key={index} className="flex items-start gap-2 text-[13px] text-foreground/90">
            <span className="mt-1 size-1 shrink-0 rounded-full bg-muted-foreground/60" />
            <span>{item}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Render a GoalDesign as a real section: scenario, non-goals, acceptance gates. */
function GoalDesignSection({ design }: { design?: GoalDesign }) {
  if (!design) {
    return (
      <EmptyState
        title="No goal design recorded"
        description="A GoalDesign captures the scenario, non-goals, and acceptance gates before work starts."
      />
    );
  }
  return (
    <div className="space-y-3 p-4">
      <div className="flex items-center gap-2">
        <MonoId>{design.id}</MonoId>
        {design.agent_team && <Badge tone="info">team: {design.agent_team}</Badge>}
      </div>
      {design.scenario_summary && (
        <p className="text-[13px] leading-relaxed text-foreground/90">
          {design.scenario_summary}
        </p>
      )}
      {design.risk_and_permission_boundaries && (
        <div>
          <p className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            <StatusDot tone="warn" /> Risk & permission boundaries
          </p>
          <p className="text-[13px] text-foreground/90">
            {design.risk_and_permission_boundaries}
          </p>
        </div>
      )}
      <LabeledList label="Non-goals" items={design.non_goals} tone="bad" />
      <LabeledList label="Required infra" items={design.required_infra} tone="info" />
      <LabeledList label="Acceptance gates" items={design.acceptance_gates} tone="good" />
    </div>
  );
}

/** Render a GoalEvaluation as a real section: outcome, what worked/failed, patterns. */
function GoalEvaluationSection({ evaluation }: { evaluation?: GoalEvaluation }) {
  if (!evaluation) {
    return (
      <EmptyState
        title="No goal evaluation recorded"
        description="A GoalEvaluation captures what worked, what failed, and reusable patterns for the next round."
      />
    );
  }
  return (
    <div className="space-y-3 p-4">
      <div className="flex items-center gap-2">
        <Badge tone={evaluationOutcomeTone(evaluation.outcome)}>
          {evaluation.outcome ?? "unknown"}
        </Badge>
        <MonoId>{evaluation.id}</MonoId>
      </div>
      {evaluation.what_worked && (
        <div>
          <p className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            <StatusDot tone="good" /> What worked
          </p>
          <p className="text-[13px] text-foreground/90">{evaluation.what_worked}</p>
        </div>
      )}
      {evaluation.what_failed && (
        <div>
          <p className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            <StatusDot tone="bad" /> What failed
          </p>
          <p className="text-[13px] text-foreground/90">{evaluation.what_failed}</p>
        </div>
      )}
      <LabeledList label="Reusable patterns" items={evaluation.reusable_patterns} tone="good" />
      <LabeledList label="Anti-patterns" items={evaluation.anti_patterns} tone="bad" />
      <LabeledList label="Missing infra" items={evaluation.missing_infra} tone="warn" />
    </div>
  );
}

/** Map a GoalEvaluation outcome (open enum) to a status tone. */
function evaluationOutcomeTone(outcome?: string): StatusTone {
  switch ((outcome ?? "").toLowerCase()) {
    case "success":
      return "good";
    case "partial":
      return "warn";
    case "failed":
      return "bad";
    case "blocked":
      return "bad";
    default:
      return "info";
  }
}

/* ------------------------------------------------------------------ */
/* Task document                                                      */
/* ------------------------------------------------------------------ */

/**
 * Delivery-proof badge for an @-mention slot. It reflects the delivery_status of
 * the instruction message behind the assignment (NOT the bare field write), so a
 * reviewer/assignee that was named but never actually instructed reads honestly
 * as "queued" / "not handed off" rather than masquerading as done — keeping the
 * assignment-proof invariant visible (concept-model Anti-Drift #2).
 */
function DeliveryBadge({
  status,
  note,
}: {
  status?: DeliveryStatus | null;
  note?: string;
}) {
  if (note) return <Badge tone="muted">{note}</Badge>;
  switch (status) {
    case "delivered":
      return <Badge tone="good">delivered</Badge>;
    case "acknowledged":
      return <Badge tone="good">acknowledged</Badge>;
    case "queued":
      return <Badge tone="warn">queued · not delivered</Badge>;
    case "failed":
      return <Badge tone="bad">delivery failed</Badge>;
    default:
      return null;
  }
}

/**
 * The @-mention assignment chip + picker for a Task property slot (executor /
 * reviewer). It is UI sugar over existing objects — picking an agent dispatches
 * the reuse action (assign / set-reviewer) and the chip carries a DeliveryBadge.
 * Read-only (offline) it collapses to a plain label so it never looks editable
 * when it isn't.
 */
function MentionSlot({
  members,
  currentId,
  live,
  onPick,
  placeholder,
  delivery,
}: {
  members: AgentMember[];
  currentId?: string | null;
  live: boolean;
  onPick: (agentId: string) => void;
  placeholder: string;
  delivery?: { status?: DeliveryStatus | null; note?: string };
}) {
  const [editing, setEditing] = useState(false);
  const current = currentId ? members.find((m) => m.id === currentId) : undefined;

  if (!current || editing) {
    if (!live) {
      return (
        <span className="text-muted-foreground">
          {current ? `@${current.name ?? current.id}` : "—"}
        </span>
      );
    }
    return (
      <span className="inline-flex items-center gap-2">
        <Select
          aria-label={placeholder}
          defaultValue=""
          onChange={(event) => {
            const id = event.target.value;
            // Reset to the placeholder so the same agent can be re-picked and the
            // slot reads as a gesture, not a sticky selection. Uncontrolled
            // (defaultValue) so React never fights the change event.
            event.target.value = "";
            if (!id) return;
            onPick(id);
            setEditing(false);
          }}
          className="h-8 max-w-[15rem]"
        >
          <option value="">{placeholder}</option>
          {members.map((member) => (
            <option key={member.id} value={member.id}>
              @{member.name ?? member.id}
            </option>
          ))}
        </Select>
        {editing && current && (
          <button
            type="button"
            className="text-[11px] text-muted-foreground hover:text-foreground"
            onClick={() => setEditing(false)}
          >
            cancel
          </button>
        )}
      </span>
    );
  }

  const status = current.runtime_status ?? current.status ?? "unknown";
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-muted/40 py-0.5 pl-1 pr-2.5">
        <Avatar name={current.name ?? current.id} size="sm" tone={memberTone(status)} />
        <span className="text-[12px] font-medium text-foreground">
          @{current.name ?? current.id}
        </span>
      </span>
      <DeliveryBadge status={delivery?.status} note={delivery?.note} />
      {live && (
        <button
          type="button"
          className="text-[11px] text-muted-foreground hover:text-foreground"
          onClick={() => setEditing(true)}
        >
          change
        </button>
      )}
    </span>
  );
}

export function TaskDocument({
  model,
  onSelectionChange,
  actionsEnabled,
  onAction,
}: SurfaceProps) {
  const task = model.selectedTask;
  if (!task) {
    return (
      <EmptyState
        icon={GitBranch}
        title="No task selected"
        description="Select a task from a goal or the activity stream."
      />
    );
  }

  const goal = model.goals.find((g) => g.id === task.goal_id);
  const parent = model.tasks.find((t) => t.id === task.parent_task_id);
  const messages = model.messages.filter((message) => message.task_id === task.id);
  const evidence = model.evidence.filter((item) => item.task_id === task.id);
  const proposals = model.proposals.filter((item) => item.task_id === task.id);
  const decision = model.decisions.find((item) => item.task_id === task.id);
  const reviews = model.reviewsForTask;
  const sessions = (model.snapshot.provider_sessions ?? []).filter(
    (s) => s.task_id === task.id,
  );
  const taskWarnings = model.warnings.filter((warning) => warning.taskId === task.id);
  const dependsOn = task.depends_on_task_ids ?? [];
  const blocks = tasksBlockedBy(task.id, model.tasks).map((t) => t.id);
  const readiness = readinessFor(task, model.taskGraph);
  const git = taskGitMetadata(task);
  const live = Boolean(actionsEnabled);

  // Assignment-proof for the @-mention chips: the delivery_status of the
  // instruction behind each slot, not the bare field. Assignee → latest
  // Message(kind=task) (assign queues, never delivers, so "queued" is the honest
  // default once a field is set). Reviewer → latest review-request message; a
  // named-but-not-handed-off reviewer reads "not handed off".
  const taskMsgs = messages.filter((m) => m.kind === "task");
  const assignmentMsg = taskMsgs[taskMsgs.length - 1];
  const assignmentDelivery = task.assignee_agent_id
    ? { status: assignmentMsg?.delivery_status ?? "queued" }
    : undefined;
  const reviewMsgs = messages.filter((m) => m.channel === "review-request");
  const reviewMsg = reviewMsgs[reviewMsgs.length - 1];
  const reviewDelivery = task.reviewer_agent_id
    ? reviewMsg
      ? { status: reviewMsg.delivery_status }
      : { note: "not handed off" }
    : undefined;

  return (
    <DocumentSurface>
      <header className="space-y-3">
        <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
          {goal && (
            <>
              <button
                type="button"
                className="inline-flex items-center gap-1 hover:text-foreground"
                onClick={() => onSelectionChange({ goalId: goal.id, surface: "goal" })}
              >
                <Target className="size-3" />
                {goal.title ?? goal.id}
              </button>
              <span className="text-border">/</span>
            </>
          )}
          {parent && (
            <>
              <button
                type="button"
                className="inline-flex items-center gap-1 hover:text-foreground"
                onClick={() => onSelectionChange({ taskId: parent.id, surface: "task" })}
              >
                <GitBranch className="size-3" />
                {parent.title ?? parent.id}
              </button>
              <span className="text-border">/</span>
            </>
          )}
          <MonoId>{task.id}</MonoId>
        </div>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">
            {task.title ?? task.id}
          </h1>
          <div className="flex shrink-0 items-center gap-1.5 pt-1">
            <ReadinessChip readiness={readiness} />
            <Badge tone={taskTone(task.status)}>{task.status}</Badge>
            <ActionButton
              enabled={actionsEnabled}
              size="sm"
              variant="secondary"
              onClick={() => dispatch(onAction, requestReview(task.id))}
            >
              <ShieldCheck className="size-3.5" />
              Request review
            </ActionButton>
          </div>
        </div>
        <DocProperties
          items={[
            { label: "Owner", value: ownerLine(model, task.owner_agent_id) },
            {
              label: "Assignee",
              value: (
                <MentionSlot
                  members={model.members}
                  currentId={task.assignee_agent_id}
                  live={live}
                  onPick={(agentId) => dispatch(onAction, assignTask(task.id, agentId))}
                  placeholder="@ assign executor…"
                  delivery={assignmentDelivery}
                />
              ),
            },
            {
              label: "Reviewer",
              value: (
                <MentionSlot
                  members={model.members}
                  currentId={task.reviewer_agent_id}
                  live={live}
                  onPick={(agentId) => dispatch(onAction, setReviewer(task.id, agentId))}
                  placeholder="@ add reviewer…"
                  delivery={reviewDelivery}
                />
              ),
            },
            { label: "Branch", value: git.branch ? <MonoId>{git.branch}</MonoId> : "—" },
            { label: "PR", value: git.pr_ref ? <MonoId>{shortBranch(git.pr_ref)}</MonoId> : "—" },
            { label: "Worktree", value: git.worktree_path ? <MonoId>{git.worktree_path}</MonoId> : "—" },
            { label: "Owned paths", value: <PathList paths={git.owned_paths} /> },
            { label: "Sessions", value: sessions.length },
            { label: "Updated", value: fmtTime(task.updated_at) },
          ]}
        />
      </header>

      <DocSection label="Objective">
        <p className="text-[15px] leading-relaxed text-foreground/90">
          {task.objective ?? "No objective recorded."}
        </p>
      </DocSection>

      {task.description && (
        <DocSection label="Description">
          <p className="whitespace-pre-wrap text-[15px] leading-relaxed text-foreground/90">
            {task.description}
          </p>
        </DocSection>
      )}

      <DocSection
        label="Acceptance criteria"
        action={
          <Badge tone={task.acceptance_criteria?.length ? "info" : "warn"}>
            {task.acceptance_criteria?.length ?? 0}
          </Badge>
        }
      >
        <div className="rounded-lg border border-border bg-card">
          <CriteriaList
            items={task.acceptance_criteria}
            empty="No acceptance criteria — this task cannot be objectively reviewed yet."
          />
        </div>
      </DocSection>

      <DocSection label="Dependencies">
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <p className="mb-1.5 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              <Link2 className="size-3" /> Depends on
            </p>
            <DependencyChips
              ids={dependsOn}
              tasks={model.tasks}
              empty="No upstream dependencies."
              onSelect={(id) => onSelectionChange({ taskId: id, surface: "task" })}
            />
          </div>
          <div>
            <p className="mb-1.5 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              <Link2 className="size-3 rotate-90" /> Blocks
            </p>
            <DependencyChips
              ids={blocks}
              tasks={model.tasks}
              empty="Nothing depends on this task."
              onSelect={(id) => onSelectionChange({ taskId: id, surface: "task" })}
            />
          </div>
        </div>
      </DocSection>

      <DocSection label="Proof chain">
        <div className="space-y-3 rounded-lg border border-border bg-card p-4">
          <ProofRow
            ok={messages.some((m) => m.kind === "task")}
            label="Assignment message"
            detail={`${messages.filter((m) => m.kind === "task").length} task message(s)`}
          />
          <ProofRow
            ok={messages.some((m) => m.kind === "report")}
            label="Member report"
            detail={`${messages.filter((m) => m.kind === "report").length} report(s)`}
          />
          <ProofRow ok={evidence.length > 0} label="Evidence" detail={`${evidence.length} item(s)`} />
          <ProofRow
            ok={reviews.length > 0}
            label="Evaluator review"
            detail={reviews.length ? `${reviews.length} review(s)` : "no structured review"}
          />
          <ProofRow ok={Boolean(decision)} label="Leader decision" detail={decision?.decision ?? "missing"} />
        </div>
      </DocSection>

      <DocSection
        label="Reviews"
        action={
          <Badge tone={reviews.some((r) => ["fail", "blocked"].includes((r.verdict ?? "").toLowerCase())) ? "bad" : reviews.length ? "good" : "muted"}>
            {reviews.length}
          </Badge>
        }
      >
        <div className="rounded-lg border border-border bg-card">
          <ReviewList reviews={reviews} />
        </div>
      </DocSection>

      <DocSection label="Decision & rationale">
        {decision ? (
          <div className="space-y-2 rounded-lg border border-border bg-card p-4">
            <div className="flex items-center gap-2">
              <Scale className="size-4 text-status-good" />
              <Badge tone="good">{decision.decision ?? "decided"}</Badge>
            </div>
            <p className="text-[13px] text-foreground/90">
              {decision.rationale ?? "No rationale recorded."}
            </p>
            {Boolean(decision.evidence_ids?.length) && (
              <div className="flex flex-wrap gap-1.5">
                {decision.evidence_ids!.map((id) => (
                  <Badge key={id} tone="muted">
                    <MonoId>{id}</MonoId>
                  </Badge>
                ))}
              </div>
            )}
          </div>
        ) : (
          <div className="rounded-lg border border-border bg-card">
            <EmptyState icon={Gavel} title="No decision yet" description="Awaiting review and a Leader decision." />
          </div>
        )}
      </DocSection>

      <DocSection label="Evidence & proposals" action={<Badge tone="muted">{evidence.length + proposals.length}</Badge>}>
        <div className="rounded-lg border border-border bg-card">
          {evidence.length || proposals.length ? (
            <div className="divide-y divide-border/60">
              {evidence.map((item) => (
                <div key={item.id} className="flex items-start gap-2.5 px-4 py-2.5">
                  <FileText className="mt-0.5 size-3.5 shrink-0 text-status-info" />
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <Badge tone="info">{item.source_type ?? "evidence"}</Badge>
                      {item.source_ref && <MonoId>{item.source_ref}</MonoId>}
                    </div>
                    <p className="mt-0.5 text-xs text-muted-foreground">{item.summary}</p>
                  </div>
                </div>
              ))}
              {proposals.map((item) => (
                <div key={item.id} className="flex items-start gap-2.5 px-4 py-2.5">
                  <ListChecks className="mt-0.5 size-3.5 shrink-0 text-status-decision" />
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium">{item.title ?? "Proposal"}</span>
                      <Badge tone="decision">{item.status ?? "draft"}</Badge>
                    </div>
                    <p className="mt-0.5 text-xs text-muted-foreground">{item.summary}</p>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <EmptyState icon={FileText} title="No evidence or proposals yet" />
          )}
        </div>
      </DocSection>

      <DocSection label="Assignment & reports">
        <div className="rounded-lg border border-border bg-card">
          {messages.length ? (
            <div className="max-h-72 overflow-y-auto">
              {messages.map((message) => (
                <TimelineRow
                  key={message.id}
                  kind={message.kind}
                  title={
                    message.kind === "task"
                      ? "Task assignment"
                      : message.kind === "report"
                        ? "Member report"
                        : "Message"
                  }
                  meta={message.delivery_status}
                  body={message.content}
                  tone={message.delivery_status === "failed" ? "bad" : "info"}
                />
              ))}
            </div>
          ) : (
            <EmptyState icon={MessageSquare} title="No messages for this task" />
          )}
        </div>
      </DocSection>

      {taskWarnings.length > 0 && (
        <DocSection label="Warnings" action={<Badge tone="bad">{taskWarnings.length}</Badge>}>
          <div className="rounded-lg border border-border bg-card">
            <WarningList
              warnings={taskWarnings}
              onSelect={() => onSelectionChange({ surface: "warnings" })}
            />
          </div>
        </DocSection>
      )}
    </DocumentSurface>
  );
}

function ownerLine(model: WorkbenchModel, id?: string | null) {
  if (!id) return "—";
  return (
    <span className="inline-flex items-center gap-1.5">
      <User className="size-3 text-muted-foreground" />
      {memberName(model.members, id)}
    </span>
  );
}

function ProofRow({ ok, label, detail }: { ok: boolean; label: string; detail: string }) {
  return (
    <div className="flex items-center gap-3">
      {ok ? (
        <CheckCircle2 className="size-4 shrink-0 text-status-good" />
      ) : (
        <AlertTriangle className="size-4 shrink-0 text-status-warn" />
      )}
      <span className="text-[13px] font-medium">{label}</span>
      <span className="ml-auto text-[11px] text-muted-foreground">{detail}</span>
    </div>
  );
}

/** Bullet list of short strings used inside a Review card (blockers / missing validation). */
function ReviewBullets({
  label,
  items,
  tone,
}: {
  label: string;
  items?: string[];
  tone: "bad" | "warn";
}) {
  if (!items?.length) return null;
  return (
    <div>
      <p className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
        {label}
      </p>
      <ul className="space-y-1">
        {items.map((item, index) => (
          <li key={index} className="flex items-start gap-1.5 text-xs text-foreground/90">
            <AlertTriangle
              className={cn(
                "mt-0.5 size-3 shrink-0",
                tone === "bad" ? "text-status-bad" : "text-status-warn",
              )}
            />
            <span>{item}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * Structured evaluator output. Today reviews are unstructured report messages;
 * this renders the Review object's verdict, blockers, residual risk and missing
 * validation so the evaluation is legible without reading raw JSON.
 */
function ReviewList({ reviews }: { reviews: Review[] }) {
  if (!reviews.length) {
    return (
      <EmptyState
        icon={ShieldAlert}
        title="No structured reviews yet"
        description="Evaluator/critic verdicts (pass/fail/blocked/needs_changes) will appear here once recorded."
      />
    );
  }
  return (
    <div className="divide-y divide-border/60">
      {reviews.map((review) => {
        const verdict = review.verdict ?? "unknown";
        const verdictIsBad = ["fail", "blocked"].includes(verdict.toLowerCase());
        return (
          <div key={review.id} className="space-y-2.5 px-4 py-3">
            <div className="flex flex-wrap items-center gap-2">
              {verdictIsBad ? (
                <ShieldAlert className="size-4 shrink-0 text-status-bad" />
              ) : (
                <ShieldCheck className="size-4 shrink-0 text-status-good" />
              )}
              <Badge tone={reviewVerdictTone(verdict)}>{verdict}</Badge>
              {review.review_kind && <Badge tone="muted">{review.review_kind}</Badge>}
              <span className="ml-auto text-[11px] text-muted-foreground">
                {memberShort(review.reviewer_agent_id)}
              </span>
            </div>
            <p className="text-[13px] leading-relaxed text-foreground/90">
              {review.summary ?? "No summary recorded."}
            </p>
            <ReviewBullets label="Blockers" items={review.blockers} tone="bad" />
            <ReviewBullets
              label="Missing validation"
              items={review.missing_validation}
              tone="warn"
            />
            {review.residual_risk && (
              <div>
                <p className="mb-0.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                  Residual risk
                </p>
                <p className="text-xs text-foreground/80">{review.residual_risk}</p>
              </div>
            )}
            {Boolean(review.evidence_ids?.length) && (
              <div className="flex flex-wrap gap-1.5">
                {review.evidence_ids!.map((id) => (
                  <Badge key={id} tone="muted">
                    <MonoId>{id}</MonoId>
                  </Badge>
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function memberShort(id?: string | null): string {
  if (!id) return "unknown reviewer";
  return id.replace(/^agent-/, "");
}

/* ------------------------------------------------------------------ */
/* Gap ledger                                                         */
/* ------------------------------------------------------------------ */

const gapSeverityGroups: { id: string; title: string }[] = [
  { id: "p0", title: "P0 · critical" },
  { id: "p1", title: "P1 · high" },
  { id: "p2", title: "P2 · normal" },
];

/**
 * The Gap ledger (absorbs the bug ledger). Grouped by severity (p0→p2); within a
 * group, unresolved gaps sort above fixed/wontfix ones (readModel pre-sorts). A
 * Bug is rendered as a Gap with category="bug", with its repro/closing-test refs.
 */
function GapLedger({
  gapsBySeverity,
  onSelect,
}: {
  gapsBySeverity: Map<string, Gap[]>;
  onSelect: (gap: Gap) => void;
}) {
  const otherGroups = [...gapsBySeverity.keys()].filter(
    (key) => !gapSeverityGroups.some((group) => group.id === key),
  );
  const groups = [
    ...gapSeverityGroups,
    ...otherGroups.map((id) => ({ id, title: id || "uncategorized" })),
  ];
  const total = [...gapsBySeverity.values()].reduce((sum, rows) => sum + rows.length, 0);

  if (!total) {
    return (
      <EmptyState
        icon={Wrench}
        title="No gaps in the ledger"
        description="Gaps and bugs (category=bug) recorded against this team's goals appear here, grouped by severity."
      />
    );
  }

  return (
    <div className="grid gap-4 lg:grid-cols-3">
      {groups.map((group) => {
        const rows = gapsBySeverity.get(group.id) ?? [];
        const openCount = rows.filter((gap) => !gapIsResolved(gap)).length;
        return (
          <Section
            key={group.id}
            title={group.title}
            action={
              <>
                {openCount > 0 && (
                  <Badge tone={gapSeverityTone(group.id)}>{openCount} open</Badge>
                )}
                <Badge tone="muted">{rows.length}</Badge>
              </>
            }
            className="rise"
          >
            {rows.length ? (
              <div className="divide-y divide-border/60">
                {rows.map((gap) => (
                  <GapRow key={gap.id} gap={gap} onSelect={() => onSelect(gap)} />
                ))}
              </div>
            ) : (
              <EmptyState title="None at this severity" />
            )}
          </Section>
        );
      })}
    </div>
  );
}

function GapRow({ gap, onSelect }: { gap: Gap; onSelect: () => void }) {
  const isBug = (gap.category ?? "").toLowerCase() === "bug";
  const resolved = gapIsResolved(gap);
  const Icon = isBug ? Bug : Wrench;
  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        "flex w-full flex-col items-stretch gap-2 px-4 py-3 text-left transition-colors hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        resolved && "opacity-60",
      )}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Icon
          className={cn(
            "size-4 shrink-0",
            toneText[gapSeverityTone(gap.severity)],
          )}
          aria-hidden
        />
        <Badge tone={gapSeverityTone(gap.severity)}>{gap.severity ?? "?"}</Badge>
        <Badge tone={gapStatusTone(gap.status)}>{gap.status ?? "open"}</Badge>
        {gap.category && <Badge tone="muted">{gap.category}</Badge>}
        {gap.owner_agent_id && (
          <span className="ml-auto text-[11px] text-muted-foreground">
            {memberShort(gap.owner_agent_id)}
          </span>
        )}
      </div>
      <p className="text-[13px] leading-relaxed text-foreground/90">
        {gap.summary ?? gap.id}
      </p>
      {gap.next_step && (
        <p className="text-xs text-muted-foreground">
          <span className="font-semibold uppercase tracking-wide text-[10px]">Next</span>{" "}
          {gap.next_step}
        </p>
      )}
      {(gap.repro_ref || gap.closing_test_ref) && (
        <div className="flex flex-wrap gap-1.5">
          {gap.repro_ref && (
            <Badge tone="muted">
              repro <MonoId>{gap.repro_ref}</MonoId>
            </Badge>
          )}
          {gap.closing_test_ref && (
            <Badge tone="muted">
              test <MonoId>{gap.closing_test_ref}</MonoId>
            </Badge>
          )}
        </div>
      )}
      {Boolean(gap.evidence_ids?.length) && (
        <div className="flex flex-wrap gap-1.5">
          {gap.evidence_ids!.map((id) => (
            <Badge key={id} tone="muted">
              <MonoId>{id}</MonoId>
            </Badge>
          ))}
        </div>
      )}
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Graph / Kanban                                                     */
/* ------------------------------------------------------------------ */

/** Product columns (archived hidden); legacy `complete` folds into `done`. */
const GOAL_COLUMNS = ["active", "blocked", "review", "done"] as const;
const TASK_COLUMNS = ["planned", "assigned", "running", "blocked", "review", "done"] as const;

function BoardColumn({
  title,
  tone,
  count,
  children,
}: {
  title: string;
  tone: StatusTone;
  count: number;
  children: ReactNode;
}) {
  return (
    <div className="flex w-72 shrink-0 flex-col rounded-lg border border-border bg-card/60">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <StatusDot tone={tone} />
        <span className="text-[12px] font-semibold capitalize">{title}</span>
        <span className="ml-auto font-mono text-[11px] text-muted-foreground">{count}</span>
      </div>
      <div className="min-h-16 space-y-1.5 p-2">{children}</div>
    </div>
  );
}

/**
 * Right-side Task slide-over (peek). Opened from the Work board by selecting a
 * card; reuses the full `TaskDocument` content (driven by `model.selectedTask`)
 * inside a narrow panel, with Close and "Open full page" affordances. Esc and
 * backdrop click close it. The full page stays reachable at `surface:"task"`.
 */
function TaskSheet({
  model,
  onSelectionChange,
  actionsEnabled,
  onAction,
  onClose,
}: SurfaceProps & { onClose: () => void }) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      <button
        type="button"
        aria-label="Close task panel"
        className="absolute inset-0 bg-foreground/20 backdrop-blur-[1px]"
        onClick={onClose}
      />
      <aside
        role="dialog"
        aria-label="Task detail"
        className="relative flex h-full w-full max-w-[660px] flex-col border-l border-border bg-background shadow-xl"
      >
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            Task
          </span>
          <div className="ml-auto flex items-center gap-1">
            <Button
              size="sm"
              variant="secondary"
              onClick={() => onSelectionChange({ surface: "task" })}
            >
              <ExternalLink className="size-3.5" />
              Open full page
            </Button>
            <button
              type="button"
              aria-label="Close"
              onClick={onClose}
              className="grid size-8 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              <X className="size-4" />
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4">
          <TaskDocument
            model={model}
            onSelectionChange={onSelectionChange}
            actionsEnabled={actionsEnabled}
            onAction={onAction}
          />
        </div>
      </aside>
    </div>
  );
}

/**
 * Unified Work board. A `[ Goals | Tasks ]` switch lays out either the Goal
 * collection (4 columns: active/blocked/review/done) or the Task graph (6
 * columns). Tasks mode supports a goal filter (`boardGoal`). Task cards carry a
 * derived ready/waiting chip distinct from the stored `blocked` column. The
 * per-goal board is just this board pre-filtered via `boardGoal`. Selecting a
 * card opens the Task slide-over (`peekTaskId`) without leaving the board.
 */
export function GraphKanban({
  model,
  onSelectionChange,
  boardScope = "tasks",
  boardGoal,
  peekTaskId,
  actionsEnabled,
  onAction,
}: SurfaceProps & {
  boardScope?: "goals" | "tasks";
  boardGoal?: string;
  peekTaskId?: string;
}) {
  const peekTask = peekTaskId
    ? model.tasks.find((task) => task.id === peekTaskId)
    : undefined;
  const goalsMode = boardScope === "goals";
  const goalById = new Map(model.goals.map((goal) => [goal.id, goal]));
  const filterGoal = boardGoal ? goalById.get(boardGoal) : undefined;
  const boardTasks = boardGoal
    ? model.tasks.filter((task) => task.goal_id === boardGoal)
    : model.tasks;

  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker={goalsMode ? "Goal collection" : "Task graph"}
        title="Work"
        description={
          goalsMode
            ? "Goals by lifecycle. A goal reaches done only after a closeout decision and evaluation — never from task activity alone."
            : "Tasks by status. The ready / waiting chip is derived from dependencies and is distinct from the blocked column."
        }
        actions={
          <div className="flex items-center gap-2">
            {!goalsMode && (
              <select
                aria-label="Filter tasks by goal"
                value={boardGoal ?? ""}
                onChange={(event) =>
                  onSelectionChange({ boardGoal: event.target.value || undefined })
                }
                className="h-8 max-w-44 truncate rounded-md border border-border bg-background/60 px-2 text-xs text-foreground outline-none transition-colors hover:border-input focus:border-ring"
              >
                <option value="">All goals</option>
                {model.goals.map((goal) => (
                  <option key={goal.id} value={goal.id}>
                    {goal.title ?? goal.id}
                  </option>
                ))}
              </select>
            )}
            <div className="flex items-center gap-1 rounded-md border border-border bg-card p-0.5">
              {(["goals", "tasks"] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => onSelectionChange({ boardScope: value })}
                  className={cn(
                    "rounded px-2.5 py-1 text-xs font-medium capitalize transition-colors",
                    boardScope === value
                      ? "bg-primary/15 text-primary"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {value}
                </button>
              ))}
            </div>
          </div>
        }
      />

      {filterGoal && (
        <div className="flex items-center gap-2 rounded-md border border-border bg-card/40 px-3 py-2 text-xs">
          <Target className="size-3.5 text-primary" />
          <span className="text-muted-foreground">Filtered to goal</span>
          <button
            type="button"
            className="font-medium hover:text-primary"
            onClick={() => onSelectionChange({ goalId: filterGoal.id, surface: "goal" })}
          >
            {filterGoal.title ?? filterGoal.id}
          </button>
          <button
            type="button"
            className="ml-auto inline-flex items-center gap-1 text-muted-foreground hover:text-foreground"
            onClick={() => onSelectionChange({ boardGoal: undefined })}
          >
            <X className="size-3" /> Clear
          </button>
        </div>
      )}

      <div className="flex gap-3 overflow-x-auto pb-2">
        {goalsMode
          ? GOAL_COLUMNS.map((status) => {
              const goals = model.goals.filter((goal) => displayGoalStatus(goal) === status);
              return (
                <BoardColumn key={status} title={status} tone={goalTone(status)} count={goals.length}>
                  {goals.length ? (
                    goals.map((goal) => (
                      <GoalCard
                        key={goal.id}
                        goal={goal}
                        model={model}
                        onSelect={() => onSelectionChange({ goalId: goal.id, surface: "goal" })}
                      />
                    ))
                  ) : (
                    <p className="px-1 py-3 text-center text-[11px] text-muted-foreground/60">None</p>
                  )}
                </BoardColumn>
              );
            })
          : TASK_COLUMNS.map((status) => {
              const tasks = boardTasks.filter((task) => task.status === status);
              return (
                <BoardColumn key={status} title={status} tone={taskTone(status)} count={tasks.length}>
                  {tasks.length ? (
                    tasks.map((task) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        readiness={readinessFor(task, model.taskGraph)}
                        goalLabel={
                          boardGoal ? undefined : goalById.get(task.goal_id ?? "")?.title
                        }
                        onClick={() => onSelectionChange({ taskId: task.id })}
                      />
                    ))
                  ) : (
                    <p className="px-1 py-3 text-center text-[11px] text-muted-foreground/60">None</p>
                  )}
                </BoardColumn>
              );
            })}
      </div>

      {peekTask && (
        <TaskSheet
          model={model}
          onSelectionChange={onSelectionChange}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onClose={() => onSelectionChange({ taskId: undefined })}
        />
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Agent detail (Notion document)                                     */
/* ------------------------------------------------------------------ */

/**
 * One-line summary of an agent's runtime health, for the properties table:
 * "delivery pass · process alive" style. Falls back to the coarse status when
 * no health object is present.
 */
function runtimeHealthSummary(member: AgentMember): string {
  const health = member.runtime_health;
  if (!health) return member.runtime_status ?? member.status ?? "unknown";
  const parts: string[] = [];
  parts.push(health.process_alive ? "process alive" : "process down");
  const delivery = (health.delivery_probe ?? "").trim();
  if (delivery) parts.push(`delivery ${delivery.toLowerCase().startsWith("pass") ? "pass" : delivery.toLowerCase().startsWith("fail") ? "fail" : "unknown"}`);
  return parts.join(" · ");
}

/**
 * The AGENT DETAIL page, rendered as a light Notion document (the same atoms as
 * the Goal/Task documents): identity header, a properties table, an assignable
 * current task, the conversation (real POST /v1/messages composer), and the
 * runtime data rendered as document sections rather than dense tiles.
 *
 * URL-addressable via `?agent=<id>`. Owns its own layout, so the global
 * Inspector is suppressed for the Agents area in WorkbenchShell.
 */
export function AgentDetail({
  model,
  onSelectionChange,
  actionsEnabled,
  onAction,
  apiUrl,
  agentTab,
}: SurfaceProps & { agentTab?: AgentTab }) {
  const member = model.selectedMember;
  if (!member) {
    return (
      <EmptyState
        icon={Bot}
        title="No agent selected"
        description="Pick an agent from the Agents list."
      />
    );
  }
  const status = member.runtime_status ?? member.status ?? "unknown";
  const currentTask = member.current_task_id
    ? model.tasks.find((task) => task.id === member.current_task_id)
    : undefined;
  const stats = model.statsByMember[member.id];
  const tab: AgentTab = agentTab ?? "conversation";

  // Full-height two-pane shell (Multica layout): a scrollable left config rail
  // and a chat-first right pane that fills the viewport so the composer pins to
  // the bottom. The Agents area already suppresses the global Inspector.
  return (
    <div className="flex h-full min-h-0">
      <ScrollArea className="hidden w-[300px] shrink-0 border-r border-border md:block">
        <AgentConfigRail
          member={member}
          status={status}
          stats={stats}
          model={model}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onSelectionChange={onSelectionChange}
        />
      </ScrollArea>
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Identity + back, shown only when the left config rail is hidden (<md). */}
        <div className="flex items-center gap-2 border-b border-border px-4 py-2 md:hidden">
          <button
            type="button"
            onClick={() => onSelectionChange({ surface: "agents", memberId: undefined })}
            className="inline-flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"
          >
            <Bot className="size-3.5" /> Agents
          </button>
          <Avatar name={member.name ?? member.id} tone={deliveryHealthTone(member)} />
          <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-foreground">
            {member.name ?? member.id}
          </span>
          <Badge tone={memberTone(status)}>{status}</Badge>
          <ProviderBadge provider={member.provider} />
        </div>
        <CurrentWorkBanner
          member={member}
          stats={stats}
          model={model}
          apiUrl={apiUrl}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onSelectionChange={onSelectionChange}
        />
        <Tabs
          value={tab}
          onValueChange={(value) => onSelectionChange({ agentTab: value as AgentTab })}
          className="flex min-h-0 flex-1 flex-col"
        >
          <div className="shrink-0 border-b border-border px-4 py-2">
            <TabsList>
              <TabsTrigger value="conversation">Conversation</TabsTrigger>
              <TabsTrigger value="tasks">Tasks</TabsTrigger>
              <TabsTrigger value="config">Config</TabsTrigger>
            </TabsList>
          </div>
          <TabsContent value="conversation" className="min-h-0 flex-1 overflow-hidden p-3">
            <ConversationStream
              model={model}
              member={member}
              actionsEnabled={actionsEnabled}
              onAction={onAction}
              apiUrl={apiUrl}
            />
          </TabsContent>
          <TabsContent value="tasks" className="min-h-0 flex-1 overflow-y-auto p-4">
            <AgentTasksTab
              model={model}
              member={member}
              currentTask={currentTask}
              stats={stats}
              actionsEnabled={actionsEnabled}
              onAction={onAction}
              onSelectionChange={onSelectionChange}
            />
          </TabsContent>
          <TabsContent value="config" className="min-h-0 flex-1 overflow-y-auto p-4">
            <AgentConfigTab model={model} member={member} />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
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

/**
 * Left config rail: identity header + properties + Workload/Last active/Sessions
 * stat rows + collapsible Runtime health and Skills. Lifts the old AgentDetail
 * header/properties verbatim; adds the stat rows from computeAgentStats.
 */
function AgentConfigRail({
  member,
  status,
  stats,
  model,
  actionsEnabled,
  onAction,
  onSelectionChange,
}: {
  member: AgentMember;
  status: string;
  stats?: AgentStats;
  model: WorkbenchModel;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const successPct =
    stats && stats.successRate != null ? `${Math.round(stats.successRate * 100)}% ok` : "—";
  return (
    <div className="space-y-4 p-4">
      <button
        type="button"
        onClick={() => onSelectionChange({ surface: "agents", memberId: undefined })}
        className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground transition-colors hover:text-foreground"
      >
        <Bot className="size-3.5" /> Agents
      </button>
      <div className="flex items-start gap-3">
        <Avatar name={member.name ?? member.id} tone={deliveryHealthTone(member)} size="lg" />
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-lg font-semibold tracking-tight text-foreground">
            {member.name ?? member.id}
          </h1>
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            <Badge tone={memberTone(status)}>{status}</Badge>
            <ProviderBadge provider={member.provider} />
          </div>
          <MonoId>{member.id}</MonoId>
        </div>
        <MemberOverflowActions
          member={member}
          sessions={model.sessionsByMember}
          inbox={model.inboxMessages}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
        />
      </div>

      <DocProperties
        items={[
          { label: "Provider", value: <ProviderBadge provider={member.provider} /> },
          { label: "Model", value: member.model ?? "—" },
          { label: "Status", value: status },
          { label: "Runtime health", value: runtimeHealthSummary(member) },
          {
            label: "Current task",
            value: member.current_task_id ? (
              <button
                type="button"
                onClick={() =>
                  onSelectionChange({ surface: "task", taskId: member.current_task_id ?? undefined })
                }
                className="text-left text-foreground hover:text-primary"
              >
                {taskTitle(model.tasks, member.current_task_id)}
              </button>
            ) : (
              "—"
            ),
          },
          {
            label: "Workload",
            value: `${member.queued_count ?? 0} queued · ${member.inbox_count ?? 0} in`,
          },
          { label: "Last active", value: relativeFromMs(stats?.lastActiveMs ?? null) },
          {
            label: "Sessions",
            value: stats ? `${stats.runsTotal} · ${successPct}` : "—",
          },
          { label: "Created", value: fmtTime(member.created_at) },
        ]}
      />

      <CollapsibleBlock label="Runtime health" defaultOpen>
        <RuntimeHealthPanel member={member} />
      </CollapsibleBlock>

      <CollapsibleBlock label={`Skills (${member.skill_refs?.length ?? 0})`}>
        {member.skill_refs?.length ? (
          <div className="flex flex-wrap gap-1.5">
            {member.skill_refs.map((skill) => (
              <Badge key={skill} tone="muted">{skill}</Badge>
            ))}
          </div>
        ) : (
          <p className="text-[12px] text-muted-foreground">No skills attached.</p>
        )}
      </CollapsibleBlock>
    </div>
  );
}

/**
 * Pane-chrome banner above the tabs: what the agent is doing RIGHT NOW, visible
 * from every tab. Three states — running (live elapsed + ▸raw turn), idle with
 * a queue (Deliver/wake), idle-empty (last active). All from existing data.
 */
function CurrentWorkBanner({
  member,
  stats,
  model,
  apiUrl,
  actionsEnabled,
  onAction,
  onSelectionChange,
}: {
  member: AgentMember;
  stats?: AgentStats;
  model: WorkbenchModel;
  apiUrl?: string;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const sessions = model.sessionsByMember;
  // Most-recent running session for this member = the live turn.
  const running = sessions
    .filter((s) => s.status === "running")
    .sort((a, b) => parseTs(b.started_at) - parseTs(a.started_at))[0];
  const queued = member.queued_count ?? 0;
  const live = Boolean(actionsEnabled);

  // A running session row whose process is no longer alive is stale, not live —
  // surface it as a warning (it likely crashed mid-turn) rather than a pulsing
  // RUNNING that never resolves.
  if (running && !member.runtime_alive) {
    const task = running.task_id ? taskTitle(model.tasks, running.task_id) : "a turn";
    return (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-status-warn/30 bg-status-warn/8 px-4 py-2 text-[12px]">
        <span className="inline-flex items-center gap-1.5 font-medium text-status-warn">
          <StatusDot tone="warn" /> Stale
        </span>
        <span className="min-w-0 truncate text-muted-foreground">
          {task} · session running but process not alive
        </span>
        <span className="ml-auto inline-flex items-center gap-2">
          <TurnDrillIn session={running} apiUrl={apiUrl} />
          <ActionButton
            enabled={live}
            size="sm"
            variant="secondary"
            onClick={() => dispatch(onAction, deliverQueued(member.id, { startRuntime: true }))}
          >
            <Send className="size-3.5" />
            Restart
          </ActionButton>
        </span>
      </div>
    );
  }

  if (running) {
    const task = running.task_id ? taskTitle(model.tasks, running.task_id) : "a turn";
    return (
      <div className="flex max-h-[55vh] flex-col gap-1 overflow-y-auto border-b border-status-running/30 bg-status-running/8 px-4 py-2 text-[12px]">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="inline-flex items-center gap-1.5 font-medium text-status-running">
            <StatusDot tone="running" pulse /> RUNNING
          </span>
          <button
            type="button"
            onClick={() => running.task_id && onSelectionChange({ surface: "task", taskId: running.task_id })}
            className="min-w-0 truncate text-foreground hover:text-primary"
          >
            {task}
          </button>
          <span className="text-muted-foreground">
            {running.provider ?? "provider"} · {formatDuration(running.started_at) ?? "0s"}
          </span>
        </div>
        {/* Auto-opened live TUI: watch the turn unfold (tool calls, results, output). */}
        <TurnDrillIn session={running} apiUrl={apiUrl} defaultOpen />
      </div>
    );
  }

  if (queued > 0) {
    return (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-status-warn/30 bg-status-warn/8 px-4 py-2 text-[12px]">
        <span className="inline-flex items-center gap-1.5 font-medium text-status-warn">
          <StatusDot tone="warn" /> Idle · {queued} queued
        </span>
        <span className="ml-auto">
          <ActionButton
            enabled={live}
            size="sm"
            variant="secondary"
            onClick={() => dispatch(onAction, deliverQueued(member.id, { startRuntime: true }))}
          >
            <Send className="size-3.5" />
            Deliver / wake
          </ActionButton>
        </span>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2 border-b border-border bg-card/40 px-4 py-2 text-[12px] text-muted-foreground">
      <StatusDot tone="idle" /> Idle
      <span className="ml-auto">last active {relativeFromMs(stats?.lastActiveMs ?? null)}</span>
    </div>
  );
}

/**
 * Tasks tab: the lightweight assign affordance (reused) + this agent's tasks
 * grouped by role — Executing (assignee), Reviewing (reviewer), Completed (30d).
 */
function AgentTasksTab({
  model,
  member,
  currentTask,
  stats,
  actionsEnabled,
  onAction,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  member: AgentMember;
  currentTask?: Task;
  stats?: AgentStats;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const isDone = (task: Task) => task.status === "done" || task.status === "archived";
  const executing = model.tasks.filter((t) => t.assignee_agent_id === member.id && !isDone(t));
  const reviewing = model.tasks.filter((t) => t.reviewer_agent_id === member.id && !isDone(t));
  const completed = model.tasks.filter(
    (t) => (t.assignee_agent_id === member.id || t.reviewer_agent_id === member.id) && isDone(t),
  );
  return (
    <div className="mx-auto max-w-[760px] space-y-5">
      <AgentCurrentTask
        model={model}
        member={member}
        currentTask={currentTask}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onSelectionChange={onSelectionChange}
      />
      {stats && (
        <p className="text-[12px] text-muted-foreground">
          {stats.runCount30d} runs · 30d ·{" "}
          {stats.successRate != null ? `${Math.round(stats.successRate * 100)}% ok` : "no terminal runs"}
          {stats.avgDurationMs != null && ` · avg ${Math.round(stats.avgDurationMs / 1000)}s`}
        </p>
      )}
      <AgentTaskGroup
        label="Executing"
        tasks={executing}
        empty="Not assigned to execute any task."
        currentTaskId={member.current_task_id}
        onSelect={(id) => onSelectionChange({ surface: "task", taskId: id })}
      />
      <AgentTaskGroup
        label="Reviewing"
        tasks={reviewing}
        empty="Not assigned to review any task."
        currentTaskId={member.current_task_id}
        onSelect={(id) => onSelectionChange({ surface: "task", taskId: id })}
      />
      <AgentTaskGroup
        label="Completed (30d)"
        tasks={completed}
        empty="No completed tasks."
        currentTaskId={member.current_task_id}
        onSelect={(id) => onSelectionChange({ surface: "task", taskId: id })}
      />
    </div>
  );
}

function AgentTaskGroup({
  label,
  tasks,
  empty,
  currentTaskId,
  onSelect,
}: {
  label: string;
  tasks: Task[];
  empty: string;
  currentTaskId?: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <DocSection label={`${label} (${tasks.length})`}>
      {tasks.length ? (
        <div className="space-y-2">
          {tasks.map((task) => (
            <button
              key={task.id}
              type="button"
              onClick={() => onSelect(task.id)}
              className="flex w-full items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-left transition-colors hover:border-input hover:bg-accent/40"
            >
              <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-foreground">
                {task.title ?? task.id}
              </span>
              {task.id === currentTaskId && <Badge tone="running">current</Badge>}
              <Badge tone={taskTone(task.status)}>{task.status}</Badge>
              {task.branch_ref && (
                <span className="hidden items-center gap-1 text-[11px] text-muted-foreground sm:inline-flex">
                  <GitBranch className="size-3" />
                  <MonoId>{shortBranch(task.branch_ref)}</MonoId>
                </span>
              )}
            </button>
          ))}
        </div>
      ) : (
        <p className="text-[12px] text-muted-foreground">{empty}</p>
      )}
    </DocSection>
  );
}

/**
 * Config tab: Multica's panes folded into collapsible blocks. 指令 + Skills +
 * Runtime are backed today; env / params / MCP read from provider_config (now in
 * the snapshot projection) and show "Not configured" when a field is unset.
 */
function AgentConfigTab({
  model,
  member,
}: {
  model: WorkbenchModel;
  member: AgentMember;
}) {
  const cfg: AgentProviderConfig = member.provider_config ?? {};
  const roots = cfg.runtime_workspace_roots ?? [];
  const mcpServers = cfg.mcp?.servers ?? [];
  const params: { label: string; value: ReactNode }[] = [
    { label: "Sandbox", value: cfg.sandbox_policy ?? "—" },
    { label: "Permission", value: cfg.permission_profile ?? "—" },
    { label: "Approval", value: cfg.approval_policy ?? "—" },
    { label: "Service tier", value: cfg.service_tier ?? "—" },
    { label: "Collaboration", value: cfg.collaboration_mode ?? "—" },
  ];
  const hasParams = params.some((p) => p.value !== "—");
  return (
    <div className="mx-auto max-w-[760px] space-y-3">
      <CollapsibleBlock label="指令 (Prompt)" defaultOpen>
        {member.prompt_ref ? (
          <MonoId>{member.prompt_ref}</MonoId>
        ) : (
          <p className="text-[12px] text-muted-foreground">No prompt reference.</p>
        )}
      </CollapsibleBlock>

      <CollapsibleBlock label={`Skills (${member.skill_refs?.length ?? 0})`}>
        {member.skill_refs?.length ? (
          <div className="flex flex-wrap gap-1.5">
            {member.skill_refs.map((skill) => (
              <Badge key={skill} tone="muted">{skill}</Badge>
            ))}
          </div>
        ) : (
          <p className="text-[12px] text-muted-foreground">No skills attached.</p>
        )}
      </CollapsibleBlock>

      <CollapsibleBlock label="Runtime">
        <AgentRuntimeSection model={model} member={member} />
      </CollapsibleBlock>

      <CollapsibleBlock label="环境变量 (Environment)">
        {cfg.environment_id || roots.length ? (
          <DocProperties
            items={[
              { label: "Environment", value: cfg.environment_id ?? "—" },
              {
                label: "Workspace roots",
                value: roots.length ? <PathList paths={roots} /> : "—",
              },
            ]}
          />
        ) : (
          <p className="text-[12px] text-muted-foreground">Not configured.</p>
        )}
      </CollapsibleBlock>

      <CollapsibleBlock label="自定义参数 (Parameters)">
        {hasParams ? (
          <DocProperties items={params} />
        ) : (
          <p className="text-[12px] text-muted-foreground">Not configured.</p>
        )}
      </CollapsibleBlock>

      <CollapsibleBlock label={`MCP (${mcpServers.length})`}>
        {mcpServers.length ? (
          <div className="space-y-2">
            {mcpServers.map((server) => (
              <div key={server.id} className="rounded-lg border border-border bg-card p-2.5 text-[12px]">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-foreground">{server.id}</span>
                  {server.transport && <Badge tone="muted">{server.transport}</Badge>}
                </div>
                {server.url && <MonoId>{server.url}</MonoId>}
                {server.command?.length ? <MonoId>{server.command.join(" ")}</MonoId> : null}
              </div>
            ))}
          </div>
        ) : (
          <p className="text-[12px] text-muted-foreground">Not configured.</p>
        )}
      </CollapsibleBlock>
    </div>
  );
}

/**
 * Current-task block on the agent detail: shows the task (when assigned) plus a
 * minimal Notion-style "assign" affordance — a picker of unassigned tasks wired
 * to POST /v1/tasks/{id}/assign. The heavy task management stays on the Work
 * board; this is just a lightweight assign from the agent's own page.
 */
function AgentCurrentTask({
  model,
  member,
  currentTask,
  actionsEnabled,
  onAction,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  member: AgentMember;
  currentTask?: Task;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  // Assignable = no assignee yet (assignment truth is assignee_agent_id), not
  // already done/archived.
  const assignable = model.tasks.filter(
    (task) =>
      !task.assignee_agent_id &&
      task.status !== "done" &&
      task.status !== "archived",
  );
  return (
    <div className="space-y-3">
      {currentTask ? (
        <button
          type="button"
          onClick={() => onSelectionChange({ surface: "task", taskId: currentTask.id })}
          className="block w-full rounded-lg border border-border bg-card p-3 text-left transition-colors hover:border-input hover:bg-accent/40"
        >
          <div className="flex items-start justify-between gap-2">
            <span className="line-clamp-2 text-[13px] font-medium leading-snug">
              {currentTask.title ?? currentTask.id}
            </span>
            <Badge tone={taskTone(currentTask.status)}>{currentTask.status}</Badge>
          </div>
          {currentTask.branch_ref && (
            <span className="mt-1.5 inline-flex items-center gap-1 text-[11px] text-muted-foreground">
              <GitBranch className="size-3" />
              <MonoId>{shortBranch(currentTask.branch_ref)}</MonoId>
            </span>
          )}
        </button>
      ) : (
        <p className="text-[13px] text-muted-foreground">No task assigned.</p>
      )}
      <AssignTaskControl
        agent={member}
        assignable={assignable}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
      />
    </div>
  );
}

/**
 * A minimal assign affordance: pick an unassigned task and assign it to this
 * agent (POST /v1/tasks/{id}/assign). Gated on `actionsEnabled`; disabled with
 * the standard tooltip offline or when there is nothing to assign.
 */
function AssignTaskControl({
  agent,
  assignable,
  actionsEnabled,
  onAction,
}: {
  agent: AgentMember;
  assignable: Task[];
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  const [taskId, setTaskId] = useState("");
  const live = Boolean(actionsEnabled);
  const canAssign = live && Boolean(taskId);
  function assign() {
    if (!canAssign) return;
    dispatch(onAction, assignTask(taskId, agent.id));
    setTaskId("");
  }
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Select
        aria-label="Pick an unassigned task"
        value={taskId}
        disabled={!live || assignable.length === 0}
        onChange={(event) => setTaskId(event.target.value)}
        className="h-9 max-w-xs flex-1"
      >
        <option value="">
          {assignable.length ? "Assign an unassigned task…" : "No unassigned tasks"}
        </option>
        {assignable.map((task) => (
          <option key={task.id} value={task.id}>
            {task.title ?? task.id}
          </option>
        ))}
      </Select>
      <ActionButton
        enabled={canAssign}
        size="sm"
        variant="secondary"
        onClick={assign}
      >
        <ClipboardList className="size-3.5" />
        Assign to {agent.name ?? agent.id}
      </ActionButton>
    </div>
  );
}

/**
 * Runtime data rendered Notion-style: the four-layer health rows, provider
 * sessions and provider-native child threads, as borderless document blocks
 * (DocSection rows) rather than the old dense StatusDot tile rail. The data is
 * unchanged — only the skin.
 */
function AgentRuntimeSection({ model, member }: { model: WorkbenchModel; member: AgentMember }) {
  const sessionCount = model.sessionsByMember.length;
  const threadCount = member.provider_child_thread_count ?? model.childThreadsByMember.length;
  return (
    <div className="space-y-5">
      <div className="rounded-lg border border-border bg-card">
        <RuntimeHealthPanel member={member} />
      </div>

      <div>
        <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          {sessionCount} provider {sessionCount === 1 ? "session" : "sessions"}
        </p>
        <div className="overflow-hidden rounded-lg border border-border bg-card">
          <SessionList sessions={model.sessionsByMember} />
        </div>
      </div>

      <div>
        <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          {threadCount} child {threadCount === 1 ? "thread" : "threads"}
        </p>
        <div className="overflow-hidden rounded-lg border border-border bg-card">
          <ChildThreadList threads={model.childThreadsByMember} parent={member} />
        </div>
      </div>
    </div>
  );
}

/**
 * LEFT pane: the conversation + action stream, grouped by provider session, with
 * a composer pinned at the bottom. Reuses the merged member timeline (re-skin +
 * regroup, no new data layer): rows nest under the session whose window they
 * fall in; session-less rows collect in a default time-ordered group at the
 * head. Operator↔agent messages render as chat bubbles; agent actions render as
 * inline cards.
 */
function ConversationStream({
  model,
  member,
  actionsEnabled,
  onAction,
  apiUrl,
}: {
  model: WorkbenchModel;
  member: AgentMember;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  apiUrl?: string;
}) {
  // A chat is ONLY the conversation — operator/agent message bubbles, oldest
  // first. Delivery lifecycle, provider sessions, evidence and agent events are
  // runtime plumbing (they render in the Runtime section), NOT chat turns; the
  // old merged-everything stream made one exchange read as 6-9 redundant rows.
  // The raw provider turn is reachable per agent reply via the bubble's
  // drill-in, so nothing is lost — it is just no longer dumped inline.
  const sessions = model.snapshot.provider_sessions ?? [];
  const chat = model.selectedMemberTimeline
    .filter((item) => item.kind === "message")
    .slice()
    // Numeric time order: createdAt is "unix-ms:<ms>" (or ISO); a string compare
    // would split the two formats into separate lexical ranges and misorder them.
    .sort((a, b) => parseTs(a.createdAt) - parseTs(b.createdAt));
  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border border-border bg-card">
      <header className="flex items-center justify-between gap-2 border-b border-border px-3.5 py-2.5">
        <span className="text-[11px] text-muted-foreground">
          Conversation · oldest first
        </span>
        <Badge tone="muted">{chat.length} messages</Badge>
      </header>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
        {chat.length ? (
          chat.map((item) => (
            <ChatBubble
              key={item.id}
              item={item}
              members={model.members}
              selfName={member.name ?? member.id}
              sessions={sessions}
              apiUrl={apiUrl}
            />
          ))
        ) : (
          <EmptyState icon={MessageSquare} title="No conversation yet for this member" />
        )}
      </div>

      <Composer
        member={member}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
      />
    </section>
  );
}

/**
 * A chat bubble for an operator↔agent message, attributed by AUTHOR identity
 * (Message.sender_kind), not raw inbox/outbox direction:
 *  - operator-authored messages (sender_kind="operator") sit on the RIGHT with
 *    an "Operator" badge — the human driving the team;
 *  - everything else is agent-authored: left-aligned, labelled with the author
 *    member's name.
 * The delivery status rides along as a small chip so delivery state stays
 * legible.
 */
function ChatBubble({
  item,
  members,
  selfName,
  sessions,
  apiUrl,
}: {
  item: TimelineItem;
  members: AgentMember[];
  selfName: string;
  sessions?: ProviderSession[];
  apiUrl?: string;
}) {
  // Operator messages are authored by the human, never a member. They are
  // outbound TO the member (direction "in" in the member timeline) AND carry
  // sender_kind="operator". An agent's own reply is authored by the member.
  const isOperator = item.senderKind === "operator";
  // Author label: "Operator" for operator rows; otherwise the authoring member
  // (the counterparty for inbound rows, the selected member for its own
  // outbound replies).
  const authorName = isOperator
    ? "Operator"
    : item.direction === "in"
      ? memberName(members, item.fromAgentId ?? item.counterpartyId)
      : selfName;
  // An agent reply that was produced by a provider turn can drill into the RAW
  // claude/codex events. The backend stamps the session ROW id 1:1 on the
  // message's delivery (item.providerSessionId), so the lookup is exact — no
  // time-window guessing. Operator messages have no turn.
  const session =
    !isOperator && item.providerSessionId
      ? (sessions ?? []).find((candidate) => candidate.id === item.providerSessionId)
      : undefined;
  return (
    <div className={cn("flex", isOperator ? "justify-end" : "justify-start")}>
      <div className={cn("max-w-[80%]", isOperator ? "items-end text-right" : "items-start")}>
        <div
          className={cn(
            "mb-0.5 flex items-center gap-1.5 text-[10px] text-muted-foreground",
            isOperator && "justify-end",
          )}
        >
          {isOperator ? (
            <Badge tone="decision" className="gap-0.5 px-1 py-0 uppercase tracking-wider">
              <User className="size-2.5" />
              Operator
            </Badge>
          ) : (
            <span className="font-semibold uppercase tracking-wider">{authorName}</span>
          )}
          {item.createdAt && <span>{fmtTime(item.createdAt)}</span>}
        </div>
        <div
          className={cn(
            "rounded-2xl border px-3 py-2 text-left text-[13px] leading-relaxed",
            isOperator
              ? "rounded-br-sm border-primary/30 bg-primary/12 text-foreground"
              : "rounded-bl-sm border-border bg-background text-foreground",
          )}
        >
          {item.body ? <Markdown source={item.body} /> : item.title}
        </div>
        <div className={cn("mt-1 flex flex-wrap items-center gap-1.5", isOperator && "justify-end")}>
          {item.deliveryStatus && (
            <Badge tone={deliveryStatusTone(item.deliveryStatus)}>{item.deliveryStatus}</Badge>
          )}
          {session && <TurnDrillIn session={session} apiUrl={apiUrl} />}
        </div>
      </div>
    </div>
  );
}

/** One raw provider event, as returned 1:1 by GET /v1/provider-sessions/{id}/events. */
interface RawTurnEvent {
  type?: string;
  subtype?: string;
  [key: string]: unknown;
}

/**
 * Per-reply drill-in to the RAW provider turn, rendered like the Claude Code TUI:
 * thinking → tool_use → tool_result → assistant text → result. For a RUNNING
 * session it LIVE-polls the events route every 1s so the operator watches the
 * turn unfold; the loop stops when the session reaches a terminal status (the
 * provider_session SSE status frame). `defaultOpen` auto-opens it (used live in
 * the current-work banner). Backend tees each event to the session NDJSON
 * mid-turn, so the growing file is what we read.
 */
function TurnDrillIn({
  session,
  apiUrl,
  defaultOpen = false,
}: {
  session: ProviderSession;
  apiUrl?: string;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const [events, setEvents] = useState<RawTurnEvent[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const running = session.status === "running";
  const duration = formatDuration(session.started_at, session.ended_at);

  useEffect(() => {
    if (!open || !apiUrl) return;
    let cancelled = false;
    const base = normalizeBaseUrl(apiUrl);
    const fetchEvents = async () => {
      if (inFlight.current) return;
      inFlight.current = true;
      try {
        const res = await fetch(
          `${base}/v1/provider-sessions/${encodeURIComponent(session.id)}/events`,
        );
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = (await res.json()) as { events?: RawTurnEvent[]; truncated?: boolean };
        if (!cancelled) {
          setEvents(data.events ?? []);
          setTruncated(Boolean(data.truncated));
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      } finally {
        inFlight.current = false;
      }
    };
    void fetchEvents();
    // Poll only while the turn is running; a terminal status stops the loop.
    if (!running) return () => { cancelled = true; };
    const id = window.setInterval(() => void fetchEvents(), 1000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [open, apiUrl, session.id, running]);

  return (
    <span className="inline-flex w-full min-w-0 flex-col">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="inline-flex items-center gap-1 self-start text-[10px] text-muted-foreground transition-colors hover:text-foreground"
      >
        {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
        {running ? (
          <>
            <StatusDot tone="running" pulse />
            <span className="font-medium text-status-running">LIVE</span>
          </>
        ) : (
          <Terminal className="size-3" />
        )}
        <span>{session.provider ?? "turn"}</span>
        {duration ? <span>· {duration}</span> : null}
        {events ? <span>· {events.length} events</span> : null}
        {!running && <span>· turn</span>}
      </button>
      {open && (
        <div className="mt-1 max-h-96 w-full overflow-y-auto rounded-md border border-border bg-muted/30 p-2 text-left">
          {error && <span className="text-[11px] text-status-bad">{error}</span>}
          {events === null ? (
            <span className="text-[11px] text-muted-foreground">loading…</span>
          ) : events.length === 0 ? (
            <span className="text-[11px] text-muted-foreground">
              {running ? "waiting for the agent…" : "no events recorded"}
            </span>
          ) : (
            <>
              <TurnTui events={events} />
              {truncated && (
                <p className="pt-1 text-[10px] text-muted-foreground">…older events truncated</p>
              )}
            </>
          )}
        </div>
      )}
    </span>
  );
}

/** message.content[] of a claude assistant/user event, as a block array. */
function turnBlocks(event: RawTurnEvent): Record<string, unknown>[] {
  const message = event.message as { content?: unknown } | undefined;
  return Array.isArray(message?.content) ? (message?.content as Record<string, unknown>[]) : [];
}

/** A tool_use input → a one-line arg (Bash command / file path / compact json). */
function toolUseArg(input: unknown): string {
  if (input && typeof input === "object") {
    const obj = input as Record<string, unknown>;
    if (typeof obj.command === "string") return obj.command;
    if (typeof obj.file_path === "string") return obj.file_path;
    if (typeof obj.path === "string") return obj.path;
    if (typeof obj.pattern === "string") return obj.pattern;
  }
  return compactJson(input);
}

/** A tool_result content (string, or array of {type:"text",text}) → text. */
function toolResultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) => (part && typeof part === "object" && typeof (part as Record<string, unknown>).text === "string"
        ? ((part as Record<string, unknown>).text as string)
        : compactJson(part)))
      .join("\n");
  }
  return compactJson(content);
}

/** One TUI line: optional glyph + label + body, with tone/indent variants. */
function TuiRow({
  glyph,
  label,
  body,
  tone,
  dim,
  muted,
  indent,
  mono,
}: {
  glyph?: string;
  label?: string;
  body?: string;
  tone?: StatusTone;
  dim?: boolean;
  muted?: boolean;
  indent?: boolean;
  mono?: boolean;
}) {
  return (
    <div className={cn("flex gap-1.5 py-0.5 text-[11px] leading-relaxed", indent && "pl-4")}>
      {glyph && <span className={cn("shrink-0", tone ? toneText[tone] : "text-muted-foreground")}>{glyph}</span>}
      {label && (
        <span className={cn("shrink-0 font-mono font-medium", tone ? toneText[tone] : dim ? "text-muted-foreground/70" : "text-foreground")}>
          {label}
        </span>
      )}
      {body && (
        <span
          className={cn(
            "min-w-0 whitespace-pre-wrap break-words",
            mono && "font-mono",
            muted || dim ? "text-muted-foreground" : "text-foreground/80",
          )}
        >
          {body}
        </span>
      )}
    </div>
  );
}

/** Result footer chip: subtype · duration · cost · tokens. */
function TurnResultFooter({ event }: { event: RawTurnEvent }) {
  const subtype = typeof event.subtype === "string" ? event.subtype : "done";
  const ms = typeof event.duration_ms === "number" ? event.duration_ms : undefined;
  const cost = typeof event.total_cost_usd === "number" ? event.total_cost_usd : undefined;
  const usage = event.usage as Record<string, unknown> | undefined;
  const inTok = usage && typeof usage.input_tokens === "number" ? usage.input_tokens : undefined;
  const outTok = usage && typeof usage.output_tokens === "number" ? usage.output_tokens : undefined;
  const parts = [
    ms != null ? `${(ms / 1000).toFixed(1)}s` : null,
    cost != null ? `$${cost.toFixed(4)}` : null,
    inTok != null || outTok != null ? `${inTok ?? "?"}→${outTok ?? "?"} tok` : null,
  ].filter(Boolean);
  return (
    <div className="mt-1 flex flex-wrap items-center gap-1.5 border-t border-border/40 pt-1 text-[10px] text-muted-foreground">
      <Badge tone={subtype === "success" ? "good" : "warn"}>result · {subtype}</Badge>
      {parts.length > 0 && <span>{parts.join(" · ")}</span>}
    </div>
  );
}

/**
 * Render a provider turn as a TUI: walk events in order, rendering thinking
 * badges, tool_use call cards (⏺), tool_result output (⎿, matched to its call),
 * assistant prose (markdown), and a result footer. Codex `item` events fall back
 * to a simple labelled row. Faithful to the real claude stream-json shapes.
 */
function TurnTui({ events }: { events: RawTurnEvent[] }) {
  const toolNames = new Map<string, string>();
  const rows: ReactNode[] = [];
  events.forEach((event, i) => {
    const type = typeof event.type === "string" ? event.type : "";
    const item = event.item as Record<string, unknown> | undefined;
    if (item && typeof item.type === "string") {
      const body =
        typeof item.text === "string"
          ? item.text
          : typeof item.command === "string"
            ? item.command
            : compactJson(item);
      rows.push(<TuiRow key={i} glyph="•" label={item.type as string} body={body} mono />);
      return;
    }
    switch (type) {
      case "system": {
        const bits = [
          typeof event.model === "string" ? `model ${event.model}` : "",
          typeof event.cwd === "string" ? `cwd ${event.cwd}` : "",
        ].filter(Boolean);
        rows.push(
          <TuiRow
            key={i}
            dim
            label={typeof event.subtype === "string" ? `system/${event.subtype}` : "system"}
            body={bits.join(" · ")}
          />,
        );
        break;
      }
      case "assistant": {
        turnBlocks(event).forEach((b, bi) => {
          const key = `${i}-${bi}`;
          if (b.type === "text" && typeof b.text === "string" && b.text.trim()) {
            rows.push(
              <div key={key} className="py-1 text-[12px] text-foreground/90">
                <Markdown source={b.text} />
              </div>,
            );
          } else if (b.type === "thinking") {
            const sig = typeof b.signature === "string" ? b.signature.length : 0;
            rows.push(
              <TuiRow key={key} glyph="✻" muted label="thinking" body={sig ? `(${sig}b · encrypted)` : "(encrypted)"} />,
            );
          } else if (b.type === "tool_use") {
            const name = typeof b.name === "string" ? b.name : "tool";
            if (typeof b.id === "string") toolNames.set(b.id, name);
            rows.push(<TuiRow key={key} glyph="⏺" tone="info" label={name} body={toolUseArg(b.input)} mono />);
          }
        });
        break;
      }
      case "user": {
        turnBlocks(event).forEach((b, bi) => {
          if (b.type !== "tool_result") return;
          const name = typeof b.tool_use_id === "string" ? toolNames.get(b.tool_use_id) : undefined;
          const text = toolResultText(b.content);
          rows.push(
            <TuiRow
              key={`${i}-${bi}`}
              glyph="⎿"
              indent
              tone={b.is_error === true ? "bad" : undefined}
              label={name}
              body={text.length > 600 ? `${text.slice(0, 600)}…` : text}
              mono
            />,
          );
        });
        break;
      }
      case "result":
        rows.push(<TurnResultFooter key={i} event={event} />);
        break;
      case "rate_limit_event":
        rows.push(<TuiRow key={i} muted label="rate_limit" body={compactJson(event.rate_limit_info)} />);
        break;
      default:
        rows.push(<TuiRow key={i} dim label={type || "event"} body={compactJson(event)} />);
    }
  });
  return <div className="space-y-0.5">{rows}</div>;
}

/** A short single-line JSON preview, capped so a big payload cannot flood. */
function compactJson(value: unknown): string {
  if (value == null) return "";
  try {
    const text = typeof value === "string" ? value : JSON.stringify(value);
    return text.length > 240 ? `${text.slice(0, 240)}…` : text;
  } catch {
    return String(value);
  }
}

/**
 * Composer pinned to the bottom of the stream. Authors a real message AS THE
 * OPERATOR (POST /v1/messages, from=OPERATOR_ID + sender_kind=operator, to =
 * member, kind = message) — it does NOT impersonate the Lead. The App refreshes
 * the snapshot after the action. Disabled with the standard tooltip while
 * actions are read-only.
 */
function Composer({
  member,
  actionsEnabled,
  onAction,
}: {
  member: AgentMember;
  actionsEnabled?: boolean;
  // Returns whether the action succeeded (App.runAction): the chat turn below
  // chains queue→deliver and must stop if the queue fails.
  onAction?: (path: string, body?: unknown) => void | Promise<boolean>;
}) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const canSend = Boolean(actionsEnabled && draft.trim() && !busy);

  // A single Send is a full chat turn: queue the operator message, THEN deliver
  // it (start the runtime if idle) so the agent actually runs and replies. Just
  // queuing left the operator staring at a message the agent never answered.
  async function send() {
    const content = draft.trim();
    if (!content || !actionsEnabled || !onAction || busy) return;
    setDraft("");
    setBusy(true);
    try {
      const message = operatorMessage({
        to: member.id,
        content,
        task: member.current_task_id ?? undefined,
      });
      // Stop on a failed queue so the follow-up deliver does not clobber the
      // error (App.runAction resets the error banner at the start of each call).
      // Restore the draft so the operator can retry without retyping.
      const queued = await onAction(message.path, message.body);
      if (queued === false) {
        setDraft(content);
        return;
      }
      const deliver = deliverQueued(member.id, { startRuntime: true });
      await onAction(deliver.path, deliver.body);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="shrink-0 border-t border-border bg-card/60 p-2.5">
      <div className="mb-1.5 flex items-center gap-1.5 text-[10px] text-muted-foreground">
        <Badge tone="decision" className="gap-0.5 px-1 py-0 uppercase tracking-wider">
          <User className="size-2.5" />
          Operator
        </Badge>
        <span>authoring as the operator (not the Lead)</span>
      </div>
      <div className="flex items-end gap-2">
        <textarea
          aria-label="Operator message to member"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              send();
            }
          }}
          rows={1}
          placeholder={
            actionsEnabled ? `Message ${member.name ?? member.id} as operator…` : ACTIONS_DISABLED_HINT
          }
          disabled={!actionsEnabled || busy}
          className="min-h-9 max-h-32 flex-1 resize-y rounded-md border border-border bg-background px-3 py-2 text-[13px] text-foreground outline-none transition-colors focus:border-ring disabled:cursor-not-allowed disabled:opacity-60"
        />
        {actionsEnabled ? (
          <Button size="sm" onClick={send} disabled={!canSend} className="shrink-0">
            <Send className="size-3.5" />
            {busy ? "Sending…" : "Send"}
          </Button>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex shrink-0">
                <Button size="sm" disabled title={ACTIONS_DISABLED_HINT}>
                  <Send className="size-3.5" />
                  Send
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent side="top">{ACTIONS_DISABLED_HINT}</TooltipContent>
          </Tooltip>
        )}
      </div>
    </div>
  );
}

/** Provider sessions under the member identity (id, status, thread/turn, source, evidence). */
function SessionList({ sessions }: { sessions: ProviderSession[] }) {
  if (!sessions.length) {
    return <EmptyState icon={Activity} title="No provider sessions" />;
  }
  return (
    <div className="space-y-2 p-3">
      {sessions.map((session) => (
        <div key={session.id} className="rounded-md border border-border bg-background/40 px-3 py-2">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate text-[13px] font-medium">
              {session.command ?? session.provider ?? "session"}
            </span>
            <Badge tone={timelineTone("session")}>{session.status ?? "unknown"}</Badge>
          </div>
          {session.prompt_summary && (
            <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
              {session.prompt_summary}
            </p>
          )}
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground">
            {session.provider_thread_id && (
              <span>thread <MonoId>{session.provider_thread_id}</MonoId></span>
            )}
            {session.provider_turn_id && (
              <span>turn <MonoId>{session.provider_turn_id}</MonoId></span>
            )}
            {session.terminal_source && <span>via {session.terminal_source}</span>}
            {session.evidence_ids?.length ? (
              <span>{session.evidence_ids.length} evidence</span>
            ) : null}
          </div>
        </div>
      ))}
    </div>
  );
}

/**
 * Provider-native child threads stay UNDER the parent member (doctrine: they
 * are not promoted to members). Renders agent path/nickname/role + status and
 * carries the provider_child_thread_count from the parent member card.
 */
function ChildThreadList({ threads, parent }: { threads: ProviderChildThread[]; parent: AgentMember }) {
  if (!threads.length) {
    return (
      <EmptyState
        icon={Bot}
        title="No child threads"
        description={
          parent.provider_child_thread_count
            ? `Parent reports ${parent.provider_child_thread_count} child thread(s) not yet in the snapshot.`
            : undefined
        }
      />
    );
  }
  return (
    <div className="space-y-2 p-3">
      {threads.map((thread) => (
        <div key={thread.id} className="rounded-md border border-border bg-background/40 px-3 py-2">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate text-[13px] font-medium">
              {thread.provider_agent_nickname ?? thread.provider_agent_path ?? thread.provider_thread_id ?? thread.id}
            </span>
            <Badge tone={timelineTone("session")}>{thread.status ?? "unknown"}</Badge>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground">
            {thread.provider_agent_role && <span>role {thread.provider_agent_role}</span>}
            {thread.provider_agent_path && (
              <span>path <MonoId>{thread.provider_agent_path}</MonoId></span>
            )}
            {thread.provider_thread_id && (
              <span>thread <MonoId>{thread.provider_thread_id}</MonoId></span>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

/**
 * Secondary/destructive member actions, wired to the real backend routes:
 * retry the most recent failed delivery, reconcile the latest running session,
 * and close the member. All gated on `actionsEnabled`.
 */
function MemberOverflowActions({
  member,
  sessions,
  inbox,
  actionsEnabled,
  onAction,
}: {
  member: AgentMember;
  sessions: ProviderSession[];
  inbox: Message[];
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  const failedMessage = inbox.find((m) => m.delivery_status === "failed");
  const activeSession = sessions.find((s) => s.status === "running") ?? sessions[0];
  const queuedCount =
    (member.queued_count ?? 0) ||
    inbox.filter((m) => m.delivery_status === "queued").length;
  return (
    <div className="flex items-center gap-2">
      <ActionButton
        enabled={Boolean(actionsEnabled && queuedCount)}
        size="sm"
        variant="default"
        onClick={() =>
          // start_runtime so deliver spins a runtime up when none is alive —
          // without it queued messages never leave Queued. The post returns the
          // refreshed snapshot, so the delivery_status chips (Queued →
          // Delivered/Acknowledged) flip live in the stream.
          dispatch(onAction, deliverQueued(member.id, { startRuntime: true }))
        }
      >
        <Inbox className="size-3.5" />
        Deliver{queuedCount ? ` (${queuedCount})` : ""}
      </ActionButton>
      <ActionButton
        enabled={Boolean(actionsEnabled && failedMessage)}
        size="sm"
        variant="secondary"
        onClick={() =>
          failedMessage &&
          dispatch(onAction, retryDelivery(member.id, { messageId: failedMessage.id }))
        }
      >
        <RefreshCw className="size-3.5" />
        Retry
      </ActionButton>
      <ActionButton
        enabled={Boolean(actionsEnabled && activeSession)}
        size="sm"
        variant="secondary"
        onClick={() =>
          activeSession &&
          dispatch(onAction, reconcileSession(member.id, { sessionId: activeSession.id }))
        }
      >
        <Wrench className="size-3.5" />
        Reconcile
      </ActionButton>
      <ActionButton
        enabled={actionsEnabled}
        size="sm"
        variant="ghost"
        onClick={() => dispatch(onAction, closeMember(member.id))}
      >
        <X className="size-3.5" />
        Close
      </ActionButton>
    </div>
  );
}

/**
 * The four-layer runtime health panel. Reads the real `member.runtime_health`
 * object emitted by the backend (process_alive / socket_exists /
 * protocol_probe / delivery_probe / checked_at) and renders one separated row
 * per layer.
 *
 * Doctrine (docs/agent-control-plane.md): the Dashboard must NOT present
 * process health as execution readiness when protocol or delivery health is
 * unknown. A null/unknown probe therefore renders amber "unknown", never green.
 */
function RuntimeHealthPanel({ member }: { member: AgentMember }) {
  const health: RuntimeHealth = member.runtime_health ?? {};
  return (
    <div className="space-y-2 p-3">
      <HealthRow
        label="Process"
        tone={health.process_alive ? "good" : "bad"}
        status={health.process_alive ? "running" : "not running"}
        detail={member.runtime_pid != null ? `pid ${member.runtime_pid}` : "no pid"}
      />
      <HealthRow
        label="Endpoint"
        tone={health.socket_exists ? "good" : "bad"}
        status={health.socket_exists ? "reachable" : "missing"}
        detail={member.control_endpoint ?? "no endpoint"}
      />
      <HealthRow label="Protocol" {...probeHealth(health.protocol_probe)} />
      <HealthRow label="Delivery" {...probeHealth(health.delivery_probe)} />
      <p className="pt-1 text-[11px] text-muted-foreground">
        {health.checked_at ? `Checked ${health.checked_at}` : "Never checked"}
      </p>
    </div>
  );
}

/**
 * Classify a probe string into a tone + status + detail. A `null`/missing probe
 * or the literal "unknown" is amber "unknown" (NOT green): execution readiness
 * is undetermined. Prefixes follow the backend probe vocabulary
 * (pass / pending / stale / failed / skipped).
 */
function probeHealth(probe?: string | null): {
  tone: StatusTone;
  status: string;
  detail: string;
} {
  if (probe == null || probe.trim() === "" || probe.toLowerCase() === "unknown") {
    return { tone: "warn", status: "unknown", detail: "not yet probed" };
  }
  const lower = probe.toLowerCase();
  if (lower.startsWith("pass")) return { tone: "good", status: "pass", detail: probe };
  if (lower.startsWith("fail")) return { tone: "bad", status: "fail", detail: probe };
  if (lower.startsWith("stale")) return { tone: "warn", status: "stale", detail: probe };
  if (lower.startsWith("pending")) return { tone: "warn", status: "pending", detail: probe };
  if (lower.startsWith("skipped")) return { tone: "idle", status: "skipped", detail: probe };
  // Any other non-empty value is an explicit report we cannot certify as healthy.
  return { tone: "warn", status: "unknown", detail: probe };
}

function HealthRow({
  label,
  tone,
  status,
  detail,
}: {
  label: string;
  tone: StatusTone;
  status: string;
  detail?: string;
}) {
  return (
    <div className="flex items-start gap-2 rounded-md border border-border bg-background/40 px-3 py-2">
      <StatusDot tone={tone} pulse={tone === "good"} className="mt-1" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs font-medium text-foreground">{label}</span>
          <span className={cn("text-[11px] font-medium", toneText[tone])}>
            {status}
          </span>
        </div>
        {detail && (
          <p className="truncate text-[11px] text-muted-foreground" title={detail}>
            {detail}
          </p>
        )}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Docs context                                                       */
/* ------------------------------------------------------------------ */

export function DocsContext({ model, apiUrl }: SurfaceProps) {
  const [docPath, setDocPath] = useState<string | null>(null);
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Mounted context"
        title="Docs context"
        description="Project docs linked to the active Vision, Goal, Task, Evidence and Decision objects."
      />
      <Section title="Mounted documents" className="rise">
        <div className="divide-y divide-border">
          {model.docs.map((doc) => (
            <button
              key={doc.path}
              type="button"
              onClick={() => setDocPath(doc.path)}
              className="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-accent/40"
            >
              <FileText className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-[13px] font-medium">{doc.title}</span>
                  <Badge tone="muted">{doc.lifecycle}</Badge>
                </div>
                <p className="text-xs text-muted-foreground">{doc.reason}</p>
                <MonoId>{doc.path}</MonoId>
              </div>
              <ExternalLink className="size-3.5 shrink-0 text-muted-foreground" />
            </button>
          ))}
        </div>
      </Section>

      {docPath && (
        <DocSheet apiUrl={apiUrl} path={docPath} onClose={() => setDocPath(null)} />
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Decision center                                                    */
/* ------------------------------------------------------------------ */

export function DecisionCenter({ model, onSelectionChange }: SurfaceProps) {
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Acceptance"
        title="Decision center"
        description="Evidence, proposals, reviews and Leader decisions waiting on operator action."
      />
      <Section
        title="Decision queue"
        action={<Badge tone={model.decisionQueue.length ? "decision" : "good"}>{model.decisionQueue.length}</Badge>}
        className="rise"
      >
        <QueueList
          items={model.decisionQueue}
          empty="No pending decisions"
          onSelect={(ref) => ref && onSelectionChange({ taskId: ref, surface: "task" })}
        />
      </Section>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Warnings & repair                                                  */
/* ------------------------------------------------------------------ */

export function WarningsRepair({ model, onSelectionChange }: SurfaceProps) {
  const groups: { id: WorkflowWarning["severity"]; title: string }[] = [
    { id: "high", title: "High" },
    { id: "medium", title: "Medium" },
    { id: "low", title: "Low" },
  ];
  const openGapCount = model.gaps.filter((gap) => !gapIsResolved(gap)).length;
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Repair"
        title="Warnings"
        description="Broken workflow invariants grouped by severity, the Gap/bug ledger, and the decision queue waiting on operator action. Each row links to the object it affects."
        actions={
          <>
            <Badge tone={model.warnings.length ? "bad" : "good"}>
              {model.warnings.length} warnings
            </Badge>
            <Badge tone={openGapCount ? "warn" : "good"}>{openGapCount} open gaps</Badge>
            <Badge tone={model.decisionQueue.length ? "decision" : "good"}>
              {model.decisionQueue.length} decisions
            </Badge>
          </>
        }
      />
      <div className="grid gap-4 lg:grid-cols-3">
        {groups.map((group) => {
          const items = model.warnings.filter((warning) => warning.severity === group.id);
          return (
            <Section
              key={group.id}
              title={group.title}
              action={<Badge tone={severityTone(group.id)}>{items.length}</Badge>}
              className="rise"
            >
              <WarningList
                warnings={items}
                onSelect={(warning) =>
                  onSelectionChange(
                    warning.taskId
                      ? { taskId: warning.taskId, surface: "task" }
                      : { surface: "warnings" },
                  )
                }
              />
            </Section>
          );
        })}
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-2 px-0.5">
          <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            Gap · bug ledger
          </p>
          <Badge tone={openGapCount ? "warn" : "good"}>
            {openGapCount} open / {model.gaps.length} total
          </Badge>
        </div>
        <GapLedger
          gapsBySeverity={model.gapsBySeverity}
          onSelect={(gap) =>
            onSelectionChange(
              gap.task_id
                ? { taskId: gap.task_id, surface: "task" }
                : gap.goal_id
                  ? { goalId: gap.goal_id, surface: "goal" }
                  : { surface: "warnings" },
            )
          }
        />
      </div>

      <Section
        kicker="Reviews · waivers · missing proof"
        title="Decision queue"
        action={
          <Badge tone={model.decisionQueue.length ? "decision" : "good"}>
            {model.decisionQueue.length}
          </Badge>
        }
        className="rise"
      >
        {model.leadMemberId && model.leadDecisionQueue.length > 0 && (
          <div className="border-b border-border bg-card/40">
            <div className="flex items-center gap-1.5 px-3.5 pt-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              <Crown className="size-3 text-primary" />
              Awaiting Lead decision
              <span className="ml-auto font-mono normal-case text-muted-foreground/70">
                {memberName(model.members, model.leadMemberId)}
              </span>
            </div>
            <QueueList
              items={model.leadDecisionQueue}
              empty="Nothing awaiting the Lead"
              onSelect={(ref) => ref && onSelectionChange({ taskId: ref, surface: "task" })}
            />
          </div>
        )}
        <QueueList
          items={model.decisionQueue}
          empty="No pending decisions"
          onSelect={(ref) => ref && onSelectionChange({ taskId: ref, surface: "task" })}
        />
      </Section>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Debug surface                                                      */
/* ------------------------------------------------------------------ */

export function DebugSurface({
  model,
  sourceLabel,
}: {
  model: WorkbenchModel;
  sourceLabel: string;
}) {
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Audit / debug"
        title="Raw snapshot"
        description="Canonical snapshot behind every derived view. Hidden from the operating surfaces by default."
        actions={<Badge tone="muted">{sourceLabel}</Badge>}
      />
      <Section title="snapshot.json" kicker="read-only" className="rise">
        <pre className="max-h-[34rem] overflow-auto p-4 font-mono text-[11px] leading-relaxed text-muted-foreground">
          {JSON.stringify(model.snapshot, null, 2)}
        </pre>
      </Section>
    </div>
  );
}
