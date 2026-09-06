# S7b existing daemon test mapping

Base: `dc34775bc6c5b9b85ca98efaf0ceb18a56d46f37`. All 62 original names present exactly once: 56 real CLI integration scenarios, six pure daemon recovery tests. Shutdown tests retain Unix cfg. No scenario ignored or removed.

| Test | Original owner | Current owner |
|---|---|---|
| a_latched_close_outranks_the_drain_retry_note | crates/firm-cli/src/supervisor_daemon/drain_blocked_member_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_blocked_member_tests.rs |
| a_no_progress_row_never_shadows_an_unsettled_recovery_requirement | crates/firm-cli/src/supervisor_daemon/recovery.rs | crates/firm-node-daemon/src/supervisor_daemon/recovery.rs |
| a_run_that_gained_a_work_operation_classifies_as_progressed_and_never_holds | crates/firm-cli/src/supervisor_daemon/drive_outcome_tests.rs | crates/firm-cli/src/daemon_integration_tests/drive_outcome_tests.rs |
| a_runner_that_meets_the_drain_fence_leaves_the_member_startable | crates/firm-cli/src/supervisor_daemon/drain_blocked_member_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_blocked_member_tests.rs |
| a_settling_run_is_not_adopted_while_its_dead_generation_writes_its_outcome | crates/firm-cli/src/supervisor_daemon/drive_outcome_tests.rs | crates/firm-cli/src/daemon_integration_tests/drive_outcome_tests.rs |
| a_start_that_only_moved_a_clock_stamp_classifies_as_no_progress_and_holds | crates/firm-cli/src/supervisor_daemon/drive_outcome_tests.rs | crates/firm-cli/src/daemon_integration_tests/drive_outcome_tests.rs |
| a_team_run_that_left_running_is_settled_not_held | crates/firm-cli/src/supervisor_daemon/drive_outcome_tests.rs | crates/firm-cli/src/daemon_integration_tests/drive_outcome_tests.rs |
| a_transient_start_failure_never_holds_adoption | crates/firm-cli/src/supervisor_daemon/adoption_tests.rs | crates/firm-cli/src/daemon_integration_tests/adoption_tests.rs |
| a_volatile_hold_keyed_to_canonical_state_is_lifted_by_canonical_change | crates/firm-cli/src/supervisor_daemon/drive_outcome_tests.rs | crates/firm-cli/src/daemon_integration_tests/drive_outcome_tests.rs |
| at_capacity_adoption_is_attempted_once_per_scan_tick_not_once_per_pass | crates/firm-cli/src/supervisor_daemon/adoption_tests.rs | crates/firm-cli/src/daemon_integration_tests/adoption_tests.rs |
| authority_bundle_rolls_back_partial_acquisition_until_every_predecessor_is_released | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| authority_renewal_failure_is_returned_by_the_team_run_events_reader | crates/firm-cli/src/supervisor_daemon/self_stop_events.rs | crates/firm-cli/src/daemon_integration_tests/self_stop_events_tests.rs |
| authority_shutdown_interrupts_contended_renewal_without_waiting_for_ttl | crates/firm-cli/src/supervisor_daemon/lease_renewal_tests.rs | crates/firm-cli/src/daemon_integration_tests/lease_renewal_tests.rs |
| close_member_for_recovery_accepts_a_reconciled_recovery_required_lane | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| close_member_for_recovery_leaves_the_lane_untouched_when_its_authority_is_fenced | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| completed_run_supervisor_loss_does_not_latch_machine_authority | crates/firm-cli/src/supervisor_daemon/lease_renewal_tests.rs | crates/firm-cli/src/daemon_integration_tests/lease_renewal_tests.rs |
| contended_space_does_not_delay_healthy_space_to_expiry | crates/firm-cli/src/supervisor_daemon/lease_renewal_tests.rs | crates/firm-cli/src/daemon_integration_tests/lease_renewal_tests.rs |
| control_response_is_one_complete_json_frame_under_backpressure | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| daemon_control_generation_fences_stale_and_successor_instances | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| default_leases_survive_ten_second_writer_contention_without_extending_ttl | crates/firm-cli/src/supervisor_daemon/lease_renewal_tests.rs | crates/firm-cli/src/daemon_integration_tests/lease_renewal_tests.rs |
| dormant_continuation_is_refused_unless_the_close_tolerates_it | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| drained_in_flight_work_is_redelivered_under_the_successor_generation | crates/firm-cli/src/supervisor_daemon/drain_inflight_work_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_inflight_work_tests.rs |
| drained_member_returns_to_a_startable_lane_without_any_host_verb | crates/firm-cli/src/supervisor_daemon/drain_blocked_member_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_blocked_member_tests.rs |
| drained_mid_turn_member_can_be_closed_by_the_host | crates/firm-cli/src/supervisor_daemon/drain_recovery_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_recovery_tests.rs |
| drained_mid_turn_member_resumes_under_the_next_supervisor_generation | crates/firm-cli/src/supervisor_daemon/drain_recovery_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_recovery_tests.rs |
| every_start_path_store_conflict_is_typed_and_records_no_hold | crates/firm-cli/src/supervisor_daemon/drive_outcome_tests.rs | crates/firm-cli/src/daemon_integration_tests/drive_outcome_tests.rs |
| expired_owned_space_still_latches_global_authority_loss | crates/firm-cli/src/supervisor_daemon/lease_renewal_tests.rs | crates/firm-cli/src/daemon_integration_tests/lease_renewal_tests.rs |
| machine_local_live_sink_rejects_invalid_and_stale_registration_then_replaces_successor | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| no_progress_hold_is_keyed_to_the_canonical_state_it_observed | crates/firm-cli/src/supervisor_daemon/recovery.rs | crates/firm-node-daemon/src/supervisor_daemon/recovery.rs |
| node_authority_heartbeat_is_independent_of_a_long_discovery_scan | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| node_daemon_socket_path_is_stable_per_node | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| node_daemon_socket_path_keeps_distinct_homes_and_nodes_isolated | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| node_daemon_socket_path_long_home_fallback | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| node_daemon_socket_path_short_home | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| node_daemon_socket_path_uses_one_identity_for_alias_equivalent_long_homes | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| old_daemon_or_supervisor_runtime_command_cannot_authorize_recovery_block | crates/firm-cli/src/supervisor_daemon/recovery.rs | crates/firm-node-daemon/src/supervisor_daemon/recovery.rs |
| only_transient_start_failures_skip_a_durable_adoption_hold | crates/firm-cli/src/supervisor_daemon/recovery.rs | crates/firm-node-daemon/src/supervisor_daemon/recovery.rs |
| only_unsettled_supervisor_recovery_markers_block_adoption | crates/firm-cli/src/supervisor_daemon/recovery.rs | crates/firm-node-daemon/src/supervisor_daemon/recovery.rs |
| readoption_hops_a_reconciled_recovery_required_lane_to_idle | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| recover_and_close_share_one_terminal_turn_boundary_predicate | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| recover_names_the_blocker_of_a_blocked_member_whose_lane_is_still_live | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| recover_names_the_clause_that_keeps_a_recovery_required_lane_shut | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| recover_reports_a_typed_block_and_still_repairs_the_drain_blocked_member | crates/firm-cli/src/supervisor_daemon/drain_blocked_member_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_blocked_member_tests.rs |
| recover_returns_a_blocked_member_on_a_dead_lane_to_a_startable_status | crates/firm-cli/src/supervisor_daemon/drain_blocked_member_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_blocked_member_tests.rs |
| recover_returns_a_recovery_required_lane_to_idle_before_restarting_the_member | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| recovery_required_lane_reaches_active_only_through_the_proved_idle_hop | crates/firm-cli/src/supervisor_daemon/recover_blocked_lane_blocker_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_blocked_lane_blocker_tests.rs |
| rejected_live_scope_does_not_discard_the_registered_serve_endpoint | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| shutdown_force_reaps_an_owned_group_before_returning | crates/firm-cli/src/supervisor_daemon/shutdown.rs | crates/firm-cli/src/daemon_integration_tests/shutdown_tests.rs |
| shutdown_renews_node_authority_until_accepted_worker_finishes | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| shutdown_returns_drain_incomplete_when_provider_thread_does_not_converge | crates/firm-cli/src/supervisor_daemon/shutdown.rs | crates/firm-cli/src/daemon_integration_tests/shutdown_tests.rs |
| start_failure_classification_is_typed_not_substring_matched | crates/firm-cli/src/supervisor_daemon/recovery.rs | crates/firm-node-daemon/src/supervisor_daemon/recovery.rs |
| started_work_lost_to_a_drain_is_recovered_by_the_host_and_redelivered | crates/firm-cli/src/supervisor_daemon/recover_lost_execution_tests.rs | crates/firm-cli/src/daemon_integration_tests/recover_lost_execution_tests.rs |
| status_remains_responsive_while_a_control_mutation_is_blocked | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| status_remains_responsive_while_execution_space_scan_is_blocked | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |
| status_remains_responsive_while_reap_joins_a_finished_supervisor | crates/firm-cli/src/supervisor_daemon/adoption_tests.rs | crates/firm-cli/src/daemon_integration_tests/adoption_tests.rs |
| stop_answers_only_after_the_managed_runtime_drains | crates/firm-cli/src/supervisor_daemon/stop_drain_tests.rs | crates/firm-cli/src/daemon_integration_tests/stop_drain_tests.rs |
| stop_reports_drain_incomplete_without_releasing_authority | crates/firm-cli/src/supervisor_daemon/stop_drain_tests.rs | crates/firm-cli/src/daemon_integration_tests/stop_drain_tests.rs |
| structurally_dead_start_failure_is_not_retried_on_every_scan | crates/firm-cli/src/supervisor_daemon/adoption_tests.rs | crates/firm-cli/src/daemon_integration_tests/adoption_tests.rs |
| supervisor_transient_failure_stops_at_confirmed_expiry | crates/firm-cli/src/supervisor_daemon/lease_renewal_tests.rs | crates/firm-cli/src/daemon_integration_tests/lease_renewal_tests.rs |
| the_adoption_hold_fingerprint_sees_the_execution_lane | crates/firm-cli/src/supervisor_daemon/drain_blocked_member_tests.rs | crates/firm-cli/src/daemon_integration_tests/drain_blocked_member_tests.rs |
| unchanged_team_run_is_adopted_at_most_once_per_canonical_state | crates/firm-cli/src/supervisor_daemon/adoption_tests.rs | crates/firm-cli/src/daemon_integration_tests/adoption_tests.rs |
| unreadable_held_space_latches_only_after_confirmed_deadline | crates/firm-cli/src/supervisor_daemon/tests.rs | crates/firm-cli/src/daemon_integration_tests/tests.rs |

## Finite default-off fixture adapter

The constructors consume owned inputs previously placed directly into `MultiTeamDaemon` and `MultiTeamContext`. Private registries retain their original empty initial state. Stop flags and context thread ownership remain shared/owned exactly as before. All rows below call or observe the same production object; no provider or application stub is introduced.

| Adapter operation | Existing retained fixture use |
|---|---|
| `new` | shutdown_tests.rs, drain_recovery_tests.rs, drain_blocked_member_tests.rs, adoption_tests.rs, drain_inflight_work_tests.rs, tests.rs, lease_renewal_tests.rs, drive_outcome_tests.rs, self_stop_events_tests.rs, stop_drain_tests.rs |
| `successor` | tests.rs |
| `set_node_identity` | adoption_tests.rs, lease_renewal_tests.rs |
| `node_id` | drain_recovery_tests.rs, drain_blocked_member_tests.rs, lease_renewal_tests.rs, self_stop_events_tests.rs |
| `daemon_id` | drain_recovery_tests.rs, drain_blocked_member_tests.rs, tests.rs, lease_renewal_tests.rs, self_stop_events_tests.rs |
| `instance_id` | lease_renewal_tests.rs, self_stop_events_tests.rs |
| `firm_home` | lease_renewal_tests.rs, stop_drain_tests.rs |
| `set_scan_interval` | adoption_tests.rs, lease_renewal_tests.rs |
| `set_lease_ttl_override` | lease_renewal_tests.rs |
| `authority_lost` | tests.rs, lease_renewal_tests.rs |
| `control_worker_failed` | tests.rs |
| `stop_requested_flag` | tests.rs, lease_renewal_tests.rs |
| `authority_shutdown_flag` | lease_renewal_tests.rs |
| `push_context` | adoption_tests.rs, lease_renewal_tests.rs, self_stop_events_tests.rs |
| `clear_contexts` | adoption_tests.rs |
| `context_count` | lease_renewal_tests.rs |
| `context_thread_finished` | lease_renewal_tests.rs |
| `wake_endpoint_count` | tests.rs |
| `wake_endpoint` | tests.rs |
| `capacity_wait_count` | adoption_tests.rs |
| `capacity_wait` | adoption_tests.rs |
| `insert_volatile_hold` | drive_outcome_tests.rs |
| `insert_settling_run` | drive_outcome_tests.rs |
| `remove_settling_run` | drive_outcome_tests.rs |
| `scan_and_adopt` | adoption_tests.rs |
| `reap_finished` | adoption_tests.rs, lease_renewal_tests.rs |
| `settle_finished_supervisor` | drive_outcome_tests.rs |
| `serve_loop` | tests.rs, stop_drain_tests.rs |
| `handle_control_command` | adoption_tests.rs |
| `ensure_node_authority_bundle` | tests.rs |
| `refresh_held_node_authorities` | tests.rs, lease_renewal_tests.rs, self_stop_events_tests.rs |
| `remember_node_lease` | tests.rs |
| `ensure_node_authority` | lease_renewal_tests.rs, self_stop_events_tests.rs |
| `registered_spaces` | lease_renewal_tests.rs, self_stop_events_tests.rs |
| `release_node_authorities` | drain_recovery_tests.rs |
| `settle_node_authorities_for_shutdown` | drain_recovery_tests.rs |
| `install_native_session_wake_endpoint` | tests.rs |
| `block_start_failure_if_unresolved` | adoption_tests.rs, drive_outcome_tests.rs |
| `clear_team_run_supervisor_recovery` | adoption_tests.rs |
| `hold_adoption_without_progress` | adoption_tests.rs |
| `team_run_adoption_is_held` | adoption_tests.rs |
| `adoption_defers_for_capacity` | adoption_tests.rs |
| `next_node_authority_refresh_delay` | lease_renewal_tests.rs |
| `graceful_shutdown_with_deadline` | shutdown_tests.rs |
| `supersede_node_authority_for_test` | self_stop_events_tests.rs |
| `write_control_response` | tests.rs |
| `graceful_shutdown_with_deadlines` | shutdown_tests.rs |
| `journal_machine_authority_loss_phase` | self_stop_events_tests.rs |
| `adoption_start_attempts` | adoption_tests.rs |
| `node_authority_refresh_interval` | tests.rs |
| `daemon_control_generation_authorized` | tests.rs |
