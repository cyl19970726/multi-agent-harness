use super::*;

#[test]
fn kimi_acp_v1_discriminator_requires_canonical_intent_and_exact_wire_shape() {
    let option = |id: &str, intent: &str| ProviderInteractionMessageOption {
        id: id.into(),
        label: "Hostile display label".into(),
        intent: Some(intent.into()),
    };

    assert_eq!(
        classify_kimi_acp_v1_interaction(
            "AskUserQuestion",
            &[
                option("q0_opt_0", "allow_once"),
                option("q0_skip", "reject_once"),
            ],
        ),
        ProviderInteractionType::Question
    );
    assert_eq!(
        classify_kimi_acp_v1_interaction(
            "ExitPlanMode",
            &[
                option("plan_approve", "allow_once"),
                option("plan_revise", "reject_once"),
                option("plan_reject_and_exit", "reject_once"),
            ],
        ),
        ProviderInteractionType::PlanReview
    );
    for (title, options, expected) in [
        (
            "AskUserQuestion",
            vec![option("q0_opt_0", "reject_once")],
            ProviderInteractionType::RejectOnly,
        ),
        (
            "ExitPlanMode",
            vec![option("plan_approve", "reject_always")],
            ProviderInteractionType::RejectOnly,
        ),
        (
            "AskUserQuestion",
            vec![option("q0_opt_0", "future_allow")],
            ProviderInteractionType::Unknown,
        ),
        (
            "AskUserQuestion",
            vec![option("q0_opt_0", "allow_once")],
            ProviderInteractionType::Unknown,
        ),
        (
            "ExitPlanMode",
            vec![
                option("plan_approve", "allow_once"),
                option("plan_revise", "reject_once"),
            ],
            ProviderInteractionType::Unknown,
        ),
        (
            "Bash",
            vec![option("plan_approve", "allow_once")],
            ProviderInteractionType::Unknown,
        ),
        (
            "Bash",
            vec![
                option("approve_once", "allow_once"),
                option("reject", "reject_once"),
            ],
            ProviderInteractionType::ToolApproval,
        ),
    ] {
        assert_eq!(classify_kimi_acp_v1_interaction(title, &options), expected);
    }
}
