import { useCallback, useEffect, useMemo, useState } from "react";
import { applyFrame, fetchSnapshot, postAction, type SseFrame } from "../api";
import { demoSnapshot } from "../model/demoSnapshot";
import { buildWorkbenchModel } from "../model/readModel";
import type { DashboardSnapshot } from "../types";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  defaultSelection,
  selectionFromLocation,
  syncSelectionToLocation,
  type SelectionState,
} from "./selection";
import { useEventStream } from "./useEventStream";
import { WorkbenchShell } from "./WorkbenchShell";

const apiDefault = "http://127.0.0.1:8787";
/** Canonical "snapshot came from the live harness" marker; gates write actions. */
const liveSource = "live";
const offlineLabel = "offline fixture";
/** Live-poll cadence: re-fetch /v1/snapshot roughly every 5s while polling. */
const pollIntervalMs = 5000;

export function App() {
  const [apiUrl, setApiUrl] = useState(apiDefault);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot>(demoSnapshot);
  // The snapshot's provenance, NOT its display label: `live` once a live
  // /v1/snapshot has loaded (enabling SSE, polling and write actions), else the
  // offline design fixture. The user-facing chip label is derived below.
  const [source, setSource] = useState<typeof liveSource | "offline">("offline");
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  // Manual opt-in interval poll (FE-WP5). Independent of the automatic polling
  // fallback that kicks in whenever the SSE stream is down.
  const [pollEnabled, setPollEnabled] = useState(false);
  // Seed selection from the URL so a member view (?surface=member&member=:id,
  // i.e. the /members/:memberId workbench) is directly addressable and
  // deep-linkable without pulling in a router.
  const [selection, setSelection] = useState<SelectionState>(() => selectionFromLocation(defaultSelection));

  // Keep the URL in sync with the selected surface/member so the address bar is
  // shareable, and honour Back/Forward navigation.
  useEffect(() => {
    syncSelectionToLocation(selection);
  }, [selection]);

  useEffect(() => {
    const onPopState = () => setSelection((current) => selectionFromLocation(current));
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  const model = useMemo(() => buildWorkbenchModel(snapshot, selection), [snapshot, selection]);

  // Actions are only honest against a live snapshot; the offline fixture is read-only.
  const isLive = source === liveSource;

  async function refreshLive() {
    setIsLoading(true);
    setSourceError(null);
    try {
      const next = await fetchSnapshot(apiUrl);
      setSnapshot(next);
      setSource(liveSource);
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      setSource("offline");
      setSnapshot(demoSnapshot);
    } finally {
      setIsLoading(false);
    }
  }

  // SSE connect: resync the full snapshot off /v1/snapshot. The SSE `snapshot`
  // frame is timestamp-only (per docs/agent-runtime.md), so the authoritative
  // full state still comes from a one-shot fetch when the stream (re)connects.
  const handleSseConnect = useCallback(() => {
    void (async () => {
      try {
        const next = await fetchSnapshot(apiUrl);
        setSnapshot(next);
        setSourceError(null);
      } catch (error) {
        setSourceError(error instanceof Error ? error.message : String(error));
      }
    })();
  }, [apiUrl]);

  // SSE delta: merge the frame into the in-memory snapshot (append/replace by
  // id, latest-wins) so the read model and Member action stream update WITHOUT
  // a full re-fetch.
  const handleSseFrame = useCallback((frame: SseFrame) => {
    setSnapshot((current) => applyFrame(current, frame));
  }, []);

  // Open the EventSource while live; it cleans up on unmount, on leaving live,
  // and on apiUrl change. `sseMode` drives both the freshness chip and the
  // polling fallback below.
  const sseMode = useEventStream({
    enabled: isLive,
    baseUrl: apiUrl,
    onConnect: handleSseConnect,
    onFrame: handleSseFrame,
  });

  // Interval poll of /v1/snapshot. Runs while live AND either the operator
  // opted in (FE-WP5) OR the SSE stream is not currently connected (automatic
  // fallback so the view keeps refreshing during an outage/reconnect). A failed
  // poll surfaces the error but keeps the last good snapshot — it does not tear
  // the view down to the demo fixture the way a manual "Load live" failure
  // does. The interval is cleared on unmount, when it is no longer needed, and
  // whenever apiUrl changes so we never poll a stale endpoint.
  const shouldPoll = isLive && (pollEnabled || sseMode !== "sse");
  useEffect(() => {
    if (!shouldPoll) return;
    let cancelled = false;
    const id = window.setInterval(() => {
      void (async () => {
        try {
          const next = await fetchSnapshot(apiUrl);
          if (!cancelled) {
            setSnapshot(next);
            setSourceError(null);
          }
        } catch (error) {
          if (!cancelled) {
            setSourceError(error instanceof Error ? error.message : String(error));
          }
        }
      })();
    }, pollIntervalMs);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [shouldPoll, apiUrl]);

  async function runAction(path: string, body?: unknown) {
    if (!isLive) return;
    setSourceError(null);
    try {
      const response = await postAction(apiUrl, path, body);
      if (response.snapshot) {
        setSnapshot(response.snapshot);
      } else {
        await refreshLive();
      }
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
    }
  }

  // Freshness chip label: which source mode is actually feeding the view.
  // "live (SSE)" while the stream is connected, "polling" once we fall back,
  // "offline fixture" when no live source is loaded.
  const sourceLabel = !isLive
    ? offlineLabel
    : sseMode === "sse"
      ? "live (SSE)"
      : "polling";

  return (
    <TooltipProvider delayDuration={200}>
      <WorkbenchShell
        apiUrl={apiUrl}
        isLoading={isLoading}
        model={model}
        onApiUrlChange={setApiUrl}
        onRefresh={refreshLive}
        onSelectionChange={setSelection}
        selection={selection}
        sourceError={sourceError}
        sourceLabel={sourceLabel}
        actionsEnabled={isLive}
        onAction={(path, body) => void runAction(path, body)}
        pollEnabled={pollEnabled}
        canPoll={isLive}
        onTogglePoll={() => setPollEnabled((on) => !on)}
      />
    </TooltipProvider>
  );
}
