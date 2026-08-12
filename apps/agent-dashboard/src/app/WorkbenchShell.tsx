import { lazy, Suspense, useState, type ReactNode } from "react";
import {
  BookOpen,
  BriefcaseBusiness,
  Bug,
  Building2,
  CheckCircle2,
  ChevronDown,
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
  ServerCog,
  ShieldAlert,
  Sparkles,
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
import { ProvenanceFooter } from "@/components/workbench/ProvenanceFooter";

import type { WorkbenchModel } from "../model/readModel";
import type { RoleActionExecutor } from "../model/roleViews";
import type { Company, ExecutionSpace, Project } from "../types";
import {
  AgentDetail,
  AgentsList,
  DebugSurface,
} from "../surfaces/Surfaces";
import { WorkflowRunDetail, WorkflowsList } from "../surfaces/Workflows";
import { AgentTeamsHome } from "../surfaces/AgentTeamsHome";
import { MissionsSurface } from "../surfaces/Missions";
import { CompanyWorkIndex } from "../surfaces/CompanyWorkIndex";
import { TeamWorkspace } from "../surfaces/TeamWorkspace";
import { MemberWorkbench } from "../surfaces/MemberWorkbench";
import { OperatorView } from "../surfaces/OperatorView";
import { isCompanyOsSurface, resolveCompanyOsRouteData } from "../company-os/routeMeta";
import { DocsV2Surface } from "../company-os/docs/DocsV2Surface";

/** Company OS page tree is large; keep it out of the initial workbench chunk
 * and load it when a Company OS surface is actually opened. */
const CompanyOsRouter = lazy(() =>
  import("../company-os/CompanyOsRouter").then((module) => ({ default: module.CompanyOsRouter })),
);
import type { SelectionState, SurfaceId } from "./selection";
import { freshnessDomains, type DomainFreshness, type FreshnessDomain } from "./freshness";

interface WorkbenchShellProps {
  apiUrl: string;
  isLoading: boolean;
  model: WorkbenchModel;
  /** Known projects for the header picker (goal-multi-project P6); empty for a
   * single-store / pre-multi-project backend, which hides the picker. */
  projects: Project[];
  /** Independent Mission/Wave/Team/Workflow coordination namespaces. */
  spaces: ExecutionSpace[];
  /** Known Company Stores for the header picker; empty in raw-store mode. */
  companies: Company[];
  /** The currently-selected project id ("" before one is chosen/adopted). */
  selectedProjectId: string;
  selectedSpaceId: string;
  /** The currently-selected Company Store id for Company OS truth. */
  selectedCompanyId: string;
  /** Switch the active project: re-points the scoped snapshot + SSE stream. */
  onSelectProject: (projectId: string) => void;
  onSelectSpace: (spaceId: string) => void;
  /** Switch the active Company Store without changing the execution project. */
  onSelectCompany: (companyId: string) => void;
  onApiUrlChange: (value: string) => void;
  onRefresh: () => void;
  onSelectionChange: (selection: SelectionState) => void;
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
    { id: "operator", label: "Operator", icon: ServerCog },
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
 * - debug: moved behind a TopBar button
 */

export function WorkbenchShell({
  apiUrl,
  isLoading,
  model,
  companies,
  projects,
  spaces,
  selectedCompanyId,
  selectedProjectId,
  selectedSpaceId,
  onSelectCompany,
  onSelectProject,
  onSelectSpace,
  onApiUrlChange,
  onRefresh,
  onSelectionChange,
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
          companies={companies}
          projects={projects}
          spaces={spaces}
          selectedCompanyId={selectedCompanyId}
          selectedProjectId={selectedProjectId}
          selectedSpaceId={selectedSpaceId}
          onSelectCompany={onSelectCompany}
          onSelectProject={onSelectProject}
          onSelectSpace={onSelectSpace}
          onApiUrlChange={onApiUrlChange}
          onRefresh={onRefresh}
          sourceError={sourceError}
          sourceLabel={sourceLabel}
          domainFreshness={domainFreshness}
          prototypeMode={
            isCompanyOsSurface(selection.surface)
            && !(isLoading && actionsEnabled && !model.snapshot.company_os)
            && resolveCompanyOsRouteData(model).mode !== "store-live"
          }
          debugActive={selection.surface === "debug"}
          onToggleDebug={() =>
            updateSelection({ surface: selection.surface === "debug" ? "home" : "debug" })
          }
          pollEnabled={pollEnabled}
          canPoll={canPoll}
          onTogglePoll={onTogglePoll}
        />}
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
                onRoleAction={onRoleAction}
                roleActionsCurrent={roleActionsCurrent}
                apiUrl={apiUrl}
                projectBindingId={selectedProjectId}
                executionSpaceId={selectedSpaceId}
                companyId={selectedCompanyId}
                isLoading={isLoading}
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
  model,
  companies,
  projects,
  spaces,
  selectedCompanyId,
  selectedProjectId,
  selectedSpaceId,
  onSelectCompany,
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
  prototypeMode,
}: Omit<
  WorkbenchShellProps,
  "selection" | "onSelectionChange" | "actionsEnabled" | "onAction" | "onRoleAction"
> & {
  currentSurface: string;
  contextLabel: string;
  debugActive: boolean;
  onToggleDebug: () => void;
  prototypeMode: boolean;
}) {
  // Product freshness is explicit: socket-open alone does not earn Live.
  const transportStreaming = sourceLabel === "Live";
  const transportOnline = ["Live", "Reconnecting", "Stale"].includes(sourceLabel);
  const isStreaming = !prototypeMode && transportStreaming;
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
        <SpacePicker
          spaces={spaces}
          selectedSpaceId={selectedSpaceId}
          onSelectSpace={onSelectSpace}
        />
        <CompanyPicker
          companies={companies}
          selectedCompanyId={selectedCompanyId}
          onSelectCompany={onSelectCompany}
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
          prototypeMode={prototypeMode}
          sourceError={sourceError}
          runtimeLabel={displayedSourceLabel}
          runtimePulse={isStreaming}
        />
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
        <p>Project Binding does not own Mission, Wave, Team, or Workflow storage.</p>
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
            <Target className="size-3.5" />
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
          <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-muted-foreground" />
        </label>
      </TooltipTrigger>
      <TooltipContent className="max-w-[36rem] space-y-1">
        <p><span className="text-muted-foreground">Execution coordination:</span> Mission / Wave / Agent Team / Workflow</p>
        <p><span className="text-muted-foreground">Store:</span> <span className="font-mono">{selected.store_root}</span></p>
        <p><span className="text-muted-foreground">Default binding:</span> <span className="font-mono">{selected.default_project_binding_id ?? "none"}</span></p>
      </TooltipContent>
    </Tooltip>
  );
}

function CompanyPicker({
  companies,
  selectedCompanyId,
  onSelectCompany,
}: {
  companies: Company[];
  selectedCompanyId: string;
  onSelectCompany: (companyId: string) => void;
}) {
  const selected = companies.find((company) => company.id === selectedCompanyId);
  if (!selected) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <label className="relative ml-1 hidden items-center md:flex" aria-label="Company Store">
          <span className="pointer-events-none absolute left-2 text-muted-foreground">
            <Building2 className="size-3.5" />
          </span>
          <select
            aria-label="Active company"
            value={selectedCompanyId}
            disabled={companies.length === 1}
            onChange={(event) => onSelectCompany(event.target.value)}
            className="h-8 max-w-[210px] appearance-none truncate rounded-md border border-border bg-background/50 pl-7 pr-7 text-[11px] text-foreground outline-none transition-colors hover:border-input focus:border-ring disabled:opacity-100"
          >
            {companies.map((company) => (
              <option key={company.id} value={company.id}>
                {companyLabel(company)}
              </option>
            ))}
          </select>
          <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-muted-foreground" />
        </label>
      </TooltipTrigger>
      <TooltipContent className="max-w-[36rem] space-y-1">
        <p>
          <span className="text-muted-foreground">Company truth store:</span>{" "}
          <span className="font-mono">{selected.store_root}</span>
        </p>
        <p>
          <span className="text-muted-foreground">Boundary:</span>{" "}
          Docs / Work / Organization / Finance. Project execution remains selected separately.
        </p>
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

function companyLabel(company: Company): string {
  const name = company.name?.trim();
  return name ? `${name} (${company.id})` : company.id;
}

const freshnessLabels: Record<FreshnessDomain, string> = {
  works: "Works",
  docs: "Docs",
  organization: "Org",
  runtime: "Runtime",
};

function DomainFreshnessStrip({
  freshness,
  prototypeMode,
  runtimeLabel,
  runtimePulse,
  sourceError,
}: {
  freshness: DomainFreshness;
  prototypeMode: boolean;
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
        const status = prototypeMode ? "offline" : freshness[domain];
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
  const selectedRun = (model.snapshot.team_runs ?? []).find((run) => run.id === selection.teamId);
  const selectedTeam = (model.snapshot.teams ?? []).find(
    (team) => team.id === selectedRun?.agent_team_id,
  );
  const missionId = selection.missionId ?? selectedTeam?.mission_id;
  const waveId = selection.waveId ?? (model.snapshot.waves ?? []).find(
    (candidate) => (candidate.executor_run_ids ?? []).includes(selectedRun?.id ?? ""),
  )?.id;
  const mission = (model.snapshot.missions ?? []).find((item) => item.id === missionId);
  const wave = (model.snapshot.waves ?? []).find((item) => item.id === waveId);
  const contextRun = selectedRun ?? (model.snapshot.team_runs ?? []).find(
    (run) =>
      (wave?.executor_run_ids ?? []).includes(run.id)
      && (model.snapshot.teams ?? []).some(
        (team) => team.id === run.agent_team_id && team.mission_id === missionId,
      ),
  );
  const contextMembers = (model.snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id === contextRun?.id,
  );
  const companyContext = isCompanyOsSurface(selection.surface);
  const selectedCompanySurface = navItems.find((item) => item.id === selection.surface);

  function navigate(id: SurfaceId) {
    onSelectionChange({
      surface: id,
      documentId: undefined,
      agentMembershipId: undefined,
      personId: undefined,
      proposalId: undefined,
      approvalId: undefined,
      moduleId: undefined,
      memberId: undefined,
      memberRunId: undefined,
      teamId: undefined,
          workflowRunId: undefined,
          orgView: undefined,
          orgTeamId: undefined,
          orgExpanded: undefined,
          workView: undefined,
          teamWorkId: undefined,
          workTeamId: undefined,
          workHostId: undefined,
          workMemberId: undefined,
          workStatus: undefined,
          workPriority: undefined,
          workSource: undefined,
          workDemand: undefined,
    });
  }

  return (
    <>
      <aside className={cn("hidden h-full w-[14.5rem] shrink-0 flex-col border-r border-sidebar-border bg-sidebar xl:flex", compact && "xl:hidden")}>
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
                {companyContext ? "Company context" : "Active context"}
              </p>
              {companyContext ? (
                <div className="rounded-lg border border-border/70 bg-background/55 px-3 py-3">
                  <p className="text-[11px] font-semibold text-foreground">
                    {selectedCompanySurface?.label ?? "Company OS"}
                  </p>
                  <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">
                    Docs holds context, Organization holds authority, Work holds commitments, and Finance records monetary effects.
                  </p>
                </div>
              ) : mission ? (
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

      <aside className={cn(
        "hidden h-full shrink-0 flex-col items-center border-r border-sidebar-border bg-sidebar py-3 sm:flex",
        compact ? "member-focus-rail w-20 xl:flex" : "w-16 xl:hidden",
      )}>
        <div className={cn("grid size-9 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm", compact && "member-focus-brand")} aria-label="Company OS">
          {compact ? <Sparkles className="size-[19px]" /> : <Building2 className="size-4" />}
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
  onRoleAction,
  roleActionsCurrent,
  apiUrl,
  projectBindingId,
  executionSpaceId,
  companyId,
  isLoading,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  sourceLabel: string;
  actionsEnabled: boolean;
  onAction: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>;
  onRoleAction: RoleActionExecutor;
  roleActionsCurrent: boolean;
  apiUrl: string;
  projectBindingId: string;
  executionSpaceId: string;
  companyId: string;
  isLoading: boolean;
}) {
  const shared = {
    model,
    onSelectionChange,
    actionsEnabled,
    onAction,
    apiUrl,
    projectBindingId,
    executionSpaceId,
  };
  if (selection.surface === "docs-v2" || (selection.surface === "docs" && selection.documentId)) {
    // AI-first Docs v2 (ADR 0054, retirement stage R2): the document focus of
    // both docs surfaces renders through the store-live v2 page endpoint
    // (legacy ledger documents appear as honest read-only legacy
    // projections). No snapshot projection or fixture fallback participates.
    return (
      <DocsV2Surface
        apiUrl={apiUrl}
        selection={selection}
        company={companyId}
        project={projectBindingId}
        space={executionSpaceId}
        onSelectionChange={onSelectionChange}
      />
    );
  }
  if (selection.surface === "work") {
    return <CompanyWorkIndex apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} refreshKey={model.snapshot.generated_at} selection={selection} onSelectionChange={onSelectionChange} />;
  }
  if (isCompanyOsSurface(selection.surface)) {
    const livePending = isLoading || (actionsEnabled && !model.snapshot.company_os);
    return (
      <Suspense fallback={<div className="grid h-full place-items-center text-sm text-muted-foreground">Loading Company OS…</div>}>
        <CompanyOsRouter model={model} selection={selection} actionsEnabled={actionsEnabled} livePending={livePending} snapshotLoading={isLoading} sourceLabel={sourceLabel} onAction={onAction} onSelectionChange={onSelectionChange} />
      </Suspense>
    );
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
      return selection.teamId ? (
        <TeamWorkspace apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} teamId={selection.teamId} refreshKey={model.snapshot.generated_at} selection={selection} onAction={onRoleAction} actionsCurrent={roleActionsCurrent} onSelectionChange={onSelectionChange} />
      ) : selection.memberRunId ? (
        <MemberWorkbench apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} memberRunId={selection.memberRunId} onAction={onRoleAction} actionsCurrent={roleActionsCurrent} />
      ) : (
        <AgentTeamsHome {...shared} />
      );
    case "operator": {
      const nodeId = selection.nodeId ?? model.snapshot.execution_nodes?.[0]?.id;
      return nodeId
        ? <OperatorView key={model.snapshot.generated_at} apiUrl={apiUrl} space={executionSpaceId} project={projectBindingId} company={companyId} nodeId={nodeId} onAction={onRoleAction} actionsCurrent={roleActionsCurrent} />
        : <div className="rounded-xl border border-dashed border-border p-10 text-center text-sm text-muted-foreground">No Execution Node is registered.</div>;
    }
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
    const team = (model.snapshot.teams ?? []).find(
      (candidate) => candidate.id === run?.agent_team_id,
    );
    const mission = team?.mission_id
      ? (model.snapshot.missions ?? []).find((candidate) => candidate.id === team.mission_id)
      : undefined;
    return memberRun?.name ?? mission?.title ?? (run ? "Team attempt" : "Agent Team attempts");
  }

  switch (selection.surface) {
    case "home":
      return "Company attention";
    case "organization":
      if (selection.personId === "actor-human-brand-owner") return "Brand Owner";
      if (selection.agentMembershipId === "actor-agent-document-architecture") return "Document Architecture Agent";
      if (selection.proposalId === "governance-proposal-trademark-management") return "Create Trademark Management module";
      return selection.personId ?? selection.agentMembershipId ?? selection.proposalId ?? "Mixed organization";
    case "work":
      return selection.teamWorkId ?? "Company work";
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
      return selection.documentId ?? selection.moduleId ?? "Operating knowledge";
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
