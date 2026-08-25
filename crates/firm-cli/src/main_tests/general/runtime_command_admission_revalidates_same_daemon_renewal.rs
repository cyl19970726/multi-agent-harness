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

#[test]
fn reopened_member_generation_gets_a_distinct_resume_command_identity() {
    let (store, _root) = temp_store("resume-command-reopened-member-generation");
    let (ledger, first_generation) = persisted_native_test_member(
        &store,
        "codex",
        "codex_app_server",
        "thread-resume-command-generation",
    );

    let first = prepare_provider_process_effect(&ledger, &first_generation, 1)
        .expect("prepare generation-1 ResumeNativeSession command");
    settle_provider_effect_not_applied(
        &ledger,
        &first,
        "generation-1 runtime was closed before provider effect".into(),
    )
    .expect("settle generation-1 command without provider effect");

    let mut reopened = first_generation.clone();
    reopened.runtime_generation += 1;
    reopened.status = MemberRunStatus::Queued;
    reopened.started_at = "unix-ms:reopened".into();
    reopened.last_event_at = Some("unix-ms:reopened".into());
    store
        .compare_and_advance_member_run_generation(&first_generation, &reopened)
        .expect("advance the durable MemberRun generation for Reopen");

    let second = prepare_provider_process_effect(&ledger, &reopened, 1)
        .expect("same transport-attempt ordinal in a new MemberRun generation must not collide");
    assert_ne!(first.command_id, second.command_id);

    let execution_space_id = store
        .trust_member_run_scope(&reopened.id)
        .expect("read MemberRun scope")
        .expect("canonical MemberRun scope");
    let commands = store
        .runtime_commands(&execution_space_id)
        .expect("read exact RuntimeCommands");
    let first_record = commands
        .iter()
        .find(|command| command.id == first.command_id)
        .expect("generation-1 command remains durable");
    let second_record = commands
        .iter()
        .find(|command| command.id == second.command_id)
        .expect("generation-2 command is independently durable");
    assert_eq!(
        first_record.binding.target_member_run_generation,
        Some(first_generation.runtime_generation)
    );
    assert_eq!(
        second_record.binding.target_member_run_generation,
        Some(reopened.runtime_generation)
    );
    assert_eq!(
        second_record.command,
        harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession
    );
}

#[test]
fn provider_process_retry_after_daemon_lease_renewal_reconciles_existing_command() {
    let (store, _root) = temp_store("provider-process-retry-after-daemon-renewal");
    let (ledger, member) = persisted_native_test_member(
        &store,
        "codex",
        "codex_app_server",
        "thread-provider-process-retry-renewal",
    );

    let first = prepare_provider_process_effect(&ledger, &member, 1)
        .expect("prepare the immutable provider-process attempt");
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
    let initial = store
        .latest_node_daemon_lease(&session.node_id)
        .expect("read NodeDaemon lease")
        .expect("active NodeDaemon lease");
    let now = current_unix_ms_u64();
    let renewed = store
        .renew_node_daemon_lease(
            &initial.node_id,
            &initial.daemon_id,
            initial.generation,
            &initial.instance_id,
            now,
            120_000,
        )
        .expect("renew the same daemon generation");
    assert_ne!(initial.expires_unix_ms, renewed.expires_unix_ms);

    let error = prepare_provider_process_effect(&ledger, &member, 1)
        .expect_err("the same attempt must reconcile instead of rebuilding a drifting envelope");
    assert!(matches!(error, CliError::RuntimeRecoveryRequired(_)));
    assert!(error.to_string().contains(&first.command_id));
    assert!(!error.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
    assert_eq!(
        store
            .runtime_commands(&execution_space_id)
            .expect("read RuntimeCommands")
            .into_iter()
            .filter(|command| command.id == first.command_id)
            .count(),
        1
    );
}

#[test]
fn changed_agent_session_version_gets_a_distinct_resume_command_identity() {
    let (store, _root) = temp_store("resume-command-changed-session-version");
    let (ledger, member) = persisted_native_test_member(
        &store,
        "codex",
        "codex_app_server",
        "thread-resume-command-session-version",
    );

    let first = prepare_provider_process_effect(&ledger, &member, 1)
        .expect("prepare the first AgentSession-version command");
    settle_provider_effect_not_applied(
        &ledger,
        &first,
        "first session-version effect was not applied".into(),
    )
    .expect("settle first command without provider effect");
    transition_provider_session_for_member(
        &ledger,
        &member,
        harness_core::agentfirm_api::AgentSessionStatus::Idle,
    )
    .expect("advance the canonical AgentSession version");

    let second = prepare_provider_process_effect(&ledger, &member, 1)
        .expect("the same transport attempt under a new session version must not collide");
    assert_ne!(first.command_id, second.command_id);

    let execution_space_id = store
        .trust_member_run_scope(&member.id)
        .expect("read MemberRun scope")
        .expect("canonical MemberRun scope");
    let commands = store
        .runtime_commands(&execution_space_id)
        .expect("read RuntimeCommands");
    let first_record = commands
        .iter()
        .find(|command| command.id == first.command_id)
        .expect("first session-version command remains durable");
    let second_record = commands
        .iter()
        .find(|command| command.id == second.command_id)
        .expect("second session-version command is independently durable");
    assert!(
        second_record.precondition.expected_session_version
            > first_record.precondition.expected_session_version
    );
}
