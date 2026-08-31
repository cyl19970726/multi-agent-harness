export type AgentDockModule = "work" | "messages";

export interface AgentDockState {
  open: boolean;
  module: AgentDockModule;
  width: number;
}

export interface DockModuleStatus {
  kind: "ready" | "loading" | "error" | "unavailable" | "stale";
  message?: string;
  lastGoodAt?: string;
  onRetry?: () => void;
}

export const AGENT_DOCK_COMPACT_WIDTH = 360;
export const AGENT_DOCK_EXPANDED_WIDTH = 560;
export const AGENT_DOCK_MIN_WIDTH = 320;
export const AGENT_DOCK_MAX_WIDTH = 640;

export function clampAgentDockWidth(width: number) {
  return Math.min(AGENT_DOCK_MAX_WIDTH, Math.max(AGENT_DOCK_MIN_WIDTH, Math.round(width)));
}
