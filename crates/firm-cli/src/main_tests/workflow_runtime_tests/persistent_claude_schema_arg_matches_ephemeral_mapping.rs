use super::*;

    #[test]
    fn persistent_claude_schema_arg_matches_ephemeral_mapping() {
        let mut spec = launch_spec_with_model_effort(None, None);
        spec.output_schema = Some(serde_json::json!({ "verdict": "pass/fail" }));
        let mut cmd = Command::new("claude");
        apply_claude_output_schema_arg(&mut cmd, &spec);

        assert_eq!(
            command_args(&cmd),
            vec![
                "--json-schema".to_string(),
                schema_to_json_schema(spec.output_schema.as_ref().unwrap()).to_string()
            ]
        );
    }

