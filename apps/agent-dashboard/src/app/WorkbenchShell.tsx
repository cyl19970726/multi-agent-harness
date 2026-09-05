import { useMemo, useState, type ReactNode } from "react";
import {
  BriefcaseBusiness,
  Bug,
  ChevronDown,
  FolderGit2,
  Globe,
  Menu,
  Pause,
  Play,
  RefreshCw,
  Search,
  ServerCog,
  Settings2,
  ShieldAlert,
  Sparkles,
  Users,
  Wrench,
  X,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Kbd, StatusDot } from "@/components/workbench/atoms";
import { ProvenanceFooter } from "@/components/workbench/ProvenanceFooter";
import { EvidenceLinkProvider } from "@/components/workbench/EvidenceLinkContext";
import { SourceViewer } from "@/components/workbench/SourceViewer";

import type { WorkbenchModel } from "../model/readModel";
import type { RoleActionExecutor } from "../model/roleViews";
import type { DashboardSnapshot, ExecutionSpace, Project } from "../types";
import {
  AgentDetail,
  AgentsList,
  DebugSurface,
  ProjectsSurface,
  ProvidersSurface,
  SettingsSurface,
} from "../surfaces/Surfaces";
import { AgentTeamsHome } from "../surfaces/AgentTeamsHome";
import { GlobalWorkIndex } from "../surfaces/GlobalWorkIndex";
import { TeamWorkspace } from "../surfaces/TeamWorkspace";
import { AgentConversationWorkspace } from "../surfaces/AgentConversationWorkspace";
import { OperatorView } from "../surfaces/OperatorView";
import type { SelectionState, SurfaceId } from "./selection";
import { freshnessDomains, type DomainFreshness, type FreshnessDomain } from "./freshness";

interface WorkbenchShellProps {
  apiUrl: string;
  isLoading: boolean;
  model: WorkbenchModel;
  /** Known projects for the header picker (goal-multi-project P6); empty for a
   * single-store / pre-multi-project backend, which hides the picker. */
  projects: Project[];
  /** Independent coordination namespaces. */
  spaces: ExecutionSpace[];
  /** The currently-selected project id ("" before one is chosen/adopted). */
  selectedProjectId: string;
  selectedSpaceId: string;
  /** Company scope still scopes authenticated RoleView reads (collaboration
   * projection); there is deliberately no Company picker or Company context
   * in navigation any more (DOC-107). */
  selectedCompanyId: string;
  /** Switch the active project: re-points the scoped snapshot + SSE stream. */
  onSelectProject: (projectId: string) => void;
  onSelectSpace: (spaceId: string) => void;
  onApiUrlChange: (value: string) => void;
  onRefresh: () => void;
  onSelectionChange: (selection: SelectionState) => void;
  onSelectionReplace: (selection: SelectionState) => void;
  selection: SelectionState;
  sourceError: string | null;
  sourceLabel: string;
  /** Per-domain convergence for the currently selected Space/Company scope. */
  domainFreshness: DomainFreshness;
  /** True only when the snapshot is the live source; gates write actions. */
  actionsEnabled: boolean;
  /** POST a harness action then refresh the snapshot. */
  onAction: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>;
  onRoleAction: RoleActionExecutor;
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

/** Retained navigation (DOC-107): primary Global Work / Agent Teams / Nodes,
 * secondary Providers / Projects / Settings. Team Workspace, Host Console,
 * Agent Workspace and Diagnostics remain deep-linkable off-rail. */
const navigationGroups: Array<{ label: "PRIMARY" | "SECONDARY"; items: NavigationItem[] }> = [
  { label: "PRIMARY", items: [
    { id: "work", label: "Global Work", icon: BriefcaseBusiness },
    { id: "team", label: "Agent Teams", icon: Users },
    { id: "operator", label: "Nodes", icon: ServerCog },
  ] },
  { label: "SECONDARY", items: [
    { id: "providers", label: "Providers", icon: Wrench },
    { id: "projects", label: "Projects", icon: FolderGit2 },
    { id: "settings", label: "Settings", icon: Settings2 },
  ] },
];

const navItems = navigationGroups.flatMap((group) => group.items);
const mobilePrimaryItems = navigationGroups[0].items;
const mobileMoreGroups = navigationGroups.slice(1);

/**
 * Surfaces reachable in code but intentionally off the primary rail:
 * - agent detail: the Agents surface with a selected agent (?agent=<id>)
 * - debug: moved behind a TopBar button
 */

export function WorkbenchShell({
  apiUrl,
  isLoading,
  model,
  projects,
  spaces,
  selectedProjectId,
  selectedSpaceId,
  selectedCompanyId,
  onSelectProject,
  onSelectSpace,
  onApiUrlChange,
  onRefresh,
  onSelectionChange,
  onSelectionReplace,
  selection,
  sourceError,
  sourceLabel,
  domainFreshness,
  actionsEnabled,
  onAction,
  onRoleAction,
  pollEnabled,
  canPoll,
  onTogglePoll,
}: WorkbenchShellProps) {
  const memberFocusMode = selection.surface === "team" && Boolean(selection.memberRunId);
  const focusedTeamMode = selection.surface === "team" && Boolean(selection.teamId);
  const compactExecutionMode = memberFocusMode || focusedTeamMode;
  const roleActionsCurrent = actionsEnabled && domainFreshness.works === "live" && domainFreshness.runtime === "live";
  function updateSelection(next: Partial<SelectionState>) {
    onSelectionChange({ ...selection, ...next });
  }
  function replaceSelection(next: Partial<SelectionState>) {
    onSelectionReplace({ ...selection, ...next });
  }

  return (
    <div className="flex h-screen overflow-hidden text-foreground">
      <AppRail
        model={model}
        selection={selection}
        onSelectionChange={updateSelection}
        compact={compactExecutionMode}
      />
      <div className="flex min-w-0 flex-1 flex-col pb-14 sm:pb-0">
        {!compactExecutionMode && <TopBar
          apiUrl={apiUrl}
          currentSurface={surfaceLabel(selection.surface)}
          contextLabel={nativeContextLabel(model, selection)}
          isLoading={isLoading}
          model={model}
          projects={projects}
          spaces={spaces}
          selectedProjectId={selectedProjectId}
          selectedSpaceId={selectedSpaceId}
          onSelectProject={onSelectProject}
          onSelectSpace={onSelectSpace}
          onApiUrlChange={onApiUrlChange}
          onRefresh={onRefresh}
          sourceError={sourceError}
          sourceLabel={sourceLabel}
          domainFreshness={domainFreshness}
          debugActive={selection.surface === "debug"}
          onToggleDebug={() =>
            updateSelection({ surface: selection.surface === "debug" ? "work" : "debug" })
          }
          pollEnabled={pollEnabled}
          canPoll={canPoll}
          onTogglePoll={onTogglePoll}
        />}
        <ActionErrorBanner error={sourceError} />
        <main className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden">
          <EvidenceLinkProvider open={(target, messageId) => updateSelection({ sourcePath: target.path, sourceLine: target.line, sourceMessageId: messageId })}>
            {(() => {
            const surface = (
              <SurfaceSwitch
                model={model}
                selection={selection}
                onSelectionChange={updateSelection}
                onSelectionReplace={replaceSelection}
                sourceLabel={sourceLabel}
                actionsEnabled={actionsEnabled}
                onAction={onAction}
                onRoleAction={onRoleAction}
                roleActionsCurrent={roleActionsCurrent}
                apiUrl={apiUrl}
                projectBindingId={selectedProjectId}
                executionSpaceId={selectedSpaceId}
                companyId={selectedCompanyId}
                projects={projects}
                isLoading={isLoading}
              />
            );
            // The agent detail is a full-bleed two-pane shell that fills the
            // remaining flex height (so it accounts for the TopBar AND the
            // ActionErrorBanner via the column, with no fragile calc). Every
            // other surface keeps the centered, padded, scrollable document.
            const fullBleed =
              (selection.surface === "agents" && Boolean(selection.memberId)) ||
              (selection.surface === "team" && Boolean(selection.teamId || selection.memberRunId));
            return fullBleed ? (
              <div className="flex h-full min-h-0 min-w-0 flex-1">{surface}</div>
            ) : (
              <div className="flex-1 overflow-y-auto">
                <div className="mx-auto w-full max-w-[1480px] p-3 sm:p-5 xl:p-6">{surface}</div>
              </div>
            );
            })()}
          </EvidenceLinkProvider>
          {selection.sourcePath && (
            <SourceViewer
              apiUrl={apiUrl}
              project={selectedProjectId}
              space={selectedSpaceId}
              path={selection.sourcePath}
              line={selection.sourceLine}
              messageId={selection.sourceMessageId}
              onBack={() => window.history.back()}
            />
          )}
        </main>
        <ProvenanceFooter apiUrl={apiUrl} projectId={selectedProjectId} spaceId={selectedSpaceId} />
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
  projects,
  spaces,
  selectedProjectId,
  selectedSpaceId,
  onSelectProject,
  onSelectSpace,
  onApiUrlChange,
  onRefresh,
  sourceError,
  sourceLabel,
  domainFreshness,
  debugActive,
  onToggleDebug,
  pollEnabled,
  canPoll,
  onTogglePoll,
}: Omit<
  WorkbenchShellProps,
  "selection" | "onSelectionChange" | "onSelectionReplace" | "actionsEnabled" | "onAction" | "onRoleAction" | "selectedCompanyId"
> & {
  currentSurface: string;
  contextLabel: string;
  debugActive: boolean;
  onToggleDebug: () => void;
}) {
  // Product freshness is explicit: socket-open alone does not earn Live.
  const transportStreaming = sourceLabel === "Live";
  const transportOnline = ["Live", "Reconnecting", "Stale"].includes(sourceLabel);
  const isStreaming = transportStreaming;
  return (
    <header className="flex h-[58px] min-w-0 shrink-0 items-center gap-2 border-b border-border bg-card/80 px-3 backdrop-blur-md lg:gap-3">
      <div className="flex min-w-0 shrink items-center gap-2.5">
        <div className="grid size-8 place-items-center rounded-md bg-primary/15 text-primary ring-1 ring-primary/40 sm:hidden">
          <Users className="size-4" />
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
        <SpacePicker
          spaces={spaces}
          selectedSpaceId={selectedSpaceId}
          onSelectSpace={onSelectSpace}
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
        <DomainFreshnessStrip
          freshness={domainFreshness}
          sourceError={sourceError}
          runtimeLabel={sourceLabel}
          runtimePulse={isStreaming}
        />
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
                ? "Stop healthy-stream safety polling (~15s); reconnect fallback remains ~5s"
                : "Poll a healthy stream every ~15s; reconnect fallback is automatic (~5s)"}
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
        {!transportOnline && (
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
 * Compact Project Binding picker in the TopBar. Switching changes provider cwd,
 * instruction/Skill discovery, Git/worktree and permission boundaries without
 * changing the selected Execution Space or its snapshot/SSE stream. The
 * `_global` (`kind: "global") binding gets a globe icon; repo bindings a
 * git-folder icon.
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
  const selected = projects.find((p) => p.id === selectedProjectId);
  if (!selected) return null;
  const isGlobal = selected?.kind === "global";
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <label className="relative ml-1 hidden items-center sm:flex" aria-label="Workspace roots">
          <span className="pointer-events-none absolute left-2 text-muted-foreground">
            {isGlobal ? <Globe className="size-3.5" /> : <FolderGit2 className="size-3.5" />}
          </span>
          <select
            aria-label="Active project"
            value={selectedProjectId}
            disabled={projects.length === 1}
            onChange={(event) => onSelectProject(event.target.value)}
            className="h-8 max-w-[180px] appearance-none truncate rounded-md border border-border bg-background/50 pl-7 pr-7 text-[11px] text-foreground outline-none transition-colors hover:border-input focus:border-ring disabled:opacity-100"
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {projectLabel(project)}
              </option>
            ))}
          </select>
          <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-muted-foreground" />
        </label>
      </TooltipTrigger>
      <TooltipContent className="max-w-[36rem] space-y-1">
        <p><span className="text-muted-foreground">Provider cwd boundary:</span> <span className="font-mono">{selected.project_root}</span></p>
        <p><span className="text-muted-foreground">Skill discovery boundary:</span> <span className="font-mono">{selected.skill_discovery_boundary ?? selected.project_root}</span></p>
        <p>Project Binding does not own coordination storage.</p>
      </TooltipContent>
    </Tooltip>
  );
}

function SpacePicker({
  spaces,
  selectedSpaceId,
  onSelectSpace,
}: {
  spaces: ExecutionSpace[];
  selectedSpaceId: string;
  onSelectSpace: (spaceId: string) => void;
}) {
  const selected = spaces.find((space) => space.id === selectedSpaceId);
  if (!selected) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <label className="relative ml-1 hidden items-center sm:flex" aria-label="Execution Space">
          <span className="pointer-events-none absolute left-2 text-primary">
            <ServerCog className="size-3.5" />
          </span>
          <select
            aria-label="Active execution space"
            value={selectedSpaceId}
            disabled={spaces.length === 1}
            onChange={(event) => onSelectSpace(event.target.value)}
            className="h-8 max-w-[190px] appearance-none truncate rounded-md border border-primary/25 bg-primary/5 pl-7 pr-7 text-[11px] text-foreground outline-none transition-colors hover:border-primary/45 focus:border-primary disabled:opacity-100"
          >
            {spaces.map((space) => (
              <option key={space.id} value={space.id}>
                {space.name?.trim() ? `${space.name} (${space.id})` : space.id}
              </option>
            ))}
          </select>
          <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-primary" />
        </label>
      </TooltipTrigger>
      <TooltipContent className="max-w-[36rem] space-y-1">
        <p><span className="text-muted-foreground">Execution coordination:</span> AgentTeam / Work / Message</p>
        <p><span className="text-muted-foreground">Store:</span> <span className="font-mono">{selected.store_root}</span></p>
        <p><span className="text-muted-foreground">Default binding:</span> <span className="font-mono">{selected.default_project_binding_id ?? "none"}</span></p>
      </TooltipContent>
    </Tooltip>
  );
}

/** Human label for a project option: the reserved `_global` reads "Global (~)";
 * every other project shows its id (the slug / content-hash). */
function projectLabel(project: Project): string {
  if (project.kind === "global" || project.id === "_global") return "Global (~)";
  return project.id;
}

const freshnessLabels: Record<FreshnessDomain, string> = {
  works: "Works",
  docs: "Docs",
  organization: "Org",
  runtime: "Runtime",
};

function DomainFreshnessStrip({
  freshness,
  runtimeLabel,
  runtimePulse,
  sourceError,
}: {
  freshness: DomainFreshness;
  runtimeLabel: string;
  runtimePulse: boolean;
  sourceError: string | null;
}) {
  return (
    <div
      role="group"
      aria-label="Scoped domain freshness"
      className="flex min-w-0 items-center gap-1 rounded-md border border-border bg-background/50 px-1 py-1"
    >
      {freshnessDomains.map((domain) => {
        const status = freshness[domain];
        const label = domain === "runtime" ? runtimeLabel : statusLabel(status);
        const tone = sourceError || status === "stale"
          ? "warn"
          : status === "live"
            ? "good"
            : "info";
        return (
          <span
            key={domain}
            role="status"
            aria-label={`${freshnessLabels[domain]} freshness: ${label}`}
            data-dashboard-freshness={status}
            data-freshness-domain={domain}
            data-freshness-status={status}
            className="flex min-w-0 items-center gap-1 rounded px-1 py-0.5 text-[9px] text-muted-foreground sm:text-[10px]"
          >
            <StatusDot tone={tone} pulse={domain === "runtime" && runtimePulse} />
            <span className="hidden xl:inline">{freshnessLabels[domain]}:</span>
            <span>{label}</span>
          </span>
        );
      })}
    </div>
  );
}

function statusLabel(status: DomainFreshness[FreshnessDomain]): string {
  if (status === "live") return "Live";
  if (status === "stale") return "Stale";
  if (status === "offline") return "Offline";
  return "Reconnecting";
}

function AppRail({
  model,
  selection,
  onSelectionChange,
  compact = false,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  compact?: boolean;
}) {
  // The context block follows the selected durable Team (never a TeamRun
  // attempt). A historical run-id deep link resolves to its owning Team first.
  const selectedRun = (model.snapshot.team_runs ?? []).find((run) => run.id === selection.teamId);
  const selectedTeam = (model.snapshot.teams ?? []).find(
    (team) => team.id === (selection.teamId ?? selectedRun?.agent_team_id),
  ) ?? (model.snapshot.teams ?? []).find((team) => team.id === selectedRun?.agent_team_id);
  const latestRun = selectedTeam
    ? [...(model.snapshot.team_runs ?? [])]
        .filter((run) => run.agent_team_id === selectedTeam.id)
        .sort((left, right) => (right.created_at ?? "").localeCompare(left.created_at ?? ""))[0]
    : undefined;
  const contextMembers = (model.snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id === latestRun?.id,
  );

  function navigate(id: SurfaceId) {
    onSelectionChange({
      surface: id,
      memberId: undefined,
      memberRunId: undefined,
      teamId: undefined,
      teamWorkId: undefined,
      workTeamId: undefined,
      workHostId: undefined,
      workMemberId: undefined,
      workAssignee: undefined,
      workStatus: undefined,
      workPriority: undefined,
    });
  }

  return (
    <>
      <aside className={cn("hidden h-full w-[14.5rem] shrink-0 flex-col border-r border-sidebar-border bg-sidebar xl:flex", compact && "xl:hidden")}>
        <div className="flex h-[58px] shrink-0 items-center gap-2.5 border-b border-border px-4">
          <div className="grid size-8 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm">
            <Users className="size-4" />
          </div>
          <div className="min-w-0">
            <p className="text-[13px] font-semibold tracking-tight">Star Harness</p>
            <p className="text-[10px] text-muted-foreground">Global Work · Agent Teams · Nodes</p>
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
                Active Team
              </p>
              {selectedTeam ? (
                <div className="space-y-0.5">
                  <ContextTreeButton
                    depth={0}
                    icon={<Users className="size-3.5" />}
                    label={selectedTeam.name ?? selectedTeam.id}
                    active={selection.surface === "team" && !selection.memberRunId}
                    onClick={() => onSelectionChange({ surface: "team", teamId: selectedTeam.id, memberRunId: undefined })}
                  />
                  {contextMembers.map((member) => (
                    <ContextTreeButton
                      key={member.id}
                      depth={1}
                      icon={<StatusDot tone={member.status === "blocked" || member.status === "failed" ? "bad" : member.status === "running" ? "running" : member.status === "completed" ? "good" : "idle"} />}
                      label={member.name ?? member.id}
                      active={selection.memberRunId === member.id}
                      onClick={() => onSelectionChange({ surface: "team", teamId: selectedTeam.id, memberRunId: member.id })}
                    />
                  ))}
                </div>
              ) : (
                <p className="px-2.5 py-2 text-[11px] leading-relaxed text-muted-foreground">
                  Open a durable Agent Team to keep its members in reach.
                </p>
              )}
            </section>

          </div>
        </ScrollArea>
      </aside>

      <aside className={cn(
        "hidden h-full shrink-0 flex-col items-center border-r border-sidebar-border bg-sidebar py-3 sm:flex",
        compact ? "member-focus-rail w-20 xl:flex" : "w-16 xl:hidden",
      )}>
        <div className={cn("grid size-9 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm", compact && "member-focus-brand")} aria-label="Star Harness">
          {compact ? <Sparkles className="size-[19px]" /> : <Users className="size-4" />}
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
        {selectedTeam && (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                aria-label={`Active Team: ${selectedTeam.name ?? selectedTeam.id}`}
                onClick={() => onSelectionChange({ surface: "team", teamId: selectedTeam.id, memberRunId: undefined })}
                className="mb-2 grid size-10 place-items-center rounded-lg border border-primary/20 bg-primary/5 text-primary"
              >
                <Users className="size-[18px]" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">{selectedTeam.name ?? selectedTeam.id}</TooltipContent>
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

export function snapshotContentRevision(snapshot: DashboardSnapshot): string {
  const content: Record<string, unknown> = { ...snapshot };
  delete content.generated_at;
  delete content.node_daemon_leases;
  delete content.team_supervisor_leases;
  delete content.live_member_activity;
  return JSON.stringify(content);
}

function SurfaceSwitch({
  model,
  selection,
  onSelectionChange,
  onSelectionReplace,
  sourceLabel,
  actionsEnabled,
  onAction,
  onRoleAction,
  roleActionsCurrent,
  apiUrl,
  projectBindingId,
  executionSpaceId,
  companyId,
  projects,
  isLoading,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  onSelectionReplace: (selection: Partial<SelectionState>) => void;
  sourceLabel: string;
  actionsEnabled: boolean;
  onAction: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>;
  onRoleAction: RoleActionExecutor;
  roleActionsCurrent: boolean;
  apiUrl: string;
  projectBindingId: string;
  executionSpaceId: string;
  companyId: string;
  projects: Project[];
  isLoading: boolean;
}) {
  // Content-derived snapshot revision excludes response generation time,
  // heartbeat-only NodeDaemon/Supervisor leases, and client-only live previews.
  // Those rows can advance without latest_op_seq or durable projection truth;
  // Work, Message, binding, session, registration, and action rows remain.
  const contentRevision = useMemo(
    () => snapshotContentRevision(model.snapshot),
    [model.snapshot],
  );
  const shared = {
    model,
    onSelectionChange,
    actionsEnabled,
    onAction,
    apiUrl,
    projectBindingId,
    executionSpaceId,
  };
  if (selection.surface === "work") {
    return <GlobalWorkIndex apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} refreshKey={contentRevision} selection={selection} onSelectionChange={onSelectionChange} teams={model.snapshot.teams ?? []} />;
  }
  switch (selection.surface) {
    case "team":
      return selection.teamId ? (
        <TeamWorkspace apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} company={companyId} teamId={selection.teamId} refreshKey={contentRevision} selection={selection} onAction={onRoleAction} onSelectionChange={onSelectionChange} onSelectionReplace={onSelectionReplace} />
      ) : selection.memberRunId ? (
        <AgentConversationWorkspace apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} company={companyId} routeIdentity={selection.memberRunId} selection={selection} refreshKey={contentRevision} onAction={onRoleAction} onSelectionChange={onSelectionChange}/>
      ) : (
        <AgentTeamsHome {...shared} loading={isLoading} />
      );
    case "operator": {
      const nodeId = selection.nodeId ?? model.snapshot.execution_nodes?.[0]?.id;
      return nodeId
        ? <OperatorView apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} company={companyId} nodeId={nodeId} onAction={onRoleAction} actionsCurrent={roleActionsCurrent} />
        : <div role={isLoading ? "status" : undefined} className="rounded-xl border border-dashed border-border p-10 text-center text-sm text-muted-foreground">{isLoading ? "Loading Execution Nodes…" : "No Execution Node is registered."}</div>;
    }
    case "providers":
      return <ProvidersSurface />;
    case "projects":
      return <ProjectsSurface projects={projects} selectedProjectId={projectBindingId} />;
    case "settings":
      return <SettingsSurface />;
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
  if (selection.surface === "team") {
    const memberRun = (model.snapshot.member_runs ?? []).find(
      (candidate) => candidate.id === selection.memberRunId,
    );
    const run = (model.snapshot.team_runs ?? []).find(
      (candidate) => candidate.id === (selection.teamId ?? memberRun?.team_run_id),
    );
    const team = (model.snapshot.teams ?? []).find(
      (candidate) => candidate.id === (selection.teamId ?? run?.agent_team_id),
    );
    return memberRun?.name ?? team?.name ?? (team ? "Agent Team" : "Agent Teams");
  }

  switch (selection.surface) {
    case "work":
      return selection.teamWorkId ?? "All Team Work";
    case "operator":
      return selection.nodeId ?? "Machine daemons";
    case "providers":
    case "projects":
    case "settings":
      return "Platform";
    case "agents":
      return "Execution directory";
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
