use super::*;

    #[test]
    fn host_rest_secret_and_mutation_shapes_fail_closed() {
        assert!(constant_time_secret_eq("host-secret-a", "host-secret-a"));
        assert!(!constant_time_secret_eq("host-secret-a", "host-secret-b"));
        assert!(!constant_time_secret_eq("short", "longer"));
        reject_unknown_json_fields(
            &serde_json::json!({"expected_revision": 1}),
            &["expected_revision"],
        )
        .expect("closed mutation shape");
        assert_eq!(
            reject_unknown_json_fields(
                &serde_json::json!({"expected_revision": 1, "actor_id": "browser"}),
                &["expected_revision"],
            )
            .expect_err("browser identity field fails closed")
            .code,
            FabricErrorCode::InvalidPayload
        );
        let artifact_path = "/v1/fabric/artifacts/artifact-a/complete";
        let trusted_origin = "https://company.example";
        let host_token = "host-secret-a";
        let mut headers = std::collections::BTreeMap::new();
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            STANDARD_FABRIC_HTTP_BODY_LIMIT,
            "unauthenticated local callers never receive the large allocation budget"
        );
        headers.insert("authorization".into(), "Bearer host-secret-a".into());
        headers.insert("origin".into(), trusted_origin.into());
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
        );
        headers.insert("origin".into(), "https://malicious.example".into());
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
    }

