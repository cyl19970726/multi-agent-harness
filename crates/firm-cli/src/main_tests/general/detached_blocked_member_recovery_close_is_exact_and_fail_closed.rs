use super::*;

struct ScopedChannelRelease(Option<std::sync::mpsc::SyncSender<()>>);

impl ScopedChannelRelease {
    fn new(sender: std::sync::mpsc::SyncSender<()>) -> Self {
        Self(Some(sender))
    }

    fn release(&mut self, reason: &str) {
        if let Some(sender) = self.0.take() {
            sender.send(()).unwrap_or_else(|_| panic!("{reason}"));
        }
    }
}

impl Drop for ScopedChannelRelease {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[test]
fn detached_blocked_member_recovery_close_is_exact_and_fail_closed() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    use harness_core::CurrentWorkDraft;

    let (store, root) = temp_store("detached-blocked-member-recovery-close");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:recovery-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");

    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-detached-recovery",
            std::process::id(),
            "test://detached-recovery",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("members")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members,
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let make_received_work = |id: &str, at: &str| {
        let mut draft = CurrentWorkDraft::new(
            id.into(),
            created.team_run.id.clone(),
            created.team_run.agent_team_id.clone(),
            format!("stale recovery Work {id}"),
            "Exercise explicit Host reconciliation before detached recovery".into(),
            "Host cancellation preserves provider receipt evidence".into(),
            WorkClaimMode::HostAssign,
            WorkPriority::Normal,
            compatibility_team_actor("host", "test"),
            at.into(),
        );
        draft.eligible_member_ids = vec![bound.agent_member_id.clone()];
        let created_work = store
            .insert_work(
                draft.into_work(),
                WorkCommandContext {
                    event_id: format!("{id}-created"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-create"),
                    created_at: at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("create recovery Work");
        let membership = store
            .fabric_team_memberships("unit-test-space")
            .expect("read TeamMemberships")
            .into_iter()
            .find(|membership| {
                membership.team_id == created.team_run.agent_team_id
                    && membership.agent_member_id == bound.agent_member_id
            })
            .expect("exact responsible TeamMembership");
        let work = store
            .assign_work_to_membership(
                &created_work.id,
                created_work.version,
                &membership.id,
                "unit-test-space",
                WorkCommandContext {
                    event_id: format!("{id}-assigned"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-assign"),
                    created_at: at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("assign stable TeamMembership responsibility");
        let claimed = claim_canonical_work_for_member(&ledger, &bound)
            .expect("claim recovery Work")
            .expect("one recovery Work claim");
        assert_eq!(claimed.work.id, work.id);
        ledger
            .complete_work_delivery(&claimed, &format!("receipt-{id}"))
            .expect("record provider receipt");
        work
    };
    let stale_work_a = make_received_work("recovery-stale-a", "unix-ms:recovery-work-a");
    let stale_work_b = make_received_work("recovery-stale-b", "unix-ms:recovery-work-b");
    transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
        .expect("idle session");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("attach provider runtime");
    let mut blocked = bound.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:recovery-blocked".into());
    ledger
        .save_member_run(&bound, &blocked)
        .expect("block member after provider failure");

    assert!(
        close_detached_blocked_member_for_recovery(
            &store,
            &run.id,
            &blocked,
            &lease,
            "host",
            "must not bypass a live handle",
        )
        .expect("attached runtime check")
        .is_none(),
        "an attached runtime must stay on the normal provider Close path"
    );

    settle_provider_attempt_release(&ledger, &blocked).expect("detach provider runtime");
    let admission = prepare_provider_process_effect(&ledger, &blocked, 2)
        .expect("prepare one intentionally ambiguous resume command");
    let (ambiguous_space_id, ambiguous_session) =
        provider_session_for_member(&ledger, &blocked).expect("ambiguous recovery Session");
    let mut ambiguous_latch = pending_close_request(
        &run.id,
        &blocked.id,
        "host",
        "Store must serialize ambiguous effect before recovery Close",
    );
    ambiguous_latch.id = "close-detached-recovery-ambiguous-writer-first".into();
    ambiguous_latch.detached_recovery_fence =
        Some(Box::new(harness_core::DetachedRecoveryCloseFence {
            execution_space_id: ambiguous_space_id,
            member_run_generation: blocked.runtime_generation,
            agent_session_id: ambiguous_session.id.clone(),
            agent_session_generation: ambiguous_session.runtime_generation,
            agent_session_version: ambiguous_session.version,
            agent_session_driver_generation: ambiguous_session.control_state.driver_generation,
            native_session_id: ambiguous_session
                .native_session_ref
                .as_ref()
                .expect("ambiguous native Session")
                .native_session_id
                .clone(),
            node_daemon_id: ambiguous_session.node_daemon_id.clone(),
            node_daemon_generation: ambiguous_session.node_daemon_generation,
            authorizing_supervisor_id: lease.supervisor_id.clone(),
            authorizing_supervisor_generation: lease.generation,
        }));
    let ambiguous_latch_error = store
        .latch_team_member_close_for_supervisor(
            &ambiguous_latch,
            &lease.supervisor_id,
            lease.generation,
        )
        .expect_err("writer-first ambiguous effect must reject recovery Close in Store");
    assert!(ambiguous_latch_error
        .to_string()
        .contains("DETACHED_RECOVERY_CLOSE_AMBIGUOUS_COMMAND"));
    assert!(store
        .team_member_close_requests()
        .expect("Close rows after ambiguous writer-first rejection")
        .is_empty());
    let ambiguous = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &blocked,
        &lease,
        "host",
        "ambiguous effects must fail closed",
    )
    .expect_err("ambiguous RuntimeCommand must fence recovery Close");
    assert!(ambiguous.to_string().contains("ambiguous RuntimeCommand"));
    settle_provider_effect_not_applied(
        &ledger,
        &admission,
        "deterministic negative receipt".to_string(),
    )
    .expect("settle ambiguous command as not applied");

    transition_provider_session_for_member(&ledger, &blocked, AgentSessionStatus::Active)
        .expect("seed active provider turn");
    let active_turn = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &blocked,
        &lease,
        "host",
        "active turn must fail closed",
    )
    .expect_err("an active provider turn must fence recovery Close");
    assert!(active_turn
        .to_string()
        .contains("not detached+idle at a terminal turn boundary"));
    transition_provider_session_for_member(&ledger, &blocked, AgentSessionStatus::Idle)
        .expect("return to terminal turn boundary");

    let (execution_space_id, detached_session) =
        provider_session_for_member(&ledger, &blocked).expect("detached recovery Session");
    let exact_fence = harness_core::DetachedRecoveryCloseFence {
        execution_space_id: execution_space_id.clone(),
        member_run_generation: blocked.runtime_generation,
        agent_session_id: detached_session.id.clone(),
        agent_session_generation: detached_session.runtime_generation,
        agent_session_version: detached_session.version,
        agent_session_driver_generation: detached_session.control_state.driver_generation,
        native_session_id: detached_session
            .native_session_ref
            .as_ref()
            .expect("native Session")
            .native_session_id
            .clone(),
        node_daemon_id: detached_session.node_daemon_id.clone(),
        node_daemon_generation: detached_session.node_daemon_generation,
        authorizing_supervisor_id: lease.supervisor_id.clone(),
        authorizing_supervisor_generation: lease.generation,
    };
    let mut same_generation_running = blocked.clone();
    same_generation_running.status = MemberRunStatus::Running;
    same_generation_running.last_event_at = Some("unix-ms:recovery-source-drift".into());
    ledger
        .save_member_run(&blocked, &same_generation_running)
        .expect("drift source MemberRun before recovery latch");
    let mut drifted_request = pending_close_request(
        &run.id,
        &blocked.id,
        "host",
        "same-generation non-Blocked source must fail closed",
    );
    drifted_request.id = "close-detached-recovery-source-drift".into();
    drifted_request.detached_recovery_fence = Some(Box::new(exact_fence.clone()));
    let drifted_error = store
        .latch_team_member_close_for_supervisor(
            &drifted_request,
            &lease.supervisor_id,
            lease.generation,
        )
        .expect_err("same-generation non-Blocked MemberRun must reject recovery latch");
    assert!(drifted_error
        .to_string()
        .contains("DETACHED_RECOVERY_CLOSE_FENCE_MISMATCH"));
    assert!(store
        .team_member_close_requests()
        .expect("Close rows after same-generation source drift")
        .is_empty());
    let mut restored_blocked = same_generation_running.clone();
    restored_blocked.status = MemberRunStatus::Blocked;
    restored_blocked.last_event_at = Some("unix-ms:recovery-source-restored".into());
    ledger
        .save_member_run(&same_generation_running, &restored_blocked)
        .expect("restore exact Blocked source before recovery latch");
    blocked = restored_blocked;
    for (case, forged_fence) in [
        (
            "wrong-session",
            harness_core::DetachedRecoveryCloseFence {
                agent_session_id: "agent-session:forged".into(),
                ..exact_fence.clone()
            },
        ),
        (
            "wrong-daemon",
            harness_core::DetachedRecoveryCloseFence {
                node_daemon_id: "node-daemon:forged".into(),
                ..exact_fence.clone()
            },
        ),
    ] {
        let mut forged = pending_close_request(
            &run.id,
            &blocked.id,
            "host",
            "forged detached recovery source fact",
        );
        forged.id = format!("close-detached-recovery-{case}");
        forged.detached_recovery_fence = Some(Box::new(forged_fence));
        let error = store
            .latch_team_member_close_for_supervisor(&forged, &lease.supervisor_id, lease.generation)
            .expect_err("forged detached recovery authority must fail closed");
        assert!(error
            .to_string()
            .contains("DETACHED_RECOVERY_CLOSE_FENCE_MISMATCH"));
    }
    assert!(store
        .team_member_close_requests()
        .expect("Close rows after forged source facts")
        .is_empty());

    let mut stale_generation = blocked.clone();
    stale_generation.runtime_generation += 1;
    let stale = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &stale_generation,
        &lease,
        "host",
        "stale generation",
    )
    .expect_err("stale MemberRun generation must fail closed");
    assert!(stale.to_string().contains("MEMBER_RUN_SCOPE_MISMATCH"));

    let mut probation_blocked = blocked.clone();
    probation_blocked.zero_output_streak = 2;
    probation_blocked.last_event_at = Some("unix-ms:recovery-probation".into());
    ledger
        .save_member_run(&blocked, &probation_blocked)
        .expect("seed a nonzero probation continuation streak");

    let multiple = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &probation_blocked,
        &lease,
        "host",
        "multiple received Works require Host reconciliation",
    )
    .expect_err("multiple provider-received active Work revisions must fence recovery Close");
    assert!(multiple
        .to_string()
        .contains("multiple provider-received active Work revisions"));
    let deliveries_before_cancel = store
        .fabric_work_deliveries(&lease.execution_space_id)
        .expect("provider-received evidence before Host reconciliation");
    let bindings_before_cancel = store
        .fabric_work_execution_bindings(&lease.execution_space_id)
        .expect("execution bindings before Host reconciliation");
    let commands_before_cancel = store
        .runtime_commands(&lease.execution_space_id)
        .expect("RuntimeCommands before Host reconciliation");
    for (index, work) in [stale_work_a, stale_work_b].into_iter().enumerate() {
        store
            .cancel_work(
                &work.id,
                work.version,
                "obsolete after detached provider recovery",
                WorkCommandContext {
                    event_id: format!("recovery-stale-cancel-{index}"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("recovery-stale-cancel-{index}"),
                    created_at: format!("unix-ms:recovery-cancel-{index}"),
                    duplicate_ok: false,
                },
            )
            .expect("Host reconciles one provider-received Work");
    }
    assert_eq!(
        store
            .fabric_work_deliveries(&lease.execution_space_id)
            .expect("provider receipts after Host reconciliation"),
        deliveries_before_cancel,
        "Host reconciliation must preserve delivery evidence"
    );
    assert_eq!(
        store
            .fabric_work_execution_bindings(&lease.execution_space_id)
            .expect("bindings after Host reconciliation"),
        bindings_before_cancel,
        "Host reconciliation must not fabricate binding release"
    );
    assert_eq!(
        store
            .runtime_commands(&lease.execution_space_id)
            .expect("RuntimeCommands after Host reconciliation"),
        commands_before_cancel,
        "Host reconciliation must not issue a provider effect"
    );

    let successor_body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members: latest_member_runs_in_append_order(&store)
            .expect("successor prepared roster")
            .into_iter()
            .filter(|member| member.team_run_id == run.id && member.coordination_is_active())
            .collect(),
    };
    assert!(successor_body
        .members
        .iter()
        .any(|member| member.id == probation_blocked.id));
    let commands_before_successor = store
        .runtime_commands(&lease.execution_space_id)
        .expect("RuntimeCommands before successor admission");
    let (close_latched_tx, close_latched_rx) = std::sync::mpsc::sync_channel(0);
    let (allow_terminal_cas_tx, allow_terminal_cas_rx) = std::sync::mpsc::sync_channel(0);
    let (terminal_member_tx, terminal_member_rx) = std::sync::mpsc::sync_channel(0);
    let (finish_close_tx, finish_close_rx) = std::sync::mpsc::sync_channel(0);
    let recovery_store = &store;
    let recovery_run_id = run.id.as_str();
    let recovery_member = &probation_blocked;
    let recovery_lease = &lease;
    let recovered = std::thread::scope(|scope| {
        let mut terminal_cas_release = ScopedChannelRelease::new(allow_terminal_cas_tx);
        let mut close_settlement_release = ScopedChannelRelease::new(finish_close_tx);
        let recovery = scope.spawn(move || {
            let mut latched_once = false;
            let mut terminal_paused_once = false;
            close_detached_blocked_member_for_recovery_with_hooks(
                recovery_store,
                recovery_run_id,
                recovery_member,
                recovery_lease,
                "host",
                "explicit detached recovery",
                DetachedRecoveryCloseMode::BlockedMemberExactGeneration,
                |_| {
                    if !latched_once {
                        latched_once = true;
                        close_latched_tx.send(()).expect("publish latched Close");
                        allow_terminal_cas_rx
                            .recv_timeout(Duration::from_secs(5))
                            .map_err(|error| {
                                CliError::Usage(format!(
                                    "timed out waiting to allow terminal MemberRun CAS: {error}"
                                ))
                            })?;
                    }
                    Ok(())
                },
                |closed| {
                    if !terminal_paused_once {
                        terminal_paused_once = true;
                        terminal_member_tx
                            .send(closed.clone())
                            .expect("publish terminal MemberRun before Close settlement");
                        finish_close_rx
                            .recv_timeout(Duration::from_secs(5))
                            .map_err(|error| {
                                CliError::Usage(format!(
                                    "timed out waiting to settle recovery Close: {error}"
                                ))
                            })?;
                    }
                    Ok(())
                },
            )
        });
        close_latched_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recovery Close reached pre-terminal fence");
        let early_pending = store
            .latest_team_member_close_request(&probation_blocked.id)
            .expect("early Pending recovery Close")
            .expect("early recovery Close row");
        assert_eq!(early_pending.status, TeamMemberCloseStatus::Pending);
        let close_rows_before_early_apply = store
            .team_member_close_requests()
            .expect("Close rows before early completion");
        let early_apply_error = store
            .complete_team_member_close(
                &run.id,
                &probation_blocked.id,
                &early_pending.id,
                "unix-ms:forged-early-apply",
            )
            .expect_err("recovery Close cannot apply before exact terminal MemberRun");
        assert!(early_apply_error
            .to_string()
            .contains("DETACHED_RECOVERY_CLOSE_POSTCONDITION_MISMATCH"));
        assert_eq!(
            store
                .team_member_close_requests()
                .expect("Close rows after rejected early completion"),
            close_rows_before_early_apply,
            "rejected early recovery completion must append no row"
        );
        terminal_cas_release.release("release terminal MemberRun CAS");
        let terminal_member = terminal_member_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recovery Close reached its in-flight settlement fence");
        assert_eq!(terminal_member.status, MemberRunStatus::Stopped);
        assert!(terminal_member.coordination_is_closed());
        let pending = store
            .latest_team_member_close_request(&probation_blocked.id)
            .expect("Pending recovery Close")
            .expect("recovery Close row");
        assert_eq!(pending.status, TeamMemberCloseStatus::Pending);
        let predecessor_fence = pending
            .detached_recovery_fence
            .as_deref()
            .expect("exact predecessor recovery fence");
        assert_eq!(
            predecessor_fence.authorizing_supervisor_id,
            lease.supervisor_id
        );
        assert_eq!(
            predecessor_fence.authorizing_supervisor_generation,
            lease.generation
        );

        store
            .release_team_supervisor_lease(
                &run.id,
                &lease.supervisor_id,
                lease.generation,
                current_unix_ms_u64(),
            )
            .expect("release predecessor Supervisor generation");
        let successor = store
            .acquire_test_supervisor_lease(
                &run.id,
                "supervisor-detached-recovery-successor",
                std::process::id(),
                "test://detached-recovery-successor",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire successor Supervisor generation");
        assert!(successor.generation > lease.generation);
        bind_team_runtime_supervisor(
            &store,
            &successor_body,
            &successor.execution_space_id,
            &successor.node_daemon_id,
            &successor.supervisor_id,
            successor.generation,
        )
        .expect("successor binds the stale prepared roster before admission");
        let successor_ledger = TeamRunLedger::new(
            &store,
            &run.id,
            &successor.supervisor_id,
            successor.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let (_, rebound_session) =
            provider_session_for_member(&successor_ledger, &probation_blocked)
                .expect("same detached Session rebound to successor");
        assert!(matches!(
            rebound_session.control_state.driver_ref,
            harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
                ref team_run_id,
                ref team_supervisor_id,
                team_supervisor_generation,
            } if team_run_id == &run.id
                && team_supervisor_id == &successor.supervisor_id
                && team_supervisor_generation == successor.generation
        ));
        let stale_error = match prepare_member_workspace_for_spawn(
            &ledger,
            &probation_blocked,
            &test_provider_environment_observation(&root),
        ) {
            Ok(_) => panic!("predecessor ledger reconciled under successor authority"),
            Err(error) => error,
        };
        assert!(stale_error.is_supervisor_lease_lost());
        let wrong_successor_ledger = TeamRunLedger::new(
            &store,
            &run.id,
            "supervisor-detached-recovery-wrong-same-generation",
            successor.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let wrong_successor_error = match prepare_member_workspace_for_spawn(
            &wrong_successor_ledger,
            &probation_blocked,
            &test_provider_environment_observation(&root),
        ) {
            Ok(_) => panic!("same-generation wrong Supervisor id reconciled Close"),
            Err(error) => error,
        };
        assert!(wrong_successor_error.is_supervisor_lease_lost());
        let successor_admission_ledger = TeamRunLedger::new(
            &store,
            &run.id,
            &successor.supervisor_id,
            successor.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let (pending_observed_tx, pending_observed_rx) = std::sync::mpsc::sync_channel(0);
        let successor_prepared = probation_blocked.clone();
        let successor_root = root.clone();
        let successor_admission = scope.spawn(move || {
            let mut observed_once = false;
            prepare_member_workspace_for_spawn_with_recovery_pending_hook(
                &successor_admission_ledger,
                &successor_prepared,
                &test_provider_environment_observation(&successor_root),
                |observed| {
                    if !observed_once {
                        observed_once = true;
                        pending_observed_tx
                            .send(observed.id.clone())
                            .expect("publish exact Pending observation");
                    }
                    Ok(())
                },
            )
        });
        assert_eq!(
            pending_observed_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("successor observed exact Pending recovery transaction"),
            pending.id
        );
        assert_eq!(
            store
                .runtime_commands(&lease.execution_space_id)
                .expect("RuntimeCommands before explicit ambiguity injection"),
            commands_before_successor,
            "successor Pending observation itself must issue no provider effect"
        );
        let post_terminal_effect =
            prepare_provider_process_effect(&successor_ledger, &probation_blocked, 9);
        let commands_after_rejected_effect = store
            .runtime_commands(&lease.execution_space_id)
            .expect("RuntimeCommands after rejected post-terminal admission");
        transition_provider_session_runtime_control(
            &successor_ledger,
            &probation_blocked,
            RuntimeResidency::Attached,
            RuntimeActivity::Idle,
        )
        .expect("attach Session after successor observed Pending");
        close_settlement_release.release("finish recovery Close");
        let changed_session_error = match successor_admission
            .join()
            .expect("successor admission thread")
        {
            Ok(_) => panic!("successor ignored Session authority drift before Applied"),
            Err(error) => error,
        };
        assert!(changed_session_error
            .to_string()
            .contains("reached Applied after its exact detached AgentSession authority or provider-effect certainty changed"));
        let post_terminal_effect_error = match post_terminal_effect {
            Ok(_) => panic!("Closed/Stopped MemberRun admitted a new provider effect"),
            Err(error) => error,
        };
        assert!(post_terminal_effect_error
            .to_string()
            .contains("MEMBER_RUN_GENERATION_FENCED"));
        assert_eq!(
            commands_after_rejected_effect, commands_before_successor,
            "terminal MemberRun must reject a new provider effect with zero command delta"
        );
        transition_provider_session_runtime_control(
            &successor_ledger,
            &probation_blocked,
            RuntimeResidency::Detached,
            RuntimeActivity::Idle,
        )
        .expect("return Session to exact detached terminal authority");
        assert!(matches!(
            prepare_member_workspace_for_spawn(
                &successor_ledger,
                &probation_blocked,
                &test_provider_environment_observation(&root),
            )
            .expect("successor reconciles Applied recovery Close after authority restoration"),
            PreSpawnWorkspacePreparation::Superseded
        ));
        recovery
            .join()
            .expect("recovery thread")
            .expect("recovery Close")
            .expect("detached recovery result")
    });
    assert_eq!(recovered["runtime_effect"], "already_detached");
    assert_eq!(recovered["provider_close_receipt"], "not_fabricated");
    let closed = latest_member_runs_in_append_order(&store)
        .expect("closed member rows")
        .into_iter()
        .find(|member| member.id == blocked.id)
        .expect("closed member");
    assert_eq!(closed.coordination_status, MemberCoordinationStatus::Closed);
    assert_eq!(closed.status, MemberRunStatus::Stopped);
    assert_eq!(
        closed.zero_output_streak, 0,
        "the consumed provider-received revision cannot probation-continue after Reopen"
    );
    assert_eq!(
        store
            .runtime_commands(&lease.execution_space_id)
            .expect("RuntimeCommands after successor reconciliation"),
        commands_before_successor,
        "successor reconciliation must issue no provider effect or replay old Work"
    );
    assert!(store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .all(|action| action.action_type != "team_supervisor_recovery_required"));

    assert_eq!(
        closed.native_session, blocked.native_session,
        "recovery Close must preserve the exact native session for the real provider-backed Reopen path"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn detached_blocked_recovery_authority_takeover_never_persists_closed_blocked() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    use std::cell::Cell;

    let (store, root) = temp_store("detached-recovery-authority-takeover");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:race-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");
    let first = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-recovery-race-first",
            std::process::id(),
            "test://recovery-race-first",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire first Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &first);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members: latest_member_runs_in_append_order(&store)
            .expect("members")
            .into_iter()
            .filter(|member| member.team_run_id == run.id)
            .collect(),
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &first.execution_space_id,
        &first.node_daemon_id,
        &first.supervisor_id,
        first.generation,
    )
    .expect("bind first Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &first.supervisor_id,
        first.generation,
        Arc::new(AtomicBool::new(true)),
    );
    transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
        .expect("idle session");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("attach runtime");
    let mut blocked = bound.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:race-blocked".into());
    ledger
        .save_member_run(&bound, &blocked)
        .expect("block member");
    settle_provider_attempt_release(&ledger, &blocked).expect("detach runtime");

    let successor_generation = Cell::new(0_u64);
    let error = close_detached_blocked_member_for_recovery_with_hook(
        &store,
        &run.id,
        &blocked,
        &first,
        "host",
        "successor races terminal recovery CAS",
        |_| {
            store.release_team_supervisor_lease(
                &run.id,
                &first.supervisor_id,
                first.generation,
                current_unix_ms_u64(),
            )?;
            let successor = store.acquire_test_supervisor_lease(
                &run.id,
                "supervisor-recovery-race-successor",
                std::process::id(),
                "test://recovery-race-successor",
                current_unix_ms_u64(),
                60_000,
            )?;
            successor_generation.set(successor.generation);
            Ok(())
        },
    )
    .expect_err("the stale Supervisor must lose terminal recovery CAS");
    assert!(error.is_supervisor_lease_lost());
    assert!(successor_generation.get() > first.generation);
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest member")
        .into_iter()
        .find(|member| member.id == blocked.id)
        .expect("member row");
    assert!(latest.coordination_is_active());
    assert_eq!(latest.status, MemberRunStatus::Blocked);
    assert!(
        !(latest.coordination_is_closed() && latest.status == MemberRunStatus::Blocked),
        "authority loss must never strand Closed + Blocked"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn detached_blocked_recovery_samples_lease_time_after_writer_lock_wait() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    use std::sync::mpsc;

    let (store, root) = temp_store("detached-recovery-lock-expiry");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:expiry-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-recovery-expiry",
            std::process::id(),
            "test://recovery-expiry",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members: latest_member_runs_in_append_order(&store)
            .expect("members")
            .into_iter()
            .filter(|member| member.team_run_id == run.id)
            .collect(),
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
        .expect("idle session");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("attach runtime");
    let mut blocked = bound.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:expiry-blocked".into());
    ledger
        .save_member_run(&bound, &blocked)
        .expect("block member");
    settle_provider_attempt_release(&ledger, &blocked).expect("detach runtime");

    let near_expiry = store
        .renew_team_supervisor_lease(
            &run.id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms_u64(),
            100,
        )
        .expect("renew near-expiry Supervisor lease");
    let member_rows_before = store.member_runs().expect("member rows before").len();
    let guard = store
        .acquire_exclusive_migration_guard()
        .expect("hold Store writer lock across lease expiry");
    let worker_store = store.clone();
    let worker_run_id = run.id.clone();
    let worker_member = blocked.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).expect("signal recovery start");
        close_detached_blocked_member_for_recovery(
            &worker_store,
            &worker_run_id,
            &worker_member,
            &near_expiry,
            "host",
            "writer contention crosses lease expiry",
        )
    });
    started_rx.recv().expect("recovery thread started");
    std::thread::sleep(Duration::from_millis(250));
    drop(guard);

    let error = worker
        .join()
        .expect("recovery thread")
        .expect_err("expired authority must fail after writer-lock wait");
    assert!(
        error.is_supervisor_lease_lost(),
        "unexpected error: {error}"
    );
    assert_eq!(
        store.member_runs().expect("member rows after").len(),
        member_rows_before,
        "expired authority must append no MemberRun revision"
    );
    assert!(
        store
            .team_member_close_requests()
            .expect("Close requests")
            .is_empty(),
        "expired authority must not even latch Close intent"
    );
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest member")
        .into_iter()
        .find(|member| member.id == blocked.id)
        .expect("blocked member");
    assert!(latest.coordination_is_active());
    assert_eq!(latest.status, MemberRunStatus::Blocked);

    std::fs::remove_dir_all(root).expect("cleanup");
}
