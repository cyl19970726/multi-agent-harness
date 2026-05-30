import {
  BookOpen,
  Bot,
  Bug,
  ClipboardList,
  Gavel,
  GitBranch,
  Inbox,
  RefreshCw,
  Search,
  Send,
  ShieldAlert,
  Target,
  Users,
  Workflow,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  EmptyState,
  Kbd,
  MonoId,
  StatusDot,
  TimelineRow,
} from "@/components/workbench/atoms";
import { Avatar } from "@/components/workbench/Avatar";
import { memberTone, taskTone, timelineTone } from "@/components/workbench/tones";

import { countBySeverity, taskTitle } from "../model/readModel";
import type { WorkbenchModel } from "../model/readModel";
import {
  DebugSurface,
  DecisionCenter,
  DocsContext,
  GoalDocument,
  GraphKanban,
  MemberWorkbench,
  TaskDocument,
  TeamWorkspace,
  VisionOverview,
  WarningsRepair,
} from "../surfaces/Surfaces";
import type { SelectionState, SurfaceId } from "./selection";

interface WorkbenchShellProps {
  apiUrl: string;
  isLoading: boolean;
  model: WorkbenchModel;
  onApiUrlChange: (value: string) => void;
  onRefresh: () => void;
  onSelectionChange: (selection: SelectionState) => void;
  selection: SelectionState;
  sourceError: string | null;
  sourceLabel: string;
}

const navItems: { id: SurfaceId; label: string; icon: typeof Users }[] = [
  { id: "team", label: "Team", icon: Users },
  { id: "vision", label: "Vision", icon: Target },
  { id: "goal", label: "Goal", icon: ClipboardList },
  { id: "task", label: "Task", icon: GitBranch },
  { id: "graph", label: "Graph", icon: Workflow },
  { id: "member", label: "Member", icon: Bot },
  { id: "docs", label: "Docs", icon: BookOpen },
  { id: "decisions", label: "Decision", icon: Gavel },
  { id: "warnings", label: "Warnings", icon: ShieldAlert },
  { id: "debug", label: "Debug", icon: Bug },
];

export function WorkbenchShell({
  apiUrl,
  isLoading,
  model,
  onApiUrlChange,
  onRefresh,
  onSelectionChange,
  selection,
  sourceError,
  sourceLabel,
}: WorkbenchShellProps) {
  function updateSelection(next: Partial<SelectionState>) {
    onSelectionChange({ ...selection, ...next });
  }

  const severity = countBySeverity(model.warnings);
  const showTeamRail = selection.surface === "team" || selection.surface === "member";

  return (
    <div className="flex h-screen flex-col overflow-hidden text-foreground">
      <TopBar
        apiUrl={apiUrl}
        currentSurface={surfaceLabel(selection.surface)}
        isLoading={isLoading}
        model={model}
        onApiUrlChange={onApiUrlChange}
        onRefresh={onRefresh}
        sourceError={sourceError}
        sourceLabel={sourceLabel}
      />
      <div className="flex min-h-0 flex-1">
        <AppRail
          selection={selection}
          onSurfaceChange={(surface) => updateSelection({ surface })}
          warnings={severity.high}
        />
        {showTeamRail && (
          <TeamRail
            className="hidden lg:flex"
            model={model}
            selection={selection}
            onSelectionChange={updateSelection}
          />
        )}
        <main className="relative min-w-0 flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-[1480px] p-5 xl:p-6">
            <SurfaceSwitch
              model={model}
              selection={selection}
              onSelectionChange={updateSelection}
              sourceLabel={sourceLabel}
            />
          </div>
        </main>
        <Inspector
          className="hidden xl:flex"
          model={model}
          onSelectionChange={updateSelection}
        />
      </div>
    </div>
  );
}

function TopBar({
  apiUrl,
  currentSurface,
  isLoading,
  model,
  onApiUrlChange,
  onRefresh,
  sourceError,
  sourceLabel,
}: Omit<WorkbenchShellProps, "selection" | "onSelectionChange"> & {
  currentSurface: string;
}) {
  const isLive = sourceLabel.includes("live");
  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-border bg-card/70 px-3 backdrop-blur-md">
      <div className="flex items-center gap-2.5">
        <div className="grid size-8 place-items-center rounded-md bg-primary/15 text-primary ring-1 ring-primary/40">
          <Workflow className="size-4" />
        </div>
        <div className="leading-tight">
          <div className="text-[13px] font-semibold tracking-tight">
            Agent Workbench
          </div>
          <div className="truncate text-[11px] text-muted-foreground">
            {currentSurface}
            <span className="mx-1 text-border">/</span>
            <span className="text-foreground/70">
              {model.selectedGoal?.title ?? "No active goal"}
            </span>
          </div>
        </div>
      </div>

      <div className="mx-2 hidden flex-1 justify-center md:flex">
        <button
          type="button"
          className="flex h-8 w-full max-w-md items-center gap-2 rounded-md border border-border bg-background/50 px-2.5 text-xs text-muted-foreground transition-colors hover:border-input"
        >
          <Search className="size-3.5" />
          <span>Search objects, members, tasks…</span>
          <span className="ml-auto">
            <Kbd>⌘K</Kbd>
          </span>
        </button>
      </div>

      <div className="ml-auto flex items-center gap-2">
        <div className="hidden items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 py-1.5 sm:flex">
          <StatusDot tone={sourceError ? "warn" : isLive ? "good" : "info"} pulse={isLive} />
          <span className="text-[11px] text-muted-foreground">{sourceLabel}</span>
        </div>
        <input
          aria-label="Harness API URL"
          value={apiUrl}
          spellCheck={false}
          onChange={(event) => onApiUrlChange(event.target.value)}
          className="hidden h-8 w-44 rounded-md border border-border bg-background/50 px-2 font-mono text-[11px] text-foreground outline-none transition-colors focus:border-ring lg:block"
        />
        <Button size="sm" onClick={onRefresh} disabled={isLoading}>
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          {isLoading ? "Loading" : "Load live"}
        </Button>
      </div>
    </header>
  );
}

function AppRail({
  selection,
  onSurfaceChange,
  warnings,
}: {
  selection: SelectionState;
  onSurfaceChange: (surface: SurfaceId) => void;
  warnings: number;
}) {
  return (
    <nav
      aria-label="Workbench navigation"
      className="flex w-16 shrink-0 flex-col items-center gap-1 border-r border-border bg-card/40 py-3"
    >
      {navItems.map((item) => {
        const active = selection.surface === item.id;
        const Icon = item.icon;
        return (
          <Tooltip key={item.id}>
            <TooltipTrigger asChild>
              <button
                type="button"
                aria-label={item.label}
                onClick={() => onSurfaceChange(item.id)}
                className={cn(
                  "relative grid size-10 place-items-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                  active && "bg-primary/12 text-primary hover:bg-primary/12 hover:text-primary",
                )}
              >
                {active && (
                  <span className="absolute -left-3 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-r bg-primary" />
                )}
                <Icon className="size-[18px]" />
                {item.id === "warnings" && warnings > 0 && (
                  <span className="absolute -right-0.5 -top-0.5 grid h-4 min-w-4 place-items-center rounded-full bg-status-bad px-1 text-[9px] font-bold text-background">
                    {warnings}
                  </span>
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">{item.label}</TooltipContent>
          </Tooltip>
        );
      })}
    </nav>
  );
}

function TeamRail({
  className,
  model,
  selection,
  onSelectionChange,
}: {
  className?: string;
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const activeMemberId = selection.memberId ?? model.selectedMember?.id;
  return (
    <aside
      aria-label="Team and member rail"
      className={cn(
        "w-72 shrink-0 flex-col border-r border-border bg-card/40",
        className,
      )}
    >
      <div className="border-b border-border p-3">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Team
        </p>
        <p className="truncate text-sm font-semibold">
          {model.selectedTeam?.name ?? "No team"}
        </p>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-4 p-3">
          <div className="rounded-lg border border-border bg-background/40 p-3">
            <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Active goal
            </p>
            <p className="mt-0.5 line-clamp-2 text-[13px] font-medium leading-snug">
              {model.selectedGoal?.title ?? "Missing goal"}
            </p>
            <div className="mt-2 flex flex-wrap gap-1.5">
              <Badge tone="info">{model.goalTasks.length} tasks</Badge>
              <Badge tone={model.warnings.length ? "warn" : "good"}>
                {model.warnings.length} warnings
              </Badge>
            </div>
          </div>

          {model.roleGroups.map((group) => (
            <div key={group.role}>
              <p className="px-1 pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {group.role}
              </p>
              <div className="space-y-0.5">
                {group.members.map((member) => {
                  const active = activeMemberId === member.id;
                  const queue =
                    (member.inbox_count ?? 0) + (member.queued_count ?? 0);
                  return (
                    <button
                      key={member.id}
                      type="button"
                      onClick={() =>
                        onSelectionChange({
                          memberId: member.id,
                          taskId: member.current_task_id ?? selection.taskId,
                          surface: "member",
                        })
                      }
                      className={cn(
                        "flex w-full items-center gap-2.5 rounded-md border border-transparent px-2 py-1.5 text-left transition-colors hover:bg-accent/50",
                        active && "border-border bg-accent/60",
                      )}
                    >
                      <Avatar
                        name={member.name ?? member.id}
                        tone={memberTone(member.runtime_status ?? member.status)}
                      />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-medium">
                          {member.name ?? member.id}
                        </span>
                        <span className="block truncate text-[11px] text-muted-foreground">
                          {member.runtime_status ?? member.status ?? "unknown"}
                          <span className="mx-1 text-border">·</span>
                          {taskTitle(model.tasks, member.current_task_id)}
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
      </ScrollArea>
      <div className="border-t border-border p-3">
        <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
          Decision pressure
        </p>
        <p className="text-lg font-semibold tabular-nums">
          {model.decisionQueue.length}
          <span className="ml-1.5 text-xs font-normal text-muted-foreground">
            open items
          </span>
        </p>
      </div>
    </aside>
  );
}

function SurfaceSwitch({
  model,
  selection,
  onSelectionChange,
  sourceLabel,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  sourceLabel: string;
}) {
  switch (selection.surface) {
    case "vision":
      return <VisionOverview model={model} onSelectionChange={onSelectionChange} />;
    case "goal":
      return <GoalDocument model={model} onSelectionChange={onSelectionChange} />;
    case "task":
      return <TaskDocument model={model} onSelectionChange={onSelectionChange} />;
    case "graph":
      return (
        <GraphKanban
          model={model}
          mode={selection.mode ?? "kanban"}
          onSelectionChange={onSelectionChange}
        />
      );
    case "member":
      return <MemberWorkbench model={model} onSelectionChange={onSelectionChange} />;
    case "docs":
      return <DocsContext model={model} onSelectionChange={onSelectionChange} />;
    case "decisions":
      return <DecisionCenter model={model} onSelectionChange={onSelectionChange} />;
    case "warnings":
      return <WarningsRepair model={model} onSelectionChange={onSelectionChange} />;
    case "debug":
      return <DebugSurface model={model} sourceLabel={sourceLabel} />;
    case "team":
    default:
      return <TeamWorkspace model={model} onSelectionChange={onSelectionChange} />;
  }
}

function Inspector({
  className,
  model,
  onSelectionChange,
}: {
  className?: string;
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const member = model.selectedMember;
  const task = model.selectedTask;
  return (
    <aside
      aria-label="Selected object inspector"
      className={cn(
        "w-80 shrink-0 flex-col border-l border-border bg-card/40",
        className,
      )}
    >
      <div className="flex h-14 shrink-0 items-center gap-2.5 border-b border-border px-4">
        {member ? (
          <Avatar
            name={member.name ?? member.id}
            tone={memberTone(member.runtime_status ?? member.status)}
          />
        ) : (
          <div className="grid size-8 place-items-center rounded-md bg-secondary text-muted-foreground">
            <Inbox className="size-4" />
          </div>
        )}
        <div className="min-w-0">
          <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
            {member ? "Selected member" : task ? "Selected task" : "Inspector"}
          </p>
          <p className="truncate text-[13px] font-semibold">
            {member?.name ?? task?.title ?? "Nothing selected"}
          </p>
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-4 p-4">
          {member && (
            <section className="space-y-2.5">
              <div className="flex flex-wrap items-center gap-1.5">
                <Badge tone={memberTone(member.runtime_status ?? member.status)}>
                  {member.runtime_status ?? member.status ?? "unknown"}
                </Badge>
                <Badge tone="info">{member.role ?? "Member"}</Badge>
                {member.provider && <Badge tone="muted">{member.provider}</Badge>}
              </div>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {member.description ??
                  "Persistent AgentMember with role, prompt, runtime state, inbox/outbox and a current task."}
              </p>
              <div className="flex gap-2">
                <Button size="sm" className="flex-1">
                  <Send className="size-3.5" />
                  Message
                </Button>
                <Button size="sm" variant="secondary" className="flex-1">
                  <Inbox className="size-3.5" />
                  Deliver
                </Button>
              </div>
            </section>
          )}

          <section className="grid grid-cols-2 gap-2">
            <Metric label="Inbox" value={member?.inbox_count ?? 0} />
            <Metric label="Queued" value={member?.queued_count ?? 0} />
          </section>

          {task && (
            <section className="rounded-lg border border-border bg-background/40 p-3">
              <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Current task
              </p>
              <button
                type="button"
                onClick={() => onSelectionChange({ surface: "task", taskId: task.id })}
                className="mt-0.5 block text-left text-[13px] font-medium text-foreground hover:text-primary"
              >
                {task.title ?? task.id}
              </button>
              <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
                {task.objective}
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                <Badge tone={taskTone(task.status)}>{task.status}</Badge>
                {task.branch_ref && (
                  <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground">
                    <GitBranch className="size-3" />
                    <MonoId>{shortBranch(task.branch_ref)}</MonoId>
                  </span>
                )}
              </div>
            </section>
          )}

          <section>
            <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Recent activity
            </p>
            <div className="overflow-hidden rounded-lg border border-border bg-background/40">
              {model.selectedMemberTimeline.length ? (
                model.selectedMemberTimeline.slice(0, 5).map((item) => (
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
                <EmptyState title="No recent activity" />
              )}
            </div>
          </section>
        </div>
      </ScrollArea>
    </aside>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md border border-border bg-background/40 px-3 py-2">
      <div className="text-lg font-semibold tabular-nums">{value}</div>
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
    </div>
  );
}

function shortBranch(value: string): string {
  const parts = value.split("/");
  return parts.length > 2 ? `…/${parts.slice(-1)[0]}` : value;
}

function surfaceLabel(surface: SurfaceId): string {
  return navItems.find((item) => item.id === surface)?.label ?? surface;
}
