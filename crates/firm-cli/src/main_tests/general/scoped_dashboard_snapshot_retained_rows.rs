use super::*;
use harness_core::CurrentWorkDraft;

fn create_unrelated_run_with_work(store: &HarnessStore, index: usize) -> CreatedTeamRun {
    let created = create_team_run(
        store,
        None,
        None,
        None,
        &format!("Unrelated projection fixture {index}"),
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &[
            TeamMemberSpec {
                agent_member_id: format!("bounded-worker-{index}"),
                name: format!("Bounded Worker {index}"),
                role: "worker".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
            TeamMemberSpec {
                agent_member_id: "host".into(),
                name: format!("Bounded Host {index}"),
                role: "host".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
        ],
    )
    .expect("create unrelated TeamRun");
    insert_fixture_work(
        store,
        &created,
        &format!("unrelated-projection-work-{index}"),
        &format!("unrelated-projection-work-event-{index}"),
    );
    created
}

fn insert_fixture_work(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    work_id: &str,
    event_id: &str,
) {
    let mut draft = CurrentWorkDraft::new(
        work_id.into(),
        created.team_run.id.clone(),
        created.team_run.agent_team_id.clone(),
        format!("Projection fixture {work_id}"),
        "Exercise scoped projection".into(),
        "The selected projection stays identical".into(),
        WorkClaimMode::HostAssign,
        WorkPriority::Normal,
        compatibility_team_actor("host", "test"),
        "unix-ms:1000".into(),
    );
    draft.eligible_member_ids = vec![created.member_runs[0].agent_member_id.clone()];
    store
        .insert_work(
            draft.into_work(),
            WorkCommandContext {
                event_id: event_id.into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: event_id.into(),
                created_at: "unix-ms:1000".into(),
                duplicate_ok: false,
            },
        )
        .expect("insert fixture Work");
}

fn append_jsonl<T: serde::Serialize>(store: &HarnessStore, name: &str, row: &T) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join(name))
        .expect("open fixture ledger");
    serde_json::to_writer(&mut file, row).expect("serialize fixture row");
    file.write_all(b"\n").expect("terminate fixture row");
    file.sync_all().expect("persist fixture row");
}

#[test]
fn retained_projected_rows_remain_bounded_with_200_unrelated_team_runs_and_works() {
    let (store, root) = temp_store("scoped-dashboard-retained-row-bound");
    let selected = create_two_member_team_run(&store);
    insert_fixture_work(
        &store,
        &selected,
        "bounded-selected-work",
        "bounded-selected-work-event",
    );

    // A second equivalence fixture exercises several real unrelated Teams and
    // Works through both projection paths before the cheaper 200-row scale
    // fixture is appended.
    for index in 0..3 {
        create_unrelated_run_with_work(&store, index);
    }
    let mut filtered_global = dashboard_team_run_snapshot_via_global(&store, &selected.team_run.id)
        .expect("filter global multi-run snapshot");
    let mut scoped = dashboard_team_run_snapshot(&store, &selected.team_run.id)
        .expect("resolve scoped multi-run snapshot");
    filtered_global["generated_at"] = serde_json::Value::Null;
    scoped["generated_at"] = serde_json::Value::Null;
    assert_eq!(
        scoped, filtered_global,
        "scoped snapshot must equal the filtered global snapshot byte-for-byte"
    );

    let baseline_counts = projected_row_counts(&scoped);
    let seed_run = selected.team_run.clone();
    let seed_operation = store
        .work_operations()
        .expect("read seed Work operation")
        .into_iter()
        .find(|operation| operation.work.id == "bounded-selected-work")
        .expect("seed Work operation exists");
    for index in 0..200 {
        let mut run = seed_run.clone();
        run.id = format!("raw-unrelated-team-run-{index}");
        run.objective = format!("Raw unrelated TeamRun {index}");
        run.member_run_ids.clear();
        append_jsonl(&store, "team_runs.jsonl", &run);

        let mut operation = seed_operation.clone();
        operation.work.id = format!("raw-unrelated-work-{index}");
        operation.work.team_run_id = run.id.clone();
        operation.event.id = format!("raw-unrelated-work-event-{index}");
        operation.event.work_id = operation.work.id.clone();
        operation.event.team_run_id = run.id;
        operation.event.idempotency_key = operation.event.id.clone();
        append_jsonl(&store, "work_operations.jsonl", &operation);
    }

    let raw_started = std::time::Instant::now();
    let raw_team_run_rows = store.team_runs().expect("deserialize TeamRun ledger").len();
    let raw_work_rows = store
        .work_operations()
        .expect("deserialize Work ledger")
        .len();
    let raw_deserialization_elapsed = raw_started.elapsed();

    let scoped_started = std::time::Instant::now();
    let after = dashboard_team_run_snapshot(&store, &selected.team_run.id)
        .expect("resolve scoped snapshot after unrelated rows");
    let scoped_total_elapsed = scoped_started.elapsed();
    let after_counts = projected_row_counts(&after);
    assert_eq!(
        after_counts, baseline_counts,
        "retained/projected target rows must not grow with unrelated TeamRuns or Works"
    );

    let projection_estimate = scoped_total_elapsed.saturating_sub(raw_deserialization_elapsed);
    eprintln!(
        "SCOPED_RETAINED_PROJECTION_BOUND target_projected_rows={} unrelated_team_runs=203 unrelated_works=203 raw_team_run_rows={} raw_work_rows={} raw_deserialization_ms={} scoped_total_ms={} projection_after_raw_probe_ms={} raw_jsonl_deserialization_scales_with_store=true",
        after_counts.values().sum::<usize>(),
        raw_team_run_rows,
        raw_work_rows,
        raw_deserialization_elapsed.as_millis(),
        scoped_total_elapsed.as_millis(),
        projection_estimate.as_millis(),
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
