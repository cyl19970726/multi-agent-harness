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
    // without naming the field anywhere in the parity rule. Current admission
    // correctly refuses to WRITE this shape (canonical None + legacy Some
    // native_session), so the historical state is seeded the way the store's
    // own `legacy_import_append_*` lib-test pattern reconstructs legacy rows:
    // admit the consistent base (both None) through ordinary admission, then
    // append the raw historical ProviderRuntimeProjection directly to the
    // legacy ledger. Production admission and the reader stay unweakened.
    let mut canonical = member_run(
        "runtime-pre-cutover-native-session",
        "member-pre-cutover-native-session",
        &team_run.id,
        false,
    );
    canonical.native_session = None;
    let runtime = runtime_member_run(&canonical, "Member member-pre-cutover-native-session");
    admit_existing_member_run(&harness.store, &host, canonical.clone(), runtime)
        .expect("admit consistent both-None native_session MemberRun");
    let mut historical = runtime_member_run(&canonical, "Member member-pre-cutover-native-session");
    historical.native_session = Some(
        serde_json::from_value(
            serde_json::to_value(native_session("session-legacy-native"))
                .expect("serialize session"),
        )
        .expect("map session"),
    );
    append_raw_legacy_member_run_row(&harness, &historical);

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

    // A both-Some divergence on `native_session` still fails closed. Current
    // admission likewise refuses to write two different Some identities, so
    // seed a consistent both-Some base and then append the raw historical row
    // whose legacy session diverges; the reader must reject exactly that.
    let mut divergent_canonical = member_run(
        "runtime-divergent-native",
        "member-divergent-native",
        &team_run.id,
        true,
    );
    divergent_canonical.native_session = Some(native_session("session-canonical-native"));
    let divergent_runtime =
        runtime_member_run(&divergent_canonical, "Member member-divergent-native");
    admit_existing_member_run(
        &harness.store,
        &host,
        divergent_canonical.clone(),
        divergent_runtime,
    )
    .expect("admit consistent both-Some native_session MemberRun");
    let mut divergent_historical =
        runtime_member_run(&divergent_canonical, "Member member-divergent-native");
    divergent_historical.native_session = Some(
        serde_json::from_value(
            serde_json::to_value(native_session("session-legacy-native-other"))
                .expect("serialize session"),
        )
        .expect("map session"),
    );
    append_raw_legacy_member_run_row(&harness, &divergent_historical);

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

/// Append one raw historical ProviderRuntimeProjection row directly to the
/// legacy ledger, mirroring the store's own `legacy_import_append_*` lib-test
/// pattern: this reconstructs a pre-cutover row that current admission
/// correctly refuses to write. It is intentionally NOT a current admission
/// path and never materializes or mutates the canonical MemberRun.
fn append_raw_legacy_member_run_row(harness: &TestStore, row: &RuntimeMemberRun) {
    let ledger = harness.root.join("member_runs.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .expect("open legacy member_runs ledger");
    let line = serde_json::to_string(row).expect("serialize historical legacy row");
    writeln!(file, "{line}").expect("append historical legacy row");
    file.sync_all().expect("persist historical legacy row");
}
