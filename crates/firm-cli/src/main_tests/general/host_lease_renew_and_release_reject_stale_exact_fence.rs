use super::*;

    #[test]
    fn host_lease_renew_and_release_reject_stale_exact_fence() {
        let (store, root) = temp_store("host-lease-stale-fence");
        let created = create_two_member_team_run(&store);
        let current = latest_team_run(&store, &created.team_run.id).expect("run");
        let mut bound = current.clone();
        bound.host_surface = "codex".into();
        bound.host_thread_id = Some("thread-1".into());
        bound.updated_at = "unix-ms:2".into();
        store
            .compare_and_append_team_run(&current, &bound)
            .expect("bind");
        let stale = store
            .acquire_host_binding_lease(
                &bound.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Interactive,
                "owner-1",
                "lease-1",
                100,
                10,
            )
            .expect("first lease");
        let current = store
            .acquire_host_binding_lease(
                &bound.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Interactive,
                "owner-2",
                "lease-2",
                111,
                100,
            )
            .expect("takeover");
        assert!(store.renew_host_binding_lease(&stale, 112, 100).is_err());
        assert!(store.release_host_binding_lease(&stale, 112).is_err());
        assert!(store.renew_host_binding_lease(&current, 112, 100).is_ok());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

