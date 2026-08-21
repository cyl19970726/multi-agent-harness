use super::*;

#[test]
fn runtime_command_exact_binding_rejects_stale_fields_before_acceptance() {
    for field in ["driver", "composition", "capability"] {
        let (store, root) = fabric_store();
        let identity_id = format!("binding-{field}");
        let session_id = format!("session-binding-{field}");
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
            &format!("binding-command-{field}"),
            RuntimeCommandKind::OpenRuntime,
            &target,
            "open_runtime",
        );
        match field {
            "driver" => command.binding.target_driver_generation = Some(2),
            "composition" => {
                command.binding.composition_fingerprint = Some("composition:stale".into())
            }
            "capability" => {
                command.binding.capability_fingerprint = Some("capability:stale".into())
            }
            _ => unreachable!(),
        }
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let before = store.canonical_operations().unwrap();
        let error = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-rejected")
            .expect_err("a stale exact-binding field must be fenced before Accepted");
        assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before, "{field}");
        assert!(store.runtime_commands("space-test").unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
