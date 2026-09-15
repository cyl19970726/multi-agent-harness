//! Machine authority is one document per `(FIRM_HOME, node_id)` (ADR 0075),
//! so a Store has to name `<FIRM_HOME>/nodes/<node_id>` before it can answer a
//! machine-authority question — and fail closed when it cannot.

use super::*;

/// A registered store root is `<FIRM_HOME>/<execution-spaces|projects>/<id>`,
/// so it already says which Firm home owns it. That is a path fact, not an
/// authority decision: it only says which directory the one lease document
/// lives in. Both registered layouts sit under the same home and both name it.
#[test]
fn a_registered_store_root_names_its_own_firm_home() {
    let firm_home = team_test_root("node-home-registered");

    for registered_dir in ["execution-spaces", "projects"] {
        let store = HarnessStore::new(firm_home.join(registered_dir).join("store-1"));
        assert_eq!(
            store.firm_home(),
            Some(firm_home.as_path()),
            "{registered_dir} is a registered store directory"
        );
        assert_eq!(
            store.node_home("node-1").expect("registered root resolves"),
            firm_home.join("nodes").join("node-1")
        );
    }
}

/// The refusal is the contract: "I cannot name the lease document" is never
/// the same as "nobody owns this machine", so it must not become a skipped
/// fence or a fall back to Execution Space data.
#[test]
fn an_unshaped_root_binds_nothing_and_fails_closed() {
    let store_root = team_test_root("node-home-unshaped");
    let store = HarnessStore::new(&store_root);

    assert_eq!(store.firm_home(), None);
    let error = store
        .node_home("node-1")
        .expect_err("an unbound Store cannot name a machine lease document");
    assert!(
        error.to_string().contains(MACHINE_LEASE_FILE_UNRESOLVED),
        "unexpected refusal: {error}"
    );
}

/// A caller that knows its Firm home out of band never depends on its store
/// root's layout, and an explicit binding always wins over the derived one.
#[test]
fn an_explicit_binding_wins_over_the_derived_one() {
    let derived_home = team_test_root("node-home-derived");
    let store_root = derived_home.join("execution-spaces").join("space-1");
    let explicit_home = team_test_root("node-home-explicit");

    let store = HarnessStore::new(&store_root).with_firm_home(&explicit_home);
    assert_eq!(store.firm_home(), Some(explicit_home.as_path()));
    assert_eq!(
        store
            .node_home("node-1")
            .expect("explicit binding resolves"),
        explicit_home.join("nodes").join("node-1")
    );

    let rescued =
        HarnessStore::new(team_test_root("node-home-rescued")).with_firm_home(&explicit_home);
    assert_eq!(
        rescued
            .node_home("node-1")
            .expect("explicit binding resolves"),
        explicit_home.join("nodes").join("node-1")
    );
}

/// Only the exact `execution-spaces` parent counts. A near miss must not
/// silently bind a wrong Firm home — that would put machine authority in a
/// directory nobody else looks in, which is worse than failing closed.
#[test]
fn a_near_miss_layout_does_not_derive_a_firm_home() {
    for parent in ["execution_spaces", "executionspaces", "spaces", "nodes"] {
        let root = team_test_root("node-home-near-miss")
            .join(parent)
            .join("space-1");
        assert_eq!(
            HarnessStore::new(&root).firm_home(),
            None,
            "{parent} must not be read as the Execution Space directory"
        );
    }

    // A bare relative root has no parent to inspect at all.
    assert_eq!(HarnessStore::new("space-1").firm_home(), None);
}

/// A node id names one directory under `<FIRM_HOME>/nodes/`, so a foreign or
/// malformed id is refused by name instead of escaping the node tree.
#[test]
fn a_node_id_that_is_not_one_path_segment_is_refused() {
    let firm_home = team_test_root("node-home-segment");
    let store = HarnessStore::new(firm_home.join("execution-spaces").join("space-1"));

    for node_id in ["", ".", "..", "../other", "a/b", "a\\b"] {
        let error = store
            .node_home(node_id)
            .expect_err("a node id must be one ordinary path segment");
        assert!(
            error.to_string().contains(MACHINE_LEASE_FILE_UNRESOLVED),
            "unexpected refusal for {node_id:?}: {error}"
        );
    }
}

/// The binding is inert until the cutover: it changes where a *future*
/// machine-authority read looks, and nothing about this Store's own data.
#[test]
fn binding_a_firm_home_changes_no_store_data() {
    let root = team_test_root("node-home-inert");
    let unbound = HarnessStore::new(&root);
    unbound.init().expect("initialize store");
    unbound
        .insert_execution_node(&ExecutionNode {
            id: "2437c3dd-0000-4000-8000-000000000001".into(),
            display_name: "inert".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");

    let bound = HarnessStore::new(&root).with_firm_home(team_test_root("node-home-inert-home"));
    assert_eq!(bound.root(), unbound.root());
    assert_eq!(
        bound.latest_execution_nodes().expect("bound read"),
        unbound.latest_execution_nodes().expect("unbound read")
    );
}
