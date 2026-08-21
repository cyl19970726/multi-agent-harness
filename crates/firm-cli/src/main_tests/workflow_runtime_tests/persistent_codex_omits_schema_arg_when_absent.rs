use super::*;

#[test]
fn persistent_codex_omits_schema_arg_when_absent() {
    let session_dir =
        std::env::temp_dir().join(format!("harness-codex-schema-{}", generated_id("test")));
    fs::create_dir_all(&session_dir).expect("create session dir");
    let spec = launch_spec_with_model_effort(None, None);
    let mut cmd = Command::new("codex");
    apply_codex_output_schema_arg(&mut cmd, &spec, &session_dir).expect("apply schema arg");

    assert!(command_args(&cmd).is_empty());
    assert!(
        !session_dir.join("output-schema.json").exists(),
        "no schema file should be written when schema is absent"
    );
    let _ = fs::remove_dir_all(&session_dir);
}
