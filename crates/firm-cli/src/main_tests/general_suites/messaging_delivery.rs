use super::*;

#[path = "../general/canonical_message_fabric_drives_lineage_status_and_member_detail_without_legacy_rows.rs"]
mod canonical_message_fabric_drives_lineage_status_and_member_detail_without_legacy_rows;
#[path = "../general/claude_member_delivery_dispatches_to_claude_stub.rs"]
mod claude_member_delivery_dispatches_to_claude_stub;
#[path = "../general/dashboard_snapshot_uses_latest_message_per_id.rs"]
mod dashboard_snapshot_uses_latest_message_per_id;
#[path = "../general/gateway_tick_delivers_queued_messages_with_same_delivery_path.rs"]
mod gateway_tick_delivers_queued_messages_with_same_delivery_path;
#[cfg(any())]
#[path = "../general/host_inbox_is_scoped_to_exact_native_thread_binding.rs"]
mod host_inbox_is_scoped_to_exact_native_thread_binding;
#[cfg(any())]
#[path = "../general/host_inbox_normalizes_surface_kimi_cli_to_kimi.rs"]
mod host_inbox_normalizes_surface_kimi_cli_to_kimi;
#[cfg(any())]
#[path = "../general/host_inbox_normalizes_surface_kimi_to_kimi_cli.rs"]
mod host_inbox_normalizes_surface_kimi_to_kimi_cli;
#[path = "../general/host_inbox_shows_work_attention_after_submit.rs"]
mod host_inbox_shows_work_attention_after_submit;
#[path = "../general/member_handoff_accepts_acp_message_chunk_shape.rs"]
mod member_handoff_accepts_acp_message_chunk_shape;
#[cfg(any())]
#[path = "../general/member_inbox_filters_delivery_states_and_malformed_recipient_rows.rs"]
mod member_inbox_filters_delivery_states_and_malformed_recipient_rows;
#[cfg(any())]
#[path = "../general/member_inbox_is_latest_wins_and_defaults_to_actionable_mail.rs"]
mod member_inbox_is_latest_wins_and_defaults_to_actionable_mail;
#[cfg(any())]
#[path = "../general/member_planning_is_ordinary_correlated_conversation.rs"]
mod member_planning_is_ordinary_correlated_conversation;
#[path = "../general/peer_message_resolver_binds_direct_membership_targets.rs"]
mod peer_message_resolver_binds_direct_membership_targets;
#[path = "../general/peer_message_resolver_fences_remote_topology_and_revisions.rs"]
mod peer_message_resolver_fences_remote_topology_and_revisions;
#[path = "../general/peer_message_resolver_reads_current_target_subscription_revision.rs"]
mod peer_message_resolver_reads_current_target_subscription_revision;
#[path = "../general/provider_answer_response_first_retry_recovers_without_duplicate_or_early_ack.rs"]
mod provider_answer_response_first_retry_recovers_without_duplicate_or_early_ack;
#[cfg(any())]
#[path = "../general/provider_interaction_message_bridge_recovers_by_reverse_request_replay.rs"]
mod provider_interaction_message_bridge_recovers_by_reverse_request_replay;
#[path = "../general/provider_request_replay_uses_canonical_message_only.rs"]
mod provider_request_replay_uses_canonical_message_only;
#[path = "../general/public_team_message_writes_use_only_canonical_authored_shapes.rs"]
mod public_team_message_writes_use_only_canonical_authored_shapes;
#[path = "../general/retry_delivery_requeues_safe_claim_without_provider_request.rs"]
mod retry_delivery_requeues_safe_claim_without_provider_request;
#[path = "../general/running_delivery_attempt_blocks_more_delivery.rs"]
mod running_delivery_attempt_blocks_more_delivery;
#[path = "../general/running_delivery_is_acknowledged_not_delivered.rs"]
mod running_delivery_is_acknowledged_not_delivered;
#[path = "../general/stale_failed_delivery_attempt_marks_message_failed_and_clears_member.rs"]
mod stale_failed_delivery_attempt_marks_message_failed_and_clears_member;
#[path = "../general/stale_unknown_delivery_attempt_blocks_more_delivery.rs"]
mod stale_unknown_delivery_attempt_blocks_more_delivery;
#[path = "../general/supervisor_binds_only_the_work_it_dispatches_in_one_pass.rs"]
mod supervisor_binds_only_the_work_it_dispatches_in_one_pass;
#[path = "../general/supervisor_claims_and_acknowledges_canonical_message_delivery_in_one_ledger.rs"]
mod supervisor_claims_and_acknowledges_canonical_message_delivery_in_one_ledger;
#[path = "../general/supervisor_claims_and_records_provider_receipt_for_canonical_work_delivery.rs"]
mod supervisor_claims_and_records_provider_receipt_for_canonical_work_delivery;
#[path = "../general/taskless_running_delivery_reconciliation_clears_member_without_fabricating_report.rs"]
mod taskless_running_delivery_reconciliation_clears_member_without_fabricating_report;
#[path = "../general/team_inbox_projection_lists_queued_then_all_with_claim_binding.rs"]
mod team_inbox_projection_lists_queued_then_all_with_claim_binding;
