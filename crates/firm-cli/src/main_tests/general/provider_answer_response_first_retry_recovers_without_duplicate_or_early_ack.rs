use super::*;

    #[test]
    fn provider_answer_response_first_retry_recovers_without_duplicate_or_early_ack() {
        let response = TeamMessageProjection {
            id: "stable-response".into(),
            team_run_id: "run".into(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: Vec::new(),
            kind: ProviderDispatchIntent::ProviderInteractionResponse,
            body: "{}".into(),
            correlation_id: "request".into(),
            causation_id: Some("request".into()),
            response_intent: Some(ProviderResponseIntent::Informational),
            evidence_refs: Vec::new(),
            deliveries: Vec::new(),
            created_at: "unix-ms:1".into(),
        };
        let published = std::cell::Cell::new(0usize);
        let acknowledged = std::cell::Cell::new(0usize);
        let injected = publish_provider_answer_response_first(
            None,
            || {
                published.set(published.get() + 1);
                Ok(response.clone())
            },
            || Err(CliError::Usage("injected crash after publish".into())),
            || {
                acknowledged.set(acknowledged.get() + 1);
                Ok(())
            },
        )
        .expect_err("crash window must surface");
        assert!(injected.to_string().contains("injected crash"));
        assert_eq!(published.get(), 1);
        assert_eq!(acknowledged.get(), 0, "ACK cannot precede publish");

        let recovered = publish_provider_answer_response_first(
            Some(response.clone()),
            || panic!("exact retry must reuse the durable response"),
            || panic!("existing response does not cross the publish crash window"),
            || {
                acknowledged.set(acknowledged.get() + 1);
                Ok(())
            },
        )
        .expect("exact retry finishes ACK");
        assert_eq!(recovered.id, response.id);
        assert_eq!(published.get(), 1, "retry cannot publish a duplicate");
        assert_eq!(acknowledged.get(), 1);
    }

