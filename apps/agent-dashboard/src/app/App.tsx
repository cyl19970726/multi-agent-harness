import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  applyFrame,
  fetchProjects,
  fetchSnapshot,
  fetchWorkflowDefs,
  matchesStreamProject,
  postAction,
  switchProject as switchProjectApi,
  SnapshotFrameBuffer,
  type SseFrame,
  type SnapshotRequestToken,
} from "../api";
import { buildWorkbenchModel } from "../model/readModel";
import type { DashboardSnapshot, Project, WorkflowDef } from "../types";
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
/**
 * Allow the harness API to be deep-linked via `?api=<url>` so a single link can
 * point the dashboard at a specific store (e.g. a second `harness serve`) without
 * hand-editing the Debug field. Falls back to the default when absent.
 */
function apiFromLocation(): string {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("api");
    return fromUrl && fromUrl.trim() ? fromUrl.trim() : apiDefault;
  } catch {
    return apiDefault;
  }
}
/**
 * localStorage key for the last-selected project id (goal-multi-project P6), so a
 * reload returns to the same project even without a `?project=` deep link.
 */
const projectStorageKey = "harness.selectedProjectId";
/**
 * Seed the selected project from the URL (`?project=<id>`) first — a deep link
 * wins — then the last choice persisted in localStorage. Returns "" when neither
 * is set, in which case the App adopts the backend's active project once the
 * project list loads. Tolerant of a missing/blocked Storage/URL (SSR, privacy).
 */
function projectFromLocation(): string {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("project");
    if (fromUrl && fromUrl.trim()) return fromUrl.trim();
  } catch {
    // fall through to localStorage
  }
  try {
    const stored = window.localStorage.getItem(projectStorageKey);
    return stored && stored.trim() ? stored.trim() : "";
  } catch {
    return "";
  }
}
/** Mirror the selected project into the URL (`?project=<id>`) without a reload so
 * the address bar is shareable; an empty id removes the param. */
function syncProjectToLocation(project: string): void {
  try {
    const url = new URL(window.location.href);
    if (project) {
      url.searchParams.set("project", project);
    } else {
      url.searchParams.delete("project");
    }
    window.history.replaceState(null, "", url.toString());
  } catch {
    // best-effort; the in-memory state remains correct
  }
}

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

function activityExpiryMs(value: string): number {
  return value.startsWith("unix-ms:") ? Number(value.slice(8)) : Date.parse(value);
}

export function App() {
  const [apiUrl, setApiUrl] = useState(apiFromLocation);
  // Selected Workspace. Seeded from URL/localStorage; "" until
  // a project is chosen or the active project is adopted from the loaded list. All
  // snapshot/SSE fetches are scoped to it so the view shows exactly one project.
  const [selectedProjectId, setSelectedProjectId] = useState<string>(projectFromLocation);
  const [projects, setProjects] = useState<Project[]>([]);
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
  // Updated before project state so a callback from the old EventSource cannot
  // merge an A frame into the newly selected B snapshot during effect cleanup.
  const selectedProjectRef = useRef(selectedProjectId);
  // Console mutations are serialized at the UI boundary. The server still
  // validates every transition, but overlapping POST responses have no safe
  // client-side ordering unless the product exposes an explicit operation id.
  const mutationInFlightRef = useRef(false);
  // A full snapshot and an SSE frame can cross in flight. Keep the tiny frame
  // journal outside React state so every fetch/action response can replay its
  // in-flight deltas before it replaces the read model.
  const snapshotFrames = useRef(new SnapshotFrameBuffer());
  const beginReadSnapshotRequest = useCallback(
    (): SnapshotRequestToken | null => snapshotFrames.current.beginReadRequest(),
    [],
  );
  const beginMutationSnapshotRequest = useCallback(
    (): SnapshotRequestToken => snapshotFrames.current.beginMutationRequest(),
    [],
  );
  const finishMutationSnapshotRequest = useCallback(
    (request: SnapshotRequestToken): void => snapshotFrames.current.finishMutation(request),
    [],
  );
  const discardSnapshotRequest = useCallback(
    (request: SnapshotRequestToken): void => snapshotFrames.current.discardRequest(request),
    [],
  );
  const adoptSnapshotResponse = useCallback(
    (request: SnapshotRequestToken, next: DashboardSnapshot): boolean => {
      const merged = snapshotFrames.current.resolveResponse(request, next);
      if (!merged) return false;
      setSnapshot(merged);
      return true;
    },
    [],
  );
  const fetchReadSnapshot = useCallback(
    async (baseUrl: string, project: string): Promise<{
      request: SnapshotRequestToken;
      snapshot: DashboardSnapshot;
    } | null> => {
      const request = beginReadSnapshotRequest();
      if (!request) return null;
      try {
        return { request, snapshot: await fetchSnapshot(baseUrl, project) };
      } catch (error) {
        discardSnapshotRequest(request);
        throw error;
      }
    },
    [beginReadSnapshotRequest, discardSnapshotRequest],
  );

  // Expiry is a data-lifecycle boundary, not merely a card-rendering choice.
  // Remove volatile previews from the shared client snapshot so Debug and every
  // other surface lose the payload too, even while SSE remains connected.
  useEffect(() => {
    const timer = window.setInterval(() => {
      const now = Date.now();
      setSnapshot((current) => {
        const activities = current.live_member_activity;
        if (!activities) return current;
        const retained = Object.entries(activities).filter(([, activity]) => {
          const expiresAt = activityExpiryMs(activity.expires_at);
          return Number.isFinite(expiresAt) && expiresAt > now;
        });
        if (retained.length === Object.keys(activities).length) return current;
        snapshotFrames.current.replaceLiveMemberActivity(
          retained.length > 0 ? Object.fromEntries(retained) : undefined,
        );
        return {
          ...current,
          live_member_activity:
            retained.length > 0 ? Object.fromEntries(retained) : undefined,
        };
      });
    }, 1_000);
    return () => window.clearInterval(timer);
  }, []);

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

  // Auto-connect to the URL-selected harness on first load so deep links and
  // capture proxies do not also issue a stray request to the default port. The
  // state already falls back to apiDefault when no `?api=` value is supplied.
  // This remains a silent attempt: explicit user actions own visible errors.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const result = await fetchReadSnapshot(apiUrl, selectedProjectId);
        if (!result) return;
        if (cancelled) {
          discardSnapshotRequest(result.request);
          return;
        }
        if (adoptSnapshotResponse(result.request, result.snapshot)) {
          setSource(liveSource);
        }
        try {
          const defs = await fetchWorkflowDefs(apiUrl);
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
  }, [adoptSnapshotResponse, apiUrl, discardSnapshotRequest, fetchReadSnapshot, selectedProjectId]);

  // Auto-retry while offline: if the initial connect failed or the backend went
  // away, silently re-attempt the default URL every few seconds so the dashboard
  // reconnects on its own — no manual button needed. Stops once live.
  useEffect(() => {
    if (source === liveSource) return;
    const id = window.setInterval(() => {
      void (async () => {
        try {
          const result = await fetchReadSnapshot(apiUrl, selectedProjectId);
          if (!result) return;
          if (adoptSnapshotResponse(result.request, result.snapshot)) {
            setSource(liveSource);
          }
        } catch {
          // still offline; retry next tick
        }
      })();
    }, 4000);
    return () => window.clearInterval(id);
  }, [source, apiUrl, selectedProjectId, adoptSnapshotResponse, fetchReadSnapshot]);

  // Load the project list (goal-multi-project P6) once a live source is up, and
  // re-load on apiUrl change (a different serve has a different registry). If no
  // project is selected yet (no URL/localStorage seed), adopt the backend's
  // active project so the picker and the scoped fetches agree from the start.
  useEffect(() => {
    if (source !== liveSource) return;
    let cancelled = false;
    void (async () => {
      try {
        const { projects: list, current } = await fetchProjects(apiUrl);
        if (cancelled) return;
        setProjects(list);
        if (!selectedProjectId && current) {
          selectedProjectRef.current = current;
          setSelectedProjectId(current);
          syncProjectToLocation(current);
        }
      } catch {
        // Single-store / old backend without /v1/projects: leave the picker empty
        // and keep the default (unscoped) snapshot — no behavior change.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [source, apiUrl]);

  // Persist + mirror the selected project so a reload (localStorage) or a shared
  // link (URL) returns to it.
  useEffect(() => {
    try {
      if (selectedProjectId) {
        window.localStorage.setItem(projectStorageKey, selectedProjectId);
      }
    } catch {
      // private mode / blocked storage: in-memory selection still works
    }
    syncProjectToLocation(selectedProjectId);
  }, [selectedProjectId]);

  // Switch the active project: clear the stale snapshot so the previous project's
  // data is never shown as current, flip the active project server-side (so
  // CLI-spawned workers converge too), then adopt the returned snapshot. The SSE
  // stream re-opens automatically — useEventStream depends on `project`, so the
  // OLD channel is torn down before the NEW one opens (acceptance: no A frames
  // while subscribed to B).
  const handleSelectProject = useCallback(
    (projectId: string) => {
      if (projectId === selectedProjectId) return;
      snapshotFrames.current.reset();
      const request = beginMutationSnapshotRequest();
      selectedProjectRef.current = projectId;
      setSelectedProjectId(projectId);
      // Drop stale data immediately so the previous project's snapshot is never
      // shown as the new one's while the switch round-trips.
      setSnapshot(emptySnapshot);
      if (source !== liveSource) {
        finishMutationSnapshotRequest(request);
        return;
      }
      void (async () => {
        try {
          const response = await switchProjectApi(apiUrl, projectId);
          if (response.snapshot) {
            adoptSnapshotResponse(request, response.snapshot);
          } else {
            adoptSnapshotResponse(request, await fetchSnapshot(apiUrl, projectId));
          }
          setSourceError(null);
        } catch (error) {
          setSourceError(error instanceof Error ? error.message : String(error));
        } finally {
          finishMutationSnapshotRequest(request);
        }
      })();
    },
    [
      adoptSnapshotResponse,
      apiUrl,
      beginMutationSnapshotRequest,
      finishMutationSnapshotRequest,
      source,
      selectedProjectId,
    ],
  );

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
      const result = await fetchReadSnapshot(apiUrl, selectedProjectId);
      if (!result) return;
      if (adoptSnapshotResponse(result.request, result.snapshot)) {
        setSource(liveSource);
      }
      try {
        setWorkflowDefs(await fetchWorkflowDefs(apiUrl));
      } catch {
        setWorkflowDefs([]);
      }
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      setSource("offline");
      // A failed manual refresh transitions away from the live connection even
      // before the stream hook's mode effect runs. Drop previews immediately so
      // offline auto-retry cannot overlay old thinking onto a fresh snapshot.
      snapshotFrames.current.clearLiveMemberActivity();
      setSnapshot(emptySnapshot);
      setWorkflowDefs([]);
    } finally {
      setIsLoading(false);
    }
  }

  // SSE connect: resync the full snapshot off /v1/snapshot. The SSE `snapshot`
  // frame is timestamp-only (per docs/agent-runtime.md), so the authoritative
  // full state still comes from a one-shot fetch when the stream (re)connects.
  const handleSseConnect = useCallback((streamProject: string) => {
    if (!matchesStreamProject(selectedProjectRef.current, streamProject)) return;
    void (async () => {
      try {
        const result = await fetchReadSnapshot(apiUrl, selectedProjectId);
        if (!result) return;
        if (adoptSnapshotResponse(result.request, result.snapshot)) {
          setSourceError(null);
        }
      } catch (error) {
        setSourceError(error instanceof Error ? error.message : String(error));
      }
    })();
  }, [adoptSnapshotResponse, apiUrl, fetchReadSnapshot, selectedProjectId]);

  // SSE delta: merge the frame into the in-memory snapshot (append/replace by
  // id, latest-wins) so the read model and Member action stream update WITHOUT
  // a full re-fetch.
  const handleSseFrame = useCallback((streamProject: string, frame: SseFrame) => {
    if (!matchesStreamProject(selectedProjectRef.current, streamProject)) return;
    snapshotFrames.current.recordFrame(frame);
    setSnapshot((current) => applyFrame(current, frame));
  }, []);

  // Open the EventSource while live; it cleans up on unmount, on leaving live,
  // and on apiUrl change. `sseMode` drives both the freshness chip and the
  // polling fallback below.
  const sseMode = useEventStream({
    enabled: isLive,
    baseUrl: apiUrl,
    project: selectedProjectId,
    onConnect: handleSseConnect,
    onFrame: handleSseFrame,
  });

  // Volatile member previews exist only for the current live connection. A
  // reconnect or polling fallback must not make old thinking look replayable.
  useEffect(() => {
    if (sseMode === "sse") return;
    snapshotFrames.current.clearLiveMemberActivity();
    setSnapshot((current) =>
      current.live_member_activity
        ? { ...current, live_member_activity: undefined }
        : current,
    );
  }, [sseMode]);

  useEffect(
    () => () => {
      snapshotFrames.current.clearLiveMemberActivity();
    },
    [],
  );

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
          const result = await fetchReadSnapshot(apiUrl, selectedProjectId);
          if (!result) return;
          if (cancelled) {
            discardSnapshotRequest(result.request);
            return;
          }
          if (adoptSnapshotResponse(result.request, result.snapshot)) {
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
  }, [
    shouldPoll,
    apiUrl,
    selectedProjectId,
    adoptSnapshotResponse,
    discardSnapshotRequest,
    fetchReadSnapshot,
  ]);

  // Returns whether the action succeeded so callers that chain actions (e.g. the
  // composer's queue-then-deliver) can stop on failure instead of clobbering the
  // first error with the next call's `setSourceError(null)`.
  async function runAction(path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }): Promise<boolean> {
    if (!isLive) return false;
    if (mutationInFlightRef.current) {
      setSourceError("Another Console action is still in progress");
      return false;
    }
    mutationInFlightRef.current = true;
    setSourceError(null);
    const request = beginMutationSnapshotRequest();
    let needsRefresh = false;
    try {
      const response = await postAction(apiUrl, path, body, selectedProjectId, options);
      if (response.snapshot) {
        adoptSnapshotResponse(request, response.snapshot);
      } else {
        needsRefresh = true;
      }
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      finishMutationSnapshotRequest(request);
      mutationInFlightRef.current = false;
    }
    if (needsRefresh) await refreshLive();
    return true;
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
        projects={projects}
        selectedProjectId={selectedProjectId}
        onSelectProject={handleSelectProject}
        onApiUrlChange={setApiUrl}
        onRefresh={refreshLive}
        onSelectionChange={setSelection}
        selection={selection}
        sourceError={sourceError}
        sourceLabel={sourceLabel}
        actionsEnabled={isLive}
        onAction={(path, body, options) => runAction(path, body, options)}
        pollEnabled={pollEnabled}
        canPoll={isLive}
        onTogglePoll={() => setPollEnabled((on) => !on)}
      />
    </TooltipProvider>
  );
}
