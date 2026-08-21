use super::*;

    #[test]
    fn macos_keychain_credentials_require_public_identity_before_acl_access() {
        let args = vec![
            "--credential-backend".into(),
            "macos-keychain".into(),
            "--keychain-service".into(),
            "agentfirm.test.must-not-be-read".into(),
        ];
        let error = match resolve_node_credentials(&args, "company-test", "node-test") {
            Ok(_) => panic!("incomplete enrolled identity must fail before Keychain access"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("--certificate-serial"));
    }

