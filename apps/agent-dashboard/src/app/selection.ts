export type SurfaceId =
  | "team"
  | "vision"
  | "goal"
  | "task"
  | "tasks"
  | "member"
  | "docs"
  | "warnings"
  | "debug";

export interface SelectionState {
  surface: SurfaceId;
  goalId?: string;
  teamId?: string;
  memberId?: string;
  taskId?: string;
  mode?: "kanban" | "graph" | "split";
}

export const defaultSelection: SelectionState = {
  surface: "team",
  mode: "kanban",
};

const surfaceIds: SurfaceId[] = [
  "team",
  "vision",
  "goal",
  "task",
  "tasks",
  "member",
  "docs",
  "warnings",
  "debug",
];

/**
 * Derive the URL-addressable selection from the current location. The member
 * workbench is reachable as `/members/:memberId` (canonical path form) or via
 * the query form `?surface=member&member=:id`; both resolve to the same
 * surface/member selection without a router dependency.
 */
export function selectionFromLocation(base: SelectionState): SelectionState {
  if (typeof window === "undefined") return base;
  const next: SelectionState = { ...base };

  // Path form: /members/:memberId
  const pathMatch = window.location.pathname.match(/\/members\/([^/?#]+)/);
  if (pathMatch) {
    next.surface = "member";
    next.memberId = decodeURIComponent(pathMatch[1]);
  }

  const params = new URLSearchParams(window.location.search);
  const surface = params.get("surface");
  if (surface && (surfaceIds as string[]).includes(surface)) {
    next.surface = surface as SurfaceId;
  }
  const member = params.get("member");
  if (member) {
    next.memberId = member;
  }
  const team = params.get("team");
  if (team) next.teamId = team;
  const goal = params.get("goal");
  if (goal) next.goalId = goal;
  const task = params.get("task");
  if (task) next.taskId = task;
  return next;
}

/**
 * Reflect the selection into the address bar (without reloading) so the current
 * member/surface is shareable. We use the query form as the canonical writer to
 * keep the static `base: "./"` Vite build working from any path, while still
 * accepting the `/members/:id` path form on read.
 */
export function syncSelectionToLocation(selection: SelectionState): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams();
  if (selection.surface && selection.surface !== "team") {
    params.set("surface", selection.surface);
  }
  if (selection.memberId) params.set("member", selection.memberId);
  if (selection.teamId) params.set("team", selection.teamId);
  if (selection.goalId) params.set("goal", selection.goalId);
  if (selection.taskId) params.set("task", selection.taskId);

  const query = params.toString();
  const url = `${window.location.pathname}${query ? `?${query}` : ""}${window.location.hash}`;
  const current = `${window.location.pathname}${window.location.search}${window.location.hash}`;
  if (url !== current) {
    window.history.replaceState(null, "", url);
  }
}
