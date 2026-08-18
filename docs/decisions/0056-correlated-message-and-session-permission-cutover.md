# ADR 0056: Correlated Message And Session Permission Cutover

- Status: accepted; breaking clean cutover
- Date: 2026-08-14
- Supersedes: ADR 0030 interaction-object and permission-routing contract
- Product authority: the current execution-foundation authority, Global Work
  (DOC-106); the former AgentFirm CompanyOS DOC-87 line is retired history
- Implementation issue: [#462](https://github.com/cyl19970726/multi-agent-harness/issues/462)

## Context

The provider bridge had two competing coordination models. Ordinary questions
could be represented as correlated Messages, while a second durable interaction
ledger, schema, API, SSE stream, and Dashboard projection described the same
pause. Tool permission callbacks were also promoted into that second lifecycle
even though every AgentSession already has an effective permission ceiling.

## Decision

1. A provider question is one correlated request `Message`; its answer is one
   correlated reply `Message` bound to the exact request, AgentSession, member,
   runtime generation, and provider option or text.
2. There is no separate provider-interaction object, store, schema, endpoint,
   SSE frame, Dashboard list, or compatibility reader.
3. One machine-scoped NodeDaemon owns local AgentSessions and provider effects.
   Team membership and subscriptions are routing overlays; they do not own the
   provider thread.
4. The AgentSession freezes its effective permission ceiling before provider
   start. In-ceiling operations proceed through the provider's native sandbox.
   Out-of-ceiling or unexpected approval callbacks fail closed and cannot widen
   the session or create a second permission workflow. Codex consumes its exact
   frozen sandbox and `approvalPolicy=never` at thread start. Kimi ACP has no
   provable narrow native sandbox, so only a frozen full-access Session may
   start and only exact `allow_once` / `allow_always` intents may be selected;
   labels and option-id substrings never grant permission.
5. Protected Company effects remain governed by their domain approval policy;
   that policy is separate from provider tool permission.
6. Historical JSONL files are not migrated, read by current status, lineage,
   detail, inbox, or replay projections, dual-written, or mutated after this
   cutover. `team_messages.jsonl` remains available only through the explicit
   read-only Legacy archive/export path.
7. HTTP and MCP expose no standalone `MemberRun` creation operation. A current
   `MemberRun` is admitted only by combined TeamRun creation or add-member,
   where the TeamRun membership, runtime projection, and canonical MemberRun
   are validated before publication through one Store authority boundary.

## Consequences

- CLI/MCP use `answer-message` / `team_run_answer_message`; HTTP uses the
  message-scoped `/messages/{id}/answer` route. MCP does not advertise or
  dispatch the retired `team_run_send_message`, `team_message_acknowledge`, or
  `team_run_reconcile_delivery` tombstones: calls fail closed as unknown tools.
- MCP status and inbox views project only canonical `Message` plus
  `CanonicalMessageDelivery`. Status exposes current request/reply and
  unresolved-response counts, never the historical manual-ACK projection.
- Provider-native session records remain transcript/tool/turn truth.
- `CanonicalMessageDelivery` and provider receipts remain transport evidence.
- Answer authority comes from the transport-authenticated AgentMember and must
  match the Team's active Host membership and current AgentSession. Request
  bodies cannot select `resolved_by`. The correlated response is written before
  the Host delivery ACK, making the crash window recoverable by exact retry.
- ADR 0030 and matching references in older ADRs are historical evidence only.
- The change is intentionally breaking because compatibility would preserve
  the ambiguity this decision removes.

## Acceptance

- active source and current docs contain no operational reference to the
  retired object, ledger, route, or projection;
- question/reply idempotency and authority tests pass on exact Messages;
- MCP `tools/list` excludes the retired acknowledgement/reconciliation tools,
  their direct invocation is byte-zero and unknown, and canonical status makes
  the request visible before reply and resolved after exact retry;
- the retired HTTP and MCP standalone MemberRun-create inputs are absent from
  advertised inventories, fail closed, and produce zero TeamRun, legacy
  runtime-projection, and canonical-operation deltas;
- governance rejects every production `team_messages.jsonl` mutator and every
  current lineage/status/detail/inbox/replay reader while retaining exactly one
  explicit read-only Legacy export inventory entry;
- Codex launches with its mapped sandbox and `approvalPolicy=never`;
- unexpected Codex approval callbacks fail closed with no question Message;
- schema, Rust, Dashboard, governance, and plugin checks pass.

### Retired `team_run_start.rs` coverage map

The deleted integration file exercised the pre-cutover interaction ledger and
therefore cannot remain executable. Its still-valid runtime claims are covered
by the following active tests; this table is the one-for-one retirement audit,
not a prose waiver:

| Retired test | Active replacement evidence |
| --- | --- |
| `team_run_start_leaves_kimi_members_idle_until_host_close` | `team_run_start_delegates_to_node_daemon_and_is_idempotent`; `idle_kimi_member_consumes_late_mail_on_the_same_native_session`; `host_close_terminates_kimi_0310_runtime_without_conflating_interrupt` |
| `kimi_can_send_work_linked_progress_after_first_acp_acceptance` | `post_team_run_message_and_start_async`; `busy_kimi_member_batches_mail_in_order_and_withholds_stale_handoff` |
| `kimi_concatenated_acp_report_remains_provider_native` | `kimi_null_error_key_on_a_successful_response_is_not_a_provider_error`; native-history assertions in `team_run_api` remain provider-store backed |
| `kimi_member_explicitly_resumes_provider_native_session` | `reviewed_recovery_redelivers_same_stable_member_without_duplicate_work_or_session`; `crashed_kimi_transport_requires_recovery_without_replaying_provider_effect` |
| `claude_member_uses_native_session_without_provider_activity_mirror` | `agent_sdk_member_binds_one_native_session_and_turn_completion_is_idle` |
| `claude_failure_keeps_native_session_and_provider_error_without_mirroring_stream` | `agent_sdk_member_records_provider_errors_instead_of_successful_rounds`; `a_silent_provider_turn_is_a_provider_error_and_stays_reconstructable` |
| `team_run_start_completes_mixed_codex_kimi_without_persisting_reasoning` | provider-neutral start/lease coverage in `team_run_daemon`; Codex and Kimi lifecycle tests in `team_run_api`; transient-reasoning assertions remain in each adapter test rather than one mixed fixture |
| `kimi_question_waits_for_lead_resolution_and_resumes_same_turn` | provider-neutral response/ACK contract in `provider_answer_response_first_retry_recovers_without_duplicate_or_early_ack`; live reverse-question journey in `codex_app_server_question_routes_to_lead_and_resumes_same_turn`; MCP transport identity, spoof rejection, exact-option validation, request/reply status visibility, actionable-plus-history inbox visibility, retry idempotency, and zero Legacy writes in `mcp_answers_canonical_provider_request_with_transport_identity_and_exact_retry`; Kimi waiting cancellation tests cover its ACP callback boundary |
| `kimi_full_access_tool_permissions_acknowledge_without_pending_interactions` | `kimi_full_access_safe_approvals_converge_to_one_bounded_receipt`; `kimi_safe_approval_rejects_closed_retired_generation_and_session_drift` |
| `kimi_reject_only_tool_permission_fails_closed_to_policy` | `kimi_permission_matching_uses_exact_intent_not_option_id_substrings` |
| `kimi_unknown_permission_request_fails_closed_to_human` | `kimi_permission_matching_uses_exact_intent_not_option_id_substrings`; `scripted_unknown_reverse_method_publishes_no_receipt` |
| `blocked_provider_outcome_leaves_member_idle_and_supervisor_can_reattach` | `kimi_provider_error_after_receipt_requires_recovery_without_replay`; `kimi_incomplete_stop_reason_requires_recovery_without_replay`; `team_run_start_delegates_to_node_daemon_and_is_idempotent` |

The submission gate runs `team_run_api`, `team_run_daemon`, `mcp_stdio`,
`claude_agent_sdk_member`, and the adapter unit/integration suites. The three
Claude replacements named above use the deterministic fake runner, require no
provider credentials, and run in the default test graph (no `ignore` or
always-false `cfg`). The MCP replacement obtains its request from a real fake
Codex provider turn through the NodeDaemon; it does not seed a request through
a retired writer. Reintroducing a disabled copy of the old file is not a
replacement.
