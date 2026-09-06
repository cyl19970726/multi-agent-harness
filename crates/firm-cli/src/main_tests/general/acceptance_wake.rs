use super::*;
use harness_core::CurrentWorkDraft;

#[test]
fn host_acceptance_drives_one_reconsideration_cycle() {
    acceptance_fixture(AcceptanceScenario::Cycle);
}

#[test]
fn acceptance_preparation_retries_real_store_lock_contention() {
    acceptance_fixture(AcceptanceScenario::Contention);
}

#[test]
fn acceptance_preparation_rechecks_close_after_idle_selection() {
    acceptance_fixture(AcceptanceScenario::Close);
}

enum AcceptanceScenario {
    Cycle,
    Contention,
    Close,
}

fn acceptance_fixture(scenario: AcceptanceScenario) {
    let (store, root) = temp_store("canonical-supervisor-work-delivery");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let work = store
        .insert_work(
            {
                let mut draft = CurrentWorkDraft::new(
                    "canonical-supervisor-work".into(),
                    created.team_run.id.clone(),
                    created.team_run.agent_team_id.clone(),
                    "Deliver canonical Work".into(),
                    "Exercise NodeDaemon canonical delivery wiring".into(),
                    "Provider receipt is canonical".into(),
                    WorkClaimMode::HostAssign,
                    WorkPriority::Normal,
                    compatibility_team_actor("host", "test"),
                    "unix-ms:3".into(),
                );
                draft.eligible_member_ids = vec![member.agent_member_id.clone()];
                draft.into_work()
            },
            WorkCommandContext {
                event_id: "canonical-supervisor-work-created".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-create".into(),
                created_at: "unix-ms:3".into(),
                duplicate_ok: false,
            },
        )
        .expect("create unassigned Work");
    let membership = store
        .fabric_team_memberships("unit-test-space")
        .expect("Team memberships")
        .into_iter()
        .find(|membership| {
            membership.team_id == created.team_run.agent_team_id
                && membership.agent_member_id == member.agent_member_id
        })
        .expect("exact member TeamMembership");
    let work = store
        .assign_work_to_membership(
            &work.id,
            work.version,
            &membership.id,
            "unit-test-space",
            WorkCommandContext {
                event_id: "canonical-supervisor-work-assigned".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-assign".into(),
                created_at: "unix-ms:4".into(),
                duplicate_ok: false,
            },
        )
        .expect("assign stable TeamMembership responsibility");
    assert_eq!(work.active_member_run_id, None);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-work-supervisor",
            std::process::id(),
            "test://canonical-work-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("claim canonical Work")
        .expect("one canonical Work claim");
    let bindings = store
        .fabric_work_execution_bindings("unit-test-space")
        .expect("canonical WorkExecutionBinding");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].team_membership_id, membership.id);
    assert_eq!(bindings[0].agent_member_id, member.agent_member_id);
    ledger
        .complete_work_delivery(&claimed, "provider-work-receipt")
        .expect("record canonical provider receipt");
    assert!(
        claim_canonical_work_for_member(&ledger, &member)
            .expect("repeat scheduler scan is safe")
            .is_none(),
        "provider-received Work must not create or claim a second delivery"
    );
    let delivery = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical WorkDelivery fabric")
        .into_iter()
        .find(|delivery| delivery.work_id == work.id)
        .expect("canonical delivery");
    assert_eq!(
        delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        delivery.provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    let current = store
        .current_work_deliveries_for_team_run(&created.team_run.id)
        .expect("current canonical WorkDelivery view");
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].delivery_id, delivery.id);
    assert_eq!(
        current[0].status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        current[0].provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    assert_eq!(current[0].attempt, 1);
    assert_eq!(
        store
            .fabric_work_execution_bindings("unit-test-space")
            .expect("bindings after repeat scan")
            .len(),
        1,
        "repeat scheduling is idempotent"
    );
    assert_eq!(
        current[0].authority,
        harness_application::CurrentWorkDeliveryAuthority::CanonicalTrust
    );
    let started = store
        .start_work(
            &work.id,
            work.version,
            &member.id,
            WorkCommandContext {
                event_id: "canonical-supervisor-work-started".into(),
                performed_by_actor: compatibility_team_actor(&member.id, "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-start".into(),
                created_at: "unix-ms:5".into(),
                duplicate_ok: false,
            },
        )
        .expect("current MemberRun starts stable responsibility Work");
    assert_eq!(started.active_member_run_id, None);
    let candidate = harness_core::agentfirm_api::CandidateRef {
        kind: harness_core::agentfirm_api::CandidateKind::GitCommit,
        value: "abcdef0123456789".into(),
    };
    let candidate_fingerprint = harness_store::canonical_json_fingerprint(
        &serde_json::to_value(&candidate).expect("serialize exact candidate"),
    );
    let result_report = harness_core::agentfirm_api::WorkReport {
        id: "canonical-supervisor-work-result".into(),
        work_id: started.id.clone(),
        work_revision: started.version + 1,
        report_revision: 1,
        kind: harness_core::agentfirm_api::WorkReportKind::Result,
        authored_by: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: member.agent_member_id.clone(),
        },
        summary: "canonical stable-responsibility result".into(),
        base_revision: None,
        candidate: Some(candidate),
        candidate_fingerprint: Some(candidate_fingerprint.clone()),
        report_only: false,
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: vec!["artifact:canonical-work".into()],
        check_refs: vec!["check:canonical-work".into()],
        github_links: Vec::new(),
        evidence_refs: vec!["evidence:canonical-work".into()],
        known_risks: Vec::new(),
        confidence: None,
        recommended_next_action: None,
        created_at: "unix-ms:6".into(),
    };
    store
        .create_trust_work_report(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: result_report.authored_by.clone(),
                authority_actor: None,
                command_name: "work_report.create".into(),
                idempotency_key: "canonical-supervisor-work-result".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &created.team_run.agent_team_id,
            result_report,
        )
        .expect("ProviderReceived execution submits exact semantic Result");
    let submitted = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == started.id)
        .unwrap();
    assert_eq!(submitted.phase, WorkPhase::Review);
    assert_eq!(submitted.active_member_run_id, None);
    let released = store
        .fabric_work_execution_bindings("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|binding| binding.work_id == submitted.id)
        .unwrap();
    assert_eq!(
        released.status,
        harness_core::agentfirm_api::WorkExecutionBindingStatus::Released
    );
    let preserved_delivery = store
        .fabric_work_deliveries("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delivery.id)
        .unwrap();
    assert_eq!(
        preserved_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        preserved_delivery.provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    let host_context = |id: &str| WorkCommandContext {
        event_id: id.into(),
        performed_by_actor: compatibility_team_actor("host", "test"),
        authority_actor: None,
        causation_ref: None,
        idempotency_key: id.into(),
        created_at: "unix-ms:6".into(),
        duplicate_ok: false,
    };
    let standing = store
        .insert_work(
            CurrentWorkDraft::new(
                "standing-acceptance-wake".into(),
                created.team_run.id.clone(),
                created.team_run.agent_team_id.clone(),
                "Standing Work".into(),
                "Wait for own result acceptance".into(),
                "Reconsider blocker".into(),
                WorkClaimMode::HostAssign,
                WorkPriority::Normal,
                compatibility_team_actor("host", "test"),
                "unix-ms:6".into(),
            )
            .into_work(),
            host_context("standing-create"),
        )
        .unwrap();
    let standing = store
        .assign_work_to_membership(
            &standing.id,
            standing.version,
            &membership.id,
            "unit-test-space",
            host_context("standing-assign"),
        )
        .unwrap();
    let standing_claim = claim_canonical_work_for_member(&ledger, &member)
        .unwrap()
        .unwrap();
    assert_eq!(standing_claim.work.id, standing.id);
    ledger
        .complete_work_delivery(&standing_claim, "standing-receipt")
        .unwrap();
    let standing = store
        .start_work(
            &standing.id,
            standing.version,
            &member.id,
            WorkCommandContext {
                performed_by_actor: compatibility_team_actor(&member.id, "test"),
                ..host_context("standing-start")
            },
        )
        .unwrap();
    let standing = store
        .block_work_as_host(
            &standing.id,
            standing.version,
            "waiting for acceptance",
            host_context("standing-block"),
        )
        .unwrap();
    let accepted = store
        .accept_trust_work(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                    id: "host".into(),
                },
                authority_actor: None,
                command_name: "work.accept".into(),
                idempotency_key: "canonical-supervisor-work-accept".into(),
                expected_version: submitted.version,
                request_fingerprint: None,
            },
            &created.team_run.agent_team_id,
            &submitted.id,
            "canonical-supervisor-work-result",
            &candidate_fingerprint,
            "unix-ms:7",
        )
        .expect("Host acceptance remains independent from provider receipt and Result");
    assert_eq!(accepted.projection.phase, WorkPhase::Closed);
    let before_run = latest_team_run(&store, &created.team_run.id).unwrap();
    let mut running = before_run.clone();
    running.status = TeamRunStatus::Running;
    store
        .compare_and_append_team_run_lifecycle(&before_run, &running)
        .unwrap();
    bind_team_runtime_supervisor(
        &store,
        &PreparedTeamRunBody {
            run_id: running.id.clone(),
            objective: running.objective.clone(),
            run: running,
            members: created.member_runs.clone(),
        },
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .unwrap();
    let mut idle_member = ledger.latest_member_run(&member.id).unwrap().unwrap();
    let before_member = idle_member.clone();
    idle_member.status = MemberRunStatus::Idle;
    store
        .compare_and_append_member_run(&before_member, &idle_member)
        .unwrap();
    let (_sender, controls) = std::sync::mpsc::channel();
    let mut backoff = supervisor_wake::WakeBackoff::new();
    let wake = poll_idle_member_wake(
        &ledger,
        &mut idle_member,
        &controls,
        &mut || Ok(()),
        0,
        None,
        &supervisor_wake::WakePolicy::default(),
        &mut backoff,
    )
    .unwrap();
    assert!(
        matches!(wake, IdleWakeStep::Ready(IdleMemberWake::Acceptance(ref wake))
        if wake.accepted_work_id == submitted.id && wake.blocked_work_ids.as_slice() == std::slice::from_ref(&standing.id))
    );
    assert_eq!(
        store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|w| w.id == standing.id)
            .unwrap(),
        standing
    );

    if matches!(scenario, AcceptanceScenario::Close) {
        let source = match &wake {
            IdleWakeStep::Ready(IdleMemberWake::Acceptance(wake)) => wake.source_record_id(),
            _ => unreachable!(),
        };
        store
            .latch_team_member_close(&harness_core::TeamMemberCloseRequest {
                id: "acceptance-close-race".into(),
                team_run_id: ledger.run_id.clone(),
                member_run_id: member.id.clone(),
                requested_by: "host".into(),
                reason: "close requested after selection".into(),
                status: harness_core::TeamMemberCloseStatus::Pending,
                requested_at: now_string(),
                applied_at: None,
                detached_recovery_fence: None,
            })
            .unwrap();
        let error = prepare_provider_effect(
            &ledger,
            &idle_member,
            &source,
            "Reconsider blocked responsibility",
            1,
        )
        .err()
        .unwrap();
        assert!(matches!(error, CliError::ProviderAdmissionRejected(_)));
        assert!(error
            .to_string()
            .contains("WORK_ACCEPTANCE_WAKE_UNAVAILABLE"));
        assert!(store
            .runtime_commands(&lease.execution_space_id)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .latest_works()
                .unwrap()
                .into_iter()
                .find(|w| w.id == standing.id)
                .unwrap(),
            standing
        );
        std::fs::remove_dir_all(root).unwrap();
        return;
    }

    if matches!(scenario, AcceptanceScenario::Contention) {
        use std::os::fd::AsRawFd;
        let source = match &wake {
            IdleWakeStep::Ready(IdleMemberWake::Acceptance(wake)) => wake.source_record_id(),
            _ => unreachable!(),
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(store.root().join(".store.lock"))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let timeout_ms = std::env::var("FIRM_TEST_STORE_WRITE_LOCK_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .unwrap_or(10_000);
        let observer_store = store.clone();
        let space = lease.execution_space_id.clone();
        let release = std::thread::spawn(move || {
            // Hold beyond one complete Store timeout, forcing the production
            // acceptance wrapper to retry rather than merely wait inside Store.
            std::thread::sleep(Duration::from_millis(timeout_ms + 100));
            assert!(
                observer_store.runtime_commands(&space).unwrap().is_empty(),
                "no command exists while the physical writer lock is held"
            );
            drop(file);
        });
        let effect = prepare_provider_effect(
            &ledger,
            &idle_member,
            &source,
            "Reconsider blocked responsibility",
            1,
        )
        .unwrap();
        release.join().unwrap();
        assert_eq!(
            store
                .runtime_commands(&lease.execution_space_id)
                .unwrap()
                .len(),
            1
        );
        settle_provider_effect_not_applied(
            &ledger,
            &effect,
            "fixture never entered provider".into(),
        )
        .unwrap();
        let error = prepare_provider_effect(
            &ledger,
            &idle_member,
            &source,
            "A changed prompt cannot authorize another attempt",
            1,
        )
        .err()
        .unwrap();
        assert!(matches!(error, CliError::ProviderAdmissionRejected(_)));
        assert_eq!(
            store
                .runtime_commands(&lease.execution_space_id)
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_dir_all(root).unwrap();
        return;
    }

    // Drive the actual shared loop and production Codex adapter through its
    // existing narrow deterministic bridge; this is not native coding dogfood.
    let before = idle_member.clone();
    idle_member.status = MemberRunStatus::Idle;
    ledger.save_member_run(&before, &idle_member).unwrap();
    transition_provider_session_for_member(
        &ledger,
        &idle_member,
        harness_core::agentfirm_api::AgentSessionStatus::Idle,
    )
    .unwrap();
    // This deterministic bridge is the reviewed execution fixture, independent
    // of whichever provider binary/version is installed on the test machine.
    let before_profile = idle_member.clone();
    let profile = idle_member.provider_profile.as_mut().unwrap();
    profile.binding_admission = harness_core::ProviderBindingAdmission::Active;
    for binding in &mut profile.capability_bindings {
        binding.status = harness_core::ProviderCapabilityStatus::Verified;
        binding.admission = harness_core::ProviderBindingAdmission::Active;
        binding.evidence = [
            harness_core::ProviderCapabilityEvidenceKind::DeterministicAcceptance,
            harness_core::ProviderCapabilityEvidenceKind::LiveCanary,
        ]
        .into_iter()
        .map(|kind| harness_core::ProviderCapabilityEvidence {
            kind,
            evidence_ref: "fixture:simulated-reviewed-bridge-capability".into(),
            observed_at: None,
            note: Some(
                "Test fixture capability inventory; not evidence of a live provider run".into(),
            ),
        })
        .collect();
    }
    ledger
        .save_member_run(&before_profile, &idle_member)
        .unwrap();
    let inputs = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut adapter = crate::codex_team_runtime::CodexTeamRuntime::new(AcceptanceBridge {
        inputs: inputs.clone(),
        terminal_sent: std::cell::Cell::new(false),
    });
    let context = MemberRuntimeContext {
        execution_space_id: Some(lease.execution_space_id.clone()),
        project_id: None,
        project_selector: None,
        cwd: root.clone(),
        timeouts: harness_runtime_contract::CycleTimeouts::default(),
        live_sink: None,
        turn_leases: Arc::new(ActiveTurnLeasePool::new(1)),
        role_action_token: "fixture-token".into(),
    };
    let outcome = crate::runtime_adapter::run_team_member_with_adapter(
        &ledger,
        "Reconsider blocked responsibility",
        &mut idle_member,
        &context,
        &mut adapter,
        &controls,
        None,
        1,
    );
    assert!(
        outcome.is_err(),
        "fixture ends its transport after the single settled cycle"
    );
    let outcome_error = outcome.err().unwrap();
    assert_eq!(
        inputs.borrow().len(),
        1,
        "one provider-native bridge entry; outcome: {outcome_error}"
    );
    assert!(inputs.borrow()[0].contains("WORK ACCEPTANCE"));
    assert!(inputs.borrow()[0].contains(&standing.id));
    let commands: Vec<_> = store
        .runtime_commands(&lease.execution_space_id)
        .unwrap()
        .into_iter()
        .filter(|command| {
            command.source_record_id.as_deref().is_some_and(|source| {
                source.starts_with(harness_core::work_acceptance::ACCEPTANCE_WAKE_SOURCE_PREFIX)
            })
        })
        .collect();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].phase,
        harness_core::agentfirm_api::RuntimeCommandPhase::Settled
    );
    assert_eq!(
        commands[0].effect_certainty,
        harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
    );
    assert!(!commands[0]
        .source_record_id
        .as_ref()
        .unwrap()
        .contains(":turn:"));
    assert!(store
        .pending_work_acceptance_wake(&lease.execution_space_id, &member.id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|w| w.id == standing.id)
            .unwrap(),
        standing
    );

    let next_work = store
        .insert_work(
            CurrentWorkDraft::new(
                "canonical-supervisor-next-work".into(),
                created.team_run.id.clone(),
                created.team_run.agent_team_id.clone(),
                "Next canonical Work".into(),
                "Prove released execution does not wedge scheduling".into(),
                "The same Member receives one new exact admission".into(),
                WorkClaimMode::HostAssign,
                WorkPriority::Normal,
                compatibility_team_actor("host", "test"),
                "unix-ms:8".into(),
            )
            .into_work(),
            WorkCommandContext {
                event_id: "canonical-supervisor-next-work-created".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-next-work-create".into(),
                created_at: "unix-ms:8".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let next_work = store
        .assign_work_to_membership(
            &next_work.id,
            next_work.version,
            &membership.id,
            "unit-test-space",
            WorkCommandContext {
                event_id: "canonical-supervisor-next-work-assigned".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-next-work-assign".into(),
                created_at: "unix-ms:9".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let next_claim = claim_canonical_work_for_member(&ledger, &member)
        .expect("scheduler remains live after semantic Result")
        .expect("same Member receives next canonical Work");
    assert_eq!(next_claim.work.id, next_work.id);
    assert_eq!(
        store
            .fabric_work_deliveries("unit-test-space")
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.work_id == submitted.id)
            .count(),
        1,
        "completed Work is never replayed as a new delivery"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

struct AcceptanceBridge {
    inputs: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    terminal_sent: std::cell::Cell<bool>,
}

impl crate::codex_team_runtime::CodexAppServerBridge for AcceptanceBridge {
    fn ensure_transport_alive(&mut self) -> harness_provider_codex::CodexResult<()> {
        if self.terminal_sent.get() {
            Err(harness_provider_codex::CodexError::Usage(
                "acceptance fixture finished transport".into(),
            ))
        } else {
            Ok(())
        }
    }
    fn thread_id(&self) -> &str {
        "thread-acceptance-fixture"
    }
    fn start_turn(
        &mut self,
        text: &str,
        _: Duration,
    ) -> harness_provider_codex::CodexResult<String> {
        self.inputs.borrow_mut().push(text.into());
        Ok("turn-acceptance-fixture".into())
    }
    fn steer(&mut self, _: &str, _: &str) -> harness_provider_codex::CodexResult<String> {
        unreachable!()
    }
    fn interrupt(&mut self, _: &str) -> harness_provider_codex::CodexResult<()> {
        unreachable!()
    }
    fn recv(&self, _: Duration) -> Result<serde_json::Value, std::sync::mpsc::RecvTimeoutError> {
        self.terminal_sent.set(true);
        Ok(serde_json::json!({"method": "turn/completed", "params": {
            "threadId": "thread-acceptance-fixture", "turn": {"id": "turn-acceptance-fixture", "status": "completed",
                "items": [{"type": "agentMessage", "text": "Blocker remains; Work stays blocked."}]}
        }}))
    }
    fn read_thread(&mut self, _: bool) -> harness_provider_codex::CodexResult<serde_json::Value> {
        Ok(serde_json::json!({"id": self.thread_id(), "status": {"type": "idle"}, "turns": []}))
    }
    fn read_thread_goal(
        &mut self,
    ) -> harness_provider_codex::CodexResult<Option<serde_json::Value>> {
        Ok(None)
    }
    fn set_thread_goal_status(
        &mut self,
        _: &str,
    ) -> harness_provider_codex::CodexResult<serde_json::Value> {
        unreachable!()
    }
    fn shutdown_with_receipt(
        &mut self,
    ) -> harness_provider_codex::CodexResult<harness_provider_codex::CodexAppServerShutdownReceipt>
    {
        Ok(harness_provider_codex::CodexAppServerShutdownReceipt {
            process_was_running: true,
            process_reaped: true,
            stdout_reader_joined: true,
            thread_id_retained: true,
            exit_status: "fixture".into(),
        })
    }
}
