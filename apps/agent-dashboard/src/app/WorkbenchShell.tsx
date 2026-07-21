import { useEffect, useState, type ReactNode } from "react";
import {
  BookOpen,
  BriefcaseBusiness,
  Bug,
  Building2,
  CheckCircle2,
  ChevronDown,
  Clock,
  Coins,
  FolderGit2,
  Globe,
  Home,
  Menu,
  Pause,
  Play,
  Plug,
  RefreshCw,
  Search,
  Settings2,
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
import { Kbd, MonoId, StatusDot } from "@/components/workbench/atoms";

import type { WorkbenchModel } from "../model/readModel";
import type { Project } from "../types";
import {
  AgentDetail,
  AgentsList,
  DebugSurface,
} from "../surfaces/Surfaces";
import { WorkflowRunDetail, WorkflowsList } from "../surfaces/Workflows";
import { AgentTeamsHome } from "../surfaces/AgentTeamsHome";
import { TeamWarRoom } from "../surfaces/TeamWarRoom";
import { MemberRunFocus } from "../surfaces/MemberRuns";
import { MissionsSurface } from "../surfaces/Missions";
import { CompanyOsRouter, isCompanyOsSurface, resolveCompanyOsRouteData } from "../company-os/CompanyOsRouter";
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
  onAction: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>;
  /** Whether opt-in interval polling of /v1/snapshot is currently on. */
  pollEnabled: boolean;
  /** Whether polling is meaningful right now (only against a live source). */
  canPoll: boolean;
  /** Toggle interval polling on/off. */
  onTogglePoll: () => void;
}

interface NavigationItem {
  id: SurfaceId;
  label: string;
  icon: typeof Users;
}

const navigationGroups: Array<{ label: "PRIMARY" | "OPERATIONS" | "EXECUTION" | "PLATFORM"; items: NavigationItem[] }> = [
  { label: "PRIMARY", items: [
    { id: "home", label: "Home", icon: Home },
    { id: "docs", label: "Docs", icon: BookOpen },
    { id: "organization", label: "Organization", icon: Building2 },
  ] },
  { label: "OPERATIONS", items: [
    { id: "work", label: "Work", icon: BriefcaseBusiness },
    { id: "approvals", label: "Approvals", icon: CheckCircle2 },
    { id: "finance", label: "Finance", icon: Coins },
  ] },
  { label: "EXECUTION", items: [
    { id: "missions", label: "Missions", icon: Target },
    { id: "workflows", label: "Workflows", icon: Workflow },
    { id: "team", label: "Agent Teams", icon: Users },
  ] },
  { label: "PLATFORM", items: [
    { id: "providers", label: "Providers", icon: Globe },
    { id: "plugins", label: "Plugins", icon: Plug },
    { id: "settings", label: "Settings", icon: Settings2 },
  ] },
];

const navItems = navigationGroups.flatMap((group) => group.items);
const mobilePrimaryItems = navigationGroups[0].items;
const mobileMoreGroups = navigationGroups.slice(1);

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

  return (
    <div className="flex h-screen overflow-hidden text-foreground">
      <AppRail
        model={model}
        selection={selection}
        onSelectionChange={updateSelection}
      />
      <div className="flex min-w-0 flex-1 flex-col pb-14 sm:pb-0">
        <TopBar
          apiUrl={apiUrl}
          currentSurface={surfaceLabel(selection.surface)}
          contextLabel={nativeContextLabel(model, selection)}
          isLoading={isLoading}
          model={model}
          projects={projects}
          selectedProjectId={selectedProjectId}
          onSelectProject={onSelectProject}
          onApiUrlChange={onApiUrlChange}
          onRefresh={onRefresh}
          sourceError={sourceError}
          sourceLabel={sourceLabel}
          prototypeMode={isCompanyOsSurface(selection.surface) && resolveCompanyOsRouteData(model).mode !== "store-live"}
          debugActive={selection.surface === "debug"}
          onToggleDebug={() =>
            updateSelection({ surface: selection.surface === "debug" ? "home" : "debug" })
          }
          pollEnabled={pollEnabled}
          canPoll={canPoll}
          onTogglePoll={onTogglePoll}
        />
        <ActionErrorBanner error={sourceError} />
        <main className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden">
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
              isCompanyOsSurface(selection.surface) ||
              (selection.surface === "agents" && Boolean(selection.memberId)) ||
              (selection.surface === "team" && Boolean(selection.teamId || selection.memberRunId)) ||
              (selection.surface === "missions" && Boolean(selection.missionId)) ||
              selection.surface === "docs";
            return fullBleed ? (
              <div className="flex h-full min-h-0 flex-1">{surface}</div>
            ) : (
              <div className="flex-1 overflow-y-auto">
                <div className="mx-auto w-full max-w-[1480px] p-3 sm:p-5 xl:p-6">{surface}</div>
              </div>
            );
          })()}
        </main>
      </div>
    </div>
  );
}

/**
 * A dismissible banner that surfaces the last failed action / fetch. Without it
 * a rejected write (for example, a governed action returning 400, or a
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
  contextLabel,
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
  prototypeMode,
}: Omit<
  WorkbenchShellProps,
  "selection" | "onSelectionChange" | "actionsEnabled" | "onAction"
> & {
  currentSurface: string;
  contextLabel: string;
  debugActive: boolean;
  onToggleDebug: () => void;
  prototypeMode: boolean;
}) {
  // Source mode reflected in the chip: "live (SSE)" while the stream is
  // connected, "polling" once we fall back, "offline fixture" otherwise. The
  // pulsing green dot is reserved for a connected stream; polling (live but no
  // push) gets a steady "good" dot; offline is neutral. The freshness
  // ("updated Ns ago") shows in any online mode.
  const transportStreaming = sourceLabel.includes("SSE");
  const transportOnline = transportStreaming || sourceLabel === "polling";
  const isStreaming = !prototypeMode && transportStreaming;
  const statusTone = sourceError ? "warn" : prototypeMode ? "info" : transportOnline ? "good" : "info";
  const displayedSourceLabel = prototypeMode ? "prototype fixture" : sourceLabel;
  return (
    <header className="flex h-[58px] min-w-0 shrink-0 items-center gap-2 border-b border-border bg-card/80 px-3 backdrop-blur-md lg:gap-3">
      <div className="flex min-w-0 shrink items-center gap-2.5">
        <div className="grid size-8 place-items-center rounded-md bg-primary/15 text-primary ring-1 ring-primary/40 sm:hidden">
          <Workflow className="size-4" />
        </div>
        <div className="min-w-0 leading-tight">
          <div className="truncate text-[13px] font-semibold tracking-tight">{currentSurface}</div>
          <div className="truncate text-[11px] text-muted-foreground">
            <span className="text-foreground/70">{contextLabel}</span>
          </div>
        </div>
        <ProjectPicker
          projects={projects}
          selectedProjectId={selectedProjectId}
          onSelectProject={onSelectProject}
        />
      </div>

      <div className="mx-1 hidden min-w-0 flex-1 justify-center lg:mx-2 lg:flex">
        <button
          type="button"
          className="flex h-8 w-full max-w-sm items-center gap-2 rounded-md border border-border bg-background/50 px-2.5 text-xs text-muted-foreground transition-colors hover:border-input xl:max-w-md"
        >
          <Search className="size-3.5 shrink-0" />
          <span className="min-w-0 truncate">Search workbench…</span>
          <span className="ml-auto">
            <Kbd>⌘K</Kbd>
          </span>
        </button>
      </div>

      <div className="ml-auto flex shrink-0 items-center gap-2">
        <div className="hidden items-center gap-1.5 rounded-md border border-border bg-background/50 px-2 py-1.5 sm:flex">
          <StatusDot tone={statusTone} pulse={isStreaming} />
          <span className="text-[11px] text-muted-foreground">{displayedSourceLabel}</span>
        </div>
        {!prototypeMode && transportOnline && <FreshnessChip generatedAt={model.generatedAt} />}
        {!prototypeMode && <Tooltip>
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
        </Tooltip>}
        {!prototypeMode && <Tooltip>
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
        </Tooltip>}
        {/* The dashboard auto-connects to the default harness on load and
            auto-retries while offline, so there is no "Load live" button. The
            URL field + a manual Reconnect appear only when not connected (e.g.
            to point at a non-default backend or recover from an outage). */}
        {!prototypeMode && !transportOnline && (
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
  model,
  selection,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const selectedRun = (model.snapshot.team_runs ?? []).find((run) => run.id === selection.teamId);
  const missionId = selection.missionId ?? selectedRun?.mission_id;
  const waveId = selection.waveId ?? selectedRun?.wave_id;
  const mission = (model.snapshot.missions ?? []).find((item) => item.id === missionId);
  const wave = (model.snapshot.waves ?? []).find((item) => item.id === waveId);
  const contextRun = selectedRun ?? (model.snapshot.team_runs ?? []).find(
    (run) => run.wave_id === waveId && run.mission_id === missionId,
  );
  const contextMembers = (model.snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id === contextRun?.id,
  );

  function navigate(id: SurfaceId) {
    onSelectionChange({
      surface: id,
      documentId: undefined,
      workItemId: undefined,
      standingAgentId: undefined,
      personId: undefined,
      proposalId: undefined,
      approvalId: undefined,
      moduleId: undefined,
      memberId: undefined,
      memberRunId: undefined,
      workflowRunId: undefined,
    });
  }

  return (
    <>
      <aside className="hidden h-full w-[14.5rem] shrink-0 flex-col border-r border-sidebar-border bg-sidebar xl:flex">
        <div className="flex h-[58px] shrink-0 items-center gap-2.5 border-b border-border px-4">
          <div className="grid size-8 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm">
            <Building2 className="size-4" />
          </div>
          <div className="min-w-0">
            <p className="text-[13px] font-semibold tracking-tight">Company OS</p>
            <p className="text-[10px] text-muted-foreground">Docs · organization · execution</p>
          </div>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="space-y-5 px-3 py-4">
            <nav aria-label="Product navigation" className="space-y-5">
              {navigationGroups.map((group) => (
                <section key={group.label} aria-labelledby={`nav-${group.label.toLowerCase()}`}>
                  <p id={`nav-${group.label.toLowerCase()}`} className="mb-1 px-2.5 text-[9px] font-semibold tracking-[0.14em] text-muted-foreground">
                    {group.label}
                  </p>
                  <div className="space-y-0.5">
                    {group.items.map((item) => {
                      const active = selection.surface === item.id;
                      const Icon = item.icon;
                      return (
                        <button
                          key={item.id}
                          type="button"
                          onClick={() => navigate(item.id)}
                          className={cn(
                            "flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                            active && "bg-primary/10 font-medium text-primary hover:bg-primary/10 hover:text-primary",
                          )}
                        >
                          <Icon className="size-4 shrink-0" />
                          <span className="whitespace-nowrap">{item.label}</span>
                        </button>
                      );
                    })}
                  </div>
                </section>
              ))}
            </nav>

            <section className="space-y-1.5">
              <p className="px-2.5 text-[9px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
                Active context
              </p>
              {mission ? (
                <div className="space-y-0.5">
                  <ContextTreeButton
                    depth={0}
                    icon={<Target className="size-3.5" />}
                    label={`Mission: ${mission.title}`}
                    active={selection.surface === "missions" && selection.missionId === mission.id && !selection.waveId}
                    onClick={() => onSelectionChange({ surface: "missions", missionId: mission.id, waveId: undefined, teamId: undefined, memberRunId: undefined })}
                  />
                  {wave && (
                    <ContextTreeButton
                      depth={1}
                      icon={<Workflow className="size-3.5" />}
                      label={`Wave ${wave.index} · ${wave.title}`}
                      active={selection.surface === "missions" && selection.waveId === wave.id}
                      onClick={() => onSelectionChange({ surface: "missions", missionId: mission.id, waveId: wave.id, teamId: undefined, memberRunId: undefined })}
                    />
                  )}
                  {contextRun && (
                    <ContextTreeButton
                      depth={2}
                      icon={<Users className="size-3.5" />}
                      label="Agent Team"
                      active={selection.surface === "team" && selection.teamId === contextRun.id && !selection.memberRunId}
                      onClick={() => onSelectionChange({ surface: "team", teamId: contextRun.id, memberRunId: undefined })}
                    />
                  )}
                  {contextMembers.map((member) => (
                    <ContextTreeButton
                      key={member.id}
                      depth={3}
                      icon={<StatusDot tone={member.status === "blocked" || member.status === "failed" ? "bad" : member.status === "running" ? "running" : member.status === "completed" ? "good" : "idle"} />}
                      label={member.name ?? member.id}
                      active={selection.memberRunId === member.id}
                      onClick={() => onSelectionChange({ surface: "team", teamId: contextRun?.id, memberRunId: member.id })}
                    />
                  ))}
                </div>
              ) : (
                <p className="px-2.5 py-2 text-[11px] leading-relaxed text-muted-foreground">
                  Open a Mission to keep its Wave, Team, and Members in reach.
                </p>
              )}
            </section>

          </div>
        </ScrollArea>
      </aside>

      <aside className="hidden h-full w-16 shrink-0 flex-col items-center border-r border-sidebar-border bg-sidebar py-3 sm:flex xl:hidden">
        <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm" aria-label="Company OS">
          <Building2 className="size-4" />
        </div>
        <nav aria-label="Compact product navigation" className="mt-4 flex min-h-0 flex-1 flex-col items-center gap-1 overflow-y-auto px-2">
          {navigationGroups.map((group, index) => (
            <div key={group.label} className={cn("flex flex-col items-center gap-1", index > 0 && "mt-2 border-t border-border pt-2")}>
              {group.items.map((item) => {
                const active = selection.surface === item.id;
                const Icon = item.icon;
                return (
                  <Tooltip key={item.id}>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        aria-label={`${group.label}: ${item.label}`}
                        onClick={() => navigate(item.id)}
                        className={cn(
                          "grid size-9 shrink-0 place-items-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                          active && "bg-primary/10 text-primary hover:bg-primary/10 hover:text-primary",
                        )}
                      >
                        <Icon className="size-4" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="right">{group.label} · {item.label}</TooltipContent>
                  </Tooltip>
                );
              })}
            </div>
          ))}
        </nav>
        {mission && (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                aria-label={`Active Mission: ${mission.title}`}
                onClick={() => onSelectionChange({ surface: "missions", missionId: mission.id, waveId: waveId ?? undefined })}
                className="mb-2 grid size-10 place-items-center rounded-lg border border-primary/20 bg-primary/5 text-primary"
              >
                <Target className="size-[18px]" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">{wave ? `Wave ${wave.index} · ${wave.title}` : mission.title}</TooltipContent>
          </Tooltip>
        )}
      </aside>

      <nav aria-label="Mobile navigation" className="fixed inset-x-0 bottom-0 z-50 flex h-14 items-center justify-around border-t border-border bg-card px-1 sm:hidden">
        {mobilePrimaryItems.map((item) => {
          const active = selection.surface === item.id;
          const Icon = item.icon;
          return (
            <button key={item.id} type="button" aria-label={item.label} onClick={() => navigate(item.id)} className={cn("flex h-12 min-w-[74px] flex-col items-center justify-center gap-0.5 rounded-lg px-1 text-[10px] text-muted-foreground", active && "bg-primary/10 font-medium text-primary")}>
              <Icon className="size-4" />
              <span className="whitespace-nowrap">{item.label}</span>
            </button>
          );
        })}
        <details className="group relative">
          <summary className="flex h-12 min-w-[74px] cursor-pointer list-none flex-col items-center justify-center gap-0.5 rounded-lg px-1 text-[10px] text-muted-foreground hover:bg-accent">
            <Menu className="size-4" />
            <span>More</span>
          </summary>
          <div className="absolute bottom-14 right-0 w-56 rounded-lg border border-border bg-card p-2 shadow-lg">
            {mobileMoreGroups.map((group) => (
              <section key={group.label} className="mb-2 last:mb-0">
                <p className="px-2 py-1 text-[9px] font-semibold tracking-wider text-muted-foreground">{group.label}</p>
                {group.items.map((item) => {
                  const Icon = item.icon;
                  return <button key={item.id} type="button" onClick={() => navigate(item.id)} className="flex h-9 w-full items-center gap-2 rounded-md px-2 text-left text-xs text-foreground hover:bg-accent"><Icon className="size-4 text-muted-foreground" />{item.label}</button>;
                })}
              </section>
            ))}
          </div>
        </details>
      </nav>
    </>
  );
}

function ContextTreeButton({
  depth,
  icon,
  label,
  active,
  onClick,
}: {
  depth: number;
  icon: ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-8 w-full items-center gap-2 rounded-md pr-2 text-left text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
        active && "bg-primary/10 font-medium text-primary hover:bg-primary/10 hover:text-primary",
      )}
      style={{ paddingLeft: `${10 + depth * 12}px` }}
    >
      <span className="shrink-0">{icon}</span>
      <span className="truncate">{label}</span>
    </button>
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
  onAction: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>;
  apiUrl: string;
}) {
  const shared = { model, onSelectionChange, actionsEnabled, onAction, apiUrl };
  if (isCompanyOsSurface(selection.surface)) {
    return <CompanyOsRouter model={model} selection={selection} actionsEnabled={actionsEnabled} onAction={onAction} />;
  }
  switch (selection.surface) {
    case "missions":
      return (
        <MissionsSurface
          {...shared}
          missionId={selection.missionId}
          waveId={selection.waveId}
        />
      );
    case "workflows":
      // One surface, self-splitting on the selected run (mirror of agents/memberId).
      return selection.workflowRunId ? (
        <WorkflowRunDetail {...shared} />
      ) : (
        <WorkflowsList {...shared} />
      );
    case "team":
      return selection.memberRunId ? (
        <MemberRunFocus {...shared} memberRunId={selection.memberRunId} />
      ) : selection.teamId ? (
        <TeamWarRoom {...shared} teamRunId={selection.teamId} />
      ) : (
        <AgentTeamsHome {...shared} />
      );
    case "debug":
      return <DebugSurface model={model} sourceLabel={sourceLabel} />;
    case "agents":
    default:
      // The Agents area is one surface: the list, or an agent's detail page when
      // an agent is selected (?agent=<id>). Both own their layout.
      return selection.memberId ? (
        <AgentDetail {...shared} />
      ) : (
        <AgentsList {...shared} />
      );
  }
}

const offRailLabels: Partial<Record<SurfaceId, string>> = {
  team: "Agent Team",
  agents: "Execution agent",
  debug: "Debug",
};

function nativeContextLabel(model: WorkbenchModel, selection: SelectionState): string {
  if (selection.surface === "missions") {
    const mission = (model.snapshot.missions ?? []).find(
      (candidate) => candidate.id === selection.missionId,
    );
    return mission?.title ?? "Mission control";
  }

  if (selection.surface === "team") {
    const memberRun = (model.snapshot.member_runs ?? []).find(
      (candidate) => candidate.id === selection.memberRunId,
    );
    const run = (model.snapshot.team_runs ?? []).find(
      (candidate) => candidate.id === (selection.teamId ?? memberRun?.team_run_id),
    );
    const mission = run?.mission_id
      ? (model.snapshot.missions ?? []).find((candidate) => candidate.id === run.mission_id)
      : undefined;
    return memberRun?.name ?? mission?.title ?? (run ? "Team attempt" : "Agent Team attempts");
  }

  switch (selection.surface) {
    case "home":
      return "Company attention";
    case "organization":
      if (selection.personId === "actor-human-brand-owner") return "Brand Owner";
      if (selection.standingAgentId === "actor-agent-document-architecture") return "Document Architecture Agent";
      if (selection.proposalId === "governance-proposal-trademark-management") return "Create Trademark Management module";
      return selection.personId ?? selection.standingAgentId ?? selection.proposalId ?? "Mixed organization";
    case "work":
      return selection.workItemId === "workitem-trademark-filing-brand-a"
        ? "Trademark filing for Brand A"
        : selection.workItemId ?? "Company work";
    case "approvals":
      return selection.approvalId === "approval-trademark-filing-fee-cn-2026-018"
        ? "Approve trademark filing fee"
        : selection.approvalId ?? "Approval inbox";
    case "finance":
      return "Financial records";
    case "providers":
    case "plugins":
    case "settings":
      return "Platform";
    case "agents":
      return "Execution compatibility";
    case "workflows":
      return "Dynamic workflows";
    case "docs":
      if (selection.documentId === "document-brand-a-content-operating-plan") return "Brand A content operating plan";
      if (selection.moduleId === "module-trademark-management") return "Trademark Management";
      return selection.documentId ?? selection.moduleId ?? "Company knowledge";
    case "debug":
      return "Diagnostics";
    default:
      return "Control plane";
  }
}

function surfaceLabel(surface: SurfaceId): string {
  return (
    navItems.find((item) => item.id === surface)?.label ??
    offRailLabels[surface] ??
    surface
  );
}
