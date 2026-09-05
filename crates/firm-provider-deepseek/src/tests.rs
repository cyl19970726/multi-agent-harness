use super::*;
use harness_core::agentfirm_api::PermissionCeiling;
use std::fs;

#[test]
fn runtime_config_debug_redacts_collaboration_bearer() {
    let token = "de".repeat(32);
    let envelope = harness_runtime_contract::CollaborationCapabilityEnvelope::new(
        harness_runtime_contract::CollaborationCapabilitySecret::new(token.clone()).unwrap(),
        harness_runtime_contract::CollaborationCapabilityBinding {
            team_run_id: "team-run-test".into(),
            member_run_id: "member-run-test".into(),
            member_run_generation: 1,
            agent_session_id: "session-test".into(),
            agent_session_generation: 1,
            node_daemon_id: "daemon-test".into(),
            node_daemon_generation: 1,
            supervisor_id: "supervisor-test".into(),
            supervisor_generation: 1,
        },
        COLLABORATION_CAPABILITY_MECHANISM,
    )
    .unwrap();
    let config = DeepSeekTeamRuntimeConfig {
        runner_path: PathBuf::from("runner.mjs"),
        cwd: PathBuf::from("/tmp/project"),
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "DeepSeek test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "full_access".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: Vec::new(),
        resume_session_id: None,
        environment: collaboration_agent_tool_environment(&envelope).unwrap(),
    };
    assert!(!format!("{config:?}").contains(&token));
}

#[test]
fn native_cycle_correlation_never_crosses_input_ids() {
    let correlation = native_cycle_correlation(
        "deepseek-cycle-2",
        ControlTransportReceipt {
            command: "deliver".into(),
            response_id: Some("deepseek-sdk-session:native:deepseek-cycle-2".into()),
            success: true,
        },
        "turn_complete",
        "native",
    );
    assert_eq!(correlation.provider_input_id, "deepseek-cycle-2");
    assert_eq!(
        correlation.terminal_provider_input_id.as_deref(),
        Some("deepseek-cycle-2")
    );
    assert_eq!(
        correlation.exact_terminal_ref.as_deref(),
        Some("deepseek_harness.turn_complete:deepseek-cycle-2:native")
    );
}

fn reviewed_runner_fixture(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "deepseek-runner-composition-{label}-{}-{}",
        std::process::id(),
        now_string().replace(':', "-")
    ));
    fs::create_dir_all(root.join("bin")).expect("runner bin");
    fs::write(root.join("bin/deepseek-member-runner.mjs"), "// fixture").expect("runner fixture");
    fs::write(
        root.join("package.json"),
        include_str!("../../../apps/deepseek-member-runner/package.json"),
    )
    .expect("package fixture");
    fs::write(
        root.join("cordis.yml"),
        include_str!("../../../apps/deepseek-member-runner/cordis.yml"),
    )
    .expect("composition fixture");
    let reviewed = embedded_reviewed_provider().expect("reviewed provider");
    for (name, version) in reviewed["dependencies"]
        .as_object()
        .expect("reviewed dependencies")
    {
        let package_dir = root.join("node_modules").join(name);
        fs::create_dir_all(&package_dir).expect("dependency fixture");
        fs::write(
            package_dir.join("package.json"),
            serde_json::to_vec(&json!({"name": name, "version": version})).unwrap(),
        )
        .expect("dependency package fixture");
    }
    root
}

#[test]
fn runner_package_and_contract_are_exactly_version_bound() {
    let root = reviewed_runner_fixture("exact");
    let runner = root.join("bin/deepseek-member-runner.mjs");
    verify_runner_harness_composition(&runner).expect("reviewed DSH composition");
    let contract: Value = serde_json::from_str(include_str!(
        "../../../apps/deepseek-member-runner/contract/runner-v1.json"
    ))
    .expect("runner contract");
    assert_eq!(contract["protocolVersion"], DEEPSEEK_NATIVE_PROTOCOL);
    assert_eq!(
        contract["commands"],
        serde_json::json!(["start", "deliver", "interrupt", "close"])
    );
    assert_eq!(
        contract["reviewedProvider"]["sourceRevision"],
        REVIEWED_DEEPSEEK_SOURCE_REVISION
    );
    assert_eq!(
        contract["reviewedProvider"]["compositionFingerprint"],
        REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn dependency_and_cordis_drift_fail_before_runner_spawn() {
    let dependency_root = reviewed_runner_fixture("dependency-drift");
    let dependency_runner = dependency_root.join("bin/deepseek-member-runner.mjs");
    fs::write(
        dependency_root.join("node_modules/@deepseek-ai/dsh-sandbox-policy/package.json"),
        r#"{"name":"@deepseek-ai/dsh-sandbox-policy","version":"0.1.1-rc.3"}"#,
    )
    .expect("drift dependency");
    let dependency_error = verify_runner_harness_composition(&dependency_runner)
        .expect_err("security plugin drift must fail closed")
        .to_string();
    assert!(dependency_error.contains("DEEPSEEK_HARNESS_DEPENDENCY_UNREVIEWED"));
    fs::remove_dir_all(dependency_root).expect("remove dependency fixture");

    let composition_root = reviewed_runner_fixture("composition-drift");
    let composition_runner = composition_root.join("bin/deepseek-member-runner.mjs");
    fs::write(
        composition_root.join("cordis.yml"),
        "- id: unreviewed-plugin\n",
    )
    .expect("drift composition");
    let composition_error = verify_runner_harness_composition(&composition_runner)
        .expect_err("Cordis drift must fail closed")
        .to_string();
    assert!(composition_error.contains("DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED"));
    fs::remove_dir_all(composition_root).expect("remove composition fixture");
}

#[test]
fn session_binding_revalidates_source_and_composition_identity() {
    let exact = json!({
        "providerVersion": REVIEWED_DEEPSEEK_HARNESS_VERSION,
        "sourceRevision": REVIEWED_DEEPSEEK_SOURCE_REVISION,
        "compositionFingerprint": REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT
    });
    assert_eq!(
        verify_session_bound_provider_identity(&exact).expect("exact provider identity"),
        REVIEWED_DEEPSEEK_HARNESS_VERSION
    );

    let mut source_drift = exact.clone();
    source_drift["sourceRevision"] = json!("unreviewed-source");
    assert!(verify_session_bound_provider_identity(&source_drift)
        .expect_err("source drift")
        .to_string()
        .contains("DEEPSEEK_HARNESS_SOURCE_REVISION_UNREVIEWED"));

    let mut composition_drift = exact;
    composition_drift["compositionFingerprint"] = json!("sha256:unreviewed");
    assert!(verify_session_bound_provider_identity(&composition_drift)
        .expect_err("composition drift")
        .to_string()
        .contains("DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED"));
}

#[test]
fn permission_ceiling_compiles_into_the_shared_dsh_policy() {
    assert_eq!(
        compile_harness_permission(PermissionCeiling::ReadOnly),
        ("read-only", "dsh-sandbox-policy")
    );
    assert_eq!(
        compile_harness_permission(PermissionCeiling::WorkspaceWrite),
        ("workspace-write", "dsh-sandbox-policy")
    );
    assert_eq!(
        compile_harness_permission(PermissionCeiling::FullAccess),
        ("danger-full-access", "dsh-sandbox-policy")
    );
}

#[test]
fn runner_contract_accepts_known_enum_only_activity_kind_and_rejects_unknown_kind() {
    let known = json!({
        "event":"provider_activity",
        "data":{"kind":"thinking","summary":"DeepSeek Harness is thinking"}
    });
    runner_contract::validate_runner_frame("eventPayloadSchemas", "event", "data", &known)
        .expect("contract-valid enum-only activity kind");

    let unknown = json!({
        "event":"provider_activity",
        "data":{"kind":"private_reasoning_payload","summary":"must fail closed"}
    });
    let error =
        runner_contract::validate_runner_frame("eventPayloadSchemas", "event", "data", &unknown)
            .expect_err("unknown activity kind must remain closed")
            .to_string();
    assert!(error.contains("provider_activity.data.kind is outside the allowed enum"));
}

// ---------------------------------------------------------------------------
// SPEC-TYPED-CYCLE-OUTCOME-01 §5: the S1 assertion family against DeepSeek.

#[cfg(unix)]
mod cycle_conformance {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{self, Sender};
    use std::time::Duration;

    fn scripted_deepseek_transport() -> (DeepSeekRunnerTransport, Sender<String>) {
        let mut child = Command::new("sh")
            .args(["-c", "cat >/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn scripted runner sink");
        let stdin = child.stdin.take().expect("scripted runner stdin");
        let (line_tx, lines) = mpsc::channel();
        (
            DeepSeekRunnerTransport {
                child: DeepSeekRunnerChild::new(child).expect("scripted runner child"),
                stdin: Some(stdin),
                lines,
                stdout_reader: None,
                stderr_reader: None,
                native_session_id: "scripted-session".to_string(),
                expected_resume_session_id: None,
                provider_version: None,
                state: TransportState::Idle,
                next_input_id: 1,
                pending_input_count: 0,
                last_cycle_terminal: false,
                last_interrupt_resumed_same_session: false,
                close_reason: None,
            },
            line_tx,
        )
    }

    fn ds_event(event: &str, data: serde_json::Value) -> String {
        serde_json::json!({"event": event, "data": data}).to_string()
    }

    fn ds_consumed(input_id: &str) -> String {
        ds_event(
            "consumed",
            serde_json::json!({"id": input_id, "kind": "runtime_cycle", "sessionId": "scripted-session"}),
        )
    }

    fn ds_assistant_message() -> String {
        ds_event(
            "assistant_message",
            serde_json::json!({"sessionId": "scripted-session", "content": "done"}),
        )
    }

    fn ds_turn_complete(input_id: &str) -> String {
        ds_event(
            "turn_complete",
            serde_json::json!({
                "sessionId": "scripted-session",
                "subtype": "success",
                "triggerMessageId": input_id,
                "evidenceRefs": [],
                "isError": false,
                "terminalReason": "end_turn",
                "apiErrorStatus": null
            }),
        )
    }

    fn ds_timeouts() -> harness_runtime_contract::CycleTimeouts {
        harness_runtime_contract::CycleTimeouts {
            input_acceptance: Duration::from_millis(1),
            transport_liveness: Duration::from_millis(1),
            control_settle: Duration::ZERO,
        }
    }

    fn drive_ds_cycle(
        events: Vec<String>,
        disconnect: bool,
        timeouts: &harness_runtime_contract::CycleTimeouts,
        control: impl FnMut() -> harness_runtime_contract::CycleControl,
    ) -> Result<harness_runtime_contract::ExecutionCycleOutcome, String> {
        drive_ds_cycle_with_silence(events, disconnect, 0, timeouts, control)
    }

    /// `silence_ms` is a REAL wall-clock silent interval injected between the
    /// first scripted event (the acceptance receipt) and the rest — the
    /// "silent tool interval" the A1/B4 regressions must prove is never a
    /// failure.
    fn drive_ds_cycle_with_silence(
        events: Vec<String>,
        disconnect: bool,
        silence_ms: u64,
        timeouts: &harness_runtime_contract::CycleTimeouts,
        mut control: impl FnMut() -> harness_runtime_contract::CycleControl,
    ) -> Result<harness_runtime_contract::ExecutionCycleOutcome, String> {
        let (mut transport, line_tx) = scripted_deepseek_transport();
        let mut events = events.into_iter();
        if let Some(first) = events.next() {
            line_tx.send(first).map_err(|error| error.to_string())?;
        }
        let rest: Vec<String> = events.collect();
        let rest_tx = if disconnect { line_tx } else { line_tx.clone() };
        let silence = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(silence_ms));
            for event in rest {
                let _ = rest_tx.send(event);
            }
        });
        let outcome = transport
            .run_cycle(
                "conformance cycle",
                *timeouts,
                &mut |_receipt| Ok(()),
                &mut |_pending, _result| Ok(()),
                &mut |_event| {},
                &mut control,
            )
            .map_err(|error| error.to_string());
        let _ = silence.join();
        outcome
    }

    struct DeepSeekCycleConformanceFixture;

    impl harness_runtime_contract::CycleConformanceFixture for DeepSeekCycleConformanceFixture {
        type Error = String;

        fn run_receipt_then_silence(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let outcome = drive_ds_cycle_with_silence(
                vec![
                    ds_consumed("deepseek-cycle-2"),
                    ds_assistant_message(),
                    ds_turn_complete("deepseek-cycle-2"),
                ],
                false,
                250,
                timeouts,
                harness_runtime_contract::CycleControl::default,
            )?;
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: outcome.interrupt.clone(),
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Outcome(Box::new(
                    outcome,
                )),
            })
        }

        fn run_no_receipt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let error = match drive_ds_cycle(
                Vec::new(),
                false,
                timeouts,
                harness_runtime_contract::CycleControl::default,
            ) {
                Ok(_) => return Err("a never-accepted cycle produced an outcome".to_string()),
                Err(error) => error,
            };
            assert!(error.contains("INPUT_ACCEPTANCE_TIMEOUT"), "{error}");
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: None,
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Failed(
                    harness_runtime_contract::CycleFailureDisposition::InputNeverAccepted,
                ),
            })
        }

        fn run_transport_dies_after_receipt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let error = match drive_ds_cycle(
                vec![ds_consumed("deepseek-cycle-2")],
                true,
                timeouts,
                harness_runtime_contract::CycleControl::default,
            ) {
                Ok(_) => return Err("a dead transport produced an outcome".to_string()),
                Err(error) => error,
            };
            assert!(error.contains("TRANSPORT_CLOSED"), "{error}");
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: None,
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Failed(
                    harness_runtime_contract::CycleFailureDisposition::AcceptedOutcomeUnknown,
                ),
            })
        }

        fn run_interrupt_not_acknowledged(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let mut first = true;
            let error = match drive_ds_cycle(
                vec![ds_consumed("deepseek-cycle-2")],
                false,
                timeouts,
                move || {
                    if std::mem::take(&mut first) {
                        harness_runtime_contract::CycleControl {
                            interrupt: true,
                            ..Default::default()
                        }
                    } else {
                        harness_runtime_contract::CycleControl::default()
                    }
                },
            ) {
                Ok(_) => return Err("an unacknowledged interrupt produced an outcome".to_string()),
                Err(error) => error,
            };
            assert!(error.contains("CONTROL_SETTLE_TIMEOUT"), "{error}");
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: None,
                control_unproven: true,
                result: harness_runtime_contract::CycleConformanceResult::Failed(
                    harness_runtime_contract::CycleFailureDisposition::AcceptedOutcomeUnknown,
                ),
            })
        }

        fn run_host_interrupt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let mut first = true;
            let outcome = drive_ds_cycle(
                vec![
                    ds_consumed("deepseek-cycle-2"),
                    ds_event(
                        "interrupted",
                        serde_json::json!({"stillQueued": [], "abandonedTriggerMessageIds": []}),
                    ),
                    ds_event(
                        "member_resumed_after_interrupt",
                        serde_json::json!({"sessionId": "scripted-session"}),
                    ),
                ],
                false,
                timeouts,
                move || {
                    if std::mem::take(&mut first) {
                        harness_runtime_contract::CycleControl {
                            interrupt: true,
                            ..Default::default()
                        }
                    } else {
                        harness_runtime_contract::CycleControl::default()
                    }
                },
            )?;
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: outcome.interrupt.clone(),
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Outcome(Box::new(
                    outcome,
                )),
            })
        }

        fn run_adapter_policy_interrupt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
            _reason: &str,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            self.run_receipt_then_silence(timeouts)
        }
    }

    #[test]
    fn deepseek_passes_the_s1_cycle_conformance_family() {
        let timeouts = ds_timeouts();
        let mut fixture = DeepSeekCycleConformanceFixture;
        harness_runtime_contract::assert_a1_accepted_input_survives_silence(
            &mut fixture,
            &timeouts,
        )
        .expect("A1");
        harness_runtime_contract::assert_a2_delivery_timeout_fails_closed(&mut fixture, &timeouts)
            .expect("A2");
        harness_runtime_contract::assert_a3_transport_death_fails_closed(&mut fixture, &timeouts)
            .expect("A3");
        harness_runtime_contract::assert_a5_control_settle_only_bounds_control(
            &mut fixture,
            &timeouts,
        )
        .expect("A5");
        harness_runtime_contract::assert_b1_host_interrupt_attribution(&mut fixture, &timeouts)
            .expect("B1");
    }

    #[test]
    fn deepseek_b4_silence_after_acceptance_never_fails_the_cycle() {
        let outcome = drive_ds_cycle_with_silence(
            vec![
                ds_consumed("deepseek-cycle-2"),
                ds_assistant_message(),
                ds_turn_complete("deepseek-cycle-2"),
            ],
            false,
            250,
            &ds_timeouts(),
            harness_runtime_contract::CycleControl::default,
        )
        .expect("a silent accepted cycle completes");
        assert_eq!(outcome.interrupt, None);
        assert!(outcome.provider_terminal_failure.is_none());
    }

    /// C1 (deepseek): a cycle whose terminal frame reports a provider error
    /// must settle its StartCycle receipt Unsatisfied — never Satisfied (#709).
    #[test]
    fn deepseek_c1_terminal_failure_settles_unsatisfied() {
        let outcome = drive_ds_cycle(
            vec![
                ds_consumed("deepseek-cycle-2"),
                ds_assistant_message(),
                ds_event(
                    "turn_complete",
                    serde_json::json!({
                        "sessionId": "scripted-session",
                        "subtype": "error",
                        "triggerMessageId": "deepseek-cycle-2",
                        "evidenceRefs": [],
                        "isError": true,
                        "terminalReason": "api_overloaded",
                        "apiErrorStatus": 529
                    }),
                ),
            ],
            false,
            &ds_timeouts(),
            harness_runtime_contract::CycleControl::default,
        )
        .expect("a terminal-failure cycle still returns an outcome");
        assert!(outcome.provider_terminal_failure.is_some());
        let receipt = harness_runtime_contract::EffectReceipt::for_cycle(
            "conformance-c1",
            harness_core::ProviderBindingAdmission::Active,
            harness_runtime_contract::CycleSettlement::from_cycle_outcome(&outcome),
        );
        harness_runtime_contract::assert_c1_terminal_failure_unsatisfied(&receipt).expect("C1");
    }
}
