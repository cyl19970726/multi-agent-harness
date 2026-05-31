import { useEffect, useState, type ComponentProps, type ReactNode } from "react";
import {
  Bot,
  Bug,
  Clock,
  Crown,
  GitBranch,
  Inbox,
  Pause,
  Play,
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
  DocsContext,
  GoalDocument,
  GraphKanban,
  MemberWorkbench,
  TaskDocument,
  TeamWorkspace,
  VisionOverview,
  WarningsRepair,
} from "../surfaces/Surfaces";
import { deliverQueued } from "../api/actions";
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
  /** True only when the snapshot is the live source; gates write actions. */
  actionsEnabled: boolean;
  /** POST a harness action then refresh the snapshot. */
  onAction: (path: string, body?: unknown) => void;
  /** Whether opt-in interval polling of /v1/snapshot is currently on. */
  pollEnabled: boolean;
  /** Whether polling is meaningful right now (only against a live source). */
  canPoll: boolean;
  /** Toggle interval polling on/off. */
  onTogglePoll: () => void;
}

/** Primary navigation rail: collapsed from 10 surfaces to 5 operating views. */
const navItems: { id: SurfaceId; label: string; icon: typeof Users }[] = [
  { id: "team", label: "Team", icon: Users },
  { id: "vision", label: "Vision", icon: Target },
  { id: "tasks", label: "Tasks", icon: Workflow },
  { id: "member", label: "Member", icon: Bot },
  { id: "warnings", label: "Warnings", icon: ShieldAlert },
];

/**
 * Surfaces reachable in code but intentionally off the primary rail:
 * - goal / task: drill-in detail views reached by selecting an object
 * - docs: kept reachable but not a rail slot
 * - debug: moved behind a TopBar button
 * - decisions: folded into the Warnings surface
 */

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
  actionsEnabled,
  onAction,
  pollEnabled,
  canPoll,
  onTogglePoll,
}: WorkbenchShellProps) {
  function updateSelection(next: Partial<SelectionState>) {
    onSelectionChange({ ...selection, ...next });
  }

  const severity = countBySeverity(model.warnings);
  const showTeamRail = selection.surface === "team" || selection.surface === "member";
  // The Member surface is a self-contained desktop-app view that OWNS its own
  // right rail, so the global Inspector is suppressed there to avoid a duplicate
  // rail. Every other surface keeps the global Inspector.
  const showInspector = selection.surface !== "member";

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
        debugActive={selection.surface === "debug"}
        onToggleDebug={() =>
          updateSelection({ surface: selection.surface === "debug" ? "team" : "debug" })
        }
        pollEnabled={pollEnabled}
        canPoll={canPoll}
        onTogglePoll={onTogglePoll}
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
              actionsEnabled={actionsEnabled}
              onAction={onAction}
            />
          </div>
        </main>
        {showInspector && (
          <Inspector
            className="hidden xl:flex"
            model={model}
            onSelectionChange={updateSelection}
            actionsEnabled={actionsEnabled}
            onAction={onAction}
          />
        )}
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
  debugActive,
  onToggleDebug,
  pollEnabled,
  canPoll,
  onTogglePoll,
}: Omit<
  WorkbenchShellProps,
  "selection" | "onSelectionChange" | "actionsEnabled" | "onAction"
> & {
  currentSurface: string;
  debugActive: boolean;
  onToggleDebug: () => void;
}) {
  // Source mode reflected in the chip: "live (SSE)" while the stream is
  // connected, "polling" once we fall back, "offline fixture" otherwise. The
  // pulsing green dot is reserved for a connected stream; polling (live but no
  // push) gets a steady "good" dot; offline is neutral. The freshness
  // ("updated Ns ago") shows in any online mode.
  const isStreaming = sourceLabel.includes("SSE");
  const isOnline = isStreaming || sourceLabel === "polling";
  const statusTone = sourceError ? "warn" : isOnline ? "good" : "info";
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
          <StatusDot tone={statusTone} pulse={isStreaming} />
          <span className="text-[11px] text-muted-foreground">{sourceLabel}</span>
        </div>
        {isOnline && <FreshnessChip generatedAt={model.generatedAt} />}
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label="Live poll"
              aria-pressed={pollEnabled}
              onClick={onTogglePoll}
              disabled={!canPoll}
              className={cn(
                "hidden h-8 items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 text-[11px] text-muted-foreground transition-colors hover:border-input hover:text-foreground sm:flex",
                pollEnabled && "border-primary/40 bg-primary/12 text-primary",
                !canPoll && "cursor-not-allowed opacity-50",
              )}
            >
              {pollEnabled ? <Pause className="size-3.5" /> : <Play className="size-3.5" />}
              <span>Live poll</span>
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            {!canPoll
              ? "Load a live source to enable polling"
              : pollEnabled
                ? "Stop auto-refresh (~5s)"
                : "Auto-refresh every ~5s"}
          </TooltipContent>
        </Tooltip>
        <input
          aria-label="Harness API URL"
          value={apiUrl}
          spellCheck={false}
          onChange={(event) => onApiUrlChange(event.target.value)}
          className="hidden h-8 w-44 rounded-md border border-border bg-background/50 px-2 font-mono text-[11px] text-foreground outline-none transition-colors focus:border-ring lg:block"
        />
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label="Debug"
              aria-pressed={debugActive}
              onClick={onToggleDebug}
              className={cn(
                "grid size-8 place-items-center rounded-md border border-border bg-background/50 text-muted-foreground transition-colors hover:border-input hover:text-foreground",
                debugActive && "border-primary/40 bg-primary/12 text-primary",
              )}
            >
              <Bug className="size-3.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            {debugActive ? "Close raw snapshot" : "Open raw snapshot"}
          </TooltipContent>
        </Tooltip>
        <Button size="sm" onClick={onRefresh} disabled={isLoading}>
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          {isLoading ? "Loading" : "Load live"}
        </Button>
      </div>
    </header>
  );
}

/** Beyond this age the snapshot is considered stale and the chip turns amber. */
const STALE_AFTER_S = 30;

/**
 * Freshness chip: how long ago the snapshot was generated, recomputed every
 * second so a paused (or slow) feed visibly ages. Amber once the snapshot is
 * older than STALE_AFTER_S; muted/neutral while fresh. Renders nothing when the
 * snapshot carries no generated_at (the chip would have nothing honest to say).
 */
function FreshnessChip({ generatedAt }: { generatedAt?: string }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  if (!generatedAt) return null;
  const generatedMs = new Date(generatedAt).getTime();
  if (Number.isNaN(generatedMs)) return null;

  const ageS = Math.max(0, Math.round((now - generatedMs) / 1000));
  const stale = ageS > STALE_AFTER_S;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          className={cn(
            "hidden items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 py-1.5 text-[11px] tabular-nums text-muted-foreground sm:flex",
            stale && "border-status-warn/40 bg-status-warn/10 text-status-warn",
          )}
        >
          <Clock className="size-3" />
          <span>updated {formatAge(ageS)}</span>
        </div>
      </TooltipTrigger>
      <TooltipContent side="bottom">
        {stale
          ? `Snapshot is stale (older than ${STALE_AFTER_S}s) — reload or enable live poll`
          : `Generated ${new Date(generatedMs).toLocaleTimeString()}`}
      </TooltipContent>
    </Tooltip>
  );
}

/** Compact relative age: "just now", "12s ago", "3m ago", "2h ago". */
function formatAge(ageS: number): string {
  if (ageS < 2) return "just now";
  if (ageS < 60) return `${ageS}s ago`;
  const ageM = Math.floor(ageS / 60);
  if (ageM < 60) return `${ageM}m ago`;
  const ageH = Math.floor(ageM / 60);
  return `${ageH}h ago`;
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

/**
 * Team switcher. The snapshot returns every active team but the read model only
 * resolves one selected team (teams[0] / ?team=); this control sets ?team= so an
 * operator can switch between teams. Renders a <select> when there is more than
 * one team; a single team just shows its name (no needless dropdown). Switching
 * team also clears the member/task selection so the rail isn't left pointing at
 * a member from the previous team.
 */
function TeamPicker({
  model,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const teams = model.snapshot.teams ?? [];
  const selectedId = model.selectedTeam?.id ?? "";
  if (teams.length <= 1) {
    return (
      <p className="truncate text-sm font-semibold">
        {model.selectedTeam?.name ?? "No team"}
      </p>
    );
  }
  return (
    <select
      aria-label="Select team"
      value={selectedId}
      onChange={(event) =>
        onSelectionChange({
          teamId: event.target.value,
          // Reset member/task so the rail re-resolves within the new team.
          memberId: undefined,
          taskId: undefined,
        })
      }
      className="mt-0.5 h-8 w-full appearance-none truncate rounded-md border border-border bg-background/60 px-2 text-sm font-semibold text-foreground outline-none transition-colors hover:border-input focus:border-ring"
    >
      {teams.map((team) => (
        <option key={team.id} value={team.id}>
          {team.name ?? team.id}
        </option>
      ))}
    </select>
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
        <TeamPicker model={model} onSelectionChange={onSelectionChange} />
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
                  const isLead = member.id === model.leadMemberId;
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
                        <span className="flex items-center gap-1.5">
                          <span className="truncate text-[13px] font-medium">
                            {member.name ?? member.id}
                          </span>
                          {isLead && (
                            <Badge tone="decision" className="shrink-0 gap-0.5 px-1 py-0">
                              <Crown className="size-2.5" />
                              Lead / Owner
                            </Badge>
                          )}
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
  actionsEnabled,
  onAction,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  sourceLabel: string;
  actionsEnabled: boolean;
  onAction: (path: string, body?: unknown) => void;
}) {
  const shared = { model, onSelectionChange, actionsEnabled, onAction };
  switch (selection.surface) {
    case "vision":
      return <VisionOverview {...shared} />;
    case "goal":
      return <GoalDocument {...shared} />;
    case "task":
      return <TaskDocument {...shared} />;
    case "tasks":
      return <GraphKanban {...shared} mode={selection.mode ?? "kanban"} />;
    case "member":
      return <MemberWorkbench {...shared} />;
    case "docs":
      return <DocsContext {...shared} />;
    case "warnings":
      return <WarningsRepair {...shared} />;
    case "debug":
      return <DebugSurface model={model} sourceLabel={sourceLabel} />;
    case "team":
    default:
      return <TeamWorkspace {...shared} />;
  }
}

function Inspector({
  className,
  model,
  onSelectionChange,
  actionsEnabled,
  onAction,
}: {
  className?: string;
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled: boolean;
  onAction: (path: string, body?: unknown) => void;
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
                <Button
                  variant="default"
                  size="sm"
                  className="flex-1"
                  onClick={() =>
                    onSelectionChange({ memberId: member.id, surface: "member" })
                  }
                >
                  <Send className="size-3.5" />
                  Open chat
                </Button>
                <InspectorAction
                  enabled={actionsEnabled}
                  variant="secondary"
                  className="flex-1"
                  onClick={() => {
                    const d = deliverQueued(member.id);
                    onAction(d.path, d.body);
                  }}
                >
                  <Inbox className="size-3.5" />
                  Deliver
                </InspectorAction>
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

const ACTIONS_DISABLED_HINT = "Connect a live source to enable actions";

/** Inspector action button: visibly disabled with a tooltip when read-only. */
function InspectorAction({
  enabled,
  children,
  ...props
}: ComponentProps<typeof Button> & { enabled: boolean; children: ReactNode }) {
  if (enabled) {
    return (
      <Button size="sm" {...props}>
        {children}
      </Button>
    );
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={cn("inline-flex", props.className)}>
          <Button size="sm" {...props} className="w-full" disabled title={ACTIONS_DISABLED_HINT}>
            {children}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom">{ACTIONS_DISABLED_HINT}</TooltipContent>
    </Tooltip>
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

const offRailLabels: Partial<Record<SurfaceId, string>> = {
  goal: "Goal",
  task: "Task",
  docs: "Docs",
  debug: "Debug",
};

function surfaceLabel(surface: SurfaceId): string {
  return (
    navItems.find((item) => item.id === surface)?.label ??
    offRailLabels[surface] ??
    surface
  );
}
