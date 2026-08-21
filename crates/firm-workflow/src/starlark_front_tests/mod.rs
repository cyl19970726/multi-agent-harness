use super::*;
use firm_core::WorkflowRunStatus;
use std::sync::Mutex;

/// The mandatory meta header every test program must declare. Prepended to the
/// per-test body so the run is not rejected for a missing `design_intent`.
const HEADER: &str =
    "workflow(\"demo\", \"scan then fix: serialize so the fix builds on the scan output\")\n";

/// A mock driver that always succeeds and records invocation order + prompts.
fn recording_driver<'a>(
    seen: &'a Mutex<Vec<(String, String)>>,
) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + 'a {
    move |spec: &AgentStepSpec| {
        seen.lock()
            .unwrap()
            .push((spec.label.clone(), spec.prompt.clone()));
        StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            output_summary: format!("ok: {}", spec.prompt),
            step_id: None,
            started_at: None,
            details: None,
            structured: None,
            ordinal: None,
        }
    }
}

// D3c: auto_apply_on_verdict / persist_changes="patch" on a NON-writable leaf
// are program errors (a read-only leaf produces no authorized diff), rejected
// across all three surfaces (agent / parallel / pipeline).

// D3c: an unknown persist_changes / write_mode value is rejected instead of
// silently falling back to defaults (which hid typos like "patchh").

// D3c: the valid combinations still parse (positive control) — a writable leaf
// with persist_changes="discard" or "patch", and a read-only leaf with an
// explicit persist_changes="discard" (harmless: nothing is persisted anyway).

// #192: schema_strict is accepted on agent()/parallel()/pipeline() when a
// schema is present, and rejected when it is not (strict mode is a no-op
// without a schema, which would silently hide a program error).

/// A driver that emulates the schema-mode contract: when the spec carries a
/// schema it returns a `structured` object (each required key -> "v:<key>");
/// otherwise it returns text only. Records the schema each step saw so a test
/// can assert the schema threaded through onto the spec.
fn structured_driver<'a>(
    schemas: &'a Mutex<Vec<Option<serde_json::Value>>>,
) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + 'a {
    move |spec: &AgentStepSpec| {
        schemas.lock().unwrap().push(spec.schema.clone());
        let structured = spec.schema.as_ref().map(|schema| {
            let obj: serde_json::Map<String, serde_json::Value> = schema
                .as_object()
                .map(|m| m.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
                .into_iter()
                .map(|k| (k.clone(), serde_json::Value::String(format!("v:{k}"))))
                .collect();
            serde_json::Value::Object(obj)
        });
        StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            output_summary: format!("text: {}", spec.prompt),
            step_id: None,
            started_at: None,
            details: None,
            structured,
            ordinal: None,
        }
    }
}

/// A driver that "spends" a fixed USD per call (via `details.cost_usd`) and
/// counts dispatches, for budget-ceiling tests.
fn spending_driver(
    calls: &std::sync::atomic::AtomicUsize,
    cost: f64,
) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + '_ {
    move |spec: &AgentStepSpec| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            output_summary: "ok".into(),
            step_id: None,
            started_at: None,
            details: Some(serde_json::json!({ "cost_usd": cost })),
            structured: None,
            ordinal: None,
        }
    }
}

/// A driver that records each spec's (label, writable) so writable-flow tests
/// can assert the kwarg reached the plain spec.
fn writable_recording_driver(
    seen: &Mutex<Vec<(String, bool)>>,
) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + '_ {
    move |spec: &AgentStepSpec| {
        seen.lock()
            .unwrap()
            .push((spec.label.clone(), spec.writable));
        StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            output_summary: "ok".into(),
            step_id: None,
            started_at: None,
            details: None,
            structured: None,
            ordinal: None,
        }
    }
}

// ----- Resume / replay tests -----

/// A driver that counts dispatches (for asserting cached leaves are NOT
/// re-dispatched) and echoes the prompt into output_summary.
fn counting_driver(
    calls: &std::sync::atomic::AtomicUsize,
) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + '_ {
    move |spec: &AgentStepSpec| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            output_summary: format!("ok: {}", spec.prompt),
            step_id: None,
            started_at: None,
            details: None,
            structured: None,
            ordinal: None,
        }
    }
}

mod a_failed_step_makes_the_run_failed;
mod a_syntax_error_is_a_parse_error;
mod agent_return_status_allows_failure_reason_retry;
mod agent_with_schema_returns_a_dict_the_script_reads;
mod agent_without_schema_returns_the_text_summary;
mod agent_writable_kwarg_flows_onto_the_spec_default_false;
mod args_are_injected_as_a_global;
mod blank_or_short_design_intent_is_rejected;
mod budget_ceiling_short_circuits_further_steps;
mod captured_meta_is_returned_to_the_caller;
mod direct_write_mode_flows_through_serial_agent_only;
mod direct_write_mode_is_rejected_in_parallel_and_pipeline_specs;
mod final_output_persists_logs_verdict_and_criterion;
mod json_encode_decode_is_available_to_scripts;
mod missing_workflow_header_is_rejected;
mod output_accepts_a_bare_string_and_last_call_wins;
mod output_surfaces_declared_result_in_final_output;
mod parallel_data_driven_comprehension_runs_every_spec;
mod parallel_isolation_kwarg_flows_onto_the_spec;
mod parallel_return_status_surfaces_failed_slot_without_breaking_default_slot;
mod parallel_returns_structured_dicts_and_summary_strings_per_spec;
mod parallel_writable_spec_field_flows;
mod passthrough_kwargs_include_service_tier_for_agent_parallel_and_pipeline_specs;
mod patch_and_artifact_authoring_intents_round_trip;
mod patch_artifact_kwargs_flow_through_parallel_and_pipeline_specs;
mod persistence_on_non_writable_leaf_is_rejected;
mod pipeline_accepts_bare_positional_stages_not_just_a_list;
mod pipeline_flows_every_item_through_all_stages_in_order;
mod pipeline_forward_injects_structured_output_into_next_stage;
mod pipeline_return_status_uses_last_stage_shape;
mod resume_partition_in_parallel;
mod resume_replayed_leaf_does_not_advance_spend;
mod resume_reuses_cached_leaves_and_skips_driver;
mod resume_with_empty_map_dispatches_all;
mod schema_strict_accepted_with_schema_across_primitives;
mod schema_strict_non_bool_value_is_rejected;
mod schema_strict_without_schema_is_rejected;
mod timeout_s_rejects_non_positive_values;
mod two_serial_agents_produce_two_completed_steps;
mod unknown_persist_changes_and_write_mode_values_are_rejected;
mod valid_persistence_combinations_are_accepted;
mod value_to_json_round_trips_with_json_to_value;
mod verdict_accepts_a_positional_reason;
mod verdict_false_makes_status_failed_even_when_steps_ran;
mod verdict_true_keeps_completed_and_surfaces_header_criterion;
mod workflow_header_budget_lowers_the_ceiling;
