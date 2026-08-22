/**
 * Retained navigation contract (DOC-107): the primary surfaces are Global Work,
 * durable Agent Teams, and Nodes; Providers, Projects/Workspaces and Settings
 * are secondary; Team Workspace, Host Console, Agent Workspace, and Diagnostics
 * stay deep-linkable. Company OS pages (Docs/Organization/Approvals/
 * Finance/Home), the Mission console and the duplicate snapshot-derived TeamWorks
 * path are removed — their old URLs resolve to the default Global Work surface.
 */
export type SurfaceId =
  | "work"
  | "team"
  | "operator"
  | "providers"
  | "projects"
  | "settings"
  | "agents"
  | "debug";

/** Tabs on the agent detail page. "conversation" is the default. */
export type AgentTab = "conversation" | "tasks" | "config";

const agentTabs: AgentTab[] = ["conversation", "tasks", "config"];

export interface SelectionState {
  surface: SurfaceId;
  /**
   * The selected durable Agent Team id, addressed as `?team=<id>`. Opens the
   * Team Workspace when set; the Agent Teams Home list shows when absent.
   * Historical `?team=<team-run id>` links still resolve server-side to their
   * owning durable Team, but navigation never writes a TeamRun id here.
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
  /** Selected Work row inside the Team Workspace. */
  teamWorkId?: string;
  /** Responsibility lens inside one Team: shared workspace or Host console. */
  teamMode?: "workspace" | "host";
  /** Addressed actor in the Team conversation workspace (`host` or AgentMember id). */
  teamConversation?: string;
  /** Center mode inside the unified Host/Member Agent Workspace. */
  agentWorkspaceMode?: "session" | "messages" | "work";
  /** Exact provider-native Session selected inside the Agent Workspace. */
  agentSessionId?: string;
  /** URL-owned Team Workspace tab and bounded Work filters. */
  teamTab?: "works" | "activity" | "members";
  /** URL-owned representation of the shared Work projection. */
  teamWorkView?: "graph" | "kanban";
  teamOwner?: string;
  teamAttention?: "all" | "blocked" | "review";
  teamQuery?: string;
  /** Machine selected in the Nodes responsibility view. */
  nodeId?: string;
  /** URL-backed filters for the Global Work aggregate. */
  workTeamId?: string;
  workHostId?: string;
  workMemberId?: string;
  workAssignee?: string;
  workStatus?: string;
  workPriority?: string;
}

/**
 * The Global Work aggregate is the default operating surface (DOC-107). Every
 * entity deep link below still implies its own surface.
 */
export const defaultSelection: SelectionState = {
  surface: "work",
};

const surfaceIds: SurfaceId[] = [
  "work",
  "team",
  "operator",
  "providers",
  "projects",
  "settings",
  "agents",
  "debug",
];

/**
 * Derive the URL-addressable selection from the current location. A single agent
 * is reachable as `?agent=<id>`; the legacy `/members/:id` path form is still
 * accepted and resolves to the Agents area with that agent selected.
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
  // previously-open record from leaking into Back/Forward routes after its
  // query parameter has disappeared.
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
  const agent = params.get("agent") ?? params.get("member");
  if (agent) {
    next.memberId = agent;
    if (!surface) next.surface = "agents";
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
  // Canonical durable Team address: ?team=<team id>; setting it implies the Team
  // surface (mirror of the ?agent= rule).
  if (team) {
    next.teamId = team;
    if (!surface) next.surface = "team";
  }
  // `?mission=`/`?wave=` were pre-cutover deep links. Both are ignored: Mission
  // judgment stays visible as Team context and Legacy Wave history is
  // read/export-only, never a navigation target.
  const teamWork = params.get("teamWork");
  if (teamWork) next.teamWorkId = teamWork;
  const teamMode = params.get("teamMode");
  if (teamMode === "workspace" || teamMode === "host") next.teamMode = teamMode;
  const teamConversation = params.get("conversation");
  if (teamConversation) next.teamConversation = teamConversation;
  const teamTab = params.get("teamTab");
  if (teamTab === "works" || teamTab === "activity" || teamTab === "members") next.teamTab = teamTab;
  const teamWorkView = params.get("teamWorkView");
  if (teamWorkView === "graph" || teamWorkView === "kanban") next.teamWorkView = teamWorkView;
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
    ["workHost", "workHostId"],
    ["workMember", "workMemberId"],
    ["workAssignee", "workAssignee"],
    ["workStatus", "workStatus"],
    ["workPriority", "workPriority"],
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
 * default, while an explicit non-default surface stays addressable. A location
 * that already resolves to the same selection (such as bare `?surface=work`) is
 * canonicalized in place via replaceState, never pushed, so browser Back is
 * never trapped. Execution Space, Project Binding, and API params are owned by
 * App-level sync and are never deleted here.
 */
export function syncSelectionToLocation(selection: SelectionState): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  // Mutate in place instead of delete-all-then-set: URLSearchParams.set keeps
  // an existing key's position, so an already-canonical location serializes
  // byte-identically and no spurious history entry is pushed. That is what
  // makes browser Back return from a focused object to the previous entry in
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
  setOrDelete("agent", selection.memberId);
  params.delete("member"); // legacy alias, never written
  // Only persist a non-default agent tab, and only when an agent is open.
  setOrDelete(
    "agentTab",
    selection.memberId && selection.agentTab && selection.agentTab !== "conversation"
      ? selection.agentTab
      : undefined,
  );
  setOrDelete("memberRun", selection.memberRunId);
  setOrDelete("team", selection.teamId);
  for (const retiredParam of ["mission", "wave", ["work", "flowRun"].join("")]) {
    params.delete(retiredParam);
  }
  setOrDelete("teamWork", selection.teamWorkId);
  setOrDelete("teamMode", selection.teamMode);
  setOrDelete("conversation", selection.teamConversation);
  setOrDelete("agentMode", selection.agentWorkspaceMode && selection.agentWorkspaceMode !== "session" ? selection.agentWorkspaceMode : undefined);
  setOrDelete("agentSession", selection.agentSessionId);
  setOrDelete("teamTab", selection.teamTab && selection.teamTab !== "works" ? selection.teamTab : undefined);
  setOrDelete("teamWorkView", selection.teamWorkView && selection.teamWorkView !== "graph" ? selection.teamWorkView : undefined);
  setOrDelete("teamOwner", selection.teamOwner && selection.teamOwner !== "all" ? selection.teamOwner : undefined);
  setOrDelete("teamAttention", selection.teamAttention && selection.teamAttention !== "all" ? selection.teamAttention : undefined);
  setOrDelete("teamQuery", selection.teamQuery);
  setOrDelete("node", selection.nodeId);
  setOrDelete("workTeam", selection.workTeamId);
  setOrDelete("workHost", selection.workHostId);
  setOrDelete("workMember", selection.workMemberId);
  setOrDelete("workAssignee", selection.workAssignee);
  setOrDelete("workStatus", selection.workStatus);
  setOrDelete("workPriority", selection.workPriority);

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
  "teamId",
  "memberId",
  "memberRunId",
  "agentTab",
  "teamWorkId",
  "teamMode",
  "teamConversation",
  "agentWorkspaceMode",
  "agentSessionId",
  "teamTab",
  "teamWorkView",
  "teamOwner",
  "teamAttention",
  "teamQuery",
  "nodeId",
  "workTeamId",
  "workHostId",
  "workMemberId",
  "workAssignee",
  "workStatus",
  "workPriority",
] as const;

function sameSelection(left: SelectionState, right: SelectionState): boolean {
  return selectionCompareKeys.every((key) => left[key] === right[key]);
}
