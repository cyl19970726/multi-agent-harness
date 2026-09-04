use super::*;

/// The reverse-RPC callback snapshot is frozen once at runtime attach and then
/// serves every later cycle of that runtime, so this table is the contract
/// deciding which later store write may fail a callback closed. Authority
/// fields must still fail closed; supervisor round bookkeeping and
/// same-generation refresh must pass. A newly added
/// `ProviderRuntimeProjection` field forces an explicit entry in one of the
/// two lists here.
#[test]
fn provider_callback_drift_separates_authority_from_round_bookkeeping() {
    type Mutate = fn(&mut ProviderRuntimeProjection);

    let authority: &[(&str, Mutate)] = &[
        ("id", |member| member.id = "member-other".into()),
        ("team_run_id", |member| {
            member.team_run_id = "team-other".into()
        }),
        ("slot_id", |member| member.slot_id = Some("slot-2".into())),
        ("agent_member_id", |member| {
            member.agent_member_id = "agent-other".into()
        }),
        ("role", |member| member.role = "host".into()),
        ("provider", |member| member.provider = "claude".into()),
        ("model", |member| member.model = Some("other-model".into())),
        ("provider_controls", |member| {
            member
                .provider_controls
                .model
                .mark_effective(Some("forced-model".into()), "test drift")
        }),
        ("coordination_status", |member| {
            member.coordination_status = MemberCoordinationStatus::Closed
        }),
        ("runtime_generation", |member| {
            member.runtime_generation += 1
        }),
        ("native_session_present", |member| {
            member.native_session = None
        }),
        ("native_session.native_session_id", |member| {
            member
                .native_session
                .as_mut()
                .expect("native session")
                .native_session_id = "replacement-session".into()
        }),
        ("native_session.availability", |member| {
            member
                .native_session
                .as_mut()
                .expect("native session")
                .availability = NativeSessionAvailability::Missing
        }),
        ("provider_cwd_hint", |member| {
            member.provider_cwd_hint = Some("/tmp/other-cwd".into())
        }),
        ("provider_environment_observation", |member| {
            member.provider_environment_observation = Some(MemberWorkspaceSnapshot {
                cwd: "/tmp/other-cwd".into(),
                project_binding_id: None,
                resolution_source: None,
                git_head: None,
                git_branch: None,
                instruction_roots: Vec::new(),
                skill_roots: Vec::new(),
            })
        }),
        ("owned_paths", |member| {
            member.owned_paths.push("crates/firm-cli".into())
        }),
    ];

    // Every entry below is rewritten by the Supervisor after an ordinary
    // settled round, or refreshed in the same generation by an operator or
    // probe path. Treating any of them as authority makes a full-access
    // member lose its reverse-RPC callbacks from the second cycle on.
    let bookkeeping: &[(&str, Mutate)] = &[
        ("zero_output_streak", |member| {
            member.zero_output_streak += 1
        }),
        ("last_consumed_work_version", |member| {
            member.last_consumed_work_version = Some(2)
        }),
        ("started_at", |member| {
            member.started_at = "unix-ms:restarted".into()
        }),
        ("finished_at_cleared", |member| member.finished_at = None),
        ("finished_at_set", |member| {
            member.finished_at = Some("unix-ms:round-end".into())
        }),
        ("status", |member| member.status = MemberRunStatus::Running),
        ("last_event_at", |member| {
            member.last_event_at = Some("unix-ms:round-end".into())
        }),
        ("native_session.last_verified_at", |member| {
            member
                .native_session
                .as_mut()
                .expect("native session")
                .last_verified_at = Some("unix-ms:after-first-turn".into())
        }),
        ("name", |member| member.name = "Renamed".into()),
    ];

    let base = {
        let mut member = native_open_test_member("kimi", "kimi_acp", "session-drift-table");
        member.finished_at = Some("unix-ms:previous-round".into());
        member
    };
    validate_provider_callback_drift(&base, &base).expect("an unchanged row is never drift");

    for (field, mutate) in authority {
        let mut latest = base.clone();
        mutate(&mut latest);
        assert_ne!(latest, base, "{field} mutation did not change the row");
        let Err(error) = validate_provider_callback_drift(&base, &latest) else {
            panic!("authority field {field} must fail the callback closed");
        };
        assert!(
            error.to_string().contains("crossed identity"),
            "unexpected {field} error: {error}"
        );
    }

    for (field, mutate) in bookkeeping {
        let mut latest = base.clone();
        mutate(&mut latest);
        assert_ne!(latest, base, "{field} mutation did not change the row");
        validate_provider_callback_drift(&base, &latest).unwrap_or_else(|error| {
            panic!("round bookkeeping field {field} must not fail closed: {error}")
        });
    }
}
