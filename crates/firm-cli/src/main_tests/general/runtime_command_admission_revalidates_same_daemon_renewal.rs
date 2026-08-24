use super::*;

#[test]
fn runtime_command_admission_revalidates_same_daemon_renewal() {
    let (store, _root) = temp_store("runtime-command-current-daemon-lease");
    let node_id = "83f00000-0000-4000-8000-000000000001";
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: "runtime command current lease".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:0".into(),
            updated_at: "unix-ms:0".into(),
        })
        .expect("insert ExecutionNode");

    let initial = store
        .acquire_node_daemon_lease(node_id, "daemon-a", "instance-a", 1_000, 1_000)
        .expect("acquire initial daemon lease");
    let renewed = store
        .renew_node_daemon_lease(
            node_id,
            &initial.daemon_id,
            initial.generation,
            &initial.instance_id,
            1_500,
            3_000,
        )
        .expect("renew exact daemon instance");

    let current = current_node_daemon_lease_after_admission_at(
        &store,
        &initial,
        2_500,
        "runtime-command:test-renewal",
    )
    .expect("the exact daemon renewal remains current after the admitted snapshot expires");
    assert_eq!(current.generation, initial.generation);
    assert_eq!(current.instance_id, initial.instance_id);
    assert_eq!(current.expires_unix_ms, renewed.expires_unix_ms);

    store
        .release_node_daemon_lease(
            node_id,
            &renewed.daemon_id,
            renewed.generation,
            &renewed.instance_id,
            2_600,
        )
        .expect("release exact daemon instance");
    let successor = store
        .acquire_node_daemon_lease(node_id, "daemon-b", "instance-b", 2_600, 3_000)
        .expect("acquire successor daemon generation");
    assert!(successor.generation > initial.generation);

    let error = current_node_daemon_lease_after_admission_at(
        &store,
        &initial,
        2_700,
        "runtime-command:test-renewal",
    )
    .expect_err("a successor daemon generation must fence the admitted snapshot");
    assert!(matches!(error, CliError::RuntimeRecoveryRequired(_)));
    assert!(error.to_string().contains("runtime-command:test-renewal"));
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_CURRENT_LEASE_FENCED_AFTER_ADMISSION"),
        "unexpected error: {error}"
    );
}

#[test]
fn successor_after_durable_command_prepare_requires_exact_reconciliation() {
    let (store, _root) = temp_store("runtime-command-successor-after-prepare");
    let (ledger, member) = persisted_native_test_member(
        &store,
        "codex",
        "codex_app_server",
        "thread-runtime-command-successor",
    );
    let admitted = prepare_provider_process_effect(&ledger, &member, 1)
        .expect("durably prepare provider process command under current daemon");
    let execution_space_id = store
        .trust_member_run_scope(&member.id)
        .expect("read MemberRun scope")
        .expect("canonical MemberRun scope");
    let session = store
        .fabric_agent_sessions(&execution_space_id)
        .expect("read AgentSessions")
        .into_iter()
        .find(|session| session.agent_member_id == member.agent_member_id)
        .expect("exact provider AgentSession");
    let prepared = store
        .runtime_commands(&execution_space_id)
        .expect("read RuntimeCommands")
        .into_iter()
        .find(|command| command.id == admitted.command_id)
        .expect("the command is durable before revalidation failure");
    assert_eq!(
        prepared.status,
        harness_core::agentfirm_api::RuntimeCommandStatus::Accepted
    );
    assert_eq!(
        prepared.effect_certainty,
        harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown
    );

    let current = store
        .latest_node_daemon_lease(&session.node_id)
        .expect("read current daemon lease")
        .expect("active daemon lease");
    let now = current_unix_ms_u64();
    store
        .release_node_daemon_lease(
            &current.node_id,
            &current.daemon_id,
            current.generation,
            &current.instance_id,
            now,
        )
        .expect("release admitted daemon generation");
    store
        .acquire_node_daemon_lease(
            &current.node_id,
            "successor-daemon",
            "successor-instance",
            now,
            60_000,
        )
        .expect("acquire successor daemon generation");

    let error =
        current_node_daemon_lease_after_admission_at(&store, &current, now, &admitted.command_id)
            .expect_err("successor authority after prepare requires reconciliation");
    assert!(matches!(error, CliError::RuntimeRecoveryRequired(_)));
    assert!(error.to_string().contains(&admitted.command_id));
}
