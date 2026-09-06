use super::*;

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let unique = format!(
            "firm-daemon-recover-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test clock")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const RECOVER_TEST_NODE_ID: &str = "33333333-3333-4333-8333-333333333333";

fn seed_recover_test_node(tag: &str) -> (TestDir, PathBuf, HarnessStore) {
    let tree = TestDir::new(tag);
    let firm_home = tree.path().join("home");
    let space = crate::execution_space::register_and_activate(
        &firm_home,
        "space-recover",
        "Space recover",
        None,
        None,
        "unix-ms:1",
    )
    .expect("register recover test Execution Space");
    let store = HarnessStore::new(space.store_root.clone());
    store.init().expect("initialize recover test Store");
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: RECOVER_TEST_NODE_ID.into(),
            display_name: "Recover Test Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert recover test Node");
    (tree, firm_home, store)
}

fn recover_args(confirm: bool) -> Vec<String> {
    let mut args = vec!["recover-predecessor".to_string()];
    if confirm {
        args.push("--confirm".to_string());
        args.push("daemon-recover-predecessor".to_string());
    }
    args
}

#[test]
fn recover_predecessor_refuses_without_confirm_and_without_predecessor() {
    let (_tree, firm_home, _store) = seed_recover_test_node("refusals");
    let missing_confirm =
        daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(false))
            .expect_err("recovery without --confirm must be refused");
    assert!(
        missing_confirm
            .to_string()
            .contains("--confirm daemon-recover-predecessor"),
        "{missing_confirm}"
    );

    let no_predecessor =
        daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(true))
            .expect_err("recovery without a predecessor lease must be refused");
    assert!(
        no_predecessor.to_string().contains("no predecessor"),
        "{no_predecessor}"
    );
}

#[test]
fn recover_predecessor_releases_dead_instance_and_is_idempotent() {
    let (_tree, firm_home, store) = seed_recover_test_node("release");
    let dead_instance_id = format!("2147483647:{}:dead-daemon", current_unix_ms_u64());
    let dead_lease = store
        .acquire_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "dead-daemon",
            &dead_instance_id,
            current_unix_ms_u64(),
            1,
        )
        .expect("seed dead-instance predecessor lease");
    std::thread::sleep(Duration::from_millis(5));

    let projection =
        daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(true))
            .expect("dead predecessor recovery succeeds");
    assert_eq!(projection["status"], "released");
    assert_eq!(projection["daemon_id"], "dead-daemon");
    assert_eq!(projection["instance_id"], dead_instance_id.as_str());
    assert_eq!(projection["generation"], dead_lease.generation);
    assert_eq!(
        projection["recovered_spaces"],
        serde_json::json!(["space-recover"])
    );
    // The receipt names what each Space settled, including the Sessions
    // recovery skipped because they were already settled (#837).
    assert_eq!(
        projection["space_settlements"],
        serde_json::json!([{
            "execution_space_id": "space-recover",
            "already_released": false,
            "supervisors_released": [],
            "sessions_detached": [],
            "sessions_already_settled": [],
        }])
    );
    assert_eq!(
        store
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .expect("recovered lease")
            .expect("lease row")
            .status,
        NodeDaemonLeaseStatus::Released
    );

    let second = daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(true))
        .expect("second run reports the already released predecessor");
    assert_eq!(second["already_released"], true);
    assert_eq!(second["status"], "released");
    assert_eq!(second["generation"], dead_lease.generation);
}

#[test]
fn recover_predecessor_partial_failure_preserves_releases_and_retry_markers() {
    let (_tree, firm_home, store) = seed_recover_test_node("partial-receipt");
    let second_space = crate::execution_space::register_and_activate(
        &firm_home,
        "space-second",
        "Second space",
        None,
        None,
        "unix-ms:1",
    )
    .expect("register second space");
    let second = HarnessStore::new(second_space.store_root);
    second.init().expect("initialize second store");
    second
        .insert_execution_node(&harness_core::ExecutionNode {
            id: RECOVER_TEST_NODE_ID.into(),
            display_name: "Recover Test Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert node in second space");
    let instance = "2147483647:partial:dead-daemon";
    let lease = store
        .acquire_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 1, 1)
        .expect("seed expired predecessor in first space");

    // The second space belongs to this Node but has no recoverable lease.
    // It must not hide the first space's irreversible release in the CLI error.
    let error = daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(true))
        .expect_err("one space fails while another releases");
    let detail = error.to_string();
    assert!(detail.contains("NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE"));
    let receipt: serde_json::Value =
        serde_json::from_str(&detail[detail.find('{').expect("JSON partial receipt")..])
            .expect("CLI preserves structured partial receipt");
    assert_eq!(receipt["status"], "partial");
    assert_eq!(
        receipt["recovered_spaces"],
        serde_json::json!(["space-recover"])
    );
    assert_eq!(receipt["space_settlements"][0]["already_released"], false);
    assert!(receipt["failures"][0]
        .as_str()
        .unwrap()
        .contains("space-second"));
    assert_eq!(
        store
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
            .status,
        NodeDaemonLeaseStatus::Released
    );

    // Repair only the failed fixture, then use the same seam as the HTTP
    // action. An empty repeat settlement is now explicitly distinguished.
    second
        .acquire_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 1, 1)
        .expect("seed second space predecessor");
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: RECOVER_TEST_NODE_ID.into(),
    };
    let repeated = recover_daemon_predecessor_spaces(
        &firm_home,
        RECOVER_TEST_NODE_ID,
        &PredecessorRecoveryIntent {
            daemon_id: "dead-daemon".into(),
            instance_id: instance.into(),
            generation: lease.generation,
        },
        &actor,
        true,
        "test:second-request-evidence",
        "test:partial-retry",
        None,
    )
    .expect("retry preserves already released space and recovers second");
    assert_eq!(repeated["status"], "released");
    let settlements = repeated["space_settlements"].as_array().unwrap();
    assert_eq!(settlements.len(), 2);
    for row in settlements {
        assert_eq!(
            row["already_released"],
            row["execution_space_id"] == "space-recover"
        );
    }
}

#[test]
fn recover_predecessor_refuses_a_live_predecessor_process() {
    let (_tree, firm_home, store) = seed_recover_test_node("live-pid");
    let live_instance_id = format!(
        "{}:{}:live-daemon",
        std::process::id(),
        current_unix_ms_u64()
    );
    store
        .acquire_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "live-daemon",
            &live_instance_id,
            current_unix_ms_u64(),
            1,
        )
        .expect("seed live-pid predecessor lease");
    std::thread::sleep(Duration::from_millis(5));

    let live = daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(true))
        .expect_err("a predecessor whose pid is still alive must be refused");
    assert!(
        live.to_string()
            .contains("predecessor process still exists"),
        "{live}"
    );
}

#[test]
fn recover_predecessor_refuses_an_unexpired_lease_naming_the_expiry() {
    let (_tree, firm_home, store) = seed_recover_test_node("unexpired");
    let dead_instance_id = format!("2147483647:{}:dead-daemon", current_unix_ms_u64());
    let lease = store
        .acquire_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "dead-daemon",
            &dead_instance_id,
            current_unix_ms_u64(),
            3_600_000,
        )
        .expect("seed unexpired dead-instance predecessor lease");

    let refusal = daemon_recover_predecessor(&firm_home, RECOVER_TEST_NODE_ID, &recover_args(true))
        .expect_err("an unexpired predecessor lease must be refused before the store");
    let message = refusal.to_string();
    assert!(message.contains("has not expired"), "{message}");
    assert!(
        message.contains(&format!("expires unix-ms:{}", lease.expires_unix_ms)),
        "{message}"
    );
    assert!(message.contains("retry after expiry"), "{message}");
}

#[test]
fn absent_status_names_each_predecessor_lease_expiry() {
    let log_path = Path::new("node-daemon.log");

    let (_unexpired_tree, unexpired_home, unexpired_store) =
        seed_recover_test_node("status-unexpired");
    let lease = unexpired_store
        .acquire_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "unexpired-daemon",
            &format!("2147483647:{}:unexpired-daemon", current_unix_ms_u64()),
            current_unix_ms_u64(),
            3_600_000,
        )
        .expect("seed unexpired predecessor lease");
    let status = daemon_absent_status(&unexpired_home, RECOVER_TEST_NODE_ID, log_path)
        .expect("absent status with an unexpired lease");
    assert!(
        status.contains(&format!(
            "expires unix-ms:{} (expires in",
            lease.expires_unix_ms
        )),
        "{status}"
    );

    let (_expired_tree, expired_home, expired_store) = seed_recover_test_node("status-expired");
    let expired_lease = expired_store
        .acquire_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "expired-daemon",
            &format!("2147483647:{}:expired-daemon", current_unix_ms_u64()),
            current_unix_ms_u64(),
            1,
        )
        .expect("seed expired predecessor lease");
    std::thread::sleep(Duration::from_millis(5));
    let status = daemon_absent_status(&expired_home, RECOVER_TEST_NODE_ID, log_path)
        .expect("absent status with an expired lease");
    assert!(
        status.contains(&format!(
            "expires unix-ms:{} (expired)",
            expired_lease.expires_unix_ms
        )),
        "{status}"
    );
}
