use super::*;

#[cfg(unix)]
#[test]
fn get_and_post_handler_errors_are_json_with_classified_retryability() {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    let home = TempHome::new("serve-handler-errors-json");
    let project_id = init_project(&home, "alpha");
    let credentials = serde_json::json!([{
        "token": "serve-fallback-host-token",
        "actor": {"kind": "agent_member", "id": FIXTURE_HOST_ID},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("FIRM_TEST_STORE_WRITE_LOCK_TIMEOUT_MS", "30"),
            ("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str()),
        ],
    );
    let (create_status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise the connection-loop POST fallback",
            "members": [{
                "name": "fallback-member",
                "role": "observer",
                "provider": "codex",
                "execution_mode": "external_interactive"
            }]
        }),
    );
    assert_eq!(create_status, 200, "create body: {created}");
    let member = &created["result"]["member_runs"][0];
    let member_id = member["id"].as_str().expect("created MemberRun id");
    let lock_path = home.spaces_dir().join(&project_id).join(".store.lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .expect("open project Store lock");
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0,
        "acquire deterministic Store contention lock"
    );

    let headers = [
        ("X-AgentFirm-Token", "serve-fallback-host-token"),
        ("Idempotency-Key", "contended-close"),
        ("If-Match", "1"),
        ("X-AgentFirm-Confirm", "close_member_run"),
    ];
    let close_path = format!("/v1/agentfirm/member-runs/{member_id}/close");
    let (post_status, post_body) = serve.post_json_with_headers(
        &close_path,
        &serde_json::json!({"action": "close_member_run"}),
        &headers,
    );
    assert_retryable_store_busy(post_status, &post_body, "POST");

    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    std::fs::write(
        home.spaces_dir()
            .join(&project_id)
            .join("work_delegation_operations.jsonl"),
        "not-json\n",
    )
    .expect("poison WorkDelegation operation ledger");

    let (get_status, get_body) = serve.get_json("/v1/work-delegations");
    assert_eq!(get_status, 400, "GET body: {get_body}");
    assert_eq!(get_body["ok"], false, "GET body: {get_body}");
    assert!(
        get_body["error"]
            .as_str()
            .is_some_and(|error| error.contains("json")),
        "GET body: {get_body}"
    );
    assert!(
        get_body.get("retryable").is_none(),
        "malformed ledger data must not be presented as retryable: {get_body}"
    );
}

fn assert_retryable_store_busy(status: u16, body: &serde_json::Value, method: &str) {
    assert_eq!(status, 503, "{method} body: {body}");
    assert_eq!(body["ok"], false, "{method} body: {body}");
    assert_eq!(body["error"], "store_busy", "{method} body: {body}");
    assert_eq!(body["retryable"], true, "{method} body: {body}");
}
