import type { AgentEvent, DashboardSnapshot, Message, ProviderSession } from "./types";

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

export async function fetchSnapshot(baseUrl: string): Promise<DashboardSnapshot> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/snapshot`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as DashboardSnapshot;
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
  | { kind: "provider_turn_event"; sessionId: string; event: Record<string, unknown> };

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
export function openEventStream(baseUrl: string, handlers: EventStreamHandlers): () => void {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const source = new EventSource(`${normalized}/v1/events`);

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
