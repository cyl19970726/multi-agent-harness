//! Integration coverage for the Codex WorkDelivery receipt contract:
//! the provider receipt is recorded at turn/start acceptance — the earliest
//! honest evidence — not after the whole turn. The fake app-server never
//! completes the first turn (no FAKE_CODEX_AUTO_COMPLETE), so any receipt
//! observed is pre-completion by construction. A Host Close during that
//! hanging turn must preserve the honest receipt instead of failing the
//! delivery.

use std::time::Duration;

mod fake_provider;
mod firm_env;
use firm_env::{current_project_id, run_firm, ServeHandle, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

#[test]
fn codex_work_receipt_lands_at_turn_acceptance_and_survives_close() {
    let home = TempHome::new("codex-work-receipt-acceptance");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-receipt"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // No FAKE_CODEX_AUTO_COMPLETE: the first turn stays in flight forever, so
    // a provider receipt can only come from turn/start acceptance.
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &[("PATH", path.as_str())]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Prove codex receipt lands at provider acceptance",
            "members": [{
                "name": "codex-receipt",
                "role": "implementer",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise receipt timing"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut receipt: Option<String> = None;
    for _ in 0..250 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let delivery = snapshot["work_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()));
        if let Some(delivery) = delivery {
            if delivery["status"].as_str() == Some("provider_received") {
                receipt = delivery["provider_receipt_id"].as_str().map(str::to_string);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let receipt = receipt
        .expect("delivery never became provider_received while the first turn was still in flight");
    assert!(
        receipt.starts_with("codex:"),
        "unexpected codex receipt id: {receipt}"
    );

    // Close during the still-hanging first turn. The receipted claim is honest
    // evidence and must survive; only unreceived claims are failed by Close.
    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "receipt proven"}),
    );
    assert_eq!(status, 200, "body: {closed}");
    let mut stopped = false;
    for _ in 0..250 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        stopped = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("stopped")
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stopped, "member did not stop after Host close");

    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let delivery = snapshot["work_deliveries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
        .expect("delivery row");
    assert_eq!(
        delivery["status"].as_str(),
        Some("provider_received"),
        "an accepted turn's receipt must survive Close: {delivery}"
    );
    assert_eq!(
        delivery["provider_receipt_id"].as_str(),
        Some(receipt.as_str())
    );
}

#[test]
fn codex_disconnect_resume_continues_receipted_work_on_same_session() {
    // Recovery contract: after a transport loss, the resumed drive must
    // continue the receipted-but-unfinished Work on the same native session
    // (runtime_recovery journal + a continuation turn), not strand it.
    let home = TempHome::new("codex-disconnect-resume");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-resume"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let exit_once_marker = home.base().join("codex-exit-once.marker");
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_AUTO_COMPLETE", "1"),
            ("FAKE_CODEX_EXIT_AFTER_FIRST_TURN", "1"),
            (
                "FAKE_CODEX_EXIT_ONCE_MARKER",
                exit_once_marker.to_str().expect("marker path"),
            ),
            // Disable the serve default idle-retire knob: recovery
            // continuation turns are exactly what this scenario proves, and
            // the knob deliberately disables them (production never sets it).
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", ""),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Prove codex resume continues receipted work",
            "members": [{
                "name": "codex-resume",
                "role": "implementer",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise disconnect recovery"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    // Wait for: one transport-loss disconnect, then a runtime_recovery journal
    // on the SAME native session, then the member back to idle after the
    // continuation turn (the resumed fake keeps running thanks to the marker).
    let mut recovered = false;
    for _ in 0..500 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let actions: Vec<&serde_json::Value> = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| action["member_run_id"].as_str() == Some(member_id.as_str()))
            .collect();
        let disconnected = actions
            .iter()
            .any(|action| action["action_type"].as_str() == Some("disconnected"));
        let runtime_recovery = actions
            .iter()
            .any(|action| action["action_type"].as_str() == Some("runtime_recovery"));
        let member_idle_same_session = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        recovered = disconnected && runtime_recovery && member_idle_same_session;
        if recovered {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let actions: Vec<String> = snapshot["member_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| action["member_run_id"].as_str() == Some(member_id.as_str()))
        .map(|action| {
            format!(
                "{}:{}",
                action["action_type"].as_str().unwrap_or("?"),
                action["status"].as_str().unwrap_or("?")
            )
        })
        .collect();
    let member_status = snapshot["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|member| member["id"].as_str() == Some(member_id.as_str()))
        .map(|member| {
            format!(
                "status={} session={}",
                member["status"].as_str().unwrap_or("?"),
                member["native_session"]["native_session_id"]
                    .as_str()
                    .unwrap_or("?")
            )
        })
        .unwrap_or_else(|| "member missing".to_string());
    let deliveries: Vec<String> = snapshot["work_deliveries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|delivery| {
            format!(
                "work={} v{} {} receipt={:?}",
                delivery["work_id"].as_str().unwrap_or("?"),
                delivery["work_version"].as_u64().unwrap_or(0),
                delivery["status"].as_str().unwrap_or("?"),
                delivery["provider_receipt_id"].as_str()
            )
        })
        .collect();
    let works: Vec<String> = snapshot["works"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|work| {
            format!(
                "{} v{} {}",
                work["id"].as_str().unwrap_or("?"),
                work["version"].as_u64().unwrap_or(0),
                work["status"].as_str().unwrap_or("?")
            )
        })
        .collect();
    assert!(
        recovered,
        "disconnect was not followed by a runtime_recovery continuation on the same native session: actions={actions:?} {member_status} deliveries={deliveries:?} works={works:?}"
    );

    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "recovery proven"}),
    );
    assert_eq!(status, 200, "body: {closed}");
}
