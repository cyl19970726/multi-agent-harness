/// C5 (all five adapters): a correlation whose terminal names a different
/// provider input than the accepted one must fail correlation with
/// PROVIDER_CYCLE_TERMINAL_MISMATCH — for every adapter's correlation shape
/// (#709). Adapters construct matching ids on their own happy path, so each
/// case scripts the mismatch at the boundary the live runtime_adapter path
/// feeds into `correlate_provider_cycle`.
#[test]
fn runtime_adapter_cycle_terminal_mismatch_rejected_for_all_five_adapters() {
    fn authority() -> harness_application::ProviderCycleAuthority {
        harness_application::ProviderCycleAuthority {
            invocation_id: "runtime-command:1".into(),
            source_delivery_id: Some("work-delivery:1".into()),
            native_session_id: "native-session:1".into(),
            agent_session_generation: 2,
            provider_attempt: 3,
        }
    }
    fn native_cycle(
        provider_input_id: &str,
        terminal_provider_input_id: Option<&str>,
        exact_terminal_ref: &str,
    ) -> harness_runtime_contract::NativeCycleCorrelation {
        harness_runtime_contract::NativeCycleCorrelation {
            provider_input_id: provider_input_id.into(),
            input_acceptance_receipt: harness_runtime_contract::ControlTransportReceipt {
                command: "deliver".into(),
                response_id: Some(format!("{provider_input_id}:receipt")),
                success: true,
            },
            terminal_provider_input_id: terminal_provider_input_id.map(str::to_string),
            exact_terminal_ref: Some(exact_terminal_ref.into()),
        }
    }

    // (provider, accepted input id, mismatched terminal input id, the
    // adapter's exact_terminal_ref shape from its own correlation code)
    let cases = [
        (
            "claude",
            "claude-cycle-9",
            "claude-cycle-8",
            "claude_sdk.turn_complete:claude-cycle-9",
        ),
        (
            "codex",
            "turn-9",
            "turn-8",
            "codex.turn.completed:turn-9:completed",
        ),
        (
            "deepseek",
            "deepseek-cycle-9",
            "deepseek-cycle-8",
            "deepseek.turn_complete:deepseek-cycle-9",
        ),
        (
            "kimi",
            "kimi-cycle-9",
            "kimi-cycle-8",
            "kimi.session_prompt.terminal:kimi-cycle-9",
        ),
        (
            "pi",
            "pi-cycle-9",
            "pi-cycle-8",
            "pi.agent_settled:pi-cycle-9",
        ),
    ];
    for (provider, input_id, stale_terminal_id, terminal_ref) in cases {
        let error = match harness_application::correlate_provider_cycle(
            authority(),
            native_cycle(input_id, Some(stale_terminal_id), terminal_ref),
            true,
            None,
        ) {
            Ok(_) => panic!("{provider}: a mismatched terminal must not correlate"),
            Err(error) => error,
        };
        assert!(
            error.contains("PROVIDER_CYCLE_TERMINAL_MISMATCH"),
            "{provider}: expected PROVIDER_CYCLE_TERMINAL_MISMATCH, got {error}"
        );
        // Positive control: the same shape with matching ids correlates.
        harness_application::correlate_provider_cycle(
            authority(),
            native_cycle(input_id, Some(input_id), terminal_ref),
            true,
            None,
        )
        .unwrap_or_else(|error| panic!("{provider}: matching ids must correlate: {error}"));
    }
}
