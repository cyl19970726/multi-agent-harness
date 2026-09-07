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
            "generation": dead_lease.generation,
            "daemon_id": "dead-daemon",
            "instance_id": dead_instance_id,
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
    let _lease = store
        .acquire_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 1, 1)
        .expect("seed expired predecessor in first space");

    // Change only the second Space after capture. Its old tuple is now
    // deterministically fenced, independent of runner speed or wall-clock TTL.
    let second_lease = second
        .acquire_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 1, 1)
        .unwrap();
    let intent =
        validate_daemon_predecessor_recovery(&firm_home, RECOVER_TEST_NODE_ID, None).unwrap();
    second
        .release_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "dead-daemon",
            second_lease.generation,
            instance,
            3,
        )
        .unwrap();
    let successor = second
        .acquire_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 4, 1)
        .unwrap();
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: RECOVER_TEST_NODE_ID.into(),
    };
    let error = recover_daemon_predecessor_spaces(
        &firm_home,
        RECOVER_TEST_NODE_ID,
        &intent,
        &actor,
        true,
        "test:first",
        "test:partial",
        None,
    )
    .unwrap_err();
    let receipt: serde_json::Value = serde_json::from_str(&error.1).unwrap();
    assert_eq!(receipt["status"], "partial");
    assert_eq!(receipt["space_settlements"][0]["already_released"], false);
    assert!(receipt["failures"][0]
        .as_str()
        .unwrap()
        .contains("NODE_DAEMON_GENERATION_FENCED"));
    assert_eq!(
        second
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap(),
        successor
    );
    // A new request captures the changed same-instance local generation,
    // retaining the successful first settlement as an idempotent skip.
    let retry_intent =
        validate_daemon_predecessor_recovery(&firm_home, RECOVER_TEST_NODE_ID, None).unwrap();
    let repeated = recover_daemon_predecessor_spaces(
        &firm_home,
        RECOVER_TEST_NODE_ID,
        &retry_intent,
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

fn second_recovery_store(home: &Path) -> HarnessStore {
    let space = execution_space::register_and_activate(
        home,
        "space-second",
        "Second",
        None,
        None,
        "unix-ms:1",
    )
    .unwrap();
    let store = HarnessStore::new(space.store_root);
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: RECOVER_TEST_NODE_ID.into(),
            display_name: "Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();
    store
}

fn seed_local_generation(store: &HarnessStore, generation: u64, instance: &str) {
    for index in 1..=generation {
        let lease = store
            .acquire_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", instance, index * 2, 1)
            .unwrap();
        assert_eq!(lease.generation, index);
        if index < generation {
            store
                .release_node_daemon_lease(
                    RECOVER_TEST_NODE_ID,
                    "dead-daemon",
                    index,
                    instance,
                    index * 2 + 1,
                )
                .unwrap();
        }
    }
}

fn recover_captured(
    home: &Path,
    intent: &PredecessorRecoveryIntent,
) -> Result<serde_json::Value, (String, String)> {
    recover_daemon_predecessor_spaces(
        home,
        RECOVER_TEST_NODE_ID,
        intent,
        &harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: RECOVER_TEST_NODE_ID.into(),
        },
        true,
        "test:captured",
        "test:captured",
        None,
    )
}

#[test]
fn recovery_uses_space_local_generations_and_http_never_expands_its_tuple() {
    let (_tree, home, first) = seed_recover_test_node("local-generations");
    let second = second_recovery_store(&home);
    let instance = "2147483647:local:dead-daemon";
    seed_local_generation(&first, 4, instance);
    seed_local_generation(&second, 26, instance);
    // Existing HTTP intent can recover generation 4 despite a higher local
    // counter elsewhere, but it cannot authorize generation 26 implicitly.
    let http = validate_daemon_predecessor_recovery(
        &home,
        RECOVER_TEST_NODE_ID,
        Some(("dead-daemon", instance, 4)),
    )
    .unwrap();
    assert_eq!(http.spaces.len(), 1);
    let result = recover_captured(&home, &http).unwrap();
    assert_eq!(result["space_settlements"][0]["generation"], 4);
    assert_ne!(
        second
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
            .status,
        NodeDaemonLeaseStatus::Released
    );
    // CLI retry includes the already released exact predecessor and remaining
    // generation 26, each passed independently to the Store.
    let result =
        daemon_recover_predecessor(&home, RECOVER_TEST_NODE_ID, &recover_args(true)).unwrap();
    assert!(
        result["generation"].is_null(),
        "no fabricated machine-wide generation"
    );
    let rows = result["space_settlements"].as_array().unwrap();
    assert!(rows
        .iter()
        .any(|row| row["generation"] == 4 && row["already_released"] == true));
    assert!(rows
        .iter()
        .any(|row| row["generation"] == 26 && row["already_released"] == false));
    assert_eq!(
        daemon_recover_predecessor(&home, RECOVER_TEST_NODE_ID, &recover_args(true)).unwrap()
            ["already_released"],
        true
    );
}

#[test]
fn recovery_refuses_same_generation_foreign_instance_and_fences_successor() {
    let (_tree, home, first) = seed_recover_test_node("foreign-instance");
    let second = second_recovery_store(&home);
    let instance = "2147483647:first:dead-daemon";
    seed_local_generation(&first, 1, instance);
    seed_local_generation(&second, 1, "2147483647:foreign:dead-daemon");
    assert!(
        validate_daemon_predecessor_recovery(&home, RECOVER_TEST_NODE_ID, None)
            .err()
            .unwrap()
            .1
            .contains("different unreleased")
    );
    assert_ne!(
        first
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
            .status,
        NodeDaemonLeaseStatus::Released
    );

    let (_tree, home, store) = seed_recover_test_node("successor-after-validate");
    seed_local_generation(&store, 1, instance);
    let intent = validate_daemon_predecessor_recovery(&home, RECOVER_TEST_NODE_ID, None).unwrap();
    store
        .release_node_daemon_lease(RECOVER_TEST_NODE_ID, "dead-daemon", 1, instance, 3)
        .unwrap();
    let successor = store
        .acquire_node_daemon_lease(
            RECOVER_TEST_NODE_ID,
            "dead-daemon",
            "2147483647:successor:dead-daemon",
            4,
            1,
        )
        .unwrap();
    let refusal = recover_captured(&home, &intent).unwrap_err();
    assert!(refusal.1.contains("NODE_DAEMON_GENERATION_FENCED"));
    assert_eq!(
        store
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap(),
        successor
    );
}

#[test]
fn recovery_does_not_add_spaces_after_validation() {
    let (_tree, home, first) = seed_recover_test_node("captured-spaces");
    let instance = "2147483647:captured:dead-daemon";
    seed_local_generation(&first, 1, instance);
    let intent = validate_daemon_predecessor_recovery(&home, RECOVER_TEST_NODE_ID, None).unwrap();
    let second = second_recovery_store(&home);
    seed_local_generation(&second, 1, instance);
    assert_eq!(
        recover_captured(&home, &intent).unwrap()["recovered_spaces"],
        serde_json::json!(["space-recover"])
    );
    assert_ne!(
        second
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
            .status,
        NodeDaemonLeaseStatus::Released
    );
}
