export type SurfaceId =
  | "agents"
  | "vision"
  | "goal"
  | "task"
  | "tasks"
  | "workflows"
  | "docs"
  | "debug";

/** Tabs on the agent detail page. "conversation" is the default. */
export type AgentTab = "conversation" | "tasks" | "config";

const agentTabs: AgentTab[] = ["conversation", "tasks", "config"];

/** Tabs on the task document. "overview" is the default. */
export type TaskTab = "overview" | "deps" | "proof" | "activity";

const taskTabs: TaskTab[] = ["overview", "deps", "proof", "activity"];

export interface SelectionState {
  surface: SurfaceId;
  goalId?: string;
  /**
   * Retained so the read model can still resolve an AgentTeam for data
   * continuity (fixtures + historical jsonl keep the AgentTeam object). There is
   * no Team concept in the UI; nothing sets this from a user gesture.
   */
  teamId?: string;
  /** The selected agent id (the AgentMember opened on the agent detail page). */
  memberId?: string;
  /** Which tab is open on the agent detail page; defaults to "conversation". */
  agentTab?: AgentTab;
  taskId?: string;
  /** The phase opened within a goal (Goal -> Phase drill-in), addressed as `?phase=<id>`. */
  phaseId?: string;
  /**
   * The doc opened on the Docs surface, addressed by its repo path
   * (e.g. "docs/prd.md"); setting it implies the docs surface.
   */
  docPath?: string;
  /** Which tab is open on the task document; defaults to "overview". */
  taskTab?: TaskTab;
  /** The selected workflow run id (opens WorkflowRunDetail on the workflows surface). */
  workflowRunId?: string;
  /**
   * Retained for URL/back-compat (goal-task-board-model retired the flat global
   * task board). The Work board now shows the Goal collection by default; a
   * `boardGoal` filter pins it to one legacy goal's task columns. Nothing reads
   * `boardScope` for view selection anymore.
   */
  boardScope?: "goals" | "tasks";
  /** Work board filter: pin to one goal's task columns (legacy phaseless fallback). */
  boardGoal?: string;
}

export const defaultSelection: SelectionState = {
  surface: "agents",
  boardScope: "tasks",
};

const surfaceIds: SurfaceId[] = [
  "agents",
  "vision",
  "goal",
  "task",
  "tasks",
  "workflows",
  "docs",
  "debug",
];

const selectionParamKeys = [
  "surface",
  "agent",
  "member",
  "agentTab",
  "team",
  "goal",
  "task",
  "phase",
  "doc",
  "taskTab",
  "workflowRun",
  "board",
  "boardGoal",
];

/**
 * Derive the URL-addressable selection from the current location. A single agent
 * is reachable as `?agent=<id>` (URL-addressable like the goal/task docs); the
 * legacy `/members/:id` path form is still accepted and resolves to the Agents
 * area with that agent selected.
 */
export function selectionFromLocation(base: SelectionState): SelectionState {
  if (typeof window === "undefined") return base;
  const next: SelectionState = { ...base };

  // Legacy path form: /members/:memberId → Agents area, that agent open.
  const pathMatch = window.location.pathname.match(/\/members\/([^/?#]+)/);
  if (pathMatch) {
    next.surface = "agents";
    next.memberId = decodeURIComponent(pathMatch[1]);
  }

  const params = new URLSearchParams(window.location.search);
  const surface = params.get("surface");
  if (surface && (surfaceIds as string[]).includes(surface)) {
    next.surface = surface as SurfaceId;
  }
  // Canonical agent address: ?agent=<id>. Accept the legacy ?member= alias too.
  const agent = params.get("agent") ?? params.get("member");
  if (agent) {
    next.memberId = agent;
    if (!surface) next.surface = "agents";
  }
  const agentTab = params.get("agentTab");
  if (agentTab && (agentTabs as string[]).includes(agentTab)) {
    next.agentTab = agentTab as AgentTab;
  }
  const team = params.get("team");
  if (team) next.teamId = team;
  const goal = params.get("goal");
  if (goal) next.goalId = goal;
  const task = params.get("task");
  if (task) next.taskId = task;
  const phase = params.get("phase");
  if (phase) next.phaseId = phase;
  // Canonical doc address: ?doc=<path>; setting it implies the docs surface
  // (mirror of the ?agent= / ?workflowRun= rules).
  const doc = params.get("doc");
  if (doc) {
    next.docPath = doc;
    if (!surface) next.surface = "docs";
  }
  const taskTab = params.get("taskTab");
  if (taskTab && (taskTabs as string[]).includes(taskTab)) {
    next.taskTab = taskTab as TaskTab;
  }
  // Canonical run address: ?workflowRun=<id>; setting it implies the workflows
  // surface (mirror of the ?agent= rule above).
  const workflowRun = params.get("workflowRun");
  if (workflowRun) {
    next.workflowRunId = workflowRun;
    if (!surface) next.surface = "workflows";
  }
  const boardScope = params.get("board");
  if (boardScope === "goals" || boardScope === "tasks") next.boardScope = boardScope;
  const boardGoal = params.get("boardGoal");
  if (boardGoal) next.boardGoal = boardGoal;
  return next;
}

/**
 * Reflect the selection into the address bar (without reloading) so the current
 * agent/surface is shareable. The selected agent is written as `?agent=<id>`,
 * the same query-form approach the goal/task docs use, which keeps the static
 * `base: "./"` Vite build working from any path.
 */
export function syncSelectionToLocation(selection: SelectionState): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  for (const key of selectionParamKeys) params.delete(key);
  if (selection.surface && selection.surface !== "agents") {
    params.set("surface", selection.surface);
  }
  if (selection.memberId) params.set("agent", selection.memberId);
  // Only persist a non-default agent tab, and only when an agent is open.
  if (selection.memberId && selection.agentTab && selection.agentTab !== "conversation") {
    params.set("agentTab", selection.agentTab);
  }
  if (selection.teamId) params.set("team", selection.teamId);
  if (selection.goalId) params.set("goal", selection.goalId);
  if (selection.taskId) params.set("task", selection.taskId);
  if (selection.phaseId) params.set("phase", selection.phaseId);
  if (selection.docPath) params.set("doc", selection.docPath);
  // Only persist a non-default task tab, and only when a task is open.
  if (selection.taskId && selection.taskTab && selection.taskTab !== "overview") {
    params.set("taskTab", selection.taskTab);
  }
  if (selection.workflowRunId) params.set("workflowRun", selection.workflowRunId);
  if (selection.boardScope === "goals") params.set("board", "goals");
  if (selection.boardGoal) params.set("boardGoal", selection.boardGoal);

  const query = params.toString();
  const url = `${window.location.pathname}${query ? `?${query}` : ""}${window.location.hash}`;
  const current = `${window.location.pathname}${window.location.search}${window.location.hash}`;
  if (url !== current) {
    window.history.replaceState(null, "", url);
  }
}
