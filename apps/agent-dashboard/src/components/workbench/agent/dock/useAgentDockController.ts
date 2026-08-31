import { useCallback, useEffect, useState } from "react";

import {
  AGENT_DOCK_COMPACT_WIDTH,
  clampAgentDockWidth,
  type AgentDockModule,
  type AgentDockState,
} from "./types";

interface StoredDockPreference {
  open?: unknown;
  module?: unknown;
  width?: unknown;
}

function readPreference(key: string, fallback: AgentDockState): AgentDockState {
  if (typeof window === "undefined") return fallback;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(key) ?? "null") as StoredDockPreference | null;
    if (!parsed) return fallback;
    return {
      open: typeof parsed.open === "boolean" ? parsed.open : fallback.open,
      module: parsed.module === "work" || parsed.module === "messages" ? parsed.module : fallback.module,
      width: typeof parsed.width === "number" ? clampAgentDockWidth(parsed.width) : fallback.width,
    };
  } catch {
    return fallback;
  }
}

export function useAgentDockController({
  preferenceKey,
  defaultOpen = false,
  defaultModule = "work",
  defaultWidth = AGENT_DOCK_COMPACT_WIDTH,
}: {
  preferenceKey: string;
  defaultOpen?: boolean;
  defaultModule?: AgentDockModule;
  defaultWidth?: number;
}) {
  const fallback = { open: defaultOpen, module: defaultModule, width: clampAgentDockWidth(defaultWidth) } satisfies AgentDockState;
  const [state, setState] = useState<AgentDockState>(() => readPreference(preferenceKey, fallback));

  useEffect(() => {
    setState(readPreference(preferenceKey, fallback));
  }, [preferenceKey]);

  useEffect(() => {
    try {
      window.localStorage.setItem(preferenceKey, JSON.stringify(state));
    } catch {
      // Browser-local preference failure never affects canonical Work or Message reads.
    }
  }, [preferenceKey, state]);

  const open = useCallback((module?: AgentDockModule) => {
    setState((current) => ({ ...current, open: true, module: module ?? current.module }));
  }, []);
  const close = useCallback(() => setState((current) => ({ ...current, open: false })), []);
  const selectModule = useCallback((module: AgentDockModule) => setState((current) => ({ ...current, open: true, module })), []);
  const setWidth = useCallback((width: number) => setState((current) => ({ ...current, width: clampAgentDockWidth(width) })), []);
  const toggleExpanded = useCallback(() => setState((current) => ({
    ...current,
    width: current.width >= 520 ? AGENT_DOCK_COMPACT_WIDTH : 560,
  })), []);

  return { state, setState, open, close, selectModule, setWidth, toggleExpanded };
}
