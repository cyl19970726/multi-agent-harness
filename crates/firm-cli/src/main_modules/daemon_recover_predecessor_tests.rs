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
        .seed_machine_authority_for_test(
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
    // recovery skipped because they were already settled (#837) and the lanes
    // a dying generation had flagged as unsettled (ADR 0073).
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
            "sessions_settlement_incomplete": [],
        }])
    );
    // Whichever path settles a predecessor, the receipt carries the exact
    // death proof that authorized it.
    assert_eq!(projection["process_death_proof"]["pid"], 2147483647_i64);
    assert_eq!(
        projection["process_death_proof"]["reason"],
        "process_absent"
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

/// ADR 0075 retarget of `recover_predecessor_partial_failure_preserves_releases_and_retry_markers`.
///
/// The old test proved that a partial failure *preserved* what it had already
/// released: per-Space leases, so one Space could be `Released` while another
/// was not, and `authority_released: false` meant "partly" (DEV-149-REVIEW-03).
/// One document per machine makes "partly released" unsayable, so the successor
/// property is the stronger one the ADR asks for: the publish is
/// **all-or-nothing**. A failure anywhere leaves the machine still carrying the
/// predecessor generation — a successor cannot acquire, and the retry has
/// something to retry — while the per-Space settlements that did land stay
/// landed, because those are Space facts and remain true.
#[test]
fn recover_predecessor_failure_publishes_no_release_and_keeps_its_retry_markers() {
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
    let captured = store
        .seed_machine_authority_for_test(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 1, 1)
        .expect("seed expired predecessor");
    let intent =
        validate_daemon_predecessor_recovery(&firm_home, RECOVER_TEST_NODE_ID, None).unwrap();
    assert_eq!(intent.spaces.len(), 2, "recovery spans both Spaces");

    // Change the machine after capture. The captured tuple is now
    // deterministically fenced, independent of runner speed or wall-clock TTL —
    // and because there is one document, it is fenced for *every* Space at
    // once, which is what makes a partial publish unconstructable.
    second
        .release_machine_authority_for_test(
            RECOVER_TEST_NODE_ID,
            "dead-daemon",
            captured.generation,
            instance,
            3,
        )
        .unwrap();
    let successor = second
        .seed_machine_authority_for_test(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 4, 1)
        .unwrap();
    assert!(successor.generation > captured.generation);
    std::thread::sleep(Duration::from_millis(5));
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
    // The retry markers: the receipt still names every Space the request
    // carried and the exact refusal each met, so the operator retries with
    // evidence rather than a guess.
    assert_eq!(receipt["space_leases"].as_array().unwrap().len(), 2);
    let failures = receipt["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 2);
    assert!(
        failures.iter().all(|failure| failure
            .as_str()
            .unwrap()
            .contains("NODE_DAEMON_GENERATION_FENCED")),
        "{failures:?}"
    );
    // All-or-nothing on the document: no Space's outcome could publish a
    // release, so the machine is exactly where it was and nothing may acquire.
    assert_eq!(receipt["authority_released"], false);
    assert_eq!(
        second
            .current_authorized_machine_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap(),
        successor
    );
    assert_ne!(successor.status, NodeDaemonLeaseStatus::Released);

    // A new request captures the current machine generation and completes, so
    // the failure above cost the operator a retry rather than the machine.
    let retry_intent =
        validate_daemon_predecessor_recovery(&firm_home, RECOVER_TEST_NODE_ID, None).unwrap();
    assert_eq!(retry_intent.spaces[0].1.generation, successor.generation);
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
    .expect("the retry settles every Space and publishes one release");
    assert_eq!(repeated["status"], "released");
    assert_eq!(repeated["authority_released"], true);
    assert_eq!(repeated["space_settlements"].as_array().unwrap().len(), 2);
    assert_eq!(
        store
            .current_authorized_machine_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
            .status,
        NodeDaemonLeaseStatus::Released
    );
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
        .seed_machine_authority_for_test(
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
        .seed_machine_authority_for_test(
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

/// ADR 0075 retarget of `absent_status_names_each_predecessor_lease_expiry`.
///
/// "Each" is the word that stopped being true. The old status named one
/// predecessor lease per Execution Space, because that is where the lease
/// lived — and in the dogfood evidence a single node carried 23 distinct
/// "current" generations across 25 Spaces, so "each" meant an operator reading
/// 25 lines that disagreed. There is now one lease, so the status names it
/// once, says which record answered (`lease_source`) and where that record is
/// (`lease_path`). Naming the file is strictly more than the old form gave: the
/// operator can open the exact document the refusal was decided from.
#[test]
fn absent_status_names_the_one_predecessor_lease_and_the_document_it_read() {
    let log_path = Path::new("node-daemon.log");

    let (_unexpired_tree, unexpired_home, unexpired_store) =
        seed_recover_test_node("status-unexpired");
    let lease = unexpired_store
        .seed_machine_authority_for_test(
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
    // Once, not once per Space — and the machine's own record said so.
    assert_eq!(
        status
            .matches("unreleased predecessor NodeDaemonLease")
            .count(),
        1,
        "{status}"
    );
    assert!(status.contains("lease_source node_file"), "{status}");
    let document = unexpired_store
        .machine_lease_path(RECOVER_TEST_NODE_ID)
        .expect("the machine lease document path");
    assert!(
        status.contains(&format!("lease_path {}", document.display())),
        "{status}"
    );

    let (_expired_tree, expired_home, expired_store) = seed_recover_test_node("status-expired");
    let expired_lease = expired_store
        .seed_machine_authority_for_test(
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

/// Drive the machine document up to `generation`, one real acquire/release pair
/// per step.
///
/// Renamed from `seed_local_generation`: ADR 0075 abolishes Space-local
/// generations, so a helper named for them would describe a counter that no
/// longer exists. Every Store handed here resolves the same
/// `<FIRM_HOME>/nodes/<node_id>/` document, which is what the callers now rely
/// on rather than work around.
fn seed_machine_generations(store: &HarnessStore, generation: u64, instance: &str) {
    for index in 1..=generation {
        let lease = store
            .seed_machine_authority_for_test(
                RECOVER_TEST_NODE_ID,
                "dead-daemon",
                instance,
                index * 2,
                1,
            )
            .unwrap();
        assert_eq!(lease.generation, index);
        if index < generation {
            store
                .release_machine_authority_for_test(
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

/// ADR 0075 retarget of `recovery_uses_space_local_generations_and_http_never_expands_its_tuple`.
///
/// The old test *depended* on the defect: it drove one Execution Space to
/// generation 4 and another to 26 for the same machine authority, then proved
/// recovery honoured each Space's own counter. That state is what this ADR
/// removes — in the live dogfood store one node carried 23 distinct "current"
/// generations, from 1 to 148, for one authority — so the successor property is
/// its inverse, and it is the cutover's central claim: **the generation is
/// machine-wide and monotonic, whichever Store asks**.
///
/// The second half is preserved unchanged: an HTTP intent authorizes exactly
/// one `(daemon, instance, generation)` tuple and can never implicitly expand
/// to another generation.
#[test]
fn the_generation_is_machine_wide_and_monotonic_and_http_never_expands_its_tuple() {
    let (_tree, home, first) = seed_recover_test_node("machine-wide-generations");
    let second = second_recovery_store(&home);
    let instance = "2147483647:local:dead-daemon";
    // Four real transitions, driven through the first Space's Store.
    seed_machine_generations(&first, 4, instance);
    let read_through = |store: &HarnessStore| {
        store
            .current_authorized_machine_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
    };
    // The second Space's Store reports the same number rather than starting its
    // own count at 1, which is exactly what it would have done before.
    assert_eq!(read_through(&first).generation, 4);
    assert_eq!(read_through(&second).generation, 4);
    assert_eq!(
        read_through(&first),
        read_through(&second),
        "one machine, one lease — not one per Execution Space"
    );

    // Monotonic across handles: a transition driven from the other Store
    // continues the same sequence instead of restarting it.
    second
        .release_machine_authority_for_test(RECOVER_TEST_NODE_ID, "dead-daemon", 4, instance, 9)
        .unwrap();
    let fifth = second
        .seed_machine_authority_for_test(RECOVER_TEST_NODE_ID, "dead-daemon", instance, 10, 1)
        .unwrap();
    assert_eq!(fifth.generation, 5);
    assert_eq!(read_through(&first).generation, 5);
    std::thread::sleep(Duration::from_millis(5));

    // An HTTP intent naming the exact live tuple recovers that tuple.
    let http = validate_daemon_predecessor_recovery(
        &home,
        RECOVER_TEST_NODE_ID,
        Some(("dead-daemon", instance, 5)),
    )
    .unwrap();
    assert_eq!(http.instance_id, instance);
    // Every Space this node serves is settled by the one recovery, because the
    // per-Space half of recovery is Session settlement, not authority.
    assert_eq!(http.spaces.len(), 2);
    assert!(http.spaces.iter().all(|(_, lease)| lease.generation == 5));
    let result = recover_captured(&home, &http).unwrap();
    assert_eq!(result["space_settlements"][0]["generation"], 5);
    assert_eq!(read_through(&first).status, NodeDaemonLeaseStatus::Released);

    // And it cannot implicitly authorize a different generation: a tuple naming
    // the superseded generation 4 is refused rather than widened to 5.
    let superseded = validate_daemon_predecessor_recovery(
        &home,
        RECOVER_TEST_NODE_ID,
        Some(("dead-daemon", instance, 4)),
    )
    .err()
    .expect("an HTTP tuple naming a superseded generation must be refused");
    assert_eq!(
        superseded.0, "SUPERVISOR_GENERATION_FENCED",
        "{superseded:?}"
    );
    assert!(
        superseded.1.contains("exact latest predecessor"),
        "{superseded:?}"
    );

    // The CLI retry reports the already released exact predecessor once, under
    // the one generation that exists.
    let retry =
        daemon_recover_predecessor(&home, RECOVER_TEST_NODE_ID, &recover_args(true)).unwrap();
    assert_eq!(retry["already_released"], true);
    assert_eq!(retry["generation"], 5);
}

/// ADR 0075 retarget of the first half only; the second half is untouched.
///
/// The first half used to construct two Spaces holding *different* unreleased
/// instances at the same generation and assert that `validate` refused with
/// "different unreleased". One document per machine keeps the property and
/// moves where it is enforced: the foreign instance is refused at acquisition,
/// so `validate` is never handed the ambiguity in the first place. That is
/// strictly stronger — the old refusal still had to be reached after both rows
/// existed — and the test name stays true, because a same-generation foreign
/// instance is still exactly what is refused.
///
/// The second half — a successor generation taken after `validate` captured its
/// intent must fence the captured recovery — is unchanged and is the reason
/// this test is migrated rather than replaced.
#[test]
fn recovery_refuses_same_generation_foreign_instance_and_fences_successor() {
    let (_tree, home, first) = seed_recover_test_node("foreign-instance");
    let second = second_recovery_store(&home);
    let instance = "2147483647:first:dead-daemon";
    seed_machine_generations(&first, 1, instance);
    let foreign = second
        .acquire_machine_lease(
            RECOVER_TEST_NODE_ID,
            "dead-daemon",
            "2147483647:foreign:dead-daemon",
            1,
            &[],
        )
        .expect_err("a foreign instance cannot take a machine an unreleased predecessor holds")
        .to_string();
    assert!(
        foreign.contains("NODE_DAEMON_PREDECESSOR_RECOVERY_REQUIRED"),
        "{foreign}"
    );
    assert!(foreign.contains(instance), "{foreign}");
    // Nothing was written, so the one predecessor `validate` can see is still
    // the exact instance that held the machine.
    let held = first
        .current_authorized_machine_lease(RECOVER_TEST_NODE_ID)
        .unwrap()
        .unwrap();
    assert_ne!(held.status, NodeDaemonLeaseStatus::Released);
    assert_eq!(held.instance_id, instance);
    assert_eq!(
        validate_daemon_predecessor_recovery(&home, RECOVER_TEST_NODE_ID, None)
            .unwrap()
            .instance_id,
        instance
    );

    let (_tree, home, store) = seed_recover_test_node("successor-after-validate");
    seed_machine_generations(&store, 1, instance);
    let intent = validate_daemon_predecessor_recovery(&home, RECOVER_TEST_NODE_ID, None).unwrap();
    store
        .release_machine_authority_for_test(RECOVER_TEST_NODE_ID, "dead-daemon", 1, instance, 3)
        .unwrap();
    let successor = store
        .seed_machine_authority_for_test(
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
            .current_authorized_machine_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap(),
        successor
    );
}

/// ADR 0075 retarget of `recovery_does_not_add_spaces_after_validation`.
///
/// The old test proved a Space registered *after* validation could not join the
/// recovery set and have its lease released behind the operator's back — a real
/// hazard while every Space carried its own authority row. Spaces no longer
/// enter the authority decision at all, so the successor property pins that
/// directly: the captured intent still settles exactly the Spaces it captured,
/// and a Space that appears afterwards changes neither the recovered set nor —
/// the half that used to matter most — who owns the machine, because it never
/// had a lease of its own to release.
#[test]
fn a_space_appearing_after_validation_joins_neither_the_recovery_set_nor_the_authority() {
    let (_tree, home, first) = seed_recover_test_node("captured-spaces");
    let instance = "2147483647:captured:dead-daemon";
    seed_machine_generations(&first, 1, instance);
    let intent = validate_daemon_predecessor_recovery(&home, RECOVER_TEST_NODE_ID, None).unwrap();
    assert_eq!(intent.spaces.len(), 1);
    // A Space that appears after capture. It carries no machine authority of
    // its own — it resolves the same document every other Store does — so there
    // is nothing here for a widened sweep to release.
    let second = second_recovery_store(&home);
    assert_eq!(
        second
            .latest_node_daemon_lease(RECOVER_TEST_NODE_ID)
            .unwrap(),
        None,
        "a new Space mints no lease row of its own after cutover"
    );
    let receipt = recover_captured(&home, &intent).unwrap();
    assert_eq!(
        receipt["recovered_spaces"],
        serde_json::json!(["space-recover"]),
        "the captured set is the settled set"
    );
    // The machine's own authority moved exactly once, for the captured
    // generation, and the late Space reads that same one answer.
    assert_eq!(receipt["authority_released"], true);
    assert_eq!(
        second
            .current_authorized_machine_lease(RECOVER_TEST_NODE_ID)
            .unwrap()
            .unwrap()
            .status,
        NodeDaemonLeaseStatus::Released
    );
}
