//! Machine authority is one document per `(FIRM_HOME, node_id)` (ADR 0075),
//! so a Store has to name `<FIRM_HOME>/nodes/<node_id>` before it can answer a
//! machine-authority question — and fail closed when it cannot.
//!
//! "Cannot" covers more than "was never told". A home that is relative, or one
//! reached through a filesystem alias, names a *different* file from the next
//! process — which is a lease per caller rather than a lease per machine, and
//! it would pass a fence silently. Those are the interesting cases below.

use super::*;

fn assert_unresolved(error: &StoreError, context: &str) {
    assert!(
        error.is_machine_lease_unresolved(),
        "{context}: expected the named machine-lease refusal, got {error}"
    );
    assert!(
        error.to_string().contains(MACHINE_LEASE_FILE_UNRESOLVED),
        "{context}: refusal should also name itself in its message: {error}"
    );
}

/// An Execution Space store root is `<FIRM_HOME>/execution-spaces/<id>`, so it
/// already says which Firm home owns it. That is a path fact, not an authority
/// decision: it only says which directory the one lease document lives in.
#[test]
fn an_execution_space_store_root_names_its_own_firm_home() {
    let firm_home = team_test_firm_home("node-home-space-shaped");
    std::fs::create_dir_all(&firm_home).expect("create Firm home");
    let store = HarnessStore::new(firm_home.join("execution-spaces").join("space-1"));

    assert_eq!(store.firm_home(), Some(firm_home.as_path()));
    assert_eq!(
        store
            .node_home("node-1")
            .expect("space-shaped root resolves"),
        std::fs::canonicalize(&firm_home)
            .expect("canonical home")
            .join("nodes")
            .join("node-1")
    );
}

/// `projects` is one of the most common directory names on a developer
/// machine, so the lexical rule deliberately does not accept it: it would read
/// `/Users/x/dev/projects/myrepo` as the Firm home `/Users/x/dev` and put a
/// machine lease document somewhere nobody else looks. A project store that
/// needs a home binds one.
#[test]
fn a_project_store_root_does_not_claim_a_firm_home() {
    assert_eq!(
        HarnessStore::new(
            team_test_firm_home("node-home-projects")
                .join("projects")
                .join("store-1")
        )
        .firm_home(),
        None
    );
    assert_eq!(
        HarnessStore::new("/Users/x/dev/projects/myrepo").firm_home(),
        None,
        "an ordinary developer directory must not be claimed as a Firm home"
    );
}

/// The refusal is the contract: "I cannot name the lease document" is never
/// the same as "nobody owns this machine", so it must not become a skipped
/// fence or a fall back to Execution Space data.
#[test]
fn an_unshaped_root_binds_nothing_and_fails_closed() {
    let store = HarnessStore::new(team_test_firm_home("node-home-unshaped"));

    assert_eq!(store.firm_home(), None);
    assert_unresolved(
        &store
            .node_home("node-1")
            .expect_err("an unbound Store cannot name a machine lease document"),
        "unbound Store",
    );
}

/// The fail-*open* case, and the reason absoluteness is a rule rather than an
/// assumption: `Path::parent` of a relative `execution-spaces/<id>` is the
/// empty path, which is `Some(_)`. Binding that would name the cwd-relative
/// `nodes/<id>`, so every process with a different working directory would own
/// a different machine.
#[test]
fn a_relative_home_is_refused_on_both_paths() {
    for relative_root in [
        "execution-spaces/id",
        "projects/id",
        "a/execution-spaces/id",
    ] {
        assert_eq!(
            HarnessStore::new(relative_root).firm_home(),
            None,
            "{relative_root} must not derive a relative Firm home"
        );
    }

    for relative_home in [".firm-test", "relative/home", ""] {
        let store = HarnessStore::new(team_test_firm_home("node-home-relative-explicit"))
            .with_firm_home(relative_home);
        assert_unresolved(
            &store
                .node_home("node-1")
                .expect_err("a relative explicit home cannot name one machine's lease"),
            &format!("explicit home {relative_home:?}"),
        );
    }
}

/// One directory reached through two spellings would be two documents under
/// two flocks — two machine authorities for one machine. The sibling
/// machine-authority path in this same directory (`daemon.sock`) canonicalizes
/// for exactly this reason, so the lease must agree with it rather than with
/// the caller's raw spelling.
#[test]
fn two_spellings_of_one_home_name_one_node_home() {
    let base = team_test_firm_home("node-home-alias");
    let real = base.join("real-home");
    std::fs::create_dir_all(real.join("nodes")).expect("create real home");
    let alias = base.join("alias-home");
    std::os::unix::fs::symlink(&real, &alias).expect("symlink the Firm home");

    let through_real = HarnessStore::new(real.join("execution-spaces").join("space-1"));
    let through_alias = HarnessStore::new(alias.join("execution-spaces").join("space-1"));
    let explicit_alias =
        HarnessStore::new(team_test_firm_home("node-home-alias-explicit")).with_firm_home(&alias);

    let expected = through_real
        .node_home("node-1")
        .expect("real home resolves");
    assert_eq!(
        through_alias.node_home("node-1").expect("alias resolves"),
        expected,
        "a symlinked Firm home must name the same node home as the real one"
    );
    assert_eq!(
        explicit_alias.node_home("node-1").expect("alias resolves"),
        expected,
        "an explicitly bound alias must resolve the same way a derived one does"
    );
    assert_ne!(
        through_alias.firm_home(),
        through_real.firm_home(),
        "the two Stores really were bound to different spellings"
    );
}

/// A home that does not exist yet still gets one identity: the deepest
/// existing ancestor is resolved and the rest re-attached, so the first daemon
/// to start does not name a different file from the second.
#[test]
fn a_home_that_does_not_exist_yet_still_resolves_once() {
    let base = team_test_firm_home("node-home-absent");
    std::fs::create_dir_all(&base).expect("create base");
    let canonical_base = std::fs::canonicalize(&base).expect("canonical base");
    let absent = base.join("not-created-yet").join("home");

    let store = HarnessStore::new(absent.join("execution-spaces").join("space-1"));
    assert_eq!(
        store
            .node_home("node-1")
            .expect("absent home still resolves"),
        canonical_base
            .join("not-created-yet")
            .join("home")
            .join("nodes")
            .join("node-1")
    );
}

/// A caller that knows its Firm home never depends on its store root's layout,
/// and an explicit binding always wins over the derived one.
#[test]
fn an_explicit_binding_wins_over_the_derived_one() {
    let derived_home = team_test_firm_home("node-home-derived");
    let explicit_home = team_test_firm_home("node-home-explicit");
    std::fs::create_dir_all(&explicit_home).expect("create explicit home");
    let expected = std::fs::canonicalize(&explicit_home)
        .expect("canonical home")
        .join("nodes")
        .join("node-1");

    let store = HarnessStore::new(derived_home.join("execution-spaces").join("space-1"))
        .with_firm_home(&explicit_home);
    assert_eq!(store.firm_home(), Some(explicit_home.as_path()));
    assert_eq!(store.node_home("node-1").expect("explicit binds"), expected);

    let rescued =
        HarnessStore::new(team_test_firm_home("node-home-rescued")).with_firm_home(&explicit_home);
    assert_eq!(
        rescued.node_home("node-1").expect("explicit binds"),
        expected
    );
}

/// Only the exact `execution-spaces` parent counts. A near miss must not
/// silently bind a wrong Firm home — that would put machine authority in a
/// directory nobody else looks in, which is worse than failing closed.
#[test]
fn a_near_miss_layout_does_not_derive_a_firm_home() {
    for parent in [
        "execution_spaces",
        "executionspaces",
        "Execution-Spaces",
        "spaces",
        "nodes",
        "project",
        "Projects",
    ] {
        let root = team_test_firm_home("node-home-near-miss")
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
/// malformed id is refused by name instead of escaping the node tree — using
/// the crate's existing allowlist for that same segment, not a second rule.
#[test]
fn a_node_id_that_is_not_one_safe_component_is_refused() {
    let firm_home = team_test_firm_home("node-home-segment");
    std::fs::create_dir_all(&firm_home).expect("create Firm home");
    let store = HarnessStore::new(firm_home.join("execution-spaces").join("space-1"));

    for node_id in [
        "",
        ".",
        "..",
        "../other",
        "a/b",
        "a\\b",
        "a b",
        "a%b",
        &"n".repeat(129),
    ] {
        assert_unresolved(
            &store
                .node_home(node_id)
                .expect_err("a node id must be one safe canonical path component"),
            &format!("node id {node_id:?}"),
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

    let bound =
        HarnessStore::new(&root).with_firm_home(team_test_firm_home("node-home-inert-home"));
    assert_eq!(bound.root(), unbound.root());
    assert_eq!(
        bound.latest_execution_nodes().expect("bound read"),
        unbound.latest_execution_nodes().expect("unbound read")
    );
}

/// The binding has to be true wherever machine authority is actually written.
/// Pinning it at the acquire path means a fixture that drifts back to a bare
/// root is caught here, by name, rather than as an unexplained fence refusal
/// once the cutover makes `node_home` load-bearing.
#[test]
fn a_store_that_can_acquire_a_node_daemon_lease_can_name_its_node_home() {
    let (root, store) = temp_store("node-home-acquire");
    store.init().expect("initialize store");
    let node_id = "11111111-1111-4111-8111-111111111111";
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: "test-node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");
    store
        .acquire_node_daemon_lease(node_id, "node-daemon:test", "instance-1", 1, 60_000)
        .expect("acquire machine authority");

    let node_home = store
        .node_home(node_id)
        .expect("a Store that owns machine authority can name where it belongs");
    let canonical_root = std::fs::canonicalize(&root).expect("canonical store root");
    assert!(
        !node_home.starts_with(&canonical_root),
        "the machine lease document belongs beside the Firm home, not inside the Space: {}",
        node_home.display()
    );
}
