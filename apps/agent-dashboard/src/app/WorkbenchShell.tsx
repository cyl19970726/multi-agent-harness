import { useEffect, useState, type ComponentProps, type ReactNode } from "react";
import {
  BookOpen,
  Bot,
  Bug,
  ChevronDown,
  Clock,
  FolderGit2,
  GitBranch,
  Globe,
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
  X,
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

import type { WorkbenchModel } from "../model/readModel";
import type { Project } from "../types";
import {
  AgentDetail,
  AgentsList,
  DebugSurface,
  DocsBrowser,
  GoalDocument,
  GraphKanban,
  TaskDocument,
  VisionOverview,
} from "../surfaces/Surfaces";
import { WorkflowRunDetail, WorkflowsList } from "../surfaces/Workflows";
import { deliverQueued } from "../api/actions";
import type { SelectionState, SurfaceId } from "./selection";

interface WorkbenchShellProps {
  apiUrl: string;
  isLoading: boolean;
  model: WorkbenchModel;
  /** Known projects for the header picker (goal-multi-project P6); empty for a
   * single-store / pre-multi-project backend, which hides the picker. */
  projects: Project[];
  /** The currently-selected project id ("" before one is chosen/adopted). */
  selectedProjectId: string;
  /** Switch the active project: re-points the scoped snapshot + SSE stream. */
  onSelectProject: (projectId: string) => void;
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

/** Primary navigation rail: Agents, Vision, Work, Workflows, Docs. */
const navItems: { id: SurfaceId; label: string; icon: typeof Users }[] = [
  { id: "agents", label: "Agents", icon: Bot },
  { id: "vision", label: "Vision", icon: Target },
  { id: "tasks", label: "Work", icon: GitBranch },
  { id: "workflows", label: "Workflows", icon: Workflow },
  { id: "docs", label: "Docs", icon: BookOpen },
];

/**
 * Surfaces reachable in code but intentionally off the primary rail:
 * - agent detail: the Agents surface with a selected agent (?agent=<id>)
 * - goal / task: drill-in detail views reached by selecting an object
 * - debug: moved behind a TopBar button
 */

export function WorkbenchShell({
  apiUrl,
  isLoading,
  model,
  projects,
  selectedProjectId,
  onSelectProject,
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

  // The Agents area (list + agent detail) owns its own layout; the Work board
  // needs full width for its columns; the Goal/Task detail pages are centered
  // Notion documents that read better without a competing rail. All suppress the
  // global Inspector; the rest keep it.
  const noInspector: SurfaceId[] = ["agents", "tasks", "goal", "task", "workflows", "docs"];
  const showInspector = !noInspector.includes(selection.surface);

  return (
    <div className="flex h-screen flex-col overflow-hidden text-foreground">
      <TopBar
        apiUrl={apiUrl}
        currentSurface={surfaceLabel(selection.surface)}
        isLoading={isLoading}
        model={model}
        projects={projects}
        selectedProjectId={selectedProjectId}
        onSelectProject={onSelectProject}
        onApiUrlChange={onApiUrlChange}
        onRefresh={onRefresh}
        sourceError={sourceError}
        sourceLabel={sourceLabel}
        debugActive={selection.surface === "debug"}
        onToggleDebug={() =>
          updateSelection({ surface: selection.surface === "debug" ? "agents" : "debug" })
        }
        pollEnabled={pollEnabled}
        canPoll={canPoll}
        onTogglePoll={onTogglePoll}
      />
      <ActionErrorBanner error={sourceError} />
      <div className="flex min-h-0 flex-1">
        <AppRail
          selection={selection}
          onSurfaceChange={(surface) => updateSelection({ surface })}
        />
        <main className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          {(() => {
            const surface = (
              <SurfaceSwitch
                model={model}
                selection={selection}
                onSelectionChange={updateSelection}
                sourceLabel={sourceLabel}
                actionsEnabled={actionsEnabled}
                onAction={onAction}
                apiUrl={apiUrl}
              />
            );
            // The agent detail is a full-bleed two-pane shell that fills the
            // remaining flex height (so it accounts for the TopBar AND the
            // ActionErrorBanner via the column, with no fragile calc). Every
            // other surface keeps the centered, padded, scrollable document.
            const fullBleed =
              (selection.surface === "agents" && Boolean(selection.memberId)) ||
              selection.surface === "docs";
            return fullBleed ? (
              <div className="min-h-0 flex-1">{surface}</div>
            ) : (
              <div className="flex-1 overflow-y-auto">
                <div className="mx-auto w-full max-w-[1480px] p-5 xl:p-6">{surface}</div>
              </div>
            );
          })()}
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

/**
 * A dismissible banner that surfaces the last failed action / fetch. Without it
 * a rejected write (e.g. the goal-design assignment gate returning 400, or a
 * delivery failure) only nudged a status dot amber — the operator would @-assign
 * and see nothing happen. Re-shows whenever a new, different error arrives.
 */
function ActionErrorBanner({ error }: { error: string | null }) {
  const [dismissed, setDismissed] = useState<string | null>(null);
  if (!error || error === dismissed) return null;
  return (
    <div className="flex items-start gap-2 border-b border-status-warn/30 bg-status-warn/10 px-4 py-2 text-[12px] text-status-warn">
      <ShieldAlert className="mt-0.5 size-3.5 shrink-0" />
      <span className="min-w-0 flex-1 break-words">{error}</span>
      <button
        type="button"
        onClick={() => setDismissed(error)}
        aria-label="Dismiss error"
        className="shrink-0 rounded p-0.5 transition-colors hover:bg-status-warn/20"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

function TopBar({
  apiUrl,
  currentSurface,
  isLoading,
  model,
  projects,
  selectedProjectId,
  onSelectProject,
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
        <ProjectPicker
          projects={projects}
          selectedProjectId={selectedProjectId}
          onSelectProject={onSelectProject}
        />
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
        {/* The dashboard auto-connects to the default harness on load and
            auto-retries while offline, so there is no "Load live" button. The
            URL field + a manual Reconnect appear only when not connected (e.g.
            to point at a non-default backend or recover from an outage). */}
        {!isOnline && (
          <>
            <input
              aria-label="Harness API URL"
              value={apiUrl}
              spellCheck={false}
              onChange={(event) => onApiUrlChange(event.target.value)}
              className="hidden h-8 w-44 rounded-md border border-border bg-background/50 px-2 font-mono text-[11px] text-foreground outline-none transition-colors focus:border-ring lg:block"
            />
            <Button size="sm" onClick={onRefresh} disabled={isLoading}>
              <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
              {isLoading ? "Connecting" : "Reconnect"}
            </Button>
          </>
        )}
      </div>
    </header>
  );
}

/**
 * Compact project picker in the TopBar (goal-multi-project P6). A native
 * `<select>` styled to match the other TopBar controls — switching re-points the
 * scoped snapshot + SSE stream (handled by the App). Renders nothing when there
 * are 0–1 projects (a single-store / pre-multi-project backend), so the picker
 * never appears where it would be meaningless. The `_global` (`kind: "global"`)
 * project gets a globe icon; repo projects a git-folder icon.
 */
function ProjectPicker({
  projects,
  selectedProjectId,
  onSelectProject,
}: {
  projects: Project[];
  selectedProjectId: string;
  onSelectProject: (projectId: string) => void;
}) {
  if (projects.length <= 1) return null;
  const selected = projects.find((p) => p.id === selectedProjectId);
  const isGlobal = selected?.kind === "global";
  return (
    <label className="relative ml-1 hidden items-center sm:flex" title="Active project">
      <span className="pointer-events-none absolute left-2 text-muted-foreground">
        {isGlobal ? <Globe className="size-3.5" /> : <FolderGit2 className="size-3.5" />}
      </span>
      <select
        aria-label="Active project"
        value={selectedProjectId}
        onChange={(event) => onSelectProject(event.target.value)}
        className="h-8 max-w-[180px] appearance-none truncate rounded-md border border-border bg-background/50 pl-7 pr-7 text-[11px] text-foreground outline-none transition-colors hover:border-input focus:border-ring"
      >
        {projects.map((project) => (
          <option key={project.id} value={project.id}>
            {projectLabel(project)}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-muted-foreground" />
    </label>
  );
}

/** Human label for a project option: the reserved `_global` reads "Global (~)";
 * every other project shows its id (the slug / content-hash). */
function projectLabel(project: Project): string {
  if (project.kind === "global" || project.id === "_global") return "Global (~)";
  return project.id;
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
}: {
  selection: SelectionState;
  onSurfaceChange: (surface: SurfaceId) => void;
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
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">{item.label}</TooltipContent>
          </Tooltip>
        );
      })}
    </nav>
  );
}

function SurfaceSwitch({
  model,
  selection,
  onSelectionChange,
  sourceLabel,
  actionsEnabled,
  onAction,
  apiUrl,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  sourceLabel: string;
  actionsEnabled: boolean;
  onAction: (path: string, body?: unknown) => void;
  apiUrl: string;
}) {
  const shared = { model, onSelectionChange, actionsEnabled, onAction, apiUrl };
  switch (selection.surface) {
    case "vision":
      return <VisionOverview {...shared} />;
    case "goal":
      return (
        <GoalDocument
          {...shared}
          phaseId={selection.phaseId}
          phaseView={selection.phaseView}
        />
      );
    case "task":
      return <TaskDocument {...shared} taskTab={selection.taskTab} />;
    case "tasks":
      // The Work board is the Goal collection; a `boardGoal` filter pins it to
      // one legacy goal's task columns (the phaseless fallback).
      return (
        <GraphKanban
          {...shared}
          boardGoal={selection.boardGoal}
          peekTaskId={selection.taskId}
        />
      );
    case "workflows":
      // One surface, self-splitting on the selected run (mirror of agents/memberId).
      return selection.workflowRunId ? (
        <WorkflowRunDetail {...shared} />
      ) : (
        <WorkflowsList {...shared} />
      );
    case "docs":
      return <DocsBrowser {...shared} docPath={selection.docPath} />;
    case "debug":
      return <DebugSurface model={model} sourceLabel={sourceLabel} />;
    case "agents":
    default:
      // The Agents area is one surface: the list, or an agent's detail page when
      // an agent is selected (?agent=<id>). Both own their layout.
      return selection.memberId ? (
        <AgentDetail {...shared} agentTab={selection.agentTab} />
      ) : (
        <AgentsList {...shared} />
      );
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
                    onSelectionChange({ memberId: member.id, surface: "agents" })
                  }
                >
                  <Send className="size-3.5" />
                  Open agent
                </Button>
                <InspectorAction
                  enabled={actionsEnabled}
                  variant="secondary"
                  className="flex-1"
                  onClick={() => {
                    // Pass start_runtime so a runtime is spun up if none is
                    // alive — otherwise queued messages just sit in Queued
                    // because deliver against a dead/absent runtime fails.
                    const d = deliverQueued(member.id, { startRuntime: true });
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
