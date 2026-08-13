export const PROVIDER_EVENT_SCHEMA_VERSION = "agentfirm.provider_observation.v1" as const;
export const PROVIDER_EVENT_ADAPTER_VERSION = "agentfirm.provider_event_adapter.v1" as const;

export const providerKinds = ["codex", "claude", "kimi", "pi"] as const;
export type ProviderKind = (typeof providerKinds)[number];

export const providerSemanticKinds = [
  "authored_response",
  "reasoning_summary",
  "tool_call_requested",
  "tool_call_started",
  "tool_call_completed",
  "tool_call_failed",
  "artifact_created",
  "usage_reported",
  "interaction_required",
  "interaction_resolved",
  "runtime_started",
  "runtime_ready",
  "runtime_stopped",
  "transport_interrupted",
  "turn_completed",
  "turn_failed",
  "turn_cancelled",
  "command_recovery_required",
  "malformed_or_incomplete",
] as const;
export type ProviderSemanticKind = (typeof providerSemanticKinds)[number];

export type ProviderObservationPayload =
  | { type: "authored_response"; text: string }
  | { type: "reasoning_summary"; summary: string }
  | { type: "tool"; tool_name: string; call_id?: string | null; display_detail?: string | null }
  | { type: "artifact"; display_name: string; media_type?: string | null; content_digest?: string | null }
  | { type: "usage"; input_tokens?: number | null; output_tokens?: number | null; total_tokens?: number | null }
  | { type: "interaction"; reason_code: string; prompt: string }
  | { type: "runtime"; state: string }
  | { type: "transport"; reason_code: string }
  | { type: "turn"; outcome: string; display_summary?: string | null }
  | { type: "recovery"; reason_code: string }
  | { type: "malformed"; reason_code: string };

export interface ProviderObservation {
  schema_version: typeof PROVIDER_EVENT_SCHEMA_VERSION;
  observation_id: string;
  provider: ProviderKind;
  adapter_version: typeof PROVIDER_EVENT_ADAPTER_VERSION;
  native_source_ref: string;
  agent_identity_id: string;
  agent_session_id: string;
  agent_session_generation: number;
  node_daemon_id: string;
  node_daemon_generation: number;
  provider_thread_id?: string | null;
  provider_turn_id?: string | null;
  provider_event_id?: string | null;
  ordering_position: number;
  causal_parent_id?: string | null;
  correlation_id?: string | null;
  runtime_command_id?: string | null;
  occurred_at?: string | null;
  observed_at: string;
  semantic_kind: ProviderSemanticKind;
  lifecycle_phase: "requested" | "started" | "progress" | "terminal" | "recovery";
  completeness: "partial" | "complete" | "incomplete" | "recovery_required";
  effect_certainty: "none" | "not_applied" | "applied" | "unknown";
  visibility: "session_owner_private" | "team_public" | "operator_only";
  validated_references: Array<{ kind: "work" | "message" | "delivery" | "evidence"; id: string }>;
  redacted: boolean;
  truncated: boolean;
  source_evidence_fingerprint: string;
  payload: ProviderObservationPayload;
}

export interface SessionEpisode {
  episode_id: string;
  provider_turn_id?: string | null;
  observations: ProviderObservation[];
  terminal: boolean;
  incomplete: boolean;
}

export interface SessionEventProjection {
  schema_version: typeof PROVIDER_EVENT_SCHEMA_VERSION;
  agent_session_id: string;
  agent_session_generation: number;
  cursor: string;
  episodes: SessionEpisode[];
  truncated: boolean;
  disabled_reason?: string | null;
}

export interface TeamRuntimeActivity {
  observation_id: string;
  agent_identity_id: string;
  semantic_kind:
    | "interaction_required"
    | "interaction_resolved"
    | "runtime_started"
    | "runtime_ready"
    | "runtime_stopped"
    | "transport_interrupted"
    | "command_recovery_required";
  lifecycle_phase: ProviderObservation["lifecycle_phase"];
  completeness: ProviderObservation["completeness"];
  effect_certainty: ProviderObservation["effect_certainty"];
  occurred_at?: string | null;
  payload: Extract<
    ProviderObservationPayload,
    { type: "interaction" | "runtime" | "transport" | "recovery" }
  >;
}
