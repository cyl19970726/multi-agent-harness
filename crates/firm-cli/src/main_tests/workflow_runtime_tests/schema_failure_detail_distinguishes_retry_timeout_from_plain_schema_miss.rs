use super::*;

    #[test]
    fn schema_failure_detail_distinguishes_retry_timeout_from_plain_schema_miss() {
        let required = vec!["ok".to_string(), "summary".to_string()];

        assert_eq!(
            schema_failure_detail(&required, false, false),
            "worker reply was not a JSON object with required keys [ok, summary]"
        );
        assert_eq!(
            schema_failure_detail(&required, true, false),
            "schema correction retry returned no valid JSON with required keys [ok, summary]"
        );
        assert_eq!(
            schema_failure_detail(&required, true, true),
            "schema correction retry timed out before producing valid JSON [ok, summary]"
        );
    }

