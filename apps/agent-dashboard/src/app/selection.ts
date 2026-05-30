export type SurfaceId =
  | "team"
  | "vision"
  | "goal"
  | "task"
  | "graph"
  | "member"
  | "docs"
  | "decisions"
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
