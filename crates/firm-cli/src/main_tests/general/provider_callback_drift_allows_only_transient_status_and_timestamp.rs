use super::*;

    #[test]
    fn provider_callback_drift_allows_only_transient_status_and_timestamp() {
        let mut round_start = native_open_test_member("kimi", "kimi_acp", "session-cas-fence");
        round_start.status = MemberRunStatus::Running;
        let mut waiting = round_start.clone();
        waiting.status = MemberRunStatus::Waiting;
        waiting.last_event_at = Some("unix-ms:2".into());
        validate_provider_callback_drift(&round_start, &waiting)
            .expect("interaction-owned transient drift is legal");

        let mut safe_same_generation = waiting.clone();
        safe_same_generation.name = "OperatorRename".into();
        safe_same_generation.provider_profile = Some(team_member_provider_profile_for_mode(
            "kimi",
            Some("kimi_acp"),
        ));
        validate_provider_callback_drift(&round_start, &safe_same_generation)
            .expect("same-generation rename and profile refresh are rebased");

        let mut refreshed_native_observation = waiting.clone();
        refreshed_native_observation
            .native_session
            .as_mut()
            .expect("native session")
            .last_verified_at = Some("unix-ms:later-provider-observation".into());
        validate_provider_callback_drift(&round_start, &refreshed_native_observation)
            .expect("native-session verification timestamp is observation, not authority drift");

        let mut unavailable_native_session = refreshed_native_observation;
        unavailable_native_session
            .native_session
            .as_mut()
            .expect("native session")
            .availability = NativeSessionAvailability::Missing;
        assert!(
            validate_provider_callback_drift(&round_start, &unavailable_native_session).is_err(),
            "native-session availability remains authority/provenance drift"
        );

        let mut changed_controls = waiting.clone();
        changed_controls.provider_controls.model.requested = Some("other-model".into());
        assert!(validate_provider_callback_drift(&round_start, &changed_controls).is_err());

        let mut replaced_generation = waiting.clone();
        replaced_generation.runtime_generation += 1;
        assert!(validate_provider_callback_drift(&round_start, &replaced_generation).is_err());

        let mut closed = waiting;
        closed.coordination_status = MemberCoordinationStatus::Closed;
        closed.status = MemberRunStatus::Stopped;
        assert!(validate_provider_callback_drift(&round_start, &closed).is_err());
    }

