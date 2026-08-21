use super::*;

    #[test]
    fn persistent_codex_schema_arg_matches_ephemeral_mapping() {
        let session_dir =
            std::env::temp_dir().join(format!("harness-codex-schema-{}", generated_id("test")));
        fs::create_dir_all(&session_dir).expect("create session dir");
        let mut spec = launch_spec_with_model_effort(None, None);
        spec.output_schema = Some(serde_json::json!({ "verdict": "pass/fail" }));
        let mut cmd = Command::new("codex");
        apply_codex_output_schema_arg(&mut cmd, &spec, &session_dir).expect("apply schema arg");

        let schema_path = session_dir.join("output-schema.json");
        assert_eq!(
            command_args(&cmd),
            vec![
                "--output-schema".to_string(),
                schema_path.to_string_lossy().to_string()
            ]
        );
        let written: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&schema_path).expect("schema file should be written"),
        )
        .expect("schema file should contain JSON");
        assert_eq!(
            written,
            schema_to_json_schema(spec.output_schema.as_ref().unwrap())
        );
        let _ = fs::remove_dir_all(&session_dir);
    }

