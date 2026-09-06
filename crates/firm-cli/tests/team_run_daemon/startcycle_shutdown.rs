use super::*;
use harness_core::agentfirm_api::{
    RuntimeCommandKind, RuntimeCommandPhase, RuntimeEffectCertainty,
};

fn wait_for(label: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn stop_without_startcycle_receipt_preserves_unknown_and_refuses_release() {
    experiment(0);
}

#[test]
fn correlated_input_receipt_during_drain_settles_before_local_quiesce_refusal() {
    experiment(1);
}

#[test]
fn same_correlated_terminal_before_drain_settles_applied() {
    experiment(2);
}

#[test]
fn quiesce_after_prepare_before_drive_rejects_without_sending_prompt() {
    experiment(3);
}

fn experiment(mode: u8) {
    let label = match mode {
        1 => "terminal-during-drain",
        2 => "terminal-before-drain",
        3 => "prepared-before-drive",
        _ => "no-receipt",
    };
    let home = TempHome::new(label);
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let kimi_bin = fake_bin.join("kimi").display().to_string();
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ready = home.base().join("prompt-ready");
    let release = home.base().join("prompt-release");
    let terminal = home.base().join("terminal-sent");
    let prompt = home.base().join("prompt.jsonl");
    let ignored_cancel = home.base().join("ignored-cancel");
    let ignored_cancel_text = ignored_cancel.display().to_string();
    let ready_text = ready.display().to_string();
    let release_text = release.display().to_string();
    let terminal_text = terminal.display().to_string();
    let prompt_text = prompt.display().to_string();
    let mut env = vec![
        ("PATH", path.as_str()),
        ("KIMI_CODE_BIN", kimi_bin.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("FAKE_KIMI_WAIT", "1"),
        ("FAKE_KIMI_FIRST_PROMPT_READY", ready_text.as_str()),
        ("FAKE_KIMI_PROMPT_MARKER", prompt_text.as_str()),
    ];
    if mode == 0 {
        // WAIT alone still answers cancellation; this negative case must emit
        // neither prompt acceptance nor a correlated cancelled terminal.
        env.extend([
            ("FAKE_KIMI_IGNORE_CANCEL", "1"),
            (
                "FAKE_KIMI_IGNORED_CANCEL_MARKER",
                ignored_cancel_text.as_str(),
            ),
        ]);
    }
    if mode == 3 {
        env.extend([
            ("FIRM_TEST_KIMI_PREPARED_CYCLE_READY", ready_text.as_str()),
            (
                "FIRM_TEST_KIMI_PREPARED_CYCLE_RELEASE",
                release_text.as_str(),
            ),
        ]);
    } else if mode != 0 {
        env.extend([
            ("FAKE_KIMI_FIRST_PROMPT_RELEASE", release_text.as_str()),
            ("FAKE_KIMI_TERMINAL_ON_FIRST_RELEASE", "1"),
            ("FAKE_KIMI_TERMINAL_SENT_MARKER", terminal_text.as_str()),
        ]);
    }
    let run_id = create_run(&home, &fixture, "worker", &env);
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon_with_acceptance_timeout(
        &home,
        &fixture,
        &env,
        if mode == 0 { "1" } else { "30" },
    );
    wait_for_socket(&mut daemon, &socket);
    let start = run_firm_with_env(
        &home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &env,
    );
    success(&start, "start experiment");
    wait_for("provider received prompt", || ready.exists());
    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    let mut before = None;
    wait_for("exact prepared StartCycle", || {
        before = store
            .runtime_commands(&fixture.execution_space_id)
            .unwrap()
            .into_iter()
            .find(|c| {
                c.command == RuntimeCommandKind::StartCycle
                    && c.phase == RuntimeCommandPhase::Prepared
                    && c.effect_certainty == RuntimeEffectCertainty::Unknown
            });
        before.is_some()
    });
    let before = before.unwrap();
    if mode == 0 {
        wait_for("cancel received but deliberately unanswered", || {
            ignored_cancel.exists()
        });
    }
    if mode == 2 {
        std::fs::write(&release, b"release while still live").unwrap();
        wait_for("same terminal Applied before stop", || {
            store
                .runtime_commands(&fixture.execution_space_id)
                .unwrap()
                .into_iter()
                .any(|command| {
                    command.id == before.id
                        && command.effect_certainty == RuntimeEffectCertainty::Applied
                })
        });
    }
    let generation = store
        .latest_node_daemon_lease(&fixture.node_id)
        .unwrap()
        .unwrap()
        .generation;
    let stop = serde_json::json!({"cmd":"stop","execution_space_id":fixture.execution_space_id,
        "daemon_generation":generation})
    .to_string();
    let stderr_path = home.base().join("daemon-stderr.log");
    let response = std::thread::scope(|scope| {
        let stop_thread = scope.spawn(|| socket_request(&socket, &stop));
        if mode == 1 || mode == 3 {
            wait_for("Supervisor drain waiting", || {
                std::fs::read_to_string(&stderr_path)
                    .unwrap_or_default()
                    .contains("[node-daemon] waiting for")
            });
            std::fs::write(&release, b"release terminal after drain began").unwrap();
            if mode == 1 {
                wait_for("correlated terminal emitted", || terminal.exists());
            }
        }
        stop_thread.join().unwrap()
    });
    wait_for("daemon exit", || daemon.try_wait().unwrap().is_some());
    let after = store
        .runtime_commands(&fixture.execution_space_id)
        .unwrap()
        .into_iter()
        .find(|c| c.id == before.id)
        .unwrap();
    let evidence = serde_json::json!({"scenario":label,"run_id":run_id,"before":before,"after":after,
        "stop_response":response,"provider_terminal_emitted":terminal.exists(),
        "daemon_stderr":std::fs::read_to_string(&stderr_path).unwrap(),
        "prompt_received":prompt.exists(), "cancel_ignored":ignored_cancel.exists()});
    let evidence_path =
        std::env::temp_dir().join(format!("daemon-877-{label}-{}.json", std::process::id()));
    std::fs::write(
        &evidence_path,
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    eprintln!("EVIDENCE {}", evidence_path.display());
    assert_eq!(before.phase, RuntimeCommandPhase::Prepared);
    assert_eq!(before.effect_certainty, RuntimeEffectCertainty::Unknown);
    // An authentic input receipt can settle during local drain, while absent
    // provider evidence must remain Unknown rather than imply non-application.
    if mode == 1 || mode == 2 {
        assert_eq!(after.effect_certainty, RuntimeEffectCertainty::Applied);
        assert_eq!(after.phase, RuntimeCommandPhase::Settled);
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["authority_released"], true, "{response}");
    }
    if mode == 3 {
        assert!(
            !prompt.exists(),
            "the provider drive must never receive a prompt"
        );
        assert_eq!(after.effect_certainty, RuntimeEffectCertainty::NotApplied);
        assert_eq!(after.phase, RuntimeCommandPhase::Rejected);
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["authority_released"], true, "{response}");
    }
    if mode == 0 {
        assert!(
            ignored_cancel.exists(),
            "the no-receipt branch also exercised cancel"
        );
        assert_eq!(after.effect_certainty, RuntimeEffectCertainty::Unknown);
    }
    if after.effect_certainty == RuntimeEffectCertainty::Unknown {
        assert_eq!(response["ok"], false);
        assert_eq!(response["authority_released"], false);
    }
}
