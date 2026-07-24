export type SurfaceId =
  | "home"
  | "organization"
  | "work"
  | "approvals"
  | "finance"
  | "providers"
  | "plugins"
  | "settings"
  | "agents"
  | "missions"
  | "team"
  | "workflows"
  | "docs"
  | "debug";

/** Tabs on the agent detail page. "conversation" is the default. */
export type AgentTab = "conversation" | "tasks" | "config";

const agentTabs: AgentTab[] = ["conversation", "tasks", "config"];

export interface SelectionState {
  surface: SurfaceId;
  /** Company OS document focus. Distinct from the legacy repository-doc path. */
  documentId?: string;
  /** Company OS WorkItem focus. */
  workItemId?: string;
  /** Durable Standing Agent organization identity, never a MemberRun. */
  standingAgentId?: string;
  /** First-class Human organization member identity. */
  personId?: string;
  /** Governance proposal focus. */
  proposalId?: string;
  /** Approval record focus. */
  approvalId?: string;
  /** BusinessModule focus. */
  moduleId?: string;
  /** Docs health review, addressed as `?health=structure`. */
  docsHealth?: string;
  /** Native Mission detail, addressed as `?mission=<id>`. */
  missionId?: string;
  /** Native Wave detail inside a Mission, addressed as `?wave=<id>`. */
  waveId?: string;
  /**
   * The selected Agent Team run id (a team_run id), addressed as `?team=<id>`.
   * Opens the Team surface's run detail when set; the list shows when absent.
   */
  teamId?: string;
  /** The selected agent id (the AgentMember opened on the agent detail page). */
  memberId?: string;
  /**
   * The selected Agent Team participation record, addressed as
   * `?memberRun=<id>`. This deliberately remains distinct from `memberId`:
   * a MemberRun is a one-attempt participation, while `memberId` identifies a
   * standing AgentMember.
   */
  memberRunId?: string;
  /** Which tab is open on the agent detail page; defaults to "conversation". */
  agentTab?: AgentTab;
  /**
   * The doc opened on the Docs surface, addressed by its repo path
   * (e.g. "docs/prd.md"); setting it implies the docs surface.
   */
  docPath?: string;
  /** The selected workflow run id (opens WorkflowRunDetail on the workflows surface). */
  workflowRunId?: string;
}

export const defaultSelection: SelectionState = {
  surface: "home",
};

const surfaceIds: SurfaceId[] = [
  "home",
  "organization",
  "work",
  "approvals",
  "finance",
  "providers",
  "plugins",
  "settings",
  "agents",
  "team",
  "missions",
  "workflows",
  "docs",
  "debug",
];

const selectionParamKeys = [
  "surface",
  "document",
  "workItem",
  "person",
  "proposal",
  "approval",
  "module",
  "health",
  "agent",
  "member",
  "memberRun",
  "agentTab",
  "team",
  "mission",
  "wave",
  "doc",
  "workflowRun",
];

/**
 * Derive the URL-addressable selection from the current location. A single agent
 * is reachable as `?agent=<id>` (URL-addressable like the goal/task docs); the
 * legacy `/members/:id` path form is still accepted and resolves to the Agents
 * area with that agent selected.
 */
export function selectionFromLocation(base: SelectionState): SelectionState {
  if (typeof window === "undefined") return base;
  // URL state is authoritative. Starting from a clean default prevents a
  // previously-open Company OS record from leaking into Back/Forward routes
  // after its query parameter has disappeared.
  const next: SelectionState = { ...defaultSelection };

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
  // `?agent=` is contextual: Organization resolves a durable Standing Agent;
  // the retained execution compatibility route resolves an AgentMember.
  const agent = params.get("agent") ?? params.get("member");
  if (agent) {
    if (next.surface === "organization") next.standingAgentId = agent;
    else {
      next.memberId = agent;
      if (!surface) next.surface = "agents";
    }
  }
  const documentId = params.get("document");
  if (documentId) {
    next.documentId = documentId;
    if (!surface) next.surface = "docs";
  }
  const workItemId = params.get("workItem");
  if (workItemId) {
    next.workItemId = workItemId;
    if (!surface) next.surface = "work";
  }
  const personId = params.get("person");
  if (personId) {
    next.personId = personId;
    if (!surface) next.surface = "organization";
  }
  const proposalId = params.get("proposal");
  if (proposalId) {
    next.proposalId = proposalId;
    if (!surface) next.surface = "organization";
  }
  const approvalId = params.get("approval");
  if (approvalId) {
    next.approvalId = approvalId;
    if (!surface) next.surface = "approvals";
  }
  const moduleId = params.get("module");
  if (moduleId) {
    next.moduleId = moduleId;
    if (!surface) next.surface = "docs";
  }
  const docsHealth = params.get("health");
  if (docsHealth) {
    next.docsHealth = docsHealth;
    if (!surface) next.surface = "docs";
  }
  // A MemberRun belongs to an AgentTeamRun attempt, not to the standing Agent
  // directory. Do not translate it into `memberId` even if a future provider
  // happens to expose a related standing identity.
  const memberRun = params.get("memberRun");
  if (memberRun) {
    next.memberRunId = memberRun;
    if (!surface) next.surface = "team";
  }
  const agentTab = params.get("agentTab");
  if (agentTab && (agentTabs as string[]).includes(agentTab)) {
    next.agentTab = agentTab as AgentTab;
  }
  const team = params.get("team");
  // Canonical team-run address: ?team=<run id>; setting it implies the Team
  // surface (mirror of the ?agent= / ?workflowRun= rules).
  if (team) {
    next.teamId = team;
    if (!surface) next.surface = "team";
  }
  const mission = params.get("mission");
  if (mission) {
    next.missionId = mission;
    if (!surface) next.surface = "missions";
  }
  const wave = params.get("wave");
  if (wave) {
    next.waveId = wave;
    if (!surface) next.surface = "missions";
  }
  // Canonical doc address: ?doc=<path>; setting it implies the docs surface
  // (mirror of the ?agent= / ?workflowRun= rules).
  const doc = params.get("doc");
  if (doc) {
    next.docPath = doc;
    if (!surface) next.surface = "docs";
  }
  // Canonical run address: ?workflowRun=<id>; setting it implies the workflows
  // surface (mirror of the ?agent= rule above).
  const workflowRun = params.get("workflowRun");
  if (workflowRun) {
    next.workflowRunId = workflowRun;
    if (!surface) next.surface = "workflows";
  }
  return next;
}

/**
 * Reflect a user selection into browser history without reloading so entity
 * deep links are shareable and Back/Forward returns through the workbench
 * journey. The selected agent is written as `?agent=<id>`; query-form routing
 * keeps the static `base: "./"` Vite build working from any path.
 */
export function syncSelectionToLocation(selection: SelectionState): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  for (const key of selectionParamKeys) params.delete(key);
  if (selection.surface && selection.surface !== "home") {
    params.set("surface", selection.surface);
  }
  if (selection.documentId) params.set("document", selection.documentId);
  if (selection.workItemId) params.set("workItem", selection.workItemId);
  if (selection.standingAgentId) params.set("agent", selection.standingAgentId);
  if (selection.personId) params.set("person", selection.personId);
  if (selection.proposalId) params.set("proposal", selection.proposalId);
  if (selection.approvalId) params.set("approval", selection.approvalId);
  if (selection.moduleId) params.set("module", selection.moduleId);
  if (selection.docsHealth) params.set("health", selection.docsHealth);
  if (selection.memberId && selection.surface !== "organization") params.set("agent", selection.memberId);
  // Only persist a non-default agent tab, and only when an agent is open.
  if (selection.memberId && selection.agentTab && selection.agentTab !== "conversation") {
    params.set("agentTab", selection.agentTab);
  }
  if (selection.memberRunId) params.set("memberRun", selection.memberRunId);
  if (selection.teamId) params.set("team", selection.teamId);
  if (selection.missionId) params.set("mission", selection.missionId);
  if (selection.waveId) params.set("wave", selection.waveId);
  if (selection.docPath) params.set("doc", selection.docPath);
  if (selection.workflowRunId) params.set("workflowRun", selection.workflowRunId);

  const query = params.toString();
  const url = `${window.location.pathname}${query ? `?${query}` : ""}${window.location.hash}`;
  const current = `${window.location.pathname}${window.location.search}${window.location.hash}`;
  if (url !== current) {
    window.history.pushState(null, "", url);
  }
}
