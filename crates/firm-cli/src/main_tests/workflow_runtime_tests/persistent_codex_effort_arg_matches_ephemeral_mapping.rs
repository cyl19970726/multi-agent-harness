use super::*;

#[test]
fn persistent_codex_effort_arg_matches_ephemeral_mapping() {
    let spec = launch_spec_with_model_effort(Some("o4-mini"), Some("high"));
    let mut cmd = Command::new("codex");
    apply_codex_model_and_effort_args(&mut cmd, &spec);

    assert_eq!(
        command_args(&cmd),
        vec!["-m", "o4-mini", "-c", "model_reasoning_effort=high"]
    );
}
