use super::*;

#[test]
fn delivery_outcome_defaults_have_no_telemetry_for_non_provider_paths() {
    let dry_run = delivery_outcome_for_test(None, None, None, None);
    let mut failure = delivery_outcome_for_test(None, None, None, None);
    failure.status = ProviderExecutionStatus::Failed;
    failure.exit_code = Some(1);

    for outcome in [dry_run, failure] {
        assert_eq!(outcome.tokens, None);
        assert_eq!(outcome.cost_usd, None);
        assert_eq!(outcome.model, None);
        assert_eq!(outcome.structured, None);
    }
}
