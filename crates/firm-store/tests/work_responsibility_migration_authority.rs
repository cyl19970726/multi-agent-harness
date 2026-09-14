//! The responsibility migration's authority and atomicity.
//!
//! `migrate-responsibility` writes ordinary `Updated` WorkOperations, so it
//! needs the exact Host of each Work's own TeamRun, it must refuse terminal
//! Work, and a refusal on any write candidate must leave the ledger untouched.

mod work_responsibility_fixtures;

use firm_core::{TeamActorKind, TeamActorRef, WorkCommandContext};
use work_responsibility_fixtures::*;

/// The two ledger-shaped `Updated` writers never advance a terminal Work.
/// Both read the raw `work_operations.jsonl` fold, so only a pre-cutover
/// terminal row reaches their guard at all.
#[test]
fn terminal_work_refuses_both_ledger_shaped_updated_writers() {
    let fixture = TestStore::new("terminal-updated");
    let store = &fixture.store;
    let run = seed_team(
        store,
        "terminal-updated",
        &["host-terminal-updated", "agent-owner"],
    );
    append_legacy_terminal_work_row(
        store,
        &run.id,
        "host-terminal-updated",
        "work-legacy-terminal",
        Some("agent-owner"),
    );
    let before = work_operations_raw(store);

    let reconcile = store
        .reconcile_work_projection_provenance(
            "work-legacy-terminal",
            1,
            host_work_context("host-terminal-updated", "reconcile-terminal", "t1"),
        )
        .expect_err("provenance recovery cannot advance a closed Work");
    assert!(
        reconcile
            .to_string()
            .contains(firm_store::WORK_TERMINAL_IMMUTABLE),
        "unexpected error: {reconcile}"
    );

    let migrate = store
        .migrate_work_responsibility(
            SPACE,
            None,
            host_work_context("host-terminal-updated", "migrate-terminal", "t2"),
        )
        .expect_err("responsibility migration cannot advance a closed Work");
    assert!(
        migrate
            .to_string()
            .contains(firm_store::WORK_TERMINAL_IMMUTABLE),
        "unexpected error: {migrate}"
    );
    assert_eq!(
        work_operations_raw(store),
        before,
        "a refused Updated writer appends nothing"
    );
}

/// The migration writes ordinary `Updated` WorkOperations, so it needs the
/// exact Host of each Work's own TeamRun — the same authority every other Host
/// Work verb needs. A Host-shaped actor that is not that Host, and a non-Host
/// actor, are both refused before any row is appended.
#[test]
fn responsibility_migration_requires_the_exact_team_run_host() {
    let fixture = TestStore::new("migration-authority");
    let store = &fixture.store;
    let run = seed_team(
        store,
        "migrate-authority",
        &["host-migrate-authority", "worker-migrate-authority"],
    );
    append_legacy_work_row(
        store,
        &run.id,
        "host-migrate-authority",
        "work-legacy-authority",
        Some("worker-migrate-authority"),
        None,
    );
    let before = work_operations_raw(store);

    let operator = store
        .migrate_work_responsibility(
            SPACE,
            None,
            WorkCommandContext {
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::Operator,
                    id: "host-migrate-authority".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                ..host_work_context("host-migrate-authority", "migrate-operator", "t1")
            },
        )
        .expect_err("a non-Host actor cannot migrate responsibility");
    assert!(
        operator.to_string().contains("Host authority is required"),
        "unexpected error: {operator}"
    );

    let impostor = store
        .migrate_work_responsibility(
            SPACE,
            None,
            host_work_context("migration-host", "migrate-impostor", "t2"),
        )
        .expect_err("a Host-shaped actor that is not the TeamRun Host is refused");
    assert!(
        impostor
            .to_string()
            .contains("TEAM_RUN_HOST_AUTHORITY_MISMATCH"),
        "unexpected error: {impostor}"
    );
    assert_eq!(
        work_operations_raw(store),
        before,
        "a refused migration appends nothing"
    );

    let report = store
        .migrate_work_responsibility(
            SPACE,
            None,
            host_work_context("host-migrate-authority", "migrate-exact", "t3"),
        )
        .expect("the exact TeamRun Host migrates");
    assert_eq!(report.migrated_work_ids, ["work-legacy-authority"]);
    // The migration revision is a `work` trust envelope since W4; the legacy
    // ledger file keeps exactly the rows it already held.
    assert_eq!(work_operations_raw(store), before);
    assert_eq!(migration_revisions(store), 1);

    // A scope narrows the sweep without widening the authority rule.
    let scoped = store
        .migrate_work_responsibility(
            SPACE,
            Some("team-run-absent"),
            host_work_context("host-migrate-authority", "migrate-scoped", "t4"),
        )
        .expect("an empty scope is a reported no-op");
    assert!(scoped.migrated_work_ids.is_empty());
    assert!(scoped.entries.is_empty());
}

/// Authority and mutability are checked per Work, so the sweep must prove the
/// whole plan legal before it appends anything. Otherwise a store whose first
/// Work is migratable and whose second is not would be left half migrated with
/// no report to say so.
#[test]
fn responsibility_migration_appends_nothing_when_a_later_work_refuses() {
    let fixture = TestStore::new("migration-atomic");
    let store = &fixture.store;
    let run_a = seed_team(
        store,
        "migrate-atomic-a",
        &["host-atomic-a", "worker-atomic-a"],
    );
    let run_z = seed_team(
        store,
        "migrate-atomic-z",
        &["host-atomic-z", "worker-atomic-z"],
    );

    // Work ids order the sweep: `work-atomic-a` is planned first and would
    // have been appended before `work-atomic-z` refused.
    append_legacy_work_row(
        store,
        &run_a.id,
        "host-atomic-a",
        "work-atomic-a",
        Some("worker-atomic-a"),
        None,
    );
    append_legacy_work_row(
        store,
        &run_z.id,
        "host-atomic-z",
        "work-atomic-z",
        Some("worker-atomic-z"),
        None,
    );
    let before = work_operations_raw(store);

    let refused = store
        .migrate_work_responsibility(
            SPACE,
            None,
            host_work_context("host-atomic-a", "migrate-atomic", "t1"),
        )
        .expect_err("the second TeamRun's Host refuses the sweep");
    assert!(
        refused
            .to_string()
            .contains("TEAM_RUN_HOST_AUTHORITY_MISMATCH"),
        "unexpected error: {refused}"
    );
    assert_eq!(
        work_operations_raw(store),
        before,
        "the first Work must not be migrated when a later one refuses"
    );

    // Scoping to the resolvable run migrates exactly that run's Work.
    let scoped = store
        .migrate_work_responsibility(
            SPACE,
            Some(&run_a.id),
            host_work_context("host-atomic-a", "migrate-atomic-scoped", "t2"),
        )
        .expect("the scoped sweep sees only its own TeamRun");
    assert_eq!(scoped.migrated_work_ids, ["work-atomic-a"]);
    assert_eq!(work_operations_raw(store), before);
    assert_eq!(migration_revisions(store), 1);
}

/// Committed responsibility-migration Work revisions, wherever they live.
fn migration_revisions(store: &firm_store::HarnessStore) -> usize {
    std::fs::read_to_string(store.root().join("agentfirm_trust_operations.jsonl"))
        .map(|contents| {
            contents
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .filter(|row| {
                    row["operation"]["event"]["aggregate_kind"] == "work"
                        && row["operation"]["event"]["payload"]["responsibility_migration"] == true
                })
                .count()
        })
        .unwrap_or_default()
}

/// The sweep plans every write before appending any, so a Work that folds but
/// cannot satisfy the current `Work` contract must refuse the whole migration
/// rather than abort after earlier Works already landed (#946). `Work::validate`
/// runs in the plan phase through `validate_work_operation_records_unlocked`;
/// this pins that it does, with the invalid Work sorting *after* a perfectly
/// migratable one so a plan-phase refusal is the only thing that can keep the
/// ledger untouched.
#[test]
fn migration_validates_every_planned_work_before_appending_any() {
    let fixture = TestStore::new("migrate-invalid-projection");
    let store = &fixture.store;
    let run = seed_team(
        store,
        "migrate-invalid-projection",
        &["host-invalid-projection", "agent-owner"],
    );
    append_legacy_work_row(
        store,
        &run.id,
        "host-invalid-projection",
        "work-legacy-a-valid",
        Some("agent-owner"),
        None,
    );
    append_legacy_work_row_with_invalid_projection(
        store,
        &run.id,
        "host-invalid-projection",
        "work-legacy-b-invalid",
    );
    let before = work_operations_raw(store);

    let error = store
        .migrate_work_responsibility(
            SPACE,
            None,
            host_work_context("host-invalid-projection", "migrate-invalid", "t1"),
        )
        .expect_err("a Work that cannot satisfy the Work contract refuses the sweep");
    assert!(
        error.to_string().contains("INVALID_WORK_PROJECTION"),
        "unexpected error: {error}"
    );
    assert_eq!(
        work_operations_raw(store),
        before,
        "a refused plan appends nothing, including the Work that would have migrated cleanly"
    );
}
