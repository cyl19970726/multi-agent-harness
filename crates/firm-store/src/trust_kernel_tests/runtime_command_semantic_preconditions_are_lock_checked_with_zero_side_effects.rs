use super::*;

#[test]
fn runtime_command_semantic_preconditions_are_lock_checked_with_zero_side_effects() {
    for case in [
        "session_version",
        "residency",
        "activity",
        "execution_driver",
        "cycle_ref",
        "continuation_ref",
        "continuation_phase",
        "runtime_idle",
        "execution_lane_quiesced",
    ] {
        let (store, root) = fabric_store();
        let identity_id = format!("precondition-{case}");
        let session_id = format!("session-precondition-{case}");
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", &identity_id, 0),
                identity(&identity_id),
            )
            .unwrap();
        let target = session(&session_id, &identity_id);
        store
            .create_agent_session(
                &service_context("session.create", &session_id, 0),
                target.clone(),
            )
            .unwrap();
        let (mut command, mut admission) = runtime_command_fixture(
            &format!("precondition-{case}"),
            RuntimeCommandKind::OpenRuntime,
            &target,
            "open_runtime",
        );
        match case {
            "session_version" => command.precondition.expected_session_version = Some(2),
            "residency" => {
                command.precondition.expected_residency = Some(RuntimeResidency::Attached)
            }
            "activity" => command.precondition.expected_activity = Some(RuntimeActivity::Running),
            "execution_driver" => {
                command.precondition.expected_execution_driver =
                    Some(MemberExecutionDriver::ProviderDriven)
            }
            "cycle_ref" => {
                command.precondition.expected_cycle_ref =
                    Some(firm_core::agentfirm_api::RuntimeNativeObjectRef {
                        id: "missing-cycle".into(),
                        revision: None,
                        fingerprint: None,
                    })
            }
            "continuation_ref" => {
                command.precondition.expected_continuation_ref =
                    Some(firm_core::agentfirm_api::RuntimeNativeObjectRef {
                        id: "missing-continuation".into(),
                        revision: None,
                        fingerprint: None,
                    })
            }
            "continuation_phase" => {
                command.precondition.expected_continuation_phase =
                    Some(NativeContinuationPhase::Active)
            }
            "runtime_idle" => {
                command.precondition.safe_point = RuntimeSafePointRequirement::RuntimeIdle
            }
            "execution_lane_quiesced" => {
                command.precondition.safe_point = RuntimeSafePointRequirement::ExecutionLaneQuiesced
            }
            _ => unreachable!(),
        }
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let before = store.canonical_operations().unwrap();
        let error = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-rejected")
            .expect_err("an unproven semantic precondition must reject before admission");
        assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before, "{case}");
        assert!(store.runtime_commands("space-test").unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
