use super::*;

    #[test]
    fn cheatsheet_command_dispatches_by_scope() {
        for scope in ["team", "work", "mission", "all"] {
            cheatsheet_command(&[scope.to_string()])
                .unwrap_or_else(|error| panic!("cheatsheet {scope} should succeed: {error}"));
        }
        // Default scope (no argument) is "all".
        cheatsheet_command(&[]).expect("cheatsheet with no scope should default to all");
        let error = cheatsheet_command(&["bogus".to_string()])
            .expect_err("cheatsheet bogus should be rejected");
        assert!(matches!(error, CliError::Usage(_)));
    }
