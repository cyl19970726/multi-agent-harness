import {
  Activity,
  AlertTriangle,
  Bot,
  Bug,
  CheckCircle2,
  ClipboardList,
  ExternalLink,
  FileText,
  Gavel,
  GitBranch,
  Inbox,
  Link2,
  ListChecks,
  MessageSquare,
  Scale,
  Send,
  ShieldAlert,
  ShieldCheck,
  Target,
  User,
  Workflow,
  Wrench,
} from "lucide-react";

import type { ComponentProps, ReactNode } from "react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
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
  gapIsResolved,
  memberName,
  taskTitle,
  type WorkbenchModel,
} from "../model/readModel";
import {
  deliverQueued,
  messageMember,
  requestReview,
  type ActionDescriptor,
} from "../api/actions";
import type {
  AgentMember,
  Gap,
  Goal,
  GoalDesign,
  GoalEvaluation,
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
 * Build a message-to-member descriptor from the model. `/v1/messages` requires
 * an authoring agent and content, so the sender resolves to the team
 * lead/owner (falling back to the recipient when no owner is known) and the
 * content is a neutral default placeholder until a compose surface lands.
 */
function memberMessageDescriptor(model: WorkbenchModel, memberId: string): ActionDescriptor {
  const from = model.selectedTeam?.owner_agent_id ?? memberId;
  return messageMember({
    from,
    to: memberId,
    content: "Message from the dashboard.",
    task: model.selectedTask?.id,
  });
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
    <div className="rounded-md border border-border bg-background/50 px-3.5 py-2 text-center">
      <div className={cn("text-xl font-semibold tabular-nums", toneText[tone])}>
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

function TaskCard({ task, onClick }: { task: Task; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group block w-full rounded-md border border-border bg-background/40 p-2.5 text-left transition-colors hover:border-input hover:bg-accent/40"
    >
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
            <ActionButton
              enabled={actionsEnabled && Boolean(member)}
              size="sm"
              onClick={() =>
                member && dispatch(onAction, memberMessageDescriptor(model, member.id))
              }
            >
              <Send className="size-3.5" />
              Message member
            </ActionButton>
          </>
        }
      />

      <div className="rise relative overflow-hidden rounded-lg border border-border bg-card">
        <span className="absolute inset-y-0 left-0 w-1 bg-primary" />
        <div className="flex flex-wrap items-center justify-between gap-4 p-4 pl-5">
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
/* Vision overview                                                    */
/* ------------------------------------------------------------------ */

export function VisionOverview({ model, onSelectionChange }: SurfaceProps) {
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
    </div>
  );
}

/** A Vision with the goals scheduled against it (goal↔vision link). */
function VisionRow({
  vision,
  goals,
  onSelectGoal,
}: {
  vision: Vision;
  goals: Goal[];
  onSelectGoal: (goalId: string) => void;
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
        <div className="mt-2 flex flex-col gap-0.5">
          {vision.source_refs.map((ref) => (
            <MonoId key={ref}>{ref}</MonoId>
          ))}
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Goal document                                                      */
/* ------------------------------------------------------------------ */

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

  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Goal document"
        title={goal.title ?? goal.id}
        description={goal.objective}
        actions={
          <>
            {goal.priority && <Badge tone="info">priority: {goal.priority}</Badge>}
            <Badge tone={goalTone(goal.status)}>{goal.status ?? "active"}</Badge>
          </>
        }
      />

      <div className="grid gap-4 lg:grid-cols-[1fr_19rem]">
        <div className="space-y-4">
          <Section kicker="Durable outcome" title="Objective" className="rise">
            <p className="p-4 text-[13px] leading-relaxed text-foreground/90">
              {goal.objective ?? "No objective recorded."}
            </p>
          </Section>

          <Section kicker="What done looks like" title="Success criteria" className="rise">
            <CriteriaList items={goal.success_criteria} empty="No success criteria recorded" />
          </Section>

          <Section kicker="Executable thesis" title="Goal design" className="rise">
            <GoalDesignSection design={design} />
          </Section>

          <Section kicker="Retrospective" title="Goal evaluation" className="rise">
            <GoalEvaluationSection evaluation={evaluation} />
          </Section>

          <Section
            kicker="Closeout invariant"
            title="Goal evaluation & decision"
            className="rise"
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
          </Section>

          <Section kicker="Task graph / Kanban" title="Tasks for this goal" className="rise">
            <LaneStack
              model={model}
              onSelect={(task) => onSelectionChange({ taskId: task.id, surface: "task" })}
            />
          </Section>
        </div>

        <div className="space-y-4">
          <Section kicker="Ownership & governance" title="Governance" className="rise">
            <div className="p-4">
              <MetaList
                items={[
                  { label: "Owner", value: memberName(model.members, goal.owner_agent_id) },
                  { label: "Team", value: model.selectedTeam?.name ?? "—" },
                  { label: "Priority", value: goal.priority ?? "—" },
                  { label: "Created", value: fmtTime(goal.created_at) },
                  { label: "Updated", value: fmtTime(goal.updated_at) },
                ]}
              />
            </div>
          </Section>

          <Section kicker="Goal learning" title="Design & evaluation" className="rise">
            <div className="p-4">
              <MetaList
                items={[
                  {
                    label: "Goal design",
                    value:
                      model.goalDesignsForGoal.length +
                      learningCount(learning?.goal_design),
                  },
                  {
                    label: "Evaluation",
                    value:
                      model.goalEvaluationsForGoal.length +
                      learningCount(learning?.goal_evaluation),
                  },
                  {
                    label: "Goal cases",
                    value:
                      model.goalCasesForGoal.length +
                      learningCount(learning?.goal_cases),
                  },
                  { label: "Member reports", value: learningCount(learning?.member_reports) },
                  { label: "Follow-ups", value: learningCount(learning?.follow_up_tasks) },
                  { label: "Blocked tasks", value: blockedTasks.length },
                ]}
              />
            </div>
          </Section>

          <Section kicker="Distance-to-vision" title="Next-round proposals" className="rise">
            <div className="space-y-2 p-3">
              {goalProposals.length ? (
                goalProposals.slice(0, 4).map((proposal) => (
                  <div key={proposal.id} className="rounded-md border border-border bg-background/40 p-2.5">
                    <div className="flex items-center gap-2">
                      <Badge tone="decision">{proposal.disposition ?? "pending"}</Badge>
                      <MonoId>{proposal.source_type ?? "observer"}</MonoId>
                    </div>
                    <p className="mt-1 line-clamp-2 text-xs text-foreground/90">
                      {proposal.summary ?? "Proposed next step"}
                    </p>
                  </div>
                ))
              ) : (
                <EmptyState icon={Target} title="No proposals for this goal" />
              )}
            </div>
          </Section>
        </div>
      </div>
    </div>
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

export function TaskDocument({ model, onSelectionChange, actionsEnabled, onAction }: SurfaceProps) {
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
  const blocks = model.tasks
    .filter((t) => (t.depends_on_task_ids ?? []).includes(task.id))
    .map((t) => t.id);

  return (
    <div className="space-y-5">
      {/* breadcrumb */}
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
        <span className="text-foreground/70">{task.title ?? task.id}</span>
      </div>

      <SurfaceHeader
        kicker="Task document"
        title={task.title ?? task.id}
        actions={
          <>
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
          </>
        }
      />

      <div className="grid gap-4 lg:grid-cols-[1fr_19rem]">
        <div className="space-y-4">
          <Section kicker="What this delivers when done" title="Objective" className="rise">
            <p className="p-4 text-[13px] leading-relaxed text-foreground/90">
              {task.objective ?? "No objective recorded."}
            </p>
          </Section>

          <Section
            kicker="Verifiable at review"
            title="Acceptance criteria"
            action={
              <Badge tone={task.acceptance_criteria?.length ? "info" : "warn"}>
                {task.acceptance_criteria?.length ?? 0}
              </Badge>
            }
            className="rise"
          >
            <CriteriaList
              items={task.acceptance_criteria}
              empty="No acceptance criteria — this task cannot be objectively reviewed yet."
            />
          </Section>

          <Section kicker="Assignment → report → evidence → decision" title="Proof chain" className="rise">
            <div className="space-y-3 p-4">
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
          </Section>

          <Section
            kicker="Structured evaluator output · pass/fail/blocked/needs_changes"
            title="Reviews"
            action={
              <Badge tone={reviews.some((r) => ["fail", "blocked"].includes((r.verdict ?? "").toLowerCase())) ? "bad" : reviews.length ? "good" : "muted"}>
                {reviews.length}
              </Badge>
            }
            className="rise"
          >
            <ReviewList reviews={reviews} />
          </Section>

          <Section kicker="Acceptance" title="Decision & rationale" className="rise">
            {decision ? (
              <div className="space-y-2 p-4">
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
              <EmptyState icon={Gavel} title="No decision yet" description="Awaiting review and a Leader decision." />
            )}
          </Section>

          <Section
            kicker="Proof artifacts"
            title="Evidence & proposals"
            action={<Badge tone="muted">{evidence.length + proposals.length}</Badge>}
            className="rise"
          >
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
          </Section>

          <Section kicker="Messages" title="Assignment & reports" className="rise">
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
          </Section>
        </div>

        <div className="space-y-4">
          <Section kicker="Accountability" title="Ownership" className="rise">
            <div className="p-4">
              <MetaList
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
                ]}
              />
            </div>
          </Section>

          <Section kicker="Where it runs" title="Workspace" className="rise">
            <div className="p-4">
              <MetaList
                items={[
                  { label: "Branch", value: task.branch_ref ? <MonoId>{task.branch_ref}</MonoId> : "—" },
                  { label: "PR", value: task.pr_ref ? <MonoId>{shortBranch(task.pr_ref)}</MonoId> : "—" },
                  { label: "Workspace", value: task.workspace_ref ? <MonoId>{task.workspace_ref}</MonoId> : "—" },
                  { label: "Owned paths", value: <PathList paths={task.owned_paths} /> },
                  { label: "Sessions", value: sessions.length },
                ]}
              />
            </div>
          </Section>

          <Section kicker="Execution order" title="Dependencies" className="rise">
            <div className="space-y-3 p-3.5">
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
          </Section>

          <Section kicker="History" title="Lifecycle" className="rise">
            <div className="p-4">
              <MetaList
                items={[
                  { label: "Status", value: <Badge tone={taskTone(task.status)}>{task.status}</Badge> },
                  { label: "Created", value: fmtTime(task.created_at) },
                  { label: "Updated", value: fmtTime(task.updated_at) },
                ]}
              />
            </div>
          </Section>

          <Section
            kicker="Risks"
            title="Warnings"
            action={<Badge tone={taskWarnings.length ? "bad" : "good"}>{taskWarnings.length}</Badge>}
            className="rise"
          >
            <WarningList
              warnings={taskWarnings}
              onSelect={() => onSelectionChange({ surface: "warnings" })}
            />
          </Section>
        </div>
      </div>
    </div>
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

export function GraphKanban({
  model,
  mode,
  onSelectionChange,
}: SurfaceProps & { mode: "kanban" | "graph" | "split" }) {
  const lanes = model.lanes.filter((lane) => lane.tasks.length);
  return (
    <div className="space-y-5">
      <SurfaceHeader
        kicker="Task relationships"
        title="Tasks"
        description="Synchronized projections of the same task read model. Kanban is the default view; the graph canvas arrives once task counts need pan/zoom."
        actions={
          <div className="flex items-center gap-1 rounded-md border border-border bg-card p-0.5">
            {(["kanban", "graph"] as const).map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => onSelectionChange({ mode: value })}
                className={cn(
                  "rounded px-2.5 py-1 text-xs font-medium capitalize transition-colors",
                  (mode === value || (value === "kanban" && mode === "split"))
                    ? "bg-primary/15 text-primary"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {value}
              </button>
            ))}
          </div>
        }
      />

      {mode === "graph" ? (
        <Section title="Dependency graph" kicker="Coming in WP5" className="rise">
          <EmptyState
            icon={Workflow}
            title="Graph canvas coming in WP5"
            description="Tasks ship as Kanban today; the semantic dependency graph canvas lands in WP5."
          />
        </Section>
      ) : lanes.length ? (
        <div className="flex gap-3 overflow-x-auto pb-2">
          {lanes.map((lane) => (
            <div
              key={lane.id}
              className="flex w-72 shrink-0 flex-col rounded-lg border border-border bg-card/60"
            >
              <div className="flex items-center gap-2 border-b border-border px-3 py-2.5">
                <StatusDot tone={taskTone(lane.id)} />
                <span className="text-[12px] font-semibold">{lane.title}</span>
                <span className="ml-auto font-mono text-[11px] text-muted-foreground">
                  {lane.tasks.length}
                </span>
              </div>
              <div className="space-y-1.5 p-2">
                {lane.tasks.map((task) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    onClick={() => onSelectionChange({ taskId: task.id, surface: "task" })}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <Section title="Task lanes" className="rise">
          <EmptyState icon={ClipboardList} title="No tasks to lay out" />
        </Section>
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
                      <span className="block truncate text-[13px] font-medium">
                        {m.name ?? m.id}
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

export function MemberWorkbench({ model, onSelectionChange, actionsEnabled, onAction }: SurfaceProps) {
  const member = model.selectedMember;
  if (!member) {
    return (
      <div className="space-y-5">
        <SurfaceHeader
          kicker="AgentMember workbench"
          title="Select a member"
          description="Pick a durable AgentMember to inspect its inbox, runtime and activity timeline."
        />
        <MemberPicker model={model} onSelectionChange={onSelectionChange} />
      </div>
    );
  }
  const tone = memberTone(member.runtime_status ?? member.status);
  return (
    <div className="space-y-5">
      <div className="rise flex flex-wrap items-center gap-4">
        <Avatar name={member.name ?? member.id} tone={tone} size="lg" />
        <div className="min-w-0">
          <p className="text-[11px] uppercase tracking-wider text-muted-foreground">
            AgentMember workbench
          </p>
          <h1 className="text-lg font-semibold tracking-tight">
            {member.name ?? member.id}
          </h1>
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            <Badge tone={tone}>{member.runtime_status ?? member.status ?? "unknown"}</Badge>
            <Badge tone="info">{member.role ?? "Member"}</Badge>
            {member.provider && <Badge tone="muted">{member.provider}</Badge>}
          </div>
        </div>
        <div className="ml-auto flex gap-2">
          <ActionButton
            enabled={actionsEnabled}
            size="sm"
            variant="secondary"
            onClick={() => dispatch(onAction, deliverQueued(member.id))}
          >
            <Inbox className="size-3.5" />
            Deliver queued
          </ActionButton>
          <ActionButton
            enabled={actionsEnabled}
            size="sm"
            onClick={() => dispatch(onAction, memberMessageDescriptor(model, member.id))}
          >
            <Send className="size-3.5" />
            Send message
          </ActionButton>
        </div>
      </div>

      {/* Picker stays available so members can be switched without the lg-only rail. */}
      <div className="lg:hidden">
        <MemberPicker model={model} onSelectionChange={onSelectionChange} />
      </div>

      <div className="grid gap-4 lg:grid-cols-[20rem_1fr]">
        <div className="space-y-4">
          <Section kicker="Identity" title="Member profile" className="rise">
            <div className="p-4">
              <MetaList
                items={[
                  { label: "Current task", value: taskTitle(model.tasks, member.current_task_id) },
                  { label: "Prompt", value: member.prompt_ref ? <MonoId>{member.prompt_ref}</MonoId> : "—" },
                  { label: "Skills", value: member.skill_refs?.join(", ") || "—" },
                  { label: "Inbox", value: member.inbox_count ?? 0 },
                  { label: "Queued", value: member.queued_count ?? 0 },
                ]}
              />
            </div>
          </Section>
          <Section kicker="Health" title="Runtime" className="rise">
            <RuntimeHealthPanel member={member} />
          </Section>
        </div>

        <Section
          kicker="inbox · outbox · sessions · events"
          title="Activity timeline"
          className="rise"
        >
          {model.selectedMemberTimeline.length ? (
            <div className="max-h-[34rem] overflow-y-auto">
              {model.selectedMemberTimeline.map((item) => (
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
              ))}
            </div>
          ) : (
            <EmptyState icon={Activity} title="No activity recorded for this member" />
          )}
        </Section>
      </div>
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

export function DocsContext({ model }: SurfaceProps) {
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
            <div key={doc.path} className="flex items-start gap-3 px-4 py-3">
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
            </div>
          ))}
        </div>
      </Section>
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
