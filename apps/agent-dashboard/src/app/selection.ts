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
  | "docs-v2"
  | "operator"
  | "debug";

/** Tabs on the agent detail page. "conversation" is the default. */
export type AgentTab = "conversation" | "tasks" | "config";

const agentTabs: AgentTab[] = ["conversation", "tasks", "config"];

export interface SelectionState {
  surface: SurfaceId;
  /** Company OS document focus. Distinct from the legacy repository-doc path. */
  documentId?: string;
  /** Durable Agent Membership organization identity, never a MemberRun. */
  agentMembershipId?: string;
  /** First-class Human organization member identity. */
  personId?: string;
  /** Governance proposal focus. */
  proposalId?: string;
  /** Approval record focus. */
  approvalId?: string;
  /** BusinessModule focus. */
  moduleId?: string;
  /** Code-declared Company OS custom page, addressed as `?page=<custom-page-definition-id>`. */
  customPageId?: string;
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
   * durable AgentMember.
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
  /**
   * View inside the Work surface, addressed as `?workView=<id>`. The native
   * Team Work execution view is `team-works`; absent means the Company Work aggregate.
   */
  workView?: string;
  /**
   * View inside the Organization surface, addressed as `?orgView=<id>`.
   * `agent-teams` is the recursive AgentTeam tree; absent means the OrgUnit
   * organization.
   */
  orgView?: string;
  /** Selected durable AgentTeam node in the recursive Organization view. */
  orgTeamId?: string;
  /** Comma-separated expanded AgentTeam ids; URL-owned tree disclosure state. */
  orgExpanded?: string;
  /** Selected native Team Work row in an owning Team War Room. */
  teamWorkId?: string;
  /** Responsibility lens inside one Team: shared workspace or Host console. */
  teamMode?: "workspace" | "host";
  /** Addressed actor in the Team conversation workspace (`host` or AgentMember id). */
  teamConversation?: string;
  /** Center mode inside the unified Host/Member Agent Workspace. */
  agentWorkspaceMode?: "session" | "messages" | "work";
  /** Exact provider-native Session selected inside the Agent Workspace. */
  agentSessionId?: string;
  /** URL-owned War Room tab and bounded Work filters. */
  teamTab?: "works" | "activity" | "members";
  teamOwner?: string;
  teamAttention?: "all" | "blocked" | "review";
  teamQuery?: string;
  /** Machine selected in the Operator responsibility view. */
  nodeId?: string;
  /** URL-backed filters for the cross-TeamRun Team Works aggregate. */
  workTeamId?: string;
  workMissionId?: string;
  workNodeId?: string;
  workHostId?: string;
  workMemberId?: string;
  workStatus?: string;
  workPriority?: string;
  workSource?: string;
  workDemand?: string;
}

/**
 * The Work operating board (its overview tab) is the default Company OS
 * operating surface for the wanchengwanling and agentos Company Stores
 * (work-wcw-agentos-work-overview-ui). Home stays reachable through the
 * navigation rail and an explicit `?surface=home` deep link; every entity
 * deep link below still implies its own surface.
 */
export const defaultSelection: SelectionState = {
  surface: "work",
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
  "docs-v2",
  "operator",
  "debug",
];

/**
 * Derive the URL-addressable selection from the current location. A single agent
 * is reachable as `?agent=<id>` (URL-addressable like the goal/task docs); the
 * legacy `/members/:id` path form is still accepted and resolves to the Agents
 * area with that agent selected.
 */
export function selectionFromLocation(base: SelectionState): SelectionState {
  if (typeof window === "undefined") return base;
  return selectionFromSearch(window.location.search, window.location.pathname);
}

/**
 * Derive the URL-addressable selection from an explicit query string (and an
 * optional path for the legacy `/members/:id` form). Split from
 * selectionFromLocation so selection sync can compare what the current
 * location and its canonical form both resolve to.
 */
function selectionFromSearch(search: string, pathname = "/"): SelectionState {
  // URL state is authoritative. Starting from a clean default prevents a
  // previously-open Company OS record from leaking into Back/Forward routes
  // after its query parameter has disappeared.
  const next: SelectionState = { ...defaultSelection };

  // Legacy path form: /members/:memberId → Agents area, that agent open.
  const pathMatch = pathname.match(/\/members\/([^/?#]+)/);
  if (pathMatch) {
    next.surface = "agents";
    next.memberId = decodeURIComponent(pathMatch[1]);
  }

  const params = new URLSearchParams(search);
  const surface = params.get("surface");
  if (surface && (surfaceIds as string[]).includes(surface)) {
    next.surface = surface as SurfaceId;
  }
  // `?agent=` is contextual: Organization resolves a durable Agent Membership;
  // the retained execution compatibility route resolves an AgentMember.
  const agent = params.get("agent") ?? params.get("member");
  if (agent) {
    if (next.surface === "organization") next.agentMembershipId = agent;
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
  const customPageId = params.get("page");
  if (customPageId) {
    next.customPageId = customPageId;
    if (!surface) next.surface = "docs";
  }
  const docsHealth = params.get("health");
  if (docsHealth) {
    next.docsHealth = docsHealth;
    if (!surface) next.surface = "docs";
  }
  // A MemberRun belongs to an AgentTeamRun attempt, not to the durable AgentMember
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
  const agentWorkspaceMode = params.get("agentMode");
  if (agentWorkspaceMode && ["session", "messages", "work"].includes(agentWorkspaceMode)) {
    next.agentWorkspaceMode = agentWorkspaceMode as SelectionState["agentWorkspaceMode"];
  }
  const agentSessionId = params.get("agentSession");
  if (agentSessionId) next.agentSessionId = agentSessionId;
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
  // In-surface views stay URL-addressable so every contracted page state is
  // reachable by deep link (docs/design/company-os-v6/recursive-org-docs-works-v1).
  const workView = params.get("workView");
  if (workView) {
    next.workView = workView;
    if (!surface) next.surface = "work";
  }
  const orgView = params.get("orgView");
  if (orgView) {
    next.orgView = orgView;
    if (!surface) next.surface = "organization";
  }
  const orgTeam = params.get("orgTeam");
  if (orgTeam) {
    next.orgTeamId = orgTeam;
    next.orgView = "agent-teams";
    if (!surface) next.surface = "organization";
  }
  const orgExpanded = params.get("orgExpanded");
  if (orgExpanded) next.orgExpanded = orgExpanded;
  const teamWork = params.get("teamWork");
  if (teamWork) next.teamWorkId = teamWork;
  const teamMode = params.get("teamMode");
  if (teamMode === "workspace" || teamMode === "host") next.teamMode = teamMode;
  const teamConversation = params.get("conversation");
  if (teamConversation) next.teamConversation = teamConversation;
  const teamTab = params.get("teamTab");
  if (teamTab === "works" || teamTab === "activity" || teamTab === "members") next.teamTab = teamTab;
  const teamOwner = params.get("teamOwner");
  if (teamOwner) next.teamOwner = teamOwner;
  const teamAttention = params.get("teamAttention");
  if (teamAttention === "all" || teamAttention === "blocked" || teamAttention === "review") next.teamAttention = teamAttention;
  const teamQuery = params.get("teamQuery");
  if (teamQuery) next.teamQuery = teamQuery;
  const node = params.get("node");
  if (node) { next.nodeId = node; if (!surface) next.surface = "operator"; }
  const filterParams = [
    ["workTeam", "workTeamId"],
    ["workMission", "workMissionId"],
    ["workNode", "workNodeId"],
    ["workHost", "workHostId"],
    ["workMember", "workMemberId"],
    ["workStatus", "workStatus"],
    ["workPriority", "workPriority"],
    ["workSource", "workSource"],
    ["workDemand", "workDemand"],
  ] as const;
  for (const [param, key] of filterParams) {
    const value = params.get(param);
    if (value) next[key] = value;
  }
  return next;
}

/**
 * Reflect a user selection into browser history without reloading so entity
 * deep links are shareable and Back/Forward returns through the workbench
 * journey. The selected agent is written as `?agent=<id>`; query-form routing
 * keeps the static `base: "./"` Vite build working from any path. The default
 * surface is omitted from the URL so a bare link round-trips to the same
 * default, while an explicit non-default surface (including `?surface=home`)
 * stays addressable. A location that already resolves to the same selection (such
 * as bare `?surface=work`) is canonicalized in place via replaceState, never
 * pushed, so browser Back is never trapped. Company Store, Execution Space,
 * Project Binding, and API params are owned by App-level sync and are never
 * deleted here.
 */
export function syncSelectionToLocation(selection: SelectionState): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  // Mutate in place instead of delete-all-then-set: URLSearchParams.set keeps
  // an existing key's position, so an already-canonical location serializes
  // byte-identically and no spurious history entry is pushed. That is what
  // makes browser Back return from a focused Company OS object to the previous entry in
  // one step.
  const setOrDelete = (key: string, value: string | undefined): void => {
    if (value) params.set(key, value);
    else params.delete(key);
  };
  setOrDelete(
    "surface",
    selection.surface && selection.surface !== defaultSelection.surface
      ? selection.surface
      : undefined,
  );
  setOrDelete("document", selection.documentId);
  // `agent` is contextual: a durable Agent Membership on Organization, otherwise
  // the selected AgentMember (never on the organization surface).
  setOrDelete(
    "agent",
    selection.agentMembershipId
      ?? (selection.memberId && selection.surface !== "organization" ? selection.memberId : undefined),
  );
  params.delete("member"); // legacy alias, never written
  setOrDelete("person", selection.personId);
  setOrDelete("proposal", selection.proposalId);
  setOrDelete("approval", selection.approvalId);
  setOrDelete("module", selection.moduleId);
  setOrDelete("page", selection.customPageId);
  setOrDelete("health", selection.docsHealth);
  // Only persist a non-default agent tab, and only when an agent is open.
  setOrDelete(
    "agentTab",
    selection.memberId && selection.agentTab && selection.agentTab !== "conversation"
      ? selection.agentTab
      : undefined,
  );
  setOrDelete("memberRun", selection.memberRunId);
  setOrDelete("team", selection.teamId);
  setOrDelete("mission", selection.missionId);
  setOrDelete("wave", selection.waveId);
  setOrDelete("doc", selection.docPath);
  setOrDelete("workflowRun", selection.workflowRunId);
  setOrDelete("workView", selection.workView);
  setOrDelete("orgView", selection.orgView);
  setOrDelete("orgTeam", selection.orgTeamId);
  setOrDelete("orgExpanded", selection.orgExpanded);
  setOrDelete("teamWork", selection.teamWorkId);
  setOrDelete("teamMode", selection.teamMode);
  setOrDelete("conversation", selection.teamConversation);
  setOrDelete("agentMode", selection.agentWorkspaceMode && selection.agentWorkspaceMode !== "session" ? selection.agentWorkspaceMode : undefined);
  setOrDelete("agentSession", selection.agentSessionId);
  setOrDelete("teamTab", selection.teamTab && selection.teamTab !== "works" ? selection.teamTab : undefined);
  setOrDelete("teamOwner", selection.teamOwner && selection.teamOwner !== "all" ? selection.teamOwner : undefined);
  setOrDelete("teamAttention", selection.teamAttention && selection.teamAttention !== "all" ? selection.teamAttention : undefined);
  setOrDelete("teamQuery", selection.teamQuery);
  setOrDelete("node", selection.nodeId);
  setOrDelete("workTeam", selection.workTeamId);
  setOrDelete("workMission", selection.workMissionId);
  setOrDelete("workNode", selection.workNodeId);
  setOrDelete("workHost", selection.workHostId);
  setOrDelete("workMember", selection.workMemberId);
  setOrDelete("workStatus", selection.workStatus);
  setOrDelete("workPriority", selection.workPriority);
  setOrDelete("workSource", selection.workSource);
  setOrDelete("workDemand", selection.workDemand);

  const query = params.toString();
  const url = `${window.location.pathname}${query ? `?${query}` : ""}${window.location.hash}`;
  const current = `${window.location.pathname}${window.location.search}${window.location.hash}`;
  if (url === current) return;
  // A location that already resolves to this selection (for example explicit
  // `?surface=work` for the default Work surface) is semantically canonical.
  // Canonicalize it in place with
  // replaceState — never pushState — so loading such a link adds no history
  // entry and browser Back can never be trapped in a canonicalization loop.
  // Genuine selection changes still pushState to preserve the Back/Forward
  // workbench journey.
  const parsedCurrent = selectionFromSearch(window.location.search, window.location.pathname);
  const parsedNext = selectionFromSearch(query ? `?${query}` : "", window.location.pathname);
  if (sameSelection(parsedCurrent, parsedNext)) {
    window.history.replaceState(null, "", url);
    return;
  }
  window.history.pushState(null, "", url);
}

const selectionCompareKeys = [
  "surface",
  "documentId",
  "agentMembershipId",
  "personId",
  "proposalId",
  "approvalId",
  "moduleId",
  "customPageId",
  "docsHealth",
  "missionId",
  "waveId",
  "teamId",
  "memberId",
  "memberRunId",
  "agentTab",
  "docPath",
  "workflowRunId",
  "workView",
  "orgView",
  "orgTeamId",
  "orgExpanded",
  "teamWorkId",
  "teamMode",
  "teamConversation",
  "agentWorkspaceMode",
  "agentSessionId",
  "teamTab",
  "teamOwner",
  "teamAttention",
  "teamQuery",
  "nodeId",
  "workTeamId",
  "workMissionId",
  "workNodeId",
  "workHostId",
  "workMemberId",
  "workStatus",
  "workPriority",
  "workSource",
  "workDemand",
] as const;

function sameSelection(left: SelectionState, right: SelectionState): boolean {
  return selectionCompareKeys.every((key) => left[key] === right[key]);
}
