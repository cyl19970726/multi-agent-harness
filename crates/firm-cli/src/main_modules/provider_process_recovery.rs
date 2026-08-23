use super::*;

pub(super) const MAX_AUTOMATIC_PROVIDER_TRANSPORT_ATTEMPTS: u64 = 3;

pub(super) fn durable_provider_process_outcome(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    transport_attempt: u64,
) -> harness_application::ProviderEffectOutcome {
    let execution_space_id = match ledger.store.trust_member_run_scope(&member.id) {
        Ok(Some(execution_space_id)) => execution_space_id,
        Ok(None) => {
            return harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: format!("runtime-command:missing-scope:{}", member.id),
            }
        }
        Err(error) => {
            return harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: format!("runtime-command-read:{error}"),
            }
        }
    };
    let commands = match ledger.store.runtime_commands(&execution_space_id) {
        Ok(commands) => commands,
        Err(error) => {
            return harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: format!("runtime-command-read:{error}"),
            }
        }
    };
    let kind = if member.native_session.is_some() {
        harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession
    } else {
        harness_core::agentfirm_api::RuntimeCommandKind::OpenRuntime
    };
    provider_process_outcome_from_commands(
        &commands,
        &member.id,
        member.runtime_generation,
        ledger.supervisor_generation,
        transport_attempt,
        kind,
    )
}

fn provider_process_outcome_from_commands(
    commands: &[harness_core::agentfirm_api::RuntimeCommandRecord],
    member_run_id: &str,
    member_run_generation: u64,
    supervisor_generation: u64,
    transport_attempt: u64,
    kind: harness_core::agentfirm_api::RuntimeCommandKind,
) -> harness_application::ProviderEffectOutcome {
    let suffix = format!(":{supervisor_generation}:{transport_attempt}:{kind:?}");
    let matches = commands
        .iter()
        .filter(|command| {
            command.command == kind
                && command.binding.target_member_run_id.as_deref() == Some(member_run_id)
                && command.binding.target_member_run_generation == Some(member_run_generation)
                && command.idempotency_key.ends_with(&suffix)
        })
        .collect::<Vec<_>>();
    let command = match matches.as_slice() {
        [] => {
            return harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: format!(
                    "runtime-command:missing-provider-process:{member_run_id}:{member_run_generation}:{supervisor_generation}:{transport_attempt}"
                ),
            }
        }
        [command] => *command,
        _ => {
            return harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: format!(
                    "runtime-command:ambiguous-provider-process:{member_run_id}:{member_run_generation}:{supervisor_generation}:{transport_attempt}"
                ),
            }
        }
    };
    match command.effect_certainty {
        harness_core::agentfirm_api::RuntimeEffectCertainty::Applied => {
            harness_application::ProviderEffectOutcome::Accepted {
                receipt_id: command.id.clone(),
            }
        }
        harness_core::agentfirm_api::RuntimeEffectCertainty::NotApplied => {
            harness_application::ProviderEffectOutcome::NotApplied {
                reason: command
                    .failure_code
                    .clone()
                    .unwrap_or_else(|| "provider process effect was not applied".into()),
            }
        }
        harness_core::agentfirm_api::RuntimeEffectCertainty::None
        | harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown => {
            harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: command.id.clone(),
            }
        }
    }
}

pub(super) fn provider_retry_authority_after_failure(
    error: &CliError,
    durable_process_outcome: &harness_application::ProviderEffectOutcome,
    transport_attempt: u64,
) -> harness_application::ProviderRetryAuthority {
    let error_outcome = error.provider_effect_outcome();
    match (&error_outcome, durable_process_outcome) {
        (harness_application::ProviderEffectOutcome::Unknown { .. }, _) => {
            harness_application::provider_retry_authority(
                &error_outcome,
                transport_attempt,
                MAX_AUTOMATIC_PROVIDER_TRANSPORT_ATTEMPTS,
            )
        }
        (_, _) if matches!(error, CliError::ProviderAdmissionRejected(_)) => {
            harness_application::ProviderRetryAuthority::StopNoRetry
        }
        (_, outcome @ harness_application::ProviderEffectOutcome::Unknown { .. }) => {
            harness_application::provider_retry_authority(
                outcome,
                transport_attempt,
                MAX_AUTOMATIC_PROVIDER_TRANSPORT_ATTEMPTS,
            )
        }
        (_, outcome) => harness_application::provider_retry_authority(
            outcome,
            transport_attempt,
            MAX_AUTOMATIC_PROVIDER_TRANSPORT_ATTEMPTS,
        ),
    }
}

pub(super) fn provider_process_idempotency_key(
    session: &harness_core::agentfirm_api::AgentSession,
    supervisor_generation: u64,
    transport_attempt: u64,
    kind: harness_core::agentfirm_api::RuntimeCommandKind,
) -> String {
    format!(
        "provider-process:{}:{}:{}:{}:{}:{}:{kind:?}",
        session.id,
        session.runtime_generation,
        session.node_daemon_generation,
        session.control_state.driver_generation,
        supervisor_generation,
        transport_attempt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliError;

    #[test]
    fn provider_process_retry_identity_is_stable_and_generation_scoped() {
        let session = test_agent_session();
        let first = provider_process_idempotency_key(
            &session,
            7,
            1,
            harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
        );
        assert_eq!(
            first,
            provider_process_idempotency_key(
                &session,
                7,
                1,
                harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
            )
        );
        assert_ne!(
            first,
            provider_process_idempotency_key(
                &session,
                7,
                2,
                harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
            )
        );
        assert_ne!(
            first,
            provider_process_idempotency_key(
                &session,
                8,
                1,
                harness_core::agentfirm_api::RuntimeCommandKind::ResumeNativeSession,
            )
        );
    }

    #[test]
    fn cli_provider_errors_preserve_effect_certainty_without_message_parsing() {
        let unknown = CliError::RuntimeRecoveryRequired("runtime-command:uncertain".into());
        assert_eq!(
            unknown.provider_effect_outcome(),
            harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: "runtime-command:uncertain".into()
            }
        );

        let not_applied = CliError::Usage("spawn failed before provider input".into());
        assert_eq!(
            not_applied.provider_effect_outcome(),
            harness_application::ProviderEffectOutcome::NotApplied {
                reason: "spawn failed before provider input".into()
            }
        );

        let trust_error = harness_core::agentfirm_api::TrustError {
            code: harness_core::agentfirm_api::TrustErrorCode::RuntimeEffectUnknown,
            message: "wording is not policy".into(),
            retryable: false,
            resource_kind: "runtime_command".into(),
            resource_id: "runtime-command:1".into(),
            current_version: None,
        };
        let store_unknown = CliError::Store(harness_store::StoreError::Conflict(
            serde_json::to_string(&trust_error).expect("TrustError serializes"),
        ));
        assert_eq!(
            store_unknown.provider_effect_outcome(),
            harness_application::ProviderEffectOutcome::Unknown {
                recovery_ref: "runtime_command:runtime-command:1".into()
            }
        );

        let missing_process_evidence = harness_application::ProviderEffectOutcome::Unknown {
            recovery_ref: "runtime-command:missing-provider-process".into(),
        };
        assert_eq!(
            provider_retry_authority_after_failure(
                &CliError::ProviderAdmissionRejected("session generation fenced".into()),
                &missing_process_evidence,
                1,
            ),
            harness_application::ProviderRetryAuthority::StopNoRetry
        );
        let accepted = CliError::ProviderEffectAccepted("runtime-command:applied".into());
        assert_eq!(
            accepted.provider_effect_outcome(),
            harness_application::ProviderEffectOutcome::Accepted {
                receipt_id: "runtime-command:applied".into()
            }
        );
        assert_eq!(
            harness_application::provider_retry_authority(
                &accepted.provider_effect_outcome(),
                1,
                3,
            ),
            harness_application::ProviderRetryAuthority::StopNoRetry
        );
        assert_eq!(
            harness_application::provider_retry_authority(&unknown.provider_effect_outcome(), 1, 3,),
            harness_application::ProviderRetryAuthority::RequireReconciliation {
                recovery_ref: "runtime-command:uncertain".into()
            }
        );
        assert_eq!(
            harness_application::provider_retry_authority(
                &not_applied.provider_effect_outcome(),
                1,
                3,
            ),
            harness_application::ProviderRetryAuthority::RetryWithNewAttempt { next_attempt: 2 }
        );
        assert_eq!(
            harness_application::provider_retry_authority(
                &not_applied.provider_effect_outcome(),
                3,
                3,
            ),
            harness_application::ProviderRetryAuthority::StopNoRetry
        );
    }

    #[test]
    fn durable_applied_process_effect_prevents_a_second_attempt_for_all_five_providers() {
        for provider in ["codex", "claude", "kimi", "deepseek_harness", "pi"] {
            let command: harness_core::agentfirm_api::RuntimeCommandRecord =
                serde_json::from_value(serde_json::json!({
                    "id": format!("runtime-command:{provider}:applied"),
                    "execution_space_id": "space",
                    "target_node_id": "node",
                    "target_node_daemon_id": "daemon",
                    "target_node_daemon_generation": 4,
                    "authenticated_actor": {"kind": "service", "id": "daemon"},
                    "command": "open_runtime",
                    "required_capability": "runtime.open",
                    "idempotency_key": format!("provider-process:session:1:4:2:7:1:OpenRuntime"),
                    "request_fingerprint": "fingerprint",
                    "status": "applied",
                    "phase": "settled",
                    "effect_certainty": "applied",
                    "postcondition_status": "satisfied",
                    "binding": {
                        "target_member_run_id": "member-run",
                        "target_member_run_generation": 1
                    },
                    "precondition": {},
                    "postcondition": {},
                    "target_session_id": "session",
                    "target_session_generation": 1,
                    "source_record_id": null,
                    "result": {"provider": provider, "phase": "runtime_attached"},
                    "failure_code": null,
                    "version": 2,
                    "created_at": "t1",
                    "updated_at": "t2"
                }))
                .expect("RuntimeCommandRecord fixture");
            let outcome = provider_process_outcome_from_commands(
                &[command],
                "member-run",
                1,
                7,
                1,
                harness_core::agentfirm_api::RuntimeCommandKind::OpenRuntime,
            );
            assert!(matches!(
                outcome,
                harness_application::ProviderEffectOutcome::Accepted { .. }
            ));
            assert_eq!(
                provider_retry_authority_after_failure(
                    &CliError::Usage("post-settlement projection write failed".into()),
                    &outcome,
                    1,
                ),
                harness_application::ProviderRetryAuthority::StopNoRetry,
                "{provider} post-settlement projection failure allocated another attempt"
            );
        }
    }

    #[test]
    fn missing_durable_process_evidence_requires_reconciliation() {
        let outcome = provider_process_outcome_from_commands(
            &[],
            "member-run",
            1,
            7,
            1,
            harness_core::agentfirm_api::RuntimeCommandKind::OpenRuntime,
        );
        assert_eq!(
            provider_retry_authority_after_failure(
                &CliError::Usage("ordinary projection failure".into()),
                &outcome,
                1,
            ),
            harness_application::ProviderRetryAuthority::RequireReconciliation {
                recovery_ref: "runtime-command:missing-provider-process:member-run:1:7:1".into(),
            }
        );
    }

    fn test_agent_session() -> harness_core::agentfirm_api::AgentSession {
        serde_json::from_value(serde_json::json!({
            "id": "agent-session:member:node:1:1",
            "agent_member_id": "member",
            "node_id": "node",
            "execution_space_id": "space",
            "node_daemon_id": "node-daemon:node",
            "node_daemon_generation": 5,
            "provider_kind": "kimi",
            "provider_profile_ref": "provider-profile:kimi",
            "runtime_generation": 1,
            "lifecycle": "active",
            "effective_permission_ceiling": "full_access",
            "workspace_cwd": "/tmp",
            "permission_envelope_ref": "permission:member",
            "native_session_ref": null,
            "current_turn_id": null,
            "queued_input_count": 0,
            "control_state": {
                "runtime_residency": "attached",
                "activity": "idle",
                "execution_driver": "host_driven",
                "driver_generation": 2,
                "driver_ref": {
                    "kind": "team_supervisor",
                    "team_run_id": "team-run",
                    "team_supervisor_id": "supervisor-7",
                    "team_supervisor_generation": 7
                },
                "composition_fingerprint": "composition",
                "capability_fingerprint": "capability"
            },
            "version": 1,
            "opened_at": "t0",
            "last_active_at": "t0",
            "closed_at": null
        }))
        .expect("test AgentSession must deserialize")
    }
}
