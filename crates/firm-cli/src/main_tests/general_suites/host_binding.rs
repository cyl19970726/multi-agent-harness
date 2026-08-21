use super::*;

#[path = "../general/bind_host_via_cas_preserves_explicit_authority_path.rs"]
mod bind_host_via_cas_preserves_explicit_authority_path;
#[path = "../general/codex_rollout_metadata_validator_creates_interactive_lease.rs"]
mod codex_rollout_metadata_validator_creates_interactive_lease;
#[path = "../general/codex_thread_id_spoof_without_rollout_stays_unleased.rs"]
mod codex_thread_id_spoof_without_rollout_stays_unleased;
#[path = "../general/create_auto_binds_from_star_harness_env.rs"]
mod create_auto_binds_from_star_harness_env;
#[path = "../general/create_refuses_partial_star_harness_env.rs"]
mod create_refuses_partial_star_harness_env;
#[path = "../general/create_warns_when_host_thread_id_is_none.rs"]
mod create_warns_when_host_thread_id_is_none;
#[path = "../general/extracts_thread_id_from_thread_start_response_before_turn_start.rs"]
mod extracts_thread_id_from_thread_start_response_before_turn_start;
#[path = "../general/host_lease_renew_and_release_reject_stale_exact_fence.rs"]
mod host_lease_renew_and_release_reject_stale_exact_fence;
#[path = "../general/host_session_validator_valid_receipt_creates_exact_interactive_lease.rs"]
mod host_session_validator_valid_receipt_creates_exact_interactive_lease;
#[path = "../general/invalid_host_session_validation_preserves_observable_unleased_binding.rs"]
mod invalid_host_session_validation_preserves_observable_unleased_binding;
#[cfg(any())]
#[path = "../general/member_to_host_is_delivered_manual_ack_but_member_mail_stays_queued.rs"]
mod member_to_host_is_delivered_manual_ack_but_member_mail_stays_queued;
#[path = "../general/persistent_member_profiles_default_to_one_host_execution_driver.rs"]
mod persistent_member_profiles_default_to_one_host_execution_driver;
#[path = "../general/start_warns_and_auto_binds_when_unbound.rs"]
mod start_warns_and_auto_binds_when_unbound;
#[path = "../general/thread_idle_without_turn_id_is_terminal_source_for_active_stream.rs"]
mod thread_idle_without_turn_id_is_terminal_source_for_active_stream;
#[path = "../general/thread_idle_without_turn_id_reconciles_single_running_session.rs"]
mod thread_idle_without_turn_id_reconciles_single_running_session;
