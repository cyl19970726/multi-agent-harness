import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import {
  Activity,
  AlertTriangle,
  BookOpen,
  Bug,
  CircleDot,
  Columns3,
  Database,
  FileText,
  GitBranch,
  Inbox,
  MessageSquare,
  Network,
  PanelRight,
  Pause,
  Play,
  Radio,
  RefreshCw,
  Search,
  Send,
  ShieldCheck,
  Target,
  Terminal,
  UserRound,
  Users,
  Workflow,
  X,
} from "lucide-react";
import { fetchSnapshot, postAction } from "./api";
import {
  activeVisionContext,
  childThreadsForMember,
  decisionQueue,
  docsContext,
  goalCollection,
  goalDocument,
  graphKanbanModel,
  inboxForMember,
  memberTimeline,
  memberWorkbench,
  normalizeSnapshot,
  outboxForMember,
  sessionsForMember,
  taskDocument,
  teamWorkspace,
  warningsForScope,
  type DocsContextItem,
  type TimelineItem,
} from "./readModel";
import type {
  AgentMember,
  AgentTeam,
  DashboardAction,
  DashboardSnapshot,
  Decision,
  Evidence,
  Goal,
  Message,
  Proposal,
  ProviderChildThread,
  ProviderSession,
  Task,
  TaskStatus,
  WorkflowWarning,
} from "./types";
import { deriveWarnings } from "./warnings";

type SurfaceKey = "team" | "work" | "member" | "warnings" | "docs" | "decisions" | "debug";
type InspectorTab = "member" | "task" | "docs" | "evidence" | "warnings" | "decision";
type WorkMode = "kanban" | "graph";
type Tone = "neutral" | "good" | "warn" | "bad" | "info";

const surfaces: Array<{ key: SurfaceKey; label: string; icon: ReactNode }> = [
  { key: "team", label: "Team", icon: <Users size={18} /> },
  { key: "work", label: "Work", icon: <Workflow size={18} /> },
  { key: "member", label: "Member", icon: <UserRound size={18} /> },
  { key: "warnings", label: "Warn", icon: <AlertTriangle size={18} /> },
  { key: "docs", label: "Docs", icon: <BookOpen size={18} /> },
  { key: "decisions", label: "Decisions", icon: <ShieldCheck size={18} /> },
  { key: "debug", label: "Debug", icon: <Bug size={18} /> },
];

const taskStatuses: TaskStatus[] = ["planned", "assigned", "running", "blocked", "review", "done", "archived"];

export function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [jsonInput, setJsonInput] = useState("");
  const [liveUrl, setLiveUrl] = useState("http://127.0.0.1:8787");
  const [isLive, setIsLive] = useState(true);
  const [debugOpen, setDebugOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionStatus, setActionStatus] = useState<string | null>(null);
  const [activeSurface, setActiveSurface] = useState<SurfaceKey>("team");
  const [workMode, setWorkMode] = useState<WorkMode>("kanban");
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("member");
  const [selectedGoalId, setSelectedGoalId] = useState<string | undefined>();
  const [selectedTaskId, setSelectedTaskId] = useState<string | undefined>();
  const [selectedMemberId, setSelectedMemberId] = useState<string | undefined>();
  const [selectedTeamId, setSelectedTeamId] = useState<string | undefined>();

  const view = useMemo(() => normalizeSnapshot(snapshot), [snapshot]);
  const warnings = useMemo(() => deriveWarnings(view), [view]);
  const vision = useMemo(() => activeVisionContext(view, selectedGoalId), [view, selectedGoalId]);
  const goalBuckets = useMemo(() => goalCollection(view), [view]);
  const activeGoal = vision.selectedGoal;
  const teamModel = useMemo(
    () => teamWorkspace(view, selectedTeamId, activeGoal?.id),
    [view, selectedTeamId, activeGoal?.id],
  );
  const selectedTeam = teamModel.team;
  const goalDoc = useMemo(() => goalDocument(view, activeGoal?.id), [view, activeGoal?.id]);
  const selectedTask =
    goalDoc.tasks.find((task) => task.id === selectedTaskId) ??
    preferredTask(goalDoc.tasks) ??
    goalDoc.tasks[0] ??
    view.tasks[0];
  const taskDoc = useMemo(() => taskDocument(view, selectedTask?.id), [view, selectedTask?.id]);
  const selectedMember =
    memberWorkbench(view, selectedMemberId) ??
    memberForTask(view.members, selectedTask) ??
    teamModel.members[0] ??
    view.members[0];
  const scopedWarnings = useMemo(
    () => warningsForScope(warnings, activeGoal?.id, goalDoc.tasks, teamModel.members),
    [warnings, activeGoal?.id, goalDoc.tasks, teamModel.members],
  );
  const graphModel = useMemo(() => graphKanbanModel(view, activeGoal?.id), [view, activeGoal?.id]);
  const docs = useMemo(
    () => docsContext(view, selectedTask?.id ?? activeGoal?.id ?? selectedMember?.id),
    [view, selectedTask?.id, activeGoal?.id, selectedMember?.id],
  );
  const decisions = useMemo(() => decisionQueue(view, activeGoal?.id), [view, activeGoal?.id]);

  useEffect(() => {
    if (!isLive) return;
    let cancelled = false;
    async function load() {
      try {
        const next = await fetchSnapshot(liveUrl);
        if (!cancelled) {
          setSnapshot(next);
          setError(null);
        }
      } catch (loadError) {
        if (!cancelled) {
          setError(loadError instanceof Error ? loadError.message : String(loadError));
          setIsLive(false);
        }
      }
    }
    load();
    const timer = window.setInterval(load, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isLive, liveUrl]);

  useEffect(() => {
    if (!selectedGoalId && view.goals[0]) setSelectedGoalId(view.goals[0].id);
    if (!selectedTeamId && view.teams[0]) setSelectedTeamId(view.teams[0].id);
    if (!selectedTaskId && selectedTask) setSelectedTaskId(selectedTask.id);
    if (!selectedMemberId && selectedMember) setSelectedMemberId(selectedMember.id);
  }, [selectedGoalId, selectedTeamId, selectedTaskId, selectedMemberId, selectedTask, selectedMember, view.goals, view.teams]);

  function loadJson(raw: string) {
    try {
      setSnapshot(JSON.parse(raw) as DashboardSnapshot);
      setError(null);
      setIsLive(false);
      setActionStatus("Offline snapshot loaded");
    } catch (parseError) {
      setError(parseError instanceof Error ? parseError.message : String(parseError));
    }
  }

  const runAction: DashboardAction = async (path: string, body: unknown = {}) => {
    try {
      const response = await postAction(liveUrl, path, body);
      if (response.snapshot) setSnapshot(response.snapshot);
      setActionStatus("Action completed");
      setError(null);
    } catch (actionError) {
      setActionStatus(null);
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    }
  };

  function selectMember(memberId: string) {
    setSelectedMemberId(memberId);
    setInspectorTab("member");
  }

  function selectTask(taskId: string) {
    setSelectedTaskId(taskId);
    setInspectorTab("task");
  }

  return (
    <div className="workbenchShell">
      <WorkbenchTopBar
        liveUrl={liveUrl}
        isLive={isLive}
        generatedAt={view.generated_at}
        selectedGoal={activeGoal}
        error={error}
        actionStatus={actionStatus}
        debugOpen={debugOpen}
        onLiveUrlChange={setLiveUrl}
        onStartLive={() => setIsLive(true)}
        onStopLive={() => setIsLive(false)}
        onToggleDebug={() => setDebugOpen((next) => !next)}
      />

      <div className="shellBody">
        <AppRail activeSurface={activeSurface} onSelectSurface={setActiveSurface} />
        <TeamRail
          teams={view.teams}
          selectedTeam={selectedTeam}
          model={teamModel}
          selectedMemberId={selectedMember?.id}
          selectedTaskId={selectedTask?.id}
          warnings={scopedWarnings}
          onSelectTeam={setSelectedTeamId}
          onSelectMember={selectMember}
          onSelectTask={selectTask}
        />

        <main className="workspaceFrame">
          <VisionGoalStrip
            vision={vision}
            goalBuckets={goalBuckets}
            selectedGoal={activeGoal}
            warnings={scopedWarnings}
            onSelectGoal={setSelectedGoalId}
          />
          <SurfaceTabs activeSurface={activeSurface} onSelectSurface={setActiveSurface} />
          <div className="workspaceScroll">
            {activeSurface === "team" && (
              <TeamSurface
                model={teamModel}
                goal={activeGoal}
                selectedMemberId={selectedMember?.id}
                selectedTaskId={selectedTask?.id}
                warnings={scopedWarnings}
                decisions={decisions}
                onSelectMember={selectMember}
                onSelectTask={selectTask}
                onOpenWarnings={() => setActiveSurface("warnings")}
              />
            )}
            {activeSurface === "work" && (
              <WorkSurface
                goalDoc={goalDoc}
                taskDoc={taskDoc}
                graphModel={graphModel}
                selectedTaskId={selectedTask?.id}
                workMode={workMode}
                onWorkModeChange={setWorkMode}
                onSelectTask={selectTask}
                onSelectMember={selectMember}
              />
            )}
            {activeSurface === "member" && (
              <AgentMemberWorkbench
                snapshot={view}
                member={selectedMember}
                timeline={memberTimeline(view, selectedMember?.id)}
                onAction={runAction}
                onSelectTask={selectTask}
              />
            )}
            {activeSurface === "warnings" && (
              <WarningsSurface warnings={scopedWarnings} onSelectTask={selectTask} onSelectMember={selectMember} />
            )}
            {activeSurface === "docs" && <DocsSurface docs={docs} selectedObject={selectedTask?.id ?? activeGoal?.id} />}
            {activeSurface === "decisions" && <DecisionsSurface decisions={decisions} evidence={goalDoc.evidence} />}
            {activeSurface === "debug" && (
              <DebugSurface
                snapshot={view}
                jsonInput={jsonInput}
                onJsonInputChange={setJsonInput}
                onLoadJson={loadJson}
              />
            )}
          </div>
        </main>

        <Inspector
          activeTab={activeSurface === "member" && inspectorTab === "member" ? "task" : inspectorTab}
          onSelectTab={setInspectorTab}
          snapshot={view}
          member={selectedMember}
          taskDoc={taskDoc}
          docs={docs}
          warnings={scopedWarnings}
          decisions={decisions}
          onAction={runAction}
          onSelectTask={selectTask}
          onSelectMember={selectMember}
        />
      </div>

      <DebugDrawer
        open={debugOpen}
        snapshot={view}
        jsonInput={jsonInput}
        error={error}
        onClose={() => setDebugOpen(false)}
        onJsonInputChange={setJsonInput}
        onLoadJson={loadJson}
      />
    </div>
  );
}

function WorkbenchTopBar({
  liveUrl,
  isLive,
  generatedAt,
  selectedGoal,
  error,
  actionStatus,
  debugOpen,
  onLiveUrlChange,
  onStartLive,
  onStopLive,
  onToggleDebug,
}: {
  liveUrl: string;
  isLive: boolean;
  generatedAt?: string;
  selectedGoal?: Goal;
  error: string | null;
  actionStatus: string | null;
  debugOpen: boolean;
  onLiveUrlChange: (value: string) => void;
  onStartLive: () => void;
  onStopLive: () => void;
  onToggleDebug: () => void;
}) {
  return (
    <header className="topBar">
      <div className="brandBlock">
        <div className="brandMark"><Network size={18} /></div>
        <div>
          <h1>Agent Workbench</h1>
          <p>{selectedGoal?.title || "No selected goal"}</p>
        </div>
      </div>
      <div className="topContext">
        <StatusPill tone={isLive ? "good" : "warn"} icon={<Radio size={12} />}>{isLive ? "live" : "offline"}</StatusPill>
        <span className="topMeta">{formatStamp(generatedAt) || "no snapshot"}</span>
        {error && <span className="topError">{error}</span>}
        {actionStatus && !error && <span className="topOk">{actionStatus}</span>}
      </div>
      <div className="topControls">
        <label className="urlControl">
          <span>API</span>
          <input value={liveUrl} onChange={(event) => onLiveUrlChange(event.target.value)} />
        </label>
        <div className="searchShell">
          <Search size={14} />
          <input placeholder="Search objects" aria-label="Search objects" />
        </div>
        <button className="iconTextButton" type="button" onClick={isLive ? onStopLive : onStartLive}>
          {isLive ? <Pause size={15} /> : <Play size={15} />}
          <span>{isLive ? "Pause" : "Live"}</span>
        </button>
        <button className={`iconButton ${debugOpen ? "active" : ""}`} type="button" title="Debug" onClick={onToggleDebug}>
          <Terminal size={17} />
        </button>
      </div>
    </header>
  );
}

function AppRail({
  activeSurface,
  onSelectSurface,
}: {
  activeSurface: SurfaceKey;
  onSelectSurface: (surface: SurfaceKey) => void;
}) {
  return (
    <nav className="appRail" aria-label="Workbench surfaces">
      {surfaces.map((surface) => (
        <button
          key={surface.key}
          className={`railIcon ${activeSurface === surface.key ? "active" : ""}`}
          type="button"
          title={surface.label}
          aria-label={surface.label}
          data-surface={surface.key}
          onClick={() => onSelectSurface(surface.key)}
        >
          {surface.icon}
          <span>{surface.label}</span>
        </button>
      ))}
    </nav>
  );
}

function TeamRail({
  teams,
  selectedTeam,
  model,
  selectedMemberId,
  selectedTaskId,
  warnings,
  onSelectTeam,
  onSelectMember,
  onSelectTask,
}: {
  teams: AgentTeam[];
  selectedTeam?: AgentTeam;
  model: ReturnType<typeof teamWorkspace>;
  selectedMemberId?: string;
  selectedTaskId?: string;
  warnings: WorkflowWarning[];
  onSelectTeam: (teamId: string) => void;
  onSelectMember: (memberId: string) => void;
  onSelectTask: (taskId: string) => void;
}) {
  return (
    <aside className="teamRail">
      <div className="railHeader">
        <div>
          <span className="eyebrow">Standing team</span>
          <h2>{selectedTeam?.name || "No team"}</h2>
        </div>
        <StatusPill tone={warnings.length ? "warn" : "good"}>{warnings.length} warn</StatusPill>
      </div>
      <div className="teamSwitch">
        {teams.map((team) => (
          <button
            className={selectedTeam?.id === team.id ? "selected" : ""}
            key={team.id}
            type="button"
            onClick={() => onSelectTeam(team.id)}
          >
            {team.name || shortId(team.id)}
          </button>
        ))}
      </div>
      <div className="railSection">
        {model.roleGroups.map((group) => (
          <div className="roleGroup" key={group.role}>
            <div className="roleGroupTitle">
              <span>{group.role}</span>
              <strong>{group.members.length}</strong>
            </div>
            {group.members.map((member) => (
              <button
                key={member.id}
                type="button"
                className={`memberRow ${selectedMemberId === member.id ? "active" : ""}`}
                onClick={() => onSelectMember(member.id)}
              >
                <Avatar label={member.name || member.id} />
                <span className="memberMain">
                  <strong>{member.name || shortId(member.id)}</strong>
                  <span>{memberStatusSummary(member)}</span>
                </span>
                <StatusDot tone={statusTone(member.runtime_status || member.status)} />
              </button>
            ))}
          </div>
        ))}
        {!model.members.length && <EmptyBlock title="No members" detail="No AgentMember records in the snapshot." />}
      </div>
      <div className="railSection compactTasks">
        <div className="roleGroupTitle">
          <span>Current tasks</span>
          <strong>{model.tasks.length}</strong>
        </div>
        {model.tasks.slice(0, 6).map((task) => (
          <button
            key={task.id}
            type="button"
            className={`taskMini ${selectedTaskId === task.id ? "active" : ""}`}
            onClick={() => onSelectTask(task.id)}
          >
            <StatusPill tone={taskTone(task.status)}>{task.status}</StatusPill>
            <span>{task.title || shortId(task.id)}</span>
          </button>
        ))}
      </div>
    </aside>
  );
}

function VisionGoalStrip({
  vision,
  goalBuckets,
  selectedGoal,
  warnings,
  onSelectGoal,
}: {
  vision: ReturnType<typeof activeVisionContext>;
  goalBuckets: ReturnType<typeof goalCollection>;
  selectedGoal?: Goal;
  warnings: WorkflowWarning[];
  onSelectGoal: (goalId: string) => void;
}) {
  const activeGoals = [...goalBuckets.active, ...goalBuckets.blocked, ...goalBuckets.proposed];
  return (
    <section className="visionStrip">
      <div className="visionPrimary">
        <Target size={19} />
        <div>
          <span className="eyebrow">Vision</span>
          <h2>{vision.title}</h2>
          <p>{vision.distanceLabel}</p>
        </div>
      </div>
      <div className="mobileGoalSummary">
        <strong>{selectedGoal?.title || "No selected goal"}</strong>
        <span>{selectedGoal?.status || "active"} / {vision.completedGoals} complete / {vision.incompleteGoals} open / {warnings.length} warn</span>
      </div>
      <div className="goalLadder">
        {activeGoals.slice(0, 4).map((goal) => (
          <button
            key={goal.id}
            type="button"
            className={selectedGoal?.id === goal.id ? "selected" : ""}
            onClick={() => onSelectGoal(goal.id)}
          >
            <span>{goal.title || shortId(goal.id)}</span>
            <StatusPill tone={goal.status?.includes("block") ? "bad" : "info"}>{goal.status || "active"}</StatusPill>
          </button>
        ))}
        <div className="goalCounts">
          <span>{vision.completedGoals} complete</span>
          <span>{vision.incompleteGoals} open</span>
          <span>{warnings.length} warnings</span>
        </div>
      </div>
    </section>
  );
}

function SurfaceTabs({
  activeSurface,
  onSelectSurface,
}: {
  activeSurface: SurfaceKey;
  onSelectSurface: (surface: SurfaceKey) => void;
}) {
  return (
    <nav className="surfaceTabs" aria-label="Active surface">
      {surfaces.map((surface) => (
        <button
          key={surface.key}
          type="button"
          className={activeSurface === surface.key ? "active" : ""}
          data-surface={surface.key}
          onClick={() => onSelectSurface(surface.key)}
        >
          {surface.icon}
          <span>{surface.label}</span>
        </button>
      ))}
    </nav>
  );
}

function TeamSurface({
  model,
  goal,
  selectedMemberId,
  selectedTaskId,
  warnings,
  decisions,
  onSelectMember,
  onSelectTask,
  onOpenWarnings,
}: {
  model: ReturnType<typeof teamWorkspace>;
  goal?: Goal;
  selectedMemberId?: string;
  selectedTaskId?: string;
  warnings: WorkflowWarning[];
  decisions: Decision[];
  onSelectMember: (memberId: string) => void;
  onSelectTask: (taskId: string) => void;
  onOpenWarnings: () => void;
}) {
  return (
    <div className="surfaceGrid teamSurface">
      <section className="primaryPanel">
        <PanelHeader
          icon={<Users size={18} />}
          title={model.team?.name || "Team workspace"}
          meta={`${model.members.length} members / ${model.messages.length} messages`}
        />
        <div className="teamStats">
          <Metric label="Goal status" value={goal?.status || "active"} />
          <Metric label="Running" value={String(model.tasks.filter((task) => task.status === "running").length)} />
          <Metric label="Blocked" value={String(model.tasks.filter((task) => task.status === "blocked").length)} tone="bad" />
          <Metric label="Queued" value={String(model.messages.filter((message) => message.delivery_status === "queued").length)} tone="warn" />
        </div>
        <div className="memberBoard">
          {model.members.map((member) => (
            <button
              className={`memberCard ${member.id === selectedMemberId ? "active" : ""}`}
              key={member.id}
              type="button"
              onClick={() => onSelectMember(member.id)}
            >
              <div className="memberCardTop">
                <Avatar label={member.name || member.id} />
                <StatusPill tone={statusTone(member.runtime_status || member.status)}>
                  {member.runtime_status || member.status || "idle"}
                </StatusPill>
              </div>
              <strong>{member.name || shortId(member.id)}</strong>
              <span>{member.role || member.provider_agent_role || "AgentMember"}</span>
              <span>member: {member.status || "unknown"}</span>
              <span>runtime: {member.runtime_status || "offline"}</span>
              <span className="mono">{member.current_task_id || "no current task"}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="secondaryPanel">
        <PanelHeader icon={<Activity size={18} />} title="Activity" meta="latest messages" />
        <ActivityList items={model.activity} onSelectTask={onSelectTask} />
      </section>

      <section className="widePanel">
        <PanelHeader icon={<Columns3 size={18} />} title="Work lanes" meta="Kanban projection" />
        <CompactLanes tasks={model.tasks} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} />
      </section>

      <section className="queuePanel">
        <PanelHeader icon={<ShieldCheck size={18} />} title="Decisions" meta={`${decisions.length} recent`} />
        <DecisionList decisions={decisions.slice(0, 4)} />
      </section>

      <section className="queuePanel">
        <PanelHeader icon={<AlertTriangle size={18} />} title="Warnings" meta={`${warnings.length} scoped`} />
        <WarningList warnings={warnings.slice(0, 4)} onSelectTask={onSelectTask} onSelectMember={onSelectMember} />
        <button className="textButton" type="button" onClick={onOpenWarnings}>Open warnings</button>
      </section>
    </div>
  );
}

function WorkSurface({
  goalDoc,
  taskDoc,
  graphModel,
  selectedTaskId,
  workMode,
  onWorkModeChange,
  onSelectTask,
  onSelectMember,
}: {
  goalDoc: ReturnType<typeof goalDocument>;
  taskDoc: ReturnType<typeof taskDocument>;
  graphModel: ReturnType<typeof graphKanbanModel>;
  selectedTaskId?: string;
  workMode: WorkMode;
  onWorkModeChange: (mode: WorkMode) => void;
  onSelectTask: (taskId: string) => void;
  onSelectMember: (memberId: string) => void;
}) {
  return (
    <div className="workSurface">
      <div className="documentColumn">
        <GoalDocumentPanel model={goalDoc} />
        <TaskDocumentPanel model={taskDoc} onSelectMember={onSelectMember} />
      </div>
      <div className="graphColumn">
        <div className="modeSwitch">
          <button className={workMode === "kanban" ? "active" : ""} type="button" onClick={() => onWorkModeChange("kanban")}>
            <Columns3 size={15} /> Kanban
          </button>
          <button className={workMode === "graph" ? "active" : ""} type="button" onClick={() => onWorkModeChange("graph")}>
            <GitBranch size={15} /> Graph
          </button>
        </div>
        {workMode === "kanban" ? (
          <KanbanLanes columns={graphModel.columns} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} />
        ) : (
          <GraphCanvas model={graphModel} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} />
        )}
      </div>
    </div>
  );
}

function AgentMemberWorkbench({
  snapshot,
  member,
  timeline,
  onAction,
  onSelectTask,
}: {
  snapshot: Required<DashboardSnapshot>;
  member?: AgentMember;
  timeline: TimelineItem[];
  onAction: DashboardAction;
  onSelectTask: (taskId: string) => void;
}) {
  const [message, setMessage] = useState("");
  if (!member) return <EmptyBlock title="No AgentMember" detail="The snapshot has no member records." />;

  const inbox = inboxForMember(snapshot, member.id);
  const outbox = outboxForMember(snapshot, member.id);
  const sessions = sessionsForMember(snapshot, member.id);
  const childThreads = childThreadsForMember(snapshot, member.id);
  const closed = ["closing", "closed", "retired"].includes(member.status ?? "");

  return (
    <div className="memberWorkbench">
      <section className="primaryPanel memberHero">
        <div className="memberIdentity">
          <Avatar label={member.name || member.id} large />
          <div>
            <span className="eyebrow">AgentMember</span>
            <h2>{member.name || member.id}</h2>
            <p>{member.description || member.role || member.provider_agent_role || "Persistent harness member"}</p>
          </div>
        </div>
        <div className="memberActionRow">
          <button type="button" disabled={closed} onClick={() => onAction(`/v1/agents/${member.id}/deliver`, { start_runtime: true })}>
            <RefreshCw size={15} /> Deliver
          </button>
          <button type="button" disabled={closed} onClick={() => onAction(`/v1/agents/${member.id}/close`, {})}>
            <X size={15} /> Close
          </button>
        </div>
        <div className="runtimeGrid">
          <Metric label="Runtime" value={member.runtime_status || member.status || "offline"} tone={statusTone(member.runtime_status || member.status)} />
          <Metric label="Queue" value={String(member.queued_count ?? inbox.filter((item) => item.delivery_status === "queued").length)} tone="warn" />
          <Metric label="Inbox" value={String(inbox.length)} />
          <Metric label="Outbox" value={String(outbox.length)} />
        </div>
        <form
          className="composer"
          onSubmit={(event) => {
            event.preventDefault();
            const content = message.trim();
            if (!content) return;
            setMessage("");
            onAction("/v1/messages", {
              from_agent_id: "dashboard",
              to_agent_id: member.id,
              channel: "dashboard-direct",
              kind: "message",
              content,
            });
          }}
        >
          <input
            value={message}
            disabled={closed}
            placeholder="Send a direct message"
            onChange={(event) => setMessage(event.target.value)}
          />
          <button type="submit" disabled={closed || !message.trim()}>
            <Send size={15} /> Send
          </button>
        </form>
      </section>

      <section className="secondaryPanel">
        <PanelHeader icon={<Activity size={18} />} title="Timeline" meta={`${timeline.length} events`} />
        <ActivityList items={timeline} onSelectTask={onSelectTask} />
      </section>

      <section className="widePanel">
        <PanelHeader icon={<Inbox size={18} />} title="Inbox / Outbox" meta={`${inbox.length + outbox.length} messages`} />
        <div className="splitList">
          <MessageColumn title="Inbox" messages={inbox} onSelectTask={onSelectTask} />
          <MessageColumn title="Outbox" messages={outbox} onSelectTask={onSelectTask} />
        </div>
      </section>

      <section className="secondaryPanel">
        <PanelHeader icon={<Terminal size={18} />} title="Runtime health" meta={member.runtime_id || "no runtime"} />
        <RuntimeHealth member={member} sessions={sessions} childThreads={childThreads} />
      </section>
    </div>
  );
}

function GoalDocumentPanel({ model }: { model: ReturnType<typeof goalDocument> }) {
  const status = model.goal?.status || "active";
  return (
    <section className="docPanel">
      <PanelHeader icon={<Target size={18} />} title={model.goal?.title || "Goal document"} meta={status} />
      <p className="docObjective">{model.goal?.objective || "No objective recorded."}</p>
      <div className="proofStrip">
        <ProofChip label="GoalDesign" value={String(model.evidence.filter((item) => item.source_type === "goal_design").length)} />
        <ProofChip label="Tasks" value={String(model.tasks.length)} />
        <ProofChip label="Evidence" value={String(model.evidence.length)} />
        <ProofChip label="Decisions" value={String(model.decisions.length)} />
      </div>
      <Checklist items={model.goal?.success_criteria ?? []} emptyLabel="No success criteria recorded." />
    </section>
  );
}

function TaskDocumentPanel({
  model,
  onSelectMember,
}: {
  model: ReturnType<typeof taskDocument>;
  onSelectMember: (memberId: string) => void;
}) {
  const task = model.task;
  return (
    <section className="docPanel">
      <PanelHeader icon={<FileText size={18} />} title={task?.title || "Task document"} meta={task?.status || "none"} />
      <p className="docObjective">{task?.objective || "No task selected."}</p>
      {task && (
        <div className="taskMetaGrid">
            <MetaButton label="Assignee" value={task.assignee_agent_id} onClick={task.assignee_agent_id ? () => onSelectMember(task.assignee_agent_id!) : undefined} />
            <MetaButton label="Reviewer" value={task.reviewer_agent_id} onClick={task.reviewer_agent_id ? () => onSelectMember(task.reviewer_agent_id!) : undefined} />
          <MetaValue label="Branch" value={task.branch_ref} />
          <MetaValue label="Worktree" value={task.workspace_ref} />
        </div>
      )}
      <ProofChain model={model} />
      <EvidenceDecisionStrip evidence={model.evidence} proposals={model.proposals} decisions={model.decisions} />
    </section>
  );
}

function ProofChain({ model }: { model: ReturnType<typeof taskDocument> | ReturnType<typeof goalDocument> }) {
  const rows: Array<{ label: string; count: number; tone: Tone }> = [
    { label: "Assignment", count: model.assignmentMessages.length, tone: model.assignmentMessages.length ? "good" : "bad" },
    { label: "Report", count: model.reportMessages.length, tone: model.reportMessages.length ? "good" : "warn" },
    { label: "Evidence", count: model.evidence.length, tone: model.evidence.length ? "good" : "warn" },
    { label: "Proposal", count: model.proposals.length, tone: model.proposals.length ? "info" : "neutral" },
    { label: "Decision", count: model.decisions.length, tone: model.decisions.length ? "good" : "warn" },
  ];
  return (
    <div className="proofChain">
      {rows.map((row) => (
        <div className="proofStep" key={row.label}>
          <StatusPill tone={row.tone}>{row.count}</StatusPill>
          <span>{row.label}</span>
        </div>
      ))}
    </div>
  );
}

function EvidenceDecisionStrip({
  evidence,
  proposals,
  decisions,
}: {
  evidence: Evidence[];
  proposals: Proposal[];
  decisions: Decision[];
}) {
  return (
    <div className="acceptanceStrip">
      <MiniLane title="Evidence" empty="No evidence" items={evidence.slice(0, 3).map((item) => ({
        id: item.id,
        label: item.source_type || "evidence",
        body: item.summary || item.source_ref || item.id,
      }))} />
      <MiniLane title="Proposal / Review" empty="No proposal" items={proposals.slice(0, 3).map((proposal) => ({
        id: proposal.id,
        label: proposal.status || "proposal",
        body: proposal.summary || proposal.title || proposal.id,
      }))} />
      <MiniLane title="Decision" empty="No decision" items={decisions.slice(0, 3).map((decision) => ({
        id: decision.id,
        label: decision.decision || "decision",
        body: decision.rationale || decision.id,
      }))} />
    </div>
  );
}

function MiniLane({
  title,
  empty,
  items,
}: {
  title: string;
  empty: string;
  items: Array<{ id: string; label: string; body: string }>;
}) {
  return (
    <div className="miniLane">
      <h3>{title}</h3>
      {items.map((item) => (
        <article key={item.id}>
          <StatusPill tone="info">{item.label}</StatusPill>
          <p>{item.body}</p>
        </article>
      ))}
      {!items.length && <span className="miniEmpty">{empty}</span>}
    </div>
  );
}

function KanbanLanes({
  columns,
  selectedTaskId,
  onSelectTask,
}: {
  columns: Array<{ status: TaskStatus; tasks: Task[] }>;
  selectedTaskId?: string;
  onSelectTask: (taskId: string) => void;
}) {
  return (
    <div className="kanbanLanes">
      {columns.map((column) => (
        <section className="kanbanLane" key={column.status}>
          <div className="laneHeader">
            <span>{column.status}</span>
            <strong>{column.tasks.length}</strong>
          </div>
          {column.tasks.map((task) => (
            <button
              type="button"
              key={task.id}
              className={`kanbanCard ${task.id === selectedTaskId ? "active" : ""}`}
              onClick={() => onSelectTask(task.id)}
            >
              <strong>{task.title || shortId(task.id)}</strong>
              <span>{task.objective || task.id}</span>
              <em>{task.assignee_agent_id || "unassigned"}</em>
            </button>
          ))}
          {!column.tasks.length && <div className="emptyLane">none</div>}
        </section>
      ))}
    </div>
  );
}

function GraphCanvas({
  model,
  selectedTaskId,
  onSelectTask,
}: {
  model: ReturnType<typeof graphKanbanModel>;
  selectedTaskId?: string;
  onSelectTask: (taskId: string) => void;
}) {
  const tasks = model.nodes.filter((node) => node.kind === "task");
  return (
    <div className="graphCanvas">
      <div className="graphGoalNode">
        <Target size={18} />
        <span>{model.nodes.find((node) => node.kind === "goal")?.label || "Goal"}</span>
      </div>
      <div className="graphNodeGrid">
        {tasks.map((node, index) => (
          <button
            key={node.id}
            type="button"
            className={`graphNode ${node.id === selectedTaskId ? "active" : ""}`}
            style={{ "--node-index": index } as CSSProperties}
            onClick={() => onSelectTask(node.id)}
          >
            <CircleDot size={14} />
            <strong>{node.label}</strong>
            <StatusPill tone={taskTone(node.status as TaskStatus)}>{node.status || "task"}</StatusPill>
          </button>
        ))}
      </div>
      <div className="edgeList">
        {model.edges.slice(0, 8).map((edge) => (
          <span key={`${edge.from}-${edge.to}-${edge.label}`}>{shortId(edge.from)} {"->"} {shortId(edge.to)} / {edge.label}</span>
        ))}
      </div>
    </div>
  );
}

function WarningsSurface({
  warnings,
  onSelectTask,
  onSelectMember,
}: {
  warnings: WorkflowWarning[];
  onSelectTask: (taskId: string) => void;
  onSelectMember: (memberId: string) => void;
}) {
  return (
    <section className="singleSurface">
      <PanelHeader icon={<AlertTriangle size={18} />} title="Warnings and repair queue" meta={`${warnings.length} open`} />
      <WarningList warnings={warnings} onSelectTask={onSelectTask} onSelectMember={onSelectMember} />
    </section>
  );
}

function DocsSurface({ docs, selectedObject }: { docs: DocsContextItem[]; selectedObject?: string }) {
  return (
    <section className="singleSurface">
      <PanelHeader icon={<BookOpen size={18} />} title="Mounted docs" meta={selectedObject ? shortId(selectedObject) : "global"} />
      <div className="docsGrid">
        {docs.map((doc) => (
          <article className="docLinkCard" key={doc.path}>
            <FileText size={17} />
            <div>
              <strong>{doc.path}</strong>
              <span>{doc.owner} / {doc.status}</span>
              <p>{doc.reason}</p>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function DecisionsSurface({ decisions, evidence }: { decisions: Decision[]; evidence: Evidence[] }) {
  return (
    <section className="singleSurface">
      <PanelHeader icon={<ShieldCheck size={18} />} title="Review and decisions" meta={`${decisions.length} recent`} />
      <DecisionList decisions={decisions} evidence={evidence} />
    </section>
  );
}

function DebugSurface({
  snapshot,
  jsonInput,
  onJsonInputChange,
  onLoadJson,
}: {
  snapshot: Required<DashboardSnapshot>;
  jsonInput: string;
  onJsonInputChange: (value: string) => void;
  onLoadJson: (value: string) => void;
}) {
  return (
    <section className="singleSurface debugSurface">
      <PanelHeader icon={<Database size={18} />} title="Debug" meta="secondary surface" />
      <SnapshotTools jsonInput={jsonInput} onJsonInputChange={onJsonInputChange} onLoadJson={onLoadJson} />
      <pre className="rawPreview">{JSON.stringify(snapshot, null, 2).slice(0, 6000)}</pre>
    </section>
  );
}

function Inspector({
  activeTab,
  onSelectTab,
  snapshot,
  member,
  taskDoc,
  docs,
  warnings,
  decisions,
  onAction,
  onSelectTask,
  onSelectMember,
}: {
  activeTab: InspectorTab;
  onSelectTab: (tab: InspectorTab) => void;
  snapshot: Required<DashboardSnapshot>;
  member?: AgentMember;
  taskDoc: ReturnType<typeof taskDocument>;
  docs: DocsContextItem[];
  warnings: WorkflowWarning[];
  decisions: Decision[];
  onAction: DashboardAction;
  onSelectTask: (taskId: string) => void;
  onSelectMember: (memberId: string) => void;
}) {
  const tabs: Array<{ key: InspectorTab; label: string; icon: ReactNode }> = [
    { key: "member", label: "Member", icon: <UserRound size={14} /> },
    { key: "task", label: "Task", icon: <FileText size={14} /> },
    { key: "docs", label: "Docs", icon: <BookOpen size={14} /> },
    { key: "evidence", label: "Evid", icon: <Inbox size={14} /> },
    { key: "warnings", label: "Warn", icon: <AlertTriangle size={14} /> },
    { key: "decision", label: "Dec", icon: <ShieldCheck size={14} /> },
  ];
  return (
    <aside className="inspector">
      <div className="inspectorTabs">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            className={activeTab === tab.key ? "active" : ""}
            type="button"
            onClick={() => onSelectTab(tab.key)}
          >
            {tab.icon}
            <span>{tab.label}</span>
          </button>
        ))}
      </div>
      <div className="inspectorBody">
        {activeTab === "member" && (
          <AgentMemberWorkbench
            snapshot={snapshot}
            member={member}
            timeline={memberTimeline(snapshot, member?.id).slice(0, 8)}
            onAction={onAction}
            onSelectTask={onSelectTask}
          />
        )}
        {activeTab === "task" && <TaskDocumentPanel model={taskDoc} onSelectMember={onSelectMember} />}
        {activeTab === "docs" && <DocsSurface docs={docs} selectedObject={taskDoc.task?.id ?? member?.id} />}
        {activeTab === "evidence" && <EvidenceList evidence={taskDoc.evidence} proposals={taskDoc.proposals} />}
        {activeTab === "warnings" && <WarningList warnings={warnings} onSelectTask={onSelectTask} onSelectMember={onSelectMember} />}
        {activeTab === "decision" && <DecisionList decisions={decisions} evidence={taskDoc.evidence} />}
      </div>
    </aside>
  );
}

function DebugDrawer({
  open,
  snapshot,
  jsonInput,
  error,
  onClose,
  onJsonInputChange,
  onLoadJson,
}: {
  open: boolean;
  snapshot: Required<DashboardSnapshot>;
  jsonInput: string;
  error: string | null;
  onClose: () => void;
  onJsonInputChange: (value: string) => void;
  onLoadJson: (value: string) => void;
}) {
  if (!open) return null;
  return (
    <aside className="debugDrawer" aria-label="Debug drawer">
      <div className="debugHeader">
        <PanelHeader icon={<Database size={18} />} title="Debug drawer" meta="closed by default" />
        <button className="iconButton" type="button" title="Close debug drawer" onClick={onClose}>
          <X size={17} />
        </button>
      </div>
      {error && <div className="inlineError">{error}</div>}
      <SnapshotTools jsonInput={jsonInput} onJsonInputChange={onJsonInputChange} onLoadJson={onLoadJson} />
      <pre className="rawPreview">{JSON.stringify(snapshot, null, 2).slice(0, 4000)}</pre>
    </aside>
  );
}

function SnapshotTools({
  jsonInput,
  onJsonInputChange,
  onLoadJson,
}: {
  jsonInput: string;
  onJsonInputChange: (value: string) => void;
  onLoadJson: (value: string) => void;
}) {
  return (
    <div className="snapshotTools">
      <textarea
        spellCheck={false}
        value={jsonInput}
        onChange={(event) => onJsonInputChange(event.target.value)}
        placeholder="Paste harness dashboard snapshot JSON"
      />
      <div className="toolRow">
        <button type="button" onClick={() => onLoadJson(jsonInput)}>
          <Database size={15} /> Load snapshot
        </button>
      </div>
    </div>
  );
}

function CompactLanes({
  tasks,
  selectedTaskId,
  onSelectTask,
}: {
  tasks: Task[];
  selectedTaskId?: string;
  onSelectTask: (taskId: string) => void;
}) {
  const columns = taskStatuses.map((status) => ({ status, tasks: tasks.filter((task) => task.status === status) }));
  return (
    <div className="compactLanes">
      {columns.map((column) => (
        <div className="compactLane" key={column.status}>
          <div className="laneHeader">
            <span>{column.status}</span>
            <strong>{column.tasks.length}</strong>
          </div>
          {column.tasks.slice(0, 3).map((task) => (
            <button
              key={task.id}
              type="button"
              className={selectedTaskId === task.id ? "active" : ""}
              onClick={() => onSelectTask(task.id)}
            >
              {task.title || shortId(task.id)}
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}

function RuntimeHealth({
  member,
  sessions,
  childThreads,
}: {
  member: AgentMember;
  sessions: ProviderSession[];
  childThreads: ProviderChildThread[];
}) {
  const health = member.runtime_health ?? {};
  return (
    <div className="runtimeStack">
      <MetaValue label="Provider" value={member.provider} />
      <MetaValue label="Runtime id" value={member.runtime_id} />
      <MetaValue label="PID" value={member.runtime_pid == null ? undefined : String(member.runtime_pid)} />
      <MetaValue label="Process" value={String(health.process_alive ?? member.runtime_alive ?? "-")} />
      <MetaValue label="Socket" value={String(health.socket_exists ?? "-")} />
      <MetaValue label="Protocol" value={String(health.protocol_probe ?? "-")} />
      <MetaValue label="Delivery" value={String(health.delivery_probe ?? "-")} />
      <MetaValue label="Prompt" value={member.prompt_ref} />
      <div className="tagWrap">
        {(member.skill_refs ?? []).slice(0, 5).map((skill) => <span key={skill}>{skill}</span>)}
        {!member.skill_refs?.length && <span>no skill refs</span>}
      </div>
      <MessageColumn title="Sessions" sessions={sessions} childThreads={childThreads} />
    </div>
  );
}

function MessageColumn({
  title,
  messages = [],
  sessions = [],
  childThreads = [],
  onSelectTask,
}: {
  title: string;
  messages?: Message[];
  sessions?: ProviderSession[];
  childThreads?: ProviderChildThread[];
  onSelectTask?: (taskId: string) => void;
}) {
  return (
    <div className="messageColumn">
      <h3>{title}</h3>
      {messages.map((message, index) => (
        <button
          className="messageLine"
          type="button"
          key={`${message.id}-${message.delivery_status}-${index}`}
          onClick={() => message.task_id && onSelectTask?.(message.task_id)}
        >
          <StatusPill tone={message.delivery_status === "failed" ? "bad" : message.delivery_status === "queued" ? "warn" : "good"}>
            {message.delivery_status}
          </StatusPill>
          <span>{message.kind} / {message.task_id || "no task"}</span>
        </button>
      ))}
      {sessions.map((session) => (
        <div className="messageLine" key={session.id}>
          <StatusPill tone={statusTone(session.status)}>{session.status || "session"}</StatusPill>
          <span>{session.task_id || session.provider_thread_id || shortId(session.id)}</span>
        </div>
      ))}
      {childThreads.map((thread) => (
        <div className="messageLine" key={thread.id}>
          <StatusPill tone={statusTone(thread.status)}>{thread.status || "thread"}</StatusPill>
          <span>{thread.provider_agent_nickname || thread.provider_thread_id || shortId(thread.id)}</span>
        </div>
      ))}
      {!messages.length && !sessions.length && !childThreads.length && <p className="muted">None</p>}
    </div>
  );
}

function WarningList({
  warnings,
  onSelectTask,
  onSelectMember,
}: {
  warnings: WorkflowWarning[];
  onSelectTask: (taskId: string) => void;
  onSelectMember: (memberId: string) => void;
}) {
  return (
    <div className="warningList">
      {warnings.map((warning) => (
        <article className={`warningItem ${warning.severity}`} key={warning.id}>
          <StatusPill tone={warning.severity === "high" ? "bad" : warning.severity === "medium" ? "warn" : "info"}>
            {warning.severity}
          </StatusPill>
          <div>
            <strong>{warning.kind}</strong>
            <p>{warning.summary}</p>
            <div className="objectRefs">
              {warning.taskId && <button type="button" onClick={() => onSelectTask(warning.taskId!)}>task {shortId(warning.taskId)}</button>}
              {warning.memberId && <button type="button" onClick={() => onSelectMember(warning.memberId!)}>member {shortId(warning.memberId)}</button>}
              {warning.goalId && <span>goal {shortId(warning.goalId)}</span>}
            </div>
            <div className="repairActions" aria-label="Repair actions">
              {warning.memberId && <button type="button" onClick={() => onSelectMember(warning.memberId!)}>Open member</button>}
              {warning.taskId && <button type="button" onClick={() => onSelectTask(warning.taskId!)}>Open task</button>}
              <button type="button" disabled title="Requires follow-up task API">Create follow-up</button>
              {warning.kind.includes("delivery") && <button type="button" disabled title="Retry delivery API is member scoped">Retry delivery</button>}
              {warning.kind.includes("report") && <button type="button" disabled title="Requires report request API">Request report</button>}
            </div>
          </div>
        </article>
      ))}
      {!warnings.length && <EmptyBlock title="No scoped warnings" detail="Current scope has no warning records." />}
    </div>
  );
}

function DecisionList({ decisions, evidence = [] }: { decisions: Decision[]; evidence?: Evidence[] }) {
  return (
    <div className="decisionList">
      {decisions.map((decision) => (
        <article className="decisionItem" key={decision.id}>
          <StatusPill tone={decision.decision?.includes("accept") ? "good" : "info"}>{decision.decision || "decision"}</StatusPill>
          <strong>{shortId(decision.id)}</strong>
          <p>{decision.rationale || "No rationale recorded."}</p>
          <span>{(decision.evidence_ids ?? []).length} evidence refs</span>
        </article>
      ))}
      {!decisions.length && <EmptyBlock title="No decisions" detail="No Leader/Critic decision is recorded for this scope." />}
      {evidence.slice(0, 3).map((item) => (
        <div className="evidenceChip" key={item.id}>{item.source_type || "evidence"} / {item.summary || item.source_ref}</div>
      ))}
    </div>
  );
}

function EvidenceList({ evidence, proposals }: { evidence: Evidence[]; proposals: Proposal[] }) {
  return (
    <div className="evidenceList">
      {evidence.map((item) => (
        <article className="evidenceItem" key={item.id}>
          <StatusPill tone={item.source_type?.includes("failed") ? "bad" : "info"}>{item.source_type || "evidence"}</StatusPill>
          <strong>{shortId(item.id)}</strong>
          <p>{item.summary || item.source_ref || "No evidence summary."}</p>
        </article>
      ))}
      {proposals.map((proposal) => (
        <article className="evidenceItem" key={proposal.id}>
          <StatusPill tone="neutral">{proposal.status || "proposal"}</StatusPill>
          <strong>{proposal.title || shortId(proposal.id)}</strong>
          <p>{proposal.summary || (proposal.changed_paths ?? []).join(", ") || "No proposal summary."}</p>
        </article>
      ))}
      {!evidence.length && !proposals.length && <EmptyBlock title="No evidence" detail="This object has no evidence or proposal refs." />}
    </div>
  );
}

function ActivityList({ items, onSelectTask }: { items: TimelineItem[]; onSelectTask: (taskId: string) => void }) {
  return (
    <div className="activityList">
      {items.map((item, index) => (
        <button
          className="activityItem"
          key={`${item.id}-${item.kind}-${item.status ?? "none"}-${index}`}
          type="button"
          disabled={!item.taskId}
          onClick={() => item.taskId && onSelectTask(item.taskId)}
        >
          <StatusDot tone={statusTone(item.status)} />
          <div>
            <strong>{item.title}</strong>
            <span>{item.detail || item.kind}</span>
          </div>
          <time>{formatStamp(item.createdAt)}</time>
        </button>
      ))}
      {!items.length && <EmptyBlock title="No activity" detail="No messages, sessions, or events match this scope." />}
    </div>
  );
}

function Checklist({ items, emptyLabel }: { items: string[]; emptyLabel: string }) {
  if (!items.length) return <p className="muted">{emptyLabel}</p>;
  return (
    <ul className="checklist">
      {items.map((item) => <li key={item}>{item}</li>)}
    </ul>
  );
}

function PanelHeader({ icon, title, meta }: { icon: ReactNode; title: string; meta?: string }) {
  return (
    <div className="panelHeader">
      <div>
        {icon}
        <h2>{title}</h2>
      </div>
      {meta && <span>{meta}</span>}
    </div>
  );
}

function Metric({ label, value, tone = "neutral" }: { label: string; value: string; tone?: Tone }) {
  return (
    <div className={`metric ${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ProofChip({ label, value }: { label: string; value: string }) {
  return (
    <div className="proofChip">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function MetaValue({ label, value }: { label: string; value?: string | null }) {
  return (
    <div className="metaValue">
      <span>{label}</span>
          <strong title={value || "-"}>{value || "-"}</strong>
    </div>
  );
}

function MetaButton({ label, value, onClick }: { label: string; value?: string | null; onClick?: () => void }) {
  if (!onClick) return <MetaValue label={label} value={value} />;
  return (
    <button className="metaValue clickableMeta" type="button" onClick={onClick}>
      <span>{label}</span>
      <strong title={value || "-"}>{value || "-"}</strong>
    </button>
  );
}

function StatusPill({ children, tone = "neutral", icon }: { children: ReactNode; tone?: Tone; icon?: ReactNode }) {
  return (
    <span className={`statusPill ${tone}`}>
      {icon}
      {children}
    </span>
  );
}

function StatusDot({ tone = "neutral" }: { tone?: Tone }) {
  return <span className={`statusDot ${tone}`} aria-hidden="true" />;
}

function Avatar({ label, large = false }: { label: string; large?: boolean }) {
  const initials = label
    .split(/[\s-_]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("") || "A";
  return <span className={`avatar ${large ? "large" : ""}`}>{initials}</span>;
}

function EmptyBlock({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="emptyBlock">
      <strong>{title}</strong>
      <span>{detail}</span>
    </div>
  );
}

function memberForTask(members: AgentMember[], task?: Task): AgentMember | undefined {
  if (!task?.assignee_agent_id) return members[0];
  return members.find((member) => member.id === task.assignee_agent_id) ?? members[0];
}

function preferredTask(tasks: Task[]): Task | undefined {
  return [...tasks].sort((left, right) => taskTime(right) - taskTime(left))[0];
}

function memberStatusSummary(member: AgentMember): string {
  const task = member.current_task_id ? shortId(member.current_task_id) : "no task";
  return `member ${member.status || "unknown"} / runtime ${member.runtime_status || "offline"} / ${task}`;
}

function taskTime(task: Task): number {
  const value = task.updated_at || task.created_at;
  if (!value) return 0;
  const match = value.match(/^unix-ms:(\d+)$/);
  if (match) return Number(match[1]);
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function shortId(value?: string | null): string {
  if (!value) return "-";
  if (value.length <= 24) return value;
  return `${value.slice(0, 10)}...${value.slice(-8)}`;
}

function formatStamp(value?: string): string {
  if (!value) return "";
  const match = value.match(/^unix-ms:(\d+)$/);
  if (!match) return value;
  const date = new Date(Number(match[1]));
  return date.toLocaleString();
}

function statusTone(status?: string | null): Tone {
  const normalized = (status ?? "").toLowerCase();
  if (["succeeded", "success", "running", "active", "delivered", "acknowledged", "done"].some((item) => normalized.includes(item))) return "good";
  if (["failed", "error", "blocked", "canceled", "closed"].some((item) => normalized.includes(item))) return "bad";
  if (["queued", "stale", "assigned", "review", "pending"].some((item) => normalized.includes(item))) return "warn";
  if (normalized) return "info";
  return "neutral";
}

function taskTone(status?: TaskStatus | string): Tone {
  if (status === "done") return "good";
  if (status === "blocked") return "bad";
  if (status === "running" || status === "review") return "warn";
  if (status === "assigned") return "info";
  return "neutral";
}
