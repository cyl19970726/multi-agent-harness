//! The cutover itself: who may decide machine authority, and under which lock
//! (ADR 0075, slice E2a-2a).
//!
//! `machine_lease_leaves_the_space_data_lock` proves the mechanism. These tests
//! prove the *switch* — that the deciders now read the node file, that a
//! `LegacySpaceRow` can never reach one, and that the lock rule survives the
//! one place it was actually violated during development.
//!
//! They are class tests, not one test per call site. A decider only reads, and
//! reads lock-free, so forty-odd per-site tests would assert the same two facts
//! forty-odd times. What a per-site sweep is genuinely needed for — "did anyone
//! add a new one?" — is mechanical, so it is a mechanical gate here rather than
//! a list someone has to remember to extend.

use super::*;
use crate::store_machine_lease::MachineLeaseSource;
use crate::store_node_runtime::current_store_unix_ms;

const NODE: &str = "2437c3dd-0000-4000-8000-00000000e2a2";

fn cutover_store(label: &str) -> (PathBuf, HarnessStore) {
    let firm_home = team_test_firm_home(label);
    let root = firm_home.join("execution-spaces").join("space");
    let store = HarnessStore::new(&root);
    store.init().expect("initialize store");
    (firm_home, store)
}

/// Lock class, rule 2: a decider that runs **while the Execution Space write
/// lock is held** resolves the node file and takes no lease lock.
///
/// This is the class ADR 0075 calls the riskiest edit in the slice — the
/// TeamSupervisorLease parent fences at `store_node_runtime.rs:125`, `:567` and
/// `:701`, plus `trust_foundation.rs:298`, `fabric_runtime_commands.rs:119`,
/// `node_daemon_shutdown.rs:41` and `node_daemon_predecessor.rs:360`. All seven
/// decide under the Space lock; all seven must read the document without
/// nesting the two locks.
///
/// **Red proof.** The debug lock registry is armed in this binary — see
/// `the_registry_is_armed_in_this_test_binary` below, which panics by name on a
/// deliberate violation. So if the resolver took the lease lock, this test
/// would not merely fail, it would panic naming both locks. That is what makes
/// "resolves without the lease lock" an assertion rather than a claim.
#[test]
fn a_decider_under_the_space_write_lock_resolves_the_node_file_lock_free() {
    let (_home, store) = cutover_store("decider-under-space-lock");
    let lease = store
        .acquire_machine_lease(NODE, "node-daemon:decider", "instance-a", 60_000, &[])
        .expect("acquire the machine lease");

    let space_lock = store.acquire_write_lock().expect("Space write lock");
    // Exactly what every Space-locked fence does with the lease it resolved.
    let resolved = store
        .authoritative_machine_lease(NODE)
        .expect("a Space-locked decider resolves the machine lease");
    assert_eq!(resolved.lease(), &lease);
    // And the Option-shaped form the sites with their own refusal use.
    assert_eq!(
        store
            .current_authorized_machine_lease(NODE)
            .expect("the Option form resolves under the Space lock too"),
        Some(lease)
    );
    drop(space_lock);
}

/// Lock class, rule 2, on the **real writer that violated it**.
///
/// The first recover-predecessor cut published the machine document from inside
/// the Space-locked body. The registry panicked by name in 0.2 s instead of
/// deadlocking under load, which is the incident that made the two-phase shape
/// non-negotiable: gather the settlement proof and settle the Space's sessions
/// under the Space write lock, return, then take the lease lock and publish.
/// The load-bearing element is that function split — the publish lives in the
/// caller, past the locked body's return — not the explicit `drop(_lock)` at
/// the end of the locked body, which only shortens the Space lock's hold by
/// the width of the return (wording corrected in E2a-2b, #993).
///
/// This test drives that exact writer end to end with the registry armed.
///
/// **Red proof**: move the publish into the Space-locked body — that is,
/// release the machine lease while the Space lock is still held — and this
/// test panics with "ADR 0075 lock rule: … was requested while this thread
/// holds an Execution Space .store.lock". It is not an assertion failure, so
/// it is written down here rather than left to be rediscovered.
#[test]
fn the_space_locked_recovery_writer_publishes_outside_the_space_lock() {
    let (_home, store) = cutover_store("recovery-writer-lock-rule");
    let instance = format!("{}:1:dead-daemon", std::process::id());
    store
        .insert_execution_node(&firm_core::ExecutionNode {
            id: NODE.into(),
            display_name: "Recovery writer Node".into(),
            status: firm_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");
    let dead = store
        .acquire_machine_lease(NODE, "dead-daemon", &instance, 1, &[])
        .expect("seed the predecessor");
    std::thread::sleep(Duration::from_millis(5));

    let now = current_store_unix_ms();
    let context = firm_core::agentfirm_api::MutationContext {
        execution_space_id: "cutover-lock-rule-space".into(),
        authenticated_actor: firm_core::agentfirm_api::ActorRef {
            kind: firm_core::agentfirm_api::ActorKind::Service,
            id: NODE.into(),
        },
        authority_actor: None,
        command_name: "node_daemon.predecessor_recover".into(),
        idempotency_key: "cutover-lock-rule".into(),
        expected_version: dead.generation,
        request_fingerprint: None,
    };
    let recovered = store
        .recover_node_daemon_predecessor(
            &context,
            NODE,
            "dead-daemon",
            dead.generation,
            &instance,
            true,
            true,
            "test:cutover-lock-rule",
            now,
            &format!("unix-ms:{now}"),
        )
        .expect("the real Space-locked recovery writer completes");

    assert!(!recovered.already_released);
    assert_eq!(
        recovered.lease.status,
        firm_core::NodeDaemonLeaseStatus::Released
    );
    // The publish landed on the document, and the history carries the
    // transition — so the phase that runs outside the Space lock really ran.
    let (current, source) = store
        .current_machine_lease(NODE)
        .expect("read the machine lease")
        .expect("the document is still there");
    assert_eq!(source, MachineLeaseSource::NodeFile);
    assert_eq!(current.status, firm_core::NodeDaemonLeaseStatus::Released);
    assert!(store
        .machine_generation_was_released(NODE, "dead-daemon", dead.generation)
        .expect("read the history"));
}

/// The registry is compiled out in release builds, so "protected structurally"
/// means "caught by debug test runs on paths a test exercises". This test
/// exists so the two above cannot pass merely because the registry was off:
/// here the violation is deliberate, and it must panic by name.
#[test]
#[should_panic(expected = "ADR 0075 lock rule")]
fn the_registry_is_armed_in_this_test_binary() {
    let (home, store) = cutover_store("registry-armed");
    let node_home = home.join("nodes").join(NODE);
    std::fs::create_dir_all(&node_home).expect("node home");
    let _space_lock = store.acquire_write_lock().expect("Space write lock");
    // Rule 2, the direction the recovery writer violated.
    let _ = crate::node_lease_lock::NodeLeaseLock::acquire(&node_home, Duration::from_millis(500));
}

/// Legacy-refusal class, store-internal fence.
///
/// A pre-cutover Store has lease rows and no document. Every fence inside
/// `firm-store` must refuse it by name rather than act on it — and must refuse
/// it as an `Err`, never as "no daemon here", because reading a legacy row as
/// absence is exactly how a fence would quietly stop fencing.
#[test]
fn a_legacy_space_row_cannot_reach_a_store_internal_fence() {
    let (_home, store) = cutover_store("legacy-row-store-fence");
    store
        .insert_execution_node(&firm_core::ExecutionNode {
            id: NODE.into(),
            display_name: "Pre-cutover Node".into(),
            status: firm_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");
    let row = store
        .acquire_node_daemon_lease(NODE, "legacy-daemon", "legacy-instance", 1_000, 600_000)
        .expect("a pre-cutover Store still writes its Space row");

    // It resolves, and it says what it is.
    let (resolved, source) = store
        .current_machine_lease(NODE)
        .expect("a legacy row is readable")
        .expect("a legacy row is present");
    assert_eq!(resolved, row);
    assert_eq!(source, MachineLeaseSource::LegacySpaceRow);
    assert!(!source.authorizes_provider_effect());

    // The fence form refuses it.
    let refusal = store
        .authoritative_machine_lease(NODE)
        .expect_err("a legacy row can never authorize a provider effect");
    assert!(refusal.is_machine_lease_unresolved(), "{refusal}");
    assert!(
        refusal
            .to_string()
            .contains("MACHINE_LEASE_NOT_AUTHORITATIVE"),
        "{refusal}"
    );

    // And the Option form refuses it as an Err, not as `None`. This is the
    // assertion that matters: `Ok(None)` here would read as "no daemon owns
    // this machine" at every site that handles benign absence.
    let option_form = store
        .current_authorized_machine_lease(NODE)
        .expect_err("a legacy row is never a benign absence");
    assert!(option_form.is_machine_lease_unresolved(), "{option_form}");
    assert!(
        store
            .current_authorized_machine_lease("11111111-1111-4111-8111-1111111111ff")
            .expect("a node with no record at all")
            .is_none(),
        "only a genuinely absent lease may be None"
    );
}

/// The mechanical half of the 46-site checklist: nothing in production may read
/// the legacy lease ledger unless it is named here with a reason.
///
/// A grep for `latest_node_daemon_lease` finds three of ADR 0075's decider
/// sites and silently misses the rest, which is why the ADR had to enumerate
/// them by hand. The inverse is mechanical and therefore worth automating: once
/// the cutover has moved every decider, any *remaining* read is either a
/// projection, the compactor, an accessor, or a bug — and a new one added later
/// should turn this red rather than ship as a silent fallback to a record
/// nothing writes.
///
/// Counts are part of the allowlist deliberately. A second read added to an
/// already-listed file is exactly the regression this gate is for.
#[test]
fn no_production_decider_reads_the_legacy_lease_ledger_outside_the_named_exclusions() {
    // (path relative to the repository root, expected reads, why it is allowed)
    const ALLOWED: &[(&str, usize, &str)] = &[
        (
            "crates/firm-store/src/store_read_models.rs",
            4,
            "the two legacy accessors themselves; every exclusion below calls one",
        ),
        (
            "crates/firm-store/src/store_jsonl.rs",
            1,
            "compact_node_daemon_leases_unlocked — bounds the legacy ledger; retires with the per-Space writers in E2b",
        ),
        (
            "crates/firm-store/src/store_node_runtime.rs",
            5,
            "the legacy per-Space acquire/renew/drain/release quartet; kept decodable until E2b, called by no fence",
        ),
        (
            "crates/firm-store/src/store_host_runtime_binding.rs",
            1,
            "host-binding projection input; reads and never refuses",
        ),
        (
            "crates/firm-store/src/store_machine_lease.rs",
            2,
            "current_machine_lease's legacy fallback (which types the row as LegacySpaceRow) and the cutover mint in seed_machine_authority_for_test",
        ),
        (
            "crates/firm-cli/src/main_modules/dashboard_projection.rs",
            3,
            "dashboard lease projection; displays pre-cutover rows by design",
        ),
        (
            "crates/firm-cli/src/main_modules/http_get_routes.rs",
            2,
            "HTTP lease projection; displays pre-cutover rows by design",
        ),
        (
            "crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs",
            1,
            "the cutover mint: the first document must start above every generation any Space ever issued (ADR 0075 Migration 3); decides nothing and refuses nothing",
        ),
    ];

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf();
    let mut found: std::collections::BTreeMap<String, usize> = Default::default();
    let mut stack = vec![repo_root.join("crates")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read crates directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let relative = path
                .strip_prefix(&repo_root)
                .expect("path under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_surface(&relative) {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read source file");
            // Occurrences, not lines: one RoleView surface builds its whole
            // JSON on a single line and reads the ledger twice there.
            let reads = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .map(|line| {
                    line.matches("latest_node_daemon_lease").count()
                        + line.matches("read_jsonl::<NodeDaemonLease>").count()
                })
                .sum::<usize>();
            if reads > 0 {
                found.insert(relative, reads);
            }
        }
    }

    let allowed: std::collections::BTreeMap<&str, (usize, &str)> = ALLOWED
        .iter()
        .map(|(path, count, reason)| (*path, (*count, *reason)))
        .collect();
    let mut problems = Vec::new();
    for (path, reads) in &found {
        match allowed.get(path.as_str()) {
            Some((expected, reason)) if expected == reads => {}
            Some((expected, reason)) => problems.push(format!(
                "{path}: {reads} legacy lease read(s), expected {expected} ({reason}). \
                 If the new read is a decider it must use `current_authorized_machine_lease` \
                 or `authoritative_machine_lease` (ADR 0075); if it is a projection, update \
                 the count here with its reason."
            )),
            None => problems.push(format!(
                "{path}: {reads} legacy lease read(s) with no recorded reason. A production \
                 site that reads the lease and refuses on it must read the node file \
                 (ADR 0075); a site that only displays it belongs in this allowlist with a \
                 one-line reason."
            )),
        }
    }
    for (path, (_, reason)) in &allowed {
        if !found.contains_key(*path) {
            problems.push(format!(
                "{path}: allowlisted for \"{reason}\" but reads the legacy ledger no more. \
                 Remove the entry so the list keeps meaning something."
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `#[cfg(test)]`, `#[cfg(feature = "test-support")]` and `tests/` sites are
/// excluded from the sweep for the reason ADR 0075 gives: a fence a test helper
/// bypasses is not a fence anyone ships.
fn is_test_surface(relative: &str) -> bool {
    relative.contains("/tests/")
        || relative.contains("/lib_tests/")
        || relative.contains("/main_tests/")
        || relative.contains("/trust_kernel_tests/")
        || relative.contains("/daemon_integration_tests/")
        || relative.contains("/daemon_cli_tests/")
        || relative.ends_with("_tests.rs")
        || relative.ends_with("test_support.rs")
        || relative.ends_with("/fixtures.rs")
}

/// Fault injection at the atomic replace, asserting the **narrowed** durability
/// claim ADR 0075 now states rather than the absolute one it first claimed.
///
/// The directory fsync is deliberately best-effort: by the time it runs, the
/// rename has already succeeded, so the new document is already current for
/// every reader on the machine. Reporting a failed directory fsync as a write
/// failure would tell the caller its write did not land when it did, and the
/// caller's only honest response — retry — would republish a document that is
/// already in place. So the claim this test pins is the true, weaker one:
///
/// * a publish that **cannot stage** leaves the previous document and its
///   history byte-for-byte intact — nothing half-landed;
/// * a publish that **returns Ok** has already renamed, so the document on disk
///   carries the new content and no staging file survives, whatever the
///   directory fsync did afterwards.
///
/// The failing-fsync branch itself is unreachable without a seam into `fsync`,
/// and adding one would test the standard library rather than this code. What
/// is assertable, and what actually makes best-effort correct, is that the
/// rename has already landed when that fsync runs — which is the second bullet.
#[test]
fn a_failed_stage_leaves_the_previous_document_intact_and_a_returned_ok_has_already_renamed() {
    let (home, store) = cutover_store("atomic-replace-fault");
    let first = store
        .acquire_machine_lease(NODE, "node-daemon:atomic", "instance-a", 60_000, &[])
        .expect("acquire the machine lease");
    let node_home = home.join("nodes").join(NODE);
    let document_path = node_home.join("node-daemon-lease.json");
    let history_path = node_home.join("node-daemon-lease-history.jsonl");
    let tmp_path = node_home.join("node-daemon-lease.json.tmp");

    // A publish that returns Ok has already renamed: the final path carries the
    // new content and the staging file is gone.
    assert!(!tmp_path.exists());
    let document_before = std::fs::read(&document_path).expect("read the document");
    assert!(
        String::from_utf8_lossy(&document_before).contains("instance-a"),
        "the rename landed before the write returned"
    );
    let history_before = std::fs::read(&history_path).expect("read the history");

    // Now make staging fail, inside the lease lock, after the decision and
    // before any rename could happen.
    std::fs::create_dir(&tmp_path).expect("block the staging file");
    let refusal = store
        .renew_machine_lease(
            NODE,
            "node-daemon:atomic",
            first.generation,
            "instance-a",
            60_000,
        )
        .expect_err("a publish that cannot stage must not half-land");
    assert!(
        matches!(refusal, StoreError::Io(_)),
        "unexpected refusal: {refusal}"
    );

    // Nothing half-landed: previous document and history byte-for-byte intact.
    assert_eq!(std::fs::read(&document_path).unwrap(), document_before);
    assert_eq!(std::fs::read(&history_path).unwrap(), history_before);
    assert_eq!(
        store
            .current_authorized_machine_lease(NODE)
            .expect("the previous document still resolves")
            .expect("the previous document is still there"),
        first
    );

    // And the failure is not permanent: with staging possible again, the very
    // next renewal lands and the document moves forward exactly once.
    std::fs::remove_dir(&tmp_path).expect("clear the staging fault");
    let renewed = store
        .renew_machine_lease(
            NODE,
            "node-daemon:atomic",
            first.generation,
            "instance-a",
            90_000,
        )
        .expect("the next publish stages, renames and returns");
    assert_eq!(renewed.generation, first.generation);
    assert!(renewed.expires_unix_ms > first.expires_unix_ms);
    assert!(
        !tmp_path.exists(),
        "the staging file never survives a publish"
    );
    assert_eq!(
        std::fs::read(&history_path).unwrap(),
        history_before,
        "a renewal is not a generation transition, so it appends no history row"
    );
}

/// E2a-2b (#993): the collapse of the per-Space walks is mechanical, so it is
/// pinned mechanically. After the cutover, every registered Space's Store
/// resolves the SAME machine document, so a loop that renews, releases, or
/// lease-checks once per Space is N answers to one question. The heartbeat is
/// one loop, the release is one publish, and settlement reads the lease once
/// (the settlement itself stays per-Space: Sessions are Space data).
///
/// **Red proof**: this test fails on the pre-collapse revision — the release
/// published inside `for (space, store) in spaces`, the heartbeat fanned out
/// one `scope.spawn` worker per held Space, and settlement re-read the lease
/// inside its per-Space loop.
#[test]
fn the_heartbeat_release_and_settlement_ask_the_machine_question_once() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf();
    let source = std::fs::read_to_string(
        repo_root.join("crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs"),
    )
    .expect("read machine_authority.rs");

    let release = function_body(&source, "fn release_node_authorities");
    assert_eq!(
        release.matches("release_machine_lease(").count(),
        1,
        "the machine release is ONE publish on the one document, not one per Space"
    );
    assert!(
        !release.contains("for (space"),
        "the release must not walk Spaces to publish one document"
    );

    for heartbeat in [
        "fn run_held_node_authorities",
        "fn refresh_held_node_authorities",
    ] {
        let body = function_body(&source, heartbeat);
        assert!(
            !body.contains("scope.spawn"),
            "{heartbeat} is the ONE machine heartbeat — no per-Space renewal workers"
        );
    }

    let settle = function_body(&source, "fn settle_node_authorities_for_shutdown");
    assert_eq!(
        settle.matches("current_authorized_machine_lease(").count(),
        1,
        "settlement asks the machine question once; only the Session settlement stays per-Space"
    );
}

/// The body of a `fn` item, braces matched, so the assertions above bind to
/// one function and not to whatever a neighbour happens to contain.
fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} must exist"));
    let rest = &source[start..];
    let open = rest.find('{').expect("function body opens") + 1;
    let mut depth = 1usize;
    for (offset, ch) in rest[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("{signature}: unbalanced braces");
}

/// E2a-2b (#993): the `*_for_test` machine-lease helpers are the only
/// remaining callers of the retired per-Space writer quartet. Ungated
/// `#[doc(hidden)] pub` kept that quartet reachable from a PRODUCTION build;
/// the gate `cfg(any(test, feature = "test-support"))` is what confines them
/// to test code, so the gate itself is pinned here rather than by review.
///
/// **Red proof**: on the pre-E2a-2b revision each helper was `#[doc(hidden)]
/// pub fn` with no cfg — the assertions below fail on all four.
#[test]
fn the_test_only_machine_lease_helpers_are_cfg_gated_out_of_production() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf();
    let source =
        std::fs::read_to_string(repo_root.join("crates/firm-store/src/store_machine_lease.rs"))
            .expect("read store_machine_lease.rs");
    for helper in [
        "seed_machine_authority_for_test",
        "drain_machine_authority_for_test",
        "release_machine_authority_for_test",
        "expire_machine_lease_for_test",
    ] {
        let at = source
            .find(&format!("pub fn {helper}"))
            .unwrap_or_else(|| panic!("{helper} must exist"));
        let before = &source[..at];
        let attribute_line = before
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .expect("an attribute line above the helper");
        assert_eq!(
            attribute_line.trim(),
            "#[cfg(any(test, feature = \"test-support\"))]",
            "{helper} must be gated out of production builds, not merely doc-hidden"
        );
    }
}
