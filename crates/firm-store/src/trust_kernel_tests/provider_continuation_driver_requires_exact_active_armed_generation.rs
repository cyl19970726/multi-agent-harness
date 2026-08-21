use super::*;

#[test]
fn provider_continuation_driver_requires_exact_active_armed_generation() {
    for case in ["exact", "disarmed", "revision"] {
        let (store, root) = fabric_store();
        let identity_id = format!("continuation-{case}");
        let session_id = format!("session-continuation-{case}");
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", &identity_id, 0),
                identity(&identity_id),
            )
            .unwrap();
        let mut target = session(&session_id, &identity_id);
        target.control_state.execution_driver = MemberExecutionDriver::ProviderDriven;
        target.control_state.driver_generation = 7;
        target.control_state.driver_ref = RuntimeDriverRef::ProviderContinuation {
            provider: "codex".into(),
            continuation_id: "native-goal-1".into(),
            continuation_revision: Some(3),
            runtime_generation: 1,
        };
        target
            .control_state
            .continuation
            .definition
            .continuation_ref = Some("native-goal-1".into());
        target.control_state.continuation.definition.revision =
            Some(if case == "revision" { 4 } else { 3 });
        target.control_state.continuation.definition.phase = NativeContinuationPhase::Active;
        target.control_state.continuation.activation = if case == "disarmed" {
            NativeContinuationActivation::Disarmed
        } else {
            NativeContinuationActivation::Armed {
                runtime_generation: 1,
                driver_generation: 7,
            }
        };
        store
            .create_agent_session(
                &service_context("session.create", &session_id, 0),
                target.clone(),
            )
            .unwrap();
        if case == "exact" {
            let (start_cycle, start_admission) = runtime_command_fixture(
                "continuation-must-not-host-start",
                RuntimeCommandKind::StartCycle,
                &target,
                "start_cycle",
            );
            let before_start = store.canonical_operations().unwrap();
            let error = store
                .prepare_runtime_command(
                    &start_admission,
                    &start_cycle,
                    current_unix_ms(),
                    "t-start-rejected",
                )
                .expect_err(
                    "an armed provider continuation must remain the only next-cycle driver",
                );
            assert!(error.to_string().contains(
                "cannot start a provider cycle while the AgentSession is provider-driven"
            ));
            assert_eq!(store.canonical_operations().unwrap(), before_start);
            assert!(store.runtime_commands("space-test").unwrap().is_empty());
        }
        let (command, admission) = runtime_command_fixture(
            &format!("continuation-command-{case}"),
            RuntimeCommandKind::InspectContinuation,
            &target,
            "inspect_continuation",
        );
        let before = store.canonical_operations().unwrap();
        let result = store.prepare_runtime_command(&admission, &command, current_unix_ms(), "t");
        if case == "exact" {
            assert_eq!(
                result.unwrap().projection.status,
                RuntimeCommandStatus::Accepted
            );
        } else {
            let error = result.expect_err("continuation fence must reject mismatch");
            assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
            assert_eq!(store.canonical_operations().unwrap(), before);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
