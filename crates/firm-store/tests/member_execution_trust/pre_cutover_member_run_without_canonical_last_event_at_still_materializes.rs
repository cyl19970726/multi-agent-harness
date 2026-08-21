use super::*;

#[test]
fn pre_cutover_member_run_without_canonical_last_event_at_still_materializes() {
    let harness = TestStore::new("pre-cutover-last-event-at");
    let host = human("host");
    let team_run = seed_team(
        &harness.store,
        "pre-cutover-last-event-at",
        &["member-pre-cutover", "member-divergent"],
    );

    // Seed the known pre-cutover (DOC-108) migration shape: the canonical
    // MemberRun materialized without `last_event_at` while the legacy
    // ProviderRuntimeProjection kept advancing one.
    let mut canonical = member_run(
        "runtime-pre-cutover",
        "member-pre-cutover",
        &team_run.id,
        false,
    );
    canonical.last_event_at = None;
    let mut runtime = runtime_member_run(&canonical, "Member member-pre-cutover");
    runtime.last_event_at = Some("t-legacy-event".into());
    admit_existing_member_run(&harness.store, &host, canonical, runtime)
        .expect("admit pre-cutover-shaped MemberRun");

    let current = harness
        .store
        .team_runs()
        .expect("read TeamRuns")
        .into_iter()
        .rev()
        .find(|candidate| candidate.id == team_run.id)
        .expect("latest TeamRun");
    let scope = harness
        .store
        .current_team_run_execution_space(&current)
        .expect("pre-cutover canonical=None + legacy=Some last_event_at must still materialize");
    assert_eq!(scope, SPACE);

    // A post-cutover both-Some divergence on the same field stays fail-closed.
    let mut divergent_canonical =
        member_run("runtime-divergent", "member-divergent", &team_run.id, false);
    divergent_canonical.last_event_at = Some("t-canonical-event".into());
    let mut divergent_runtime = runtime_member_run(&divergent_canonical, "Member member-divergent");
    divergent_runtime.last_event_at = Some("t-legacy-event".into());
    admit_existing_member_run(
        &harness.store,
        &host,
        divergent_canonical,
        divergent_runtime,
    )
    .expect("admit both-Some-divergent MemberRun");

    let current = harness
        .store
        .team_runs()
        .expect("read TeamRuns")
        .into_iter()
        .rev()
        .find(|candidate| candidate.id == team_run.id)
        .expect("latest TeamRun");
    let error = harness
        .store
        .current_team_run_execution_space(&current)
        .expect_err("both-Some-divergent last_event_at must still fail closed")
        .to_string();
    assert!(
        error.contains("MEMBER_RUN_MATERIALIZATION_MISMATCH"),
        "{error}"
    );
    assert!(error.contains("last_event_at"), "{error}");
}
