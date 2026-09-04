import { useEffect, useRef } from "react";

import type { LiveMode } from "./useEventStream";

/** Preserve the existing recovery cadence while the event stream is degraded. */
export const degradedSnapshotPollIntervalMs = 5_000;
/** A healthy SSE stream owns freshness; opt-in polling becomes a slow safety net. */
export const healthySnapshotPollIntervalMs = 15_000;

export interface SnapshotPollingOptions {
  isLive: boolean;
  pollEnabled: boolean;
  streamMode: LiveMode;
  onPoll: () => void;
}

export function snapshotPollIntervalMs({
  isLive,
  pollEnabled,
  streamMode,
}: Omit<SnapshotPollingOptions, "onPoll">): number | null {
  if (!isLive) return null;
  if (streamMode !== "sse") return degradedSnapshotPollIntervalMs;
  return pollEnabled ? healthySnapshotPollIntervalMs : null;
}

/**
 * Poll `/v1/snapshot` only as a transport-health fallback. A disconnected or
 * connecting SSE stream uses the historical five-second cadence. Once SSE is
 * healthy, named invalidation frames drive refreshes; the operator's opt-in
 * poll remains available only as a documented 15-second safety net.
 */
export function useSnapshotPolling(options: SnapshotPollingOptions): void {
  const onPollRef = useRef(options.onPoll);
  onPollRef.current = options.onPoll;
  const intervalMs = snapshotPollIntervalMs(options);

  useEffect(() => {
    if (intervalMs === null) return;
    const timer = globalThis.setInterval(() => onPollRef.current(), intervalMs);
    return () => globalThis.clearInterval(timer);
  }, [intervalMs]);
}
