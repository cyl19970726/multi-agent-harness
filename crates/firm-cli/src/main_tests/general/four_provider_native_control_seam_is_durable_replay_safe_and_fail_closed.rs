use super::*;

#[test]
fn four_provider_native_control_seam_is_durable_replay_safe_and_fail_closed() {
    use harness_core::agentfirm_api::{
        AgentSessionStatus, PermissionCeiling, RuntimeCommandStatus, RuntimeDispatchMode,
        RuntimeEffectCertainty, RuntimePostconditionStatus,
    };

    let cases = [
        (
            "codex",
            crate::provider_adapter::NativeControlPrimitive::CodexTurnInterrupt,
        ),
        (
            "claude",
            crate::provider_adapter::NativeControlPrimitive::ClaudeAgentSdkInterrupt,
        ),
        (
            "kimi",
            crate::provider_adapter::NativeControlPrimitive::KimiAcpCancel,
        ),
        (
            "pi",
            crate::provider_adapter::NativeControlPrimitive::PiRpcInterrupt,
        ),
    ];

    for (provider, primitive) in cases {
        let (store, root) = temp_store(&format!("provider-control-seam-{provider}"));
        let created = create_two_member_team_run_for_provider(&store, provider);
        let member = created.member_runs[0].clone();
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                &format!("provider-control-{provider}"),
                std::process::id(),
                "test://provider-control",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire provider-control Supervisor");
        ensure_test_runtime_fabric(&store, &created, &lease);
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        transition_provider_session_for_member(&ledger, &member, AgentSessionStatus::Active)
            .expect("activate provider session");

        let requested_ceiling = if matches!(provider, "kimi" | "pi") {
            // Pi's tool allowlist cannot enforce a workspace path
            // boundary. The production adapter admits either read-only
            // or explicit trusted full access; Kimi is likewise admitted
            // only under its exact full-access callback mapping.
            PermissionCeiling::FullAccess
        } else {
            PermissionCeiling::WorkspaceWrite
        };
        let mapping = crate::provider_adapter::map_permission(provider, requested_ceiling)
            .expect("provider permission mapping");
        assert_eq!(mapping.effective, requested_ceiling);
        assert_eq!(
            crate::provider_adapter::effective_delivery_mode(
                provider,
                RuntimeDispatchMode::InjectIfSafe,
                AgentSessionStatus::Active,
                false,
            )
            .expect("safe-injection decision"),
            RuntimeDispatchMode::QueueOnly,
            "{provider} must downgrade unproven injection"
        );

        let mut shim = FaithfulProviderControlShim {
            provider,
            primitive,
            native_effects: 0,
            fail_after_dispatch: false,
        };
        let pending = match crate::provider_adapter::execute_team_control(
            &ledger,
            &member,
            &format!("{provider}-control-source"),
            "operator requested interrupt",
            false,
            &mut shim,
        )
        .expect("durably prepare and dispatch provider-native control")
        {
            crate::provider_adapter::ProviderControlDispatch::Pending(pending) => pending,
            crate::provider_adapter::ProviderControlDispatch::Replayed => {
                panic!("first dispatch cannot be replay")
            }
        };
        assert_eq!(shim.native_effects, 1);
        crate::provider_adapter::settle_team_control(
            &ledger,
            &pending,
            Some("faithful_shim_terminal_ack"),
        )
        .expect("settle terminal provider acknowledgement");
        let applied = store
            .runtime_commands(&lease.execution_space_id)
            .expect("runtime commands")
            .into_iter()
            .rev()
            .find(|command| command.id == pending.command_id())
            .expect("durable provider control command");
        assert_eq!(applied.status, RuntimeCommandStatus::Applied);
        assert_eq!(applied.effect_certainty, RuntimeEffectCertainty::Applied);
        assert_eq!(
            applied.postcondition_status,
            RuntimePostconditionStatus::Satisfied
        );

        assert!(matches!(
            crate::provider_adapter::execute_team_control(
                &ledger,
                &member,
                &format!("{provider}-control-source"),
                "operator requested interrupt",
                false,
                &mut shim,
            )
            .expect("exact replay"),
            crate::provider_adapter::ProviderControlDispatch::Replayed
        ));
        assert_eq!(shim.native_effects, 1, "replay repeated {provider} effect");

        shim.fail_after_dispatch = true;
        let error = crate::provider_adapter::execute_team_control(
            &ledger,
            &member,
            &format!("{provider}-uncertain-source"),
            "transport loss exercise",
            false,
            &mut shim,
        )
        .expect_err("uncertain native effect must fail closed");
        assert!(error.to_string().contains("PROVIDER_CONTROL_FAILED"));
        let uncertain = store
            .runtime_commands(&lease.execution_space_id)
            .expect("runtime commands after transport loss")
            .into_iter()
            .find(|command| {
                command.status == RuntimeCommandStatus::RecoveryRequired
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
            })
            .expect("uncertain provider control enters recovery inventory");
        assert_eq!(
            uncertain.postcondition_status,
            RuntimePostconditionStatus::Unknown
        );
        assert!(uncertain.failure_code.as_deref().is_some_and(|failure| {
            failure.contains("faithful shim transport lost after native dispatch")
        }));
        let before_retry = shim.native_effects;
        let replay_error = crate::provider_adapter::execute_team_control(
            &ledger,
            &member,
            &format!("{provider}-uncertain-source"),
            "transport loss exercise",
            false,
            &mut shim,
        )
        .expect_err("uncertain replay requires governed recovery");
        assert!(replay_error
            .to_string()
            .contains("RUNTIME_COMMAND_RECOVERY_REQUIRED"));
        assert_eq!(shim.native_effects, before_retry);
        std::fs::remove_dir_all(root).expect("cleanup");

        // Use an independent canonical session because a RecoveryRequired
        // command correctly blocks every later effect in the same session.
        // This second matrix case exercises the distinct gap where native
        // dispatch returned Ok but the terminal acknowledgement vanished.
        let (ack_store, ack_root) = temp_store(&format!("provider-control-ack-lost-{provider}"));
        let ack_created = create_two_member_team_run_for_provider(&ack_store, provider);
        let ack_member = ack_created.member_runs[0].clone();
        let ack_lease = ack_store
            .acquire_test_supervisor_lease(
                &ack_created.team_run.id,
                &format!("provider-control-ack-lost-{provider}"),
                std::process::id(),
                "test://provider-control-ack-lost",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire ack-loss provider-control Supervisor");
        ensure_test_runtime_fabric(&ack_store, &ack_created, &ack_lease);
        let ack_ledger = TeamRunLedger::new(
            &ack_store,
            &ack_created.team_run.id,
            &ack_lease.supervisor_id,
            ack_lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        transition_provider_session_for_member(
            &ack_ledger,
            &ack_member,
            AgentSessionStatus::Active,
        )
        .expect("activate ack-loss provider session");
        let mut ack_shim = FaithfulProviderControlShim {
            provider,
            primitive,
            native_effects: 0,
            fail_after_dispatch: false,
        };
        let ack_lost_pending = match crate::provider_adapter::execute_team_control(
            &ack_ledger,
            &ack_member,
            &format!("{provider}-terminal-ack-lost-source"),
            "terminal acknowledgement loss exercise",
            false,
            &mut ack_shim,
        )
        .expect("native dispatch succeeds before terminal acknowledgement is lost")
        {
            crate::provider_adapter::ProviderControlDispatch::Pending(pending) => pending,
            crate::provider_adapter::ProviderControlDispatch::Replayed => {
                panic!("first ack-loss dispatch cannot be replay")
            }
        };
        assert_eq!(ack_shim.native_effects, 1);
        crate::provider_adapter::settle_team_control(&ack_ledger, &ack_lost_pending, None)
            .expect("lost terminal acknowledgement enters recovery inventory");
        let ack_lost_command = ack_store
            .runtime_commands(&ack_lease.execution_space_id)
            .expect("runtime commands after terminal acknowledgement loss")
            .into_iter()
            .find(|command| command.id == ack_lost_pending.command_id())
            .expect("durable ack-loss provider control command");
        assert_eq!(
            ack_lost_command.status,
            RuntimeCommandStatus::RecoveryRequired
        );
        assert_eq!(
            ack_lost_command.effect_certainty,
            RuntimeEffectCertainty::Unknown
        );
        let effects_before_ack_lost_replay = ack_shim.native_effects;
        let ack_lost_replay = crate::provider_adapter::execute_team_control(
            &ack_ledger,
            &ack_member,
            &format!("{provider}-terminal-ack-lost-source"),
            "terminal acknowledgement loss exercise",
            false,
            &mut ack_shim,
        )
        .expect_err("ack-loss replay requires governed recovery");
        assert!(ack_lost_replay
            .to_string()
            .contains("RUNTIME_COMMAND_RECOVERY_REQUIRED"));
        assert_eq!(
            ack_shim.native_effects, effects_before_ack_lost_replay,
            "ack-loss replay repeated {provider} native effect"
        );
        std::fs::remove_dir_all(ack_root).expect("cleanup ack-loss store");
    }
}
