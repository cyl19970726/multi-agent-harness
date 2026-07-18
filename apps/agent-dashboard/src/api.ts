import type {
  AgentEvent,
  DashboardSnapshot,
  DocRegistryEntry,
  HarnessTurnEvent,
  Message,
  Project,
  ProviderSession,
  TeamRunEvent,
  WorkflowDef,
  WorkflowRun,
  WorkflowStep,
} from "./types";

export interface ActionResponse {
  ok: boolean;
  result?: unknown;
  snapshot?: DashboardSnapshot;
  error?: string;
}

/** Trim a trailing slash so `${base}/v1/...` never double-slashes. */
export function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/$/, "");
}

/**
 * Append `?project=<id>` to a `/v1/...` path so a single serve can multiplex
 * many project stores (goal-multi-project P6). An absent/empty id yields the
 * bare path, which the backend resolves to the active/`_global` project — old
 * clients (and the picker before a project is chosen) keep working unchanged.
 * Project ids are restricted to `[A-Za-z0-9._-]`, so no percent-encoding is
 * needed to match the backend's `query_param` parser.
 */
function withProject(path: string, project?: string | null): string {
  const id = project?.trim();
  if (!id) return path;
  const sep = path.includes("?") ? "&" : "?";
  return `${path}${sep}project=${encodeURIComponent(id)}`;
}

export async function fetchSnapshot(
  baseUrl: string,
  project?: string | null,
): Promise<DashboardSnapshot> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}${withProject("/v1/snapshot", project)}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as DashboardSnapshot;
}

/**
 * Enumerate known projects via `GET /v1/projects` (registry + on-disk stores +
 * reserved `_global`). The response also names the currently-active project so
 * the picker can default to it. In raw `--store`/`HARNESS_ROOT` override mode the
 * backend reports only the served store as a synthetic default. Throws on
 * missing source / HTTP error.
 */
export async function fetchProjects(
  baseUrl: string,
): Promise<{ projects: Project[]; current: string }> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/projects`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  const data = (await response.json()) as { projects?: Project[]; current?: string };
  return { projects: data.projects ?? [], current: data.current ?? "" };
}

/**
 * Fetch the active project id via `GET /v1/projects/current`. Read live so a
 * `switch` (API or CLI) is reflected without a serve restart.
 */
export async function fetchCurrentProject(
  baseUrl: string,
): Promise<{ current: string; store_root?: string; project?: Project | null }> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/projects/current`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as {
    current: string;
    store_root?: string;
    project?: Project | null;
  };
}

/**
 * Flip the active project via `POST /v1/projects/switch {project}` so a live
 * serve AND CLI-spawned workers converge on the same central store (#89
 * invariant). The response carries the NEW active project's snapshot, returned
 * here so the caller can swap the read model without a second fetch.
 */
export async function switchProject(
  baseUrl: string,
  project: string,
): Promise<ActionResponse> {
  return postAction(baseUrl, "/v1/projects/switch", { project });
}

/**
 * Fetch a project doc body via `GET /v1/docs?path=docs/...` (ADR 0019). The
 * backend allow-lists the `docs/` tree. Used to render Vision `source_refs`.
 * Only works against a live source; the offline fixture has no docs server.
 */
export async function fetchDoc(
  baseUrl: string,
  path: string,
): Promise<{ path: string; content: string }> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/docs?path=${encodeURIComponent(path)}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as { path: string; content: string };
}

/**
 * Fetch the docs manifest (`docs/registry.json`) and return its `documents`
 * array. Reuses the allow-listed `/v1/docs` route — the registry lives under
 * `docs/`, so no extra endpoint is needed. The Docs surface builds its tree from
 * this. Throws on missing source / HTTP error / malformed JSON.
 */
export async function fetchDocRegistry(baseUrl: string): Promise<DocRegistryEntry[]> {
  const doc = await fetchDoc(baseUrl, "docs/registry.json");
  const parsed = JSON.parse(doc.content) as { documents?: DocRegistryEntry[] };
  return parsed.documents ?? [];
}

/**
 * Fetch the registered workflow catalog via `GET /v1/workflows` — the
 * run-independent `{ name, summary }` defs from the compiled registry. Only the
 * live source serves this; offline returns an empty list (caller shows an
 * "unavailable" empty state). Network/HTTP errors propagate to the caller.
 */
export async function fetchWorkflowDefs(baseUrl: string): Promise<WorkflowDef[]> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/workflows`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as WorkflowDef[];
}

/**
 * A single frame off the backend `/v1/events` SSE stream. The backend emits
 * provider-neutral objects (ADR 0011): an `AgentEvent`, `Message`, or
 * `ProviderSession` payload identical for Codex and Claude, plus a `snapshot`
 * frame on connect (timestamp only — clients resync via /v1/snapshot).
 */
export type SseFrame =
  | { kind: "snapshot"; generatedAt?: string }
  | { kind: "agent_event"; event: AgentEvent }
  | { kind: "message"; message: Message }
  | { kind: "provider_session"; session: ProviderSession }
  // A single raw provider turn event teed live during a delivery (Stage B): the
  // agent TUI consumes these for sub-second streaming, falling back to polling.
  | { kind: "provider_turn_event"; sessionId: string; event: Record<string, unknown> }
  // The NORMALIZED companion (Stage B): canonical HarnessTurnEvent[] expanded
  // from one raw event, so the provider-agnostic TUI streams live without
  // re-normalizing at the render layer. Merged by `seq` against the snapshot.
  | { kind: "provider_turn_event_normalized"; sessionId: string; events: HarnessTurnEvent[] }
  | { kind: "workflow_run"; run: WorkflowRun }
  | { kind: "workflow_step"; step: WorkflowStep }
  // A single team-run log entry (team-console): appended to team_run_events,
  // latest-wins by id so a replayed frame self-heals.
  | { kind: "team_run_event"; event: TeamRunEvent };

export interface EventStreamHandlers {
  /** Connection established (the initial `snapshot` frame arrived). */
  onSnapshot: (generatedAt?: string) => void;
  /** An incremental delta frame arrived. */
  onFrame: (frame: SseFrame) => void;
  /** The stream errored or closed; caller decides on fallback/retry. */
  onError: (error: Event) => void;
}

/**
 * Open an `EventSource` against `{baseUrl}/v1/events` and route each named SSE
 * frame to `handlers`. Returns a disposer that closes the underlying source.
 *
 * Parsing is defensive: a malformed `data:` payload is dropped (logged) rather
 * than tearing the stream down, so one bad line never blocks live updates.
 */
export function openEventStream(
  baseUrl: string,
  handlers: EventStreamHandlers,
  project?: string | null,
): () => void {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  // Scope the SSE channel to the selected project so a client subscribed to
  // project A never receives project B frames (P6 per-project broadcast).
  const source = new EventSource(`${normalized}${withProject("/v1/events", project)}`);

  const parse = <T,>(event: MessageEvent): T | null => {
    try {
      return JSON.parse(event.data) as T;
    } catch (error) {
      console.warn("[sse] dropping unparseable frame", error);
      return null;
    }
  };

  source.addEventListener("snapshot", (event) => {
    const data = parse<{ generated_at?: string }>(event as MessageEvent);
    handlers.onSnapshot(data?.generated_at);
  });
  source.addEventListener("agent_event", (event) => {
    const data = parse<AgentEvent>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "agent_event", event: data });
  });
  source.addEventListener("message", (event) => {
    const data = parse<Message>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "message", message: data });
  });
  source.addEventListener("provider_session", (event) => {
    const data = parse<ProviderSession>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "provider_session", session: data });
  });
  source.addEventListener("provider_turn_event", (event) => {
    const data = parse<{ session_id?: string; event?: Record<string, unknown> }>(event as MessageEvent);
    if (data?.session_id && data.event) {
      handlers.onFrame({ kind: "provider_turn_event", sessionId: data.session_id, event: data.event });
    }
  });
  source.addEventListener("provider_turn_event_normalized", (event) => {
    const data = parse<{ session_id?: string; events?: HarnessTurnEvent[] }>(event as MessageEvent);
    if (data?.session_id && Array.isArray(data.events)) {
      handlers.onFrame({
        kind: "provider_turn_event_normalized",
        sessionId: data.session_id,
        events: data.events,
      });
    }
  });
  source.addEventListener("workflow_run", (event) => {
    const data = parse<WorkflowRun>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "workflow_run", run: data });
  });
  source.addEventListener("workflow_step", (event) => {
    const data = parse<WorkflowStep>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "workflow_step", step: data });
  });
  source.addEventListener("team_run_event", (event) => {
    const data = parse<TeamRunEvent>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "team_run_event", event: data });
  });
  source.addEventListener("error", handlers.onError);

  return () => source.close();
}

/**
 * Merge a single SSE delta frame into the in-memory snapshot, latest-wins by
 * id: an incoming record replaces the matching row in place (preserving order)
 * or is appended when new. A delta also advances `generated_at` to now so the
 * freshness chip reads "just now" while the stream is actively pushing. Returns
 * the same reference unchanged for the timestamp-only `snapshot` frame so React
 * can skip a needless re-render.
 */
export function applyFrame(snapshot: DashboardSnapshot, frame: SseFrame): DashboardSnapshot {
  switch (frame.kind) {
    case "snapshot":
      return frame.generatedAt && frame.generatedAt !== snapshot.generated_at
        ? { ...snapshot, generated_at: frame.generatedAt }
        : snapshot;
    case "agent_event":
      return {
        ...snapshot,
        events: upsertById(snapshot.events, frame.event),
        generated_at: new Date().toISOString(),
      };
    case "message":
      return {
        ...snapshot,
        messages: upsertById(snapshot.messages, frame.message),
        generated_at: new Date().toISOString(),
      };
    case "provider_session":
      return {
        ...snapshot,
        provider_sessions: upsertById(snapshot.provider_sessions, frame.session),
        generated_at: new Date().toISOString(),
      };
    case "provider_turn_event": {
      // Append the raw event to this session's live buffer (transient; capped so
      // a long turn cannot grow memory unbounded). The agent TUI prefers this
      // sub-second stream over its 1s poll; the per-session NDJSON stays the
      // durable catch-up source.
      const LIVE_CAP = 2000;
      const current = snapshot.live_turn_events ?? {};
      const existing = current[frame.sessionId] ?? [];
      const next = existing.length >= LIVE_CAP ? existing : [...existing, frame.event];
      return {
        ...snapshot,
        live_turn_events: { ...current, [frame.sessionId]: next },
        generated_at: new Date().toISOString(),
      };
    }
    case "provider_turn_event_normalized": {
      // Merge this session's normalized events by `seq` (latest-wins), so a
      // duplicate replay or out-of-order frame self-heals and the buffer stays
      // sorted/aligned with the /normalized-events read endpoint. Capped like
      // the raw buffer so a long turn cannot grow memory unbounded.
      const LIVE_CAP = 2000;
      const current = snapshot.live_normalized_events ?? {};
      const existing = current[frame.sessionId] ?? [];
      const bySeq = new Map<number, HarnessTurnEvent>();
      for (const event of existing) bySeq.set(event.seq, event);
      for (const event of frame.events) bySeq.set(event.seq, event);
      const merged = Array.from(bySeq.values())
        .sort((a, b) => a.seq - b.seq)
        .slice(0, LIVE_CAP);
      return {
        ...snapshot,
        live_normalized_events: { ...current, [frame.sessionId]: merged },
        generated_at: new Date().toISOString(),
      };
    }
    case "workflow_run":
      return {
        ...snapshot,
        workflow_runs: upsertById(snapshot.workflow_runs, frame.run),
        generated_at: new Date().toISOString(),
      };
    case "workflow_step":
      return {
        ...snapshot,
        workflow_steps: upsertById(snapshot.workflow_steps, frame.step),
        generated_at: new Date().toISOString(),
      };
    case "team_run_event":
      return {
        ...snapshot,
        team_run_events: upsertById(snapshot.team_run_events, frame.event),
        generated_at: new Date().toISOString(),
      };
  }
}

/** Replace the row sharing `incoming.id` (latest-wins) or append it. */
function upsertById<T extends { id: string }>(list: T[] | undefined, incoming: T): T[] {
  const current = list ?? [];
  const index = current.findIndex((row) => row.id === incoming.id);
  if (index === -1) {
    return [...current, incoming];
  }
  const next = current.slice();
  next[index] = incoming;
  return next;
}

export async function postAction(baseUrl: string, path: string, body: unknown = {}): Promise<ActionResponse> {
  const normalized = baseUrl.trim().replace(/\/$/, "");
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const payload = (await response.json()) as ActionResponse;
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || `HTTP ${response.status}`);
  }
  return payload;
}
