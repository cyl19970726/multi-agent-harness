//! Regression coverage for #729: a WorkDelivery the Supervisor mints but does
//! not dispatch in the same pass is dead on arrival.
//!
//! The Supervisor claims exactly one delivery per idle pass. When a member owns
//! more than one ready Work at that pass, the extra bindings were minted
//! anyway; their `queued` deliveries could never be claimed under the runtime
//! facts of a later pass, so they were released
//! (`WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM`) and re-minted only at the
//! member's next round boundary. `member work start` on such a Work answers
//! `DELIVERY_NOT_DISPATCHED` (#722) for a delivery that will never be
//! dispatched.
//!
//! Every provider here is a deterministic PATH shim; no real provider is
//! invoked and no network call is made.

mod fake_provider;
mod firm_env;

use std::time::{Duration, Instant};

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm,
    run_firm_with_env, TempHome,
};
use harness_store::HarnessStore;

const HOST_ID: &str = "agent-prestart-host";
const WORKER_ID: &str = "prestart-worker";
const TEAM_ID: &str = "team-prestart-fixture";

struct Fixture {
    project_id: String,
    space_id: String,
}

fn ok(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn provider_env<'a>(bin: &'a str, idle_ms: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("PATH", bin),
        ("FAKE_CODEX_AUTO_COMPLETE", "1"),
        ("FAKE_KIMI_RESULT", "done"),
        ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", idle_ms),
    ]
}

fn bootstrap(home: &TempHome) -> Fixture {
    let root = home.base().to_path_buf();
    ok(&run_firm(home, &root, &["init"]), "firm init");
    let project_id = current_project_id(home);
    let space_id = current_space_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    ok(&node, "node init");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();
    ok(
        &run_firm(
            home,
            &root,
            &[
                "node",
                "project",
                "register",
                "--node-id",
                &node_id,
                "--project-binding-id",
                &project_id,
            ],
        ),
        "node project register",
    );
    ok(
        &create_canonical_agent_member(
            home,
            &root,
            &project_id,
            HOST_ID,
            "prestart-host",
            "host",
            "codex",
            &[],
        ),
        "host member create",
    );
    ok(
        &create_canonical_agent_member(
            home,
            &root,
            &project_id,
            WORKER_ID,
            "prestart-worker",
            "implementer",
            "codex",
            &[],
        ),
        "worker member create",
    );
    ok(
        &run_firm(
            home,
            &root,
            &[
                "team",
                "create",
                "--id",
                TEAM_ID,
                "--name",
                "Pre-start dispatch team",
                "--description",
                "Flat pre-start dispatch test team",
                "--host-agent-id",
                HOST_ID,
                "--node-id",
                &node_id,
                "--member",
                HOST_ID,
                "--member",
                WORKER_ID,
            ],
        ),
        "team create",
    );
    Fixture {
        project_id,
        space_id,
    }
}

fn spawn_daemon(home: &TempHome, env: &[(&str, &str)]) -> std::process::Child {
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
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env_remove("KIMI_CODE_BIN")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn NodeDaemon");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = run_firm(home, home.base(), &["daemon", "status"]);
        if status.status.success() && !String::from_utf8_lossy(&status.stdout).contains("absent") {
            return child;
        }
        assert!(
            child.try_wait().expect("inspect NodeDaemon").is_none(),
            "NodeDaemon exited before becoming ready"
        );
        assert!(Instant::now() < deadline, "NodeDaemon readiness timeout");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn deliveries(
    home: &TempHome,
    space_id: &str,
) -> Vec<harness_core::agentfirm_api::CanonicalWorkDelivery> {
    HarnessStore::new(home.spaces_dir().join(space_id))
        .fabric_work_deliveries(space_id)
        .expect("canonical WorkDelivery fabric")
}

#[test]
fn prestart_work_is_dispatched_without_a_dead_queued_delivery() {
    let home = TempHome::new("prestart-dispatch");
    let fixture = bootstrap(&home);
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let env = provider_env(&path, "4000");

    let create = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "create",
            "--agent-team-id",
            TEAM_ID,
            "--objective",
            "Prove a pre-start Work is dispatched without a dead queued delivery",
            "--host-runtime-mode",
            "external_interactive",
            "--member",
            &format!("{HOST_ID}:host:codex/external_interactive"),
            "--member",
            &format!("{WORKER_ID}:implementer:codex:gpt-5.6#Bootstrap objective"),
        ],
        &env,
    );
    ok(&create, "team-run create");
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();

    let status = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "status",
            "--id",
            &run_id,
            "--json",
        ],
        &env,
    );
    ok(&status, "team-run status");
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).expect("status JSON");
    let member_run_id = status["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["member_run"]["agent_member_id"] == WORKER_ID)
        .and_then(|member| member["member_run"]["id"].as_str())
        .expect("worker member run id")
        .to_string();
    let membership_id =
        firm_env::membership_id_for_member_run(&home, &fixture.space_id, &member_run_id);

    // The Host's own Work, created and assigned BEFORE `team-run start`.
    let created = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            "Host pre-start Work",
            "--completion-criteria",
            "the Supervisor dispatches this delivery",
        ],
        &env,
    );
    ok(&created, "team-run work create");
    let created: serde_json::Value = serde_json::from_slice(&created.stdout).expect("Work JSON");
    let host_work_id = created["id"].as_str().expect("work id").to_string();
    let version = created["version"].as_u64().expect("work version");
    let assigned = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "work",
            "assign",
            "--work-id",
            &host_work_id,
            "--expected-version",
            &version.to_string(),
            "--membership-id",
            &membership_id,
        ],
        &env,
    );
    ok(&assigned, "team-run work assign");

    let mut daemon = spawn_daemon(&home, &env);
    let started = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &env,
    );
    ok(&started, "team-run start");

    // No Host message is ever sent below: the Supervisor alone must dispatch.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut observed;
    loop {
        observed = deliveries(&home, &fixture.space_id);
        let dispatched = observed.iter().any(|delivery| {
            delivery.work_id == host_work_id
                && delivery.status != harness_core::agentfirm_api::WorkDeliveryStatus::Queued
        });
        if dispatched || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();

    let report = observed
        .iter()
        .map(|delivery| {
            format!(
                "{} work={} status={:?} claim={:?} receipt={:?} failure={:?}",
                delivery.id,
                delivery.work_id,
                delivery.status,
                delivery.claim_id,
                delivery.provider_receipt_id,
                delivery.failure_code
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !observed
            .iter()
            .any(|delivery| delivery.failure_code.as_deref()
                == Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM")),
        "the Supervisor minted a delivery it could not dispatch:\n{report}"
    );
    let host_deliveries = observed
        .iter()
        .filter(|delivery| delivery.work_id == host_work_id)
        .collect::<Vec<_>>();
    let [host_delivery] = host_deliveries.as_slice() else {
        panic!("the pre-start Work must have exactly one delivery:\n{report}");
    };
    assert_eq!(
        host_delivery.id,
        format!("work-delivery:{host_work_id}:1"),
        "no binding generation may be burned before the first dispatch:\n{report}"
    );
    assert_ne!(
        host_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::Queued,
        "the pre-start Work was never dispatched:\n{report}"
    );
    assert!(
        host_delivery
            .claim_id
            .as_deref()
            .is_some_and(|claim| !claim.trim().is_empty()),
        "the dispatched delivery must carry a real claim:\n{report}"
    );
}
