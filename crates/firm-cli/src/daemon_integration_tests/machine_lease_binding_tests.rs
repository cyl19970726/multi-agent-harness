//! The NodeDaemon's own Stores name this machine's lease (ADR 0075).
//!
//! Every machine-lease writer — acquire, renew, drain, release — runs on a
//! Store handed out by `registered_spaces()`. If those Stores inferred their
//! Firm home from a Space root's lexical parent, the daemon and the CLI could
//! name two different lease documents for one machine: exactly the
//! two-authorities failure ADR 0075 exists to remove, arriving through the path
//! layer instead of the lock layer. So the daemon binds the home it already
//! holds, and the derivation stays an affordance for fixtures and tools.
//!
//! Without `.with_firm_home(&self.firm_home)` in `registered_spaces`, the
//! second Space below derives no home at all and `node_home` returns the
//! `MACHINE_LEASE_FILE_UNRESOLVED` refusal — this test is red on that revision.

use super::*;
use harness_node_daemon::test_support::{TestDaemon, TestDaemonConfig};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

const NODE_ID: &str = "11111111-1111-4111-8111-000000000b40";

/// A Firm home with two registered Execution Spaces: one in production's
/// layout, one registered somewhere else entirely under the same home. Where a
/// Space is registered is not supposed to decide who owns the machine.
fn firm_home_with_two_spaces(label: &str) -> PathBuf {
    let firm_home = std::env::temp_dir().join(format!(
        "firm-machine-lease-binding-{label}-{}-{}",
        std::process::id(),
        current_unix_ms_u64()
    ));
    let shaped = firm_home.join("execution-spaces").join("shaped-space");
    let unshaped = firm_home.join("registered-elsewhere").join("other-space");
    std::fs::create_dir_all(&shaped).expect("create Space in production's layout");
    std::fs::create_dir_all(&unshaped).expect("create Space registered elsewhere");

    let mut registry = crate::execution_space::ExecutionSpaceRegistry {
        format_version: 1,
        current_space_id: Some("shaped-space".into()),
        spaces: vec![
            crate::execution_space::ExecutionSpaceRegistryEntry {
                id: "shaped-space".into(),
                name: "shaped".into(),
                store_root: shaped,
                default_project_binding_id: None,
                company_id: None,
                created_at: "unix-ms:1".into(),
                last_opened_at: "unix-ms:1".into(),
            },
            crate::execution_space::ExecutionSpaceRegistryEntry {
                id: "other-space".into(),
                name: "elsewhere".into(),
                store_root: unshaped,
                default_project_binding_id: None,
                company_id: None,
                created_at: "unix-ms:1".into(),
                last_opened_at: "unix-ms:1".into(),
            },
        ],
    };
    registry.save(&firm_home).expect("write Space registry");
    firm_home
}

fn daemon_for(firm_home: PathBuf) -> TestDaemon {
    TestDaemon::new(TestDaemonConfig {
        firm_home,
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "machine-lease-binding-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    })
}

#[test]
fn daemon_stores_name_one_node_home_however_a_space_is_registered() {
    let firm_home = firm_home_with_two_spaces("one-home");
    let daemon = daemon_for(firm_home.clone());

    let spaces = daemon.registered_spaces().expect("list Execution Spaces");
    assert_eq!(spaces.len(), 2, "fixture registers two Spaces");

    let expected = std::fs::canonicalize(&firm_home)
        .expect("canonical Firm home")
        .join("nodes")
        .join(NODE_ID);
    for (space, store) in spaces {
        assert_eq!(
            store.node_home(NODE_ID).unwrap_or_else(|error| panic!(
                "Space {} yields a Store that cannot name this machine's lease: {error}",
                space.id
            )),
            expected,
            "Space {} must not move machine authority",
            space.id
        );
        assert!(
            !store
                .node_home(NODE_ID)
                .expect("node home")
                .starts_with(std::fs::canonicalize(store.root()).expect("canonical Space root")),
            "the machine lease belongs beside the Firm home, not inside Space {}",
            space.id
        );
    }
}

/// The binding is explicit, not derived: it survives a Space root that says
/// nothing about which home owns it, which is the case
/// `FIRM_ALLOW_EXTERNAL_STORE_ROOT` makes reachable in production.
#[test]
fn a_space_root_that_names_no_home_still_resolves_through_the_daemons_binding() {
    let firm_home = firm_home_with_two_spaces("unshaped-root");
    let daemon = daemon_for(firm_home.clone());

    let (space, store) = daemon
        .registered_spaces()
        .expect("list Execution Spaces")
        .into_iter()
        .find(|(space, _)| space.id == "other-space")
        .expect("the Space registered outside execution-spaces/");

    assert_eq!(
        harness_store::firm_home_of_execution_space_root(&space.store_root),
        None,
        "this Space root deliberately derives no Firm home on its own"
    );
    assert_eq!(
        store
            .node_home(NODE_ID)
            .expect("the daemon's binding answers"),
        std::fs::canonicalize(&firm_home)
            .expect("canonical Firm home")
            .join("nodes")
            .join(NODE_ID)
    );
}

/// A pre-cutover Execution Space: legacy lease rows, no machine document.
fn pre_cutover_home(label: &str) -> (PathBuf, HarnessStore, harness_core::NodeDaemonLease) {
    let firm_home = std::env::temp_dir().join(format!(
        "firm-machine-lease-legacy-{label}-{}-{}",
        std::process::id(),
        current_unix_ms_u64()
    ));
    let space = crate::execution_space::register_and_activate(
        &firm_home,
        "legacy-space",
        "Legacy Space",
        Some("legacy-project".into()),
        None,
        "unix-ms:1",
    )
    .expect("register the pre-cutover Space");
    let store = HarnessStore::new(space.store_root).with_firm_home(&firm_home);
    store.init().expect("initialize the pre-cutover Store");
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: NODE_ID.into(),
            display_name: "Pre-cutover Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert the pre-cutover Node");
    store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: NODE_ID.into(),
                execution_space_id: "legacy-space".into(),
                project_binding_id: "legacy-project".into(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            "legacy-space",
        )
        .expect("register the pre-cutover project");
    // Only the legacy writer runs here, exactly as it did before the cutover.
    let row = store
        .acquire_node_daemon_lease(
            NODE_ID,
            &format!("node-daemon:{NODE_ID}"),
            "machine-lease-binding-instance",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("a pre-cutover Store still writes its Space row");
    (firm_home, store, row)
}

/// Legacy-refusal class, **daemon fence**.
///
/// The heartbeat renews authority it already holds. Handed a Store whose only
/// record is a pre-cutover Space row, it must refuse rather than renew — and it
/// must refuse loudly, latching machine authority loss, because "I resolved a
/// record that cannot authorize" is not "nothing to do here". A `LegacySpaceRow`
/// read as a benign absence is precisely how a daemon would keep driving on
/// authority it no longer has (ADR 0075).
#[test]
fn a_legacy_space_row_cannot_reach_the_daemon_heartbeat_fence() {
    let (firm_home, store, row) = pre_cutover_home("daemon-fence");
    let daemon = daemon_for(firm_home);
    daemon.remember_node_lease("legacy-space", &store, &row);

    let error = daemon
        .refresh_held_node_authorities()
        .expect_err("a legacy Space row can never renew machine authority");
    assert!(
        error
            .to_string()
            .contains("MACHINE_LEASE_NOT_AUTHORITATIVE"),
        "the refusal must name the legacy source: {error}"
    );
    assert!(daemon.authority_lost());
    // And nothing was written: the pre-cutover row is untouched and no document
    // was invented for it.
    assert_eq!(
        store
            .latest_node_daemon_lease(NODE_ID)
            .expect("read the legacy row")
            .expect("the legacy row is still there"),
        row
    );
    assert!(!store
        .machine_lease_path(NODE_ID)
        .expect("machine lease path")
        .exists());
}

/// Legacy-refusal class, **CLI fence**.
///
/// `firm daemon recover-predecessor` validates before it settles anything. On a
/// pre-cutover Store it must refuse by name instead of treating the legacy row
/// as a predecessor it may recover — recovering one would settle Sessions and
/// publish a release against a record no fence reads.
#[test]
fn a_legacy_space_row_cannot_reach_the_cli_recovery_fence() {
    let (firm_home, store, row) = pre_cutover_home("cli-fence");
    let (code, detail) = crate::daemon_predecessor_recovery::validate_daemon_predecessor_recovery(
        &firm_home, NODE_ID, None,
    )
    .err()
    .expect("a legacy Space row is not a recoverable predecessor");
    assert_eq!(code, "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE");
    assert!(
        detail.contains("MACHINE_LEASE_NOT_AUTHORITATIVE"),
        "the refusal must name the legacy source: {detail}"
    );
    assert_eq!(
        store
            .latest_node_daemon_lease(NODE_ID)
            .expect("read the legacy row")
            .expect("the legacy row is still there"),
        row,
        "a refused validation settles nothing"
    );
}
