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
  Plus,
  RefreshCw,
  Scale,
  Send,
  ShieldAlert,
  ShieldCheck,
  Target,
  Terminal,
  Users,
  User,
  UserPlus,
  Workflow,
  Wrench,
  X,
  Zap,
} from "lucide-react";

import { useEffect, useState, type ComponentProps, type ReactNode } from "react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  DocProperties,
  DocSection,
  DocumentSurface,
  EmptyState,
  MetaList,
  MonoId,
  Section,
  StatusDot,
  SurfaceHeader,
  TimelineRow,
  toneText,
  type StatusTone,
} from "@/components/workbench/atoms";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import { fetchDoc } from "../api";
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
  groupMemberTimelineBySession,
  memberName,
  taskTitle,
  tasksBlockedBy,
  taskGitMetadata,
  type MemberSessionGroup,
  type TimelineItem,
  type WorkbenchModel,
} from "../model/readModel";
import {
  closeMember,
  createAgent,
  createGoal,
  createTeam,
  deliverQueued,
  operatorMessage,
  reconcileSession,
  requestReview,
  retryDelivery,
  type ActionDescriptor,
} from "../api/actions";
import type {
  AgentMember,
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
import type { SelectionState } from "../app/selection";

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

function LaneStack({
  model,
  onSelect,
}: {
  model: WorkbenchModel;
  onSelect: (task: Task) => void;
}) {
  const lanes = model.lanes.filter((lane) => lane.tasks.length);
  if (!lanes.length) {
    return (
      <EmptyState
        icon={ClipboardList}
        title="No tasks in scope"
        description="Tasks for the active goal will appear here."
      />
    );
  }
  return (
    <div className="space-y-3 p-3">
      {lanes.map((lane) => (
        <div key={lane.id}>
          <div className="mb-1.5 flex items-center gap-2">
            <StatusDot tone={taskTone(lane.id)} />
            <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
              {lane.title}
            </span>
            <span className="font-mono text-[11px] text-muted-foreground/60">
              {lane.tasks.length}
            </span>
          </div>
          <div className="space-y-1.5">
            {lane.tasks.map((task) => (
              <TaskCard key={task.id} task={task} onClick={() => onSelect(task)} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
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
/* Team workspace (flagship)                                          */
/* ------------------------------------------------------------------ */

export function TeamWorkspace({ model, onSelectionChange, actionsEnabled, onAction }: SurfaceProps) {
  const team = model.selectedTeam;
  const goal = model.selectedGoal;
  const member = model.selectedMember;
  // Lead band: the team Lead is the team owner (authoritative). Tie the active
  // goal's owner to the Lead so it is visible whether goal ownership and team
  // ownership are the same agent or have diverged.
  const leadId = model.leadMemberId;
  const goalOwnerIsLead = Boolean(goal?.owner_agent_id && goal.owner_agent_id === leadId);
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Persistent AgentTeam"
        title={team?.name ?? "No active team"}
        description={
          team?.description ??
          "Standing members, current work, messages, decisions and warnings in one operating surface."
        }
        actions={
          <>
            <OperatorBar model={model} actionsEnabled={actionsEnabled} onAction={onAction} />
            <ActionButton
              enabled={actionsEnabled && Boolean(model.selectedTask)}
              variant="secondary"
              size="sm"
              onClick={() =>
                model.selectedTask &&
                dispatch(onAction, requestReview(model.selectedTask.id))
              }
            >
              <ShieldCheck className="size-3.5" />
              Request review
            </ActionButton>
            <Button
              variant="secondary"
              size="sm"
              disabled={!member}
              onClick={() =>
                member && onSelectionChange({ memberId: member.id, surface: "member" })
              }
            >
              <Send className="size-3.5" />
              Open conversation
            </Button>
          </>
        }
      />

      <div className="rounded-lg border border-border bg-card">
        <div className="flex flex-wrap items-center justify-between gap-4 p-4">
          <div className="min-w-0">
            <div className="flex items-center gap-1.5 text-[11px] uppercase tracking-wider text-muted-foreground">
              <Target className="size-3.5 text-primary" /> Active Vision / Goal
            </div>
            <p className="mt-1 truncate text-[15px] font-semibold">
              {goal?.title ?? "Missing active goal"}
            </p>
            <p className="mt-0.5 line-clamp-1 text-sm text-muted-foreground">
              {goal?.objective}
            </p>
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              {leadId && (
                <button
                  type="button"
                  onClick={() => onSelectionChange({ memberId: leadId, surface: "member" })}
                  className="inline-flex"
                >
                  <Badge tone="decision" className="gap-1">
                    <Crown className="size-3" />
                    Lead {memberName(model.members, leadId)}
                  </Badge>
                </button>
              )}
              {goal?.owner_agent_id && (
                <Badge tone={goalOwnerIsLead ? "good" : "warn"} className="gap-1">
                  Goal owner {memberName(model.members, goal.owner_agent_id)}
                  {goalOwnerIsLead ? " · same as Lead" : " · differs from Lead"}
                </Badge>
              )}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <ProofStat label="Tasks" value={model.goalTasks.length} tone="info" />
            <ProofStat
              label="Warnings"
              value={model.warnings.length}
              tone={model.warnings.length ? "warn" : "good"}
            />
            <ProofStat
              label="Decisions"
              value={model.decisionQueue.length}
              tone={model.decisionQueue.length ? "decision" : "good"}
            />
          </div>
        </div>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.35fr_1fr]">
        <Section
          kicker="Messages · evidence · decisions"
          title="Canonical Activity"
          action={<Badge tone="muted">{model.activity.length}</Badge>}
          className="rise"
        >
          <div className="max-h-[28rem] overflow-y-auto">
            {model.activity.length ? (
              model.activity.map((item) => (
                <TimelineRow
                  key={item.id}
                  kind={item.kind}
                  title={item.title}
                  meta={item.meta}
                  body={item.body}
                  tone={timelineTone(item.kind, item.severity)}
                  onClick={() =>
                    item.objectRef &&
                    onSelectionChange({ taskId: item.objectRef, surface: "task" })
                  }
                />
              ))
            ) : (
              <EmptyState
                icon={Activity}
                title="No activity yet"
                description="Messages, proposals and decisions will stream here."
              />
            )}
          </div>
        </Section>

        <Section
          kicker="What can move now"
          title="Current Work Pressure"
          className="rise"
        >
          <LaneStack
            model={model}
            onSelect={(task) =>
              onSelectionChange({ taskId: task.id, surface: "task" })
            }
          />
        </Section>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Section
          kicker="Reviews · waivers · missing proof"
          title="Decision Queue"
          className="rise"
        >
          {leadId && model.leadDecisionQueue.length > 0 && (
            <div className="border-b border-border bg-card/40">
              <div className="flex items-center gap-1.5 px-3.5 pt-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                <Crown className="size-3 text-primary" />
                Awaiting Lead decision
                <span className="ml-auto font-mono normal-case text-muted-foreground/70">
                  {memberName(model.members, leadId)}
                </span>
              </div>
              <QueueList
                items={model.leadDecisionQueue}
                empty="Nothing awaiting the Lead"
                onSelect={(ref) =>
                  ref && onSelectionChange({ taskId: ref, surface: "task" })
                }
              />
            </div>
          )}
          <QueueList
            items={model.decisionQueue}
            empty="No pending decisions"
            onSelect={(ref) =>
              ref && onSelectionChange({ taskId: ref, surface: "task" })
            }
          />
        </Section>
        <Section
          kicker="Broken workflow invariants"
          title="Warnings"
          action={
            <Badge tone={model.warnings.length ? "bad" : "good"}>
              {model.warnings.length}
            </Badge>
          }
          className="rise"
        >
          <WarningList
            warnings={model.warnings.slice(0, 6)}
            onSelect={(warning) =>
              onSelectionChange(
                warning.taskId
                  ? { taskId: warning.taskId, surface: "warnings" }
                  : { surface: "warnings" },
              )
            }
          />
        </Section>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Operator forms (WP-iii): drive the team with ZERO CLI               */
/* ------------------------------------------------------------------ */

/**
 * The operator action bar in the Team workspace header: New team, New agent,
 * Brief the Lead. Each opens a dialog wired to the matching WP-ii create route
 * through the actions seam. Every write is gated on `actionsEnabled` (live);
 * offline the buttons render disabled with the standard tooltip.
 */
function OperatorBar({
  model,
  actionsEnabled,
  onAction,
}: {
  model: WorkbenchModel;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  const [dialog, setDialog] = useState<null | "team" | "agent" | "goal">(null);
  const live = Boolean(actionsEnabled);
  return (
    <>
      <OperatorActionButton enabled={live} onClick={() => setDialog("team")}>
        <Plus className="size-3.5" />
        New team
      </OperatorActionButton>
      <OperatorActionButton
        enabled={live}
        variant="secondary"
        onClick={() => setDialog("agent")}
      >
        <UserPlus className="size-3.5" />
        New agent
      </OperatorActionButton>
      <OperatorActionButton
        enabled={live}
        variant="secondary"
        onClick={() => setDialog("goal")}
      >
        <Target className="size-3.5" />
        Brief the Lead
      </OperatorActionButton>

      <NewTeamForm
        open={dialog === "team"}
        model={model}
        actionsEnabled={live}
        onAction={onAction}
        onClose={() => setDialog(null)}
      />
      <NewAgentForm
        open={dialog === "agent"}
        model={model}
        actionsEnabled={live}
        onAction={onAction}
        onClose={() => setDialog(null)}
      />
      <BriefLeadForm
        open={dialog === "goal"}
        model={model}
        actionsEnabled={live}
        onAction={onAction}
        onClose={() => setDialog(null)}
      />
    </>
  );
}

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

/** A member <select> for picking an owner/Lead, listing every known member. */
function MemberSelect({
  id,
  value,
  members,
  onChange,
  placeholder = "Select a member…",
}: {
  id: string;
  value: string;
  members: AgentMember[];
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  return (
    <Select id={id} value={value} onChange={(event) => onChange(event.target.value)}>
      <option value="">{placeholder}</option>
      {members.map((member) => (
        <option key={member.id} value={member.id}>
          {member.name ?? member.id}
          {member.role ? ` · ${member.role}` : ""}
        </option>
      ))}
    </Select>
  );
}

/**
 * NEW TEAM (POST /v1/teams). Requires name, description and an owner (the
 * Lead/owner agent). On submit the team is created; it appears via the next
 * snapshot refresh / SSE frame.
 */
function NewTeamForm({
  open,
  model,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  model: WorkbenchModel;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [owner, setOwner] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setDescription("");
      setOwner(model.leadMemberId ?? model.selectedTeam?.owner_agent_id ?? "");
    }
  }, [open, model.leadMemberId, model.selectedTeam?.owner_agent_id]);

  const canSubmit = Boolean(name.trim() && description.trim() && owner.trim());
  function submit() {
    if (!canSubmit || !actionsEnabled) return;
    dispatch(
      onAction,
      createTeam({
        name: name.trim(),
        description: description.trim(),
        owner: owner.trim(),
      }),
    );
    onClose();
  }

  return (
    <Dialog
      open={open}
      title="New team"
      description="Stand up a persistent AgentTeam. POST /v1/teams."
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
              placeholder="e.g. Polymarket HFT"
            />
          )}
        </Field>
        <Field label="Description" required>
          {(id) => (
            <TextArea
              id={id}
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="What this team owns."
            />
          )}
        </Field>
        <Field label="Owner (Lead)" required hint="The team Lead / owner agent.">
          {(id) => (
            <MemberSelect
              id={id}
              value={owner}
              members={model.members}
              onChange={setOwner}
              placeholder="Select the Lead…"
            />
          )}
        </Field>
        <DialogFooter
          submitLabel="Create team"
          actionsEnabled={actionsEnabled}
          canSubmit={canSubmit}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

/**
 * NEW AGENT (POST /v1/agents). Requires name + role; provider (codex|claude),
 * description and skills are optional. The new member joins the selected team
 * and appears in the roster on the next snapshot.
 */
function NewAgentForm({
  open,
  model,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  model: WorkbenchModel;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [role, setRole] = useState("");
  const [provider, setProvider] = useState("");
  const [description, setDescription] = useState("");
  const [skills, setSkills] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setRole("");
      setProvider("");
      setDescription("");
      setSkills("");
    }
  }, [open]);

  const teamId = model.selectedTeam?.id;
  const teamName = model.selectedTeam?.name ?? teamId;
  const canSubmit = Boolean(name.trim() && role.trim());
  function submit() {
    if (!canSubmit || !actionsEnabled) return;
    dispatch(
      onAction,
      createAgent({
        name: name.trim(),
        role: role.trim(),
        provider: provider || undefined,
        description: description.trim() || undefined,
        skills: parseList(skills),
        teamIds: teamId ? [teamId] : undefined,
      }),
    );
    onClose();
  }

  return (
    <Dialog
      open={open}
      title="New agent"
      description={
        teamName ? `Add an Agent Member to ${teamName}. POST /v1/agents.` : "Add an Agent Member. POST /v1/agents."
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

/**
 * BRIEF THE LEAD / SET GOAL. Creates a Goal owned by the Lead (POST /v1/goals)
 * AND — when "also message the Lead" is on — emits an operator Message
 * (kind=task, sender_kind=operator, from=operator, to=Lead) so the objective
 * shows BOTH as durable Goal state and in the Lead's conversation.
 */
function BriefLeadForm({
  open,
  model,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  model: WorkbenchModel;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onClose: () => void;
}) {
  const [title, setTitle] = useState("");
  const [objective, setObjective] = useState("");
  const [owner, setOwner] = useState("");
  const [success, setSuccess] = useState("");
  const [alsoMessage, setAlsoMessage] = useState(true);

  useEffect(() => {
    if (open) {
      setTitle("");
      setObjective("");
      setOwner(model.leadMemberId ?? model.selectedTeam?.owner_agent_id ?? "");
      setSuccess("");
      setAlsoMessage(true);
    }
  }, [open, model.leadMemberId, model.selectedTeam?.owner_agent_id]);

  const canSubmit = Boolean(title.trim() && objective.trim() && owner.trim());
  function submit() {
    if (!canSubmit || !actionsEnabled) return;
    const trimmedTitle = title.trim();
    const trimmedObjective = objective.trim();
    // 1) Durable Goal state, owned by the Lead.
    dispatch(
      onAction,
      createGoal({
        title: trimmedTitle,
        objective: trimmedObjective,
        owner: owner.trim(),
        success: parseList(success),
      }),
    );
    // 2) Optional operator brief into the Lead's conversation (kind=task).
    if (alsoMessage) {
      dispatch(
        onAction,
        operatorMessage({
          to: owner.trim(),
          kind: "task",
          content: `Goal: ${trimmedTitle}\n\n${trimmedObjective}`,
        }),
      );
    }
    onClose();
  }

  return (
    <Dialog
      open={open}
      title="Brief the Lead"
      description="Set a Goal for the Lead. POST /v1/goals (+ optional operator message)."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Goal title" required>
          {(id) => (
            <TextInput
              id={id}
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="e.g. Ship the operator console"
            />
          )}
        </Field>
        <Field label="Objective" required>
          {(id) => (
            <TextArea
              id={id}
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              placeholder="What success looks like, in prose."
            />
          )}
        </Field>
        <Field label="Lead (owner)" required hint="The Lead who owns this goal.">
          {(id) => (
            <MemberSelect
              id={id}
              value={owner}
              members={model.members}
              onChange={setOwner}
              placeholder="Select the Lead…"
            />
          )}
        </Field>
        <Field label="Success criteria" hint="One per line or comma separated (optional).">
          {(id) => (
            <TextArea
              id={id}
              value={success}
              onChange={(event) => setSuccess(event.target.value)}
              placeholder={"e.g. gate green\noperator can drive with zero CLI"}
            />
          )}
        </Field>
        <label className="flex items-center gap-2 text-[12px] text-foreground">
          <input
            type="checkbox"
            checked={alsoMessage}
            onChange={(event) => setAlsoMessage(event.target.checked)}
            className="size-3.5 rounded border-border accent-primary"
          />
          Also message the Lead with this brief (operator → Lead)
        </label>
        <DialogFooter
          submitLabel="Brief the Lead"
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
                <span>
                  {memberName(model.members, task.assignee_agent_id)}
                  <span className="ml-1 text-[10px] text-muted-foreground">(projection)</span>
                </span>
              ),
            },
            { label: "Reviewer", value: memberName(model.members, task.reviewer_agent_id) },
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
/* Member workbench                                                   */
/* ------------------------------------------------------------------ */

/**
 * Role-grouped member picker. The team rail is hidden below `lg`, so the Member
 * surface ships its own picker to keep member selection working at all widths.
 */
function MemberPicker({
  model,
  onSelectionChange,
}: SurfaceProps) {
  const activeId = model.selectedMember?.id;
  if (!model.members.length) {
    return <EmptyState icon={Bot} title="No members in this team" />;
  }
  return (
    <Section kicker="Pick a member" title="Team members" className="rise">
      <div className="space-y-4 p-3">
        {model.roleGroups.map((group) => (
          <div key={group.role}>
            <p className="px-1 pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {group.role}
            </p>
            <div className="grid gap-1.5 sm:grid-cols-2">
              {group.members.map((m) => {
                const active = activeId === m.id;
                const isLead = m.id === model.leadMemberId;
                const queue = (m.inbox_count ?? 0) + (m.queued_count ?? 0);
                return (
                  <button
                    key={m.id}
                    type="button"
                    onClick={() =>
                      onSelectionChange({
                        memberId: m.id,
                        taskId: m.current_task_id ?? undefined,
                        surface: "member",
                      })
                    }
                    className={cn(
                      "flex w-full items-center gap-2.5 rounded-md border border-transparent px-2 py-1.5 text-left transition-colors hover:bg-accent/50",
                      active && "border-border bg-accent/60",
                    )}
                  >
                    <Avatar
                      name={m.name ?? m.id}
                      tone={memberTone(m.runtime_status ?? m.status)}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1.5">
                        <span className="truncate text-[13px] font-medium">
                          {m.name ?? m.id}
                        </span>
                        {isLead && (
                          <Badge tone="decision" className="shrink-0 gap-0.5 px-1 py-0">
                            <Crown className="size-2.5" />
                            Lead / Owner
                          </Badge>
                        )}
                      </span>
                      <span className="block truncate text-[11px] text-muted-foreground">
                        {m.runtime_status ?? m.status ?? "unknown"}
                        <span className="mx-1 text-border">·</span>
                        {taskTitle(model.tasks, m.current_task_id)}
                      </span>
                    </span>
                    {queue > 0 && (
                      <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                        {queue}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </Section>
  );
}

/**
 * AgentMember surface, redesigned as a Claude/Codex DESKTOP-APP two-pane layout:
 *  - LEFT (flex-1): the conversation + action stream, grouped by provider
 *    session, with a composer pinned at the bottom.
 *  - RIGHT (member-owned rail, ~340px): current task, inbox/outbox tiles,
 *    runtime health + sessions + child threads, and identity/policy.
 *  - HEADER band: delivery-toned avatar, name, role + provider (neutral) badges,
 *    status, and gated overflow actions.
 *
 * This member view OWNS its right rail; the global Inspector is suppressed for
 * the member surface in WorkbenchShell so there is no duplicate rail.
 */
export function MemberWorkbench({ model, onSelectionChange, actionsEnabled, onAction }: SurfaceProps) {
  const member = model.selectedMember;
  if (!member) {
    return (
      <div className="space-y-5">
        <SurfaceHeader
          kicker="AgentMember workbench"
          title="Select a member"
          description="Pick a durable AgentMember to open its conversation, runtime and current work."
        />
        <MemberPicker model={model} onSelectionChange={onSelectionChange} />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <MemberHeaderBand
        model={model}
        member={member}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
      />

      {/* Picker stays available so members can be switched without the lg-only rail. */}
      <div className="lg:hidden">
        <MemberPicker model={model} onSelectionChange={onSelectionChange} />
      </div>

      {/* Two-pane desktop-app body: conversation+action stream | member rail. */}
      <div className="grid min-h-0 gap-4 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <ConversationStream
          model={model}
          member={member}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onSelectionChange={onSelectionChange}
        />
        <MemberRail model={model} member={member} onSelectionChange={onSelectionChange} />
      </div>
    </div>
  );
}

/**
 * Header band: delivery-toned avatar, name, role + provider (neutral) + status
 * badges, and the overflow actions (deliver / retry / reconcile / close), all
 * gated on `actionsEnabled`. Provider-neutral: the provider only ever appears as
 * a muted badge.
 */
function MemberHeaderBand({
  model,
  member,
  actionsEnabled,
  onAction,
}: {
  model: WorkbenchModel;
  member: AgentMember;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}) {
  // Avatar/identity is toned by DELIVERY health (not mere process presence): a
  // live process whose delivery is unconfirmed reads amber, never green.
  const tone = deliveryHealthTone(member);
  return (
    <div className="rise flex flex-wrap items-center gap-4 rounded-lg border border-border bg-card px-4 py-3">
      <Avatar name={member.name ?? member.id} tone={tone} size="lg" />
      <div className="min-w-0">
        <p className="text-[11px] uppercase tracking-wider text-muted-foreground">
          AgentMember
          <span className="mx-1 text-border">·</span>
          <MonoId>members/{member.id}</MonoId>
        </p>
        <h1 className="truncate text-lg font-semibold tracking-tight">
          {member.name ?? member.id}
        </h1>
        <div className="mt-1 flex flex-wrap items-center gap-1.5">
          <Badge tone={memberTone(member.runtime_status ?? member.status)}>
            {member.runtime_status ?? member.status ?? "unknown"}
          </Badge>
          <Badge tone="info">{member.role ?? "Member"}</Badge>
          {member.provider && <Badge tone="muted">{member.provider}</Badge>}
        </div>
      </div>
      <div className="ml-auto flex flex-wrap items-center gap-2">
        <MemberOverflowActions
          member={member}
          sessions={model.sessionsByMember}
          inbox={model.inboxMessages}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
        />
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
  onSelectionChange,
}: {
  model: WorkbenchModel;
  member: AgentMember;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const groups = groupMemberTimelineBySession(
    model.selectedMemberTimeline,
    model.sessionsByMember,
  );
  return (
    <section className="rise flex min-h-[36rem] min-w-0 flex-col overflow-hidden rounded-lg border border-border bg-card">
      <header className="flex items-center justify-between gap-2 border-b border-border px-3.5 py-2.5">
        <div className="min-w-0">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            conversation · sessions · actions
          </p>
          <h2 className="truncate text-[13px] font-semibold text-foreground">
            Conversation &amp; action stream
          </h2>
        </div>
        <Badge tone="muted">{model.selectedMemberTimeline.length} events</Badge>
      </header>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
        {model.selectedMemberTimeline.length ? (
          groups.map((group) => (
            <SessionBlock
              key={group.id}
              group={group}
              members={model.members}
              memberName={member.name ?? member.id}
              onSelectionChange={onSelectionChange}
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
 * One collapsible session block: header (provider, status, start→end/duration,
 * thread/turn id) and the nested rows. The default (session-less) group renders
 * a plain "Standalone messages" header. Within a block, input order is preserved
 * so assignment renders before its report.
 */
function SessionBlock({
  group,
  members,
  memberName,
  onSelectionChange,
}: {
  group: MemberSessionGroup;
  members: AgentMember[];
  memberName: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const [open, setOpen] = useState(true);
  const session = group.session;
  const duration = session
    ? formatDuration(session.started_at, session.ended_at)
    : undefined;
  const tone = session ? timelineTone("session") : "idle";
  return (
    <div className="overflow-hidden rounded-lg border border-border bg-background/40">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-accent/40"
      >
        {open ? (
          <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        {session ? (
          <Terminal className="size-3.5 shrink-0 text-status-running" />
        ) : (
          <MessageSquare className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5">
            <span className="truncate text-[12px] font-semibold text-foreground">
              {session
                ? `Provider session${session.provider ? ` · ${session.provider}` : ""}`
                : "Standalone messages"}
            </span>
            {session && (
              <Badge tone={tone}>{session.status ?? "unknown"}</Badge>
            )}
          </span>
          {session && (
            <span className="mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-[10px] text-muted-foreground">
              {session.started_at && <span>{fmtTime(session.started_at)}</span>}
              {duration && (
                <span className="inline-flex items-center gap-1">
                  <Clock className="size-3" />
                  {session.ended_at ? duration : `running ${duration}`}
                </span>
              )}
              {session.provider_thread_id && (
                <span>thread <MonoId>{session.provider_thread_id}</MonoId></span>
              )}
              {session.provider_turn_id && (
                <span>turn <MonoId>{session.provider_turn_id}</MonoId></span>
              )}
            </span>
          )}
        </span>
        <span className="shrink-0 font-mono text-[10px] text-muted-foreground/70">
          {group.items.length}
        </span>
      </button>
      {open && (
        <div className="space-y-2 border-t border-border/60 px-3 py-2.5">
          {group.items.length ? (
            group.items.map((item) => (
              <StreamRow
                key={item.id}
                item={item}
                members={members}
                memberName={memberName}
                onSelectionChange={onSelectionChange}
              />
            ))
          ) : (
            <p className="px-1 py-2 text-[11px] text-muted-foreground">
              No activity recorded in this session window.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * One row in the stream. Messages render as chat bubbles (operator/outbound
 * right-aligned, agent/inbound left); every other kind renders as an inline
 * action card.
 */
function StreamRow({
  item,
  members,
  memberName,
  onSelectionChange,
}: {
  item: TimelineItem;
  members: AgentMember[];
  memberName: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  if (item.kind === "message") {
    return <ChatBubble item={item} members={members} selfName={memberName} />;
  }
  return <ActionCard item={item} onSelectionChange={onSelectionChange} />;
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
}: {
  item: TimelineItem;
  members: AgentMember[];
  selfName: string;
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
          {item.body ?? item.title}
        </div>
        {item.deliveryStatus && (
          <div className={cn("mt-1 flex items-center gap-1", isOperator && "justify-end")}>
            <Badge tone={deliveryStatusTone(item.deliveryStatus)}>{item.deliveryStatus}</Badge>
          </div>
        )}
      </div>
    </div>
  );
}

/** Lucide icon + tone for a non-message stream row, by kind/verdict. */
function actionVisual(item: TimelineItem): { icon: typeof Activity; tone: StatusTone } {
  switch (item.kind) {
    case "session":
      return { icon: Terminal, tone: timelineTone("session") };
    case "evidence":
      return { icon: FileCheck2, tone: "good" };
    case "proposal":
      return { icon: FileText, tone: "decision" };
    case "review":
      return { icon: Gavel, tone: reviewVerdictTone(item.verdict) };
    case "warning":
      return { icon: AlertTriangle, tone: severityTone(item.severity) };
    case "event":
    default:
      return { icon: Zap, tone: "info" };
  }
}

/**
 * Inline action card for an agent action: session start/stop, AgentEvent,
 * Evidence, Proposal, Review, or Warning. Shows an icon, a label, a count/refs
 * line when present ("Ran N commands" / "Edited N files"), and links to the
 * referenced object when one exists.
 */
function ActionCard({
  item,
  onSelectionChange,
}: {
  item: TimelineItem;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const { icon: Icon, tone } = actionVisual(item);
  const countLabel =
    item.count != null && item.countNoun
      ? `${item.count} ${item.countNoun}${item.count === 1 ? "" : "s"}`
      : undefined;
  const sessionLine =
    item.kind === "session"
      ? sessionRunLine(item)
      : undefined;
  return (
    <button
      type="button"
      onClick={() => item.objectRef && onSelectionChange({ taskId: item.objectRef, surface: "task" })}
      className="flex w-full items-start gap-2.5 rounded-lg border border-border bg-card px-3 py-2 text-left transition-colors hover:bg-accent/40"
    >
      <span className={cn("mt-0.5 grid size-6 shrink-0 place-items-center rounded-md bg-background", toneText[tone])}>
        <Icon className="size-3.5" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-center gap-1.5">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {item.eventType ?? item.kind}
          </span>
          {item.verdict && <Badge tone={reviewVerdictTone(item.verdict)}>{item.verdict}</Badge>}
          {countLabel && <span className="text-[11px] text-muted-foreground">{countLabel}</span>}
          {item.createdAt && (
            <span className="ml-auto text-[10px] text-muted-foreground">{fmtTime(item.createdAt)}</span>
          )}
        </span>
        <span className="block truncate text-[13px] font-medium text-foreground">
          {sessionLine ?? item.title}
        </span>
        {item.body && (
          <span className="mt-0.5 block line-clamp-2 text-xs text-muted-foreground">{item.body}</span>
        )}
      </span>
    </button>
  );
}

/** "Provider session running 1m54s" / "Provider session succeeded · 2m" line. */
function sessionRunLine(item: TimelineItem): string {
  const status = item.sessionStatus ?? "session";
  const duration = formatDuration(item.startedAt, item.endedAt);
  if (!duration) return `Provider session ${status}`;
  const running = status === "running" || !item.endedAt;
  return running
    ? `Provider session running ${duration}`
    : `Provider session ${status} · ${duration}`;
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
  onAction?: (path: string, body?: unknown) => void;
}) {
  const [draft, setDraft] = useState("");
  const canSend = Boolean(actionsEnabled && draft.trim());

  function send() {
    const content = draft.trim();
    if (!content || !actionsEnabled) return;
    dispatch(
      onAction,
      operatorMessage({ to: member.id, content, task: member.current_task_id ?? undefined }),
    );
    setDraft("");
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
          disabled={!actionsEnabled}
          className="min-h-9 max-h-32 flex-1 resize-y rounded-md border border-border bg-background px-3 py-2 text-[13px] text-foreground outline-none transition-colors focus:border-ring disabled:cursor-not-allowed disabled:opacity-60"
        />
        {actionsEnabled ? (
          <Button size="sm" onClick={send} disabled={!canSend} className="shrink-0">
            <Send className="size-3.5" />
            Send
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

/**
 * RIGHT pane: the member-owned rail. Carries current task (title / status /
 * branch / acceptance + current proposal), distinct inbox/outbox count tiles,
 * the four-layer runtime panel (+ checked_at + sessions + child threads, in a
 * collapsible block), and the identity/policy block (prompt / skills /
 * permission profile / team membership). Provider-neutral throughout. The Lead
 * responsibilities lane and Lead chip are intentionally NOT here.
 */
function MemberRail({
  model,
  member,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  member: AgentMember;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const currentTask = member.current_task_id
    ? model.tasks.find((task) => task.id === member.current_task_id)
    : undefined;
  const currentProposal = member.current_proposal_id
    ? model.proposals.find((proposal) => proposal.id === member.current_proposal_id)
    : undefined;
  return (
    <aside aria-label="Member rail" className="min-w-0 space-y-4">
      <Section kicker="Current work" title="Current task" className="rise">
        <div className="space-y-2 p-3">
          <button
            type="button"
            onClick={() =>
              member.current_task_id &&
              onSelectionChange({ surface: "task", taskId: member.current_task_id })
            }
            className="block w-full text-left text-[13px] font-medium text-foreground hover:text-primary"
          >
            {taskTitle(model.tasks, member.current_task_id)}
          </button>
          {currentTask && (
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge tone={taskTone(currentTask.status)}>{currentTask.status}</Badge>
              {currentTask.branch_ref && (
                <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground">
                  <GitBranch className="size-3" />
                  <MonoId>{shortBranch(currentTask.branch_ref)}</MonoId>
                </span>
              )}
            </div>
          )}
          {currentTask?.acceptance_criteria?.length ? (
            <div className="rounded-md border border-border bg-background/40 px-2.5 py-1.5">
              <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Acceptance
              </p>
              <ul className="mt-1 space-y-0.5">
                {currentTask.acceptance_criteria.slice(0, 4).map((criterion, index) => (
                  <li key={index} className="flex items-start gap-1.5 text-[11px] text-muted-foreground">
                    <CheckCircle2 className="mt-0.5 size-3 shrink-0 text-status-good" />
                    <span className="min-w-0">{criterion}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          {currentProposal ? (
            <div className="rounded-md border border-border bg-background/40 px-2.5 py-1.5">
              <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Current proposal
              </p>
              <p className="truncate text-xs font-medium">{currentProposal.title ?? currentProposal.id}</p>
              <Badge tone="decision" className="mt-1">{currentProposal.status ?? "draft"}</Badge>
            </div>
          ) : member.current_proposal_id ? (
            <p className="text-[11px] text-muted-foreground">
              Proposal <MonoId>{member.current_proposal_id}</MonoId>
            </p>
          ) : null}
        </div>
      </Section>

      {/* Inbox / Outbox as distinct, countable tiles */}
      <div className="grid grid-cols-2 gap-2">
        <CountTile label="Inbox" value={model.inboxMessages.length} icon={Inbox} />
        <CountTile label="Outbox" value={model.outboxMessages.length} icon={Send} />
      </div>

      <RuntimeRail model={model} member={member} />

      <Section kicker="Identity · policy" title="Prompt · skills · profile" className="rise">
        <div className="p-4">
          <MetaList
            items={[
              { label: "Prompt", value: member.prompt_ref ? <MonoId>{member.prompt_ref}</MonoId> : "—" },
              { label: "Skills", value: member.skill_refs?.join(", ") || "—" },
              {
                label: "Profile",
                value: member.provider_agent_role ? (
                  <Badge tone="muted">{member.provider_agent_role}</Badge>
                ) : (
                  "—"
                ),
              },
              {
                label: "Teams",
                value: member.team_ids?.length ? (
                  <span className="flex flex-wrap gap-1">
                    {member.team_ids.map((id) => (
                      <Badge key={id} tone="muted" className="gap-1">
                        <Users className="size-3" />
                        {id}
                      </Badge>
                    ))}
                  </span>
                ) : (
                  "—"
                ),
              },
            ]}
          />
        </div>
      </Section>
    </aside>
  );
}

/**
 * Runtime block in the member rail: the four-layer RuntimeHealthPanel
 * (process / endpoint / protocol / delivery + checked_at), the provider session
 * list and the child-thread list, collapsible to keep the rail compact.
 */
function RuntimeRail({ model, member }: { model: WorkbenchModel; member: AgentMember }) {
  const [open, setOpen] = useState(true);
  return (
    <Section
      kicker="Health · sessions · child threads"
      title={
        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          className="flex items-center gap-1.5 text-[13px] font-semibold hover:text-primary"
        >
          {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
          Runtime
        </button>
      }
      action={
        <Badge tone={memberTone(member.runtime_status ?? member.status)}>
          {member.runtime_status ?? member.status ?? "unknown"}
        </Badge>
      }
      className="rise"
    >
      {open && (
        <div>
          <RuntimeHealthPanel member={member} />
          <div className="border-t border-border px-3 pt-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {model.sessionsByMember.length} provider sessions
          </div>
          <SessionList sessions={model.sessionsByMember} />
          <div className="border-t border-border px-3 pt-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {member.provider_child_thread_count ?? model.childThreadsByMember.length} child threads
          </div>
          <ChildThreadList threads={model.childThreadsByMember} parent={member} />
        </div>
      )}
    </Section>
  );
}

/** A compact labelled count tile (inbox/outbox), distinct and countable. */
function CountTile({
  label,
  value,
  icon: Icon,
}: {
  label: string;
  value: number;
  icon: typeof Inbox;
}) {
  return (
    <div className="rounded-lg border border-border bg-card px-3 py-2.5">
      <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-muted-foreground">
        <Icon className="size-3.5" />
        {label}
      </div>
      <div className="mt-0.5 text-xl font-semibold tabular-nums">{value}</div>
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
