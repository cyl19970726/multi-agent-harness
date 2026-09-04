use super::*;

fn make_member(
    status: MemberRunStatus,
    coordination_status: MemberCoordinationStatus,
    has_session: bool,
    session_supports_resume: bool,
    session_availability: NativeSessionAvailability,
    is_external: bool,
) -> ProviderRuntimeProjection {
    let execution_mode = if is_external {
        EXECUTION_MODE_EXTERNAL_INTERACTIVE.to_string()
    } else {
        "codex_app_server".to_string()
    };
    ProviderRuntimeProjection {
        id: "mr-test".into(),
        team_run_id: "tr-test".into(),
        slot_id: Some("slot-test".into()),
        agent_member_id: "agent-test".into(),
        name: "test-member".into(),
        role: "builder".into(),
        provider: "codex".into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: if has_session || is_external {
            Some(ProviderIntegrationProfile {
                agent_runtime_provider: Some(harness_core::AgentRuntimeProvider("codex".into())),
                model_route: None,
                provider: "codex".into(),
                execution_mode: execution_mode.clone(),
                execution_driver: if is_external {
                    MemberExecutionDriver::UserDriven
                } else {
                    MemberExecutionDriver::default()
                },
                provider_version: None,
                adapter_contract_version: None,
                reviewed_provider_versions: Vec::new(),
                compatibility_status: ProviderCompatibilityStatus::Current,
                adapter_reviewed_at: None,
                compatibility_note: None,
                interaction_mode: ProviderInteractionMode::PauseAndResume,
                ordinary_message_boundary: OrdinaryMessageBoundary::Unknown,
                plan_mode: ProviderFeatureMode::Unknown,
                goal_mode: ProviderFeatureMode::Unknown,
                tool_event_fidelity: ProviderEventFidelity::Structured,
                artifact_event_fidelity: ProviderEventFidelity::Structured,
                supports_cancel: true,
                supports_resume: session_supports_resume,
                observes_native_subagents: false,
                observes_background_tasks: false,
                thinking_transient_only: true,
                control_topology: ControlTopology::default(),
                composition_fingerprint: None,
                capability_fingerprint: None,
                capability_bindings: Vec::new(),
                binding_admission: harness_core::ProviderBindingAdmission::Failed,
                adapter_bridge_revision: None,
                security_enforcement_locus: SecurityEnforcementLocus::default(),
            })
        } else {
            None
        },
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status,
        runtime_generation: 1,
        status,
        native_session: if has_session {
            Some(NativeSessionRef {
                provider: "codex".into(),
                execution_mode: execution_mode.clone(),
                native_session_id: "ns-1".into(),
                native_locator_kind: "codex_sqlite".into(),
                provider_version: None,
                adapter_contract_version: "1.0".into(),
                availability: session_availability,
                supports_resume: session_supports_resume,
                last_verified_at: None,
                parent_native_session_id: None,
            })
        } else {
            None
        },
        provider_cwd_hint: None,
        provider_environment_observation: None,
        owned_paths: Vec::new(),
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    }
}

#[test]
fn active_with_supervisor_returns_already_active() {
    let member = make_member(
        MemberRunStatus::Running,
        MemberCoordinationStatus::Active,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    assert_eq!(
        classify_member_recovery_path(&member, true, false),
        MemberRecoveryPath::AlreadyActive
    );
}

#[test]
fn compatible_session_returns_resume() {
    let member = make_member(
        MemberRunStatus::Stopped,
        MemberCoordinationStatus::Closed,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    assert_eq!(
        classify_member_recovery_path(&member, false, false),
        MemberRecoveryPath::ResumeCompatible
    );
}

#[test]
fn incompatible_session_returns_rebind() {
    let member = make_member(
        MemberRunStatus::Stopped,
        MemberCoordinationStatus::Closed,
        true,
        false,
        NativeSessionAvailability::Incompatible,
        false,
    );
    let result = classify_member_recovery_path(&member, false, false);
    assert!(
        matches!(result, MemberRecoveryPath::RebindIncompatible { .. }),
        "expected RebindIncompatible, got {:?}",
        result
    );
}

#[test]
fn missing_session_returns_rebind() {
    let member = make_member(
        MemberRunStatus::Stopped,
        MemberCoordinationStatus::Closed,
        false,
        false,
        NativeSessionAvailability::Missing,
        false,
    );
    let result = classify_member_recovery_path(&member, false, false);
    assert!(
        matches!(result, MemberRecoveryPath::RebindIncompatible { .. }),
        "expected RebindIncompatible, got {:?}",
        result
    );
}

#[test]
fn retired_member_returns_terminal() {
    let member = make_member(
        MemberRunStatus::Stopped,
        MemberCoordinationStatus::Retired,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    let result = classify_member_recovery_path(&member, false, false);
    assert!(
        matches!(result, MemberRecoveryPath::Terminal { .. }),
        "expected Terminal, got {:?}",
        result
    );
}

#[test]
fn external_interactive_always_resume() {
    let member = make_member(
        MemberRunStatus::Stopped,
        MemberCoordinationStatus::Closed,
        false,
        false,
        NativeSessionAvailability::Missing,
        true, // external interactive
    );
    assert_eq!(
        classify_member_recovery_path(&member, false, false),
        MemberRecoveryPath::ResumeCompatible
    );
}

#[test]
fn already_active_member_no_supervisor_remains_already_active() {
    // Active coordination but no supervisor lease: still AlreadyActive.
    let member = make_member(
        MemberRunStatus::Running,
        MemberCoordinationStatus::Active,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    assert_eq!(
        classify_member_recovery_path(&member, false, false),
        MemberRecoveryPath::AlreadyActive
    );
}

/// A Blocked, actively-coordinated member with an available native session —
/// the shape every #779 recovery decision is made on — with one piece of typed
/// block provenance applied.
fn blocked_member_with(
    mutate: impl FnOnce(&mut ProviderRuntimeProjection),
) -> ProviderRuntimeProjection {
    let mut member = make_member(
        MemberRunStatus::Blocked,
        MemberCoordinationStatus::Active,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    mutate(&mut member);
    member
}

fn capacity_snapshot(state: harness_core::ProviderCapacityState) -> ProviderCapacitySnapshot {
    let mut snapshot = ProviderCapacitySnapshot::unknown(
        "codex",
        "codex_app_server",
        "unix-ms:1",
        1,
        harness_core::ProviderCapacityEvidence::ProviderError,
        "test capacity observation",
    );
    snapshot.state = state;
    snapshot
}

fn compatibility_cause() -> harness_core::ProviderCompatibilityBlockCause {
    harness_core::ProviderCompatibilityBlockCause {
        schema_version: 1,
        id: "provider-compatibility-block:mr-test:1".into(),
        member_run_id: "mr-test".into(),
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        provider_version: "0.0.0-test".into(),
        adapter_contract_version: "1.0".into(),
        boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        source: harness_core::ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: "unix-ms:1".into(),
    }
}

/// #779 review P1-1: `Blocked` alone names no authority. Three gates write it
/// and leave typed evidence that owns their own recovery; only a runtime fence
/// leaves none. Reading the status alone would let `recover` clear a live
/// diagnosis — and, for the compatibility class, produce a row
/// `ProviderRuntimeProjection::validate` refuses, aborting the whole run.
#[test]
fn block_provenance_names_the_gate_that_owns_each_block() {
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|_| {})),
        BlockedMemberProvenance::Untyped,
        "a member a runtime fence blocked carries no typed provenance"
    );
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.provider_compatibility_block_cause = Some(compatibility_cause());
        })),
        BlockedMemberProvenance::ProviderCompatibility
    );
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.provider_capacity = Some(capacity_snapshot(
                harness_core::ProviderCapacityState::Exhausted,
            ));
        })),
        BlockedMemberProvenance::ProviderCapacity
    );
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.provider_capacity = Some(capacity_snapshot(
                harness_core::ProviderCapacityState::Unauthorized,
            ));
        })),
        BlockedMemberProvenance::ProviderCapacity
    );
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.provider_capacity = Some(capacity_snapshot(
                harness_core::ProviderCapacityState::Available,
            ));
        })),
        BlockedMemberProvenance::Untyped,
        "only a known-unavailable capacity snapshot is a capacity-authored block"
    );
    // The wake loop degrades at the threshold, and deliberately keeps waking a
    // member below it, so a streak under it is ordinary bounded probation. If
    // that read as a degradation verdict, a drain-fenced member with a streak
    // of 1 would be refused by recover, refused by the start claim for being
    // Blocked, and could never reset its streak — #779's wedge, recreated for
    // exactly the members recover exists to repair.
    let threshold = supervisor_wake::WakePolicy::default().zero_output_degradation_threshold;
    assert!(
        threshold > 1,
        "a threshold of 1 would make the probation window untestable"
    );
    for streak in 1..threshold {
        assert_eq!(
            blocked_member_provenance(&blocked_member_with(|member| {
                member.zero_output_streak = streak;
            })),
            BlockedMemberProvenance::Untyped,
            "a streak of {streak} is bounded probation, below the degradation threshold {threshold}"
        );
    }
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.zero_output_streak = threshold;
        })),
        BlockedMemberProvenance::ZeroOutputDegradation,
        "the degradation verdict starts exactly at the wake loop's own threshold"
    );
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.zero_output_streak = threshold + 1;
        })),
        BlockedMemberProvenance::ZeroOutputDegradation
    );
    assert_eq!(
        blocked_member_provenance(&blocked_member_with(|member| {
            member.provider_compatibility_block_cause = Some(compatibility_cause());
            member.provider_capacity = Some(capacity_snapshot(
                harness_core::ProviderCapacityState::Exhausted,
            ));
            member.zero_output_streak = threshold;
        })),
        BlockedMemberProvenance::ProviderCompatibility,
        "the typed cause validation binds to Blocked decides when several apply"
    );
}

/// Each excluded class is reported, never restarted — even on a lane that
/// proves the runtime gone, which is exactly the state a compatibility block
/// leaves behind (it is written from Idle|Queued|Disconnected, so the Session
/// is Cold, detached and disarmed).
#[test]
fn a_block_with_typed_provenance_is_reported_and_never_restarted() {
    for (label, member) in [
        (
            "provider compatibility",
            blocked_member_with(|member| {
                member.provider_compatibility_block_cause = Some(compatibility_cause());
            }),
        ),
        (
            "provider capacity",
            blocked_member_with(|member| {
                member.provider_capacity = Some(capacity_snapshot(
                    harness_core::ProviderCapacityState::Exhausted,
                ));
            }),
        ),
        (
            "zero-output degradation",
            blocked_member_with(|member| {
                member.zero_output_streak =
                    supervisor_wake::WakePolicy::default().zero_output_degradation_threshold;
            }),
        ),
    ] {
        let path = classify_member_recovery_path(&member, false, true);
        let MemberRecoveryPath::BlockedByTypedProvenance { provenance } = path else {
            panic!("{label} must be reported, not restarted; got {path:?}");
        };
        assert_ne!(provenance, BlockedMemberProvenance::Untyped);
        assert!(
            !provenance.reason().is_empty(),
            "{label} must name why it was not restarted"
        );
        assert_eq!(
            classify_member_recovery_path(&member, false, false),
            MemberRecoveryPath::BlockedByTypedProvenance { provenance },
            "{label} is decided by its provenance, not by the lane"
        );
    }
}

/// #779: a Blocked member whose lane already proves the runtime is gone is the
/// one case `recover` must act on rather than skip — and only that case.
#[test]
fn blocked_member_on_a_dead_lane_is_restarted_and_nothing_else_is() {
    let blocked = blocked_member_with(|_| {});
    assert_eq!(
        classify_member_recovery_path(&blocked, false, true),
        MemberRecoveryPath::RestartBlockedDetachedLane
    );
    assert_eq!(
        classify_member_recovery_path(&blocked, true, true),
        MemberRecoveryPath::RestartBlockedDetachedLane,
        "the lane's own state is the proof; a live Supervisor lease is not required"
    );
    assert_eq!(
        classify_member_recovery_path(&blocked, false, false),
        MemberRecoveryPath::AlreadyActive,
        "a lane that does not prove the runtime gone keeps its block"
    );

    let running = make_member(
        MemberRunStatus::Running,
        MemberCoordinationStatus::Active,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    assert_eq!(
        classify_member_recovery_path(&running, false, true),
        MemberRecoveryPath::AlreadyActive,
        "only a Blocked member is restarted; recover never re-labels healthy work"
    );

    let external = make_member(
        MemberRunStatus::Blocked,
        MemberCoordinationStatus::Active,
        false,
        false,
        NativeSessionAvailability::Missing,
        true,
    );
    assert_eq!(
        classify_member_recovery_path(&external, false, true),
        MemberRecoveryPath::AlreadyActive,
        "an external interactive member owns no Harness-driven lane to restart"
    );
}

#[test]
fn completed_member_no_supervisor_returns_terminal() {
    let member = make_member(
        MemberRunStatus::Completed,
        MemberCoordinationStatus::Closed,
        true,
        true,
        NativeSessionAvailability::Available,
        false,
    );
    let result = classify_member_recovery_path(&member, false, false);
    assert!(
        matches!(result, MemberRecoveryPath::Terminal { .. }),
        "expected Terminal, got {:?}",
        result
    );
}

// ── supervisor lease liveness helpers ─────────────────────────

fn make_lease(
    status: harness_core::TeamSupervisorLeaseStatus,
    expires_ms: u64,
    pid: u32,
) -> harness_core::TeamSupervisorLease {
    harness_core::TeamSupervisorLease {
        team_run_id: "tr-test".into(),
        node_id: "node-test".into(),
        node_daemon_id: "daemon-test".into(),
        node_daemon_generation: 1,
        execution_space_id: "space-test".into(),
        project_binding_id: "project-test".into(),
        supervisor_id: "sv-test".into(),
        generation: 1,
        owner_process_id: pid,
        owner_locator: "test".into(),
        status,
        acquired_unix_ms: 1,
        heartbeat_unix_ms: 1,
        expires_unix_ms: expires_ms,
        released_unix_ms: None,
    }
}

#[test]
fn diagnosis_live_lease() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Active,
        now + 60_000,
        std::process::id(),
    );
    let (live, diagnosis) = supervisor_lease_live_diagnosis(&lease);
    assert!(live, "expected live, got diagnosis: {diagnosis}");
    assert_eq!(diagnosis, "live");
}

#[test]
fn diagnosis_expired_lease() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Active,
        now.saturating_sub(1), // expired
        std::process::id(),
    );
    let (live, diagnosis) = supervisor_lease_live_diagnosis(&lease);
    assert!(!live, "expected not live");
    assert!(
        diagnosis.contains("expired"),
        "diagnosis should mention expired: {diagnosis}"
    );
}

#[test]
fn diagnosis_released_status() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Released,
        now + 60_000,
        std::process::id(),
    );
    let (live, diagnosis) = supervisor_lease_live_diagnosis(&lease);
    assert!(!live, "expected not live for Released status");
    assert!(
        diagnosis.contains("released"),
        "diagnosis should mention released: {diagnosis}"
    );
}

#[test]
fn diagnosis_dead_pid() {
    let now = current_unix_ms_u64();
    // PID 0 is treated as dead by pid_exists_libc
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Active,
        now + 60_000,
        0,
    );
    let (live, diagnosis) = supervisor_lease_live_diagnosis(&lease);
    assert!(!live, "expected not live for PID 0");
    assert!(
        diagnosis.contains("PID"),
        "diagnosis should mention PID: {diagnosis}"
    );
}

#[test]
fn diagnosis_multiple_failures() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Released,
        now.saturating_sub(1), // expired
        0,                     // dead PID
    );
    let (live, diagnosis) = supervisor_lease_live_diagnosis(&lease);
    assert!(!live);
    // Should mention all three failures
    assert!(
        diagnosis.contains("released"),
        "missing status: {diagnosis}"
    );
    assert!(
        diagnosis.contains("expired"),
        "missing expired: {diagnosis}"
    );
    assert!(diagnosis.contains("PID"), "missing PID: {diagnosis}");
}

#[test]
fn is_supervisor_current_live_process() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Active,
        now + 60_000,
        std::process::id(),
    );
    assert!(is_supervisor_current(&lease));
}

#[test]
fn is_supervisor_current_expired() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Active,
        now.saturating_sub(1),
        std::process::id(),
    );
    assert!(!is_supervisor_current(&lease));
}

#[test]
fn is_supervisor_current_released() {
    let now = current_unix_ms_u64();
    let lease = make_lease(
        harness_core::TeamSupervisorLeaseStatus::Released,
        now + 60_000,
        std::process::id(),
    );
    assert!(!is_supervisor_current(&lease));
}
