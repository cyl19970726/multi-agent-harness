import type {
  AgentEvent,
  Company,
  DashboardSnapshot,
  DocRegistryEntry,
  ExecutionSpace,
  HarnessMeta,
  HostAttention,
  LiveMemberActivity,
  MemberAction,
  MemberRun,
  Message,
  Mission,
  NativeActivityProjection,
  PendingInteraction,
  Project,
  TeamMessage,
  TeamMemberCloseRequest,
  TeamSupervisorLease,
  AgentMessageRoute,
  TeamRun,
  TeamRunEvent,
  Wave,
  WorkflowDef,
  WorkflowRun,
  WorkflowStep,
} from "./types";

export interface ActionResponse {
  ok: boolean;
  result?: unknown;
  snapshot?: DashboardSnapshot;
  error?: string;
  detail?: string;
}

export interface ActionRequestOptions {
  headers?: Readonly<Record<string, string>>;
}

/** Trim a trailing slash so `${base}/v1/...` never double-slashes. */
export function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/$/, "");
}

/** True only when an SSE callback belongs to the selected coordination stream. */
export function matchesStreamProject(
  selectedProject: string | null | undefined,
  streamProject: string | null | undefined,
): boolean {
  return (selectedProject ?? "") === (streamProject ?? "");
}

/**
 * Add independent selectors to one API path. `space` chooses coordination
 * truth; `project` chooses provider cwd/config/Skill boundaries; `company`
 * chooses Company OS truth.
 */
function withQuery(
  path: string,
  params: Readonly<Record<string, string | null | undefined>>,
): string {
  const entries = Object.entries(params).filter(([, value]) => value?.trim());
  if (entries.length === 0) return path;
  const query = entries
    .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value?.trim() ?? "")}`)
    .join("&");
  const sep = path.includes("?") ? "&" : "?";
  return `${path}${sep}${query}`;
}

function withProjectAndCompany(
  path: string,
  project?: string | null,
  company?: string | null,
  space?: string | null,
): string {
  return withQuery(path, { space, project, company });
}

/**
 * Company OS requests carry both selectors when needed: Project remains the
 * execution/source boundary, Company is the truth-store boundary.
 */
function withCompanyOsRoute(
  path: string,
  project?: string | null,
  company?: string | null,
  space?: string | null,
): string {
  return path.startsWith("/v1/company-os/")
    ? withProjectAndCompany(path, project, company, space)
    : withQuery(path, { space, project });
}

export async function fetchSnapshot(
  baseUrl: string,
  project?: string | null,
  company?: string | null,
  space?: string | null,
): Promise<DashboardSnapshot> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}${withProjectAndCompany("/v1/snapshot", project, company, space)}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as DashboardSnapshot;
}

export async function fetchTeamRunSnapshot(
  baseUrl: string,
  teamRunId: string,
  project?: string | null,
  company?: string | null,
  space?: string | null,
): Promise<DashboardSnapshot> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) throw new Error("Harness API URL is required");
  const path = `/v1/team-runs/${encodeURIComponent(teamRunId)}/snapshot`;
  const response = await fetch(`${normalized}${withProjectAndCompany(path, project, company, space)}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return (await response.json()) as DashboardSnapshot;
}

/**
 * Fetch server build/data provenance via `GET /v1/meta` (issue #307). Used by
 * the persistent provenance footer to prove which build served this session
 * and to detect a stale frontend build (its own compiled-in rev disagreeing
 * with the server's) without reading server logs.
 */
export async function fetchMeta(
  baseUrl: string,
  project?: string | null,
  space?: string | null,
): Promise<HarnessMeta> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) throw new Error("Harness API URL is required");
  const response = await fetch(`${normalized}${withQuery("/v1/meta", { space, project })}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return (await response.json()) as HarnessMeta;
}

/** Read a display-only projection directly from the provider's native session.
 * The backend does not copy these items into Harness storage. */
export async function fetchNativeMemberActivity(
  baseUrl: string,
  memberRunId: string,
  project?: string | null,
  space?: string | null,
): Promise<NativeActivityProjection> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) throw new Error("Harness API URL is required");
  const response = await fetch(`${normalized}${withQuery(`/v1/member-runs/${encodeURIComponent(memberRunId)}/native-activity`, { space, project })}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return (await response.json()) as NativeActivityProjection;
}

/** Reconciled latest HostAttention rows for one TeamRun. */
export async function fetchHostAttentions(
  baseUrl: string,
  teamRunId: string,
  project?: string | null,
  space?: string | null,
): Promise<HostAttention[]> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) throw new Error("Harness API URL is required");
  const response = await fetch(`${normalized}${withQuery("/v1/host-attentions", { space, project, team_run_id: teamRunId })}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const payload = (await response.json()) as { attentions?: HostAttention[] };
  return payload.attentions ?? [];
}

export async function fetchNativeWorkflowStepActivity(
  baseUrl: string,
  workflowStepId: string,
  project?: string | null,
  space?: string | null,
): Promise<NativeActivityProjection> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) throw new Error("Harness API URL is required");
  const response = await fetch(`${normalized}${withQuery(`/v1/workflow-steps/${encodeURIComponent(workflowStepId)}/native-activity`, { space, project })}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return (await response.json()) as NativeActivityProjection;
}

/**
 * Enumerate Project Bindings. These entries define execution/source boundaries
 * and do not own Mission/Wave/Team/Workflow storage.
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

export async function fetchSpaces(
  baseUrl: string,
): Promise<{ spaces: ExecutionSpace[]; current: string }> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) throw new Error("Harness API URL is required");
  const response = await fetch(`${normalized}/v1/spaces`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const data = (await response.json()) as { spaces?: ExecutionSpace[]; current?: string };
  return { spaces: data.spaces ?? [], current: data.current ?? "" };
}

/**
 * Enumerate Company Stores. These are Company OS truth boundaries independent
 * from Project execution/source bindings.
 */
export async function fetchCompanies(
  baseUrl: string,
): Promise<{ companies: Company[]; current: string }> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/companies`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  const data = (await response.json()) as { companies?: Company[]; current?: string };
  return { companies: data.companies ?? [], current: data.current ?? "" };
}

export async function fetchCurrentCompany(
  baseUrl: string,
): Promise<{ current?: string | null; store_root?: string | null; company?: Company | null }> {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/companies/current`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as {
    current?: string | null;
    store_root?: string | null;
    company?: Company | null;
  };
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
 * Flip the active Project Binding. This changes the default provider workspace,
 * not the selected Execution Space or its coordination snapshot.
 */
export async function switchProject(
  baseUrl: string,
  project: string,
): Promise<ActionResponse> {
  return postAction(baseUrl, "/v1/projects/switch", { project });
}

export async function switchSpace(
  baseUrl: string,
  space: string,
): Promise<ActionResponse> {
  return postAction(baseUrl, "/v1/spaces/switch", { space });
}

export async function switchCompany(
  baseUrl: string,
  company: string,
): Promise<ActionResponse> {
  return postAction(baseUrl, "/v1/companies/switch", { company });
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
 * Harness-owned coordination deltas plus a timestamp-only `snapshot` frame.
 * Provider-native activity is read through NativeSessionRef endpoints.
 */
export type SseFrame =
  | { kind: "snapshot"; generatedAt?: string }
  | { kind: "projection_invalidated"; invalidation: ProjectionInvalidation | null }
  | { kind: "agent_event"; event: AgentEvent }
  | { kind: "message"; message: Message }
  | { kind: "workflow_run"; run: WorkflowRun }
  | { kind: "workflow_step"; step: WorkflowStep }
  // A single team-run log entry (team-console): appended to team_run_events,
  // latest-wins by id so a replayed frame self-heals.
  | { kind: "team_run_event"; event: TeamRunEvent }
  | { kind: "mission"; mission: Mission }
  | { kind: "wave"; wave: Wave }
  | { kind: "agent_team_run"; run: TeamRun }
  | { kind: "member_run"; member: MemberRun }
  | { kind: "team_message"; message: TeamMessage }
  | { kind: "team_supervisor_lease"; lease: TeamSupervisorLease }
  | { kind: "team_member_close_request"; request: TeamMemberCloseRequest }
  | { kind: "agent_message_route"; route: AgentMessageRoute }
  | { kind: "member_action"; action: MemberAction }
  | { kind: "pending_interaction"; interaction: PendingInteraction }
  | { kind: "member_activity"; activity: LiveMemberActivity };

export type ProjectionScope = "execution_space" | "company";
export type ProjectionInvalidationReason = "append" | "truncate" | "replace" | "delete";

/** A freshness signal only. It never carries row truth and must trigger a
 * scoped authoritative snapshot read rather than local state synthesis. */
export interface ProjectionInvalidation {
  scope: ProjectionScope;
  scope_id: string;
  ledger: string;
  revision: number;
  reason: ProjectionInvalidationReason;
  stream_epoch: string;
}

export interface SseSnapshotMarker {
  generatedAt?: string;
  executionSpaceId?: string;
  companyScopeId?: string;
  streamEpoch?: string;
}

export interface EventStreamHandlers {
  /** Connection established (the initial `snapshot` frame arrived). Returning
   * false rejects a scope-mismatched marker and asks the owner to reconnect. */
  onSnapshot: (marker: SseSnapshotMarker) => boolean | void;
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
  space?: string | null,
  company?: string | null,
): () => void {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  // Scope durable coordination by Execution Space. `project` remains present
  // for compatibility and provider-bound live actions, but does not select the
  // event ledger on a native-space server.
  const source = new EventSource(`${normalized}${withQuery("/v1/events", { space, project, company })}`);

  const parse = <T,>(event: MessageEvent): T | null => {
    try {
      return JSON.parse(event.data) as T;
    } catch (error) {
      console.warn("[sse] dropping unparseable frame", error);
      return null;
    }
  };

  source.addEventListener("snapshot", (event) => {
    const data = parse<{
      generated_at?: string;
      execution_space_id?: string;
      company_scope_id?: string;
      stream_epoch?: string;
    }>(event as MessageEvent);
    handlers.onSnapshot({
      generatedAt: data?.generated_at,
      executionSpaceId: data?.execution_space_id,
      companyScopeId: data?.company_scope_id,
      streamEpoch: data?.stream_epoch,
    });
  });
  source.addEventListener("projection_invalidated", (event) => {
    const data = parse<unknown>(event as MessageEvent);
    handlers.onFrame({
      kind: "projection_invalidated",
      invalidation: projectionInvalidation(data),
    });
  });
  source.addEventListener("agent_event", (event) => {
    const data = parse<AgentEvent>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "agent_event", event: data });
  });
  source.addEventListener("message", (event) => {
    const data = parse<Message>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "message", message: data });
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
  source.addEventListener("mission", (event) => {
    const data = parse<Mission>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "mission", mission: data });
  });
  source.addEventListener("wave", (event) => {
    const data = parse<Wave>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "wave", wave: data });
  });
  source.addEventListener("agent_team_run", (event) => {
    const data = parse<TeamRun>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "agent_team_run", run: data });
  });
  source.addEventListener("member_run", (event) => {
    const data = parse<MemberRun>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "member_run", member: data });
  });
  source.addEventListener("team_message", (event) => {
    const data = parse<TeamMessage>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "team_message", message: data });
  });
  source.addEventListener("team_supervisor_lease", (event) => {
    const data = parse<TeamSupervisorLease>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "team_supervisor_lease", lease: data });
  });
  source.addEventListener("team_member_close_request", (event) => {
    const data = parse<TeamMemberCloseRequest>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "team_member_close_request", request: data });
  });
  source.addEventListener("agent_message_route", (event) => {
    const data = parse<AgentMessageRoute>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "agent_message_route", route: data });
  });
  source.addEventListener("member_action", (event) => {
    const data = parse<MemberAction>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "member_action", action: data });
  });
  source.addEventListener("pending_interaction", (event) => {
    const data = parse<PendingInteraction>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "pending_interaction", interaction: data });
  });
  source.addEventListener("member_activity", (event) => {
    const data = parse<LiveMemberActivity>(event as MessageEvent);
    if (data) handlers.onFrame({ kind: "member_activity", activity: data });
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
    case "projection_invalidated":
      // The frame intentionally carries no row data. App owns the bounded
      // authoritative refresh and never treats this token as business truth.
      return snapshot;
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
    case "mission":
      return {
        ...snapshot,
        missions: upsertById(snapshot.missions, frame.mission),
        generated_at: new Date().toISOString(),
      };
    case "wave":
      return {
        ...snapshot,
        waves: upsertById(snapshot.waves, frame.wave),
        generated_at: new Date().toISOString(),
      };
    case "agent_team_run":
      return {
        ...snapshot,
        team_runs: upsertById(snapshot.team_runs, frame.run),
        generated_at: new Date().toISOString(),
      };
    case "member_run":
      return {
        ...snapshot,
        member_runs: upsertById(snapshot.member_runs, frame.member),
        generated_at: new Date().toISOString(),
      };
    case "team_message":
      return {
        ...snapshot,
        team_messages: upsertById(snapshot.team_messages, frame.message),
        generated_at: new Date().toISOString(),
      };
    case "team_supervisor_lease":
      return {
        ...snapshot,
        team_supervisor_leases: upsertByKey(
          snapshot.team_supervisor_leases,
          frame.lease,
          "team_run_id",
        ),
        generated_at: new Date().toISOString(),
      };
    case "team_member_close_request":
      return {
        ...snapshot,
        team_member_close_requests: upsertByKey(
          snapshot.team_member_close_requests,
          frame.request,
          "member_run_id",
        ),
        generated_at: new Date().toISOString(),
      };
    case "agent_message_route":
      return {
        ...snapshot,
        agent_message_routes: upsertById(snapshot.agent_message_routes, frame.route),
        generated_at: new Date().toISOString(),
      };
    case "member_action":
      return {
        ...snapshot,
        member_actions: upsertById(snapshot.member_actions, frame.action),
        generated_at: new Date().toISOString(),
      };
    case "pending_interaction":
      return {
        ...snapshot,
        pending_interactions: upsertById(snapshot.pending_interactions, frame.interaction),
        generated_at: new Date().toISOString(),
      };
    case "member_activity": {
      const current = snapshot.live_member_activity ?? {};
      const existing = current[frame.activity.member_run_id];
      if (existing && existing.revision >= frame.activity.revision) return snapshot;
      return {
        ...snapshot,
        live_member_activity: {
          ...current,
          [frame.activity.member_run_id]: frame.activity,
        },
        generated_at: new Date().toISOString(),
      };
    }
  }
}

function projectionInvalidation(value: unknown): ProjectionInvalidation | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const row = value as Record<string, unknown>;
  if (
    (row.scope !== "execution_space" && row.scope !== "company")
    || typeof row.scope_id !== "string"
    || !row.scope_id
    || typeof row.ledger !== "string"
    || !row.ledger
    || !Number.isSafeInteger(row.revision)
    || Number(row.revision) < 1
    || (row.reason !== "append" && row.reason !== "truncate" && row.reason !== "replace" && row.reason !== "delete")
    || typeof row.stream_epoch !== "string"
    || !row.stream_epoch
  ) return null;
  return row as unknown as ProjectionInvalidation;
}

export interface ConfirmedProjectionScope {
  executionSpaceId?: string;
  companyScopeId?: string;
}

export type InvalidationDecision =
  | { kind: "ignore"; reason: "other_scope" | "duplicate" }
  | { kind: "refresh"; gap: boolean; malformed: boolean };

/** Per-stream-epoch monotonic invalidation memory. Revisions are process-local
 * hints, so an epoch change clears every remembered ledger revision. */
export class ProjectionInvalidationTracker {
  private epoch: string | undefined;
  private revisions = new Map<string, number>();

  reset(streamEpoch?: string): void {
    this.epoch = streamEpoch;
    this.revisions.clear();
  }

  observe(
    invalidation: ProjectionInvalidation | null,
    scope: ConfirmedProjectionScope,
  ): InvalidationDecision {
    if (!invalidation) return { kind: "refresh", gap: false, malformed: true };
    if (invalidation.stream_epoch !== this.epoch) this.reset(invalidation.stream_epoch);
    const expectedScopeId = invalidation.scope === "execution_space"
      ? scope.executionSpaceId
      : scope.companyScopeId;
    if (!expectedScopeId || invalidation.scope_id !== expectedScopeId) {
      return { kind: "ignore", reason: "other_scope" };
    }
    const key = `${invalidation.scope}\u0000${invalidation.scope_id}\u0000${invalidation.ledger}`;
    const previous = this.revisions.get(key);
    if (previous !== undefined && invalidation.revision <= previous) {
      return { kind: "ignore", reason: "duplicate" };
    }
    this.revisions.set(key, invalidation.revision);
    return {
      kind: "refresh",
      gap: previous !== undefined && invalidation.revision > previous + 1,
      malformed: false,
    };
  }
}

/** Stable identity for one requested EventSource. Both Company and Execution
 * Space participate so late callbacks cannot cross either selection boundary. */
export function streamSelectionKey(
  space?: string | null,
  project?: string | null,
  company?: string | null,
): string {
  return JSON.stringify([space ?? "", project ?? "", company ?? ""]);
}

/**
 * A request token for a full snapshot read. The token remembers the exact SSE
 * position at which the request began, so the response can be made current by
 * replaying every delta received while that read was in flight.
 */
export interface SnapshotRequestToken {
  id: number;
  frameSequence: number;
  kind: "read" | "mutation";
  generation: number;
}

/**
 * Coordinates full snapshot responses with the live SSE stream.
 *
 * A snapshot is a point-in-time read, but HTTP and SSE race in normal use: an
 * SSE delta may arrive after the HTTP request begins and before its response is
 * rendered. Replacing the client state with that response would lose the delta
 * until some unrelated future refresh. This small client-only journal fixes the
 * race by recording live frames and replaying the frames newer than each
 * request's starting position onto its response.
 *
 * Reads are latest-wins among reads. Mutations take causal priority over reads:
 * a read cannot begin while an action POST is in flight, and all pre-mutation
 * reads are invalidated. `reset` is used at a project boundary to invalidate
 * requests, frames, and transient activity together.
 */
export class SnapshotFrameBuffer {
  private static readonly MAX_BUFFERED_FRAMES = 4_096;
  private nextRequestId = 0;
  private nextFrameSequence = 0;
  private frames: Array<{ sequence: number; frame: SseFrame }> = [];
  private pendingRequests = new Map<number, SnapshotRequestToken>();
  private latestReadId = 0;
  private latestMutationId = 0;
  private mutationGeneration = 0;
  private activeMutations = new Set<number>();
  // This lives only in the browser process. It is never written to or accepted
  // from a server snapshot, but lets an unexpired preview survive a crossing
  // with a snapshot that correctly omits thinking.
  private liveMemberActivity = new Map<string, LiveMemberActivity>();

  /** Begin a background/full read, unless an action mutation is in flight. */
  beginReadRequest(): SnapshotRequestToken | null {
    if (this.activeMutations.size > 0) return null;
    // Only the newest overlapping read may commit, so an older read no longer
    // needs to hold a journal claim while its HTTP request winds down.
    if (this.latestReadId) this.dropPending(this.latestReadId);
    const request = this.begin("read");
    this.latestReadId = request.id;
    return request;
  }

  /** Begin an action whose response may carry a snapshot. */
  beginMutationRequest(): SnapshotRequestToken {
    // A mutation invalidates every read that began before its POST. Its response
    // (or a fresh read after completion) is the first state allowed to commit.
    if (this.latestReadId) this.dropPending(this.latestReadId);
    this.latestReadId = 0;
    const request = this.begin("mutation");
    this.latestMutationId = request.id;
    this.activeMutations.add(request.id);
    this.mutationGeneration += 1;
    return { ...request, generation: this.mutationGeneration };
  }

  finishMutation(request: SnapshotRequestToken): void {
    if (request.kind !== "mutation") return;
    this.activeMutations.delete(request.id);
    this.dropPending(request.id);
  }

  /** Release a failed/cancelled HTTP request so an idle stream retains no log. */
  discardRequest(request: SnapshotRequestToken): void {
    if (request.kind === "read" && request.id === this.latestReadId) {
      this.latestReadId = 0;
    }
    this.dropPending(request.id);
  }

  /** Record a delta; durable frames are journaled only while a request is pending. */
  recordFrame(frame: SseFrame): void {
    if (frame.kind === "member_activity") {
      const current = this.liveMemberActivity.get(frame.activity.member_run_id);
      if (!current || current.revision < frame.activity.revision) {
        this.liveMemberActivity.set(frame.activity.member_run_id, frame.activity);
      }
    }
    if (this.pendingRequests.size === 0) return;
    this.frames.push({ sequence: ++this.nextFrameSequence, frame });
    if (this.frames.length > SnapshotFrameBuffer.MAX_BUFFERED_FRAMES) {
      this.frames = this.frames.slice(-SnapshotFrameBuffer.MAX_BUFFERED_FRAMES);
    }
  }

  /** Keep the client-only preview registry aligned with expiry/disconnect UI. */
  replaceLiveMemberActivity(activity: Record<string, LiveMemberActivity> | undefined): void {
    this.liveMemberActivity = new Map(Object.entries(activity ?? {}));
  }

  clearLiveMemberActivity(): void {
    this.liveMemberActivity.clear();
  }

  /**
   * Return a response merged with in-flight deltas, or `null` when it is no
   * longer causally current. A successful/ignored resolution releases its
   * journal claim, so idle streams retain no durable frame history.
   */
  resolveResponse(
    request: SnapshotRequestToken,
    snapshot: DashboardSnapshot,
  ): DashboardSnapshot | null {
    const isCurrentRead =
      request.kind === "read" &&
      this.activeMutations.size === 0 &&
      request.id === this.latestReadId &&
      request.generation === this.mutationGeneration;
    const isCurrentMutation =
      request.kind === "mutation" && request.id === this.latestMutationId;
    if (!isCurrentRead && !isCurrentMutation) {
      this.dropPending(request.id);
      return null;
    }

    // The server snapshot must never establish/replay thinking. Strip any
    // accidental field, then overlay only active in-memory ephemeral state.
    const { live_member_activity: _serverActivity, ...withoutServerActivity } = snapshot;
    let merged: DashboardSnapshot = withoutServerActivity;
    for (const entry of this.frames) {
      if (entry.sequence > request.frameSequence) {
        merged = applyFrame(merged, entry.frame);
      }
    }
    if (this.liveMemberActivity.size > 0) {
      merged = {
        ...merged,
        live_member_activity: Object.fromEntries(this.liveMemberActivity),
      };
    }
    this.dropPending(request.id);
    return merged;
  }

  reset(): void {
    this.pendingRequests.clear();
    this.activeMutations.clear();
    this.latestReadId = 0;
    this.latestMutationId = 0;
    this.mutationGeneration += 1;
    this.frames = [];
    this.liveMemberActivity.clear();
  }

  private begin(kind: SnapshotRequestToken["kind"]): SnapshotRequestToken {
    const request: SnapshotRequestToken = {
      id: ++this.nextRequestId,
      frameSequence: this.nextFrameSequence,
      kind,
      generation: this.mutationGeneration,
    };
    this.pendingRequests.set(request.id, request);
    this.pruneFrames();
    return request;
  }

  private dropPending(id: number): void {
    this.pendingRequests.delete(id);
    this.pruneFrames();
  }

  /** Retain only frames still needed by at least one unresolved response. */
  private pruneFrames(): void {
    if (this.pendingRequests.size === 0) {
      this.frames = [];
      return;
    }
    let earliestSequence = Number.POSITIVE_INFINITY;
    for (const request of this.pendingRequests.values()) {
      earliestSequence = Math.min(earliestSequence, request.frameSequence);
    }
    this.frames = this.frames.filter((entry) => entry.sequence > earliestSequence);
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

function upsertByKey<T, K extends keyof T>(
  list: T[] | undefined,
  incoming: T,
  key: K,
): T[] {
  const current = list ?? [];
  const index = current.findIndex((item) => item[key] === incoming[key]);
  if (index < 0) return [...current, incoming];
  const next = current.slice();
  next[index] = incoming;
  return next;
}

export async function postAction(
  baseUrl: string,
  path: string,
  body: unknown = {},
  project?: string | null,
  company?: string | null,
  options: ActionRequestOptions = {},
  space?: string | null,
): Promise<ActionResponse> {
  const normalized = baseUrl.trim().replace(/\/$/, "");
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}${withCompanyOsRoute(path, project, company, space)}`, {
    method: "POST",
    headers: { ...options.headers, "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const payload = (await response.json()) as ActionResponse;
  if (!response.ok || !payload.ok) {
    throw new Error(payload.detail || payload.error || `HTTP ${response.status}`);
  }
  return payload;
}
