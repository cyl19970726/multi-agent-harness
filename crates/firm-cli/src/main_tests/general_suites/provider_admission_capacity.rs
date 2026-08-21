use super::*;

#[path = "../general/provider_version_drift_requires_adapter_review.rs"]
mod provider_version_drift_requires_adapter_review;
#[path = "../general/provider_admit_command_exact_replay_exits_zero_and_reuses_record.rs"]
mod provider_admit_command_exact_replay_exits_zero_and_reuses_record;
#[path = "../general/provider_admit_command_refuses_source_reviewed_current_tuple_without_writing.rs"]
mod provider_admit_command_refuses_source_reviewed_current_tuple_without_writing;
#[path = "../general/provider_admit_execution_space_omission_fails_before_probe_or_write.rs"]
mod provider_admit_execution_space_omission_fails_before_probe_or_write;
#[path = "../general/compatibility_block_provenance_is_exact_and_foreign_blocks_never_recover.rs"]
mod compatibility_block_provenance_is_exact_and_foreign_blocks_never_recover;
#[path = "../general/admitted_compatibility_block_recovers_into_start_machine_once.rs"]
mod admitted_compatibility_block_recovers_into_start_machine_once;
#[path = "../general/preflight_compatibility_block_overrides_capacity_proceed.rs"]
mod preflight_compatibility_block_overrides_capacity_proceed;
#[path = "../general/capacity_recovery_clears_only_capacity_origin_blocked_projection.rs"]
mod capacity_recovery_clears_only_capacity_origin_blocked_projection;
#[path = "../general/capacity_recovery_applies_close_latched_before_successful_cas_returns.rs"]
mod capacity_recovery_applies_close_latched_before_successful_cas_returns;
#[path = "../general/capacity_recovery_applies_close_after_member_cas_conflict.rs"]
mod capacity_recovery_applies_close_after_member_cas_conflict;
#[path = "../general/capacity_recovery_preserves_failed_member_after_cas_conflict.rs"]
mod capacity_recovery_preserves_failed_member_after_cas_conflict;
#[path = "../general/capacity_recovery_preserves_failed_member_after_successful_cas.rs"]
mod capacity_recovery_preserves_failed_member_after_successful_cas;
#[path = "../general/capacity_block_applies_close_latched_before_successful_cas_returns.rs"]
mod capacity_block_applies_close_latched_before_successful_cas_returns;
#[path = "../general/capacity_block_applies_close_after_member_cas_conflict.rs"]
mod capacity_block_applies_close_after_member_cas_conflict;
#[path = "../general/capacity_execution_mode_only_names_the_mode_it_probes.rs"]
mod capacity_execution_mode_only_names_the_mode_it_probes;
#[path = "../general/runtime_context_reports_proxy_routing_without_its_credentials.rs"]
mod runtime_context_reports_proxy_routing_without_its_credentials;
#[path = "../general/kimi_capacity_is_unknown_with_no_invented_windows.rs"]
mod kimi_capacity_is_unknown_with_no_invented_windows;
#[path = "../general/only_provider_structured_terminal_metadata_classifies_capacity.rs"]
mod only_provider_structured_terminal_metadata_classifies_capacity;
#[path = "../general/member_authored_text_can_never_produce_a_capacity_verdict.rs"]
mod member_authored_text_can_never_produce_a_capacity_verdict;
#[path = "../general/kimi_and_codex_failures_never_fabricate_capacity.rs"]
mod kimi_and_codex_failures_never_fabricate_capacity;
#[path = "../general/recorded_provider_errors_expire_with_the_capacity_ttl.rs"]
mod recorded_provider_errors_expire_with_the_capacity_ttl;
#[path = "../general/capacity_preflight_toggle_and_ttl_read_the_documented_env_contract.rs"]
mod capacity_preflight_toggle_and_ttl_read_the_documented_env_contract;
