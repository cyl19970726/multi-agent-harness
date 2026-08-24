use harness_runtime_contract::{ControlTransportReceipt, NativeCycleCorrelation};

pub(super) fn cycle_ref(
    input_id: &str,
    receipt: ControlTransportReceipt,
    terminal_kind: &str,
) -> NativeCycleCorrelation {
    NativeCycleCorrelation {
        provider_input_id: input_id.to_string(),
        input_acceptance_receipt: receipt,
        terminal_provider_input_id: Some(input_id.to_string()),
        exact_terminal_ref: Some(format!("claude_sdk.{terminal_kind}:{input_id}")),
    }
}
