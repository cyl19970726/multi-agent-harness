import {
  Activity,
  BookOpen,
  Bot,
  Bug,
  ClipboardList,
  Gavel,
  GitBranch,
  Inbox,
  MessageSquare,
  PanelRightOpen,
  RefreshCw,
  Search,
  Send,
  ShieldAlert,
  Target,
  Users,
  Workflow,
} from "lucide-react";
import type { SurfaceId, SelectionState } from "./selection";
import type { WorkbenchModel } from "../model/readModel";
import { countBySeverity, memberName, objectShortId, taskTitle } from "../model/readModel";
import { ActionButton, IconButton, StatusBadge, TimelineRow } from "../ui/primitives";
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

  const severityCounts = countBySeverity(model.warnings);

  return (
    <main className={`workbenchShell surface-${selection.surface}`}>
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
      <AppRail selection={selection} onSurfaceChange={(surface) => updateSelection({ surface })} warnings={severityCounts.high} />
      <TeamRail model={model} selection={selection} onSelectionChange={updateSelection} />
      <section className="workspace" aria-label="Primary workbench workspace">
        <SurfaceSwitch model={model} selection={selection} onSelectionChange={updateSelection} sourceLabel={sourceLabel} />
      </section>
      <Inspector model={model} onSelectionChange={updateSelection} />
      <MobileNav selection={selection} onSurfaceChange={(surface) => updateSelection({ surface })} />
    </main>
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
}: Omit<WorkbenchShellProps, "selection" | "onSelectionChange"> & { currentSurface: string }) {
  return (
    <header className="topBar">
      <div className="brandBlock">
        <strong>Agent Workbench</strong>
        <span>{currentSurface} · {model.selectedGoal?.title ?? "No active goal"}</span>
      </div>
      <div className="sourceState">
        <StatusBadge tone={sourceError ? "warn" : sourceLabel.includes("live") ? "good" : "info"}>{sourceLabel}</StatusBadge>
        {sourceError && <span className="sourceError">{sourceError}</span>}
      </div>
      <label className="apiControl" htmlFor="api-url">
        <span>Harness API</span>
        <input id="api-url" value={apiUrl} onChange={(event) => onApiUrlChange(event.target.value)} spellCheck={false} />
      </label>
      <ActionButton icon={RefreshCw} disabled={isLoading} onClick={onRefresh} tone="primary">
        {isLoading ? "Loading" : "Load live"}
      </ActionButton>
      <div className="searchGhost" aria-label="Search placeholder">
        <Search size={15} aria-hidden="true" />
        <span>Search objects</span>
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
    <nav className="appRail" aria-label="Workbench navigation">
      <div className="railMark">AW</div>
      {navItems.map((item) => (
        <IconButton
          key={item.id}
          active={selection.surface === item.id}
          icon={item.icon}
          label={item.label}
          onClick={() => onSurfaceChange(item.id)}
        />
      ))}
      {warnings > 0 && <span className="railWarning">{warnings}</span>}
    </nav>
  );
}

function TeamRail({
  model,
  selection,
  onSelectionChange,
}: {
  model: WorkbenchModel;
  selection: SelectionState;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  return (
    <aside className="teamRail" aria-label="Team and member rail">
      <header className="railHeader">
        <span>Team</span>
        <strong>{model.selectedTeam?.name ?? "No team"}</strong>
      </header>
      <div className="goalRailSummary">
        <span>Active goal</span>
        <strong>{model.selectedGoal?.title ?? "Missing goal"}</strong>
        <small>{model.goalTasks.length} tasks · {model.warnings.length} warnings</small>
      </div>
      <div className="roleGroups">
        {model.roleGroups.map((group) => (
          <section key={group.role} className="roleGroup">
            <h2>{group.role}</h2>
            {group.members.map((member) => (
              <button
                key={member.id}
                type="button"
                className={`memberRow${selection.memberId === member.id || model.selectedMember?.id === member.id ? " active" : ""}`}
                onClick={() => onSelectionChange({ memberId: member.id, taskId: member.current_task_id ?? selection.taskId, surface: "member" })}
              >
                <span className="avatar">{initials(member.name ?? member.id)}</span>
                <span className="memberText">
                  <strong>{member.name ?? member.id}</strong>
                  <small>{member.runtime_status ?? member.status ?? "unknown"} · {taskTitle(model.tasks, member.current_task_id)}</small>
                </span>
                <span className="queueCount">{(member.inbox_count ?? 0) + (member.queued_count ?? 0)}</span>
              </button>
            ))}
          </section>
        ))}
      </div>
      <footer className="railQueue">
        <span>Decision pressure</span>
        <strong>{model.decisionQueue.length}</strong>
        <small>reviews, waivers, and missing proof</small>
      </footer>
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
      return <GraphKanban model={model} mode={selection.mode ?? "kanban"} onSelectionChange={onSelectionChange} />;
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

function Inspector({ model, onSelectionChange }: { model: WorkbenchModel; onSelectionChange: (selection: Partial<SelectionState>) => void }) {
  const member = model.selectedMember;
  const task = model.selectedTask;
  return (
    <aside className="inspector" aria-label="Selected object inspector">
      <header className="inspectorHeader">
        <PanelRightOpen size={17} aria-hidden="true" />
        <div>
          <span>{member ? "Selected member" : task ? "Selected task" : "Inspector"}</span>
          <strong>{member?.name ?? task?.title ?? "Nothing selected"}</strong>
        </div>
      </header>
      {member && (
        <section className="inspectorBlock">
          <p className="blockLabel">Member identity</p>
          <h2>{member.name ?? member.id}</h2>
          <div className="inlineBadges">
            <StatusBadge tone={member.runtime_alive ? "good" : "warn"}>{member.runtime_status ?? "unknown"}</StatusBadge>
            <StatusBadge tone="info">{member.role ?? "Member"}</StatusBadge>
          </div>
          <p>{member.description ?? "Persistent AgentMember with role, prompt, runtime state, inbox/outbox, and current task."}</p>
          <div className="inspectorActions">
            <ActionButton icon={Send} tone="primary">Send message</ActionButton>
            <ActionButton icon={Inbox}>Deliver queued</ActionButton>
          </div>
        </section>
      )}
      {task && (
        <section className="inspectorBlock">
          <p className="blockLabel">Current task</p>
          <button type="button" className="linkButton" onClick={() => onSelectionChange({ surface: "task", taskId: task.id })}>
            {task.title ?? task.id}
          </button>
          <p>{task.objective}</p>
          <div className="inlineBadges">
            <StatusBadge tone={task.status === "done" ? "good" : task.status === "blocked" ? "bad" : "info"}>{task.status}</StatusBadge>
            <StatusBadge>{objectShortId(task.branch_ref)}</StatusBadge>
          </div>
        </section>
      )}
      <section className="inspectorBlock">
        <p className="blockLabel">Inbox / outbox</p>
        <div className="splitMetrics">
          <span><strong>{member?.inbox_count ?? 0}</strong> inbox</span>
          <span><strong>{member?.queued_count ?? 0}</strong> queued</span>
        </div>
      </section>
      <section className="inspectorBlock">
        <p className="blockLabel">Recent member activity</p>
        <div className="compactTimeline">
          {model.selectedMemberTimeline.slice(0, 4).map((item) => (
            <TimelineRow key={item.id} kind={item.kind} title={item.title} meta={item.meta} body={item.body} severity={item.severity} />
          ))}
        </div>
      </section>
    </aside>
  );
}

function MobileNav({ selection, onSurfaceChange }: { selection: SelectionState; onSurfaceChange: (surface: SurfaceId) => void }) {
  return (
    <nav className="mobileNav" aria-label="Mobile Workbench tabs">
      {navItems.map((item) => (
        <button
          key={item.id}
          type="button"
          className={selection.surface === item.id ? "active" : ""}
          onClick={() => onSurfaceChange(item.id)}
        >
          <item.icon size={16} aria-hidden="true" />
          <span>{item.label}</span>
        </button>
      ))}
    </nav>
  );
}

function initials(value: string): string {
  return value
    .split(/[-_\s]/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

function surfaceLabel(surface: SurfaceId): string {
  return navItems.find((item) => item.id === surface)?.label ?? surface;
}
