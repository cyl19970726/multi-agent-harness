use super::*;

    #[test]
    fn capacity_recovery_clears_only_capacity_origin_blocked_projection() {
        let (store, root) = temp_store("capacity-recovery-provenance");
        let created = create_two_member_team_run(&store);
        let base = created.member_runs[0].clone();

        let mut without_session = base.clone();
        without_session.status = MemberRunStatus::Blocked;
        without_session.finished_at = Some("unix-ms:90".into());
        without_session.provider_capacity =
            Some(capacity_test_snapshot(ProviderCapacityState::Exhausted));
        apply_nonblocking_capacity_observation(
            &mut without_session,
            capacity_test_snapshot(ProviderCapacityState::Available),
        );
        assert_eq!(without_session.status, MemberRunStatus::Idle);
        assert!(without_session.finished_at.is_none());
        assert_eq!(
            without_session.provider_capacity.as_ref().unwrap().state,
            ProviderCapacityState::Available
        );

        let mut with_session = base.clone();
        with_session.status = MemberRunStatus::Blocked;
        with_session.native_session = Some(capacity_test_session());
        with_session.provider_capacity =
            Some(capacity_test_snapshot(ProviderCapacityState::Unauthorized));
        apply_nonblocking_capacity_observation(
            &mut with_session,
            capacity_test_snapshot(ProviderCapacityState::Available),
        );
        assert_eq!(with_session.status, MemberRunStatus::Disconnected);

        let mut unrelated_block = base.clone();
        unrelated_block.status = MemberRunStatus::Blocked;
        unrelated_block.provider_capacity =
            Some(capacity_test_snapshot(ProviderCapacityState::Unknown));
        apply_nonblocking_capacity_observation(
            &mut unrelated_block,
            capacity_test_snapshot(ProviderCapacityState::Available),
        );
        assert_eq!(unrelated_block.status, MemberRunStatus::Blocked);

        let mut closed_capacity_block = base;
        closed_capacity_block.status = MemberRunStatus::Blocked;
        closed_capacity_block.coordination_status = MemberCoordinationStatus::Closed;
        closed_capacity_block.provider_capacity =
            Some(capacity_test_snapshot(ProviderCapacityState::Exhausted));
        apply_nonblocking_capacity_observation(
            &mut closed_capacity_block,
            capacity_test_snapshot(ProviderCapacityState::Available),
        );
        assert_eq!(closed_capacity_block.status, MemberRunStatus::Blocked);
        let _ = std::fs::remove_dir_all(root);
    }

