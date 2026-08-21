use super::*;

#[test]
fn pre_cutover_member_run_materialization_tolerance_is_field_generic() {
    let harness = TestStore::new("pre-cutover-field-generic");
    let host = human("host");
    let team_run = seed_team(
        &harness.store,
        "pre-cutover-field-generic",
        &[
            "member-pre-cutover-last-event",
            "member-pre-cutover-native-session",
            "member-divergent-native",
        ],
    );

    // The PR #486 shape on `last_event_at`: canonical None, legacy Some.
    let mut canonical = member_run(
        "runtime-pre-cutover-last-event",
        "member-pre-cutover-last-event",
        &team_run.id,
        false,
    );
    canonical.last_event_at = None;
    let mut runtime = runtime_member_run(&canonical, "Member member-pre-cutover-last-event");
    runtime.last_event_at = Some("t-legacy-event".into());
    admit_existing_member_run(&harness.store, &host, canonical, runtime)
        .expect("admit pre-cutover-shaped last_event_at MemberRun");

    // The same pre-cutover shape on `native_session` must be tolerated too,
    // without naming the field anywhere in the parity rule.
    let mut canonical = member_run(
        "runtime-pre-cutover-native-session",
        "member-pre-cutover-native-session",
        &team_run.id,
        false,
    );
    canonical.native_session = None;
    let mut runtime = runtime_member_run(&canonical, "Member member-pre-cutover-native-session");
    runtime.native_session = Some(
        serde_json::from_value(
            serde_json::to_value(native_session("session-legacy-native"))
                .expect("serialize session"),
        )
        .expect("map session"),
    );
    admit_existing_member_run(&harness.store, &host, canonical, runtime)
        .expect("admit pre-cutover-shaped native_session MemberRun");

    // One materialization pass proves the generic rule for both fields.
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
        .expect("canonical=None + legacy=Some must be tolerated for ANY field");
    assert_eq!(scope, SPACE);

    // A both-Some divergence on `native_session` still fails closed.
    let mut divergent_canonical = member_run(
        "runtime-divergent-native",
        "member-divergent-native",
        &team_run.id,
        true,
    );
    divergent_canonical.native_session = Some(native_session("session-canonical-native"));
    let mut divergent_runtime =
        runtime_member_run(&divergent_canonical, "Member member-divergent-native");
    divergent_runtime.native_session = Some(
        serde_json::from_value(
            serde_json::to_value(native_session("session-legacy-native-other"))
                .expect("serialize session"),
        )
        .expect("map session"),
    );
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
        .expect_err("both-Some-divergent native_session must still fail closed")
        .to_string();
    assert!(
        error.contains("MEMBER_RUN_MATERIALIZATION_MISMATCH"),
        "{error}"
    );
    assert!(error.contains("native_session"), "{error}");
}
