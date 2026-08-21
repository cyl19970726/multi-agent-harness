use super::*;

#[test]
fn ephemeral_codex_service_tier_arg_is_a_config_override() {
    let mut cmd = Command::new("codex");
    apply_codex_ephemeral_model_effort_service_tier_args(
        &mut cmd,
        Some("gpt-5"),
        Some("high"),
        Some("priority"),
    );

    assert_eq!(
        command_args(&cmd),
        vec![
            "-m",
            "gpt-5",
            "-c",
            "model_reasoning_effort=high",
            "-c",
            "service_tier=priority",
        ]
    );
}
