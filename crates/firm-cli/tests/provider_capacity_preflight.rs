//! Integration coverage for the provider capacity preflight
//! (`work-agentos-provider-capacity-preflight-v1`).
//!
//! Capacity is runtime availability of one provider ACCOUNT for one execution
//! mode. It is not adapter compatibility, and an unknown answer is never
//! upgraded to "available". Every provider here is a deterministic PATH shim;
//! no real provider is invoked and no network call is made.

use std::path::Path;

mod fake_provider;
mod firm_env;

use firm_env::{create_canonical_agent_member, current_project_id, run_firm, TempHome};
use harness_store::HarnessStore;

fn canonical_work_deliveries(home: &TempHome, execution_space_id: &str) -> Vec<serde_json::Value> {
    HarnessStore::new(home.spaces_dir().join(execution_space_id))
        .fabric_work_deliveries(execution_space_id)
        .expect("canonical WorkDelivery fabric")
        .into_iter()
        .map(|delivery| serde_json::to_value(delivery).expect("WorkDelivery JSON"))
        .collect()
}

/// A rate-limit payload whose only saturated signal is the one under test.
fn rate_limits_json(used_percent: i64, reached_type: &str, spend_control: bool) -> String {
    format!(
        r#"{{"rateLimits":{{"limitId":"codex","primary":{{"usedPercent":{used_percent},"windowDurationMins":10080,"resetsAt":1786161121}},"secondary":null,"rateLimitReachedType":{reached_type},"spendControlReached":{spend_control}}}}}"#
    )
}

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    let registration = run_firm(
        home,
        &root,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    assert!(
        registration.status.success(),
        "register failed: {registration:?}"
    );
    let mission = run_firm(
        home,
        &root,
        &[
            "mission",
            "create",
            "--id",
            "mission-capacity-fixture",
            "--title",
            "Provider capacity mission",
            "--objective",
            "Verify provider capacity preflight",
        ],
    );
    assert!(
        mission.status.success(),
        "mission create failed: {mission:?}"
    );
    let host = create_canonical_agent_member(
        home,
        &root,
        &project_id,
        "agent-capacity-host",
        "capacity-host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host create failed: {host:?}");
    let worker = create_canonical_agent_member(
        home,
        &root,
        &project_id,
        "codex-worker",
        "codex-worker",
        "implementer",
        "codex",
        &[],
    );
    assert!(worker.status.success(), "worker create failed: {worker:?}");
    let team = run_firm(
        home,
        &root,
        &[
            "team",
            "create",
            "--id",
            "team-capacity-fixture",
            "--name",
            "Provider capacity team",
            "--description",
            "Flat provider capacity test team",
            "--mission-id",
            "mission-capacity-fixture",
            "--host-agent-id",
            "agent-capacity-host",
            "--node-id",
            node_id,
            "--member",
            "agent-capacity-host",
            "--member",
            "codex-worker",
        ],
    );
    assert!(team.status.success(), "team create failed: {team:?}");
    project_id
}

struct FakeCodex<'a> {
    bin: &'a Path,
    account_json: Option<String>,
    rate_limits_json: Option<String>,
    thread_marker: Option<std::path::PathBuf>,
    ttl_ms: Option<String>,
}

impl<'a> FakeCodex<'a> {
    fn new(bin: &'a Path) -> Self {
        Self {
            bin,
            account_json: None,
            rate_limits_json: None,
            thread_marker: None,
            ttl_ms: None,
        }
    }

    fn run(&self, home: &TempHome, args: &[&str]) -> std::process::Output {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_firm"));
        command
            .args(args)
            .current_dir(home.base())
            .envs(home.envs());
        self.configure(home, &mut command);
        command.output().expect("run harness")
    }

    fn configure(&self, home: &TempHome, command: &mut std::process::Command) {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command
            .env_remove("FIRM_ROOT")
            .env_remove("FIRM_PROJECT")
            .env_remove("FIRM_SPACE")
            .env_remove("FIRM_COMPANY")
            .env_remove("KIMI_CODE_BIN")
            .env("PATH", path)
            .env("FAKE_KIMI_RESULT", "done")
            .env("FAKE_CODEX_AUTO_COMPLETE", "1")
            .env(
                "FIRM_CLAUDE_MEMBER_RUNNER",
                home.base().join("definitely-missing-claude-member-runner"),
            )
            .env("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100");
        if let Some(account) = &self.account_json {
            command.env("FAKE_CODEX_ACCOUNT_JSON", account);
        }
        if let Some(limits) = &self.rate_limits_json {
            command.env("FAKE_CODEX_RATE_LIMITS_JSON", limits);
        }
        if let Some(marker) = &self.thread_marker {
            command.env("FAKE_CODEX_THREAD_MARKER", marker);
        }
        if let Some(ttl) = &self.ttl_ms {
            command.env("FIRM_CAPACITY_TTL_MS", ttl);
        }
    }
}

/// Provider preflight and execution belong to the machine NodeDaemon, so the
/// deterministic provider environment is installed on that process rather
/// than on the public `team-run start` request.
fn spawn_node_authority(
    home: &TempHome,
    fake: &FakeCodex<'_>,
    extra_env: &[(&str, &str)],
) -> std::process::Child {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args([
            "daemon",
            "serve",
            "--scan-interval-secs",
            "1",
            "--idle-timeout-secs",
            "60",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    fake.configure(home, &mut command);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn NodeDaemon authority");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = run_firm(home, home.base(), &["daemon", "status"]);
        if status.status.success() && !String::from_utf8_lossy(&status.stdout).contains("absent") {
            return child;
        }
        assert!(
            child.try_wait().expect("inspect NodeDaemon").is_none(),
            "NodeDaemon exited before becoming ready"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "NodeDaemon readiness timeout"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn stop_node_authority(home: &TempHome, child: &mut std::process::Child) {
    let stop = run_firm(home, home.base(), &["daemon", "stop"]);
    assert!(stop.status.success(), "NodeDaemon stop failed: {stop:?}");
    assert!(
        child.wait().expect("wait NodeDaemon").success(),
        "NodeDaemon authority did not exit cleanly"
    );
}

fn store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.spaces_dir().join(project_id).join(file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(trimmed).unwrap_or_else(|e| panic!("{file} row not JSON: {e}"));
        let id = row["id"].as_str().expect("row id").to_string();
        ids.retain(|known| known != &id);
        ids.push(id.clone());
        by_id.insert(id, row);
    }
    ids.into_iter()
        .map(|id| by_id.remove(&id).unwrap())
        .collect()
}

fn wait_for_runtime_projection(description: &str, mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn preflight_row(stdout: &[u8], provider: &str) -> serde_json::Value {
    let parsed: serde_json::Value =
        serde_json::from_slice(stdout).expect("preflight stdout is JSON");
    assert_eq!(parsed["ok"], serde_json::json!(true), "{parsed}");
    parsed["result"]["providers"]
        .as_array()
        .expect("providers array")
        .iter()
        .find(|row| row["provider"].as_str() == Some(provider))
        .unwrap_or_else(|| panic!("no {provider} row in {parsed}"))
        .clone()
}

/// Create a one-member TeamRun and return `(project_id, run_id)`.
fn create_single_member_run(
    home: &TempHome,
    fake: &FakeCodex<'_>,
    project_id: &str,
    member_spec: &str,
) -> String {
    let member_with_work = format!("{member_spec}#Run the capacity-gated provider Work");
    let create = fake.run(
        home,
        &[
            "--project",
            project_id,
            "team-run",
            "create",
            "--agent-team-id",
            "team-capacity-fixture",
            "--objective",
            "Prove the capacity preflight gates provider start without consuming Work",
            "--member",
            &member_with_work,
        ],
    );
    assert!(
        create.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    String::from_utf8_lossy(&create.stdout).trim().to_string()
}

#[test]
fn codex_preflight_reads_account_and_rate_limits_without_starting_a_thread() {
    let home = TempHome::new("capacity-codex-no-thread");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let thread_marker = home.base().join("codex-threads.log");
    let fake = FakeCodex {
        thread_marker: Some(thread_marker.clone()),
        ..FakeCodex::new(&bin)
    };

    let out = fake.run(
        &home,
        &["--project", &project_id, "member", "preflight", "--json"],
    );

    assert!(
        out.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let codex = preflight_row(&out.stdout, "codex");
    assert_eq!(
        codex["execution_mode"],
        serde_json::json!("codex_app_server")
    );
    assert_eq!(codex["capacity"]["state"], serde_json::json!("available"));
    assert_eq!(
        codex["capacity"]["evidence_source"],
        serde_json::json!("provider_quota_api")
    );
    assert_eq!(
        codex["capacity"]["confidence"],
        serde_json::json!("observed")
    );
    // The account/source boundary and the observed window come from the
    // provider, not from the adapter.
    assert_eq!(
        codex["capacity"]["account"]["source"],
        serde_json::json!("chatgpt")
    );
    assert_eq!(
        codex["capacity"]["account"]["identifier"],
        serde_json::json!("fake@example.com")
    );
    assert_eq!(
        codex["capacity"]["windows"][0]["used_percent"],
        serde_json::json!(7)
    );
    assert_eq!(
        codex["capacity"]["windows"][0]["resets_at"],
        serde_json::json!("unix-ms:1786161121000")
    );
    assert_eq!(codex["capacity_freshness"], serde_json::json!("fresh"));
    assert_eq!(
        codex["start_decision"]["decision"],
        serde_json::json!("proceed")
    );
    // The whole point: no thread was started, resumed, or named.
    assert!(
        !thread_marker.exists(),
        "the capacity preflight started a thread: {}",
        std::fs::read_to_string(&thread_marker).unwrap_or_default()
    );
}

#[test]
fn preflight_keeps_capacity_and_adapter_compatibility_separate() {
    let home = TempHome::new("capacity-vs-compatibility");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let fake = FakeCodex {
        // A reviewed-current adapter whose ACCOUNT is out of capacity is a
        // normal, expressible state.
        rate_limits_json: Some(rate_limits_json(4, "\"rate_limit_reached\"", false)),
        ..FakeCodex::new(&bin)
    };

    let out = fake.run(
        &home,
        &[
            "--project",
            &project_id,
            "member",
            "preflight",
            "--provider",
            "codex",
            "--json",
        ],
    );

    assert!(out.status.success(), "{out:?}");
    let codex = preflight_row(&out.stdout, "codex");
    assert_eq!(codex["capacity"]["state"], serde_json::json!("exhausted"));
    assert_eq!(
        codex["compatibility"]["status"],
        serde_json::json!("current"),
        "adapter compatibility must not move with capacity"
    );
    // Capacity carries no compatibility field and compatibility carries no
    // capacity field: two sibling truths, never merged.
    assert!(
        codex["capacity"].get("compatibility_status").is_none(),
        "{codex}"
    );
    assert!(codex["compatibility"].get("state").is_none(), "{codex}");
    assert_eq!(
        codex["start_decision"]["decision"],
        serde_json::json!("block")
    );
}

#[test]
fn kimi_reports_unknown_without_inventing_a_quota_percentage() {
    let home = TempHome::new("capacity-kimi-unknown");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake = FakeCodex::new(&bin);

    let out = fake.run(
        &home,
        &[
            "--project",
            &project_id,
            "member",
            "preflight",
            "--provider",
            "kimi",
            "--json",
        ],
    );

    assert!(out.status.success(), "{out:?}");
    let kimi = preflight_row(&out.stdout, "kimi");
    assert_eq!(kimi["execution_mode"], serde_json::json!("kimi_acp"));
    assert_eq!(kimi["capacity"]["state"], serde_json::json!("unknown"));
    assert_eq!(
        kimi["capacity"]["evidence_source"],
        serde_json::json!("not_exposed")
    );
    assert_eq!(kimi["capacity"]["confidence"], serde_json::json!("unknown"));
    assert_eq!(
        kimi["capacity"]["windows"],
        serde_json::json!([]),
        "no reviewed quota API means no window may be reported"
    );
    assert_eq!(kimi["capacity"]["reset_at"], serde_json::Value::Null);
    // Unknown stays honest AND stays ungated.
    assert_eq!(
        kimi["start_decision"]["decision"],
        serde_json::json!("proceed")
    );
}

#[test]
fn unauthorized_codex_account_is_not_reported_as_exhausted() {
    let home = TempHome::new("capacity-codex-unauthorized");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let fake = FakeCodex {
        account_json: Some(r#"{"account":null,"requiresOpenaiAuth":true}"#.to_string()),
        ..FakeCodex::new(&bin)
    };

    let out = fake.run(
        &home,
        &[
            "--project",
            &project_id,
            "member",
            "preflight",
            "--provider",
            "codex",
            "--json",
            "--fail-on-unavailable",
        ],
    );

    let codex = preflight_row(&out.stdout, "codex");
    assert_eq!(
        codex["capacity"]["state"],
        serde_json::json!("unauthorized")
    );
    assert_eq!(
        codex["capacity"]["account"]["source"],
        serde_json::json!("signed_out")
    );
    assert_eq!(
        codex["start_decision"]["decision"],
        serde_json::json!("block")
    );
    assert!(
        !out.status.success(),
        "--fail-on-unavailable must exit non-zero on a fresh known-unavailable answer"
    );
}

#[test]
fn claude_preflight_reports_auth_metadata_not_capacity_and_never_a_rate_limit() {
    let home = TempHome::new("capacity-claude-metadata");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake = FakeCodex::new(&bin);

    let out = fake.run(
        &home,
        &[
            "--project",
            &project_id,
            "member",
            "preflight",
            "--provider",
            "claude",
            "--json",
        ],
    );

    assert!(out.status.success(), "{out:?}");
    let claude = preflight_row(&out.stdout, "claude");
    assert_eq!(
        claude["execution_mode"],
        serde_json::json!("claude_agent_sdk")
    );
    // Without a canary, auth metadata alone may never claim availability.
    assert_eq!(claude["capacity"]["state"], serde_json::json!("unknown"));
    assert_eq!(
        claude["capacity"]["evidence_source"],
        serde_json::json!("auth_metadata")
    );
    assert_eq!(
        claude["capacity"]["windows"],
        serde_json::json!([]),
        "Claude rate limits are never surfaced"
    );
    // The proxy/runtime context is reported so a 403 can be diagnosed instead
    // of guessed. Values of credential keys are withheld.
    let facts = claude["capacity"]["runtime_context"]
        .as_array()
        .expect("runtime context facts");
    assert!(facts
        .iter()
        .any(|fact| fact["key"] == serde_json::json!("HTTPS_PROXY")));
    let api_key = facts
        .iter()
        .find(|fact| fact["key"] == serde_json::json!("ANTHROPIC_API_KEY"))
        .expect("credential presence fact");
    assert_eq!(api_key["note"], serde_json::json!("value withheld"));
    // Capacity remains unknown (and specifically is not reported as exhausted),
    // while the independent operational compatibility probe fails closed when
    // the Claude SDK/version probe is unavailable in this test environment.
    assert_eq!(
        claude["start_decision"]["decision"],
        serde_json::json!("block")
    );
    assert_eq!(
        claude["start_decision"]["gate"],
        serde_json::json!("provider_compatibility")
    );
    assert_eq!(
        claude["compatibility"]["operational"]["status"],
        serde_json::json!("unavailable")
    );
    assert_eq!(
        claude["compatibility"]["operational"]["source"],
        serde_json::json!("version_probe")
    );
    assert!(
        claude["compatibility"]["operational"]["probe_error"].is_string(),
        "the compatibility refusal preserves the version-probe failure: {claude}"
    );
}

#[test]
fn missing_proxy_is_diagnosed_as_runtime_context_not_an_account_limit() {
    let home = TempHome::new("capacity-claude-proxy-gap");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "member",
            "preflight",
            "--provider",
            "claude",
            "--json",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        // Reproduce the Wave 2 host: no proxy anywhere in the process env.
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env(
            "FIRM_CLAUDE_MEMBER_RUNNER",
            home.base().join("definitely-missing-claude-member-runner"),
        )
        .env("PATH", path)
        .output()
        .expect("run harness");

    assert!(out.status.success(), "{out:?}");
    let claude = preflight_row(&out.stdout, "claude");
    let diagnosis = claude["capacity"]["diagnosis"]
        .as_str()
        .expect("a missing-proxy diagnosis");
    assert!(
        diagnosis.contains("PROXY"),
        "the diagnosis names the missing runtime context: {diagnosis}"
    );
    // A runtime-context gap is not an account verdict.
    assert_eq!(claude["capacity"]["state"], serde_json::json!("unknown"));
    assert_eq!(
        claude["start_decision"]["decision"],
        serde_json::json!("block"),
        "operational compatibility may block independently of account capacity"
    );
    assert_eq!(
        claude["start_decision"]["gate"],
        serde_json::json!("provider_compatibility"),
        "the missing proxy must not be mistaken for an exhausted account"
    );
    assert_eq!(
        claude["compatibility"]["operational"]["status"],
        serde_json::json!("unavailable")
    );
    assert_eq!(
        claude["compatibility"]["operational"]["source"],
        serde_json::json!("version_probe")
    );
}

#[test]
fn fresh_exhausted_capacity_blocks_start_and_leaves_work_queued() {
    let home = TempHome::new("capacity-start-guard-block");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let thread_marker = home.base().join("codex-threads.log");
    let fake = FakeCodex {
        rate_limits_json: Some(rate_limits_json(100, "null", false)),
        thread_marker: Some(thread_marker.clone()),
        ..FakeCodex::new(&bin)
    };
    let mut daemon = spawn_node_authority(&home, &fake, &[]);
    let run_id = create_single_member_run(
        &home,
        &fake,
        &project_id,
        "codex-worker:implementer:codex:gpt-5.6",
    );
    let before = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .next()
        .expect("member run before start");

    let start = fake.run(
        &home,
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(
        start.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    wait_for_runtime_projection("exhausted capacity refusal", || {
        store_rows(&home, &project_id, "member_runs.jsonl")
            .into_iter()
            .next()
            .is_some_and(|member| member["provider_capacity"]["state"] == "exhausted")
            && store_rows(&home, &project_id, "member_actions.jsonl")
                .iter()
                .any(|action| action["action_type"] == "provider_unavailable")
    });

    // 0. Identity stays reconstructable. Everything a Host needs to resume the
    //    same member — rather than create a new one — is byte-identical; only
    //    the status, the new observation, and its timestamp moved.
    let after = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .next()
        .expect("member run after start");
    for field in [
        "id",
        "team_run_id",
        "agent_member_id",
        "name",
        "role",
        "provider",
        "model",
        "owned_paths",
        "provider_cwd_hint",
        "native_session",
        "started_at",
    ] {
        assert_eq!(
            after[field], before[field],
            "the capacity gate must not rewrite {field}: {after}"
        );
    }
    assert_eq!(
        after["provider_profile"]["provider_version"].as_str(),
        Some("0.145.0-alpha.18"),
        "the compatibility gate must record the installed reviewed version before capacity refusal: {after}"
    );
    assert_eq!(
        after["provider_profile"]["compatibility_status"].as_str(),
        Some("current")
    );

    // 1. Capacity refusal happens before WorkExecutionBinding/WorkDelivery
    // materialization. Ownership remains on Work, while the canonical runtime
    // fabric has no delivery attempt to claim or accidentally advance.
    assert!(
        canonical_work_deliveries(&home, &project_id).is_empty(),
        "a blocked start must not materialize a canonical WorkDelivery"
    );
    assert!(
        HarnessStore::new(home.spaces_dir().join(&project_id))
            .latest_works()
            .expect("Works")
            .into_iter()
            .any(|work| work.team_run_id == run_id && !work.is_terminal()),
        "the assigned Work must remain open and owned"
    );

    // 2. `provider_unavailable` is recorded, with the observed evidence.
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    let unavailable = actions
        .iter()
        .find(|action| action["action_type"].as_str() == Some("provider_unavailable"))
        .unwrap_or_else(|| panic!("no provider_unavailable action in {actions:?}"));
    assert_eq!(unavailable["status"], serde_json::json!("failed"));
    let summary = unavailable["summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("codex_app_server") && summary.contains("exhausted"),
        "the record names the execution mode and the state: {summary}"
    );

    // 3. No handoff was fabricated and the member is blocked, not completed.
    assert!(
        !home
            .spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl")
            .exists(),
        "a blocked start must not fabricate TeamMessages"
    );
    let member = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .next()
        .expect("member run");
    assert_eq!(member["status"], serde_json::json!("blocked"));
    // 4. The snapshot is durable on the ProviderRuntimeProjection, so the Dashboard sees it
    //    without a second probe, and it stays distinct from compatibility.
    assert_eq!(
        member["provider_capacity"]["state"],
        serde_json::json!("exhausted")
    );
    assert_eq!(
        member["provider_capacity"]["evidence_source"],
        serde_json::json!("provider_quota_api")
    );
    // Compatibility was independently proven current before capacity was
    // checked; the exhausted verdict neither borrows nor overwrites it.
    assert_eq!(
        member["provider_profile"]["compatibility_status"],
        serde_json::json!("current"),
        "an exhausted account must not be recorded as an incompatible adapter"
    );
    // 5. The provider was never asked to open a thread.
    assert!(
        !thread_marker.exists(),
        "a blocked member must not start a native thread: {}",
        std::fs::read_to_string(&thread_marker).unwrap_or_default()
    );

    // 6. A replacement NodeDaemon generation may not silently adopt the old
    // AgentSession even when capacity has recovered. Until an explicit atomic
    // session handoff/rebind is performed, successor start fails closed and
    // leaves Work/runtime ledgers unchanged.
    let recovered = FakeCodex {
        rate_limits_json: Some(rate_limits_json(5, "null", false)),
        thread_marker: Some(thread_marker.clone()),
        ..FakeCodex::new(&bin)
    };
    stop_node_authority(&home, &mut daemon);
    let mut daemon = spawn_node_authority(&home, &recovered, &[]);
    let restart = recovered.run(
        &home,
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(
        !restart.status.success(),
        "successor must not adopt old session"
    );
    assert!(
        String::from_utf8_lossy(&restart.stderr).contains("AGENT_SESSION_RECOVERY_REQUIRED"),
        "restart must return a typed recovery fence: {}",
        String::from_utf8_lossy(&restart.stderr)
    );
    assert!(canonical_work_deliveries(&home, &project_id).is_empty());
    assert!(
        !thread_marker.exists(),
        "rejected successor must not start provider"
    );
    stop_node_authority(&home, &mut daemon);
}

#[test]
fn unknown_capacity_still_starts_the_member_and_delivers_work() {
    let home = TempHome::new("capacity-start-guard-unknown");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let fake = FakeCodex {
        // No rate-limit payload at all: honestly unknown.
        rate_limits_json: Some("null".to_string()),
        ..FakeCodex::new(&bin)
    };
    let mut daemon = spawn_node_authority(&home, &fake, &[]);
    let run_id = create_single_member_run(
        &home,
        &fake,
        &project_id,
        "codex-worker:implementer:codex:gpt-5.6",
    );

    let start = fake.run(
        &home,
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(
        start.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    wait_for_runtime_projection("unknown capacity provider delivery", || {
        store_rows(&home, &project_id, "member_runs.jsonl")
            .into_iter()
            .next()
            .is_some_and(|member| member["provider_capacity"]["state"] == "unknown")
            && canonical_work_deliveries(&home, &project_id)
                .into_iter()
                .next()
                .is_some_and(|delivery| delivery["status"] != "queued")
    });

    let member = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .next()
        .expect("member run");
    assert_eq!(
        member["provider_capacity"]["state"],
        serde_json::json!("unknown"),
        "unknown must be recorded honestly, not upgraded"
    );
    assert_ne!(member["status"], serde_json::json!("blocked"));
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(
        !actions
            .iter()
            .any(|action| action["action_type"].as_str() == Some("provider_unavailable")),
        "unknown capacity must not record provider_unavailable: {actions:?}"
    );
    // ProviderWorkDispatch was consumed by a real round, proving the guard opened.
    let delivery = canonical_work_deliveries(&home, &project_id)
        .into_iter()
        .next()
        .expect("ProviderWorkDispatch");
    assert_ne!(
        delivery["status"],
        serde_json::json!("queued"),
        "an ungated member must claim its Work: {delivery}"
    );
    stop_node_authority(&home, &mut daemon);
}

#[test]
fn a_disabled_preflight_records_no_snapshot_and_never_blocks() {
    let home = TempHome::new("capacity-preflight-off");
    let project_id = init_project(&home, "alpha");
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let disabled = FakeCodex {
        rate_limits_json: Some(rate_limits_json(100, "null", false)),
        ..FakeCodex::new(&bin)
    };
    let mut daemon = spawn_node_authority(&home, &disabled, &[("FIRM_CAPACITY_PREFLIGHT", "off")]);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let create = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            "team-capacity-fixture",
            "--objective",
            "Prove the preflight can be disabled without changing semantics",
            "--member",
            "codex-worker:implementer:codex:gpt-5.6#Run provider Work with preflight disabled",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env("PATH", path.clone())
        .output()
        .expect("run harness");
    assert!(create.status.success(), "{create:?}");
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();

    let start = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env("PATH", path)
        .env("FAKE_CODEX_AUTO_COMPLETE", "1")
        .env("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        // An exhausted account that is never observed cannot gate anything.
        .env(
            "FAKE_CODEX_RATE_LIMITS_JSON",
            rate_limits_json(100, "null", false),
        )
        .env("FIRM_CAPACITY_PREFLIGHT", "off")
        .output()
        .expect("run harness");
    assert!(
        start.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    wait_for_runtime_projection("preflight-disabled provider delivery", || {
        canonical_work_deliveries(&home, &project_id)
            .into_iter()
            .next()
            .is_some_and(|delivery| delivery["status"] != "queued")
    });

    let member = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .next()
        .expect("member run");
    assert_eq!(
        member["provider_capacity"],
        serde_json::Value::Null,
        "a disabled probe observes nothing rather than asserting availability"
    );
    assert_ne!(member["status"], serde_json::json!("blocked"));
    stop_node_authority(&home, &mut daemon);
}
