use super::*;

/// ADR 0074. Losing the durable Supervisor lease used to leave a running
/// provider turn untouched: the drain revoked the heartbeat, waited for a
/// cooperative exit that a mid-turn provider cannot perform, and then SIGKILLed
/// the provider process group. This proves the turn now receives one
/// cooperative interrupt on the provider's own wire while it is still live, and
/// that the interrupt itself is not a RuntimeCommand — under lost authority no
/// provider effect may be admitted, and the interrupted terminal still settles
/// nothing.
#[test]
fn authority_loss_cooperatively_interrupts_a_live_provider_turn() {
    let home = TempHome::new("team-run-authority-loss-interrupt");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let cancel_marker = home.base().join("kimi-authority-loss-cancel.log");
    let cancel_marker_value = cancel_marker.display().to_string();
    let prompt_marker = home.base().join("kimi-authority-loss-prompt.log");
    let prompt_marker_value = prompt_marker.display().to_string();
    let mut serve_env = vec![
        ("KIMI_CODE_BIN", fake_kimi.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        // The provider takes the prompt and then never answers it, so the turn
        // is unambiguously live when authority is lost.
        ("FAKE_KIMI_WAIT", "1"),
        ("FAKE_KIMI_PROMPT_MARKER", prompt_marker_value.as_str()),
        ("FAKE_KIMI_CANCEL_MARKER", cancel_marker_value.as_str()),
        ("FIRM_TEAM_SUPERVISOR_LEASE_MS", "10000"),
        ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);

    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Interrupt a live turn when authority is lost",
            "members": [{
                "name": "kimi-authority-loss",
                "role": "runtime_reliability",
                "provider": "kimi",
                "model": "k2.5",
                "initial_work": "Hold a live provider turn"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    // The prompt reaching the provider is the exact boundary after which a
    // turn is live and polling control; `running` alone can precede it.
    wait_for_file(&prompt_marker, "Kimi prompt reached the provider");
    assert!(
        !cancel_marker.exists(),
        "no interrupt may be issued while the Supervisor still holds its lease"
    );

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let member_commands = |store: &HarnessStore| {
        store
            .runtime_commands(&current_space_id(&home))
            .expect("canonical RuntimeCommands")
            .into_iter()
            .filter(|command| {
                command.binding.target_member_run_id.as_deref() == Some(member_id.as_str())
            })
            .collect::<Vec<_>>()
    };
    let semantic_before = member_semantic_row_counts(&store, &member_id);

    // Take the durable Supervisor lease away from the generation driving the
    // live turn. Its heartbeat observes the terminal renewal failure and
    // latches the loss, exactly as a real lease takeover does.
    replace_supervisor_lease(&store, &run_id);

    wait_for_file(
        &cancel_marker,
        "Kimi cooperative interrupt on authority loss",
    );
    let cancel_frame = std::fs::read_to_string(&cancel_marker).expect("cancel notification");
    assert!(
        cancel_frame.contains(r#""method":"session/cancel""#),
        "the loss path must use the provider's own interrupt primitive: {cancel_frame}"
    );
    assert!(
        !cancel_frame.contains(r#""id":"#),
        "ACP session/cancel stays a notification: {cancel_frame}"
    );
    assert_eq!(
        cancel_frame.lines().count(),
        1,
        "exactly one cooperative interrupt reaches the provider: {cancel_frame}"
    );

    // The interrupted terminal must still hit the ordinary authority refusal.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        member_semantic_row_counts(&store, &member_id),
        semantic_before,
        "an interrupted turn settled member/action/Handoff state under lost authority"
    );
    let commands = member_commands(&store);
    // The interrupt is a process-local action, so it leaves no durable command
    // of its own. Under lost authority none could be admitted anyway.
    assert!(
        commands.iter().all(|command| !matches!(
            command.command,
            harness_core::agentfirm_api::RuntimeCommandKind::CancelProviderTurn
                | harness_core::agentfirm_api::RuntimeCommandKind::InterruptCurrentCycle
                | harness_core::agentfirm_api::RuntimeCommandKind::StopSession
                | harness_core::agentfirm_api::RuntimeCommandKind::CloseMember
        )),
        "the cooperative interrupt must not create a RuntimeCommand: {commands:?}"
    );
    let cycles = commands
        .iter()
        .filter(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cycles.len(),
        1,
        "no provider replay is permitted: {cycles:?}"
    );
    // The interrupted terminal hits the authority refusal before any
    // settlement, so the admitted StartCycle keeps its unproven certainty and
    // becomes explicit recovery work rather than a satisfied effect.
    assert_eq!(
        cycles[0].effect_certainty,
        harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown
    );
    assert_eq!(
        cycles[0].postcondition_status,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown
    );
    assert_ne!(
        cycles[0].phase,
        harness_core::agentfirm_api::RuntimeCommandPhase::Settled,
        "an interrupted turn must not settle its provider effect under lost authority"
    );
}
