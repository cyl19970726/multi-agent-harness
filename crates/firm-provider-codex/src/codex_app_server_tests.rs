use super::*;

fn wait_for_live_turn_terminal(
    client: &CodexAppServerClient,
    expected_turn_id: &str,
    timeout: Duration,
) -> serde_json::Value {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for Codex turn/completed for {expected_turn_id}"
        );
        let frame = client
                .recv(remaining.min(Duration::from_secs(5)))
                .unwrap_or_else(|error| {
                    panic!(
                        "Codex app-server disconnected or timed out before turn/completed for {expected_turn_id}: {error}"
                    )
                });
        if frame.get("id").is_some() && frame.get("method").is_some() {
            panic!("unexpected reverse request during Codex live canary: {frame}");
        }
        if frame.get("method").and_then(serde_json::Value::as_str) != Some("turn/completed") {
            continue;
        }
        let turn = frame
            .pointer("/params/turn")
            .unwrap_or_else(|| panic!("Codex turn/completed omitted turn projection: {frame}"));
        if turn.get("id").and_then(serde_json::Value::as_str) != Some(expected_turn_id) {
            continue;
        }
        return turn.clone();
    }
}

fn assert_live_thread_idle(client: &mut CodexAppServerClient) {
    let thread = client
        .read_thread(true)
        .expect("live Codex thread/read after terminal turn");
    assert_eq!(
        thread
            .pointer("/status/type")
            .and_then(serde_json::Value::as_str),
        Some("idle"),
        "live Codex thread must be idle after the terminal turn: {thread}"
    );
}

fn wait_for_live_command_start(
    client: &CodexAppServerClient,
    expected_turn_id: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for a live Codex command item in {expected_turn_id}"
        );
        let frame = client
                .recv(remaining.min(Duration::from_secs(5)))
                .unwrap_or_else(|error| {
                    panic!(
                        "Codex app-server disconnected or timed out before command start in {expected_turn_id}: {error}"
                    )
                });
        if frame.get("id").is_some() && frame.get("method").is_some() {
            panic!("unexpected reverse request during Codex live canary: {frame}");
        }
        let method = frame.get("method").and_then(serde_json::Value::as_str);
        let frame_turn_id = frame
            .pointer("/params/turnId")
            .or_else(|| frame.pointer("/params/turn/id"))
            .and_then(serde_json::Value::as_str);
        if frame_turn_id.is_some_and(|turn_id| turn_id != expected_turn_id) {
            continue;
        }
        if method == Some("turn/completed") {
            panic!("Codex interrupt target completed before a command item became active: {frame}");
        }
        if method == Some("item/started")
            && frame
                .pointer("/params/item/type")
                .and_then(serde_json::Value::as_str)
                == Some("commandExecution")
        {
            return;
        }
    }
}

/// Exact-version live gate for the provider-neutral Codex Member runtime.
///
/// This is intentionally ignored in ordinary CI because it consumes a
/// signed-in Codex account. It proves native thread creation, one completed
/// cycle, explicit process Close with the thread retained, exact
/// thread/resume, current-cycle interrupt, a later completed cycle on that
/// same native thread, and a second explicit Close.
#[test]
#[ignore = "requires RUN_CODEX_0148_LIVE_CANARY=1 and a signed-in codex-cli 0.148.0-alpha.9"]
fn live_codex_0148_round_interrupt_close_and_same_thread_resume() {
    assert_eq!(
        std::env::var("RUN_CODEX_0148_LIVE_CANARY").as_deref(),
        Ok("1"),
        "set RUN_CODEX_0148_LIVE_CANARY=1 to acknowledge this real-provider canary"
    );
    let version = Command::new("codex")
        .arg("--version")
        .output()
        .expect("probe codex --version");
    assert!(version.status.success(), "codex --version must succeed");
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "codex-cli 0.148.0-alpha.9",
        "live canary evidence is exact-version only"
    );

    let canary_root = std::env::temp_dir().join(format!(
        "star-harness-codex-0148-live-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&canary_root).expect("create isolated Codex canary cwd");
    let options = |resume_thread_id| CodexAppServerSpawnOptions {
        model: None,
        reasoning_effort: None,
        service_tier: None,
        resume_thread_id,
        member_name: "DEV-26 Codex 0.148 runtime canary",
        collaboration_env: &[],
        plan_mode: false,
        sandbox: "danger-full-access",
        approval_policy: "never",
    };

    let mut first = CodexAppServerClient::spawn(&canary_root, options(None))
        .expect("open first live Codex app-server runtime");
    let native_thread_id = first.thread_id().to_string();
    assert!(!native_thread_id.trim().is_empty());
    assert_eq!(
        first.read_thread_goal().expect("inspect native Goal"),
        None,
        "HostDriven canary must not activate a provider-native Goal"
    );
    let first_turn = first
        .start_turn(
            "Reply with exactly DEV26_CODEX_CANARY_ROUND_ONE. Do not use tools or modify files.",
            std::time::Duration::from_secs(120),
        )
        .expect("start first live Codex turn");
    let first_terminal = wait_for_live_turn_terminal(&first, &first_turn, Duration::from_secs(180));
    assert_eq!(
        first_terminal
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("completed"),
        "first live Codex turn must complete: {first_terminal}"
    );
    assert_live_thread_idle(&mut first);
    let first_close = first
        .shutdown_with_receipt()
        .expect("close first live Codex runtime");
    assert!(first_close.process_reaped);
    assert!(first_close.stdout_reader_joined);
    assert!(first_close.thread_id_retained);

    let mut resumed = CodexAppServerClient::spawn(&canary_root, options(Some(&native_thread_id)))
        .expect("resume exact native Codex thread");
    assert_eq!(resumed.thread_id(), native_thread_id);
    let interrupted_turn = resumed
            .start_turn(
                "Use the shell tool to run exactly `sleep 30`, wait for it to finish, then reply with DEV26_CODEX_INTERRUPT_TARGET. Do not perform any other action.",
                std::time::Duration::from_secs(120),
            )
            .expect("start interrupt target turn");
    wait_for_live_command_start(&resumed, &interrupted_turn, Duration::from_secs(90));
    resumed
        .interrupt(&interrupted_turn)
        .expect("interrupt current live Codex turn");
    let interrupted_terminal =
        wait_for_live_turn_terminal(&resumed, &interrupted_turn, Duration::from_secs(60));
    assert_eq!(
        interrupted_terminal
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("interrupted"),
        "turn/interrupt must converge to matching terminal evidence: {interrupted_terminal}"
    );
    assert_live_thread_idle(&mut resumed);

    let resumed_turn = resumed
        .start_turn(
            "Reply with exactly DEV26_CODEX_CANARY_ROUND_TWO. Do not use tools or modify files.",
            std::time::Duration::from_secs(120),
        )
        .expect("start post-interrupt live Codex turn");
    let resumed_terminal =
        wait_for_live_turn_terminal(&resumed, &resumed_turn, Duration::from_secs(180));
    assert_eq!(
        resumed_terminal
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("completed"),
        "same-thread post-interrupt turn must complete: {resumed_terminal}"
    );
    assert_live_thread_idle(&mut resumed);
    let second_close = resumed
        .shutdown_with_receipt()
        .expect("close resumed live Codex runtime");
    assert!(second_close.process_reaped);
    assert!(second_close.stdout_reader_joined);
    assert!(second_close.thread_id_retained);
}

#[test]
fn new_thread_uses_the_explicit_frozen_permission_mapping() {
    let params = thread_open_params(
        Path::new("/tmp/project"),
        Some("gpt-test"),
        Some("max"),
        Some("priority"),
        None,
        "workspace-write",
        "never",
    );

    assert_eq!(params["sandbox"], "workspace-write");
    assert_eq!(params["approvalPolicy"], "never");
    assert_eq!(params["ephemeral"], false);
    assert_eq!(params["config"]["model_reasoning_effort"], "max");
    assert_eq!(
        params["config"]["sandbox_workspace_write"]["network_access"],
        true
    );
    assert_eq!(params["serviceTier"], "priority");
    assert!(params.get("threadId").is_none());
}

#[test]
fn resumed_thread_keeps_the_explicit_frozen_permission_mapping() {
    let params = thread_open_params(
        Path::new("/tmp/project"),
        Some("gpt-test"),
        Some("high"),
        Some("default"),
        Some("thread-123"),
        "read-only",
        "never",
    );

    assert_eq!(params["sandbox"], "read-only");
    assert_eq!(params["approvalPolicy"], "never");
    assert_eq!(params["threadId"], "thread-123");
    assert!(params.get("ephemeral").is_none());
    assert_eq!(params["config"]["model_reasoning_effort"], "high");
    assert!(params["config"].get("sandbox_workspace_write").is_none());
}

#[test]
fn resumed_workspace_write_thread_keeps_local_supervisor_network_access() {
    let params = thread_open_params(
        Path::new("/tmp/project"),
        Some("gpt-test"),
        None,
        Some("default"),
        Some("thread-123"),
        "workspace-write",
        "never",
    );

    assert_eq!(params["sandbox"], "workspace-write");
    assert_eq!(params["approvalPolicy"], "never");
    assert_eq!(params["threadId"], "thread-123");
    assert_eq!(
        params["config"]["sandbox_workspace_write"]["network_access"],
        true
    );
}

#[test]
fn native_thread_name_uses_the_member_identity() {
    assert_eq!(
        thread_name_params("thread-123", "RuntimeFixer"),
        serde_json::json!({
            "threadId": "thread-123",
            "name": "Agent Team · RuntimeFixer"
        })
    );
}

#[test]
fn effective_model_prefers_current_top_level_app_server_shape() {
    let response = serde_json::json!({
        "result": {
            "model": "gpt-current",
            "thread": {"id": "thread-123", "model": "gpt-legacy"}
        }
    });

    assert_eq!(
        effective_thread_model(&response).as_deref(),
        Some("gpt-current")
    );
}

#[test]
fn effective_model_accepts_legacy_nested_but_never_invents_a_requested_receipt() {
    let legacy = serde_json::json!({
        "result": {"thread": {"id": "thread-123", "model": "gpt-legacy"}}
    });
    let omitted = serde_json::json!({
        "result": {"thread": {"id": "thread-123"}}
    });

    assert_eq!(
        effective_thread_model(&legacy).as_deref(),
        Some("gpt-legacy")
    );
    assert_eq!(effective_thread_model(&omitted), None);
}

#[test]
fn requested_reasoning_and_service_controls_require_native_confirmation() {
    let response = serde_json::json!({
        "result": {
            "reasoningEffort": "max",
            "serviceTier": "priority"
        }
    });
    assert_eq!(
        effective_thread_reasoning_effort(&response).as_deref(),
        Some("max")
    );
    assert_eq!(
        effective_thread_service_tier(&response).as_deref(),
        Some("priority")
    );
    require_requested_setting("reasoning effort", Some("max"), Some("max"))
        .expect("matching native receipt");
    assert!(require_requested_setting("service tier", Some("priority"), Some("default")).is_err());
}

#[test]
fn requested_permission_controls_require_effective_native_confirmation() {
    let response = serde_json::json!({
        "result": {
            "approvalPolicy": "never",
            "sandbox": {"type": "workspaceWrite"}
        }
    });
    assert_eq!(
        effective_thread_approval_policy(&response).as_deref(),
        Some("never")
    );
    assert_eq!(
        effective_thread_sandbox_mode(&response).as_deref(),
        Some("workspace-write")
    );
    require_requested_setting("approval policy", Some("never"), Some("never"))
        .expect("matching native approval receipt");
    require_requested_setting(
        "sandbox mode",
        Some("workspace-write"),
        Some("workspace-write"),
    )
    .expect("matching native sandbox receipt");
    assert!(require_requested_setting("approval policy", Some("never"), None).is_err());
    assert!(require_requested_setting(
        "sandbox mode",
        Some("danger-full-access"),
        Some("workspace-write")
    )
    .is_err());
}

#[test]
fn resume_and_steer_receipts_require_exact_native_ids() {
    require_resumed_thread_identity(Some("thread-1"), "thread-1").expect("exact resumed thread");
    require_resumed_thread_identity(None, "thread-new").expect("new thread has no prior id");
    assert!(require_resumed_thread_identity(Some("thread-1"), "thread-other").is_err());

    let exact = serde_json::json!({"result": {"turnId": "turn-1"}});
    assert_eq!(exact_steer_receipt(&exact, "turn-1").unwrap(), "turn-1");
    assert!(exact_steer_receipt(&exact, "turn-other").is_err());
    assert!(exact_steer_receipt(&serde_json::json!({"result": {}}), "turn-1").is_err());
}

#[test]
fn thread_and_goal_receipts_require_the_exact_native_identity_and_status() {
    let thread = serde_json::json!({
        "result": {"thread": {"id": "thread-1", "status": {"type": "idle"}}}
    });
    assert_eq!(
        exact_thread_projection(&thread, "thread-1").unwrap()["status"]["type"],
        "idle"
    );
    assert!(exact_thread_projection(&thread, "thread-2").is_err());

    let goal = serde_json::json!({
        "result": {"goal": {"threadId": "thread-1", "status": "paused", "updatedAt": 7}}
    });
    assert_eq!(
        exact_goal_projection(&goal, "thread-1", Some("paused"))
            .unwrap()
            .unwrap()["updatedAt"],
        7
    );
    assert!(exact_goal_projection(&goal, "thread-1", Some("active")).is_err());
    assert!(exact_goal_projection(&goal, "thread-2", None).is_err());
}

#[test]
fn absent_goal_is_valid_for_inspection_but_never_for_a_set_receipt() {
    let absent = serde_json::json!({"result": {"goal": null}});
    assert_eq!(
        exact_goal_projection(&absent, "thread-1", None).unwrap(),
        None
    );
    assert!(exact_goal_projection(&absent, "thread-1", Some("paused")).is_err());
}

/// Verbatim shape of a live `codex-cli 0.145.0` reply, captured by driving
/// `initialize` + `initialized` + the two account reads over stdio without
/// ever sending `thread/start`.
fn live_capacity_read(used_percent: i64, reached: Option<&str>) -> CodexAccountCapacityRead {
    CodexAccountCapacityRead {
        account: serde_json::json!({
            "account": {"type": "chatgpt", "email": "operator@example.com", "planType": "pro"},
            "requiresOpenaiAuth": true
        }),
        rate_limits: serde_json::json!({
            "rateLimits": {
                "limitId": "codex",
                "limitName": null,
                "primary": {
                    "usedPercent": used_percent,
                    "windowDurationMins": 10080,
                    "resetsAt": 1_786_161_121i64
                },
                "secondary": null,
                "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                "individualLimit": null,
                "spendControlReached": false,
                "planType": "pro",
                "rateLimitReachedType": reached
            },
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": used_percent,
                        "windowDurationMins": 10080,
                        "resetsAt": 1_786_161_121i64
                    },
                    "secondary": null,
                    "rateLimitReachedType": reached
                },
                "codex_bengalfox": {
                    "limitId": "codex_bengalfox",
                    "limitName": "GPT-5.3-Codex-Spark",
                    "primary": {
                        "usedPercent": 0,
                        "windowDurationMins": 10080,
                        "resetsAt": 1_786_177_280i64
                    },
                    "secondary": null
                }
            },
            "rateLimitResetCredits": {"availableCount": 0, "credits": []}
        }),
    }
}

#[test]
fn account_read_maps_every_reviewed_credential_source() {
    assert_eq!(
        account_ref_from_account_read(&serde_json::json!({
            "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
            "requiresOpenaiAuth": true
        })),
        ProviderAccountRef {
            source: "chatgpt".into(),
            identifier: Some("a@b.c".into()),
            plan: Some("pro".into())
        }
    );
    assert_eq!(
        account_ref_from_account_read(&serde_json::json!({"account": {"type": "apiKey"}})).source,
        "api_key"
    );
    assert_eq!(
        account_ref_from_account_read(
            &serde_json::json!({"account": null, "requiresOpenaiAuth": true})
        )
        .source,
        "signed_out"
    );
}

#[test]
fn live_codex_payload_reports_available_with_observed_windows() {
    let snapshot = codex_capacity_snapshot(
        "codex_app_server",
        &live_capacity_read(3, None),
        "unix-ms:1000",
        1_000,
    );

    assert_eq!(snapshot.state, ProviderCapacityState::Available);
    assert_eq!(
        snapshot.evidence_source,
        ProviderCapacityEvidence::ProviderQuotaApi
    );
    assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Observed);
    assert_eq!(snapshot.account.source, "chatgpt");
    assert_eq!(snapshot.account.plan.as_deref(), Some("pro"));
    // Both metered buckets are reported, keyed by their provider limit id.
    let labels: Vec<&str> = snapshot
        .windows
        .iter()
        .map(|window| window.label.as_str())
        .collect();
    assert_eq!(labels, vec!["codex.primary", "codex_bengalfox.primary"]);
    assert_eq!(snapshot.windows[0].used_percent, Some(3));
    assert_eq!(
        snapshot.windows[0].resets_at.as_deref(),
        Some("unix-ms:1786161121000")
    );
    // An available account has no reset to report.
    assert_eq!(snapshot.reset_at, None);
}

#[test]
fn provider_reported_limits_drive_limited_and_exhausted() {
    let limited = codex_capacity_snapshot(
        "codex_app_server",
        &live_capacity_read(93, None),
        "unix-ms:1000",
        1_000,
    );
    assert_eq!(limited.state, ProviderCapacityState::Limited);
    assert_eq!(
        limited.reset_at.as_deref(),
        Some("unix-ms:1786161121000"),
        "a constrained window reports when it reopens"
    );

    let saturated = codex_capacity_snapshot(
        "codex_app_server",
        &live_capacity_read(100, None),
        "unix-ms:1000",
        1_000,
    );
    assert_eq!(saturated.state, ProviderCapacityState::Exhausted);

    let reached = codex_capacity_snapshot(
        "codex_app_server",
        &live_capacity_read(4, Some("rate_limit_reached")),
        "unix-ms:1000",
        1_000,
    );
    assert_eq!(
        reached.state,
        ProviderCapacityState::Exhausted,
        "a provider-reported reached type outranks a low percentage"
    );
    assert!(reached
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("rate_limit_reached"));
}

#[test]
fn spend_control_reached_is_exhausted_even_with_headroom() {
    let mut read = live_capacity_read(1, None);
    read.rate_limits["rateLimits"]["spendControlReached"] = serde_json::json!(true);

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
    assert!(snapshot
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("spendControlReached"));
}

#[test]
fn missing_or_unusable_payloads_stay_unknown_and_never_invent_a_number() {
    let empty = CodexAccountCapacityRead {
        account: serde_json::json!({
            "account": {"type": "chatgpt", "email": null, "planType": "pro"},
            "requiresOpenaiAuth": true
        }),
        rate_limits: serde_json::Value::Null,
    };
    let snapshot = codex_capacity_snapshot("codex_app_server", &empty, "unix-ms:1", 1);
    assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
    assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Unknown);
    assert!(snapshot.windows.is_empty());

    // A payload whose windows omit `usedPercent` yields no window at all
    // rather than a fabricated zero.
    let unusable = CodexAccountCapacityRead {
        account: empty.account.clone(),
        rate_limits: serde_json::json!({"rateLimits": {"primary": {"resetsAt": 1}}}),
    };
    let snapshot = codex_capacity_snapshot("codex_app_server", &unusable, "unix-ms:1", 1);
    assert!(snapshot.windows.is_empty());
    assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
}

/// The VERBATIM payload shape captured from `codex-cli 0.145.0` by driving
/// `initialize` + `initialized` + the two account reads over stdio. Kept
/// whole — extra keys included — so a regression is exercised against the
/// real wire shape, not a convenience subset.
fn live_multi_bucket_payload() -> serde_json::Value {
    serde_json::json!({
        "rateLimits": {
            "limitId": "codex",
            "limitName": null,
            "primary": {"usedPercent": 3, "windowDurationMins": 10080, "resetsAt": 1786161121i64},
            "secondary": null,
            "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
            "individualLimit": null,
            "spendControlReached": false,
            "planType": "pro",
            "rateLimitReachedType": null
        },
        "rateLimitsByLimitId": {
            "codex_bengalfox": {
                "limitId": "codex_bengalfox",
                "limitName": "GPT-5.3-Codex-Spark",
                "primary": {"usedPercent": 0, "windowDurationMins": 10080, "resetsAt": 1786177280i64},
                "secondary": null,
                "credits": null,
                "individualLimit": null,
                "spendControlReached": null,
                "planType": "pro",
                "rateLimitReachedType": null
            },
            "codex": {
                "limitId": "codex",
                "limitName": null,
                "primary": {"usedPercent": 3, "windowDurationMins": 10080, "resetsAt": 1786161121i64},
                "secondary": null,
                "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                "individualLimit": null,
                "spendControlReached": false,
                "planType": "pro",
                "rateLimitReachedType": null
            }
        },
        "rateLimitResetCredits": {"availableCount": 0, "credits": []}
    })
}

#[test]
fn the_real_live_multi_bucket_payload_classifies_from_the_account_bucket() {
    let read = CodexAccountCapacityRead {
        account: serde_json::json!({
            "account": {"type": "chatgpt", "email": "operator@example.com", "planType": "pro"},
            "requiresOpenaiAuth": true
        }),
        rate_limits: live_multi_bucket_payload(),
    };

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    assert_eq!(snapshot.state, ProviderCapacityState::Available);
    assert_eq!(snapshot.account.plan.as_deref(), Some("pro"));
    // Both real buckets are reported, sorted and keyed by provider limit id.
    let labels: Vec<&str> = snapshot
        .windows
        .iter()
        .map(|window| window.label.as_str())
        .collect();
    assert_eq!(labels, vec!["codex.primary", "codex_bengalfox.primary"]);
    assert_eq!(snapshot.reset_at, None, "an available account has no reset");

    // Saturate ONLY the per-model bucket in that same real payload.
    let mut spark_spent = read.rate_limits.clone();
    spark_spent["rateLimitsByLimitId"]["codex_bengalfox"]["primary"]["usedPercent"] =
        serde_json::json!(100);
    spark_spent["rateLimitsByLimitId"]["codex_bengalfox"]["rateLimitReachedType"] =
        serde_json::json!("rate_limit_reached");
    let snapshot = codex_capacity_snapshot(
        "codex_app_server",
        &CodexAccountCapacityRead {
            account: read.account.clone(),
            rate_limits: spark_spent,
        },
        "unix-ms:1000",
        1_000,
    );
    assert_eq!(
        snapshot.state,
        ProviderCapacityState::Available,
        "a spent per-model bucket must not refuse an account at 3%: {:?}",
        snapshot.detail
    );

    // Saturate the ACCOUNT bucket in the same real payload.
    let mut account_spent = read.rate_limits.clone();
    account_spent["rateLimits"]["rateLimitReachedType"] = serde_json::json!("rate_limit_reached");
    let snapshot = codex_capacity_snapshot(
        "codex_app_server",
        &CodexAccountCapacityRead {
            account: read.account.clone(),
            rate_limits: account_spent,
        },
        "unix-ms:1000",
        1_000,
    );
    assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
    assert_eq!(
        snapshot.reset_at.as_deref(),
        Some("unix-ms:1786161121000"),
        "the reset comes from the account bucket's own window"
    );
}

#[test]
fn a_saturated_per_model_bucket_is_not_an_account_verdict() {
    // Live payloads carry several metered buckets (`codex`,
    // `codex_bengalfox`). Reading the peak across all of them would refuse
    // every codex member because one per-model bucket is spent, while the
    // account bucket the member would actually draw on is at 3%.
    let mut read = live_capacity_read(3, None);
    read.rate_limits["rateLimitsByLimitId"]["codex_bengalfox"]["primary"]["usedPercent"] =
        serde_json::json!(100);

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    assert_eq!(
        snapshot.state,
        ProviderCapacityState::Available,
        "the account bucket has headroom: {:?}",
        snapshot.detail
    );
    // The hot bucket is still visible so an operator can see it.
    assert!(snapshot
        .windows
        .iter()
        .any(
            |window| window.limit_id.as_deref() == Some("codex_bengalfox")
                && window.used_percent == Some(100)
        ));
}

#[test]
fn a_reached_flag_is_read_from_the_same_bucket_as_the_percentage() {
    // The account mirror and the keyed view must never be mixed: a reached
    // flag in one and a low percentage in the other previously produced
    // `available` with `observed` confidence.
    let read = CodexAccountCapacityRead {
        account: serde_json::json!({
            "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
            "requiresOpenaiAuth": true
        }),
        rate_limits: serde_json::json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "primary": {"usedPercent": 40, "resetsAt": 1_786_161_121i64},
                    "rateLimitReachedType": "rate_limit_reached"
                }
            }
        }),
    };

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
    assert!(snapshot
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("rate_limit_reached"));
}

#[test]
fn several_buckets_without_an_account_mirror_are_not_attributable() {
    let read = CodexAccountCapacityRead {
        account: serde_json::json!({
            "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
            "requiresOpenaiAuth": true
        }),
        rate_limits: serde_json::json!({
            "rateLimitsByLimitId": {
                "codex": {"limitId": "codex", "primary": {"usedPercent": 4}},
                "codex_bengalfox": {"limitId": "codex_bengalfox", "primary": {"usedPercent": 99}}
            }
        }),
    };

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    // Neither `available` (which would ignore the hot bucket) nor
    // `exhausted` (which would refuse a healthy account) is honest here.
    assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
    assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Unknown);
    assert_eq!(snapshot.windows.len(), 2, "both buckets stay visible");
}

#[test]
fn reset_reports_when_the_last_constraining_window_reopens() {
    let read = CodexAccountCapacityRead {
        account: serde_json::json!({
            "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
            "requiresOpenaiAuth": true
        }),
        rate_limits: serde_json::json!({
            "rateLimits": {
                "limitId": "codex",
                // Saturated for five more days...
                "primary": {"usedPercent": 100, "resetsAt": 1_786_600_000i64},
                // ...while a second constrained window reopens in an hour.
                "secondary": {"usedPercent": 95, "resetsAt": 1_786_100_000i64}
            }
        }),
    };

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
    assert_eq!(
        snapshot.reset_at.as_deref(),
        Some("unix-ms:1786600000000"),
        "the account is usable again only when the LAST constraint reopens"
    );
}

#[test]
fn float_usage_percentages_are_read_rather_than_silently_dropped() {
    let mut read = live_capacity_read(3, None);
    read.rate_limits["rateLimits"]["primary"]["usedPercent"] = serde_json::json!(93.4);

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

    assert_eq!(
        snapshot.state,
        ProviderCapacityState::Limited,
        "a float window must not empty the probe: {:?}",
        snapshot.detail
    );
}

#[test]
fn signed_out_account_is_unauthorized_not_exhausted() {
    let read = CodexAccountCapacityRead {
        account: serde_json::json!({"account": null, "requiresOpenaiAuth": true}),
        rate_limits: serde_json::json!({"rateLimits": {"primary": {"usedPercent": 0}}}),
    };

    let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1", 1);

    assert_eq!(snapshot.state, ProviderCapacityState::Unauthorized);
    assert_eq!(
        snapshot.evidence_source,
        ProviderCapacityEvidence::AuthMetadata
    );
    assert_eq!(snapshot.account.source, "signed_out");
}

#[test]
fn reviewed_account_read_methods_never_name_a_thread() {
    // Guards the acceptance requirement literally: the preflight speaks
    // only these two methods, and neither is a thread verb.
    for method in [ACCOUNT_READ_METHOD, ACCOUNT_RATE_LIMITS_READ_METHOD] {
        assert!(method.starts_with("account/"), "{method}");
        assert!(!method.contains("thread"), "{method}");
        assert!(!method.contains("turn"), "{method}");
    }
}
