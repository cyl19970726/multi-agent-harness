import { useEffect, useRef, useState } from "react";
import {
  openEventStream,
  streamSelectionKey,
  type SseFrame,
  type SseSnapshotMarker,
} from "../api";

/**
 * Live source mode surfaced by the freshness chip:
 * - `sse`: the `/v1/events` stream is connected and pushing deltas.
 * - `reconnecting`: the stream is down; bounded HTTP probes preserve freshness
 *   while the controlled reconnect ladder is active.
 * - `connecting`: an EventSource is open but the initial snapshot frame has not
 *   arrived yet.
 */
export type LiveMode = "sse" | "reconnecting" | "connecting";

export interface EventStreamState {
  mode: LiveMode;
  /** Browser receipt time of the newest snapshot marker or named frame. */
  lastActivityAt: number | null;
  /** Increments for every controlled EventSource connect attempt. */
  connectionAttempt: number;
}

export interface UseEventStreamOptions {
  /** Only connect while true (i.e. the snapshot source is live). */
  enabled: boolean;
  /** Harness API base; a change re-opens the stream against the new endpoint. */
  baseUrl: string;
  /**
   * Selected Project Binding. It remains in the request for compatibility and
   * provider-bound actions, but native coordination routing is owned by space.
   */
  project?: string | null;
  /** Selected Execution Space; this scopes the durable coordination stream. */
  space?: string | null;
  /** Selected Company Store; included in stream isolation and invalidations. */
  company?: string | null;
  /** Connection confirmed; includes the full selection captured at open. */
  onConnect: (streamKey: string, marker: SseSnapshotMarker) => boolean | void;
  /** A delta frame arrived; includes the full selection captured at open. */
  onFrame: (streamKey: string, frame: SseFrame) => void;
}

/** Reconnect backoff: 1s, 2s, 4s, 8s, capped at 15s. */
function backoffMs(attempt: number): number {
  return Math.min(15_000, 1_000 * 2 ** attempt);
}

/**
 * Subscribe to the backend SSE stream for the lifetime of `enabled`.
 *
 * Lifecycle: while enabled we open an `EventSource`; the initial `snapshot`
 * frame flips the mode to `sse`. On error/close we close the source, flip to
 * `polling` (the caller's interval poll takes over), and schedule a reconnect
 * with exponential backoff. Everything (source + pending timer) is torn down on
 * unmount, on `enabled` going false, and on `baseUrl` change so we never leak a
 * connection or push deltas onto a stale endpoint.
 */
export function useEventStream({
  enabled,
  baseUrl,
  project,
  space,
  company,
  onConnect,
  onFrame,
}: UseEventStreamOptions): EventStreamState {
  const [state, setState] = useState<EventStreamState>({
    mode: "connecting",
    lastActivityAt: null,
    connectionAttempt: 0,
  });

  // Keep the latest callbacks in refs so the effect depends only on
  // enabled/baseUrl — handler identity churn must not reconnect the stream.
  const onConnectRef = useRef(onConnect);
  const onFrameRef = useRef(onFrame);
  onConnectRef.current = onConnect;
  onFrameRef.current = onFrame;

  useEffect(() => {
    if (!enabled) {
      return;
    }

    let disposed = false;
    let closeSource: (() => void) | null = null;
    let retryTimer: number | null = null;
    let attempt = 0;
    // Capture this effect's project, rather than reading a callback ref's latest
    // selection. A late event from a just-disposed A stream can then be rejected
    // by App after the user has synchronously selected B.
    const streamKey = streamSelectionKey(space, project, company);

    const clearRetry = () => {
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer);
        retryTimer = null;
      }
    };

    const scheduleReconnect = () => {
      clearRetry();
      const delay = backoffMs(attempt);
      attempt += 1;
      retryTimer = window.setTimeout(connect, delay);
    };

    const connect = () => {
      if (disposed) return;
      setState((current) => ({
        ...current,
        mode: current.mode === "sse" ? "connecting" : current.mode,
        connectionAttempt: current.connectionAttempt + 1,
      }));
      try {
        closeSource = openEventStream(
          baseUrl,
          {
            onSnapshot: (marker) => {
              if (disposed) return;
              const accepted = onConnectRef.current(streamKey, marker);
              if (accepted === false) {
                closeSource?.();
                closeSource = null;
                setState((current) => ({ ...current, mode: "reconnecting" }));
                scheduleReconnect();
                return;
              }
              attempt = 0; // a clean, scope-confirmed connect resets backoff
              setState((current) => ({ ...current, mode: "sse", lastActivityAt: Date.now() }));
            },
            onFrame: (frame) => {
              if (disposed) return;
              setState((current) => ({ ...current, lastActivityAt: Date.now() }));
              onFrameRef.current(streamKey, frame);
            },
            onError: () => {
              if (disposed) return;
              // Tear the broken source down ourselves (EventSource would retry on
              // its own cadence and we want controlled backoff + a polling
              // fallback in the meantime).
              closeSource?.();
              closeSource = null;
              setState((current) => ({ ...current, mode: "reconnecting" }));
              scheduleReconnect();
            },
          },
          project,
          space,
          company,
        );
      } catch {
        // baseUrl was empty/invalid: stay reconnecting and retry on the ladder.
        setState((current) => ({ ...current, mode: "reconnecting" }));
        scheduleReconnect();
      }
    };

    connect();

    return () => {
      disposed = true;
      clearRetry();
      closeSource?.();
    };
  }, [enabled, baseUrl, project, space, company]);

  return state;
}
