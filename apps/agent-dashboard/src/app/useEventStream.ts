import { useEffect, useRef, useState } from "react";
import { openEventStream, type SseFrame } from "../api";

/**
 * Live source mode surfaced by the freshness chip:
 * - `sse`: the `/v1/events` stream is connected and pushing deltas.
 * - `polling`: the stream is down; we fell back to interval polling.
 * - `connecting`: an EventSource is open but the initial snapshot frame has not
 *   arrived yet (we poll in this window so the view is never starved).
 */
export type LiveMode = "sse" | "polling" | "connecting";

export interface UseEventStreamOptions {
  /** Only connect while true (i.e. the snapshot source is live). */
  enabled: boolean;
  /** Harness API base; a change re-opens the stream against the new endpoint. */
  baseUrl: string;
  /** Connection confirmed (initial `snapshot` frame); caller resyncs full state. */
  onConnect: (generatedAt?: string) => void;
  /** A delta frame arrived; caller merges it into the in-memory snapshot. */
  onFrame: (frame: SseFrame) => void;
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
export function useEventStream({ enabled, baseUrl, onConnect, onFrame }: UseEventStreamOptions): LiveMode {
  const [mode, setMode] = useState<LiveMode>("connecting");

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

    const clearRetry = () => {
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer);
        retryTimer = null;
      }
    };

    const connect = () => {
      if (disposed) return;
      setMode((current) => (current === "sse" ? "connecting" : current));
      try {
        closeSource = openEventStream(baseUrl, {
          onSnapshot: (generatedAt) => {
            if (disposed) return;
            attempt = 0; // a clean connect resets the backoff ladder
            setMode("sse");
            onConnectRef.current(generatedAt);
          },
          onFrame: (frame) => {
            if (disposed) return;
            onFrameRef.current(frame);
          },
          onError: () => {
            if (disposed) return;
            // Tear the broken source down ourselves (EventSource would retry on
            // its own cadence and we want controlled backoff + a polling
            // fallback in the meantime).
            closeSource?.();
            closeSource = null;
            setMode("polling");
            clearRetry();
            const delay = backoffMs(attempt);
            attempt += 1;
            retryTimer = window.setTimeout(connect, delay);
          },
        });
      } catch {
        // baseUrl was empty/invalid: stay in polling and retry on the ladder.
        setMode("polling");
        clearRetry();
        const delay = backoffMs(attempt);
        attempt += 1;
        retryTimer = window.setTimeout(connect, delay);
      }
    };

    connect();

    return () => {
      disposed = true;
      clearRetry();
      closeSource?.();
    };
  }, [enabled, baseUrl]);

  return mode;
}
