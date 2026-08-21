use super::*;

    #[test]
    fn runtime_context_reports_proxy_routing_without_its_credentials() {
        // Corporate proxies routinely embed userinfo, and a gateway base URL
        // can carry a token in its path or query. Neither may reach the durable
        // ledger or a CI log.
        assert_eq!(
            redact_url_to_origin("http://alice:s3cret@corp-proxy:8080"),
            "http://corp-proxy:8080 (credentials redacted)"
        );
        assert_eq!(
            redact_url_to_origin("https://gateway.example.com/v1?token=abcd1234"),
            "https://gateway.example.com"
        );
        // A plain proxy is routing information and stays readable.
        assert_eq!(
            redact_url_to_origin("http://127.0.0.1:7897"),
            "http://127.0.0.1:7897"
        );
        // A NO_PROXY host list carries no credential and is left intact.
        assert_eq!(
            redact_url_to_origin("localhost,127.0.0.1,.local"),
            "localhost,127.0.0.1,.local"
        );
        // An `@` AFTER the authority belongs to the path/query, not to
        // userinfo. Searching the whole string for it mis-parsed the origin
        // and echoed part of the query back out.
        assert_eq!(
            redact_url_to_origin("https://gw.example.com/v1?user=alice@corp.com"),
            "https://gw.example.com"
        );
        assert_eq!(
            redact_url_to_origin("https://gw.example.com/tenants/a@b/keys/SECRET"),
            "https://gw.example.com"
        );
        // Userinfo AND a path secret together.
        assert_eq!(
            redact_url_to_origin("https://u:p@gw.example.com/v1/KEY123?t=z@1"),
            "https://gw.example.com (credentials redacted)"
        );
        // A base URL whose secret is the whole path keeps only the origin.
        assert_eq!(
            redact_url_to_origin("https://gw.example.com/sk-live-abcdef"),
            "https://gw.example.com"
        );
        for (raw, secrets) in [
            (
                "https://u:p@gw.example.com/v1/KEY123?t=z@1",
                ["KEY123", "u:p"].as_slice(),
            ),
            (
                "https://gw.example.com/tenants/a@b/keys/SECRET",
                ["SECRET"].as_slice(),
            ),
            (
                "https://gw.example.com/sk-live-abcdef",
                ["sk-live-abcdef"].as_slice(),
            ),
        ] {
            let redacted = redact_url_to_origin(raw);
            for secret in secrets {
                assert!(
                    !redacted.contains(secret),
                    "redaction leaked {secret} from {raw} as {redacted}"
                );
            }
        }
        for secret in ["s3cret", "abcd1234"] {
            for raw in [
                "http://alice:s3cret@corp-proxy:8080",
                "https://gateway.example.com/v1?token=abcd1234",
            ] {
                assert!(
                    !redact_url_to_origin(raw).contains(secret),
                    "redaction leaked {secret} from {raw}"
                );
            }
        }
    }

