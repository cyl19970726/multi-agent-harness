import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  applyFrame,
  fetchCompanies,
  fetchProjects,
  fetchSpaces,
  fetchSnapshot,
  fetchTeamRunSnapshot,
  fetchWorkflowDefs,
  postAction,
  ProjectionInvalidationTracker,
  streamSelectionKey,
  switchProject as switchProjectApi,
  switchSpace as switchSpaceApi,
  SnapshotFrameBuffer,
  type SseFrame,
  type SseSnapshotMarker,
  type SnapshotRequestToken,
} from "../api";
import { buildWorkbenchModel } from "../model/readModel";
import type { Company, DashboardSnapshot, ExecutionSpace, Project, WorkflowDef } from "../types";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  defaultSelection,
  selectionFromLocation,
  syncSelectionToLocation,
  type SelectionState,
} from "./selection";
import { useEventStream } from "./useEventStream";
import { WorkbenchShell } from "./WorkbenchShell";
import {
  freshnessDomains,
  freshnessDomainsForInvalidation,
  uniformFreshness,
  updateFreshness,
  type DomainFreshness,
  type FreshnessDomain,
} from "./freshness";

declare global {
  interface Window {
    __AGENTFIRM_BOOTSTRAP__?: { apiBase?: string; capabilityToken?: string };
  }
}
const apiDefault = typeof window !== "undefined" && /^https?:$/.test(window.location.protocol)
  ? window.location.origin
  : "http://127.0.0.1:8787";
/**
 * Allow the harness API to be deep-linked via `?api=<url>` so a single link can
 * point the dashboard at a specific store (e.g. a second `harness serve`) without
 * hand-editing the Debug field. Falls back to the default when absent.
 */
function apiFromLocation(): string {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("api");
    return fromUrl && fromUrl.trim()
      ? fromUrl.trim()
      : window.__AGENTFIRM_BOOTSTRAP__?.apiBase?.trim() || apiDefault;
  } catch {
    return apiDefault;
  }
}
/**
 * localStorage key for the last-selected project id (goal-multi-project P6), so a
 * reload returns to the same project even without a `?project=` deep link.
 */
const projectStorageKey = "harness.selectedProjectId";
const spaceStorageKey = "harness.selectedSpaceId";
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

function companyFromLocation(): string {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("company");
    if (fromUrl && fromUrl.trim()) return fromUrl.trim();
  } catch {
    // Fall through to the backend registry default. Company selection is
    // deliberately URL/in-memory scoped and never shared through localStorage.
  }
  return "";
}

function spaceFromLocation(): string {
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("space");
    if (fromUrl?.trim()) return fromUrl.trim();
  } catch {
    // fall through
  }
  try {
    return window.localStorage.getItem(spaceStorageKey)?.trim() ?? "";
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

function syncCompanyToLocation(company: string): void {
  try {
    const url = new URL(window.location.href);
    if (company) {
      url.searchParams.set("company", company);
    } else {
      url.searchParams.delete("company");
    }
    window.history.replaceState(null, "", url.toString());
  } catch {
    // best-effort; the in-memory state remains correct
  }
}

function syncSpaceToLocation(space: string): void {
  try {
    const url = new URL(window.location.href);
    if (space) url.searchParams.set("space", space);
    else url.searchParams.delete("space");
    window.history.replaceState(null, "", url.toString());
  } catch {
    // best effort
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
/** One bounded authoritative probe per quiet connection epoch. SSE comments
 * are invisible to EventSource, so a successful probe is sufficient to restore
 * freshness even when no named event arrives. */
const streamStaleAfterMs = 45_000;
type FreshnessState = "live" | "reconnecting" | "stale";

function activityExpiryMs(value: string): number {
  return value.startsWith("unix-ms:") ? Number(value.slice(8)) : Date.parse(value);
}

function freshnessAfterSnapshot(runtimeConnected: boolean): DomainFreshness {
  return {
    ...uniformFreshness("live"),
    runtime: runtimeConnected ? "live" : "reconnecting",
  };
}

export function App() {
  const [apiUrl, setApiUrl] = useState(apiFromLocation);
  // Selected Workspace. Seeded from URL/localStorage; "" until
  // a project is chosen or the active project is adopted from the loaded list. All
  // snapshot/SSE fetches are scoped to it so the view shows exactly one project.
  const [selectedProjectId, setSelectedProjectId] = useState<string>(projectFromLocation);
  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedSpaceId, setSelectedSpaceId] = useState<string>(spaceFromLocation);
  const [spaces, setSpaces] = useState<ExecutionSpace[]>([]);
  const [selectedCompanyId, setSelectedCompanyId] = useState<string>(companyFromLocation);
  const [companies, setCompanies] = useState<Company[]>([]);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot>(emptySnapshot);
  // The registered workflow catalog (GET /v1/workflows) is run-independent and
  // lives outside the snapshot, so it is fetched alongside the snapshot.
  const [workflowDefs, setWorkflowDefs] = useState<WorkflowDef[]>([]);
  // The snapshot's provenance, NOT its display label: `live` once a live
  // /v1/snapshot has loaded (enabling SSE, polling and write actions), else an
  // empty (not-connected) workspace. The user-facing chip label is derived below.
  const [source, setSource] = useState<typeof liveSource | "offline">("offline");
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [selectorRecoveryNotice, setSelectorRecoveryNotice] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  // Manual opt-in interval poll (FE-WP5). Automatic recovery is bounded and
  // edge-triggered; it never enables this permanent interval implicitly.
  const [pollEnabled, setPollEnabled] = useState(false);
  const [freshnessState, setFreshnessState] = useState<FreshnessState>("reconnecting");
  const [domainFreshness, setDomainFreshness] = useState<DomainFreshness>(() =>
    uniformFreshness("reconnecting"),
  );
  const [selectorRefreshGeneration, setSelectorRefreshGeneration] = useState(0);
  // Seed selection from the URL so a member view (?surface=member&member=:id,
  // i.e. the /members/:memberId workbench) is directly addressable and
  // deep-linkable without pulling in a router.
  const [selection, setSelection] = useState<SelectionState>(() => selectionFromLocation(defaultSelection));
  // Updated before selection state so a callback from the old EventSource
  // cannot merge one Execution Space's frame into another during cleanup.
  const selectedStreamRef = useRef(
    streamSelectionKey(selectedSpaceId, selectedProjectId, selectedCompanyId),
  );
  const confirmedScopeRef = useRef<{
    executionSpaceId?: string;
    companyScopeId?: string;
  }>({});
  const invalidationTracker = useRef(new ProjectionInvalidationTracker());
  const resyncGenerationRef = useRef(0);
  const resyncInFlightRef = useRef(false);
  const resyncDirtyRef = useRef(false);
  const resyncRunnerRef = useRef<(() => void) | null>(null);
  const resyncRetryTimerRef = useRef<number | null>(null);
  const resyncRetryAttemptRef = useRef(0);
  const staleConnectionAttemptRef = useRef<number | null>(null);
  const disconnectedProbeAttemptRef = useRef<number | null>(null);
  const streamConnectedRef = useRef(false);
  const streamScopeTrustedRef = useRef(true);
  const readProjectionKeyRef = useRef(
    selection.surface === "team" && selection.teamId ? `team:${selection.teamId}` : "full",
  );
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
    async (baseUrl: string, project: string, company: string, space: string): Promise<{
      request: SnapshotRequestToken;
      snapshot: DashboardSnapshot;
    } | null> => {
      const request = beginReadSnapshotRequest();
      if (!request) return null;
      try {
        // Resolve a team definition id (team-xxx, not team-run-xxx) to its
        // latest team-run id before calling fetchTeamRunSnapshot so the
        // backend receives a valid team-run id from the very first request.
        let effectiveTeamId = selection.teamId;
        if (
          selection.surface === "team" &&
          effectiveTeamId &&
          effectiveTeamId.startsWith("team-") &&
          !effectiveTeamId.startsWith("team-run-")
        ) {
          try {
            const fullSnapshot = await fetchSnapshot(baseUrl, project, company, space);
            const matchingRun = (fullSnapshot.team_runs ?? [])
              .filter((run) => run.agent_team_id === effectiveTeamId)
              .sort((a, b) => (b.created_at ?? "").localeCompare(a.created_at ?? ""))[0];
            if (matchingRun) {
              effectiveTeamId = matchingRun.id;
              setSelection((prev) =>
                prev.teamId === selection.teamId
                  ? { ...prev, teamId: effectiveTeamId }
                  : prev,
              );
            }
          } catch {
            // Resolution failed; leave effectiveTeamId as-is so
            // fetchTeamRunSnapshot surfaces the real error.
          }
        }
        const next = selection.surface === "team" && effectiveTeamId
          ? await fetchTeamRunSnapshot(baseUrl, effectiveTeamId, project, company, space)
          : await fetchSnapshot(baseUrl, project, company, space);
        return { request, snapshot: next };
      } catch (error) {
        discardSnapshotRequest(request);
        throw error;
      }
    },
    [beginReadSnapshotRequest, discardSnapshotRequest, selection.surface, selection.teamId],
  );

  /**
   * Coalesce freshness signals into scoped authoritative reads. At most one
   * request is in flight; a signal received during that request occupies one
   * dirty slot and causes a follow-up read. SnapshotFrameBuffer still owns the
   * HTTP/SSE causal crossing, while the selection key rejects another
   * Execution Space or Company before any response can commit.
   */
  const requestAuthoritativeResync = useCallback((
    affectedDomains: readonly FreshnessDomain[] = freshnessDomains,
  ): void => {
    resyncDirtyRef.current = true;
    setFreshnessState("stale");
    setDomainFreshness((current) => updateFreshness(current, affectedDomains, "stale"));

    const drain = () => {
      if (resyncInFlightRef.current || !resyncDirtyRef.current) return;
      if (resyncRetryTimerRef.current !== null) {
        window.clearTimeout(resyncRetryTimerRef.current);
        resyncRetryTimerRef.current = null;
      }
      resyncInFlightRef.current = true;
      void (async () => {
        let committed = false;
        let passes = 0;
        let failed = false;
        let superseded = false;
        const drainGeneration = resyncGenerationRef.current;
        try {
          // One initial read plus one dirty follow-up. Further writes retain the
          // dirty bit and schedule another bounded drain after this burst.
          while (resyncDirtyRef.current && passes < 2) {
            resyncDirtyRef.current = false;
            passes += 1;
            const streamKey = selectedStreamRef.current;
            const generation = resyncGenerationRef.current;
            const result = await fetchReadSnapshot(
              apiUrl,
              selectedProjectId,
              selectedCompanyId,
              selectedSpaceId,
            );
            if (!result) {
              // A mutation currently owns the causal driver. Its finally block
              // restarts this drain after the mutation response is settled.
              resyncDirtyRef.current = true;
              break;
            }
            if (generation !== resyncGenerationRef.current) {
              discardSnapshotRequest(result.request);
              superseded = true;
              break;
            }
            if (streamKey !== selectedStreamRef.current) {
              discardSnapshotRequest(result.request);
              superseded = true;
              break;
            }
            if (adoptSnapshotResponse(result.request, result.snapshot)) {
              committed = true;
              resyncRetryAttemptRef.current = 0;
              setSource(liveSource);
              setSourceError(null);
              setDomainFreshness(freshnessAfterSnapshot(streamConnectedRef.current));
              setSelectorRefreshGeneration((current) => current + 1);
            } else if (
              streamKey === selectedStreamRef.current
              && generation === resyncGenerationRef.current
            ) {
              // Another read outranked this response. Never raw-install it;
              // retain a dirty signal until one authoritative read commits.
              resyncDirtyRef.current = true;
            }
          }
        } catch (error) {
          if (drainGeneration !== resyncGenerationRef.current) {
            superseded = true;
          } else {
            failed = true;
            resyncDirtyRef.current = true;
            setSourceError(error instanceof Error ? error.message : String(error));
          }
        } finally {
          resyncInFlightRef.current = false;
          if (
            committed
            && !failed
            && !resyncDirtyRef.current
            && drainGeneration === resyncGenerationRef.current
          ) {
            setFreshnessState(streamConnectedRef.current ? "live" : "reconnecting");
          }
          if (superseded) {
            // A scope boundary cannot cancel fetch(), but it invalidates this
            // whole drain. The newest signal has already installed its runner;
            // hand the retained dirty bit to that runner without an old pass 2.
            window.setTimeout(() => resyncRunnerRef.current?.(), 0);
          } else if (failed && drainGeneration === resyncGenerationRef.current) {
            const delay = Math.min(15_000, 1_000 * 2 ** resyncRetryAttemptRef.current);
            resyncRetryAttemptRef.current += 1;
            resyncRetryTimerRef.current = window.setTimeout(() => {
              resyncRetryTimerRef.current = null;
              resyncRunnerRef.current?.();
            }, delay);
          } else if (resyncDirtyRef.current && passes >= 2) {
            window.setTimeout(() => resyncRunnerRef.current?.(), 100);
          }
        }
      })();
    };
    // Install the latest closure even while an older scope owns the physical
    // request. Its finally block can then hand off to this runner safely.
    resyncRunnerRef.current = drain;
    if (resyncInFlightRef.current) return;
    drain();
  }, [
    adoptSnapshotResponse,
    apiUrl,
    discardSnapshotRequest,
    fetchReadSnapshot,
    selectedCompanyId,
    selectedProjectId,
    selectedSpaceId,
  ]);

  const moveStreamBoundary = useCallback((nextStreamKey: string): void => {
    selectedStreamRef.current = nextStreamKey;
    confirmedScopeRef.current = {};
    invalidationTracker.current.reset();
    snapshotFrames.current.reset();
    resyncGenerationRef.current += 1;
    resyncDirtyRef.current = false;
    if (resyncRetryTimerRef.current !== null) {
      window.clearTimeout(resyncRetryTimerRef.current);
      resyncRetryTimerRef.current = null;
    }
    resyncRetryAttemptRef.current = 0;
    staleConnectionAttemptRef.current = null;
    disconnectedProbeAttemptRef.current = null;
    streamConnectedRef.current = false;
    streamScopeTrustedRef.current = true;
    setFreshnessState("reconnecting");
    setDomainFreshness(uniformFreshness("reconnecting"));
  }, []);

  // TeamRun focus may use a deliberately bounded snapshot, but that response
  // must never become the backing model for Work/Docs/Org after navigation.
  // Crossing either direction performs one authoritative read for the newly
  // selected projection.
  useEffect(() => {
    const next = selection.surface === "team" && selection.teamId
      ? `team:${selection.teamId}`
      : "full";
    if (next === readProjectionKeyRef.current) return;
    readProjectionKeyRef.current = next;
    // A bounded TeamRun snapshot and the full Dashboard snapshot are distinct
    // causal projections even though they share one SSE scope. Supersede the
    // complete old drain so its response and dirty pass 2 cannot cross this
    // boundary using a stale selection closure.
    resyncGenerationRef.current += 1;
    resyncDirtyRef.current = false;
    snapshotFrames.current.reset();
    if (resyncRetryTimerRef.current !== null) {
      window.clearTimeout(resyncRetryTimerRef.current);
      resyncRetryTimerRef.current = null;
    }
    resyncRetryAttemptRef.current = 0;
    setSnapshot(emptySnapshot);
    if (source === liveSource) requestAuthoritativeResync();
  }, [requestAuthoritativeResync, selection.surface, selection.teamId, source]);

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
      setIsLoading(true);
      try {
        const result = await fetchReadSnapshot(apiUrl, selectedProjectId, selectedCompanyId, selectedSpaceId);
        if (!result) return;
        if (cancelled) {
          discardSnapshotRequest(result.request);
          return;
        }
        if (adoptSnapshotResponse(result.request, result.snapshot)) {
          setSource(liveSource);
          setDomainFreshness(freshnessAfterSnapshot(streamConnectedRef.current));
        }
        try {
          const defs = await fetchWorkflowDefs(apiUrl);
          if (!cancelled) setWorkflowDefs(defs);
        } catch {
          // Catalog is best-effort; the surface shows an "unavailable" state.
        }
      } catch {
        // Stay offline/empty; the auto-retry effect below keeps trying.
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [adoptSnapshotResponse, apiUrl, discardSnapshotRequest, fetchReadSnapshot, selectedCompanyId, selectedProjectId, selectedSpaceId]);

  // Auto-retry while offline: if the initial connect failed or the backend went
  // away, silently re-attempt the default URL every few seconds so the dashboard
  // reconnects on its own — no manual button needed. Stops once live.
  useEffect(() => {
    if (source === liveSource) return;
    const id = window.setInterval(() => {
      void (async () => {
        try {
          const result = await fetchReadSnapshot(apiUrl, selectedProjectId, selectedCompanyId, selectedSpaceId);
          if (!result) return;
          if (adoptSnapshotResponse(result.request, result.snapshot)) {
            setSource(liveSource);
            setDomainFreshness(freshnessAfterSnapshot(streamConnectedRef.current));
          }
        } catch {
          // still offline; retry next tick
        }
      })();
    }, 4000);
    return () => window.clearInterval(id);
  }, [source, apiUrl, selectedCompanyId, selectedProjectId, selectedSpaceId, adoptSnapshotResponse, fetchReadSnapshot]);

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
          selectedStreamRef.current = streamSelectionKey(
            selectedSpaceId,
            current,
            selectedCompanyId,
          );
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

  // Bootstrap truth selectors independently from snapshot success. A stale URL
  // or localStorage id must not brick boot now that the runtime correctly 404s
  // unknown spaces/companies. Reconcile both selectors atomically to registry
  // `current`, then perform a fresh explicitly-scoped read.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [spaceResult, companyResult] = await Promise.allSettled([
        fetchSpaces(apiUrl),
        fetchCompanies(apiUrl),
      ]);
      if (cancelled) return;
      const spaceData = spaceResult.status === "fulfilled" ? spaceResult.value : null;
      const companyData = companyResult.status === "fulfilled" ? companyResult.value : null;
      if (spaceData) setSpaces(spaceData.spaces);
      if (companyData) setCompanies(companyData.companies);

      const spaceValid = !spaceData || !selectedSpaceId
        || Boolean(spaceData?.spaces.some((space) => space.id === selectedSpaceId));
      const companyValid = !companyData || !selectedCompanyId
        || Boolean(companyData?.companies.some((company) => company.id === selectedCompanyId));
      const fallbackSpace = spaceData?.spaces.some((space) => space.id === spaceData.current)
        ? spaceData.current
        : spaceData?.spaces[0]?.id ?? "";
      const fallbackCompany = companyData?.companies.some((company) => company.id === companyData.current)
        ? companyData.current
        : companyData?.companies[0]?.id ?? "";
      const nextSpace = !selectedSpaceId
        ? fallbackSpace
        : !spaceData
          ? selectedSpaceId
        : spaceValid
          ? selectedSpaceId
          : fallbackSpace;
      const nextCompany = !selectedCompanyId
        ? fallbackCompany
        : !companyData
          ? selectedCompanyId
        : companyValid
          ? selectedCompanyId
          : fallbackCompany;
      if (nextSpace === selectedSpaceId && nextCompany === selectedCompanyId) return;

      const notices: string[] = [];
      if (selectedSpaceId && nextSpace !== selectedSpaceId) {
        notices.push(nextSpace
          ? `Execution Space "${selectedSpaceId}" was not found; recovered to "${nextSpace}".`
          : `Execution Space "${selectedSpaceId}" was not found; cleared the stale selection.`);
      }
      if (selectedCompanyId && nextCompany !== selectedCompanyId) {
        notices.push(nextCompany
          ? `Company "${selectedCompanyId}" was not found; recovered to "${nextCompany}".`
          : `Company "${selectedCompanyId}" was not found; cleared the stale selection.`);
      }
      moveStreamBoundary(streamSelectionKey(nextSpace, selectedProjectId, nextCompany));
      if (nextSpace !== selectedSpaceId) {
        setSelectedSpaceId(nextSpace);
        syncSpaceToLocation(nextSpace);
      }
      if (nextCompany !== selectedCompanyId) {
        setSelectedCompanyId(nextCompany);
        syncCompanyToLocation(nextCompany);
      }
      if (notices.length > 0) setSelectorRecoveryNotice(notices.join(" "));
    })();
    return () => {
      cancelled = true;
    };
  }, [apiUrl, moveStreamBoundary, selectedCompanyId, selectedProjectId, selectedSpaceId, selectorRefreshGeneration]);

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

  useEffect(() => {
    try {
      if (selectedSpaceId) window.localStorage.setItem(spaceStorageKey, selectedSpaceId);
      else window.localStorage.removeItem(spaceStorageKey);
    } catch {
      // in-memory state remains authoritative for this tab
    }
    syncSpaceToLocation(selectedSpaceId);
  }, [selectedSpaceId]);

  useEffect(() => syncCompanyToLocation(selectedCompanyId), [selectedCompanyId]);

  // Switch the default Project Binding. In native Execution Space mode this
  // changes provider cwd/config/Skill context only, so coordination stays
  // visible. Compatibility backends retain the historical snapshot switch.
  const handleSelectProject = useCallback(
    (projectId: string) => {
      if (projectId === selectedProjectId) return;
      setSelectorRecoveryNotice(null);
      moveStreamBoundary(streamSelectionKey(selectedSpaceId, projectId, selectedCompanyId));
      setSelectedProjectId(projectId);
      if (source !== liveSource) return;
      if (selectedSpaceId) {
        void switchProjectApi(apiUrl, projectId)
          .then(() => setSourceError(null))
          .catch((error) =>
            setSourceError(error instanceof Error ? error.message : String(error)),
          );
        return;
      }

      const request = beginMutationSnapshotRequest();
      setIsLoading(true);
      setSnapshot(emptySnapshot);
      void (async () => {
        try {
          const response = await switchProjectApi(apiUrl, projectId);
          if (response.snapshot) {
            adoptSnapshotResponse(request, response.snapshot);
          } else {
            adoptSnapshotResponse(request, await fetchSnapshot(apiUrl, projectId, selectedCompanyId, selectedSpaceId));
          }
          setSourceError(null);
        } catch (error) {
          setSourceError(error instanceof Error ? error.message : String(error));
        } finally {
          finishMutationSnapshotRequest(request);
          setIsLoading(false);
          if (resyncDirtyRef.current) resyncRunnerRef.current?.();
        }
      })();
    },
    [
      adoptSnapshotResponse,
      apiUrl,
      beginMutationSnapshotRequest,
      finishMutationSnapshotRequest,
      moveStreamBoundary,
      selectedCompanyId,
      selectedSpaceId,
      source,
      selectedProjectId,
    ],
  );

  const handleSelectCompany = useCallback(
    (companyId: string) => {
      if (companyId === selectedCompanyId) return;
      setSelectorRecoveryNotice(null);
      moveStreamBoundary(streamSelectionKey(selectedSpaceId, selectedProjectId, companyId));
      setSelectedCompanyId(companyId);
      setIsLoading(true);
      setSnapshot(emptySnapshot);
      // The selected Company is browser-tab scope. Changing it must never call
      // /v1/companies/switch, which mutates the CLI/server default for every tab.
      // The scoped load effect observes selectedCompanyId and performs the read.
    },
    [
      moveStreamBoundary,
      selectedCompanyId,
      selectedProjectId,
      selectedSpaceId,
    ],
  );

  const handleSelectSpace = useCallback(
    (spaceId: string) => {
      if (spaceId === selectedSpaceId) return;
      setSelectorRecoveryNotice(null);
      moveStreamBoundary(streamSelectionKey(spaceId, selectedProjectId, selectedCompanyId));
      const request = beginMutationSnapshotRequest();
      setSelectedSpaceId(spaceId);
      setIsLoading(true);
      setSnapshot(emptySnapshot);
      if (source !== liveSource) {
        finishMutationSnapshotRequest(request);
        setIsLoading(false);
        return;
      }
      void (async () => {
        try {
          const response = await switchSpaceApi(apiUrl, spaceId);
          if (response.snapshot) {
            adoptSnapshotResponse(request, response.snapshot);
          } else {
            adoptSnapshotResponse(
              request,
              await fetchSnapshot(apiUrl, selectedProjectId, selectedCompanyId, spaceId),
            );
          }
          setSourceError(null);
        } catch (error) {
          setSourceError(error instanceof Error ? error.message : String(error));
        } finally {
          finishMutationSnapshotRequest(request);
          setIsLoading(false);
          if (resyncDirtyRef.current) resyncRunnerRef.current?.();
        }
      })();
    },
    [
      adoptSnapshotResponse,
      apiUrl,
      beginMutationSnapshotRequest,
      finishMutationSnapshotRequest,
      moveStreamBoundary,
      selectedCompanyId,
      selectedProjectId,
      selectedSpaceId,
      source,
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
      const result = await fetchReadSnapshot(apiUrl, selectedProjectId, selectedCompanyId, selectedSpaceId);
      if (!result) return;
      if (adoptSnapshotResponse(result.request, result.snapshot)) {
        setSource(liveSource);
        setFreshnessState("live");
        setDomainFreshness(freshnessAfterSnapshot(streamConnectedRef.current));
        setSelectorRefreshGeneration((current) => current + 1);
      }
      try {
        setWorkflowDefs(await fetchWorkflowDefs(apiUrl));
      } catch {
        setWorkflowDefs([]);
      }
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      setSource("offline");
      setFreshnessState("reconnecting");
      setDomainFreshness(uniformFreshness("offline"));
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

  // The initial marker confirms both stream scopes and the serve-process epoch.
  // Every connect/reconnect then performs one authoritative read; the marker is
  // never mistaken for a full snapshot.
  const handleSseConnect = useCallback((streamKey: string, marker: SseSnapshotMarker): boolean => {
    if (selectedStreamRef.current !== streamKey) return false;
    const scopeMismatch = Boolean(
      (selectedSpaceId && marker.executionSpaceId && selectedSpaceId !== marker.executionSpaceId)
      || (selectedCompanyId && marker.companyScopeId && selectedCompanyId !== marker.companyScopeId),
    );
    streamScopeTrustedRef.current = !scopeMismatch;
    confirmedScopeRef.current = scopeMismatch ? {} : {
      executionSpaceId: marker.executionSpaceId ?? selectedSpaceId ?? selectedProjectId,
      companyScopeId: marker.companyScopeId ?? (selectedCompanyId || undefined),
    };
    invalidationTracker.current.reset(marker.streamEpoch);
    staleConnectionAttemptRef.current = null;
    disconnectedProbeAttemptRef.current = null;
    streamConnectedRef.current = !scopeMismatch;
    setFreshnessState(scopeMismatch ? "stale" : "reconnecting");
    setDomainFreshness(uniformFreshness(scopeMismatch ? "stale" : "reconnecting"));
    if (!scopeMismatch) requestAuthoritativeResync();
    return !scopeMismatch;
  }, [requestAuthoritativeResync, selectedCompanyId, selectedProjectId, selectedSpaceId]);

  // SSE delta: merge the frame into the in-memory snapshot (append/replace by
  // id, latest-wins) so the read model and Member action stream update WITHOUT
  // a full re-fetch.
  const handleSseFrame = useCallback((streamKey: string, frame: SseFrame) => {
    if (selectedStreamRef.current !== streamKey || !streamScopeTrustedRef.current) return;
    if (frame.kind === "projection_invalidated") {
      const decision = invalidationTracker.current.observe(
        frame.invalidation,
        confirmedScopeRef.current,
      );
      if (decision.kind === "refresh") {
        requestAuthoritativeResync(freshnessDomainsForInvalidation(frame.invalidation));
      }
      return;
    }
    staleConnectionAttemptRef.current = null;
    snapshotFrames.current.recordFrame(frame);
    setSnapshot((current) => applyFrame(current, frame));
    if (!resyncDirtyRef.current && !resyncInFlightRef.current) {
      setFreshnessState("live");
      setDomainFreshness((current) => updateFreshness(current, ["runtime"], "live"));
    }
  }, [requestAuthoritativeResync]);

  // Open the EventSource while live; it cleans up on unmount, on leaving live,
  // and on apiUrl or either scope changing.
  const eventStream = useEventStream({
    enabled: isLive,
    baseUrl: apiUrl,
    project: selectedProjectId,
    space: selectedSpaceId,
    company: selectedCompanyId,
    onConnect: handleSseConnect,
    onFrame: handleSseFrame,
  });

  // Volatile member previews exist only for the current live connection. A
  // reconnect or polling fallback must not make old thinking look replayable.
  useEffect(() => {
    if (eventStream.mode === "sse") {
      streamConnectedRef.current = streamScopeTrustedRef.current;
      if (!streamScopeTrustedRef.current) setFreshnessState("stale");
      return;
    }
    streamConnectedRef.current = false;
    setFreshnessState("reconnecting");
    setDomainFreshness(uniformFreshness("reconnecting"));
    snapshotFrames.current.clearLiveMemberActivity();
    setSnapshot((current) =>
      current.live_member_activity
        ? { ...current, live_member_activity: undefined }
        : current,
    );
    if (
      isLive
      && disconnectedProbeAttemptRef.current !== eventStream.connectionAttempt
    ) {
      disconnectedProbeAttemptRef.current = eventStream.connectionAttempt;
      requestAuthoritativeResync();
      // Transport state wins over the temporary Stale state used while the
      // authoritative read is in flight.
      setFreshnessState("reconnecting");
    }
  }, [eventStream.connectionAttempt, eventStream.mode, isLive, requestAuthoritativeResync]);

  // EventSource does not expose SSE keepalive comments. A quiet open stream gets
  // exactly one bounded authoritative probe per connection attempt. The probe
  // itself is visibly Stale, but a successful read restores Live; silence alone
  // is not treated as proof that a healthy stream is stale.
  useEffect(() => {
    if (!isLive || eventStream.mode !== "sse" || eventStream.lastActivityAt === null) return;
    const lastActivityAt = eventStream.lastActivityAt;
    const inspect = () => {
      if (
        Date.now() - lastActivityAt >= streamStaleAfterMs
        && staleConnectionAttemptRef.current !== eventStream.connectionAttempt
      ) {
        staleConnectionAttemptRef.current = eventStream.connectionAttempt;
        requestAuthoritativeResync();
      }
    };
    inspect();
    const timer = window.setInterval(inspect, 1_000);
    return () => window.clearInterval(timer);
  }, [
    eventStream.connectionAttempt,
    eventStream.lastActivityAt,
    eventStream.mode,
    isLive,
    requestAuthoritativeResync,
  ]);

  // Browser lifecycle recovery is edge-triggered: one read on visibility
  // regain or online, never a hidden permanent polling loop.
  useEffect(() => {
    const onVisibility = () => {
      if (document.visibilityState === "visible" && isLive) {
        requestAuthoritativeResync();
      }
    };
    const onOnline = () => {
      if (isLive) requestAuthoritativeResync();
    };
    const onOffline = () => {
      setFreshnessState("reconnecting");
      setDomainFreshness(uniformFreshness("reconnecting"));
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  }, [isLive, requestAuthoritativeResync]);

  useEffect(
    () => () => {
      snapshotFrames.current.clearLiveMemberActivity();
      if (resyncRetryTimerRef.current !== null) {
        window.clearTimeout(resyncRetryTimerRef.current);
      }
    },
    [],
  );

  // Interval poll of /v1/snapshot is manual opt-in only. Automatic recovery is
  // edge-triggered above and on stream reconnect, so an outage does not create
  // permanent high-frequency polling. A failed
  // poll surfaces the error but keeps the last good snapshot — it does not tear
  // the view down to the empty workspace the way a manual "Load live" failure
  // does. The interval is cleared on unmount, when it is no longer needed, and
  // whenever apiUrl changes so we never poll a stale endpoint.
  const shouldPoll = isLive && pollEnabled;
  useEffect(() => {
    if (!shouldPoll) return;
    let cancelled = false;
    const id = window.setInterval(() => {
      void (async () => {
        try {
          const result = await fetchReadSnapshot(apiUrl, selectedProjectId, selectedCompanyId, selectedSpaceId);
          if (!result) return;
          if (cancelled) {
            discardSnapshotRequest(result.request);
            return;
          }
          if (adoptSnapshotResponse(result.request, result.snapshot)) setSourceError(null);
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
    selectedCompanyId,
    selectedProjectId,
    selectedSpaceId,
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
      const capabilityToken = window.__AGENTFIRM_BOOTSTRAP__?.capabilityToken;
      const authenticatedOptions = capabilityToken
        ? { ...options, headers: { "X-AgentFirm-Token": capabilityToken, ...options?.headers } }
        : options;
      const response = await postAction(apiUrl, path, body, selectedProjectId, selectedCompanyId, authenticatedOptions, selectedSpaceId);
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
      if (resyncDirtyRef.current) resyncRunnerRef.current?.();
    }
    if (needsRefresh) await refreshLive();
    return true;
  }

  // Product freshness, not merely socket state. A connected EventSource is not
  // labelled Live while an invalidation/gap is awaiting authoritative state.
  const sourceLabel = !isLive
    ? offlineLabel
    : freshnessState === "live"
      ? "Live"
      : freshnessState === "stale"
        ? "Stale"
        : "Reconnecting";

  return (
    <TooltipProvider delayDuration={200}>
      <WorkbenchShell
        apiUrl={apiUrl}
        isLoading={isLoading}
        model={model}
        companies={companies}
        projects={projects}
        spaces={spaces}
        selectedCompanyId={selectedCompanyId}
        selectedProjectId={selectedProjectId}
        selectedSpaceId={selectedSpaceId}
        onSelectCompany={handleSelectCompany}
        onSelectProject={handleSelectProject}
        onSelectSpace={handleSelectSpace}
        onApiUrlChange={setApiUrl}
        onRefresh={refreshLive}
        onSelectionChange={setSelection}
        selection={selection}
        sourceError={sourceError ?? selectorRecoveryNotice}
        sourceLabel={sourceLabel}
        domainFreshness={domainFreshness}
        actionsEnabled={isLive}
        onAction={(path, body, options) => runAction(path, body, options)}
        pollEnabled={pollEnabled}
        canPoll={isLive}
        onTogglePoll={() => setPollEnabled((on) => !on)}
      />
    </TooltipProvider>
  );
}
