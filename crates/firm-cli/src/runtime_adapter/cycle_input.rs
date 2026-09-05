//! The wake-to-cycle-input projection: one idle wake becomes the exact
//! `CycleInput` the driven loop consumes. Pure move out of
//! `runtime_adapter.rs` ahead of the S4 size gate (DEV-156 S3 review 01).

use super::*;

/// One cycle's input, projected from an idle wake.
pub(super) struct CycleInput {
    pub(super) prompt: String,
    pub(super) active_work: Option<ClaimedWork>,
    pub(super) accepted_messages: Vec<TeamMessageProjection>,
    pub(super) host_attentions: Vec<HostAttention>,
    /// New `last_consumed_work_version`; None leaves the tracker unchanged.
    pub(super) consumed_work_version: Option<u64>,
}

/// The two previously per-loop, twice-per-loop wake match blocks collapsed
/// into one shared projection.
fn idle_wake_into_cycle<A: TeamRuntimeAdapter<Error = CliError>>(
    wake: IdleMemberWake,
    ledger: &TeamRunLedger,
    objective: &str,
    context: &MemberRuntimeContext,
    member_row: &mut ProviderRuntimeProjection,
    adapter: &mut A,
) -> CliResult<Result<CycleInput, MemberOutcome>> {
    match wake {
        IdleMemberWake::Work(claimed) => {
            let envelope = member_work_collaboration_envelope(
                ledger,
                context.execution_space_id.as_deref(),
                context.project_id.as_deref(),
                context.project_selector.as_deref(),
                member_row,
                Some(&claimed.work),
            )?;
            let consumed = claimed.work.version;
            let prompt = work_contract_prompt(objective, member_row, &claimed.work, &envelope);
            Ok(Ok(CycleInput {
                prompt,
                active_work: Some(*claimed),
                accepted_messages: Vec::new(),
                host_attentions: Vec::new(),
                consumed_work_version: Some(consumed),
            }))
        }
        IdleMemberWake::ActiveWorkContinuation(work) => {
            let envelope = member_work_collaboration_envelope(
                ledger,
                context.execution_space_id.as_deref(),
                context.project_id.as_deref(),
                context.project_selector.as_deref(),
                member_row,
                Some(&work),
            )?;
            let consumed = work.version;
            let prompt = active_work_continuation_prompt(objective, member_row, &work, &envelope);
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: Vec::new(),
                host_attentions: Vec::new(),
                consumed_work_version: Some(consumed),
            }))
        }
        IdleMemberWake::Messages {
            messages,
            host_attentions,
        } => {
            let mut prompt = team_messages_prompt(
                "TEAM MESSAGES arrived. They are conversation, not Work ownership. \
                 Address the question or coordination request, and use the Works \
                 board for any durable responsibility.",
                &messages,
            );
            if !host_attentions.is_empty() {
                prompt.push_str(
                    "\n\nBATCHED TEAM STATUS (coordination facts, not Work ownership):\n",
                );
                for attention in &host_attentions {
                    prompt.push_str(&format!(
                        "- {:?}: work={} version={} source={}\n",
                        attention.kind,
                        attention.work_id,
                        attention.work_version,
                        attention.source_event_ref
                    ));
                }
            }
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: messages,
                host_attentions,
                consumed_work_version: None,
            }))
        }
        IdleMemberWake::HostAttentions(attentions) => {
            let mut prompt = String::from(
                "TEAM STATUS ATTENTION arrived for the Host. These are durable coordination facts, not new Work ownership. Review, respond, or route only when a decision is required.\n\n",
            );
            for attention in &attentions {
                prompt.push_str(&format!(
                    "- {:?}: work={} version={} source={} member_run={}\n",
                    attention.kind,
                    attention.work_id,
                    attention.work_version,
                    attention.source_event_ref,
                    attention.member_run_id.as_deref().unwrap_or("none")
                ));
            }
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: Vec::new(),
                host_attentions: attentions,
                consumed_work_version: None,
            }))
        }
        IdleMemberWake::CloseRequested { close, reply } => {
            let result = close_idle_runtime(ledger, member_row, adapter, &close);
            match result {
                Ok((outcome, close_receipt)) => {
                    if let Some(reply) = reply {
                        let _ = reply.send(Ok(serde_json::json!({
                            "member_run_id": member_row.id,
                            "status": "closed",
                            "provider_ack": "member_runtime_close_applied",
                            "provider_terminal_evidence": {
                                "provider_terminal_event": "idle_before_close",
                                "member_runtime_close": close_receipt,
                            },
                        })));
                    }
                    Ok(Err(outcome))
                }
                Err(error) => {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(CliError::Usage(error.to_string())));
                    }
                    Err(error)
                }
            }
        }
        IdleMemberWake::TestRetired => Ok(Err(MemberOutcome::new(
            member_row,
            MemberRunStatus::Idle,
            format!(
                "{} member test runtime retired while idle",
                adapter.display_name()
            ),
        ))),
        IdleMemberWake::Degraded(reason) => Ok(Err(MemberOutcome::new(
            member_row,
            MemberRunStatus::Blocked,
            format!("{} member degraded: {reason}", adapter.display_name()),
        ))),
    }
}

/// Wait for the next wake and project it into a cycle input (or a terminal
/// outcome). Shared by the first wait and the loop-tail wait.
#[allow(clippy::too_many_arguments)]
pub(super) fn await_next_cycle<A: TeamRuntimeAdapter<Error = CliError>>(
    ledger: &TeamRunLedger,
    objective: &str,
    context: &MemberRuntimeContext,
    member_row: &mut ProviderRuntimeProjection,
    adapter: &mut A,
    live_control: &ControlReceiver<MemberControlCommand>,
    zero_output_streak: u32,
    last_consumed_work_version: Option<u64>,
    wake_policy: &WakePolicy,
    wake_backoff: &mut WakeBackoff,
) -> CliResult<Result<CycleInput, MemberOutcome>> {
    let wake = {
        let agent_member_id = member_row.agent_member_id.clone();
        wait_for_idle_member_wake(
            ledger,
            member_row,
            live_control,
            || {
                require_provider_session_authority(ledger, &agent_member_id, false)?;
                adapter.ensure_alive()
            },
            zero_output_streak,
            last_consumed_work_version,
            wake_policy,
            wake_backoff,
        )?
    };
    idle_wake_into_cycle(wake, ledger, objective, context, member_row, adapter)
}
