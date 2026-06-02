import { useCallback, useEffect, useMemo, useState } from "react";
import {
  applyFrame,
  fetchSnapshot,
  fetchWorkflowDefs,
  postAction,
  type SseFrame,
} from "../api";
import { buildWorkbenchModel } from "../model/readModel";
import type { DashboardSnapshot, WorkflowDef } from "../types";
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
const offlineLabel = "not connected";
/**
 * Before a live `/v1/snapshot` loads (and after a failed Load live), the view
 * holds an empty workspace — no baked-in demo agents/goals/learning artifacts.
 * Every snapshot field is optional, so `{}` renders honest empty states across
 * all surfaces ("No agents yet", "No visions recorded", empty Work board). The
 * only way objects appear is creating them (live) or connecting to a harness
 * that already has them.
 */
const emptySnapshot: DashboardSnapshot = {};
/** Live-poll cadence: re-fetch /v1/snapshot roughly every 5s while polling. */
const pollIntervalMs = 5000;

export function App() {
  const [apiUrl, setApiUrl] = useState(apiDefault);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot>(emptySnapshot);
  // The registered workflow catalog (GET /v1/workflows) is run-independent and
  // lives outside the snapshot, so it is fetched alongside the snapshot.
  const [workflowDefs, setWorkflowDefs] = useState<WorkflowDef[]>([]);
  // The snapshot's provenance, NOT its display label: `live` once a live
  // /v1/snapshot has loaded (enabling SSE, polling and write actions), else an
  // empty (not-connected) workspace. The user-facing chip label is derived below.
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

  // Auto-connect to the default harness on first load so the dashboard shows
  // real data immediately — no manual "Load live" click. This is a silent
  // attempt: on failure we stay in the empty "not connected" state WITHOUT
  // raising the error banner (that is reserved for an explicit Load live / write
  // the user triggered). Runs once on mount against the default API URL.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const next = await fetchSnapshot(apiDefault);
        if (!cancelled) {
          setSnapshot(next);
          setSource(liveSource);
        }
        try {
          const defs = await fetchWorkflowDefs(apiDefault);
          if (!cancelled) setWorkflowDefs(defs);
        } catch {
          // Catalog is best-effort; the surface shows an "unavailable" state.
        }
      } catch {
        // Stay offline/empty; the auto-retry effect below keeps trying.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Auto-retry while offline: if the initial connect failed or the backend went
  // away, silently re-attempt the default URL every few seconds so the dashboard
  // reconnects on its own — no manual button needed. Stops once live.
  useEffect(() => {
    if (source === liveSource) return;
    const id = window.setInterval(() => {
      void (async () => {
        try {
          const next = await fetchSnapshot(apiUrl);
          setSnapshot(next);
          setSource(liveSource);
        } catch {
          // still offline; retry next tick
        }
      })();
    }, 4000);
    return () => window.clearInterval(id);
  }, [source, apiUrl]);

  const model = useMemo(
    () => buildWorkbenchModel(snapshot, selection, workflowDefs),
    [snapshot, selection, workflowDefs],
  );

  // Actions are only honest against a live snapshot; an empty workspace is read-only.
  const isLive = source === liveSource;

  async function refreshLive() {
    setIsLoading(true);
    setSourceError(null);
    try {
      const next = await fetchSnapshot(apiUrl);
      setSnapshot(next);
      setSource(liveSource);
      try {
        setWorkflowDefs(await fetchWorkflowDefs(apiUrl));
      } catch {
        setWorkflowDefs([]);
      }
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      setSource("offline");
      setSnapshot(emptySnapshot);
      setWorkflowDefs([]);
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
  // the view down to the empty workspace the way a manual "Load live" failure
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

  // Returns whether the action succeeded so callers that chain actions (e.g. the
  // composer's queue-then-deliver) can stop on failure instead of clobbering the
  // first error with the next call's `setSourceError(null)`.
  async function runAction(path: string, body?: unknown): Promise<boolean> {
    if (!isLive) return false;
    setSourceError(null);
    try {
      const response = await postAction(apiUrl, path, body);
      if (response.snapshot) {
        setSnapshot(response.snapshot);
      } else {
        await refreshLive();
      }
      return true;
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      return false;
    }
  }

  // Freshness chip label: which source mode is actually feeding the view.
  // "live (SSE)" while the stream is connected, "polling" once we fall back,
  // "not connected" when no live source is loaded.
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
        onAction={(path, body) => runAction(path, body)}
        pollEnabled={pollEnabled}
        canPoll={isLive}
        onTogglePoll={() => setPollEnabled((on) => !on)}
      />
    </TooltipProvider>
  );
}
