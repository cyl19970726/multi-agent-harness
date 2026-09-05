use super::*;

#[cfg(unix)]
#[test]
fn host_close_contention_is_bounded_or_machine_fenced_without_writing() {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    let home = TempHome::new("team-run-close-store-busy");
    let project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-close-store-busy"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            // The serve's own product bound for one store write-lock
            // acquisition attempt (debug-only override, firm-store
            // store_write_lock_policy); every budget below derives from it.
            ("FIRM_TEST_STORE_WRITE_LOCK_TIMEOUT_MS", "30"),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
            ("FAKE_CODEX_AUTO_COMPLETE", "1"),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise bounded Host close contention",
            "members": [{"name": "close-busy", "role": "observer", "provider": "codex"}]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    // Phases 'member binds a native session' and 'member completes its first
    // turn': gate on the serve's own durable record (the member_runs
    // ledger), never on snapshot-build cadence. There is no product bound
    // for provider startup, so each budget is a wedge detector derived from
    // the fixture's own supervisor idle cadence
    // (FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS=10000, 10s per cycle): 300s is
    // 30 stalled cycles and ~100x the isolated startup (~3s) — exhaustion
    // means a genuine wedge, and the failure names the phase, the elapsed
    // time and the last ledger row it observed.
    let mut last_member_row = serde_json::Value::Null;
    let mut wait_member = |phase: &str,
                           ready: &dyn Fn(&serde_json::Value) -> bool,
                           started_at: std::time::Instant|
     -> serde_json::Value {
        loop {
            let row = firm_env::store_jsonl_rows(&home, &project_id, "member_runs.jsonl")
                .into_iter()
                .find(|member| member["id"].as_str() == Some(member_id.as_str()));
            if let Some(row) = row {
                if ready(&row) {
                    return row;
                }
                last_member_row = row;
            }
            assert!(
                started_at.elapsed() < Duration::from_secs(300),
                "phase '{phase}' exceeded its hard deadline: {:?} elapsed; last member_runs row: {last_member_row}",
                started_at.elapsed()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let session_bound_at = std::time::Instant::now();
    wait_member(
        "member binds a native session",
        &|member| {
            member["native_session"]["native_session_id"]
                .as_str()
                .is_some()
        },
        session_bound_at,
    );
    let first_turn_at = std::time::Instant::now();
    wait_member(
        "member completes its first turn and goes idle",
        &|member| member["status"].as_str() == Some("idle"),
        first_turn_at,
    );

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let close_rows_before = store
        .team_member_close_requests()
        .expect("read initial Close rows")
        .len();

    // Phase 'acquire deterministic Store contention lock': gate on the held
    // flock itself. The serve's per-write lock acquisition is bounded at
    // 30ms (FIRM_TEST_STORE_WRITE_LOCK_TIMEOUT_MS above); 10s is >300x that
    // product bound, so exhaustion means a wedged store, not scheduling
    // jitter, and fails with the phase name and elapsed time.
    let lock_path = home.spaces_dir().join(&project_id).join(".store.lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .expect("open project Store lock");
    let lock_acquire_started = std::time::Instant::now();
    loop {
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        assert!(
            lock_acquire_started.elapsed() < Duration::from_secs(10),
            "phase 'acquire deterministic Store contention lock' exceeded its hard deadline: {:?} elapsed",
            lock_acquire_started.elapsed()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Phase 'bounded close under the held lock': the response itself is the
    // gate. The test holds the flock through the whole request, so the
    // serve's write attempt must either fail its 30ms bound (503 store_busy,
    // retryable) or fence on machine authority (400) — never block, never
    // write the durable Close latch.
    let (status, outcome) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "deterministic contention"}),
    );
    assert_eq!(outcome["ok"], false);
    if status == 503 {
        assert_eq!(outcome["error"], "store_busy");
        assert_eq!(outcome["retryable"], true);
    } else {
        assert_eq!(status, 400, "body: {outcome}");
        assert_ne!(outcome["error"], "store_busy");
    }
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    drop(lock);
    assert_eq!(
        store
            .team_member_close_requests()
            .expect("read Close rows after authority loss")
            .len(),
        close_rows_before,
        "bounded contention or machine authority loss must fence Close before its durable latch"
    );

    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let member = snapshot["member_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["id"].as_str() == Some(member_id.as_str()))
        .expect("member after exhausted close");
    assert_eq!(member["coordination_status"], "active");
}
