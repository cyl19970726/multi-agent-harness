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
